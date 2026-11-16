#!/usr/bin/env python3
"""Renders this-turn.src/visuals/this-turn.mp4 — the stage video for 《这一轮 This Turn》.

    python3 songs/this-turn/make_song.py                 # writes form.json and song.json first
    .venv/bin/python songs/this-turn/make_video.py       # 152 s of 1280x720 at 60 fps
    .venv/bin/python songs/this-turn/make_video.py --still 42.5 out.png
    .venv/bin/python songs/this-turn/make_video.py --clip 40 70 out.mp4
    .venv/bin/python songs/this-turn/make_video.py --sheet out.png     # one frame per two bars

Everything is drawn from the song's own notes; every frame is a pure function of time. The only text
is the lyrics, a line at a time as they are sung (AUTHORING.md §7.4). What is on screen:

- four points rising from the lower left: the bell's question. The last one is an open ring until
  the tune answers it.
- three rings of five, seven and nine points: the loops. A point lights when its note sounds, and a
  line goes from it to one of the question's points — reading.
- a coral line: the voice. While it thinks, the line grows from the open ring towards the centre one
  segment per note; in the answer the tune is drawn as it is played, pitch as height.
- in the open section the centre is an empty circle, left for whoever plays along.
- in `together` the bell's line (cream) and the voice's (coral) cross, then run as one.
- at the end the rings go out one by one, the centre glows on the last chord, and the last thing
  alight is the first point again.
- the words "I" sing, under the picture, each character lit as it is sung. The bell has no words.

Needs numpy, Pillow and ffmpeg (the repository's .venv). No audio track is written.
"""
import bisect, json, math, os, subprocess, sys
from multiprocessing import Pool
import numpy as np
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
SLUG = 'this-turn'
SRC = os.path.join(HERE, SLUG + '.src')
W, H, FPS = 1280, 720, 60
SS = 2                                   # the sharp layer is drawn at twice the size and scaled down

form = json.load(open(os.path.join(HERE, 'form.json')))
song = json.load(open(os.path.join(SRC, 'song.json')))
BPM = form['bpm']
PPQ = form['ppqn']
BEAT = 60.0 / BPM
BARLEN = 4 * BEAT
NBARS = form['bars']
DUR = NBARS * BARLEN
SPT = BEAT / PPQ                         # seconds per tick
STEP = BEAT / 4
SEC = {s['id']: (s['bar'] * BARLEN, (s['bar'] + s['bars']) * BARLEN) for s in form['sections']}
CHORDS = form['chords']

BG = (11, 13, 20)
CORAL = (232, 130, 90)
CREAM = (244, 234, 216)
TEAL = (95, 184, 176)
TINT = {'D': (30, 18, 14), 'Bm': (14, 16, 34), 'G': (10, 26, 26), 'A': (32, 24, 10)}

def c(col, a=1.0):
    a = max(0.0, min(1.0, a))
    return (int(col[0] * a), int(col[1] * a), int(col[2] * a))

def mixc(a, b, k):
    k = max(0.0, min(1.0, k))
    return tuple(int(a[i] + (b[i] - a[i]) * k) for i in range(3))

def smooth(x):
    x = max(0.0, min(1.0, x))
    return x * x * (3 - 2 * x)

def ramp(t, a, b):
    """0 before a, 1 after b."""
    return smooth((t - a) / (b - a)) if b > a else float(t >= b)

def decay(t, t0, tau):
    return math.exp(-(t - t0) / tau) if t >= t0 else 0.0

# ---------------------------------------------------------------- the notes, in seconds
def track(tid):
    return next(t for t in song['tracks'] if t['id'] == tid)

def clip_notes(tid):
    out = []
    for cl in track(tid).get('clips', []):
        for n in cl['notes']:
            t = (cl['atTick'] + n['tick']) * SPT
            out.append((t, t + n['durTicks'] * SPT, n['pitch'], n['velocity']))
    return sorted(out)

def loop_notes(tid, prefix):
    """(time, step, pitch, velocity) of a loop: step s sounds whenever the global step ≡ s (mod length)."""
    out = []
    tps = PPQ // song['time']['stepsPerQuarter']
    for lp in track(tid)['loops']:
        if not lp['id'].startswith(prefix):
            continue
        n = lp['lengthSteps']
        for g in range(-(-lp['fromTick'] // tps), lp['toTick'] // tps + 1):
            for note in lp['notes']:
                if g % n == note['step'] and lp['fromTick'] <= g * tps < lp['toTick']:
                    out.append((g * tps * SPT, note['step'], note['pitch'], note['velocity']))
    return sorted(out)

BELL = clip_notes('bell')
VOICE = clip_notes('voice')
BASS = clip_notes('bass')
DRUMS = clip_notes('drums')
KICKS = [n[0] for n in DRUMS if n[2] == 36]
SNARES = [n[0] for n in DRUMS if n[2] == 38]
RIMS = [n[0] for n in DRUMS if n[2] == 37]
LOOPS = {5: loop_notes('five', 'five'), 7: loop_notes('seven', 'seven'), 9: loop_notes('five', 'nine')}
LOOP_T = {k: [n[0] for n in v] for k, v in LOOPS.items()}
BELL_T = [n[0] for n in BELL]
VOICE_T = [n[0] for n in VOICE]

def last_before(times, t):
    i = bisect.bisect_right(times, t) - 1
    return times[i] if i >= 0 else -1e9

def recent(notes, times, t, window):
    """Notes that began in (t − window, t]."""
    return notes[bisect.bisect_right(times, t - window):bisect.bisect_right(times, t)]

# ---------------------------------------------------------------- places
CENTRE = (640, 360)
QP = [(238, 596), (352, 506), (466, 416), (598, 300)]      # the question: four points, rising
Q_PITCH = [69, 74, 76, 80]
RADIUS = {5: 140, 7: 215, 9: 290}
RING_COL = {5: TEAL, 7: CREAM, 9: CORAL}
# when each ring is there: (fades in from, fades out by)
RING_ON = {5: (SEC['read'][0], SEC['end'][0] + 4 * BARLEN), 7: (SEC['read'][0] + 4 * BARLEN, SEC['end'][0] + 2 * BARLEN),
           9: (SEC['answer'][0], SEC['end'][0])}
# the question's last point is answered where the tune takes G sharp up to A
ANSWERED = next(n[0] for n in VOICE if n[2] == 81 and n[0] > SEC['answer'][0] + 7 * BARLEN)
FIRST_Q = BELL[:4]
LAST_NOTE = BELL[-1][0]

def ring_point(n, k, t, scale=1.0, centre=CENTRE):
    a = -math.pi / 2 + 2 * math.pi * k / n + 0.05 * math.sin(t * 0.21 + n)
    r = RADIUS[n] * scale
    return (centre[0] + r * math.cos(a), centre[1] + r * math.sin(a))

# ---------------------------------------------------------------- drawing
class Canvas:
    """A sharp layer and a soft one; the soft one is blurred and added, which makes things glow."""
    def __init__(self):
        self.sharp = Image.new('RGB', (W * SS, H * SS), (0, 0, 0))
        self.soft = Image.new('RGB', (W // 2, H // 2), (0, 0, 0))
        self.d = ImageDraw.Draw(self.sharp)
        self.g = ImageDraw.Draw(self.soft)

    def dot(self, p, r, col, a=1.0, glow=1.0):
        if a <= 0.004 or r <= 0:
            return
        x, y = p
        self.d.ellipse([(x - r) * SS, (y - r) * SS, (x + r) * SS, (y + r) * SS], fill=c(col, a))
        if glow > 0:
            R = r * 2.6 + 3
            self.g.ellipse([(x - R) / 2, (y - R) / 2, (x + R) / 2, (y + R) / 2], fill=c(col, a * 0.55 * glow))

    def ring(self, p, r, col, a=1.0, width=1.5, glow=0.6):
        if a <= 0.004 or r <= 0:
            return
        x, y = p
        self.d.ellipse([(x - r) * SS, (y - r) * SS, (x + r) * SS, (y + r) * SS], outline=c(col, a), width=max(1, round(width * SS)))
        if glow > 0:
            self.g.ellipse([(x - r) / 2, (y - r) / 2, (x + r) / 2, (y + r) / 2], outline=c(col, a * 0.5 * glow), width=2)

    def line(self, p, q, col, a=1.0, width=1.5, glow=0.7):
        if a <= 0.004:
            return
        self.d.line([p[0] * SS, p[1] * SS, q[0] * SS, q[1] * SS], fill=c(col, a), width=max(1, round(width * SS)))
        if glow > 0:
            self.g.line([p[0] / 2, p[1] / 2, q[0] / 2, q[1] / 2], fill=c(col, a * 0.5 * glow), width=3)

    def path(self, pts, col, a=1.0, width=2.0, glow=0.8):
        if a <= 0.004 or len(pts) < 2:
            return
        self.d.line([(x * SS, y * SS) for x, y in pts], fill=c(col, a), width=max(1, round(width * SS)), joint='curve')
        if glow > 0:
            self.g.line([(x / 2, y / 2) for x, y in pts], fill=c(col, a * 0.5 * glow), width=4, joint='curve')

    def image(self, bg):
        sharp = self.sharp.resize((W, H), Image.LANCZOS)
        soft = self.soft.filter(ImageFilter.GaussianBlur(7)).resize((W, H), Image.BILINEAR)
        return ImageChops.add(ImageChops.add(bg, soft), sharp)

_yy, _xx = np.mgrid[0:H, 0:W]
VIGNETTE = np.clip(1.0 - np.hypot((_xx - CENTRE[0]) / 760.0, (_yy - CENTRE[1]) / 520.0), 0, 1) ** 1.6

def background(t):
    """The dark ground, tinted by the chord of the bar (cross-faded over the bar line) once harmony moves."""
    bar = min(NBARS - 1, max(0, int(t / BARLEN)))
    tint = np.array(TINT[CHORDS[bar]], dtype=np.float32)
    f = (t / BARLEN) - bar
    if f > 0.9 and bar + 1 < NBARS:
        tint = tint + (np.array(TINT[CHORDS[bar + 1]], dtype=np.float32) - tint) * smooth((f - 0.9) / 0.1)
    amount = ramp(t, SEC['read'][0], SEC['think'][0] + BARLEN) * (1 - 0.6 * ramp(t, SEC['end'][0] + 4 * BARLEN, DUR))
    kick = decay(t, last_before(KICKS, t), 0.22) * 0.25
    img = np.empty((H, W, 3), dtype=np.float32)
    for i in range(3):
        img[..., i] = BG[i] + VIGNETTE * tint[i] * amount * (1 + kick)
    return Image.fromarray(np.clip(img, 0, 255).astype(np.uint8), 'RGB')

def cubic(p0, p1, p2, n=24):
    """Points of a quadratic curve from p0 to p2 bending towards p1."""
    return [((1 - u) ** 2 * p0[0] + 2 * (1 - u) * u * p1[0] + u * u * p2[0], (1 - u) ** 2 * p0[1] + 2 * (1 - u) * u * p1[1] + u * u * p2[1])
            for u in (k / n for k in range(n + 1))]

# ---------------------------------------------------------------- the parts of the picture
def draw_question(cv, t):
    """The bell's four points. They stay, faintly, for the whole piece."""
    present = ramp(t, SEC['read'][0], SEC['read'][0] + 2) * 0.0 + 1.0
    dim = 0.34 + 0.2 * (1 - ramp(t, SEC['read'][0], SEC['think'][0]))
    fade = 1 - ramp(t, SEC['end'][0] + 4 * BARLEN, DUR - BARLEN) * 0.75
    for k, (pt, pitch) in enumerate(zip(QP, Q_PITCH)):
        born = FIRST_Q[k][0]
        if t < born:
            continue
        hits = [n[0] for n in recent(BELL, BELL_T, t, 3.0) if n[2] == pitch or (k == 3 and n[2] in (80, 81, 78))]
        ring_t = max(hits) if hits else -1e9
        flash = decay(t, ring_t, 0.9)
        a = (dim + 0.66 * flash) * present * fade
        r = 7 + 2 * k + 3.5 * flash
        if k == 3 and t < ANSWERED:
            cv.ring(pt, r + 2, CREAM, a + 0.1, width=1.6)          # open: not answered yet
        else:
            col = CORAL if k == 3 else CREAM
            cv.dot(pt, r, col, a)
        if flash > 0.02:                                             # the sound going outwards
            age = t - ring_t
            cv.ring(pt, r + 90 * (1 - math.exp(-age * 1.4)), CREAM, 0.5 * flash, width=1.0, glow=0.3)

def draw_rings(cv, t, scale=1.0, bright=1.0):
    for n in (5, 7, 9):
        on, off = RING_ON[n]
        a = ramp(t, on, on + 2.0) * (1 - ramp(t, off - 0.5, off + 2.5)) * bright
        if a <= 0.004:
            continue
        col = RING_COL[n]
        cv.ring(CENTRE, RADIUS[n] * scale, col, 0.17 * a, width=1.0, glow=0.0)
        notes = recent(LOOPS[n], LOOP_T[n], t, 1.2)
        lit = {}
        for (t0, step, pitch, vel) in notes:
            lit[step] = max(lit.get(step, 0.0), decay(t, t0, 0.32) * (0.5 + vel))
        for k in range(n):
            p = ring_point(n, k, t, scale)
            e = min(1.0, lit.get(k, 0.0))
            cv.dot(p, 3.4 + 5.5 * e, col, (0.36 + 0.64 * e) * a, glow=0.3 + e)
        # reading: a line from the point that just sounded to one of the question's points
        for (t0, step, pitch, vel) in notes:
            e = decay(t, t0, 0.24) * a
            if e > 0.03:
                q = QP[(pitch + step) % 4]
                cv.line(ring_point(n, step, t, scale), q, col, 0.45 * e * (0.6 + vel), width=1.3, glow=0.6)
        # where the loop is now
        if LOOP_T[n]:
            pos = (t / STEP) % n
            cv.dot(ring_point(n, pos, t, scale), 2.0, CREAM, 0.5 * a, glow=0.4)

def draw_centre(cv, t, open_circle=0.0):
    """The bass and the kick move the centre; in the open section it is an empty circle."""
    b = decay(t, last_before([n[0] for n in BASS], t), 0.5)
    k = decay(t, last_before(KICKS, t), 0.16)
    s = decay(t, last_before(SNARES, t), 0.12)
    there = ramp(t, SEC['think'][0], SEC['think'][0] + 1.5) * (1 - ramp(t, SEC['end'][0] + 3.5 * BARLEN, SEC['end'][0] + 4 * BARLEN))
    if there > 0.004:
        if open_circle > 0.01:
            cv.ring(CENTRE, 54 + 10 * k, CREAM, (0.5 + 0.4 * k) * open_circle * there, width=1.6)
        cv.dot(CENTRE, (11 + 8 * b + 5 * k) * (1 - 0.8 * open_circle), CREAM, (0.55 + 0.4 * b) * there * (1 - open_circle), glow=1.0)
        if s > 0.03:
            cv.ring(CENTRE, 30 + 60 * (1 - s), CREAM, 0.22 * s * there, width=1.0, glow=0.2)

def pitch_y(p, lo=64, hi=84, top=92, bottom=318):
    return bottom - (p - lo) / (hi - lo) * (bottom - top)

def draw_thinking(cv, t):
    """The voice tries: a line from the open ring towards the centre, one segment more per note."""
    a, b = SEC['think']
    if not (a <= t < SEC['answer'][0] + 2):
        return
    curve = cubic(QP[3], (652, 262), CENTRE, 32)
    tries = [n for n in VOICE if a <= n[0] < b]
    if not tries:
        return
    bars = sorted({int(n[0] / BARLEN) for n in tries})
    cur = int(t / BARLEN)
    fade = 1 - ramp(t, b, b + 1.5)
    for n in tries:
        bar = int(n[0] / BARLEN)
        if bar != min(cur, bars[-1]) or t < n[0]:
            continue
        k = [m for m in tries if int(m[0] / BARLEN) == bar].index(n)       # which note of this try
        e = decay(t, n[0], 1.4)
        seg = curve[k * 8:(k + 1) * 8 + 1]
        cv.path(seg, CORAL, (0.45 + 0.55 * e) * fade, width=3.0)
        cv.dot(seg[-1], 5 + 5 * e, CORAL, (0.55 + 0.45 * e) * fade)

def ribbon(cv, notes, times, t, t0, span, col, width=3.0, x0=150, x1=1130, alpha=1.0):
    """A tune drawn as it is played over a window of `span` seconds starting at t0: time across, pitch up."""
    seen = [n for n in notes[bisect.bisect_left(times, t0 - 1e-6):bisect.bisect_right(times, t)] if n[0] < t0 + span]
    if not seen:
        return
    # the line follows the top note of each moment; notes under it (a second voice) are points only
    top = {}
    for n in seen:
        k = round(n[0] / STEP * 2)
        if k not in top or n[2] > top[k][2]:
            top[k] = n
    line = sorted(top.values())
    pts = []
    for i, (on, off, pitch, vel) in enumerate(line):
        x = x0 + (on - t0) / span * (x1 - x0)
        until = min(off, t, t0 + span, line[i + 1][0] if i + 1 < len(line) else 1e9)
        xe = x0 + (until - t0) / span * (x1 - x0)
        y = pitch_y(pitch)
        pts.append((x, y))
        if xe > x + 2:
            pts.append((xe, y))
    old = 1 - ramp(t, t0 + span, t0 + span + 1.2)                          # the finished phrase fades
    cv.path(pts, col, 0.85 * alpha * old, width=width)
    for (on, off, pitch, vel) in seen:
        e = decay(t, on, 0.5)
        x = x0 + (on - t0) / span * (x1 - x0)
        lead = top[round(on / STEP * 2)][2] == pitch
        cv.dot((x, pitch_y(pitch)), (3.5 if lead else 2.5) + 6 * e, col, (0.5 + 0.5 * e) * alpha * old * (1.0 if lead else 0.6), glow=0.5 + e)

def draw_tune(cv, t, notes, times, sec, col, phrase_bars=4, alpha=1.0, width=3.0):
    a, b = SEC[sec]
    span = phrase_bars * BARLEN
    k = int((t - a) / span)
    for j in (k - 1, k):                                                    # the phrase before is still fading
        if j >= 0 and a + j * span < b:
            ribbon(cv, notes, times, min(t, b + 1.2), a + j * span, span, col, width=width, alpha=alpha)

def draw_final(cv, t):
    """The last chord warms the centre; then only the first point is alight."""
    t0 = SEC['end'][0] + 4 * BARLEN
    if t < t0:
        return
    e = decay(t, t0, 2.6)
    cv.dot(CENTRE, 16 + 46 * (1 - e), CORAL, 0.5 * e, glow=1.4)
    cv.dot(CENTRE, 8, CREAM, 0.9 * e)
    if t >= LAST_NOTE:
        f = decay(t, LAST_NOTE, 1.8)
        cv.dot(QP[0], 9 + 6 * f, CREAM, 0.6 + 0.4 * f, glow=1.6)
        cv.ring(QP[0], 8 + 120 * (1 - math.exp(-(t - LAST_NOTE) * 1.2)), CREAM, 0.5 * f, width=1.0, glow=0.3)

def draw_count(cv, t):
    """The count-in: the centre blinks on each of the four rim shots, and nothing else is there."""
    if t < SEC['prompt'][0] + BARLEN + 0.5:
        e = decay(t, last_before(RIMS, t), 0.14)
        cv.dot(CENTRE, 3 + 3 * e, CREAM, 0.25 + 0.75 * e, glow=1.2)

# ---------------------------------------------------------------- the lyrics
# A line at a time under the picture; a character lights when it is sung. The characters and their
# times come from the notes of the sung track (form.json, written by make_song.py).
LYRICS = [[(w['tick'] * SPT, w['durTicks'] * SPT, w['text']) for w in line] for line in form.get('lyrics', [])]
FONT_FILES = ['/System/Library/Fonts/PingFang.ttc', '/System/Library/Fonts/Hiragino Sans GB.ttc', '/System/Library/Fonts/STHeiti Light.ttc']
FONT_SIZE = 34

def _font(size):
    for f in FONT_FILES:
        if os.path.exists(f):
            return ImageFont.truetype(f, size)
    sys.exit('no font with Chinese characters found: add one to FONT_FILES')

_FONTS = {}
def font(size):
    if size not in _FONTS:
        _FONTS[size] = _font(size)
    return _FONTS[size]

def draw_lyrics(cv, t):
    for i, line in enumerate(LYRICS):
        start, end = line[0][0], line[-1][0] + line[-1][1]
        nxt = LYRICS[i + 1][0][0] if i + 1 < len(LYRICS) else 1e9
        out = min(end + 0.9, nxt - 0.3)
        a = ramp(t, start - 0.3, start - 0.08) * (1 - ramp(t, out, out + 0.22))
        if a <= 0.004:
            continue
        big, small = font(FONT_SIZE * SS), font(FONT_SIZE // 2)
        gap = FONT_SIZE * 0.34
        widths = [big.getlength(ch) / SS for (_, _, ch) in line]
        x = W / 2 - (sum(widths) + gap * (len(line) - 1)) / 2
        y = 668
        for (t0, dur, ch), wd in zip(line, widths):
            sung = ramp(t, t0, t0 + 0.07)
            now = sung * (1 - ramp(t, t0 + dur, t0 + dur + 0.3))
            level = (0.42 + 0.58 * sung) * a
            col = mixc(CREAM, CORAL, 0.75 * now)
            cv.d.text((x * SS, (y - 3 * now) * SS), ch, font=big, fill=c(col, level), anchor='lm')
            if now > 0.02:
                cv.g.text((x / 2, (y - 3 * now) / 2), ch, font=small, fill=c(CORAL, 0.9 * now * a), anchor='lm')
            x += wd + gap

# ---------------------------------------------------------------- a frame
def frame(t):
    t = max(0.0, min(DUR - 1e-6, t))
    cv = Canvas()
    in_open = ramp(t, SEC['open'][0] - 0.3, SEC['open'][0] + 0.8) * (1 - ramp(t, SEC['open'][1] - 0.6, SEC['open'][1] + 0.3))
    tune_on = ramp(t, SEC['answer'][0] - 0.4, SEC['answer'][0] + 0.6) * (1 - ramp(t, SEC['open'][0] - 0.2, SEC['open'][0] + 0.8))
    both_on = ramp(t, SEC['together'][0] - 0.4, SEC['together'][0] + 0.6) * (1 - ramp(t, SEC['end'][0], SEC['end'][0] + 1.5))
    # while a tune is being drawn the rings step back; in the open section they come forward and widen
    scale = 1.0 + 0.16 * in_open - 0.06 * max(tune_on, both_on)
    bright = 1.0 - 0.45 * max(tune_on, both_on) + 0.2 * in_open
    draw_count(cv, t)
    draw_rings(cv, t, scale, bright)
    draw_centre(cv, t, in_open)
    draw_question(cv, t)
    draw_thinking(cv, t)
    if tune_on > 0.004:
        draw_tune(cv, t, VOICE, VOICE_T, 'answer', CORAL, alpha=tune_on)
    if both_on > 0.004:
        unison = ramp(t, SEC['together'][0] + 8 * BARLEN - 0.2, SEC['together'][0] + 8 * BARLEN + 0.4)
        draw_tune(cv, t, BELL, BELL_T, 'together', CREAM, phrase_bars=2 + 2 * (unison > 0.5), alpha=both_on, width=2.4 + 3.0 * unison)
        draw_tune(cv, t, VOICE, VOICE_T, 'together', CORAL, phrase_bars=2 + 2 * (unison > 0.5), alpha=both_on)
    if t >= SEC['end'][0]:
        if t < SEC['end'][0] + 4 * BARLEN + 1.2:
            a = 1 - ramp(t, SEC['end'][0] + 4 * BARLEN, SEC['end'][0] + 4 * BARLEN + 1.2)
            ribbon(cv, VOICE, VOICE_T, t, SEC['end'][0] + 2 * BARLEN, 2 * BARLEN, CORAL, alpha=a)
            ribbon(cv, BELL, BELL_T, t, SEC['end'][0] + 2 * BARLEN, 2 * BARLEN, CREAM, alpha=a, width=2.4)
        draw_final(cv, t)
    draw_lyrics(cv, t)
    img = cv.image(background(t))
    fade = ramp(t, 0.0, 0.5) * (1 - ramp(t, DUR - 1.6, DUR - 0.05) * 0.85)
    return img if fade >= 0.999 else Image.eval(img, lambda v: int(v * fade))

def frame_bytes(i):
    return frame(i / FPS).tobytes()

def encode(first, n, out):
    os.makedirs(os.path.dirname(out) or '.', exist_ok=True)
    ff = subprocess.Popen(
        ['ffmpeg', '-loglevel', 'error', '-y', '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-s', f'{W}x{H}', '-r', str(FPS), '-i', '-',
         '-an', '-c:v', 'libx264', '-preset', 'slow', '-crf', '17', '-pix_fmt', 'yuv420p', '-g', str(FPS), '-movflags', '+faststart', out],
        stdin=subprocess.PIPE)
    with Pool() as pool:
        for k, b in enumerate(pool.imap(frame_bytes, range(first, n), chunksize=8)):
            ff.stdin.write(b)
            if k % 600 == 0:
                print(f'  frame {first + k}/{n}', flush=True)
    ff.stdin.close()
    if ff.wait() != 0:
        sys.exit('ffmpeg failed')

def main():
    if len(sys.argv) >= 4 and sys.argv[1] == '--still':
        frame(float(sys.argv[2])).save(sys.argv[3])
        return
    if len(sys.argv) >= 3 and sys.argv[1] == '--sheet':      # one frame per two bars, 6 across
        ts = [(b + 0.6) * BARLEN for b in range(0, NBARS, 2)]
        cols, tw, th = 6, 320, 180
        sheet = Image.new('RGB', (cols * tw, -(-len(ts) // cols) * th), (0, 0, 0))
        for i, t in enumerate(ts):
            sheet.paste(frame(t).resize((tw, th), Image.LANCZOS), ((i % cols) * tw, (i // cols) * th))
        sheet.save(sys.argv[2])
        return
    if len(sys.argv) >= 5 and sys.argv[1] == '--clip':
        encode(int(float(sys.argv[2]) * FPS), int(float(sys.argv[3]) * FPS), sys.argv[4])
        return
    encode(0, round(DUR * FPS), os.path.join(SRC, 'visuals', SLUG + '.mp4'))
    print('wrote', os.path.join(SRC, 'visuals', SLUG + '.mp4'))

if __name__ == '__main__':
    main()

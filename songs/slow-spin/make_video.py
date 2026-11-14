#!/usr/bin/env python3
"""Renders slow-spin.src/visuals/slow-spin.mp4 — the stage video for 《午夜慢转 Slow Spin》.

    python3 songs/slow-spin/make_song.py      # writes form.json and song.json first
    python3 songs/slow-spin/make_video.py     # ~190 s of 1280x720 at 30 fps
    python3 songs/slow-spin/make_video.py --still 42.5 out.png   # one frame, for checking
    python3 songs/slow-spin/make_video.py --clip 40 70 out.mp4   # a stretch, for checking motion

The picture is a pure function of time: every frame is drawn from the song's own notes
(song.json) and form (form.json), so it stays on the bar when the file is regenerated.
Needs Pillow and ffmpeg. No audio track is written (FORMAT.md §12.1).
"""
import bisect, json, math, os, subprocess, sys
from multiprocessing import Pool
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, 'slow-spin.src')
W, H, FPS = 1280, 720, 30
SCANLINES = False      # they look good but triple the file size

form = json.load(open(os.path.join(HERE, 'form.json')))
song = json.load(open(os.path.join(SRC, 'song.json')))
BPM = form['bpm']
BEAT = 60.0 / BPM
BARLEN = 4 * BEAT
NBARS = form['bars']
DUR = NBARS * BARLEN
SEC_PER_TICK = BEAT / song['time']['ppqn']
SECTIONS = form['sections']           # [id, name, startBar, bars]
CHORDS = form['chords']               # per bar: [[beat, name], ...]
LAST = form['lastBar']

BG = (14, 12, 28)
ROSE = (255, 107, 129)
TEAL = (86, 214, 196)
AMBER = (255, 176, 72)
INDIGO = (104, 90, 200)
CREAM = (255, 236, 200)
WHITE = (255, 255, 255)
METAL = (40, 34, 66)
PAL = [ROSE, TEAL, AMBER, INDIGO]

def c(col, a=1.0):
    a = max(0.0, min(1.0, a))
    return (int(col[0] * a), int(col[1] * a), int(col[2] * a))

def mix(a, b, k):
    k = max(0.0, min(1.0, k))
    return tuple(int(a[i] + (b[i] - a[i]) * k) for i in range(3))

def hsh(*xs):
    """Deterministic 0..1 noise."""
    v = 0x9E3779B9
    for x in xs:
        v = (v ^ (int(x) + 0x7F4A7C15 + (v << 6) + (v >> 2))) & 0xFFFFFFFF
        v = (v * 2654435761) & 0xFFFFFFFF
    return ((v >> 8) & 0xFFFF) / 65535.0

def frac(x): return x - math.floor(x)
def smooth(x):
    x = max(0.0, min(1.0, x))
    return x * x * (3 - 2 * x)

# ------------------------------------------------------------------ notes from the song
def track_notes(tid):
    out = []
    for tr in song['tracks']:
        if tr['id'] != tid: continue
        for cl in tr.get('clips', []):
            for n in cl['notes']:
                t0 = (cl['atTick'] + n['tick']) * SEC_PER_TICK
                out.append((t0, t0 + n['durTicks'] * SEC_PER_TICK, n['pitch'], n['velocity']))
    return sorted(out)

DR = track_notes('drums')
KICKS = [n[0] for n in DR if n[2] == 36]
SNARES = [n[0] for n in DR if n[2] == 38 and n[3] > 0.4]
HATS = [n[0] for n in DR if n[2] in (42, 46)]
CRASH = [n[0] for n in DR if n[2] == 49]
RIMS = [n[0] for n in DR if n[2] == 37]
LEAD = track_notes('lead')
VIBES = track_notes('vibes')
BASS = track_notes('bass')
HORN_T = [n[0] for n in LEAD]
VIB_T = [n[0] for n in VIBES]
GTR_T = sorted({round(n[0], 3) for n in track_notes('guitar')})
KEYS = sorted({round(n[0], 3) for n in track_notes('keys')})
BASS_T = [n[0] for n in BASS]

def env(times, t, k):
    i = bisect.bisect_right(times, t) - 1
    return math.exp(-(t - times[i]) * k) if i >= 0 else 0.0

def count(times, t):
    return bisect.bisect_right(times, t)

# ------------------------------------------------------------------ fonts
def font(path, size, index=0):
    try: return ImageFont.truetype(path, size, index=index)
    except OSError: return ImageFont.load_default(size)
_fonts = {}
def F(kind, size):
    key = (kind, size)
    if key not in _fonts:
        _fonts[key] = {
            'black': lambda: font('/System/Library/Fonts/Supplemental/Arial Black.ttf', size),
            'din':   lambda: font('/System/Library/Fonts/Supplemental/DIN Condensed Bold.ttf', size),
            'cn':    lambda: font('/System/Library/Fonts/Hiragino Sans GB.ttc', size, 1),
            'mono':  lambda: font('/System/Library/Fonts/Menlo.ttc', size, 1),
        }[kind]()
    return _fonts[key]

def text(d, xy, s, kind, size, fill, anchor='mm'):
    d.text(xy, s, font=F(kind, size), fill=fill, anchor=anchor)

# ------------------------------------------------------------------ frame context
class Ctx:
    def __init__(self, t):
        self.t = t
        self.beat = t / BEAT
        self.bar = min(NBARS - 1, int(t / BARLEN))
        self.inbar = self.beat - self.bar * 4
        for sid, name, start, n in SECTIONS:
            if start <= self.bar < start + n:
                self.sid, self.name, self.sstart, self.sbars = sid, name, start, n
        self.sbar = self.bar - self.sstart
        self.st = t - self.sstart * BARLEN                 # seconds into the section
        self.sp = self.st / (self.sbars * BARLEN)          # 0..1 through the section
        self.kick = env(KICKS, t, 9)
        self.snare = env(SNARES, t, 10)
        self.hat = env(HATS, t, 22)
        self.crash = env(CRASH, t, 2.2)
        self.horn = env(HORN_T, t, 6)
        self.gtr = env(GTR_T, t, 14)
        self.keys = env(KEYS, t, 5)
        self.bassenv = env(BASS_T, t, 8)
        self.vib = env(VIB_T, t, 5)
        self.rim = env(RIMS, t, 8)
        self.pulse = math.exp(-frac(self.beat) * 4)        # the click, when the drums are out
        self.base = Image.new('RGB', (W, H), BG)
        self.neon = Image.new('RGB', (W, H), (0, 0, 0))
        self.db = ImageDraw.Draw(self.base)
        self.dn = ImageDraw.Draw(self.neon)
        self.wash = None                                   # a colour added over the whole frame

# ------------------------------------------------------------------ building blocks
_grad = {}
def gradient(top, mid, bottom, split=0.55):
    key = (top, mid, bottom, split)
    if key not in _grad:
        im = Image.new('RGB', (1, H))
        for y in range(H):
            k = y / (H - 1)
            col = mix(top, mid, k / split) if k < split else mix(mid, bottom, (k - split) / (1 - split))
            im.putpixel((0, y), col)
        _grad[key] = im.resize((W, H))
    return _grad[key]

def sky(x, top=(14, 12, 28), mid=(44, 36, 84), bottom=(14, 12, 28)):
    x.base.paste(gradient(top, mid, bottom))

def stars(x, n=90, ymax=330):
    for i in range(n):
        px, py = hsh(i, 1) * W, hsh(i, 2) * ymax
        tw = 0.35 + 0.65 * abs(math.sin(x.t * (0.6 + hsh(i, 3) * 2) + i))
        x.dn.point((px, py), fill=c(WHITE, tw * 0.8))
        if hsh(i, 4) > 0.86:
            x.dn.line([(px - 3, py), (px + 3, py)], fill=c(TEAL, tw * 0.5))
            x.dn.line([(px, py - 3), (px, py + 3)], fill=c(TEAL, tw * 0.5))

_BUILD = []
def buildings():
    if not _BUILD:
        px = -20
        i = 0
        while px < W + 20:
            w = 46 + int(hsh(i, 11) * 70)
            h = 70 + int(hsh(i, 12) * 210)
            _BUILD.append((px, w, h, i))
            px += w + int(hsh(i, 13) * 10)
            i += 1
    return _BUILD

def skyline(x, ground, rise=1.0, lit=1.0):
    step = int(x.beat * 2)
    for px, w, h, i in buildings():
        hh = h * rise
        top = ground - hh
        x.db.rectangle([px, top, px + w, ground], fill=(12, 10, 26))
        x.dn.rectangle([px, top, px + w, ground], fill=(0, 0, 0))
        x.dn.line([(px, top), (px + w, top)], fill=c(PAL[i % 3], 0.5 * lit), width=2)
        if hsh(i, 14) > 0.7:                       # antenna
            x.dn.line([(px + w / 2, top), (px + w / 2, top - 26)], fill=c(ROSE, 0.5 * lit))
            if int(x.beat) % 2 == i % 2:
                x.dn.ellipse([px + w / 2 - 3, top - 30, px + w / 2 + 3, top - 24], fill=c(ROSE, lit))
        wy = top + 12
        row = 0
        while wy < ground - 12:
            wx = px + 8
            col = 0
            while wx < px + w - 10:
                on = hsh(i, row, col, step // 4 + (1 if hsh(i, row, col) > 0.8 else 0) * step) > 0.62
                if on:
                    colr = PAL[int(hsh(i, row, col, 5) * 3)]
                    x.dn.rectangle([wx, wy, wx + 6, wy + 8], fill=c(colr, (0.45 + 0.4 * x.hat) * lit))
                wx += 15
                col += 1
            wy += 20
            row += 1

def rain(x, n=110, a=0.35):
    for i in range(n):
        px = hsh(i, 21) * (W + 200) - 100
        sp = 600 + hsh(i, 22) * 500
        py = frac(hsh(i, 23) + x.t * sp / H) * (H + 80) - 40
        x.dn.line([(px, py), (px - 8, py + 26)], fill=c(TEAL, a * (0.4 + 0.6 * hsh(i, 24))))

def eq_bars(x, x0, x1, base, hmax, n=32, col=TEAL, a=0.9, seedk=0):
    bw = (x1 - x0) / n
    step = int(x.beat * 4)
    for i in range(n):
        target = hsh(i, step, seedk)
        prev = hsh(i, step - 1, seedk)
        v = prev + (target - prev) * smooth(frac(x.beat * 4))
        lo = 1 - i / n
        v = v * (0.35 + 0.65 * (lo * x.kick + (1 - lo) * max(x.hat, x.gtr) + 0.4 * x.bassenv))
        h = 6 + v * hmax
        colr = mix(col, ROSE, i / n)
        x.dn.rectangle([x0 + i * bw + 2, base - h, x0 + (i + 1) * bw - 2, base], fill=c(colr, a))

def rings(x, cx, cy, times, col, k=2.5, rmax=520, width=4):
    """Expanding rings, one per event."""
    i = bisect.bisect_right(times, x.t) - 1
    while i >= 0 and x.t - times[i] < 1.2:
        age = x.t - times[i]
        r = 30 + age * rmax
        x.dn.ellipse([cx - r, cy - r, cx + r, cy + r], outline=c(col, math.exp(-age * k)), width=width)
        i -= 1

def spotlights(x, n=4, a=0.22, y0=-40):
    for i in range(n):
        ox = W * (i + 0.5) / n
        ang = math.sin(x.beat * math.pi / 4 + i * 1.7) * 0.5
        tx = ox + math.tan(ang) * (H + 40)
        col = PAL[i % 3]
        x.dn.polygon([(ox - 8, y0), (ox + 8, y0), (tx + 150, H), (tx - 150, H)], fill=c(col, a * (0.6 + 0.4 * x.kick)))

# ------------------------------------------------------------------ the robot band
def limb(x, p1, p2, w, col):
    x.dn.line([p1, p2], fill=c(col, 0.75), width=w + 5)
    for p in (p1, p2):
        x.dn.ellipse([p[0] - w / 2 - 2, p[1] - w / 2 - 2, p[0] + w / 2 + 2, p[1] + w / 2 + 2], fill=c(col, 0.75))
    x.dn.line([p1, p2], fill=(0, 0, 0), width=w)
    x.db.line([p1, p2], fill=METAL, width=w)
    for p in (p1, p2):
        x.dn.ellipse([p[0] - w / 2, p[1] - w / 2, p[0] + w / 2, p[1] + w / 2], fill=(0, 0, 0))
        x.db.ellipse([p[0] - w / 2, p[1] - w / 2, p[0] + w / 2, p[1] + w / 2], fill=(58, 46, 108))

def rbox(x, box, r, col, fill=METAL, width=3):
    x.db.rounded_rectangle(box, r, fill=fill)
    x.dn.rounded_rectangle(box, r, fill=(0, 0, 0), outline=c(col, 0.9), width=width)

def robot(x, px, py, s, col, role, phase=0.0, energy=1.0, phones=False):
    """A dancing android. (px, py) is the point between its feet; s is the scale."""
    b = x.beat + phase
    bounce = (x.kick * 0.7 + 0.3 * abs(math.sin(b * math.pi))) * energy
    sway = math.sin(b * math.pi / 2) * 10 * s * energy
    hip_y = py - 92 * s + bounce * 12 * s
    hx = px + sway
    sh_y = hip_y - 78 * s
    # shadow
    x.db.ellipse([px - 60 * s, py - 6 * s, px + 60 * s, py + 10 * s], fill=(5, 2, 16))
    x.dn.ellipse([px - 60 * s, py - 6 * s, px + 60 * s, py + 10 * s], outline=c(col, 0.25), width=2)
    # legs
    for side in (-1, 1):
        foot = (px + side * 26 * s, py - 4 * s)
        knee = (px + side * (24 + 14 * bounce) * s + sway * 0.5, (hip_y + py) / 2 + 4 * s)
        hip = (hx + side * 16 * s, hip_y)
        limb(x, hip, knee, int(13 * s), col)
        limb(x, knee, foot, int(12 * s), col)
        rbox(x, [foot[0] - 17 * s, foot[1] - 6 * s, foot[0] + 17 * s, foot[1] + 6 * s], int(5 * s), col)
    # arms (behind the torso for the far one)
    def arm(side, elbow, hand):
        sh = (hx + side * 36 * s, sh_y + 10 * s)
        limb(x, sh, elbow, int(11 * s), col)
        limb(x, elbow, hand, int(10 * s), col)
        x.dn.ellipse([hand[0] - 8 * s, hand[1] - 8 * s, hand[0] + 8 * s, hand[1] + 8 * s], outline=c(col), width=2)
        return hand
    up = x.horn if role == 'horn' else 0.0
    if role == 'bass':
        strum = math.sin(x.beat * math.pi * 4) * 6 * s * (0.4 + x.bassenv)
        arm(-1, (hx - 62 * s, sh_y + 34 * s), (hx - 84 * s, sh_y - 2 * s))
    elif role == 'keys':
        hit = x.keys * 10 * s
        arm(-1, (hx - 56 * s, sh_y + 44 * s), (hx - 44 * s, sh_y + 78 * s + hit))
    elif role == 'horn':
        arm(-1, (hx - 54 * s, sh_y + 30 * s - up * 20 * s), (hx - 30 * s, sh_y - 14 * s - up * 30 * s))
    else:
        a = math.sin(b * math.pi) * energy
        arm(-1, (hx - 66 * s, sh_y + 6 * s - a * 26 * s), (hx - 84 * s, sh_y - 44 * s - a * 34 * s))
    # torso
    rbox(x, [hx - 40 * s, sh_y, hx + 40 * s, hip_y + 8 * s], int(14 * s), col)
    core = 9 * s * (1 + 0.5 * x.kick)
    x.dn.ellipse([hx - core, sh_y + 30 * s - core, hx + core, sh_y + 30 * s + core], fill=c(col, 0.6 + 0.4 * x.kick))
    for i in range(3):
        x.dn.line([(hx - 22 * s, hip_y - (8 + i * 8) * s), (hx + 22 * s, hip_y - (8 + i * 8) * s)], fill=c(col, 0.35))
    # head
    tilt = math.sin(b * math.pi / 2 + 0.6) * 6 * s * energy
    hy = sh_y - 14 * s - bounce * 3 * s
    limb(x, (hx, sh_y), (hx + tilt * 0.5, hy), int(10 * s), col)
    hb = [hx - 34 * s + tilt, hy - 54 * s, hx + 34 * s + tilt, hy]
    rbox(x, hb, int(16 * s), col)
    if phones:
        x.dn.arc([hb[0] - 6 * s, hb[1] - 22 * s, hb[2] + 6 * s, hb[1] + 40 * s], 180, 360, fill=c(CREAM), width=max(3, int(5 * s)))
        for ex in (hb[0] - 8 * s, hb[2] + 8 * s):
            x.dn.rounded_rectangle([ex - 8 * s, hb[1] + 8 * s, ex + 8 * s, hb[1] + 40 * s], int(6 * s), fill=c(ROSE, 0.9))
    vis = [hb[0] + 8 * s, hb[1] + 16 * s, hb[2] - 8 * s, hb[1] + 34 * s]
    x.dn.rounded_rectangle(vis, int(8 * s), fill=c(col, 0.28), outline=c(col), width=2)
    blink = frac(x.t * 0.31 + phase) > 0.96
    for side in (-1, 1):
        ex = (vis[0] + vis[2]) / 2 + side * 13 * s
        ey = (vis[1] + vis[3]) / 2
        if blink: x.dn.line([(ex - 5 * s, ey), (ex + 5 * s, ey)], fill=WHITE, width=2)
        else: x.dn.ellipse([ex - 4.5 * s, ey - 4.5 * s, ex + 4.5 * s, ey + 4.5 * s], fill=WHITE)
    ant = (hx + tilt, hb[1])
    x.dn.line([ant, (ant[0] + tilt * 0.6, ant[1] - 20 * s)], fill=c(col), width=2)
    r = 5 * s * (1 + x.hat)
    x.dn.ellipse([ant[0] + tilt * 0.6 - r, ant[1] - 20 * s - r, ant[0] + tilt * 0.6 + r, ant[1] - 20 * s + r], fill=c(AMBER))
    # instrument and near arm
    if role == 'bass':
        body = (hx + 34 * s, hip_y - 14 * s)
        head = (hx - 96 * s, sh_y - 10 * s)
        x.dn.line([body, head], fill=c(AMBER), width=int(7 * s))
        rbox(x, [body[0] - 30 * s, body[1] - 24 * s, body[0] + 34 * s, body[1] + 26 * s], int(20 * s), AMBER, fill=(60, 30, 70))
        for i in range(2):
            o = (i * 5 - 2) * s + (strum if i == 0 else -strum) * 0.25
            x.dn.line([(body[0], body[1] + o), (head[0], head[1] + o * 0.3)], fill=c(WHITE, 0.5 + 0.5 * x.bassenv))
        arm(1, (hx + 58 * s, sh_y + 44 * s), (body[0] + 6 * s, body[1] - 2 * s + strum))
    elif role == 'keys':
        ky = sh_y + 84 * s
        rbox(x, [hx - 108 * s, ky, hx + 108 * s, ky + 26 * s], int(6 * s), TEAL, fill=(20, 16, 50))
        for i in range(14):
            kx = hx - 100 * s + i * 14.4 * s
            lit = hsh(i, count(KEYS, x.t)) > 0.7
            x.dn.rectangle([kx, ky + 5 * s, kx + 11 * s, ky + 20 * s], fill=c(AMBER if lit else WHITE, (0.5 + 0.5 * x.keys) if lit else 0.3))
        for side in (-1, 1):
            limb(x, (hx + side * 90 * s, ky + 26 * s), (hx + side * 70 * s, py), int(6 * s), TEAL)
        arm(1, (hx + 56 * s, sh_y + 44 * s), (hx + 44 * s, sh_y + 78 * s + x.keys * 10 * s * (1 if int(x.beat * 2) % 2 else 0.3)))
    elif role == 'horn':
        mouth = (hx + tilt - 30 * s, hy - 12 * s)
        bell = (mouth[0] - 96 * s, mouth[1] - (10 + up * 46) * s)
        x.dn.line([mouth, bell], fill=c(AMBER), width=int(8 * s))
        dx, dy = bell[0] - mouth[0], bell[1] - mouth[1]
        ln = math.hypot(dx, dy)
        nx, ny = -dy / ln, dx / ln
        fl = 24 * s * (1 + 0.5 * up)
        x.dn.polygon([(bell[0] + dx / ln * 26 * s + nx * fl, bell[1] + dy / ln * 26 * s + ny * fl),
                      (bell[0] + dx / ln * 26 * s - nx * fl, bell[1] + dy / ln * 26 * s - ny * fl),
                      (bell[0] + nx * 6 * s, bell[1] + ny * 6 * s), (bell[0] - nx * 6 * s, bell[1] - ny * 6 * s)], fill=c(AMBER))
        if up > 0.08:                               # the blast
            for k in range(3):
                rr = (30 + k * 26 + (1 - up) * 60) * s
                ctr = (bell[0] + dx / ln * 40 * s, bell[1] + dy / ln * 40 * s)
                x.dn.arc([ctr[0] - rr, ctr[1] - rr, ctr[0] + rr, ctr[1] + rr], 130, 230, fill=c(AMBER, up), width=3)
        arm(1, (hx + 20 * s, sh_y + 44 * s - up * 16 * s), (mouth[0] - 46 * s, mouth[1] - (4 + up * 24) * s))
    else:
        a = -math.sin(b * math.pi) * energy
        arm(1, (hx + 66 * s, sh_y + 6 * s - a * 26 * s), (hx + 84 * s, sh_y - 44 * s - a * 34 * s))

# ------------------------------------------------------------------ shared pieces
def melody_ribbon(x, notes, y0, y1, lo=60, hi=86):
    """The tune as a scrolling piano roll: now is at the centre, the future comes from the right."""
    pps = 150.0
    cx = W / 2
    i = bisect.bisect_left([n[0] for n in notes], x.t - 5)
    pts = []
    for n in notes[i:]:
        if n[0] > x.t + 5: break
        xa, xb = cx + (n[0] - x.t) * pps, cx + (n[1] - x.t) * pps
        yy = y1 - (n[2] - lo) / (hi - lo) * (y1 - y0)
        on = n[0] <= x.t < n[1]
        past = n[1] <= x.t
        col = AMBER if on else (c(TEAL, 0.35) if past else TEAL)
        x.dn.rounded_rectangle([xa, yy - 7, max(xa + 6, xb), yy + 7], 6, fill=col)
        if on:
            x.dn.ellipse([cx - 16, yy - 16, cx + 16, yy + 16], outline=WHITE, width=3)
            pts.append(yy)
    x.dn.line([(cx, y0 - 16), (cx, y1 + 16)], fill=c(WHITE, 0.35), width=1)

def scope(x, x0, x1, cy, amp, col):
    pts = []
    n = 160
    for i in range(n + 1):
        u = i / n
        w = math.sin(u * math.pi)
        y = (math.sin(u * 34 + x.t * 22) * (0.3 + x.bassenv) + math.sin(u * 91 - x.t * 40) * 0.4 * x.hat
             + math.sin(u * 13 + x.t * 9) * 0.8 * x.kick)
        pts.append((x0 + (x1 - x0) * u, cy + y * amp * w))
    x.dn.line(pts, fill=col, width=3)

def piano_roll(x, notes, lo=60, hi=96, ytop=110, ykey=500, ybot=610):
    whites = [p for p in range(lo, hi + 1) if p % 12 in (0, 2, 4, 5, 7, 9, 11)]
    kw = W / len(whites)
    xpos = {}
    for i, p in enumerate(whites):
        xpos[p] = (i * kw, (i + 1) * kw)
    for p in range(lo, hi + 1):
        if p not in xpos:
            a = xpos[p - 1][1]
            xpos[p] = (a - kw * 0.3, a + kw * 0.3)
    active = {n[2] for n in notes if n[0] <= x.t < n[1]}
    pps = 190.0
    for n in notes:
        if n[1] < x.t - 0.1 or n[0] > x.t + 3.2 or n[2] not in xpos: continue
        xa, xb = xpos[n[2]]
        y1 = ykey - max(0.0, n[0] - x.t) * pps
        y0 = ykey - (n[1] - x.t) * pps
        y0 = max(ytop, y0)
        if y1 - y0 < 4: continue
        on = n[0] <= x.t
        col = AMBER if on else PAL[n[2] % 2]
        x.dn.rounded_rectangle([xa + 3, y0, xb - 3, y1], 7, fill=c(col, 1.0 if on else 0.75))
        if on:
            mx = (xa + xb) / 2
            for k in range(5):
                ang = hsh(n[2], k, int(x.t * 20)) * math.pi
                ln = 20 + 50 * hsh(k, int(x.t * 30))
                x.dn.line([(mx, ykey), (mx + math.cos(ang) * ln, ykey - math.sin(ang) * ln)], fill=c(WHITE, 0.8))
    for p in whites:
        xa, xb = xpos[p]
        on = p in active
        x.db.rectangle([xa + 1, ykey, xb - 1, ybot], fill=(250, 225, 120) if on else (214, 210, 232))
        if on: x.dn.rectangle([xa + 1, ykey, xb - 1, ybot], fill=c(AMBER, 0.6))
        else: x.dn.rectangle([xa + 1, ykey, xb - 1, ybot], fill=(0, 0, 0))
    for p in range(lo, hi + 1):
        if p in whites: continue
        xa, xb = xpos[p]
        on = p in active
        x.db.rectangle([xa, ykey, xb, ykey + 68], fill=(20, 14, 44))
        x.dn.rectangle([xa, ykey, xb, ykey + 68], fill=c(ROSE, 0.9) if on else (0, 0, 0), outline=c(ROSE, 0.5))
    x.dn.line([(0, ykey), (W, ykey)], fill=c(TEAL), width=3)

# ------------------------------------------------------------------ scenes
TURN = 2 * math.pi * 33.33 / 60          # a record at 33 rpm, in radians per second
NOTE = ['C', 'Db', 'D', 'Eb', 'E', 'F', 'Gb', 'G', 'Ab', 'A', 'Bb', 'B']

def moon(x, cx, cy, r):
    x.dn.ellipse([cx - r - 22, cy - r - 22, cx + r + 22, cy + r + 22], outline=c(AMBER, 0.18), width=10)
    x.db.ellipse([cx - r, cy - r, cx + r, cy + r], fill=(232, 216, 180))
    x.dn.ellipse([cx - r, cy - r, cx + r, cy + r], fill=(0, 0, 0), outline=c(AMBER, 0.6), width=3)
    for i in range(6):
        a, d, cr = hsh(i, 41) * 6.28, hsh(i, 42) * r * 0.62, 5 + hsh(i, 43) * r * 0.16
        px, py = cx + math.cos(a) * d, cy + math.sin(a) * d
        x.db.ellipse([px - cr, py - cr, px + cr, py + cr], fill=(204, 186, 150))

def vinyl(x, cx, cy, r, ang, label=AMBER):
    k = x.kick
    x.dn.ellipse([cx - r - 8 - 6 * k, cy - r - 8 - 6 * k, cx + r + 8 + 6 * k, cy + r + 8 + 6 * k], outline=c(TEAL, 0.25 + 0.5 * k), width=3)
    x.db.ellipse([cx - r, cy - r, cx + r, cy + r], fill=(18, 16, 34))
    x.dn.ellipse([cx - r, cy - r, cx + r, cy + r], fill=(0, 0, 0), outline=c(TEAL, 0.7), width=2)
    n = max(8, int(r / 9))
    for i in range(n):
        rr = r * (0.40 + 0.56 * i / n)
        x.dn.ellipse([cx - rr, cy - rr, cx + rr, cy + rr], outline=c(INDIGO, 0.3 + 0.3 * hsh(i, 51)))
    for a0 in (-62, 118):                 # the light on the grooves stays put while the record turns
        x.dn.pieslice([cx - r * 0.96, cy - r * 0.96, cx + r * 0.96, cy + r * 0.96], a0 - 7, a0 + 7, fill=c(CREAM, 0.1))
    deg = math.degrees(ang)
    for i in range(3):                    # marks that turn with it
        rr = r * (0.5 + 0.16 * i)
        x.dn.arc([cx - rr, cy - rr, cx + rr, cy + rr], deg + i * 120, deg + i * 120 + 40, fill=c(ROSE if i == 1 else AMBER, 0.8), width=3)
    lr = r * 0.34
    x.db.ellipse([cx - lr, cy - lr, cx + lr, cy + lr], fill=(120, 78, 30))
    x.dn.ellipse([cx - lr, cy - lr, cx + lr, cy + lr], fill=c(label, 0.45), outline=c(label), width=2)
    for i in range(4):
        a = ang + i * math.pi / 2
        x.dn.line([(cx + math.cos(a) * lr * 0.3, cy + math.sin(a) * lr * 0.3), (cx + math.cos(a) * lr * 0.85, cy + math.sin(a) * lr * 0.85)], fill=BG, width=3)
    sr = max(3, lr * 0.12)
    x.dn.ellipse([cx - sr, cy - sr, cx + sr, cy + sr], fill=(0, 0, 0))
    x.db.ellipse([cx - sr, cy - sr, cx + sr, cy + sr], fill=(10, 8, 20))

def tonearm(x, pivot, head):
    x.dn.line([pivot, head], fill=(0, 0, 0), width=9)
    x.db.line([pivot, head], fill=(208, 196, 172), width=9)
    x.dn.line([pivot, head], fill=c(CREAM, 0.35), width=2)
    px, py = pivot
    x.db.ellipse([px - 26, py - 26, px + 26, py + 26], fill=(60, 52, 90))
    x.dn.ellipse([px - 26, py - 26, px + 26, py + 26], fill=(0, 0, 0), outline=c(CREAM, 0.8), width=3)
    hx, hy = head
    x.dn.rounded_rectangle([hx - 20, hy - 12, hx + 20, hy + 12], 5, fill=c(ROSE, 0.9))

def turntable(x, ang, arm, cx=760, cy=392, r=215):
    x.base.paste(gradient((10, 8, 20), (30, 24, 52), (10, 8, 20)))
    rbox(x, [cx - 320, cy - 248, cx + 400, cy + 248], 26, AMBER, fill=(26, 22, 44))
    vinyl(x, cx, cy, r, ang)
    rest, play = (cx + 352, cy + 150), (cx + 128, cy + 96)
    a = smooth(arm)
    tonearm(x, (cx + 330, cy - 180), (rest[0] + (play[0] - rest[0]) * a, rest[1] + (play[1] - rest[1]) * a))
    for i in range(3):
        bx = cx + 300 + i * 30
        x.dn.ellipse([bx - 7, cy + 205, bx + 7, cy + 219], fill=c(TEAL if i else ROSE, 0.4 + 0.6 * (x.hat if i else x.kick)))

def title_column(x, a=1.0):
    text(x.dn, (215, 230), 'SLOW', 'black', 84, c(AMBER, a * (0.8 + 0.2 * x.keys)))
    text(x.dn, (215, 320), 'SPIN', 'black', 84, c(ROSE, a * (0.8 + 0.2 * x.keys)))
    text(x.dn, (215, 405), '午 夜 慢 转', 'cn', 40, c(CREAM, a))
    text(x.dn, (215, 452), 'F minor · 86 BPM', 'mono', 20, c(TEAL, a))

def scene_intro(x):
    ang = TURN * x.st * x.st / (2 * BARLEN) if x.st < BARLEN else TURN * (x.st - BARLEN / 2)   # the platter spins up over the count
    turntable(x, ang, (x.st - BEAT) / (BEAT * 2.5))
    if x.sbar == 0:
        text(x.dn, (215, 340), str(int(x.inbar) + 1), 'black', 170, c(CREAM, 0.35 + 0.65 * x.rim))
    else:
        title_column(x, smooth((x.st - BARLEN) / BEAT))
    if x.sbar == 3:
        text(x.dn, (215, 545), str(4 - int(x.inbar)), 'black', 90, c(WHITE, 0.4 + 0.6 * x.pulse))

# -- the room: a beat-maker by the window
WIN = (110, 120, 760, 470)

def cat(x, cx, base):
    dark = (8, 6, 16)
    a = math.sin(x.beat * math.pi / 2)
    shapes = [('e', [cx - 34, base - 62, cx + 34, base]), ('e', [cx, base - 100, cx + 44, base - 56]),
              ('p', [(cx + 4, base - 88), (cx + 8, base - 116), (cx + 22, base - 96)]),
              ('p', [(cx + 26, base - 96), (cx + 40, base - 116), (cx + 42, base - 86)])]
    tail = [(cx - 28 - j * 9, base - 8 - j * j * 0.9 * (1 + 0.35 * a) - 4 * math.sin(j * 0.8 + a)) for j in range(8)]
    for d, col in ((x.db, dark), (x.dn, (0, 0, 0))):
        for kind, pts in shapes:
            (d.ellipse if kind == 'e' else d.polygon)(pts, fill=col)
        d.line(tail, fill=col, width=9)
    if frac(x.t * 0.23) < 0.94:
        for ex in (cx + 14, cx + 30):
            x.dn.ellipse([ex - 3, base - 82, ex + 3, base - 76], fill=c(AMBER))

def pads(x, x0, y0):
    src = {0: x.hat, 1: x.rim, 3: x.vib, 5: x.keys, 6: x.gtr, 10: x.horn, 12: x.kick, 13: x.snare, 15: x.bassenv}
    for j in range(16):
        px, py = x0 + (j % 4) * 60, y0 + (j // 4) * 30
        v = src.get(j, 0.0)
        x.dn.rounded_rectangle([px, py, px + 52, py + 24], 5, fill=c(PAL[j % 4], 0.12 + 0.88 * v), outline=c(PAL[j % 4], 0.5))

def scene_room(x):
    sky(x, (10, 10, 30), (40, 34, 84), (20, 16, 40))
    stars(x, n=60, ymax=300)
    moon(x, 620, 200, 50)
    skyline(x, 470, lit=0.8)
    rain(x, n=90, a=0.3)
    x0, y0, x1, y1 = WIN
    wall = (24, 18, 34)
    for box in ([0, 0, W, y0], [0, y1, W, H], [0, y0, x0, y1], [x1, y0, W, y1]):
        x.db.rectangle(box, fill=wall); x.dn.rectangle(box, fill=(0, 0, 0))
    x.dn.rectangle([x0, y0, x1, y1], outline=c(AMBER, 0.7), width=4)
    x.dn.line([((x0 + x1) / 2, y0), ((x0 + x1) / 2, y1)], fill=c(AMBER, 0.5), width=3)
    x.dn.line([(x0, (y0 + y1) / 2), (x1, (y0 + y1) / 2)], fill=c(AMBER, 0.5), width=3)
    x.db.rectangle([x0 - 20, y1, x1 + 20, y1 + 14], fill=(50, 38, 56))
    cat(x, 230, y1)
    x.db.rectangle([662, y1 - 30, 698, y1], fill=(200, 190, 170)); x.dn.rectangle([662, y1 - 30, 698, y1], fill=(0, 0, 0))
    for k in range(3):                          # steam
        pts = [(680 + (k - 1) * 10 + math.sin(x.t * 1.5 + j * 0.6 + k) * 6, y1 - 36 - j * 8) for j in range(9)]
        x.dn.line(pts, fill=c(CREAM, 0.35), width=2)
    # shelf of records: one spine lights up per bar
    x.db.rectangle([800, 226, 1240, 236], fill=(70, 52, 60))
    for i in range(27):
        h = 58 + hsh(i, 61) * 22
        on = i == x.bar % 27
        x.dn.rectangle([806 + i * 16, 226 - h, 818 + i * 16, 226], fill=c(PAL[int(hsh(i, 62) * 4)], 0.9 if on else 0.28))
    # lamp
    x.dn.line([(950, 236), (950, 268)], fill=c(CREAM, 0.5), width=2)
    x.dn.polygon([(928, 296), (972, 296), (962, 268), (938, 268)], fill=c(AMBER, 0.9))
    x.dn.polygon([(930, 298), (970, 298), (1130, 560), (790, 560)], fill=c(AMBER, 0.07 + 0.05 * x.keys))
    # desk, sampler, a small deck
    x.db.rectangle([780, 560, W, H], fill=(36, 28, 40)); x.dn.rectangle([780, 560, W, H], fill=(0, 0, 0))
    x.dn.line([(780, 560), (W, 560)], fill=c(AMBER, 0.6), width=3)
    rbox(x, [816, 412, 1076, 560], 12, TEAL, fill=(20, 18, 40))
    pads(x, 830, 428)
    vinyl(x, 1172, 492, 62, TURN * x.t)
    rings(x, 946, 486, VIB_T, TEAL, rmax=190, width=2)
    robot(x, 560, 694, 1.2, TEAL, 'keys', 0.5, phones=True)

def scene_a1(x): scene_room(x)

# -- the roof: the band under the moon
def string_lights(x, y=104):
    n = 24
    pts = []
    for i in range(n + 1):
        u = i / n
        px, py = 384 + u * 730, y + 150 * u * (1 - u)
        pts.append((px, py))
    x.dn.line(pts, fill=c(CREAM, 0.3))
    for i, (px, py) in enumerate(pts):
        on = hsh(i, int(x.beat * 2)) > 0.35
        r = 5 if on else 3
        x.dn.ellipse([px - r, py + 2, px + r, py + 2 + 2 * r], fill=c(PAL[i % 4], (0.45 + 0.55 * x.hat) if on else 0.25))

def fireflies(x, n=46):
    for i in range(n):
        px = hsh(i, 71) * W + math.sin(x.t * 0.5 + i) * 40
        py = 130 + hsh(i, 72) * 400 + math.sin(x.t * 0.7 + i * 2) * 30
        a = 0.25 + 0.75 * abs(math.sin(x.t * (1 + hsh(i, 73)) + i))
        x.dn.ellipse([px - 2.5, py - 2.5, px + 2.5, py + 2.5], fill=c(AMBER, a))

def roof(x, roles, full=False):
    sky(x, (8, 8, 24), (46, 36, 92), (30, 20, 50))
    stars(x, n=110, ymax=360)
    moon(x, 236, 232, 78)
    skyline(x, 540, lit=0.9)
    x.db.rectangle([0, 560, W, H], fill=(16, 13, 28)); x.dn.rectangle([0, 560, W, H], fill=(0, 0, 0))
    x.dn.line([(0, 560), (W, 560)], fill=c(AMBER, 0.8), width=3)
    x.dn.line([(0, 544), (W, 544)], fill=c(AMBER, 0.35), width=2)
    for px in range(20, W, 64):
        x.dn.line([(px, 544), (px, 560)], fill=c(AMBER, 0.35), width=2)
    string_lights(x)
    melody_ribbon(x, LEAD, 170, 320)
    spots = {'bass': (560, ROSE, 0.0), 'keys': (810, TEAL, 0.5), 'horn': (1090, AMBER, 1.0)}
    for r in roles:
        px, col, ph = spots[r]
        robot(x, px, 694, 1.1, col, r, ph)
    rings(x, 1000, 420, HORN_T, AMBER, rmax=220, width=3)
    if full:
        fireflies(x)
        rings(x, 810, 300, VIB_T, TEAL, k=3, rmax=300, width=2)

def scene_a2(x): roof(x, ('keys', 'horn'))
def scene_a4(x): roof(x, ('bass', 'keys', 'horn'), full=True)

# -- the night train: the bridge goes somewhere else
def skyline_scroll(x, ground, offset, pitch, hmax, lit, seed, fill):
    k0 = math.floor(offset / pitch)
    for k in range(-1, int(W / pitch) + 2):
        idx = k + k0
        px = k * pitch - (offset % pitch)
        w = pitch * (0.6 + 0.35 * hsh(idx, seed, 1))
        h = hmax * (0.3 + 0.7 * hsh(idx, seed, 2))
        x.db.rectangle([px, ground - h, px + w, ground], fill=fill)
        x.dn.rectangle([px, ground - h, px + w, ground], fill=(0, 0, 0))
        x.dn.line([(px, ground - h), (px + w, ground - h)], fill=c(PAL[idx % 3], 0.4 * lit), width=2)
        wy, row = ground - h + 10, 0
        while wy < ground - 10:
            wx, col = px + 7, 0
            while wx < px + w - 9:
                if hsh(idx, row, col, seed) > 0.6:
                    x.dn.rectangle([wx, wy, wx + 5, wy + 7], fill=c(PAL[int(hsh(idx, row, col, 7) * 3)], 0.55 * lit))
                wx += 14; col += 1
            wy += 18; row += 1

def scene_b1(x):
    sky(x, (12, 10, 34), (70, 40, 96), (150, 80, 90))
    stars(x, n=70, ymax=260)
    moon(x, 230, 250, 62)
    skyline_scroll(x, 500, x.t * 40, 120, 190, 0.6, 3, (24, 20, 48))
    skyline_scroll(x, 500, x.t * 110, 170, 290, 1.0, 5, (12, 10, 26))
    melody_ribbon(x, VIBES, 150, 300)
    for y in (140, 158):
        x.dn.line([(0, y), (W, y)], fill=c(CREAM, 0.22))
    px = W * (1.15 - 1.3 * frac(x.beat / 2))        # a pole goes by every two beats
    for d, col in ((x.db, (8, 6, 16)), (x.dn, (0, 0, 0))):
        d.rectangle([px - 8, 120, px + 8, 500], fill=col)
        d.rectangle([px - 44, 136, px + 44, 144], fill=col)
    wall = (26, 20, 38)
    for box in ([0, 0, W, 120], [0, 500, W, H], [0, 120, 50, 500], [W - 50, 120, W, 500], [428, 120, 452, 500], [828, 120, 852, 500]):
        x.db.rectangle(box, fill=wall); x.dn.rectangle(box, fill=(0, 0, 0))
    x.dn.rounded_rectangle([50, 120, W - 50, 500], 18, outline=c(TEAL, 0.7), width=4)
    x.dn.line([(380, 100), (W - 170, 100)], fill=c(CREAM, 0.5), width=3)
    for i in range(6):                               # straps sway with the car
        hx = 420 + i * 130
        sw = math.sin(x.beat * math.pi / 2 + i * 0.4) * 9
        x.dn.line([(hx, 100), (hx + sw, 150)], fill=c(CREAM, 0.5), width=3)
        x.dn.ellipse([hx + sw - 14, 150, hx + sw + 14, 178], outline=c(CREAM, 0.7), width=3)
    for i in range(5):
        sx = 70 + i * 232
        rbox(x, [sx, 566, sx + 204, 730], 18, ROSE if i % 2 else AMBER, fill=(40, 30, 50))
    x.dn.line([(0, 520), (W, 520)], fill=c(TEAL, 0.4), width=2)
    robot(x, 1040, 700, 1.0, TEAL, 'dance', 0.0, energy=0.35, phones=True)

# -- the record, close up
def scene_a3(x):
    x.base.paste(gradient((10, 8, 22), (34, 24, 60), (10, 8, 22)))
    cx, cy, r = 800, 372, 290
    n = 96
    for i in range(n):                               # the horn and the vibes light the rim
        a = i * 2 * math.pi / n + x.t * 0.15
        v = (0.25 + 0.75 * hsh(i, int(x.beat * 4))) * (0.2 + 0.8 * max(x.horn, x.vib * 0.8))
        r0, r1 = r + 16, r + 24 + v * 60
        x.dn.line([(cx + math.cos(a) * r0, cy + math.sin(a) * r0), (cx + math.cos(a) * r1, cy + math.sin(a) * r1)], fill=mix(AMBER, ROSE, i / n), width=5)
    vinyl(x, cx, cy, r, TURN * x.t)
    tonearm(x, (1215, 150), (cx + 180, cy - 66))
    rings(x, cx, cy, VIB_T, TEAL, k=3, rmax=420, width=3)
    robot(x, 262, 606, 1.0, AMBER, 'horn', 0.0)
    on = [n for n in LEAD if n[0] <= x.t < n[1]]
    if on:                                           # the note the horn is on
        text(x.dn, (250, 196), NOTE[on[-1][2] % 12], 'black', 96, c(AMBER, 0.95))
    text(x.dn, (250, 268), '小号', 'cn', 24, c(CREAM, 0.7))

# -- the tape: the break
def scene_break(x):
    x.base.paste(gradient((8, 6, 18), (28, 18, 46), (8, 6, 18)))
    sh = 5 * x.kick
    rbox(x, [290, 150 + sh, 990, 580 + sh], 28, AMBER, fill=(30, 24, 46), width=4)
    x.db.rounded_rectangle([330, 180 + sh, 950, 300 + sh], 12, fill=(60, 44, 40))
    x.dn.rounded_rectangle([330, 180 + sh, 950, 300 + sh], 12, outline=c(CREAM, 0.6), width=2)
    text(x.dn, (640, 224 + sh), 'SLOW SPIN', 'black', 50, c(AMBER, 0.7 + 0.3 * x.keys))
    text(x.dn, (640, 272 + sh), 'SIDE A  ·  午夜慢转  ·  86 BPM', 'cn', 22, c(CREAM, 0.8))
    x.db.rounded_rectangle([400, 330 + sh, 880, 480 + sh], 20, fill=(12, 10, 24))
    x.dn.rounded_rectangle([400, 330 + sh, 880, 480 + sh], 20, fill=(0, 0, 0), outline=c(TEAL, 0.8), width=3)
    for side, rx in ((-1, 520), (1, 760)):           # the tape winds from the left reel to the right one
        rr = 36 + 30 * ((1 - x.sp) if side < 0 else x.sp)
        cy = 405 + sh
        x.dn.ellipse([rx - rr, cy - rr, rx + rr, cy + rr], fill=c(INDIGO, 0.5), outline=c(INDIGO), width=2)
        x.dn.ellipse([rx - 26, cy - 26, rx + 26, cy + 26], fill=(0, 0, 0), outline=c(CREAM, 0.9), width=3)
        for i in range(6):
            a = x.t * 2.4 + i * math.pi / 3
            x.dn.line([(rx + math.cos(a) * 10, cy + math.sin(a) * 10), (rx + math.cos(a) * 24, cy + math.sin(a) * 24)], fill=c(CREAM, 0.9), width=3)
    x.dn.line([(520, 468 + sh), (760, 468 + sh)], fill=c(INDIGO), width=3)
    for px, py in ((316, 176), (964, 176), (316, 554), (964, 554)):
        x.dn.ellipse([px - 7, py - 7 + sh, px + 7, py + 7 + sh], outline=c(AMBER, 0.8), width=2)
    for i in range(5):
        x.dn.ellipse([520 + i * 60 - 9, 524 + sh, 520 + i * 60 + 9, 542 + sh], outline=c(AMBER, 0.5), width=2)
    text(x.dn, (640, 92), 'BREAK', 'black', 64, c(CREAM, 0.3 + 0.5 * x.pulse))
    scope(x, 430, 1240, 652, 34, c(ROSE, 0.8))
    text(x.dn, (1135, 250), '%d' % (x.sbars - x.sbar), 'black', 130, c(CREAM, 0.6 + 0.3 * x.pulse))
    text(x.dn, (1135, 340), 'BARS LEFT', 'din', 30, c(AMBER))
    if x.sbar == x.sbars - 1:                        # the fill: count the soloist in
        x.wash = c(ROSE, 0.18 * (x.inbar / 4) ** 2)
        text(x.dn, (1135, 430), 'SOLO  %d' % (4 - int(x.inbar)), 'black', 46, c(AMBER, 0.5 + 0.5 * x.pulse))

# -- the vibraphone, from above
def scene_solo(x):
    sky(x, (8, 6, 22), (36, 26, 72), (8, 6, 22))
    spotlights(x, n=5, a=0.12)
    for i in range(1, 22):
        x.dn.line([(W * i / 22, 100), (W * i / 22, 460)], fill=c(INDIGO, 0.22))
    piano_roll(x, VIBES, lo=60, hi=96, ytop=110, ykey=460, ybot=560)
    for i in range(22):                              # resonator tubes
        px = (i + 0.5) * W / 22
        ln = min(110 - i * 3.2, 50 if px < 410 else 200)
        x.dn.line([(px, 568), (px, 568 + ln)], fill=c(AMBER, 0.3 + 0.5 * x.vib), width=10)
    text(x.dn, (W / 2, 56), 'VIBES SOLO', 'black', 44, c(TEAL, 0.7 + 0.3 * x.kick))

# -- the open section
def scene_open(x):
    sky(x, (10, 8, 28), (56, 30, 90), (10, 8, 28))
    spotlights(x, n=6, a=0.16)
    cx, cy, r0 = W / 2, 372, 156
    n = 72
    step = int(x.beat * 4)
    for i in range(n):
        v = hsh(i % 36, step) * 0.5 + hsh(i % 36, step - 1) * 0.5
        v *= 0.3 + 0.7 * max(x.kick * (1 - (i % 36) / 36), x.hat * ((i % 36) / 36), x.bassenv * 0.6)
        ang = x.t * 0.2 + i * 2 * math.pi / n
        r1 = r0 + 12 + v * 140
        x.dn.line([(cx + math.cos(ang) * r0, cy + math.sin(ang) * r0), (cx + math.cos(ang) * r1, cy + math.sin(ang) * r1)],
                  fill=mix(TEAL, ROSE, (i % 36) / 36), width=6)
    vinyl(x, cx, cy, r0 - 8, TURN * x.t)
    left = x.sbars - x.sbar
    text(x.dn, (1085, 320), str(left), 'black', 190, c(CREAM, 0.75 + 0.25 * x.pulse))
    text(x.dn, (1085, 440), 'BARS LEFT', 'din', 36, c(AMBER))
    if left == 1:
        text(x.dn, (1085, 500), '下一段：主题  %d' % (4 - int(x.inbar)), 'cn', 28, c(ROSE, 0.5 + 0.5 * x.pulse))
    text(x.dn, (190, 210), 'OPEN', 'black', 84, c(mix(TEAL, ROSE, 0.5 + 0.5 * math.sin(x.t * 2)), 0.9))
    text(x.dn, (190, 300), 'SOLO', 'black', 84, c(mix(ROSE, TEAL, 0.5 + 0.5 * math.sin(x.t * 2)), 0.9))
    text(x.dn, (190, 380), '开放段 · 现场即兴', 'cn', 30, c(AMBER, 0.9))
    text(x.dn, (190, 424), 'F dorian / F blues', 'mono', 20, c(CREAM, 0.7))

# -- the record runs out
def scene_outro(x):
    k = x.t - LAST * BARLEN
    if k < 0:
        turntable(x, TURN * x.t, 1.0)
        title_column(x)
        return
    turntable(x, TURN * (LAST * BARLEN + k - k * k / (2 * BARLEN)), 1 - (k - BEAT) / (BEAT * 2))   # slowing to a stop, the arm goes home
    rings(x, 760, 392, [LAST * BARLEN], WHITE, k=1.6, rmax=700, width=5)
    text(x.dn, (215, 250), 'Fm9', 'black', 104, c(CREAM))
    text(x.dn, (215, 350), 'SLOW SPIN', 'black', 44, mix(AMBER, ROSE, 0.5 + 0.5 * math.sin(x.t * 1.5)))
    text(x.dn, (215, 410), '午 夜 慢 转', 'cn', 38, c(AMBER))

SCENES = {'intro': scene_intro, 'a1': scene_a1, 'a2': scene_a2, 'b1': scene_b1, 'a3': scene_a3, 'break': scene_break,
          'solo': scene_solo, 'open': scene_open, 'a4': scene_a4, 'outro': scene_outro}

# ------------------------------------------------------------------ the players' strip (what the band reads on stage)
def hud(x):
    d = x.dn
    # section name and where we are in it
    if x.sid != 'intro' or x.st > BARLEN:
        d.rounded_rectangle([24, 22, 360, 88], 12, fill=(0, 0, 0), outline=c(TEAL, 0.6), width=2)
        x.db.rounded_rectangle([24, 22, 360, 88], 12, fill=(8, 4, 24))
        text(d, (40, 44), x.name, 'cn', 24, WHITE, anchor='lm')
        text(d, (346, 44), '%d / %d' % (x.sbar + 1, x.sbars), 'mono', 20, c(AMBER), anchor='rm')
        for i in range(x.sbars):
            bx = 40 + i * (306 / x.sbars)
            wdt = 306 / x.sbars - 4
            col = c(AMBER) if i == x.sbar else (c(TEAL, 0.8) if i < x.sbar else c(TEAL, 0.2))
            d.rectangle([bx, 66, bx + wdt, 74], fill=col)
            if i == x.sbar:
                d.rectangle([bx, 66, bx + wdt * x.inbar / 4, 74], fill=WHITE)
    # chord now and next
    if x.sid == 'intro' and x.sbar == 0: return
    seq = []
    for b in range(x.bar, min(NBARS, x.bar + 3)):
        for beat, name in CHORDS[b]:
            seq.append(((b - x.bar) * 4 + beat, name))
    cur = [s for s in seq if s[0] <= x.inbar][-1]
    nxt = [s for s in seq if s[0] > x.inbar and s[1] != cur[1]]
    d.rounded_rectangle([24, H - 96, 400, H - 22], 12, fill=(0, 0, 0), outline=c(ROSE, 0.7), width=2)
    x.db.rounded_rectangle([24, H - 96, 400, H - 22], 12, fill=(8, 4, 24))
    text(d, (44, H - 58), cur[1], 'black', 40, WHITE, anchor='lm')
    if nxt:
        left = nxt[0][0] - x.inbar
        text(d, (380, H - 70), '→ ' + nxt[0][1], 'black', 22, c(TEAL, 0.6 + 0.4 * (left < 1)), anchor='rm')
        text(d, (380, H - 42), '%d 拍后' % math.ceil(left), 'cn', 16, c(AMBER, 0.9), anchor='rm')
    # beat lamps
    for i in range(4):
        on = int(x.inbar) == i
        px = W - 150 + i * 30
        r = 9 if on else 6
        d.ellipse([px - r, 54 - r, px + r, 54 + r], fill=c(AMBER if i == 0 else TEAL, 1.0 if on else 0.25))
    text(d, (W - 105, 30), '小节 %d' % (x.bar + 1), 'cn', 18, c(WHITE, 0.8))

def title_card(x):
    """Name of the section, for its first bar."""
    if x.sid == 'intro' or x.st > BARLEN * 1.2: return
    a = smooth(x.st / 0.25) * (1 - smooth((x.st - BARLEN * 0.8) / (BARLEN * 0.4)))
    slide = (1 - smooth(x.st / 0.3)) * 80
    y = H * 0.36
    x.dn.rectangle([0, y - 48, W, y + 48], fill=c(ROSE, 0.16 * a))
    text(x.dn, (W / 2 - slide, y), x.name, 'cn', 64, c(WHITE, a))

# ------------------------------------------------------------------ compose
_scan = None
def scanlines():
    global _scan
    if _scan is None:
        _scan = Image.new('L', (W, H), 255)
        d = ImageDraw.Draw(_scan)
        for y in range(0, H, 3):
            d.line([(0, y), (W, y)], fill=222)
        _scan = Image.merge('RGB', (_scan, _scan, _scan))
    return _scan

def frame(i):
    t = i / FPS
    x = Ctx(t)
    SCENES[x.sid](x)
    title_card(x)
    hud(x)
    out = ImageChops.screen(x.base, x.neon)
    bloom = x.neon.resize((W // 4, H // 4), Image.BILINEAR).filter(ImageFilter.GaussianBlur(5)).resize((W, H), Image.BILINEAR)
    out = ImageChops.add(out, bloom.point(lambda v: int(v * 0.85)))
    if x.wash:
        out = ImageChops.add(out, Image.new('RGB', (W, H), x.wash))
    flash = 0.5 * x.crash ** 2 if x.sid != 'intro' else 0.0
    if flash > 0.02:
        out = ImageChops.add(out, Image.new('RGB', (W, H), c(WHITE, flash)))
    if SCANLINES:
        out = ImageChops.multiply(out, scanlines())
    fade = min(1.0, t / 0.6, (DUR - t) / 1.6)
    if fade < 1.0:
        out = out.point(lambda v: int(v * max(0.0, fade)))
    return out

def frame_bytes(i):
    return frame(i).tobytes()

def main():
    if len(sys.argv) >= 4 and sys.argv[1] == '--still':
        frame(int(float(sys.argv[2]) * FPS)).save(sys.argv[3])
        return
    out = os.path.join(SRC, 'visuals', 'slow-spin.mp4')
    first, n = 0, int(DUR * FPS)
    if len(sys.argv) >= 5 and sys.argv[1] == '--clip':        # --clip <from s> <to s> <out.mp4>: a preview
        first, n, out = int(float(sys.argv[2]) * FPS), int(float(sys.argv[3]) * FPS), sys.argv[4]
    os.makedirs(os.path.dirname(out), exist_ok=True)
    ff = subprocess.Popen(
        ['ffmpeg', '-loglevel', 'error', '-y', '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-s', f'{W}x{H}', '-r', str(FPS), '-i', '-',
         '-an', '-c:v', 'libx264', '-preset', 'veryslow', '-tune', 'animation', '-crf', '29', '-pix_fmt', 'yuv420p', '-g', str(2 * FPS), '-movflags', '+faststart', out],
        stdin=subprocess.PIPE)
    with Pool() as pool:
        for k, b in enumerate(pool.imap(frame_bytes, range(first, n), chunksize=8)):
            ff.stdin.write(b)
            if k % 300 == 0:
                print(f'{k}/{n}', flush=True)
    ff.stdin.close()
    if ff.wait() != 0:
        sys.exit('ffmpeg failed')
    print(out, f'{os.path.getsize(out) / 1e6:.1f} MB')

if __name__ == '__main__':
    main()

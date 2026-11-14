#!/usr/bin/env python3
"""Renders neon-strut.src/visuals/neon-strut.mp4 — the stage video for 《霓虹快步 Neon Strut》.

    python3 songs/neon-strut/make_song.py      # writes form.json and song.json first
    python3 songs/neon-strut/make_video.py     # ~175 s of 1280x720 at 30 fps
    python3 songs/neon-strut/make_video.py --still 42.5 out.png   # one frame, for checking
    python3 songs/neon-strut/make_video.py --clip 40 70 out.mp4   # a stretch, for checking motion

The picture is a pure function of time: every frame is drawn from the song's own notes
(song.json) and form (form.json), so it stays on the bar when the file is regenerated.
Needs Pillow and ffmpeg. No audio track is written (FORMAT.md §12.1).
"""
import bisect, json, math, os, subprocess, sys
from multiprocessing import Pool
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, 'neon-strut.src')
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
HITS, LAST = form['hitsBar'], form['lastBar']

BG = (11, 6, 32)
MAG = (255, 46, 136)
CYAN = (25, 227, 255)
YEL = (255, 210, 63)
PUR = (122, 31, 214)
WHITE = (255, 255, 255)
METAL = (38, 30, 74)
PAL = [MAG, CYAN, YEL, PUR]

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
ALLDR = sorted(n[0] for n in DR if n[3] > 0.3)
BRASS = sorted({round(n[0], 3) for n in track_notes('brass')})
CLAV = sorted({round(n[0], 3) for n in track_notes('clav') if n[3] > 0.5})
KEYS = sorted({round(n[0], 3) for n in track_notes('keys')})
BASS = track_notes('bass')
BASS_T = [n[0] for n in BASS]
LEAD = track_notes('lead')
ORGAN = track_notes('organ')

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
        self.brass = env(BRASS, t, 6)
        self.clav = env(CLAV, t, 14)
        self.keys = env(KEYS, t, 5)
        self.bassenv = env(BASS_T, t, 8)
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

def sky(x, top=(11, 6, 32), mid=(52, 18, 100), bottom=(11, 6, 32)):
    x.base.paste(gradient(top, mid, bottom))

def stars(x, n=90, ymax=330):
    for i in range(n):
        px, py = hsh(i, 1) * W, hsh(i, 2) * ymax
        tw = 0.35 + 0.65 * abs(math.sin(x.t * (0.6 + hsh(i, 3) * 2) + i))
        x.dn.point((px, py), fill=c(WHITE, tw * 0.8))
        if hsh(i, 4) > 0.86:
            x.dn.line([(px - 3, py), (px + 3, py)], fill=c(CYAN, tw * 0.5))
            x.dn.line([(px, py - 3), (px, py + 3)], fill=c(CYAN, tw * 0.5))

def sun(x, cx, cy, r, glow=1.0):
    rr = r * (1 + 0.05 * x.kick)
    x.dn.ellipse([cx - rr - 14, cy - rr - 14, cx + rr + 14, cy + rr + 14], outline=c(MAG, 0.35 * glow), width=3)
    shift = frac(x.t * 0.25) * 34
    y = -rr
    while y < rr:
        half = math.sqrt(max(0.0, rr * rr - y * y))
        k = (y + rr) / (2 * rr)
        cut = k > 0.52 and frac((y - shift) / 34) < 0.14 + 0.22 * (k - 0.52) / 0.48
        if not cut:
            col = mix(YEL, MAG, k * 1.6) if k < 0.62 else mix(MAG, PUR, (k - 0.62) / 0.38)
            x.db.line([(cx - half, cy + y), (cx + half, cy + y)], fill=c(col, glow), width=3)
        y += 3

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
        x.db.rectangle([px, top, px + w, ground], fill=(14, 8, 40))
        x.dn.rectangle([px, top, px + w, ground], fill=(0, 0, 0))
        x.dn.line([(px, top), (px + w, top)], fill=c(PAL[i % 3], 0.5 * lit), width=2)
        if hsh(i, 14) > 0.7:                       # antenna
            x.dn.line([(px + w / 2, top), (px + w / 2, top - 26)], fill=c(MAG, 0.5 * lit))
            if int(x.beat) % 2 == i % 2:
                x.dn.ellipse([px + w / 2 - 3, top - 30, px + w / 2 + 3, top - 24], fill=c(MAG, lit))
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

def grid_floor(x, horizon, col_a=MAG, col_b=CYAN, speed=1.0, bright=1.0):
    x.db.rectangle([0, horizon, W, H], fill=(9, 4, 26))
    x.dn.rectangle([0, horizon, W, H], fill=(0, 0, 0))
    cx = W / 2
    for i in range(-16, 17):
        x.dn.line([(cx + i * 36, horizon), (cx + i * 260, H)], fill=c(col_a, 0.5 * bright), width=2)
    n = 12
    ph = frac(x.beat * 0.5 * speed)
    for k in range(n + 1):
        z = (k + ph) / n
        y = horizon + (H - horizon) * z ** 2.2
        x.dn.line([(0, y), (W, y)], fill=c(col_b, (0.2 + 0.6 * z) * bright), width=2)
    x.dn.line([(0, horizon), (W, horizon)], fill=c(col_b, bright), width=3)

def rain(x, n=110, a=0.35):
    for i in range(n):
        px = hsh(i, 21) * (W + 200) - 100
        sp = 600 + hsh(i, 22) * 500
        py = frac(hsh(i, 23) + x.t * sp / H) * (H + 80) - 40
        x.dn.line([(px, py), (px - 8, py + 26)], fill=c(CYAN, a * (0.4 + 0.6 * hsh(i, 24))))

def eq_bars(x, x0, x1, base, hmax, n=32, col=CYAN, a=0.9, seedk=0):
    bw = (x1 - x0) / n
    step = int(x.beat * 4)
    for i in range(n):
        target = hsh(i, step, seedk)
        prev = hsh(i, step - 1, seedk)
        v = prev + (target - prev) * smooth(frac(x.beat * 4))
        lo = 1 - i / n
        v = v * (0.35 + 0.65 * (lo * x.kick + (1 - lo) * max(x.hat, x.clav) + 0.4 * x.bassenv))
        h = 6 + v * hmax
        colr = mix(col, MAG, i / n)
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

def confetti(x, n=160, a=0.9):
    for i in range(n):
        life = 2.2 + hsh(i, 31) * 2
        ph = frac(x.t / life + hsh(i, 32))
        px = hsh(i, 33, int(x.t / life + hsh(i, 32))) * W + math.sin(ph * 9 + i) * 30
        py = -20 + ph * (H + 40)
        s = 4 + hsh(i, 34) * 6
        rot = x.t * (3 + hsh(i, 35) * 5) + i
        dx, dy = math.cos(rot) * s, math.sin(rot) * s * 0.5
        x.dn.polygon([(px - dx, py - dy), (px + dy, py - dx * 0.4), (px + dx, py + dy), (px - dy, py + dx * 0.4)],
                     fill=c(PAL[i % 4], a * (1 - 0.6 * ph)))

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

def robot(x, px, py, s, col, role, phase=0.0, energy=1.0):
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
    up = x.brass if role == 'horn' else 0.0
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
    x.dn.ellipse([ant[0] + tilt * 0.6 - r, ant[1] - 20 * s - r, ant[0] + tilt * 0.6 + r, ant[1] - 20 * s + r], fill=c(YEL))
    # instrument and near arm
    if role == 'bass':
        body = (hx + 34 * s, hip_y - 14 * s)
        head = (hx - 96 * s, sh_y - 10 * s)
        x.dn.line([body, head], fill=c(YEL), width=int(7 * s))
        rbox(x, [body[0] - 30 * s, body[1] - 24 * s, body[0] + 34 * s, body[1] + 26 * s], int(20 * s), YEL, fill=(60, 30, 70))
        for i in range(2):
            o = (i * 5 - 2) * s + (strum if i == 0 else -strum) * 0.25
            x.dn.line([(body[0], body[1] + o), (head[0], head[1] + o * 0.3)], fill=c(WHITE, 0.5 + 0.5 * x.bassenv))
        arm(1, (hx + 58 * s, sh_y + 44 * s), (body[0] + 6 * s, body[1] - 2 * s + strum))
    elif role == 'keys':
        ky = sh_y + 84 * s
        rbox(x, [hx - 108 * s, ky, hx + 108 * s, ky + 26 * s], int(6 * s), CYAN, fill=(20, 16, 50))
        for i in range(14):
            kx = hx - 100 * s + i * 14.4 * s
            lit = hsh(i, count(KEYS, x.t)) > 0.7
            x.dn.rectangle([kx, ky + 5 * s, kx + 11 * s, ky + 20 * s], fill=c(YEL if lit else WHITE, (0.5 + 0.5 * x.keys) if lit else 0.3))
        for side in (-1, 1):
            limb(x, (hx + side * 90 * s, ky + 26 * s), (hx + side * 70 * s, py), int(6 * s), CYAN)
        arm(1, (hx + 56 * s, sh_y + 44 * s), (hx + 44 * s, sh_y + 78 * s + x.keys * 10 * s * (1 if int(x.beat * 2) % 2 else 0.3)))
    elif role == 'horn':
        mouth = (hx + tilt - 30 * s, hy - 12 * s)
        bell = (mouth[0] - 96 * s, mouth[1] - (10 + up * 46) * s)
        x.dn.line([mouth, bell], fill=c(YEL), width=int(8 * s))
        dx, dy = bell[0] - mouth[0], bell[1] - mouth[1]
        ln = math.hypot(dx, dy)
        nx, ny = -dy / ln, dx / ln
        fl = 24 * s * (1 + 0.5 * up)
        x.dn.polygon([(bell[0] + dx / ln * 26 * s + nx * fl, bell[1] + dy / ln * 26 * s + ny * fl),
                      (bell[0] + dx / ln * 26 * s - nx * fl, bell[1] + dy / ln * 26 * s - ny * fl),
                      (bell[0] + nx * 6 * s, bell[1] + ny * 6 * s), (bell[0] - nx * 6 * s, bell[1] - ny * 6 * s)], fill=c(YEL))
        if up > 0.08:                               # the blast
            for k in range(3):
                rr = (30 + k * 26 + (1 - up) * 60) * s
                ctr = (bell[0] + dx / ln * 40 * s, bell[1] + dy / ln * 40 * s)
                x.dn.arc([ctr[0] - rr, ctr[1] - rr, ctr[0] + rr, ctr[1] + rr], 130, 230, fill=c(YEL, up), width=3)
        arm(1, (hx + 20 * s, sh_y + 44 * s - up * 16 * s), (mouth[0] - 46 * s, mouth[1] - (4 + up * 24) * s))
    else:
        a = -math.sin(b * math.pi) * energy
        arm(1, (hx + 66 * s, sh_y + 6 * s - a * 26 * s), (hx + 84 * s, sh_y - 44 * s - a * 34 * s))

def band(x, ground, energy=1.0, roles=('bass', 'keys', 'horn'), s=1.25):
    spots = {'bass': (W * 0.2, MAG, 0.0), 'keys': (W * 0.5, CYAN, 0.5), 'horn': (W * 0.8, YEL, 1.0), 'dance': (W * 0.5, CYAN, 0.25)}
    for r in roles:
        px, col, ph = spots[r]
        robot(x, px, ground, s, col, r, ph, energy)

# ------------------------------------------------------------------ scenes
def scene_intro(x):
    sky(x, (4, 2, 14), (24, 8, 54), (4, 2, 14))
    rise = smooth((x.st - BARLEN * 0.5) / (BARLEN * 2.5))
    stars(x)
    skyline(x, 500, rise=rise, lit=rise)
    grid_floor(x, 500, bright=0.25 + 0.75 * rise)
    rain(x)
    # the sign switches on letter by letter with the drum fill, then flickers until the band is in
    title = 'NEON STRUT'
    lit = min(len(title), count(ALLDR, x.t) + (1 if x.t > BARLEN else 0) * len(title))
    f = F('black', 132)
    total = sum(f.getlength(ch) for ch in title)
    px = W / 2 - total / 2
    for i, ch in enumerate(title):
        w = f.getlength(ch)
        if i < lit:
            fl = 1.0 if x.st > BARLEN * 2 else (0.35 if hsh(i, int(x.t * 18)) > 0.82 else 1.0)
            col = mix(CYAN, MAG, i / (len(title) - 1))
            x.dn.text((px, 190), ch, font=f, fill=c(col, fl * (0.8 + 0.2 * x.clav)), anchor='ls')
        else:
            x.dn.text((px, 190), ch, font=f, fill=(18, 14, 40), anchor='ls')
        px += w
    if x.st > BARLEN:
        a = smooth((x.st - BARLEN) / BEAT)
        text(x.dn, (W / 2, 250), '霓 虹 快 步', 'cn', 54, c(YEL, a))
        eq_bars(x, 240, 1040, 498, 150, n=40, a=0.8 * a)
    if x.sbar == 3:
        n = 4 - int(x.inbar)
        text(x.dn, (W / 2, 610), str(n), 'black', 130, c(WHITE, 0.4 + 0.6 * x.pulse))

def scene_street(x, roles, level=1.0):
    sky(x)
    stars(x)
    sun(x, W / 2, 330, 190)
    skyline(x, 470)
    grid_floor(x, 470, speed=1.0)
    eq_bars(x, 0, W, 468, 70, n=64, a=0.5)
    band(x, 668, energy=level, roles=roles)
    rings(x, W * 0.8, 380, BRASS, YEL, rmax=260, width=3)

def scene_a1(x): scene_street(x, ('bass',) if x.sbar < 4 else ('bass', 'keys'))
def scene_a2(x): scene_street(x, ('bass', 'keys', 'horn'))

def disco_ball(x, cx, cy, r):
    x.dn.line([(cx, 0), (cx, cy - r)], fill=c(WHITE, 0.5), width=2)
    x.db.ellipse([cx - r, cy - r, cx + r, cy + r], fill=(30, 26, 60))
    x.dn.ellipse([cx - r, cy - r, cx + r, cy + r], fill=(0, 0, 0), outline=c(WHITE, 0.6), width=2)
    rot = x.t * 0.9
    step = int(x.beat * 2)
    nlat, nlon = 11, 26
    for i in range(nlat):
        phi = (i + 0.5) / nlat * math.pi - math.pi / 2
        yy = cy + r * math.sin(phi)
        rw = r * math.cos(phi)
        fh = r * math.pi / nlat * abs(math.cos(phi)) * 0.42 + 2
        for j in range(nlon):
            th = j / nlon * 2 * math.pi + rot
            z = math.cos(th)
            if z <= 0.05: continue
            xx = cx + rw * math.sin(th)
            fw = rw * 2 * math.pi / nlon * z * 0.42
            v = hsh(i, j, step)
            col = PAL[int(hsh(i, j) * 4)] if v > 0.72 else WHITE
            a = (0.25 + 0.75 * v) * (0.4 + 0.6 * z)
            x.dn.rectangle([xx - fw, yy - fh, xx + fw, yy + fh], fill=c(col, a))

def light_rays(x, cx, cy, n=14, a=0.16):
    rot = x.t * 0.35
    for i in range(n):
        ang = rot + i * 2 * math.pi / n
        wdt = 0.07
        p1 = (cx + math.cos(ang - wdt) * 1500, cy + math.sin(ang - wdt) * 1500)
        p2 = (cx + math.cos(ang + wdt) * 1500, cy + math.sin(ang + wdt) * 1500)
        x.dn.polygon([(cx, cy), p1, p2], fill=c(PAL[i % 4], a * (0.6 + 0.4 * x.kick)))

def dance_floor(x, horizon):
    x.db.rectangle([0, horizon, W, H], fill=(8, 4, 22))
    x.dn.rectangle([0, horizon, W, H], fill=(0, 0, 0))
    rows, cols = 7, 9
    step = int(x.beat)
    cx = W / 2
    def row_y(k): return horizon + (H - horizon) * (k / rows) ** 1.6
    def sc(k): return 0.42 + 0.9 * (k / rows) ** 1.6
    tw = 150
    for k in range(rows):
        y0, y1, s0, s1 = row_y(k), row_y(k + 1), sc(k), sc(k + 1)
        for j in range(-cols, cols):
            v = hsh(j, k, step)
            wave = (j + k + step) % 4 == 0
            if v > 0.55 or wave:
                col = PAL[(j + k * 2 + step) % 4]
                a = (0.25 + 0.55 * x.kick) if wave else 0.14 + 0.2 * v
                quad = [(cx + j * tw * s0 + 3, y0 + 2), (cx + (j + 1) * tw * s0 - 3, y0 + 2),
                        (cx + (j + 1) * tw * s1 - 3, y1 - 2), (cx + j * tw * s1 + 3, y1 - 2)]
                x.dn.polygon(quad, fill=c(col, a))
    x.dn.line([(0, horizon), (W, horizon)], fill=c(MAG, 0.9), width=3)

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
        col = YEL if on else (c(CYAN, 0.35) if past else CYAN)
        x.dn.rounded_rectangle([xa, yy - 7, max(xa + 6, xb), yy + 7], 6, fill=col)
        if on:
            x.dn.ellipse([cx - 16, yy - 16, cx + 16, yy + 16], outline=WHITE, width=3)
            pts.append(yy)
    x.dn.line([(cx, y0 - 16), (cx, y1 + 16)], fill=c(WHITE, 0.35), width=1)

def scene_chorus(x, more=False):
    sky(x, (16, 4, 40), (70, 14, 110), (11, 6, 32))
    light_rays(x, W / 2, 150, a=0.2 if more else 0.15)
    dance_floor(x, 480)
    disco_ball(x, W / 2, 150, 92 * (1 + 0.04 * x.kick))
    melody_ribbon(x, LEAD, 290, 440)
    robot(x, W * 0.14, 676, 1.15, MAG, 'bass', 0.0)
    robot(x, W * 0.86, 676, 1.15, YEL, 'horn', 1.0)
    robot(x, W * 0.5, 690, 1.05, CYAN, 'dance', 0.25)
    if more:
        confetti(x)
        rings(x, W / 2, 150, SNARES, WHITE, k=4, rmax=700, width=2)

def scene_b1(x): scene_chorus(x)
def scene_b2(x): scene_chorus(x, more=True)

def scene_tunnel(x):
    cx, cy = W / 2, H / 2 - 20
    n = 14
    ph = frac(x.beat / 2)
    for k in range(n, -1, -1):
        d = k + ph
        r = 26 * 1.42 ** d
        if r > 1100: continue
        rot = d * 0.22 + x.t * 0.25 + x.kick * 0.05
        col = PAL[(k - int(x.beat / 2)) % 3]
        a = min(1.0, 0.25 + d / n) * (0.55 + 0.45 * x.kick)
        pts = [(cx + math.cos(rot + i * math.pi / 3) * r, cy + math.sin(rot + i * math.pi / 3) * r * 0.78) for i in range(6)]
        x.dn.polygon(pts, outline=c(col, a), width=4 if d > 4 else 2)
        if k % 2 == 0:
            for p in pts:
                x.dn.ellipse([p[0] - 4, p[1] - 4, p[0] + 4, p[1] + 4], fill=c(WHITE, a))
    for i in range(6):
        ang = x.t * 0.25 + i * math.pi / 3
        x.dn.line([(cx, cy), (cx + math.cos(ang) * 1200, cy + math.sin(ang) * 900)], fill=c(PUR, 0.5), width=1)
    rings(x, cx, cy, BRASS, YEL, k=3, rmax=640, width=5)
    b = x.brass
    if b > 0.05:
        text(x.dn, (cx, cy), 'Bb', 'black', int(150 + 60 * b), c(YEL, b))
    band(x, 690, roles=('bass', 'horn'), s=1.05)

def speaker(x, cx, cy, s, col):
    rbox(x, [cx - 150 * s, cy - 230 * s, cx + 150 * s, cy + 230 * s], int(18 * s), col, fill=(16, 12, 40))
    for (oy, r0) in ((-120, 62), (70, 112)):
        push = 1 + 0.16 * x.kick * (1 if r0 > 80 else 0.5) + 0.08 * x.pulse
        for k, rr in enumerate((1.0, 0.72, 0.45, 0.2)):
            r = r0 * s * rr * (push if k else 1.0)
            x.dn.ellipse([cx - r, cy + oy * s - r, cx + r, cy + oy * s + r], outline=c(col if k % 2 == 0 else WHITE, 0.9 - 0.15 * k),
                         width=3, fill=c(col, 0.5 * x.kick) if k == 3 else None)

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

def scene_break(x):
    build = smooth((x.sbar + x.inbar / 4 - 4) / 4)
    x.base.paste(gradient((4, 2, 12), mix((14, 4, 34), (90, 16, 90), build), (4, 2, 12)))
    speaker(x, 190, 370, 1.0, MAG)
    speaker(x, W - 190, 370, 1.0, CYAN)
    scope(x, 380, W - 380, 300, 90, c(YEL, 0.9))
    cow = env([b * BEAT for b in range(int(x.beat) + 1)], x.t, 7)
    text(x.dn, (W / 2, 130), 'BREAK', 'black', 110, c(WHITE, 0.25 + 0.6 * cow))
    # four on the floor: one block per beat of the bar
    for i in range(4):
        on = int(x.inbar) == i
        bx = W / 2 - 230 + i * 120
        x.dn.rounded_rectangle([bx, 440, bx + 100, 540], 14, outline=c(MAG, 0.9), width=3,
                               fill=c(MAG, 0.25 + 0.75 * x.pulse) if on else None)
    if x.sbar >= 4:
        eq_bars(x, 380, W - 380, 690, 100 + 120 * build, n=30, a=0.5 + 0.5 * build)
    if x.sbar == 7:                                   # the roll: strobe and a rising wall
        s16 = int(x.inbar * 4)
        if s16 % 2 == 0:
            x.dn.rectangle([0, 0, W, H], outline=c(WHITE, 0.9), width=10 + s16)
        x.wash = c(MAG, 0.22 * (x.inbar / 4) ** 2)
        text(x.dn, (W / 2, 620), 'SOLO  %d' % (4 - int(x.inbar)), 'black', 70, c(YEL, 0.5 + 0.5 * x.pulse))

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
        col = YEL if on else PAL[n[2] % 2]
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
        if on: x.dn.rectangle([xa + 1, ykey, xb - 1, ybot], fill=c(YEL, 0.6))
        else: x.dn.rectangle([xa + 1, ykey, xb - 1, ybot], fill=(0, 0, 0))
    for p in range(lo, hi + 1):
        if p in whites: continue
        xa, xb = xpos[p]
        on = p in active
        x.db.rectangle([xa, ykey, xb, ykey + 68], fill=(20, 14, 44))
        x.dn.rectangle([xa, ykey, xb, ykey + 68], fill=c(MAG, 0.9) if on else (0, 0, 0), outline=c(MAG, 0.5))
    x.dn.line([(0, ykey), (W, ykey)], fill=c(CYAN), width=3)

def scene_solo(x):
    sky(x, (6, 2, 20), (40, 10, 84), (6, 2, 20))
    spotlights(x, n=5, a=0.14)
    for i in range(1, 12):                         # lane guides
        x.dn.line([(W * i / 12, 100), (W * i / 12, 500)], fill=c(PUR, 0.25))
    piano_roll(x, ORGAN)
    text(x.dn, (W / 2, 56), 'ORGAN SOLO', 'black', 44, c(CYAN, 0.7 + 0.3 * x.kick))

def scene_open(x):
    sky(x, (10, 3, 30), (60, 12, 96), (10, 3, 30))
    spotlights(x, n=6, a=0.2)
    cx, cy = W / 2, 330
    n = 72
    step = int(x.beat * 4)
    rot = x.t * 0.2
    r0 = 150 + 14 * x.kick
    for i in range(n):
        v = hsh(i % 36, step) * 0.5 + hsh(i % 36, step - 1) * 0.5
        v *= 0.3 + 0.7 * max(x.kick * (1 - (i % 36) / 36), x.hat * ((i % 36) / 36), x.bassenv * 0.6)
        ang = rot + i * 2 * math.pi / n
        r1 = r0 + 12 + v * 170
        col = mix(CYAN, MAG, (i % 36) / 36)
        x.dn.line([(cx + math.cos(ang) * r0, cy + math.sin(ang) * r0), (cx + math.cos(ang) * r1, cy + math.sin(ang) * r1)],
                  fill=col, width=6)
    x.dn.ellipse([cx - r0 + 8, cy - r0 + 8, cx + r0 - 8, cy + r0 - 8], outline=c(YEL, 0.8), width=4)
    left = x.sbars - x.sbar
    text(x.dn, (cx, cy - 14), str(left), 'black', 150, c(WHITE, 0.75 + 0.25 * x.pulse))
    text(x.dn, (cx, cy + 82), 'BARS LEFT', 'din', 34, c(YEL))
    text(x.dn, (W / 2, 70), 'OPEN SOLO', 'black', 84, c(mix(CYAN, MAG, 0.5 + 0.5 * math.sin(x.t * 2)), 0.9))
    text(x.dn, (W / 2, 132), '开放段 · 现场即兴', 'cn', 34, c(YEL, 0.9))
    robot(x, W * 0.13, 690, 1.1, MAG, 'bass', 0.0)
    robot(x, W * 0.87, 690, 1.1, CYAN, 'keys', 0.5)
    rings(x, W * 0.87, 380, BRASS, YEL, rmax=240, width=3)

def scene_a3(x): scene_tunnel(x)

def scene_outro(x):
    if x.bar == LAST:
        sky(x, (6, 2, 20), (60, 14, 100), (6, 2, 20))
        k = x.t - LAST * BARLEN
        sun(x, W / 2, 300, 200 + 20 * math.exp(-k * 2), glow=1.0)
        grid_floor(x, 470, speed=0.4)
        rings(x, W / 2, 300, [LAST * BARLEN], WHITE, k=1.6, rmax=900, width=6)
        text(x.dn, (W / 2, 300), 'Bb', 'black', 230, (0, 0, 0))
        text(x.db, (W / 2, 300), 'Bb', 'black', 230, BG)
        text(x.dn, (W / 2, 560), 'NEON STRUT', 'black', 92, mix(CYAN, MAG, 0.5 + 0.5 * math.sin(x.t * 1.5)))
        text(x.dn, (W / 2, 640), '霓 虹 快 步', 'cn', 44, YEL)
        return
    scene_street(x, ('bass', 'keys', 'horn'))
    confetti(x, n=90, a=0.7)
    if x.bar == HITS:
        hits = [HITS * BARLEN + s * BEAT / 4 for s in (0, 3, 6, 10, 12, 14)]
        e = env(hits, x.t, 9)
        i = count(hits, x.t)
        if e > 0.04:
            x.wash = c(PAL[i % 3], 0.3 * e)
            text(x.dn, (W / 2, 300), 'Bb', 'black', int(260 + 120 * e), c(WHITE, e))

SCENES = {'intro': scene_intro, 'a1': scene_a1, 'a2': scene_a2, 'b1': scene_b1, 'a3': scene_a3, 'break': scene_break,
          'solo': scene_solo, 'open': scene_open, 'b2': scene_b2, 'outro': scene_outro}

# ------------------------------------------------------------------ the players' strip (what the band reads on stage)
def hud(x):
    d = x.dn
    # section name and where we are in it
    if x.sid != 'intro' or x.st > BARLEN:
        d.rounded_rectangle([24, 22, 360, 88], 12, fill=(0, 0, 0), outline=c(CYAN, 0.6), width=2)
        x.db.rounded_rectangle([24, 22, 360, 88], 12, fill=(8, 4, 24))
        text(d, (40, 44), x.name, 'cn', 24, WHITE, anchor='lm')
        text(d, (346, 44), '%d / %d' % (x.sbar + 1, x.sbars), 'mono', 20, c(YEL), anchor='rm')
        for i in range(x.sbars):
            bx = 40 + i * (306 / x.sbars)
            wdt = 306 / x.sbars - 4
            col = c(YEL) if i == x.sbar else (c(CYAN, 0.8) if i < x.sbar else c(CYAN, 0.2))
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
    d.rounded_rectangle([24, H - 96, 400, H - 22], 12, fill=(0, 0, 0), outline=c(MAG, 0.7), width=2)
    x.db.rounded_rectangle([24, H - 96, 400, H - 22], 12, fill=(8, 4, 24))
    text(d, (44, H - 58), cur[1], 'black', 40, WHITE, anchor='lm')
    if nxt:
        left = nxt[0][0] - x.inbar
        text(d, (380, H - 70), '→ ' + nxt[0][1], 'black', 22, c(CYAN, 0.6 + 0.4 * (left < 1)), anchor='rm')
        text(d, (380, H - 42), '%d 拍后' % math.ceil(left), 'cn', 16, c(YEL, 0.9), anchor='rm')
    # beat lamps
    for i in range(4):
        on = int(x.inbar) == i
        px = W - 150 + i * 30
        r = 9 if on else 6
        d.ellipse([px - r, 54 - r, px + r, 54 + r], fill=c(YEL if i == 0 else CYAN, 1.0 if on else 0.25))
    text(d, (W - 105, 30), '小节 %d' % (x.bar + 1), 'cn', 18, c(WHITE, 0.8))

def title_card(x):
    """Name of the section, for its first bar."""
    if x.sid == 'intro' or x.st > BARLEN * 1.2: return
    a = smooth(x.st / 0.25) * (1 - smooth((x.st - BARLEN * 0.8) / (BARLEN * 0.4)))
    slide = (1 - smooth(x.st / 0.3)) * 80
    y = H * 0.36
    x.dn.rectangle([0, y - 48, W, y + 48], fill=c(MAG, 0.16 * a))
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
    out = os.path.join(SRC, 'visuals', 'neon-strut.mp4')
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

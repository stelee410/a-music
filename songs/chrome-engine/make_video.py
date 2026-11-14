#!/usr/bin/env python3
"""Renders chrome-engine.src/visuals/chrome-engine.mp4 — the stage video for 《铬色引擎 Chrome Engine》.

    python3 songs/chrome-engine/make_song.py                      # writes form.json and song.json first
    .venv/bin/python songs/chrome-engine/make_video.py --prepare  # unpack the clips in media/ to stills (once)
    .venv/bin/python songs/chrome-engine/make_video.py            # ~189 s of 1280x720 at 30 fps
    .venv/bin/python songs/chrome-engine/make_video.py --still 42.5 out.png
    .venv/bin/python songs/chrome-engine/make_video.py --clip 40 70 out.mp4

Two kinds of picture are cut together on the bar lines:
- 3D particle scenes computed here (tunnel, point-cloud city, reactor, an eight-cylinder engine
  firing in sixteenths, vortex, terrain), drawn from the song's own notes;
- pictures and clips made with Grok (media/, prompts in media_prompts.json), which this script
  only plays, zooms and lights in time with the kick. A clip that is missing falls back to its still.

Every frame is a pure function of time. Needs numpy, Pillow and ffmpeg. No audio track is written.
"""
import bisect, json, math, os, subprocess, sys
from multiprocessing import Pool
import numpy as np
from PIL import Image, ImageChops, ImageDraw, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, 'chrome-engine.src')
W, H, FPS = 1280, 720, 60
CLIP_FPS = 60                                  # the clips are interpolated to this by --prepare
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

BG = (5, 6, 12)
MAG = (255, 43, 214)
CYAN = (0, 229, 255)
ORANGE = (255, 122, 26)
VIOLET = (120, 80, 255)
PALE = (220, 235, 255)
WHITE = (255, 255, 255)
METAL = (40, 34, 66)
PAL = [MAG, CYAN, ORANGE, VIOLET]

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
CLAPS = [n[0] for n in DR if n[2] == 39]
HATS = [n[0] for n in DR if n[2] in (42, 46)]
CRASH = [n[0] for n in DR if n[2] == 49]
RIMS = [n[0] for n in DR if n[2] == 37]
def onsets(tid): return sorted({round(n[0], 3) for n in track_notes(tid)})
BASS_T, ACID_T, STAB_T, METAL_T, GROWL_T = (onsets(t) for t in ('bass', 'acid', 'stab', 'metal', 'growl'))
ENG = track_notes('engine')                     # the sampler keys: 38 blip, 40 pass-by, 41 run through the gears, 45 backfire
REVS, PASSES, RISES, BACKS = ([n[0] for n in ENG if n[2] == k] for k in (38, 40, 41, 45))

def env(times, t, k):
    i = bisect.bisect_right(times, t) - 1
    return math.exp(-(t - times[i]) * k) if i >= 0 else 0.0

def count(times, t):
    return bisect.bisect_right(times, t)

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
        self.clap = env(CLAPS, t, 9)
        self.hat = env(HATS, t, 22)
        self.crash = env(CRASH, t, 2.2)
        self.rim = env(RIMS, t, 8)
        self.bassenv = env(BASS_T, t, 10)
        self.acid = env(ACID_T, t, 12)
        self.stab = env(STAB_T, t, 5)
        self.metal = env(METAL_T, t, 6)
        self.growl = env(GROWL_T, t, 6)
        self.pulse = math.exp(-frac(self.beat) * 4)        # the click, when the drums are out
        self.base = Image.new('RGB', (W, H), BG)
        self.neon = Image.new('RGB', (W, H), (0, 0, 0))
        self.db = ImageDraw.Draw(self.base)
        self.dn = ImageDraw.Draw(self.neon)
        self.wash = None                                   # a colour added over the whole frame

# ------------------------------------------------------------------ pictures and clips made with Grok (media/)
MEDIA = os.path.join(HERE, 'media')
FRAMES = os.path.join(MEDIA, '.frames')       # clips unpacked to 60 fps stills by --prepare (not committed)
_img = {}

def _load(path):
    if path not in _img:
        if len(_img) > 40: _img.clear()
        _img[path] = Image.open(path).convert('RGB').resize((W, H), Image.BILINEAR)
    return _img[path]

def clip_len(name):
    d = os.path.join(FRAMES, name)
    return len(os.listdir(d)) if os.path.isdir(d) else 0

def footage(name, local, zoom=0.0, bright=1.0, shift=(0.0, 0.0), span=None):
    """A frame of a clip or, without one, its still picture. A clip is played forwards then backwards,
    so it never jumps; with `span` (seconds) it is instead stretched to play once over that time.
    zoom crops towards the centre; returns float32 H x W x 3."""
    n = clip_len(name)
    if n > 1:
        if span:
            p = min(n - 1.0, max(0.0, local / span * (n - 1)))
        else:
            p = (local * CLIP_FPS) % (2 * n - 2)
            if p > n - 1: p = 2 * n - 2 - p
        i = int(p)
        im = _load(os.path.join(FRAMES, name, '%04d.jpg' % (i + 1)))
        if p - i > 0.02 and i + 1 < n:                      # between two frames: blend, never repeat one
            im = Image.blend(im, _load(os.path.join(FRAMES, name, '%04d.jpg' % (i + 2))), p - i)
    else:
        im = _load(os.path.join(MEDIA, name + '.jpg'))
        zoom += 0.10 * smooth(local / 24.0) + 0.02          # a still gets a slow push-in
    if zoom > 0.001 or shift != (0.0, 0.0):
        z = 1.0 / (1.0 + zoom)
        cw, ch = W * z, H * z
        x0 = (W - cw) / 2 + shift[0] * (W - cw) / 2
        y0 = (H - ch) / 2 + shift[1] * (H - ch) / 2
        im = im.resize((W, H), Image.BILINEAR, box=(x0, y0, x0 + cw, y0 + ch))
    return np.asarray(im, dtype=np.float32) * bright

# ------------------------------------------------------------------ particles
RNG = np.random.default_rng(132)
C_CYAN = np.array(CYAN, np.float32) / 255
C_MAG = np.array(MAG, np.float32) / 255
C_ORANGE = np.array(ORANGE, np.float32) / 255
C_WHITE = np.array((1.0, 1.0, 1.0), np.float32)

def new_acc():
    return np.zeros((H * W, 3), np.float32)

def splat(acc, px, py, col):
    """Add points (bilinear) to the accumulation buffer. col: N x 3, linear."""
    ok = (px >= 0) & (px < W - 1) & (py >= 0) & (py < H - 1)
    if not ok.any(): return
    px, py, col = px[ok], py[ok], col[ok]
    x0 = px.astype(np.int64); y0 = py.astype(np.int64)
    fx = (px - x0).astype(np.float32); fy = (py - y0).astype(np.float32)
    for dx, dy, w in ((0, 0, (1 - fx) * (1 - fy)), (1, 0, fx * (1 - fy)), (0, 1, (1 - fx) * fy), (1, 1, fx * fy)):
        idx = (y0 + dy) * W + (x0 + dx)
        for ch in range(3):
            acc[:, ch] += np.bincount(idx, weights=col[:, ch] * w, minlength=H * W)

def rot_y(p, a):
    c_, s_ = math.cos(a), math.sin(a)
    return np.stack([p[:, 0] * c_ + p[:, 2] * s_, p[:, 1], -p[:, 0] * s_ + p[:, 2] * c_], 1)

def rot_x(p, a):
    c_, s_ = math.cos(a), math.sin(a)
    return np.stack([p[:, 0], p[:, 1] * c_ - p[:, 2] * s_, p[:, 1] * s_ + p[:, 2] * c_], 1)

def project(acc, p, col, f=760.0, gain=1.0, near=0.25, cx=W / 2, cy=H / 2, falloff=2.0):
    """Perspective projection of camera-space points (camera at the origin, looking down +z)."""
    z = p[:, 2]
    ok = z > near
    if not ok.any(): return
    p, col, z = p[ok], col[ok], z[ok]
    b = np.minimum(gain / z ** falloff, 3.0).astype(np.float32)
    splat(acc, cx + f * p[:, 0] / z, cy - f * p[:, 1] / z, col * b[:, None])

def palette_cols(n, weights=(0.5, 0.35, 0.15)):
    k = RNG.choice(3, n, p=weights)
    return np.stack([C_CYAN, C_MAG, C_ORANGE])[k].astype(np.float32)

# -- tunnel: rings of light rushing past
TN = 9000
T_TH = RNG.random(TN) * 2 * np.pi
T_Z = np.where(RNG.random(TN) < 0.6, np.floor(RNG.random(TN) * 40) * 2.0, RNG.random(TN) * 80)
T_SIDE = np.floor(T_TH / (np.pi / 3))                      # six walls
T_COL = np.where((T_Z // 2 % 2 == 0)[:, None], C_CYAN, C_MAG).astype(np.float32)
def p_tunnel(acc, travel, twist=0.0, gain=1.0, radius=3.2):
    for k in range(3):                                      # three samples along the motion: streaks
        z = (T_Z - travel + k * 0.22) % 80.0 + 0.3
        th = T_TH + twist * z * 0.02
        r = radius * (1 + 0.04 * np.cos(6 * T_TH))          # a little hexagonal
        p = np.stack([r * np.cos(th), r * np.sin(th), z], 1)
        project(acc, p, T_COL, gain=gain * (1.0 - 0.3 * k) * 5.0, falloff=1.6)

# -- city: a point cloud of towers to fly through
def _city():
    pts, cols = [], []
    for gx in range(-9, 10):
        if gx == 0: continue                                # the road
        for gz in range(28):
            h = 3.0 + 18 * RNG.random() ** 2.0
            w = 1.0 + RNG.random() * 0.45
            x0, z0 = gx * 3.4 + (0.8 if gx > 0 else -0.8), gz * 4.0
            n = int(70 + h * 16)
            kind = RNG.random(n)
            ex = RNG.choice([-w, w], n); ez = RNG.choice([-w, w], n)      # the four vertical edges
            y = RNG.random(n) * h
            top = kind < 0.16                                               # the roof line
            ex = np.where(top, RNG.uniform(-w, w, n), ex); y = np.where(top, h, y)
            win = kind > 0.55                                               # lit windows, in floors and columns
            ex = np.where(win, np.round(RNG.uniform(-w, w, n) / (w / 3)) * (w / 3), ex)
            y = np.where(win, np.round(y / 0.7) * 0.7, y)
            pts.append(np.stack([x0 + ex, y, z0 + ez], 1))
            c_ = np.where(win[:, None], C_MAG if RNG.random() < 0.55 else C_ORANGE, C_CYAN)
            cols.append(c_ * np.where(top, 1.5, np.where(win, 0.8, 1.0))[:, None])
    road = np.stack([RNG.choice([-1.3, 1.3, 0.0], 1800), np.zeros(1800), RNG.random(1800) * 112], 1)
    pts.append(road); cols.append(np.tile(C_WHITE * 0.7, (1800, 1)))
    return np.concatenate(pts).astype(np.float32), np.concatenate(cols).astype(np.float32)
CITY_P, CITY_C = _city()
def p_city(acc, travel, height=1.6, sway=0.0, lift=1.0, gain=1.0):
    p = CITY_P.copy()
    p[:, 2] = (p[:, 2] - travel) % 112.0 + 0.4
    p[:, 1] = p[:, 1] * lift - height
    p[:, 0] += sway
    project(acc, p, CITY_C, gain=gain * 90.0, falloff=2.0)

# -- reactor: a sphere that breathes with the kick, in three rings
SN = 7000
_i = np.arange(SN) + 0.5
S_PHI = np.arccos(1 - 2 * _i / SN); S_TH = np.pi * (1 + 5 ** 0.5) * _i
S_DIR = np.stack([np.sin(S_PHI) * np.cos(S_TH), np.cos(S_PHI), np.sin(S_PHI) * np.sin(S_TH)], 1).astype(np.float32)
S_COL = np.where((np.floor(S_PHI * 9) % 2 == 0)[:, None], C_CYAN, C_MAG).astype(np.float32)
RING_T = RNG.random(2400) * 2 * np.pi
SH_DIR = RNG.standard_normal((1600, 3)); SH_DIR /= np.linalg.norm(SH_DIR, axis=1)[:, None]
def p_reactor(acc, x, dist=7.5, gain=1.0):
    t = x.t
    wob = 0.16 * np.sin(S_PHI * 7 + t * 3.1) * np.sin(S_TH * 3 - t * 2.2)
    r = 2.0 * (1 + 0.22 * x.kick + wob * (0.4 + x.acid))
    p = rot_x(rot_y(S_DIR * r[:, None], t * 0.5), 0.35)
    p[:, 2] += dist
    project(acc, p, S_COL, gain=gain * 16.0)
    for k in range(3):                                      # rings
        th = RING_T[k * 800:(k + 1) * 800] + t * (0.9 + 0.4 * k) * (1 if k % 2 else -1)
        rr = 3.0 + 0.55 * k + 0.25 * x.kick
        ring = np.stack([rr * np.cos(th), 0.05 * np.sin(th * 9), rr * np.sin(th)], 1)
        ring = rot_x(rot_y(ring, 0.7 * k + t * 0.2), 0.5 + 0.7 * k)
        ring[:, 2] += dist
        project(acc, ring, np.tile(C_ORANGE if k == 1 else C_WHITE * 0.8, (800, 1)), gain=gain * 22.0)
    i = bisect.bisect_right(CLAPS, t) - 1                   # a shell of sparks off every clap
    while i >= 0 and t - CLAPS[i] < 0.9:
        age = t - CLAPS[i]
        sh = SH_DIR * (2.2 + age * 9.0)
        sh[:, 2] += dist
        project(acc, sh.astype(np.float32), np.tile(C_WHITE, (1600, 1)), gain=gain * 30.0 * math.exp(-age * 4.5))
        i -= 1

# -- pistons: eight cylinders firing in sixteenths (two beats are one engine cycle)
PN = 420
P_TH = RNG.random(PN) * 2 * np.pi
P_Y = RNG.random(PN)
ORDER = [0, 4, 3, 5, 6, 2, 7, 1]                              # which cylinder fires on each sixteenth
F_DIR = RNG.standard_normal((260, 3)); F_DIR[:, 1] = np.abs(F_DIR[:, 1]) + 0.3
def p_pistons(acc, x, gain=1.0):
    t = x.t
    cyc = x.beat / 2.0
    pts, cols = [], []
    for cyl in range(8):
        row, k = cyl // 4, cyl % 4
        fire_at = ORDER.index(cyl) / 8.0
        ph = (cyc - fire_at) % 1.0                          # 0 at the moment it fires (top of the stroke)
        up = 0.75 * math.cos(2 * math.pi * ph * 2)          # two strokes up and down per cycle
        bx, bz, lean = (k - 1.5) * 1.5, (row - 0.5) * 1.3, (row - 0.5) * 0.5
        head = np.stack([0.48 * np.cos(P_TH), up + 0.5 * P_Y, 0.48 * np.sin(P_TH)], 1)
        bore = np.stack([0.56 * np.cos(P_TH[:200]), -1.2 + 2.9 * P_Y[:200], 0.56 * np.sin(P_TH[:200])], 1)
        rod = np.stack([np.zeros(90), -2.0 + (up + 2.0) * P_Y[:90], np.zeros(90)], 1)
        for part, col, a in ((head, C_WHITE, 1.0), (bore, C_CYAN, 0.35), (rod, C_MAG, 0.8)):
            q = part.copy()
            q[:, 2] += q[:, 1] * lean                       # the two banks lean apart: a V
            q[:, 0] += bx; q[:, 2] += bz
            pts.append(q); cols.append(np.tile(col * a, (len(q), 1)))
        if ph < 0.09:                                       # the charge going off
            age = ph * 2 * BEAT
            fl = F_DIR * (0.3 + age * 9.0)
            fl[:, 0] += bx; fl[:, 1] += 1.5; fl[:, 2] += bz + 1.5 * lean
            pts.append(fl); cols.append(np.tile(C_ORANGE * 3.0 * math.exp(-age * 14), (len(fl), 1)))
    crank = np.stack([np.linspace(-3.4, 3.4, 400), -2.0 + 0.12 * np.sin(np.linspace(0, 50, 400) + cyc * 4 * math.pi), np.zeros(400)], 1)
    pts.append(crank); cols.append(np.tile(C_MAG, (400, 1)))
    p = np.concatenate(pts).astype(np.float32)
    p = rot_x(rot_y(p, t * 0.22 + 0.5 * math.sin(t * 0.31)), 0.42)
    p[:, 2] += 8.0
    project(acc, p, np.concatenate(cols).astype(np.float32), gain=gain * 18.0)

# -- vortex: everything drains to the centre
VN = 12000
V_R = 0.3 + RNG.random(VN) ** 0.7 * 9.0
V_TH = RNG.integers(0, 3, VN) * (2 * np.pi / 3) + RNG.normal(0, 0.28, VN)
V_Y = RNG.normal(0, 0.12, VN)
def p_vortex(acc, t, gain=1.0, tilt=1.0):
    r = (V_R - t * 0.22) % 9.0 + 0.3
    th = V_TH + 2.3 * np.log(r) + t * 0.5 / np.sqrt(r)
    p = np.stack([r * np.cos(th), V_Y * r * 0.4, r * np.sin(th)], 1)
    p = rot_x(p, tilt)
    p[:, 2] += 10.0
    k = np.clip(r / 9.0, 0, 1)[:, None]
    col = (C_WHITE * (1 - k) + (C_CYAN * (1 - k) + C_MAG * k) * (0.4 + 0.6 * k)).astype(np.float32)
    project(acc, p.astype(np.float32), col, gain=gain * 50.0)

# -- terrain: a grid of light rolling under the camera
TX, TZ = np.meshgrid(np.linspace(-34, 34, 150), np.arange(0, 90, 1.0))
TX = TX.ravel().astype(np.float32); TZ = TZ.ravel().astype(np.float32)
def p_terrain(acc, x, travel, gain=1.0):
    zz = (TZ - travel) % 90.0 + 0.5
    za = TZ + np.floor((travel - TZ) / 90.0 + 1) * 90.0     # the row's own place in the world
    ridge = np.clip((np.abs(TX) - 3.0) / 10.0, 0, 1)
    y = -2.2 + ridge * (2.6 * np.sin(TX * 0.31 + za * 0.13) * np.cos(za * 0.21) + 1.4 * x.bassenv * np.sin(TX * 0.9 + za))
    col = np.where((np.abs(TX) < 3.2)[:, None], C_MAG, C_CYAN).astype(np.float32)
    project(acc, np.stack([TX, y, zz], 1), col, gain=gain * 36.0, falloff=1.5)

# -- screen-space extras for the footage
RAIN = RNG.random((1400, 3))
def p_rain(acc, t, gain=0.35):
    for k in range(3):
        y = ((RAIN[:, 1] + t * (1.3 + RAIN[:, 2])) % 1.0) * (H + 40) - 20 + k * 7
        xx = RAIN[:, 0] * (W + 200) - 100 - (y / H) * 60 - k * 2
        splat(acc, xx, y, np.tile(C_CYAN * gain * (0.4 + 0.6 * RAIN[:, 2:3].mean()), (1400, 1)) * (0.4 + 0.6 * RAIN[:, 2:3]))

EMB = RNG.random((700, 4))
def p_embers(acc, t, gain=0.9, cx=0.5, spread=1.0):
    life = (EMB[:, 2] + t * (0.18 + 0.25 * EMB[:, 3])) % 1.0
    xx = (cx + (EMB[:, 0] - 0.5) * spread) * W + np.sin(life * 9 + EMB[:, 1] * 30) * 26
    y = H * (1.05 - life * 1.1)
    splat(acc, xx, y, (C_ORANGE * gain)[None, :] * ((1 - life) * (0.5 + EMB[:, 3]))[:, None].astype(np.float32))

SPD = RNG.random((900, 3))
def p_speed(acc, t, strength=1.0, speed=1.6):
    th = SPD[:, 0] * 2 * np.pi
    for k in range(5):
        r = ((SPD[:, 1] + t * speed * (0.5 + SPD[:, 2])) % 1.0) ** 2 * 820 + 30 - k * 9
        col = np.where((SPD[:, 2] > 0.5)[:, None], C_CYAN, C_WHITE) * (strength * (r / 820) * (1 - 0.15 * k))[:, None]
        splat(acc, W / 2 + np.cos(th) * r, H / 2 + np.sin(th) * r * 0.62, col.astype(np.float32))

PASSP = RNG.random((1200, 3))
def p_pass(acc, t):
    """A car going by: a bar of light across the frame, once per pass-by sample."""
    i = bisect.bisect_right(PASSES, t) - 1
    if i < 0 or t - PASSES[i] > 1.6: return
    age = (t - PASSES[i]) / 1.6
    head = -300 + age * (W + 1500)
    xx = head - PASSP[:, 0] ** 2 * 1300
    y = H * 0.66 + (PASSP[:, 1] - 0.5) * 70 * (0.3 + PASSP[:, 0])
    col = np.where((PASSP[:, 2] > 0.5)[:, None], C_MAG, C_WHITE) * ((1 - PASSP[:, 0]) * 1.4)[:, None]
    splat(acc, xx, y, col.astype(np.float32))

BF = RNG.standard_normal((500, 2))
def p_backfire(acc, t):
    i = bisect.bisect_right(BACKS, t) - 1
    if i < 0 or t - BACKS[i] > 0.5: return
    age = t - BACKS[i]
    r = 20 + age * 900
    splat(acc, W / 2 + BF[:, 0] * r, H * 0.86 - np.abs(BF[:, 1]) * r * 0.5, np.tile(C_ORANGE * 2.5 * math.exp(-age * 9), (500, 1)).astype(np.float32))

# ------------------------------------------------------------------ putting a frame together
EXPOSURE = 14.0
def compose(x, bg, acc, glow=1.3):
    part = 1.0 - np.exp(-acc.reshape(H, W, 3) * EXPOSURE)      # soft clip: dense clouds glow instead of burning out
    small = Image.fromarray(np.clip(part * 200, 0, 255).astype(np.uint8)).resize((W // 4, H // 4), Image.BILINEAR)
    bloom = np.asarray(small.filter(ImageFilter.GaussianBlur(5)).resize((W, H), Image.BILINEAR), dtype=np.float32)
    out = part * 255.0 + bloom * glow
    if bg is not None: out += bg
    x.base = Image.fromarray(np.clip(out, 0, 255).astype(np.uint8))
    x.db = ImageDraw.Draw(x.base)

def travel_of(x, per_beat):
    """Distance covered: steady, with a lurch on every beat."""
    return per_beat * (x.beat + 0.35 * (1 - math.exp(-frac(x.beat) * 6)))

def tach(x, cx=W - 190, cy=H - 150, r=96):
    """Rev counter, driven by the engine samples: the run through the gears, a blip, the idle."""
    i = bisect.bisect_right(RISES, x.t) - 1
    gear, v = 0, 0.12 + 0.02 * math.sin(x.t * 31)
    if i >= 0 and x.t - RISES[i] < 2 * BARLEN:
        k = (x.t - RISES[i]) / (BARLEN / 2)
        gear = int(k) + 1
        v = 0.35 + 0.1 * (gear - 1) + (0.5 - 0.08 * (gear - 1)) * frac(k)
    j = bisect.bisect_right(REVS, x.t) - 1
    if j >= 0 and x.t - REVS[j] < 1.7:
        a = x.t - REVS[j]
        v = max(v, 0.15 + 0.8 * (a / 0.5 if a < 0.5 else math.exp(-(a - 0.5) * 2.2)))
    d = x.dn
    d.arc([cx - r, cy - r, cx + r, cy + r], 150, 390, fill=c(CYAN, 0.5), width=3)
    d.arc([cx - r, cy - r, cx + r, cy + r], 345, 390, fill=c(MAG, 0.9), width=6)
    for k in range(9):
        a = math.radians(150 + k * 30)
        d.line([(cx + math.cos(a) * (r - 12), cy + math.sin(a) * (r - 12)), (cx + math.cos(a) * r, cy + math.sin(a) * r)], fill=c(WHITE, 0.7), width=2)
    a = math.radians(150 + 240 * min(1.0, v))
    d.line([(cx, cy), (cx + math.cos(a) * (r - 18), cy + math.sin(a) * (r - 18))], fill=c(ORANGE), width=5)

def extras(x, acc):
    p_pass(acc, x.t)
    p_backfire(acc, x.t)

# ------------------------------------------------------------------ scenes
def scene_intro(x):
    acc = new_acc()
    catch = BEAT * 2.2                                      # the starter gives way to the engine
    if x.st < BARLEN:
        bg = footage('engine', 0.0, zoom=0.25, bright=0.25 * smooth((x.st - catch) / 0.4) + 0.06 * abs(math.sin(x.t * 21)) * (x.st < catch))
        p_embers(acc, x.t, gain=0.5 + 2.0 * (x.st > catch), cx=0.5, spread=0.5)
    else:
        shake = (math.sin(x.t * 55) * 0.25, math.cos(x.t * 47) * 0.25)      # idling
        bg = footage('engine', x.st - BARLEN, zoom=0.12 + 0.03 * x.kick, bright=0.5 + 0.3 * x.kick, shift=shake, span=7 * BARLEN)   # it heats up through the intro
        p_embers(acc, x.t, gain=1.2)
    extras(x, acc)
    compose(x, bg, acc)
    tach(x)

def scene_a1(x):
    acc = new_acc()
    if x.sbar < 8:                                          # the city as a cloud of points
        p_city(acc, travel_of(x, 2.2), lift=1 + 0.12 * x.bassenv)
        bg = footage('skyline', x.st, bright=0.16)
    else:                                                   # then the real road
        bg = footage('highway', x.st - 8 * BARLEN, zoom=0.04 * x.kick, bright=0.62 + 0.22 * x.kick)
        p_rain(acc, x.t)
        p_speed(acc, x.t, strength=0.25 + 0.3 * x.kick)
    extras(x, acc)
    compose(x, bg, acc)

def scene_build(x, name='tunnel'):
    acc = new_acc()
    k = x.sp
    travel = BEAT * 0 + (x.st / BEAT) * (2.0 + 5.0 * k) * 0.5 + 6.0 * k * k * x.sbars     # it keeps accelerating
    p_tunnel(acc, travel, twist=1.0 + 3.0 * k, gain=0.7 + 0.8 * k)
    p_speed(acc, x.t, strength=0.2 + 0.9 * k, speed=1.0 + 2.5 * k)
    bg = footage(name, x.st, zoom=0.3 * k, bright=0.25 + 0.2 * k)
    extras(x, acc)
    compose(x, bg, acc)
    tach(x)
    if x.sbar == x.sbars - 1:
        x.wash = c(WHITE, 0.25 * (x.inbar / 4) ** 3)

def shot(x, kind, local, acc):
    """One shot of a drop. Footage shots punch in on the kick; particle shots are full 3D."""
    punch = 0.05 * x.kick
    if kind == 'car':
        p_speed(acc, x.t, strength=0.5 + 0.5 * x.kick, speed=2.4)
        return footage('car', local, zoom=punch, bright=0.75 + 0.3 * x.kick)
    if kind == 'tunnel':
        p_tunnel(acc, travel_of(x, 7.0), twist=2.0, gain=0.6)
        return footage('tunnel', local, zoom=punch + 0.05, bright=0.6 + 0.3 * x.kick)
    if kind == 'foundry':
        p_embers(acc, x.t * 2.2, gain=1.8)
        return footage('foundry', local, zoom=punch, bright=0.75 + 0.3 * x.kick)
    if kind == 'turbine':
        p_vortex(acc, x.t * 1.6, gain=0.5, tilt=0.15)
        return footage('turbine', local, zoom=punch + 0.04, bright=0.7 + 0.35 * x.kick)
    if kind == 'highway':
        p_rain(acc, x.t, gain=0.5); p_speed(acc, x.t, strength=0.6, speed=2.2)
        return footage('highway', local, zoom=punch, bright=0.7 + 0.3 * x.kick)
    if kind == 'skyline':
        p_city(acc, travel_of(x, 4.0), height=5.0, gain=0.5)
        return footage('skyline', local, zoom=punch, bright=0.6 + 0.3 * x.kick)
    if kind == 'reactor':
        p_reactor(acc, x)
        return footage('turbine', local, zoom=0.3, bright=0.12)
    if kind == 'pistons':
        p_pistons(acc, x)
        return footage('engine', local, zoom=0.2, bright=0.14)
    if kind == 'city':
        p_city(acc, travel_of(x, 6.0), lift=1 + 0.25 * x.kick, sway=1.2 * math.sin(x.beat * math.pi / 4))
        return None
    if kind == 'terrain':
        p_terrain(acc, x, travel_of(x, 6.0))
        return footage('dawn', local, zoom=0.1, bright=0.12)
    if kind == 'vortex':
        p_vortex(acc, x.t * 2.0, gain=1.2, tilt=0.9 + 0.2 * math.sin(x.t))
        return None
    raise ValueError(kind)

SHOTS = {
    'drop1': (2, ['car', 'reactor', 'tunnel', 'pistons', 'foundry', 'city', 'turbine', 'vortex']),
    'drop2': (2, ['tunnel', 'pistons', 'car', 'terrain', 'turbine', 'city', 'highway', 'reactor']),
    'drop3': (1, ['car', 'reactor', 'tunnel', 'city', 'foundry', 'pistons', 'turbine', 'terrain',
                  'highway', 'vortex', 'car', 'reactor', 'skyline', 'pistons', 'tunnel', 'city']),
}
def scene_drop(x):
    bars, seq = SHOTS[x.sid]
    i = (x.sbar // bars) % len(seq)
    local = x.st - (x.sbar // bars) * bars * BARLEN
    acc = new_acc()
    bg = shot(x, seq[i], local, acc)
    extras(x, acc)
    compose(x, bg, acc)
    if local < 0.085 and x.sbar:                        # a flash on the cut
        x.wash = c(WHITE, 0.35)
    if x.clap > 0.5 and seq[i] in ('reactor', 'city', 'vortex', 'pistons', 'terrain'):
        x.wash = c(MAG, 0.06 * x.clap)

def scene_break(x):
    acc = new_acc()
    bg = footage('skyline', x.st, bright=0.7, shift=(-0.6 + 1.2 * x.sp, 0.0), zoom=0.12)
    p_vortex(acc, x.t * 0.5, gain=0.35 + 0.5 * x.stab, tilt=1.15)
    p_embers(acc, x.t * 0.5, gain=0.5, spread=1.4)
    extras(x, acc)
    compose(x, bg, acc)
    tach(x)
    if x.sbar >= x.sbars - 2:
        x.wash = c(WHITE, 0.2 * ((x.st - (x.sbars - 2) * BARLEN) / (2 * BARLEN)) ** 3)

def scene_open(x):
    acc = new_acc()
    p_pistons(acc, x, gain=1.2)
    bg = footage('engine', x.st, zoom=0.2 + 0.03 * x.kick, bright=0.2 + 0.1 * x.kick)
    extras(x, acc)
    compose(x, bg, acc)

def scene_outro(x):
    acc = new_acc()
    k = x.t - LAST * BARLEN
    if k < 0:
        bg = footage('dawn', x.st, bright=0.45 + 0.45 * x.sp, zoom=0.03 * x.kick, span=8 * BARLEN)      # one slow sunrise
        p_embers(acc, x.t * 0.4, gain=0.5, spread=1.4)
    else:
        bg = footage('dawn', x.st, bright=0.95, span=8 * BARLEN)
        sh = SH_DIR * (1.0 + k * 7.0); sh[:, 2] += 9.0
        project(acc, sh.astype(np.float32), np.tile(C_WHITE, (1600, 1)), gain=40.0 * math.exp(-k * 2.2))
    extras(x, acc)
    compose(x, bg, acc)

SCENES = {'intro': scene_intro, 'a1': scene_a1, 'build1': scene_build, 'drop1': scene_drop, 'break': scene_break,
          'drop2': scene_drop, 'open': scene_open, 'drop3': scene_drop, 'outro': scene_outro}

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
    if len(sys.argv) >= 2 and sys.argv[1] == '--prepare':      # media/<name>.mp4 -> media/.frames/<name>/0001.jpg … at 60 fps (motion-interpolated, so stretched footage stays smooth)
        for f in sorted(os.listdir(MEDIA)):
            if not f.endswith('.mp4'): continue
            d = os.path.join(FRAMES, f[:-4])
            os.makedirs(d, exist_ok=True)
            for old in os.listdir(d): os.remove(os.path.join(d, old))
            subprocess.run(['ffmpeg', '-loglevel', 'error', '-y', '-i', os.path.join(MEDIA, f), '-an',
                            '-vf', f'minterpolate=fps={CLIP_FPS}:mi_mode=mci:mc_mode=aobmc:me_mode=bidir:vsbmc=1,scale={W}:{H}:force_original_aspect_ratio=increase,crop={W}:{H}', '-q:v', '3',
                            os.path.join(d, '%04d.jpg')], check=True)
            print(f, len(os.listdir(d)), 'frames')
        return
    if len(sys.argv) >= 4 and sys.argv[1] == '--still':
        frame(int(float(sys.argv[2]) * FPS)).save(sys.argv[3])
        return
    out = os.path.join(SRC, 'visuals', 'chrome-engine.mp4')
    first, n = 0, int(DUR * FPS)
    if len(sys.argv) >= 5 and sys.argv[1] == '--clip':        # --clip <from s> <to s> <out.mp4>: a preview
        first, n, out = int(float(sys.argv[2]) * FPS), int(float(sys.argv[3]) * FPS), sys.argv[4]
    os.makedirs(os.path.dirname(out), exist_ok=True)
    ff = subprocess.Popen(
        ['ffmpeg', '-loglevel', 'error', '-y', '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-s', f'{W}x{H}', '-r', str(FPS), '-i', '-',
         '-an', '-c:v', 'libx264', '-preset', 'veryslow', '-tune', 'animation', '-crf', '20', '-pix_fmt', 'yuv420p', '-g', str(2 * FPS), '-movflags', '+faststart', out],
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

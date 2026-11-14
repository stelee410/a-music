#!/usr/bin/env python3
"""Writes chrome-engine.src/samples/*.wav — the car-engine sounds of 《铬色引擎 Chrome Engine》.

    .venv/bin/python songs/chrome-engine/make_samples.py

These are synthesised, not recorded: a model of an eight-cylinder engine (one
"engine cycle" is two turns of the crank; the sound is a sum of harmonics of the
cycle frequency, strongest at the firing orders, shaped by exhaust resonances,
plus combustion noise gated by the firing pulse, then driven into soft clipping).
Everything is seeded, so the files are the same on every run. Needs numpy.
"""
import os, wave
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, 'chrome-engine.src', 'samples')
SR = 48000
BPM = 132
BAR = 4 * 60 / BPM
E1 = 41.2034

def resonance(f):
    """Gain of the exhaust and body at frequency f (Hz): a few broad peaks over a falling slope."""
    f = np.maximum(f, 1.0)
    g = 1.0 / (1.0 + (f / 900.0) ** 1.6)
    for fc, q, a in ((78, 0.5, 1.4), (190, 0.45, 1.0), (430, 0.4, 0.8), (1150, 0.35, 0.5), (2600, 0.3, 0.25)):
        g += a * np.exp(-0.5 * (np.log(f / fc) / q) ** 2)
    return g * (f > 24)

def order_amp(k):
    """Amplitude of harmonic k of the engine cycle. Eight firings a cycle: k = 8, 16, 24 … carry the note;
    k = 4 and the odd orders are the uneven-firing rumble."""
    a = 1.0 / k ** 0.55
    if k % 8 == 0: a *= 3.2
    elif k % 4 == 0: a *= 1.9
    elif k % 2 == 0: a *= 0.9
    else: a *= 0.55
    return a

def engine(cycle_hz, seed=1, load=None, rough=0.3, drive=2.2, orders=64, periodic=False):
    """cycle_hz: engine-cycle frequency for every sample (rpm / 120). load: 0–1, throttle (brighter, louder)."""
    rng = np.random.default_rng(seed)
    n = len(cycle_hz)
    load = np.full(n, 0.6) if load is None else load
    phase = np.cumsum(cycle_hz) / SR                      # in engine cycles
    x = np.zeros(n)
    for k in range(1, orders + 1):
        f = cycle_hz * k
        if f.min() > 9000: break
        amp = order_amp(k) * resonance(f) * (0.55 + 0.45 * load) * (1.0 + load * min(k, 40) / 40.0) * (f < 10000)
        x += amp * np.sin(2 * np.pi * (k * phase + rng.random()))
    # every cylinder fires a little differently, every time
    if periodic:
        cycles = int(round(phase[-1]))
        jitter = np.tile(1 + rough * 0.5 * rng.standard_normal(8), cycles + 1)
    else:
        jitter = 1 + rough * 0.5 * rng.standard_normal(int(phase[-1] * 8) + 9)
    x *= jitter[(phase * 8).astype(int)]
    # combustion noise, let through by each firing
    if periodic:
        spec = rng.standard_normal(n // 2 + 1) + 1j * rng.standard_normal(n // 2 + 1)
        noise = np.fft.irfft(spec, n)
    else:
        noise = rng.standard_normal(n)
    spec = np.fft.rfft(noise)
    fr = np.fft.rfftfreq(n, 1 / SR)
    noise = np.fft.irfft(spec * resonance(fr) * (fr > 60), n)
    noise /= np.abs(noise).max() + 1e-9
    gate = np.maximum(0, np.sin(2 * np.pi * 8 * phase)) ** 3
    x = x / (np.abs(x).max() + 1e-9) + rough * (0.5 + load) * 1.6 * noise * (0.25 + gate)
    x = np.tanh(x * drive * (0.7 + 0.6 * load))
    return x

def env(n, attack=0.005, release=0.05):
    e = np.ones(n)
    a, r = int(attack * SR), int(release * SR)
    e[:a] = np.linspace(0, 1, a)
    e[-r:] = np.linspace(1, 0, r)
    return e

def curve(points, n):
    """Piecewise curve through (time s, value) points, smoothed."""
    t = np.arange(n) / SR
    y = np.interp(t, [p[0] for p in points], [p[1] for p in points])
    k = int(0.03 * SR)
    return np.convolve(np.pad(y, k, mode='edge'), np.ones(2 * k + 1) / (2 * k + 1), mode='valid')

def rpm(r): return r / 120.0

def write(name, x, peak=0.9):
    x = x - x.mean()
    x = x / (np.abs(x).max() + 1e-9) * peak
    os.makedirs(OUT, exist_ok=True)
    with wave.open(os.path.join(OUT, name), 'wb') as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes((x * 32767).astype('<i2').tobytes())
    print(f'{name}: {len(x) / SR:.2f} s, {len(x)} frames')
    return len(x)

frames = {}

# 1. start: the starter turns the engine over three times, it catches, flares, settles to idle
n = int(2.6 * SR)
c = curve([(0, 2.2), (0.9, 2.6), (1.0, 6), (1.25, 21), (1.6, 12), (2.6, 8)], n)
l = curve([(0, 0.2), (0.95, 0.3), (1.1, 1.0), (1.4, 0.5), (2.6, 0.3)], n)
x = engine(c, seed=11, load=l, rough=0.5)
t = np.arange(n) / SR
whine = 0.5 * np.sin(2 * np.pi * np.cumsum(curve([(0, 180), (0.4, 420), (0.95, 460), (1.05, 0)], n)) / SR) * (t < 1.02)
whine *= 0.6 + 0.4 * np.sin(2 * np.pi * 3.4 * t)          # the starter labours on every compression
frames['start'] = write('engine_start.wav', (x * np.where(t < 1.0, 0.45, 1.0) + whine * 0.5) * env(n, 0.01, 0.2))

# 2. rev: a blip of the throttle
n = int(1.7 * SR)
c = curve([(0, rpm(1300)), (0.42, rpm(6400)), (0.6, rpm(6600)), (1.2, rpm(2400)), (1.7, rpm(1400))], n)
l = curve([(0, 0.9), (0.55, 1.0), (0.7, 0.1), (1.7, 0.15)], n)
frames['rev'] = write('engine_rev.wav', engine(c, seed=12, load=l) * env(n, 0.004, 0.25))

# 3. pass: a car going by flat out; the pitch drops as it passes
n = int(2.4 * SR)
t = np.arange(n) / SR
doppler = 1 - 0.16 * np.tanh((t - 1.1) / 0.16)
c = rpm(6900) * doppler * (1 + 0.03 * t)
x = engine(c, seed=13, load=np.full(n, 0.95), rough=0.25)
frames['pass'] = write('engine_pass.wav', x * np.exp(-0.5 * ((t - 1.1) / 0.42) ** 2) * env(n, 0.05, 0.3))

# 4. rise: two bars of acceleration through the gears, for the bars before a drop
n = int(2 * BAR * SR)
half = BAR / 2
pts, tt = [], 0.0
for lo, hi in ((2600, 6800), (3900, 7000), (4700, 7200), (5300, 7600)):
    pts += [(tt, rpm(lo)), (tt + half - 0.05, rpm(hi))]
    tt += half
c = curve(pts, n)
x = engine(c, seed=14, load=np.full(n, 1.0), rough=0.25)
frames['rise'] = write('engine_rise.wav', x * np.linspace(0.45, 1.0, n) * env(n, 0.02, 0.03))

# 5. idle: a lumpy idle that loops (a whole number of engine cycles, periodic noise)
cycles = 15
n = int(round(cycles / rpm(880) * SR))
x = engine(np.full(n, cycles * SR / n), seed=15, load=np.full(n, 0.25), rough=0.6, periodic=True)
frames['idle'] = write('engine_idle.wav', x, peak=0.8)

# 6. growl: the engine held at the speed where its firing note is E — a loop to be played as an instrument.
#    Cycle frequency E1 / 4: order 4 sounds E1, order 8 (the firing note) E2.
cycles = 24
n = int(round(cycles / (E1 / 4) * SR))
x = engine(np.full(n, cycles * SR / n), seed=16, load=np.full(n, 0.85), rough=0.22, drive=2.6, periodic=True)
frames['growl'] = write('engine_growl.wav', x, peak=0.85)

# 7. backfire: two pops on the overrun
n = int(0.5 * SR)
rng = np.random.default_rng(17)
t = np.arange(n) / SR
x = np.zeros(n)
for at, a in ((0.0, 1.0), (0.11, 0.7), (0.19, 0.45)):
    i = int(at * SR)
    m = n - i
    burst = rng.standard_normal(m) * np.exp(-np.arange(m) / (0.018 * SR))
    spec = np.fft.rfft(burst); fr = np.fft.rfftfreq(m, 1 / SR)
    x[i:] += a * (np.fft.irfft(spec * resonance(fr) * (fr > 70), m) * 6 + 0.8 * np.sin(2 * np.pi * 62 * np.arange(m) / SR) * np.exp(-np.arange(m) / (0.05 * SR)))
frames['backfire'] = write('engine_backfire.wav', np.tanh(x * 1.5) * env(n, 0.001, 0.1))

import json
json.dump(frames, open(os.path.join(HERE, 'samples.json'), 'w'), indent=1)

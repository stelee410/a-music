#!/usr/bin/env python3
"""Writes simulated-heart.src/samples/*.wav — the band's instruments for 《拟心 Simulated Heart》.

    .venv/bin/python songs/simulated-heart/make_samples.py

Everything is synthesised, nothing is recorded: plucked strings by the Karplus-Strong method (a burst
of noise circulating in a delay line that loses a little treble each time round) with a model of the
pick, of the string's two planes of vibration and of the pickup; and a drum kit built from swept sines, tuned modes and filtered noise. Seeded, so every run gives the same
files. Needs numpy. Also writes samples.json (name → frames, root pitch) for make_song.py.
"""
import json, os, wave
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, 'simulated-heart.src', 'samples')
SR = 48000

def hz(midi): return 440.0 * 2 ** ((midi - 69) / 12)

def shape(x, lo=None, hi=None, order=2):
    """Zero-phase band limits in the frequency domain (soft slopes)."""
    n = len(x)
    f = np.fft.rfftfreq(n, 1 / SR)
    g = np.ones_like(f)
    if hi: g *= 1 / np.sqrt(1 + (f / hi) ** (2 * order))
    if lo: g *= 1 - 1 / np.sqrt(1 + (f / lo) ** (2 * order))
    return np.fft.irfft(np.fft.rfft(x) * g, n)

def peak_eq(x, fc, gain, q=1.2):
    n = len(x)
    f = np.maximum(np.fft.rfftfreq(n, 1 / SR), 1.0)
    g = 1 + (gain - 1) * np.exp(-0.5 * (np.log(f / fc) * q * 2) ** 2)
    return np.fft.irfft(np.fft.rfft(x) * g, n)

def string(midi, seconds, rng, bright=0.5, ring=1.6, pick=0.13, hard=1.0, tilt=1.0):
    """One plucked string. bright: 0 dull … 1 glassy; ring: seconds for the note to fall to about a third;
    pick: where it is plucked. High strings would otherwise lose their treble in a few periods, so the
    loss per period is eased as the pitch rises."""
    f0 = hz(midi)
    sustain = np.exp(-1.0 / (ring * f0))
    bright = min(0.97, bright + max(0.0, midi - 52) * 0.007)
    period = int(round(SR / f0))                       # whole samples; the exact pitch is set by resampling below
    n = int(seconds * SR * (SR / period) / f0) + period * 2
    # The string at the moment the pick lets go: bent to a point at the picking position. Its harmonics
    # follow from that shape — the k-th is sin(k·π·pick) / k^tilt, absent where it has a node under the
    # pick — and a soft pick rounds the point off, which removes the upper ones. Only a little is left
    # to chance (the hand is never quite the same), so two takes of a note are alike but not identical.
    k = np.arange(period // 2 + 1, dtype=float)
    k[0] = 1.0
    amp = np.abs(np.sin(np.pi * k * pick)) / k ** tilt
    amp *= np.exp(-((k * f0) / (1500 + 9000 * bright * hard)) ** 2)
    amp *= 1 + 0.12 * rng.standard_normal(len(k))
    amp[0] = 0
    burst = np.fft.irfft(-1j * amp * (period / 2), period)
    out = np.zeros(n)
    blocks = n // period
    cur = burst / (np.abs(burst).max() + 1e-9)
    a = 0.5 - 0.42 * bright                            # how much of the neighbouring sample is mixed in: the treble loss
    for b in range(blocks):
        out[b * period:(b + 1) * period] = cur
        cur = sustain * ((1 - a) * cur + a * np.roll(cur, 1))
    # to the exact pitch
    ratio = (SR / period) / f0
    idx = np.arange(int(seconds * SR)) * (1 / ratio)
    idx = idx[idx < n - 1]
    return np.interp(idx, np.arange(n), out)

def fade(x, attack=0.0015, release=0.06):
    a, r = int(attack * SR), int(release * SR)
    x = x.copy()
    x[:a] *= np.linspace(0, 1, a)
    x[-r:] *= np.linspace(1, 0, r)
    return x

def write(name, x, peak=0.9):
    x = x - x.mean()
    x = x / (np.abs(x).max() + 1e-9) * peak
    os.makedirs(OUT, exist_ok=True)
    with wave.open(os.path.join(OUT, name), 'wb') as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes((x * 32767).astype('<i2').tobytes())
    return len(x)

index = {}

# ---- electric guitar, as it leaves the instrument (before any amplifier)
#
# What is modelled, in the order the sound is made:
#   the pick     — how hard it is (a soft pick leaves fewer high harmonics) and where along the string
#                  it strikes (a notch at every harmonic that has a node there), plus the click of
#                  the pick itself letting go;
#   the string   — two planes of vibration, a few cents apart and dying at different rates, which is
#                  why a real note does not fade evenly but breathes; low strings ring longer;
#   the pickup   — it sits at one point of the string, so it hears each harmonic more or less
#                  strongly depending on where that harmonic's nodes are (another comb), and its coil
#                  resonates: a peak around 3.6 kHz for a single coil, lower and broader for a humbucker.
# The amplifier and the speaker are not here: they are the track's `amp` effect in the player,
# because a chord has to be distorted as a chord (see crates/agb-dsp/src/fx.rs).
#
# Every note is made at two strengths of picking and in two takes, so that the player can choose by
# velocity and never has to play the same recording twice in a row.
PICKUPS = {
    # name: (position along the string, resonance Hz, Q, level of the second plane, ring of an open low string in s)
    'sc': (0.09, 3700.0, 2.6, 0.55, 2.3),             # single coil near the bridge: bright, wiry
    'hb': (0.17, 2500.0, 1.7, 0.7, 3.4),              # humbucker: thicker, sings longer
}

def pickup_tone(x, f0, where, res, q):
    n = len(x)
    f = np.maximum(np.fft.rfftfreq(n, 1 / SR), 1.0)
    comb = np.abs(np.sin(np.pi * (f / f0) * where)) ** 0.8          # what a pickup at `where` hears of each harmonic
    comb = 0.25 + 0.75 * comb / (comb.max() + 1e-9)
    r = f / res
    coil = 1.0 / np.sqrt((1 - r * r) ** 2 + (r / q) ** 2)           # the coil's resonance, then −12 dB per octave
    return np.fft.irfft(np.fft.rfft(x) * comb * coil, n)

def guitar(midi, rng, kind, hard):
    where, res, q, second, ring_low = PICKUPS[kind]
    seconds = 2.6 if kind == 'sc' else 3.2
    ring = ring_low * 2 ** (-(midi - 40) / 30.0)                    # higher notes die sooner
    pick = 0.10 + 0.08 * rng.random()                               # never plucked in exactly the same place
    bright = 0.45 + 0.3 * hard
    a = string(midi, seconds, rng, bright=bright, ring=ring, pick=pick, tilt=1.15 - 0.45 * hard)
    b = string(midi + 0.035 + 0.03 * rng.random(), seconds, rng, bright=bright - 0.1, ring=ring * 0.55, pick=pick * 1.3, tilt=1.2 - 0.4 * hard)
    n = min(len(a), len(b))
    x = a[:n] / (np.abs(a).max() + 1e-9) + second * b[:n] / (np.abs(b).max() + 1e-9)
    x = pickup_tone(x, hz(midi), where, res, q)
    x = x / (np.abs(x).max() + 1e-9)
    # the pick letting go of the string: a few milliseconds of click, brighter when picked hard
    k = int(0.006 * SR)
    click = shape(rng.standard_normal(k * 4), lo=1800, hi=7000)[:k] * np.exp(-np.arange(k) / (0.0012 * SR))
    x[:k] += (0.10 + 0.22 * hard) * click / (np.abs(click).max() + 1e-9)
    x = shape(x, lo=35)
    return fade(x, attack=0.0008, release=0.08)

rng = np.random.default_rng(310)
for kind, top in (('sc', 89), ('hb', 89)):
    for m in range(40, top, 3):
        for layer, hard, peak in (('s', 0.25, 0.55), ('h', 0.9, 0.9)):
            for take in (1, 2):
                name = f'{kind}_{m}_{layer}{take}'
                index[name] = {'file': f'gtr_{name}.wav', 'root': m,
                               'frames': write(f'gtr_{name}.wav', guitar(m, rng, kind, hard), peak)}

# ---- bass: a thick string played with fingers
def bass(midi, rng):
    s = string(midi, 2.2, rng, bright=0.16, ring=1.5, pick=0.3, hard=0.25, tilt=1.5)
    x = shape(s, lo=18, hi=1100)
    x = x / (np.abs(x).max() + 1e-9)
    x = np.tanh(x * 1.8)
    # weight towards the fundamental, with a little finger noise left around 900 Hz
    return fade(peak_eq(shape(x, hi=1800), 900, 1.25), attack=0.003)

rng = np.random.default_rng(304)
for m in range(28, 49, 3):
    for take in (1, 2):
        index[f'bass_{m}_{take}'] = {'file': f'bass_{m}_{take}.wav', 'root': m, 'frames': write(f'bass_{m}_{take}.wav', bass(m, rng), 0.9)}

# ---- drums
def env(n, tau): return np.exp(-np.arange(n) / (tau * SR))
def noise(n, rng, lo=None, hi=None): return shape(rng.standard_normal(n), lo=lo, hi=hi)
def sweep(n, f_from, f_to, tau):
    f = f_to + (f_from - f_to) * env(n, tau)
    return np.sin(2 * np.pi * np.cumsum(f) / SR)

def modes(n, freqs, taus, amps, rng):
    t = np.arange(n) / SR
    return sum(a * np.sin(2 * np.pi * (f * t + rng.random())) * np.exp(-t / tau) for f, tau, a in zip(freqs, taus, amps))

def cymbal(n, rng, base, count, spread, tau, noise_lo, noise_amt, ping=0.0):
    """Many inharmonic partials that die at different rates, over a wash of filtered noise."""
    freqs = base * (1 + spread * np.sort(rng.random(count)) ** 1.3 * 9)
    taus = tau * (0.35 + 0.65 * rng.random(count)) * (base / freqs) ** 0.5
    x = modes(n, freqs, taus, 1 / np.sqrt(np.arange(1, count + 1)), rng)
    x = x / (np.abs(x).max() + 1e-9)
    w = noise(n, rng, lo=noise_lo) * env(n, tau * 0.6)
    x = x + noise_amt * w / (np.abs(w).max() + 1e-9)
    if ping:
        k = int(0.004 * SR)
        x[:k] += ping * rng.standard_normal(k) * np.linspace(1, 0, k)
    return shape(x, lo=noise_lo * 0.5, hi=11000)

rng = np.random.default_rng(305)
kit = {}
n = int(0.5 * SR)
kit['kick'] = np.tanh((sweep(n, 165, 47, 0.035) * env(n, 0.16) + 0.5 * noise(n, rng, lo=1800, hi=6000) * env(n, 0.004)) * 1.6)
n = int(0.42 * SR)
kit['snare'] = np.tanh((modes(n, [186, 331, 512], [0.09, 0.07, 0.04], [1.0, 0.6, 0.3], rng)
                        + 0.8 * noise(n, rng, lo=1100, hi=7000) * env(n, 0.11)
                        + 0.45 * noise(n, rng, lo=3000, hi=9000) * env(n, 0.012)) * 1.5)
n = int(0.12 * SR)
kit['rim'] = modes(n, [520, 1730, 2900], [0.02, 0.025, 0.012], [0.8, 1.0, 0.6], rng) + 0.7 * noise(n, rng, lo=2500) * env(n, 0.003)
n = int(0.14 * SR)
kit['hat'] = cymbal(n, rng, 2700, 28, 0.42, 0.035, 3800, 0.8)
n = int(0.75 * SR)
kit['hat_open'] = cymbal(n, rng, 2700, 28, 0.42, 0.26, 3600, 0.8)
n = int(0.9 * SR)
kit['ride'] = cymbal(n, rng, 2100, 40, 0.5, 0.55, 2600, 0.25, ping=0.9)
n = int(1.0 * SR)
kit['bell'] = 0.8 * modes(n, [742, 1108, 1563, 2210, 3120], [0.6, 0.5, 0.4, 0.3, 0.2], [1.0, 0.8, 0.6, 0.4, 0.3], rng) + 0.35 * cymbal(n, rng, 2600, 24, 0.5, 0.45, 3000, 0.2, ping=0.8)
n = int(2.0 * SR)
kit['crash'] = cymbal(n, rng, 1500, 60, 0.6, 1.1, 1400, 1.0, ping=0.5) * np.minimum(1, np.arange(n) / (0.004 * SR))
n = int(1.4 * SR)
kit['china'] = np.tanh(cymbal(n, rng, 1100, 50, 0.75, 0.7, 1000, 1.1, ping=0.6) * 1.8)
for name, f in (('tom_hi', 196), ('tom_mid', 147), ('tom_lo', 104)):
    n = int(0.55 * SR)
    kit[name] = np.tanh((sweep(n, f * 1.5, f, 0.03) * env(n, 0.2) + 0.3 * sweep(n, f * 2.4, f * 1.6, 0.03) * env(n, 0.08)
                         + 0.35 * noise(n, rng, lo=900, hi=5000) * env(n, 0.012)) * 1.4)
for name, x in kit.items():
    index[f'drum_{name}'] = {'file': f'drum_{name}.wav', 'root': 60, 'frames': write(f'drum_{name}.wav', fade(x, 0.0005, 0.15 if len(x) > SR * 0.8 else 0.02), 0.9)}

json.dump(index, open(os.path.join(HERE, 'samples.json'), 'w'), indent=1)
total = sum(v['frames'] for v in index.values())
print(f'{len(index)} samples, {total / SR:.1f} s, {total * 2 / 1e6:.1f} MB')

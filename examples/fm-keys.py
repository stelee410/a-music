#!/usr/bin/env python3
"""Writes fm-keys.src/song.json: sixteen bars whose electric piano is played by a VST 3 plug-in (Dexed, a free
FM synthesizer) when the listener has it, and by a built-in sound when not. FORMAT.md §8.6."""
import json, os

PPQ, BAR = 960, 3840
N = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}


def midi(name):
    acc = name.count("#") - name.count("b")
    return 12 * (int(name[-1]) + 1) + N[name[0]] + acc


def dexed_epiano():
    """The plug-in's own parameters, by title, normalised 0…1 (`agb plugin info Dexed` lists them)."""
    p = {"ALGORITHM": (5 - 1) / 31, "FEEDBACK": 6 / 7, "OSC KEY SYNC": 1.0, "LFO PM DEPTH": 0.0, "LFO AM DEPTH": 0.0}
    # per operator: rates, levels, output, ratio, detune (−7…7), velocity sensitivity (0…7)
    ops = {
        1: ([96, 25, 25, 67], [99, 75, 0, 0], 99, 1, 3, 2),
        2: ([95, 50, 35, 78], [99, 75, 0, 0], 58, 14, 0, 7),  # the tine: a short bright partial
        3: ([95, 20, 20, 50], [99, 95, 0, 0], 99, 1, 0, 2),
        4: ([95, 29, 20, 50], [99, 95, 0, 0], 89, 1, 0, 6),
        5: ([95, 20, 20, 50], [99, 95, 0, 0], 99, 1, -7, 0),
        6: ([95, 29, 20, 50], [99, 95, 0, 0], 79, 1, 7, 6),
    }
    for n, (rates, levels, out, coarse, detune, vel) in ops.items():
        for k in range(4):
            p[f"OP{n} EG RATE {k + 1}"] = rates[k] / 99
            p[f"OP{n} EG LEVEL {k + 1}"] = levels[k] / 99
        p[f"OP{n} OUTPUT LEVEL"] = out / 99
        p[f"OP{n} F COARSE"] = coarse / 31
        p[f"OP{n} F FINE"] = 0.0
        p[f"OP{n} OSC DETUNE"] = (detune + 7) / 14
        p[f"OP{n} KEY VELOCITY"] = vel / 7
    return {k: round(v, 4) for k, v in p.items()}


CHORDS = [  # root for the bass, voicing for the keys
    ("D2", ["F3", "A3", "C4", "E4"]),   # Dm9
    ("G1", ["F3", "A3", "B3", "E4"]),   # G13
    ("C2", ["E3", "G3", "B3", "D4"]),   # Cmaj9
    ("A1", ["G3", "C#4", "F4"]),        # A7b13
]
MELODY = [  # (bar, beat, pitch, beats) over the second half
    (8, 0.0, "A4", 1.5), (8, 2.5, "C5", 0.5), (8, 3.0, "E5", 1.0),
    (9, 0.5, "D5", 1.0), (9, 2.0, "B4", 2.0),
    (10, 0.0, "G4", 1.5), (10, 2.5, "B4", 0.5), (10, 3.0, "D5", 1.0),
    (11, 0.5, "C#5", 1.5), (11, 2.5, "A4", 1.5),
    (12, 0.0, "E5", 1.0), (12, 1.5, "F5", 0.5), (12, 2.0, "E5", 1.0), (12, 3.5, "C5", 0.5),
    (13, 0.0, "B4", 2.0), (13, 2.5, "A4", 1.5),
    (14, 0.0, "G4", 1.0), (14, 1.5, "E4", 0.5), (14, 2.0, "D5", 2.0),
    (15, 0.0, "C#5", 1.0), (15, 1.5, "F5", 2.5),
]

keys, bass = [], []
for bar in range(16):
    root, voicing = CHORDS[bar % 4]
    t = bar * BAR
    for at, beats, vel in ((0.0, 2.25, 0.72), (2.5, 1.25, 0.58)):
        for i, n in enumerate(voicing):
            keys.append({"tick": t + int(at * PPQ) + i * 14, "pitch": midi(n), "durTicks": int(beats * PPQ), "velocity": round(vel - 0.03 * i, 2)})
    for at, beats, vel in ((0.0, 1.5, 0.85), (2.5, 0.5, 0.7), (3.0, 1.0, 0.8)):
        bass.append({"tick": t + int(at * PPQ), "pitch": midi(root) + (12 if at == 2.5 else 0), "durTicks": int(beats * PPQ * 0.9), "velocity": vel})
for bar, beat, n, beats in MELODY:
    keys.append({"tick": bar * BAR + int(beat * PPQ), "pitch": midi(n), "durTicks": int(beats * PPQ * 0.95), "velocity": 0.84})
keys.sort(key=lambda n: (n["tick"], n["pitch"]))

song = {
    "format": "agbsong",
    "formatVersion": "0.1.0",
    "meta": {
        "title": "FM Keys",
        "artists": [{"name": "Agent Groovebox", "kind": "ai"}],
        "description": "Sixteen bars for a plug-in: the electric piano is Dexed (VST 3) where it is installed, a built-in sound elsewhere.",
        "generator": {"name": "examples/fm-keys.py", "version": "1"},
    },
    "requires": ["core"],
    "uses": ["plugin"],
    "tuning": {"a4Hz": 440, "key": "C", "scale": "major"},
    "time": {"bpm": 92, "ppqn": PPQ, "stepsPerQuarter": 4, "lengthTicks": 16 * BAR},
    "master": {"gainDb": -1.5},
    "patches": {
        "epiano": {
            "engine": "plugin.vst3",
            "plugin": {"name": "Dexed", "vendor": "Digital Suburban", "url": "https://asb2m10.github.io/dexed/", "params": dexed_epiano()},
            "fallback": [{"preset": "preset:synth.waveform:keys:sine_keys@v1.0"}],
        },
        "bass": {"engine": "synth.subtractive", "params": {"amp.releaseMs": 140, "filter.cutoffHz": 520}},
        "kit": {"preset": "preset:drum.analog808style:drums:classic_kit@v1.0"},
    },
    "tracks": [
        {"id": "keys", "role": "chords", "patch": "epiano", "gainDb": -8,
         "fx": [{"type": "reverb", "params": {"decaySec": 1.4, "mix": 0.16}}],
         "clips": [{"atTick": 0, "lengthTicks": 16 * BAR, "notes": keys}]},
        {"id": "bass", "role": "bass", "patch": "bass", "gainDb": 0, "clips": [{"atTick": 0, "lengthTicks": 16 * BAR, "notes": bass}]},
        {"id": "drums", "role": "drums", "patch": "kit", "gainDb": -2, "loops": [
            {"id": "kick", "lengthSteps": 16, "notes": [{"step": s, "pitch": 36, "durSteps": 1, "velocity": v} for s, v in ((0, 0.9), (7, 0.6), (10, 0.8))]},
            {"id": "snare", "lengthSteps": 16, "notes": [{"step": s, "pitch": 38, "durSteps": 1, "velocity": 0.7} for s in (4, 12)]},
            {"id": "hat", "lengthSteps": 16, "notes": [{"step": s, "pitch": 42, "durSteps": 1, "velocity": 0.45 if s % 4 else 0.3} for s in range(0, 16, 2)]},
        ]},
    ],
}
song["uses"] = ["fx", "plugin"]
here = os.path.dirname(os.path.abspath(__file__))
json.dump(song, open(os.path.join(here, "fm-keys.src", "song.json"), "w"), indent=1, ensure_ascii=False)
print("wrote song.json:", len(keys), "key notes")

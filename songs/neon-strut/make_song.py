#!/usr/bin/env python3
"""Writes neon-strut.src/song.json — 《霓虹快步 Neon Strut》, an original funk tune in Bb major.

    python3 songs/neon-strut/make_song.py
    agb song pack songs/neon-strut/neon-strut.src --out songs/neon-strut.agbsong

Follows AUTHORING.md: one generator per song, fixed seed, sections declared once in FORM
and shared with make_video.py through form.json.
"""
import hashlib, json, os, random

random.seed(104)
HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, 'neon-strut.src')
Q = 960
S16 = Q // 4          # one sixteenth
BAR = 4 * Q
BPM = 104

# ---------------------------------------------------------------- form
FORM = [  # id, name, bars, intent
    ('intro',  '引子',     4,  '鼓的过门之后，拨弦键盘独自亮出两小节的律动，踩镲陪着'),
    ('a1',     '律动',     8,  '贝斯和整套鼓进来，降 B 六九和弦到降 E 九和弦的两小节循环'),
    ('a2',     '律动加厚', 8,  '电钢琴垫和弦，铜管在每个偶数小节的后半回答，邦戈鼓加入'),
    ('b1',     '副歌',     8,  '转到下属和弦，合成主音唱旋律，铃鼓推着走，最后两小节 ii–V 回主'),
    ('a3',     '律动再现', 8,  '回到循环，铜管的句子更长，风琴在高处垫长音'),
    ('break',  '拆解',     8,  '只留贝斯、底鼓四拍和牛铃；后四小节拨弦键盘回来，小军鼓滚奏推向独奏'),
    ('solo',   '风琴独奏', 8,  '风琴在循环上独奏，五声音阶加降三音到三音的滑音'),
    ('open',   '开放段',   8,  '只有节奏组和铜管点缀，留给现场乐手或 AI 即兴；可以反复延长'),
    ('b2',     '副歌再现', 8,  '副歌回来，旋律高处加装饰，铜管叠在旋律后面'),
    ('outro',  '尾声',     8,  '循环再走六小节，全体齐奏切分重音，最后一小节停在降 B 六九和弦上'),
]
START = {}
_b = 0
for sid, _, n, _ in FORM:
    START[sid] = _b
    _b += n
NBARS = _b
END = {sid: START[sid] + n for sid, _, n, _ in FORM}
LAST = NBARS - 1          # the held final chord
HITS = NBARS - 2          # the unison hits

def section_of(bar):
    for sid, _, n, _ in FORM:
        if START[sid] <= bar < START[sid] + n:
            return sid, bar - START[sid]
    raise ValueError(bar)

# ---------------------------------------------------------------- helpers
NAMES = {'C': 0, 'D': 2, 'E': 4, 'F': 5, 'G': 7, 'A': 9, 'B': 11}
def m(name):
    n = NAMES[name[0]]; rest = name[1:]
    if rest[0] == '#': n += 1; rest = rest[1:]
    elif rest[0] == 'b': n -= 1; rest = rest[1:]
    return (int(rest) + 1) * 12 + n

def vel(v, spread=0.04):
    return round(max(0.05, min(1.0, v + random.uniform(-spread, spread))), 2)

class Part:
    """Collects notes as (bar, step, pitch, durSteps, velocity) and cuts them into one clip per section."""
    def __init__(self, human=0):
        self.notes = []
        self.human = human
    def add(self, bar, step, pitch, dur=1.0, v=0.7, spread=0.04):
        if isinstance(pitch, str): pitch = m(pitch)
        tick = bar * BAR + round(step * S16)
        if self.human and tick > 0:
            tick += random.randint(-self.human, self.human)
        self.notes.append((tick, pitch, max(40, round(dur * S16) - 12), vel(v, spread)))
    def chord(self, bar, step, pitches, dur=1.0, v=0.7):
        for p in pitches.split():
            self.add(bar, step, p, dur, v)
    def clips(self):
        out = []
        for sid, _, n, _ in FORM:
            a, b = START[sid] * BAR, (START[sid] + n) * BAR
            ns = sorted((t, p, d, v) for (t, p, d, v) in self.notes if a <= t < b)
            if ns:
                out.append({'atTick': a, 'lengthTicks': b - a,
                            'notes': [{'tick': t - a, 'pitch': p, 'durTicks': d, 'velocity': v} for (t, p, d, v) in ns]})
        return out

# ---------------------------------------------------------------- harmony
# voicings: (keys, bass root, clav double-stop, brass triad)
CH = {
    'Bb69':  ('D3 G3 C4 F4',   'Bb1', 'D4 G4',  'F4 Bb4 D5'),
    'Eb9':   ('Db3 G3 Bb3 F4', 'Eb2', 'Db4 G4', 'G4 Bb4 Db5'),
    'Gm7':   ('F3 Bb3 D4',     'G1',  'D4 F4',  'F4 Bb4 D5'),
    'C9':    ('E3 Bb3 D4',     'C2',  'E4 Bb4', 'G4 Bb4 E5'),
    'Cm7':   ('Eb3 G3 Bb3 D4', 'C2',  'Eb4 G4', 'G4 Bb4 Eb5'),
    'F9sus': ('Eb3 G3 Bb3 C4', 'F1',  'Eb4 G4', 'G4 C5 Eb5'),
    'F9':    ('Eb3 A3 C4 G4',  'F1',  'Eb4 A4', 'A4 C5 Eb5'),
}
CHORUS = ['Eb9', 'Eb9', 'Bb69', 'Bb69', 'Gm7', 'C9', 'Cm7', 'F9sus']
VAMP_SECTIONS = ('intro', 'a1', 'a2', 'a3', 'break', 'solo', 'open', 'outro')

def chord_at(bar):
    sid, i = section_of(bar)
    if sid in ('b1', 'b2'):
        return CHORUS[i]
    return 'Bb69' if i % 2 == 0 else 'Eb9'   # the second bar goes back to Bb on beat 3 (see parts)

# ---------------------------------------------------------------- drums (clips: full control of fills and ghosts)
KICK, RIM, SNARE, CLAP, HAT, OHAT, CRASH = 36, 37, 38, 39, 42, 46, 49
TOMS = [50, 48, 45, 43]
drums = Part()

def groove(bar, kicks, ghosts=(7, 9, 15), hat='16', open_at=(), snare=True, level=1.0, hat_until=16):
    for s in kicks:
        drums.add(bar, s, KICK, 1, (.92 if s in (0, 10) else .74) * level, .03)
    if snare:
        for s in (4, 12):
            drums.add(bar, s, SNARE, 1, .86 * level, .03)
        for s in ghosts:
            if s not in kicks:
                drums.add(bar, s, SNARE, 1, .2, .03)
    for s in range(hat_until):     # the hats stop where a fill starts
        if s in open_at:
            drums.add(bar, s, OHAT, 2, .5, .03)
        elif hat == '16':
            drums.add(bar, s, HAT, 1, (.52 if s % 4 == 0 else .4 if s % 2 == 0 else .26) * level, .03)
        elif hat == '8' and s % 2 == 0:
            drums.add(bar, s, HAT, 1, (.5 if s % 4 == 0 else .36) * level, .03)

def fill(bar, kind):
    if kind == 'toms':      # last two beats: snare into toms
        for i, s in enumerate(range(8, 16)):
            p = SNARE if i < 2 else TOMS[(i - 2) // 2]
            drums.add(bar, s, p, 1, .55 + .05 * i, .03)
    elif kind == 'snare':   # last beat: four sixteenths
        for i, s in enumerate(range(12, 16)):
            drums.add(bar, s, SNARE, 1, .5 + .1 * i, .03)
    elif kind == 'roll':    # whole bar: sixteenths growing
        for s in range(16):
            drums.add(bar, s, SNARE, 1, .25 + .04 * s, .02)
            if s % 4 == 0: drums.add(bar, s, KICK, 1, .85, .02)

K1, K2 = (0, 6, 10), (0, 3, 10, 13)
for bar in range(NBARS):
    sid, i = section_of(bar)
    n = END[sid] - START[sid]
    last = i == n - 1
    if i == 0 and sid not in ('intro', 'break'):
        drums.add(bar, 0, CRASH, 4, .62)
    if sid == 'intro':
        if i == 0:      # pickup: the band is counted in by the kit
            for s, p, v in [(8, SNARE, .5), (10, SNARE, .6), (11, SNARE, .4), (12, TOMS[0], .7), (13, TOMS[1], .7), (14, TOMS[2], .75), (15, TOMS[3], .8)]:
                drums.add(bar, s, p, 1, v)
        else:
            groove(bar, (), hat='8', snare=False, open_at=(14,) if i == 3 else ())
            if i == 3: fill(bar, 'snare')
    elif sid == 'break':
        if i < 7:
            groove(bar, (0, 4, 8, 12), hat='8' if i >= 4 else 'none', snare=False, level=.9)
            if i >= 4:
                for s in (4, 12): drums.add(bar, s, CLAP, 1, .6)
        else:
            fill(bar, 'roll')
    elif bar == HITS:
        for s in (0, 3, 6, 10, 12, 14):
            drums.add(bar, s, KICK, 1, .9); drums.add(bar, s, SNARE, 1, .8)
        drums.add(bar, 15, SNARE, 1, .6)
    elif bar == LAST:
        drums.add(bar, 0, KICK, 1, .95); drums.add(bar, 0, CRASH, 8, .75); drums.add(bar, 0, SNARE, 1, .8)
    else:
        kicks = K1 if i % 2 == 0 else K2
        chorus = sid in ('b1', 'b2')
        open_at = (14,) if i % 2 == 1 else ((6,) if chorus else ())
        filling = last or bar == HITS - 1
        toms = sid in ('a2', 'b1', 'open', 'b2', 'outro')
        groove(bar, kicks, open_at=() if filling else open_at, ghosts=(7,) if filling else (7, 9, 15),
               hat_until=(8 if toms else 12) if filling else 16)
        if chorus:
            drums.add(bar, 4, CLAP, 1, .45); drums.add(bar, 12, CLAP, 1, .45)
        if filling:
            fill(bar, 'toms' if toms else 'snare')
        elif i % 4 == 3:
            drums.add(bar, 14, SNARE, 1, .45); drums.add(bar, 15, SNARE, 1, .6)

# ---------------------------------------------------------------- percussion (loops: the part an AI can swap live)
def lp(lid, steps, a, b, length=16, gain=0.0):
    return {'id': lid, 'lengthSteps': length, 'fromTick': a * BAR, 'toTick': b * BAR, 'gainDb': gain,
            'notes': [{'step': s, 'pitch': p, 'velocity': v} for (s, p, v) in steps]}
COWBELL, TAMB, BONGO_H, BONGO_L = 56, 54, 60, 61
bongo = [(2, BONGO_H, .5), (3, BONGO_H, .34), (6, BONGO_L, .5), (8, BONGO_H, .3), (10, BONGO_H, .5),
         (11, BONGO_L, .4), (14, BONGO_L, .52), (15, BONGO_H, .36)]
tamb = [(s, TAMB, .5 if s % 4 == 2 else .26) for s in range(16)]
cow = [(0, COWBELL, .6), (4, COWBELL, .45), (6, COWBELL, .4), (8, COWBELL, .6), (11, COWBELL, .4), (12, COWBELL, .5), (14, COWBELL, .42)]
perc_loops = [
    lp('bongo', bongo, START['a2'], HITS),
    lp('tamb1', tamb, START['b1'], END['b1'], gain=-3),
    lp('cowbell', cow, START['break'], END['open'], gain=-2),
    lp('tamb2', tamb, START['b2'], END['b2'], gain=-3),
]

# ---------------------------------------------------------------- bass
bass = Part(human=4)
RIFF = [  # two bars: (step, pitch, dur, vel)
    [(0, 'Bb1', 2, .92), (3, 'Bb1', 1, .6), (6, 'Bb2', 1, .82), (7, 'F2', 1, .55), (10, 'G2', 1, .72), (11, 'Bb2', 1, .6), (14, 'Db2', 1, .66), (15, 'D2', 1, .72)],
    [(0, 'Eb2', 2, .92), (3, 'Eb2', 1, .6), (6, 'Eb3', 1, .8), (7, 'Db3', 1, .6), (8, 'Bb2', 1.5, .78), (10, 'G2', 1, .64), (12, 'F2', 2, .72), (14, 'G1', 1, .6), (15, 'A1', 1, .68)],
]
RIFF_ALT = [(0, 'Eb2', 2, .92), (3, 'Eb2', 1, .6), (6, 'G2', 1, .7), (7, 'Bb2', 1, .7), (8, 'Db3', 1, .8), (9, 'C3', 1, .6), (10, 'Bb2', 1, .7), (12, 'G2', 1, .66), (13, 'F2', 1, .6), (14, 'G1', 1, .62), (15, 'A1', 1, .7)]

def chorus_bass(bar, name, nxt):
    r = m(CH[name][1])
    bass.add(bar, 0, r, 2.5, .92)
    bass.add(bar, 3, r, 1, .58)
    bass.add(bar, 6, r + 12, 1, .8)
    bass.add(bar, 8, r + 7, 1.5, .7)
    bass.add(bar, 10, r + 12, 1, .66)
    bass.add(bar, 11, r + 10 if name != 'Bb69' else r + 9, 1, .58)
    bass.add(bar, 12, r, 2, .78)
    target = m(CH[nxt][1])            # walk into the next root from below
    bass.add(bar, 14, target - 2, 1, .6)
    bass.add(bar, 15, target - 1, 1, .68)

for bar in range(NBARS):
    sid, i = section_of(bar)
    if sid == 'intro' and i < 3:
        continue
    if sid == 'intro':                       # pickup into the groove
        for s, p in [(12, 'F1'), (13, 'G1'), (14, 'Ab1'), (15, 'A1')]:
            bass.add(bar, s, p, 1, .7)
    elif sid in ('b1', 'b2'):
        nxt = CHORUS[i + 1] if i < 7 else 'Bb69'
        chorus_bass(bar, CHORUS[i], nxt)
    elif bar == HITS:
        for s, p in [(0, 'Bb1'), (3, 'Bb1'), (6, 'Bb2'), (10, 'G1'), (12, 'Ab1'), (14, 'A1')]:
            bass.add(bar, s, p, 1.5, .92)
    elif bar == LAST:
        bass.add(bar, 0, 'Bb1', 10, .95)
    else:
        pat = RIFF[i % 2]
        if i % 4 == 3 and sid != 'break':
            pat = RIFF_ALT
        for s, p, d, v in pat:
            bass.add(bar, s, p, d, v, .03)

# ---------------------------------------------------------------- clav (sixteenth-note double stops)
clav = Part(human=3)
CLAV_RHYTHM = [  # step, accent
    [(0, 1), (2, 0), (3, 1), (6, 1), (7, 0), (9, 1), (10, 0), (12, 1), (14, 0), (15, 1)],
    [(0, 1), (3, 1), (4, 0), (6, 1), (8, 1), (10, 0), (11, 1), (13, 0), (14, 1)],
]
def clav_bar(bar, name, pat, level=1.0, second_half=None):
    for s, acc in CLAV_RHYTHM[pat]:
        c = second_half if (second_half and s >= 8) else name
        stops = CH[c][2].split()
        if acc:
            for p in stops: clav.add(bar, s, p, .9, .7 * level)
        else:                                  # muted scratch: one low, short, quiet note
            clav.add(bar, s, stops[0], .45, .34 * level)

for bar in range(NBARS):
    sid, i = section_of(bar)
    if sid == 'intro':
        if i >= 1: clav_bar(bar, 'Bb69' if i % 2 == 1 else 'Eb9', (i + 1) % 2, second_half='Bb69' if i % 2 == 0 else None)
    elif sid == 'break':
        if 4 <= i < 7: clav_bar(bar, chord_at(bar), i % 2, level=.8 + .06 * (i - 4), second_half='Bb69' if i % 2 else None)
    elif sid in ('b1', 'b2'):
        clav_bar(bar, CHORUS[i], i % 2, level=.85)
    elif bar == HITS:
        for s in (0, 3, 6, 10, 12, 14):
            clav.chord(bar, s, 'D4 G4', 1.0, .8)
    elif bar == LAST:
        clav.chord(bar, 0, 'D4 G4 C5', 6, .7)
    else:
        clav_bar(bar, chord_at(bar), i % 2, second_half='Bb69' if i % 2 else None)

# ---------------------------------------------------------------- electric piano (comping)
keys = Part(human=5)
for bar in range(NBARS):
    sid, i = section_of(bar)
    if sid in ('intro', 'a1', 'break'):
        continue
    name = chord_at(bar)
    v = CH[name][0]
    if sid in ('a3', 'outro'):          # the brass and organ are busy here: three-note voicings keep us under 32 voices
        v = ' '.join(v.split()[:3])
    if bar == HITS:
        for s in (0, 3, 6, 10, 12, 14): keys.chord(bar, s, 'D3 G3 C4', 1.2, .7)
    elif bar == LAST:
        keys.chord(bar, 0, 'D3 G3 C4 F4', 10, .66)
    elif sid in ('b1', 'b2'):
        keys.chord(bar, 0, v, 5.5, .56)
        keys.chord(bar, 6, v, 1.6, .5)
        keys.chord(bar, 10, v, 3.5 if i < 7 else 1.5, .52)
        if i == 7: keys.chord(bar, 12, CH['F9'][0], 2, .58)   # sus resolves before the turnaround
    elif sid == 'solo' or sid == 'open':
        keys.chord(bar, 0, v, 1.6, .5)
        keys.chord(bar, 6, v, 1.4, .46)
        if i % 2: keys.chord(bar, 8, CH['Bb69'][0], 5, .48)
    else:
        keys.chord(bar, 0, v, 2.6, .52)
        keys.chord(bar, 6, v, 1.5, .48)
        if i % 2:
            keys.chord(bar, 8, CH['Bb69'][0], 2.6, .5)
            keys.chord(bar, 14, CH['Bb69'][0], 1.6, .44)
        else:
            keys.chord(bar, 11, v, 2.5, .46)

# ---------------------------------------------------------------- brass
brass = Part(human=3)
def unison(bar, line, octave_below=True, level=1.0):
    for s, p, d, v in line:
        brass.add(bar, s, p, d, v * level)
        if octave_below: brass.add(bar, s, m(p) - 12, d, v * level * .85)

ANSWER  = [(8, 'F4', 1, .7), (10, 'G4', 1, .72), (11, 'Bb4', 1, .76), (13, 'G4', 1, .66), (14, 'Bb4', 2, .8)]
ANSWER2 = [(8, 'D5', 1, .76), (9, 'C5', 1, .66), (10, 'Bb4', 1, .72), (12, 'G4', 1, .68), (13, 'Bb4', 1, .7), (14, 'C5', 1, .74), (15, 'D5', 1, .8)]
LONG    = [(0, 'F4', 1, .72), (2, 'G4', 1, .7), (3, 'Bb4', 2, .78), (6, 'C5', 1, .72), (7, 'Db5', 1, .7), (8, 'D5', 3, .84), (12, 'Bb4', 1, .7), (13, 'G4', 1, .66), (14, 'F4', 2, .74)]

for bar in range(NBARS):
    sid, i = section_of(bar)
    name = chord_at(bar)
    if sid == 'a2':
        if i % 2 == 0: brass.chord(bar, 0, CH[name][3], 1, .7)
        else: unison(bar, ANSWER if i % 4 == 1 else ANSWER2)
    elif sid == 'a3':
        if i % 4 == 0: unison(bar, LONG)
        elif i % 4 == 1: unison(bar, ANSWER2)
        elif i % 4 == 2: brass.chord(bar, 0, CH[name][3], 1, .74); brass.chord(bar, 6, CH[name][3], 1, .66)
        else: unison(bar, ANSWER)
    elif sid == 'b1':
        if i in (3, 7): brass.chord(bar, 8, CH[name][3], 1, .66); brass.chord(bar, 11, CH[name][3], 1, .6); brass.chord(bar, 14, CH['F9' if i == 7 else name][3], 2, .74)
    elif sid == 'b2':
        brass.chord(bar, 0, CH[name][3], 5, .5)                 # pad behind the tune
        if i % 2: brass.chord(bar, 10, CH[name][3], 1, .62); brass.chord(bar, 14, CH['F9' if i == 7 else name][3], 2, .72)
    elif sid == 'solo':
        if i % 4 == 3: brass.chord(bar, 12, CH['Bb69'][3], 1, .6); brass.chord(bar, 14, CH['Bb69'][3], 2, .7)
    elif sid == 'open':
        if i % 2 == 0: brass.chord(bar, 0, CH[name][3], 1, .6 + .03 * i)
        if i >= 4 and i % 2: unison(bar, ANSWER, level=.8 + .05 * (i - 4))
    elif sid == 'outro':
        if bar == HITS:
            for s in (0, 3, 6): brass.chord(bar, s, 'F4 Bb4 D5', 1.2, .84)
            for s, c in [(10, 'D4 G4 Bb4'), (12, 'Eb4 Ab4 C5'), (14, 'E4 A4 Db5')]: brass.chord(bar, s, c, 1.0, .86)
        elif bar == LAST:
            brass.chord(bar, 0, 'F4 Bb4 D5', 10, .82)
        elif i % 2 == 0: unison(bar, LONG)
        else: unison(bar, ANSWER2)

# ---------------------------------------------------------------- lead (chorus tune)
lead = Part(human=4)
TUNE = [
    [(2, 'G4', 2, .7), (4, 'Bb4', 2, .72), (6, 'Db5', 3, .8), (9, 'C5', 1, .62), (10, 'Bb4', 2, .7), (12, 'G4', 3.5, .68)],
    [(2, 'F4', 2, .66), (4, 'G4', 2, .7), (6, 'Bb4', 5.5, .76), (12, 'C5', 2, .72), (14, 'Db5', 1, .7), (15, 'D5', 1, .78)],
    [(0, 'D5', 6, .82), (6, 'C5', 2, .7), (8, 'Bb4', 2, .7), (10, 'G4', 2, .66), (12, 'F4', 3.5, .68)],
    [(2, 'F4', 1, .6), (3, 'G4', 1, .64), (4, 'Bb4', 4, .74), (8, 'G4', 2, .66), (10, 'F4', 2, .64), (12, 'D4', 3, .62)],
    [(0, 'D5', 2, .78), (2, 'D5', 1, .62), (3, 'F5', 3, .82), (6, 'D5', 2, .72), (8, 'Bb4', 4, .72), (12, 'G4', 2, .66), (14, 'Bb4', 2, .7)],
    [(0, 'E5', 6, .84), (6, 'D5', 2, .72), (8, 'C5', 2, .72), (10, 'Bb4', 2, .7), (12, 'G4', 2, .66), (14, 'Bb4', 2, .7)],
    [(0, 'Eb5', 2, .8), (2, 'D5', 2, .72), (4, 'C5', 2, .72), (6, 'Bb4', 2, .7), (8, 'G4', 2, .66), (10, 'Bb4', 2, .7), (12, 'C5', 3.5, .76)],
    [(0, 'F5', 7, .86), (8, 'Eb5', 2, .74), (10, 'C5', 2, .7), (12, 'A4', 2, .7), (14, 'C5', 2, .74)],
]
TUNE2 = [list(b) for b in TUNE]   # the return: lifted and decorated
TUNE2[2] = [(0, 'D5', 3, .82), (3, 'F5', 3, .86), (6, 'G5', 2, .86), (8, 'F5', 2, .78), (10, 'D5', 2, .72), (12, 'Bb4', 3.5, .7)]
TUNE2[4] = [(0, 'D5', 2, .8), (2, 'F5', 1, .7), (3, 'G5', 3, .88), (6, 'F5', 2, .76), (8, 'D5', 2, .74), (10, 'Bb4', 2, .7), (12, 'G4', 2, .66), (14, 'Bb4', 2, .7)]
TUNE2[7] = [(0, 'F5', 4, .86), (4, 'G5', 2, .84), (6, 'A5', 2, .86), (8, 'C6', 4, .9), (12, 'A5', 1, .76), (13, 'G5', 1, .74), (14, 'F5', 1, .74), (15, 'C5', 1, .7)]
for i, b in enumerate(TUNE):
    for s, p, d, v in b: lead.add(START['b1'] + i, s, p, d, v, .03)
for i, b in enumerate(TUNE2):
    for s, p, d, v in b: lead.add(START['b2'] + i, s, p, d, v, .03)
lead.add(LAST, 0, 'Bb5', 9, .8)

# ---------------------------------------------------------------- organ (pad in a3, solo, last chord)
organ = Part(human=5)
for i in range(8):
    b = START['a3'] + i
    organ.chord(b, 0, 'F5 Bb5' if i % 2 == 0 else 'G5 Bb5', 15, .4)
SOLO = [
    [(2, 'F4', 1, .7), (3, 'G4', 1, .7), (4, 'Bb4', 2, .78), (6, 'Db5', 1, .7), (7, 'D5', 3, .84), (10, 'Bb4', 1, .7), (11, 'G4', 1, .66), (12, 'F4', 1, .66), (13, 'G4', 1, .68), (14, 'Bb4', 2, .76)],
    [(0, 'Db5', 1, .72), (1, 'D5', 1, .7), (2, 'F5', 2, .82), (4, 'G5', 3, .86), (7, 'F5', 1, .72), (8, 'D5', 2, .76), (10, 'Bb4', 2, .7), (12, 'G4', 4, .7)],
    [(0, 'Bb4', 1, .7), (1, 'C5', 1, .7), (2, 'D5', 1, .74), (3, 'F5', 1, .76), (4, 'G5', 1, .8), (5, 'F5', 1, .72), (6, 'D5', 1, .72), (7, 'F5', 1, .76), (8, 'G5', 1, .8), (9, 'Bb5', 3, .9), (12, 'G5', 1, .76), (13, 'F5', 1, .72), (14, 'D5', 2, .74)],
    [(0, 'Db5', 2, .76), (2, 'Bb4', 1, .7), (3, 'G4', 1, .66), (4, 'Bb4', 4, .74), (10, 'F5', 1, .74), (11, 'G5', 1, .78), (12, 'Bb5', 1, .84), (13, 'G5', 1, .76), (14, 'Bb5', 2, .86)],
    [(0, 'D6', 4, .92), (4, 'C6', 1, .78), (5, 'Bb5', 1, .76), (6, 'G5', 2, .78), (8, 'Bb5', 1, .8), (9, 'G5', 1, .74), (10, 'F5', 1, .74), (11, 'D5', 1, .7), (12, 'F5', 2, .76), (14, 'G5', 2, .78)],
    [(0, 'Db6', 1, .82), (1, 'D6', 2, .88), (3, 'Bb5', 1, .76), (4, 'Db6', 1, .82), (5, 'D6', 2, .88), (7, 'Bb5', 1, .76), (8, 'Db6', 1, .84), (9, 'D6', 2, .9), (11, 'F6', 1, .9), (12, 'D6', 1, .82), (13, 'Bb5', 1, .78), (14, 'G5', 2, .76)],
    [(0, 'F5', 1, .74), (2, 'F5', 1, .7), (3, 'G5', 1, .74), (4, 'Bb5', 1, .8), (6, 'Bb5', 1, .74), (7, 'C6', 1, .78), (8, 'D6', 1, .84), (10, 'D6', 1, .78), (11, 'F6', 1, .86), (12, 'G6', 4, .94)],
    [(0, 'F6', 1, .86), (1, 'D6', 1, .8), (2, 'Bb5', 1, .78), (3, 'G5', 1, .74), (4, 'F5', 1, .72), (5, 'D5', 1, .7), (6, 'Bb4', 1, .7), (7, 'G4', 1, .66), (8, 'F4', 1, .66), (9, 'G4', 1, .68), (10, 'Bb4', 1, .72), (11, 'Db5', 1, .72), (12, 'D5', 4, .8)],
]
for i, b in enumerate(SOLO):
    for s, p, d, v in b: organ.add(START['solo'] + i, s, p, d, v, .03)
organ.chord(LAST, 0, 'G4 C5 F5', 10, .6)

# ---------------------------------------------------------------- song.json
def sha(rel):
    path = os.path.join(SRC, rel)
    if not os.path.exists(path):      # first pass: the video is rendered from this script's output
        print(f'note: {rel} does not exist yet; run make_video.py, then this script again')
        return 'sha256:' + '0' * 64
    return 'sha256:' + hashlib.sha256(open(path, 'rb').read()).hexdigest()

def preset(kind, name):
    return f'preset:{kind}:{name}@v1.0'

sections = [{'id': sid, 'name': name, 'fromTick': START[sid] * BAR, 'toTick': END[sid] * BAR, 'intent': intent}
            for sid, name, _, intent in FORM]

song = {
    'format': 'agbsong', 'formatVersion': '0.1.0',
    'requires': ['core'], 'uses': ['agent', 'visuals'],
    'meta': {
        'title': '霓虹快步 Neon Strut',
        'artists': [{'name': 'Claude', 'kind': 'ai', 'model': 'claude-fable-5-2'}],
        'description': '原创 funk，降 B 大调，104 BPM。两小节的十六分律动（降 B 六九 → 降 E 九），副歌走 IV–I–vi–II–ii–V；中段有拆解、风琴独奏和一个留给现场即兴的开放段。',
        'tags': ['funk', 'groove', 'live'],
        'created': '2026-11-14T00:00:00Z', 'license': 'CC-BY-4.0',
        'generator': {'name': 'agb', 'version': '0.1.0'}, 'cover': 'cover'},
    'time': {'ppqn': Q, 'bpm': BPM, 'meter': [4, 4], 'stepsPerQuarter': 4, 'lengthTicks': NBARS * BAR},
    'tuning': {'a4Hz': 440, 'key': 'Bb', 'scale': 'major'},
    'seed': 104,
    'assets': {
        'cover': {'path': 'cover.svg', 'kind': 'image', 'sha256': sha('cover.svg')},
        'video': {'path': 'visuals/neon-strut.mp4', 'kind': 'video', 'sha256': sha('visuals/neon-strut.mp4'),
                  'license': {'spdx': 'CC-BY-4.0', 'redistributable': True,
                              'source': '程序生成的动画（songs/neon-strut/make_video.py），按曲式逐小节绘制'}}},
    'patches': {
        'kit':   {'preset': preset('drum.analog808style', 'drums:tight_kit')},
        'perc':  {'preset': preset('drum.analog808style', 'drums:classic_kit')},
        'bass':  {'preset': preset('synth.moogish', 'bass:fat_ladder_bass'), 'params': {'amp.releaseMs': 70}},
        'clav':  {'preset': preset('synth.subtractive', 'pluck:bright_pluck'), 'params': {'amp.releaseMs': 60}},
        'keys':  {'preset': preset('synth.subtractive', 'keys:ep_keys')},
        'brass': {'preset': preset('synth.moogish', 'lead:ladder_brass')},
        'lead':  {'preset': preset('synth.subtractive', 'lead:soft_lead')},
        'organ': {'preset': preset('synth.subtractive', 'keys:organ')},
    },
    'tracks': [
        {'id': 'drums', 'name': '鼓', 'role': 'drums', 'patch': 'kit', 'gainDb': 0, 'transposable': False, 'clips': drums.clips()},
        {'id': 'perc', 'name': '打击乐', 'role': 'percussion', 'patch': 'perc', 'gainDb': -2, 'transposable': False, 'loops': perc_loops},
        {'id': 'bass', 'name': '贝斯', 'role': 'bass', 'patch': 'bass', 'gainDb': -3, 'clips': bass.clips()},
        {'id': 'clav', 'name': '拨弦键盘', 'role': 'rhythm', 'patch': 'clav', 'gainDb': -7, 'clips': clav.clips()},
        {'id': 'keys', 'name': '电钢琴', 'role': 'keys', 'patch': 'keys', 'gainDb': -9, 'clips': keys.clips()},
        {'id': 'brass', 'name': '铜管', 'role': 'brass', 'patch': 'brass', 'gainDb': -5, 'clips': brass.clips()},
        {'id': 'lead', 'name': '合成主音', 'role': 'lead', 'patch': 'lead', 'gainDb': -4, 'clips': lead.clips()},
        {'id': 'organ', 'name': '风琴', 'role': 'solo', 'patch': 'organ', 'gainDb': -4, 'clips': organ.clips()},
    ],
    'master': {'gainDb': -5, 'limiter': 'soft'},
    'sections': sections,
    'visuals': {'palette': ['#0b0620', '#ff2e88', '#19e3ff', '#ffd23f'], 'scene': 'spectrum',
                'video': {'asset': 'video', 'sync': 'song'},
                'bindings': [{'source': 'track.drums.level', 'target': 'scene.pulse', 'scale': 0.8},
                             {'source': 'track.brass.noteOn', 'target': 'scene.flash'}]},
    'performance': {
        'transpose': {'min': -5, 'max': 6}, 'bpm': {'min': 88, 'max': 120},
        'agent': {'brief': '降 B 大调的 funk，104 BPM，十六分律动；两小节循环是降 B 六九到降 E 九',
                  'persona': '一支紧凑的 funk 乐队：重拍在一，音符短，留空拍',
                  'rules': ['贝斯每小节第一拍必须落在根音上', '十六分音符不摇摆，靠力度区分重音和闷音',
                            '即兴用降 B 大调五声音阶，可以用降三音滑到三音', '开放段（open）可以自由发挥，其余段落保留写好的主题',
                            '不要让和弦乐器同时铺满，留出贝斯和鼓的位置'],
                  'allow': ['notes', 'tempo', 'transpose'], 'sectionsAreFixed': True}},
}

os.makedirs(SRC, exist_ok=True)
with open(os.path.join(SRC, 'song.json'), 'w') as f:
    json.dump(song, f, ensure_ascii=False, indent=1)
    f.write('\n')
SHOW = {'Bb69': 'Bb6/9', 'F9sus': 'F9sus4'}
def chords_of(bar):
    sid, i = section_of(bar)
    name = chord_at(bar)
    if bar >= HITS:
        return [[0, 'Bb6/9']]
    if sid not in ('b1', 'b2') and i % 2 == 1:
        return [[0, 'Eb9'], [2, 'Bb6/9']]
    if sid in ('b1', 'b2') and i == 7:
        return [[0, 'F9sus4'], [3, 'F9']]
    return [[0, SHOW.get(name, name)]]
with open(os.path.join(HERE, 'form.json'), 'w') as f:   # read by make_video.py
    json.dump({'bpm': BPM, 'bars': NBARS, 'hitsBar': HITS, 'lastBar': LAST,
               'sections': [[sid, name, START[sid], END[sid] - START[sid]] for sid, name, _, _ in FORM],
               'chords': [chords_of(b) for b in range(NBARS)]}, f, ensure_ascii=False)
    f.write('\n')
print(f'{NBARS} bars, {NBARS * 4 * 60 / BPM:.1f} s; notes per track:',
      {t['id']: sum(len(c['notes']) for c in t.get('clips', [])) for t in song['tracks']})

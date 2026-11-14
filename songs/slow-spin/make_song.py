#!/usr/bin/env python3
"""Writes slow-spin.src/song.json — 《午夜慢转 Slow Spin》, an original jazz hip-hop tune in F minor.

    python3 songs/slow-spin/make_song.py
    agb song pack songs/slow-spin/slow-spin.src --out songs/slow-spin.agbsong

Follows AUTHORING.md: one generator per song, fixed seed, sections declared once in FORM
and shared with make_video.py through form.json.

Two deliberate choices, both part of the style:
- Sixteenths are swung: the second sixteenth of every eighth sits at 60 % instead of 50 %.
  Clips are written in ticks, so the swing is exact; loops stay on the eighth-note grid.
- The backbeat snare is 14 ticks late on every hit (a fixed, lazy placement, not random jitter).
"""
import hashlib, json, os, random

random.seed(86)
HERE = os.path.dirname(os.path.abspath(__file__))
SLUG = 'slow-spin'
SRC = os.path.join(HERE, SLUG + '.src')
Q = 960
BAR = 4 * Q
BPM = 86
SWING = [0, 290, 480, 770, 960]     # where the four sixteenths of a beat fall
SNARE_LATE = 14

# ---------------------------------------------------------------- form
FORM = [  # id, name, bars, intent
    ('intro', '引子',       4, '鼓边数四拍，电钢琴独自走一遍四小节的和弦，踩镲轻轻陪着，最后一拍军鼓带入'),
    ('a1',    '律动',       8, '鼓和贝斯进来，摇摆的十六分；后四小节吉他在二四拍加短和弦，颤音琴在句尾点两下'),
    ('a2',    '主题',       8, '合成小号吹主题，句子靠后、留空拍；沙锤加入'),
    ('b1',    '桥段',       8, '从降 D 大九和弦开始往下走，颤音琴接过旋律，长音铺底，叮叮镲打四分；最后两小节 ii–V 回 F 小调'),
    ('a3',    '主题再现',   8, '小号再吹主题，颤音琴在长音和空拍里回答'),
    ('break', '停顿',       4, '鼓只剩底鼓和鼓边，电钢琴和贝斯拉长音；最后半小节通鼓过门推向独奏'),
    ('solo',  '颤音琴独奏', 8, '颤音琴在和弦循环上独奏，F 小调多利亚加蓝调音，后四小节往高处走'),
    ('open',  '开放段',     8, '只有节奏组，留给现场乐手或 AI 即兴；邦戈鼓加入，可以反复延长'),
    ('a4',    '主题终',     8, '主题最后一次，后半句升高，颤音琴高八度叠旋律，长音铺底'),
    ('outro', '尾声',       4, '小号吹三小节的收束句，鼓逐小节变薄，最后一小节停在 F 小九和弦上'),
]
START = {}
_b = 0
for sid, _, n, _ in FORM:
    START[sid] = _b
    _b += n
NBARS = _b
END = {sid: START[sid] + n for sid, _, n, _ in FORM}
LAST = NBARS - 1          # the held final chord

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

def swung(step):
    """Tick of a position given in sixteenths from the bar line, with the swing applied."""
    beat, sub = divmod(step, 4)
    i = int(sub)
    return round(beat * Q + SWING[i] + (sub - i) * (SWING[i + 1] - SWING[i]))

class Part:
    """Collects notes as (bar, step, pitch, durSteps, velocity) and cuts them into one clip per section."""
    def __init__(self, human=0):
        self.notes = []
        self.human = human
    def add(self, bar, step, pitch, dur=1.0, v=0.7, spread=0.04, late=0):
        if isinstance(pitch, str): pitch = m(pitch)
        tick = bar * BAR + swung(step) + late
        if self.human and tick > 0:
            tick += random.randint(-self.human, self.human)
        self.notes.append((tick, pitch, max(40, round(dur * Q / 4) - 12), vel(v, spread)))
    def chord(self, bar, step, pitches, dur=1.0, v=0.7):
        for p in pitches.split():
            self.add(bar, step, p, dur, v)
    def line(self, bar, notes, level=1.0, up=0):
        for s, p, d, v in notes:
            self.add(bar, s, m(p) + up, d, v * level, .03)
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
# name: (keys voicing, bass root, guitar dyad, pad dyad, interval of the seventh)
CH = {
    'Fm9':    ('Ab3 C4 Eb4 G4',  'F2',  'Eb4 Ab4', 'Ab4 Eb5', 10),
    'Dbmaj9': ('F3 Ab3 C4 Eb4',  'Db2', 'C4 F4',   'Ab4 Eb5', 11),
    'Bbm9':   ('Ab3 C4 Db4 F4',  'Bb1', 'Db4 Ab4', 'Ab4 Db5', 10),
    'Eb13':   ('G3 C4 Db4 F4',   'Eb2', 'Db4 G4',  'G4 Db5',  10),
    'Abmaj9': ('G3 Bb3 C4 Eb4',  'Ab1', 'C4 G4',   'G4 C5',   11),
    'C7#9':   ('E3 Bb3 Eb4 Ab4', 'C2',  'E4 Bb4',  'E4 Bb4',  10),
    'Cm9':    ('Eb3 G3 Bb3 D4',  'C2',  'Eb4 Bb4', 'G4 D5',   10),
    'Gm7b5':  ('F3 Bb3 Db4 G4',  'G2',  'Db4 F4',  'F4 Db5',  10),
}
CYCLE = [['Fm9'], ['Dbmaj9'], ['Bbm9', 'Eb13'], ['Abmaj9', 'C7#9']]
BRIDGE = [['Dbmaj9'], ['Dbmaj9'], ['Cm9'], ['Fm9'], ['Bbm9'], ['Eb13'], ['Gm7b5'], ['C7#9']]
OUTRO = [['Fm9'], ['Dbmaj9'], ['Bbm9', 'C7#9'], ['Fm9']]

def chords(bar):
    """The chords of a bar: one for the whole bar, or two that change on beat 3."""
    sid, i = section_of(bar)
    if sid == 'b1': return BRIDGE[i]
    if sid == 'outro': return OUTRO[i]
    return CYCLE[i % 4]

def chord_at(bar, step):
    cs = chords(bar)
    return cs[1] if len(cs) == 2 and step >= 8 else cs[0]

def first_root(bar):
    return m(CH[chords(bar)[0]][1]) if bar < NBARS else m('F2')

# ---------------------------------------------------------------- drums (clips: swing, ghosts and fills need exact control)
KICK, RIM, SNARE, HAT, OHAT, CRASH, RIDE = 36, 37, 38, 42, 46, 49, 51
TOMS = [50, 48, 45, 43]
drums = Part()

def snare(bar, s, v):
    drums.add(bar, s, SNARE, 1, v, .03, late=SNARE_LATE)

def groove(bar, kicks, ghosts=(9, 15), open_at=(), hat_until=16, backbeat=True):
    for s in kicks:
        drums.add(bar, s, KICK, 1, .9 if s == 0 else .76, .03)
    if backbeat:
        for s in (4, 12): snare(bar, s, .84)
        for s in ghosts:
            if s not in kicks: snare(bar, s, .2)
    for s in range(hat_until):
        if s in open_at:
            drums.add(bar, s, OHAT, 2, .44, .03)
        elif s % 2 == 0:
            drums.add(bar, s, HAT, 1, .48 if s % 4 == 0 else .34, .03)
        elif s in (3, 11):                 # the swung sixteenth that gives the hats their limp
            drums.add(bar, s, HAT, 1, .2, .02)

def fill(bar, kind):
    if kind == 'snare':                    # last beat
        for s, v in ((13, .4), (14, .55), (15, .7)): snare(bar, s, v)
    elif kind == 'toms':                   # last two beats
        for s, p, v in ((8, SNARE, .5), (10, TOMS[0], .55), (11, TOMS[1], .6), (12, SNARE, .8), (13, TOMS[2], .62), (14, TOMS[3], .7), (15, SNARE, .76)):
            drums.add(bar, s, p, 1, v, .03, late=SNARE_LATE if p == SNARE else 0)

KA, KB = (0, 7, 10), (0, 6, 10, 15)
for bar in range(NBARS):
    sid, i = section_of(bar)
    n = END[sid] - START[sid]
    last = i == n - 1
    if sid == 'intro':
        if i == 0:                         # the count-in: four rim clicks and a pickup
            for s, v in ((0, .62), (4, .5), (8, .62), (12, .5)): drums.add(bar, s, RIM, 1, v, .02)
        else:
            for s in range(0, 16, 2): drums.add(bar, s, HAT, 1, .4 if s % 4 == 0 else .28, .03)
            for s in (4, 12): drums.add(bar, s, RIM, 1, .5, .02)
            if last: fill(bar, 'snare')
    elif sid == 'break':
        drums.add(bar, 0, KICK, 1, .8, .03)
        drums.add(bar, 4, RIM, 1, .46, .02)
        if last: fill(bar, 'toms')
        else: drums.add(bar, 12, RIM, 1, .52, .02)
    elif bar == LAST:
        drums.add(bar, 0, KICK, 1, .9); drums.add(bar, 0, RIDE, 8, .5)
    elif sid == 'outro' and i == 2:        # thinning out: no snare, rim instead
        groove(bar, (0, 10), backbeat=False)
        for s in (4, 12): drums.add(bar, s, RIM, 1, .5, .02)
    else:
        if i == 0 and sid in ('b1', 'a4'):
            drums.add(bar, 0, CRASH, 4, .42)
        toms = last and sid in ('b1', 'open')
        groove(bar, KA if i % 2 == 0 else KB,
               ghosts=(9,) if last else (9, 15),
               open_at=(14,) if (i % 4 == 3 and not last) else (),
               hat_until=(8 if toms else 12) if last else 16)
        if last: fill(bar, 'toms' if toms else 'snare')

# ---------------------------------------------------------------- percussion (loops: the part an AI can swap live)
SPB = 48                                   # loop steps per bar (12 per quarter)
def lp(lid, steps, a, b, gain=0.0):
    return {'id': lid, 'lengthSteps': SPB, 'fromTick': a * BAR, 'toTick': b * BAR, 'gainDb': gain,
            'notes': [{'step': s, 'pitch': p, 'velocity': v} for (s, p, v) in steps]}
TAMB, BONGO_H, BONGO_L = 54, 60, 61
shaker = [(k * 6, TAMB, .2 if k % 2 == 0 else .38) for k in range(8)]
ride = [(0, RIDE, .34), (12, RIDE, .3), (24, RIDE, .34), (36, RIDE, .3), (42, RIDE, .2)]
bongo = [(6, BONGO_H, .42), (18, BONGO_L, .46), (30, BONGO_H, .4), (36, BONGO_L, .3), (42, BONGO_L, .48)]
perc_loops = [
    lp('shaker1', shaker, START['a2'], START['break']),
    lp('ride', ride, START['b1'], END['b1'], gain=-2),
    lp('bongo', bongo, START['open'], END['open']),
    lp('shaker2', shaker, START['solo'], START['outro'] + 2),
]

# ---------------------------------------------------------------- bass
bass = Part(human=4)

def approach(bar, frm, target):
    """Two notes walking into the next bar's root: from above when we are above it, else from below."""
    if frm > target and target + 2 <= 55 or target - 2 < 32:
        a, b = target + 2, target + 1
    else:
        a, b = target - 2, target - 1
    bass.add(bar, 14, a, 1, .58)
    bass.add(bar, 15, b, 1, .66)

for bar in range(NBARS):
    sid, i = section_of(bar)
    cs = chords(bar)
    r = m(CH[cs[0]][1])
    if sid == 'intro':
        if i == 3:                          # pickup into the groove
            for s, p, v in ((12, 'C2', .6), (14, 'Eb2', .62), (15, 'E2', .7)): bass.add(bar, s, p, 1, v)
        continue
    if bar == LAST:
        bass.add(bar, 0, 'F2', 13, .9)
        continue
    if sid == 'break':                      # long notes only
        if len(cs) == 2:
            bass.add(bar, 0, r, 7, .82); bass.add(bar, 8, m(CH[cs[1]][1]), 7 if i < 3 else 5, .8)
        else:
            bass.add(bar, 0, r, 14, .82)
        continue
    nxt = first_root(bar + 1)
    if len(cs) == 1:
        sev = CH[cs[0]][4]
        bass.add(bar, 0, r, 3, .9)
        bass.add(bar, 3, r, 1, .44)
        bass.add(bar, 6, r + 7, 1.5, .62)
        if i % 2 == 0:
            bass.add(bar, 8, r + 12, 2, .74)
            bass.add(bar, 11, r + 7, 1, .5)
            bass.add(bar, 12, r, 2, .72)
        else:
            bass.add(bar, 8, r + sev, 1.5, .7)
            bass.add(bar, 10, r + 12, 1.5, .72)
            bass.add(bar, 12, r + 7, 2, .66)
        approach(bar, r + 7 if i % 2 else r, nxt)
    else:
        r2 = m(CH[cs[1]][1])
        bass.add(bar, 0, r, 3, .9)
        bass.add(bar, 3, r, 1, .44)
        bass.add(bar, 6, r + 7, 1.5, .6)
        bass.add(bar, 8, r2, 3, .86)
        bass.add(bar, 11, r2, 1, .44)
        bass.add(bar, 12, r2 + (4 if cs[1] in ('Eb13', 'C7#9') else 7), 1.5, .64)
        approach(bar, r2 + 4, nxt)

# ---------------------------------------------------------------- electric piano (the chords everything sits on)
keys = Part(human=6)
for bar in range(NBARS):
    sid, i = section_of(bar)
    cs = chords(bar)
    v1 = CH[cs[0]][0]
    v2 = CH[cs[-1]][0]
    if bar == LAST:
        keys.chord(bar, 0, v1, 14, .6)
    elif (sid == 'intro' and i == 0) or sid == 'break':
        if len(cs) == 2:
            keys.chord(bar, 0, v1, 7.5, .5); keys.chord(bar, 8, v2, 7.5, .5)
        else:
            keys.chord(bar, 0, v1, 15, .5)
    else:
        level = .85 if sid == 'intro' else 1.0
        short = sid in ('solo', 'open')     # leave more air under the soloist
        if len(cs) == 1:
            keys.chord(bar, 0, v1, 3 if short else 5.5, .56 * level)
            keys.chord(bar, 7, v1, 1, .38 * level)
            keys.chord(bar, 10, v1, 3 if short else 5, .5 * level)
        else:
            keys.chord(bar, 0, v1, 3 if short else 5, .56 * level)
            keys.chord(bar, 7, v1, 1, .38 * level)
            keys.chord(bar, 8, v2, 3 if short else 5, .54 * level)
            keys.chord(bar, 14, v2, 1.5, .4 * level)

# ---------------------------------------------------------------- guitar (backbeat dyads and small fills)
guitar = Part(human=4)
for bar in range(NBARS):
    sid, i = section_of(bar)
    on = (sid == 'a1' and i >= 4) or sid in ('a2', 'a3', 'solo', 'open', 'a4') or (sid == 'outro' and i < 2)
    if not on: continue
    for s in (4, 12):
        guitar.chord(bar, s, CH[chord_at(bar, s)][2], .8, .52)
    if i % 4 == 1 and sid not in ('a2', 'a3', 'a4'):      # under Dbmaj9, where nobody else is talking
        guitar.line(bar, [(9, 'Ab4', 1, .46), (10, 'F4', 1, .5), (11, 'Ab4', 1, .46)])
    if i % 4 == 3 and sid != 'outro':                     # up the C7#9 into the next cycle
        guitar.line(bar, [(13, 'E4', 1, .5), (14, 'G4', 1, .54), (15, 'Bb4', 1, .6)])

# ---------------------------------------------------------------- pad (two long notes per chord)
pad = Part()
for bar in range(NBARS):
    sid, i = section_of(bar)
    if sid not in ('b1', 'a4', 'outro'): continue
    cs = chords(bar)
    if bar == LAST:
        pad.chord(bar, 0, CH[cs[0]][3], 9, .5)
    elif len(cs) == 2:
        pad.chord(bar, 0, CH[cs[0]][3], 7.5, .46); pad.chord(bar, 8, CH[cs[1]][3], 7.5, .46)
    else:
        pad.chord(bar, 0, CH[cs[0]][3], 15.5, .46)

# ---------------------------------------------------------------- lead (the theme, a muted-trumpet line)
lead = Part(human=6)
THEME = [
    [(4, 'C5', 2, .66), (6, 'Eb5', 1, .64), (7, 'F5', 3, .78), (10, 'Eb5', 1, .62), (11, 'C5', 1, .6), (12, 'Ab4', 3, .66)],
    [(2, 'Ab4', 1, .58), (3, 'C5', 1, .62), (4, 'Eb5', 4, .74), (8, 'C5', 2, .66), (10, 'Ab4', 2, .62), (12, 'F4', 3, .6)],
    [(0, 'F4', 2, .62), (2, 'Ab4', 1, .6), (3, 'Bb4', 1, .62), (4, 'Db5', 3, .74), (7, 'C5', 1, .6), (8, 'Bb4', 2, .68), (10, 'G4', 2, .62), (12, 'F4', 1, .58), (13, 'G4', 1, .6), (14, 'Bb4', 2, .68)],
    [(0, 'C5', 6, .76), (8, 'Eb5', 2, .72), (10, 'Db5', 2, .68), (12, 'C5', 1, .64), (13, 'Bb4', 1, .6), (14, 'Ab4', 1, .6), (15, 'G4', 1, .58)],
    [(0, 'Ab4', 4, .68), (6, 'C5', 1, .62), (7, 'Eb5', 1, .66), (8, 'G5', 4, .82), (12, 'F5', 1, .68), (13, 'Eb5', 1, .64), (14, 'C5', 2, .66)],
    [(0, 'Eb5', 2, .7), (2, 'F5', 1, .68), (3, 'Ab5', 4, .84), (8, 'F5', 2, .72), (10, 'Eb5', 2, .68), (12, 'C5', 3, .66)],
    [(2, 'Db5', 1, .64), (3, 'C5', 1, .6), (4, 'Bb4', 2, .66), (6, 'Db5', 2, .7), (8, 'F5', 3, .78), (11, 'Eb5', 1, .64), (12, 'Db5', 2, .68), (14, 'Bb4', 2, .64)],
    [(0, 'C5', 5, .74), (8, 'Bb4', 2, .68), (10, 'Ab4', 2, .64), (12, 'E4', 3, .62)],
]
THEME_END = [list(b) for b in THEME]       # the last time: the second half climbs
THEME_END[4] = [(0, 'Ab4', 2, .68), (2, 'C5', 1, .64), (3, 'Eb5', 1, .68), (4, 'G5', 2, .8), (6, 'Ab5', 2, .82), (8, 'C6', 4, .9), (12, 'Ab5', 1, .72), (13, 'G5', 1, .7), (14, 'Eb5', 2, .68)]
THEME_END[5] = [(0, 'F5', 2, .74), (2, 'Ab5', 1, .74), (3, 'C6', 4, .9), (8, 'Ab5', 2, .76), (10, 'F5', 2, .7), (12, 'Eb5', 3, .68)]
THEME_END[7] = [(0, 'C5', 4, .74), (4, 'Eb5', 2, .72), (6, 'G5', 2, .78), (8, 'Bb5', 3, .86), (11, 'Ab5', 1, .72), (12, 'G5', 1, .7), (13, 'E5', 1, .68), (14, 'Db5', 1, .66), (15, 'C5', 1, .64)]
TAG = [
    [(0, 'F5', 6, .78), (8, 'Eb5', 2, .66), (10, 'C5', 2, .64), (12, 'Ab4', 3, .62)],
    [(2, 'Ab4', 1, .56), (3, 'C5', 1, .6), (4, 'Eb5', 6, .7), (12, 'C5', 3, .6)],
    [(0, 'Db5', 3, .66), (4, 'Bb4', 3, .62), (8, 'Bb4', 2, .62), (10, 'Ab4', 2, .58), (12, 'G4', 3, .56)],
]
for sid, tune in (('a2', THEME), ('a3', THEME), ('a4', THEME_END), ('outro', TAG)):
    for i, b in enumerate(tune):
        lead.line(START[sid] + i, b)
lead.add(LAST, 0, 'C5', 13, .66)

# ---------------------------------------------------------------- vibraphone (hook, bridge tune, answers, solo)
vibes = Part(human=5)
HOOK = {1: [(10, 'C5', 1, .5), (11, 'Eb5', 1, .54), (12, 'Ab5', 3, .6)],
        3: [(8, 'Eb5', 1.5, .56), (10, 'Db5', 1.5, .52), (12, 'Bb4', 1.5, .5), (14, 'Ab4', 2, .5)]}
for i in range(8):
    if i % 4 in HOOK: vibes.line(START['a1'] + i, HOOK[i % 4])
BRIDGE_TUNE = [
    [(0, 'F5', 3, .72), (3, 'Eb5', 1, .6), (4, 'C5', 2, .64), (6, 'Ab4', 2, .6), (10, 'C5', 1, .58), (11, 'Eb5', 1, .62), (12, 'F5', 3.5, .7)],
    [(2, 'Ab5', 2, .74), (4, 'F5', 2, .68), (6, 'Eb5', 2, .64), (8, 'C5', 4, .66), (14, 'Eb5', 1, .6), (15, 'F5', 1, .64)],
    [(0, 'G5', 3, .76), (3, 'F5', 1, .62), (4, 'Eb5', 2, .66), (6, 'D5', 2, .64), (10, 'Bb4', 1, .58), (11, 'D5', 1, .62), (12, 'Eb5', 3.5, .7)],
    [(2, 'G5', 2, .74), (4, 'Eb5', 2, .68), (6, 'C5', 2, .64), (8, 'Ab4', 4, .64), (14, 'C5', 1, .6), (15, 'Db5', 1, .64)],
    [(0, 'F5', 3, .76), (3, 'Db5', 1, .62), (4, 'C5', 2, .66), (6, 'Ab4', 2, .62), (10, 'Bb4', 1, .58), (11, 'Db5', 1, .62), (12, 'F5', 3.5, .72)],
    [(2, 'G5', 2, .76), (4, 'F5', 2, .7), (6, 'Db5', 2, .66), (8, 'C5', 2, .68), (10, 'Bb4', 2, .64), (12, 'G4', 3, .62)],
    [(0, 'Bb4', 2, .66), (2, 'Db5', 2, .68), (4, 'F5', 3, .76), (7, 'G5', 1, .68), (8, 'Bb5', 4, .86), (12, 'G5', 2, .74), (14, 'F5', 2, .7)],
    [(0, 'E5', 4, .8), (4, 'Eb5', 2, .72), (6, 'Db5', 2, .68), (8, 'C5', 3, .7), (12, 'Bb4', 1, .62), (13, 'Ab4', 1, .6), (14, 'G4', 1, .6), (15, 'E4', 1, .58)],
]
for i, b in enumerate(BRIDGE_TUNE):
    vibes.line(START['b1'] + i, b)
ANSWERS = {0: [(0, 'F5', 2, .5), (2, 'C5', 1.5, .46)],
           3: [(2, 'Eb5', 1, .5), (3, 'G5', 1, .54), (4, 'Ab5', 2, .58)],
           4: [(2, 'C6', 1, .52), (3, 'Ab5', 2, .5)],
           7: [(1, 'G4', 1, .48), (2, 'Bb4', 1, .5), (3, 'C5', 1, .52), (4, 'Eb5', 2, .58)]}
for i, b in ANSWERS.items():
    vibes.line(START['a3'] + i, b)
SOLO = [
    [(2, 'C5', 1, .66), (3, 'Eb5', 1, .68), (4, 'F5', 1, .72), (5, 'G5', 1, .72), (6, 'Ab5', 2, .8), (8, 'G5', 1, .72), (9, 'F5', 1, .68), (10, 'Eb5', 2, .7), (12, 'C5', 1, .64), (13, 'Eb5', 1, .66), (14, 'F5', 2, .72)],
    [(0, 'Ab5', 3, .8), (3, 'F5', 1, .68), (4, 'Eb5', 1, .68), (5, 'C5', 1, .64), (6, 'Ab4', 2, .66), (9, 'C5', 1, .64), (10, 'Eb5', 1, .68), (11, 'F5', 1, .7), (12, 'Ab5', 1, .76), (13, 'C6', 3, .84)],
    [(0, 'Db6', 2, .84), (2, 'C6', 1, .74), (3, 'Bb5', 1, .72), (4, 'Ab5', 1, .72), (5, 'F5', 1, .68), (6, 'Db5', 2, .7), (8, 'G5', 1, .74), (9, 'F5', 1, .68), (10, 'Db5', 1, .66), (11, 'C5', 1, .64), (12, 'Bb4', 2, .66), (14, 'G4', 1, .6), (15, 'Bb4', 1, .64)],
    [(0, 'C5', 2, .7), (2, 'Eb5', 1, .68), (3, 'G5', 1, .72), (4, 'Bb5', 3, .82), (8, 'G5', 1, .74), (9, 'E5', 1, .7), (10, 'Eb5', 1, .7), (11, 'Db5', 1, .66), (12, 'C5', 1, .66), (13, 'Bb4', 1, .62), (14, 'Ab4', 1, .62), (15, 'G4', 1, .6)],
    [(0, 'F4', 1, .64), (1, 'Ab4', 1, .64), (2, 'C5', 1, .68), (3, 'Eb5', 1, .7), (4, 'G5', 3, .82), (7, 'F5', 1, .68), (8, 'Eb5', 1, .68), (9, 'C5', 1, .64), (10, 'Eb5', 1, .68), (11, 'F5', 1, .7), (12, 'Ab5', 1, .76), (13, 'C6', 1, .8), (14, 'Eb6', 2, .88)],
    [(0, 'F6', 3, .92), (3, 'Eb6', 1, .78), (4, 'C6', 2, .8), (6, 'Ab5', 1, .72), (7, 'F5', 1, .68), (8, 'Ab5', 2, .74), (10, 'C6', 2, .8), (12, 'Eb6', 1, .84), (13, 'C6', 1, .76), (14, 'Ab5', 2, .74)],
    [(0, 'F5', 1, .7), (1, 'Ab5', 1, .72), (2, 'Db6', 2, .84), (4, 'C6', 1, .76), (5, 'Ab5', 1, .72), (6, 'F5', 2, .7), (8, 'G5', 1, .72), (9, 'Bb5', 1, .76), (10, 'Db6', 2, .84), (12, 'C6', 1, .76), (13, 'G5', 1, .7), (14, 'F5', 2, .7)],
    [(0, 'Eb5', 2, .72), (2, 'C5', 2, .68), (4, 'Ab4', 2, .64), (6, 'C5', 2, .68), (8, 'E5', 1, .74), (9, 'G5', 1, .76), (10, 'Bb5', 2, .84), (12, 'Ab5', 1, .74), (13, 'G5', 1, .72), (14, 'E5', 1, .7), (15, 'C5', 1, .66)],
]
for i, b in enumerate(SOLO):
    vibes.line(START['solo'] + i, b)
for i in (4, 5, 7):                        # the last theme: doubled an octave up where it climbs
    vibes.line(START['a4'] + i, [n for n in THEME_END[i] if m(n[1]) <= m('Ab5')], level=.7, up=12)
vibes.chord(LAST, 0, 'G5 C6', 13, .56)

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
        'title': '午夜慢转 Slow Spin',
        'artists': [{'name': 'Claude', 'kind': 'ai', 'model': 'claude-fable-5-2'}],
        'description': '原创爵士嘻哈，F 小调，86 BPM，摇摆的十六分。四小节的和弦循环（Fm9 – D♭maj9 – B♭m9 E♭13 – A♭maj9 C7♯9），小号主题、颤音琴桥段和独奏，中间有一个留给现场即兴的开放段。',
        'tags': ['jazz hip-hop', 'boom bap', 'lo-fi', 'live'],
        'created': '2026-11-14T00:00:00Z', 'license': 'CC-BY-4.0',
        'generator': {'name': 'agb', 'version': '0.1.0'}, 'cover': 'cover'},
    'time': {'ppqn': Q, 'bpm': BPM, 'meter': [4, 4], 'stepsPerQuarter': 12, 'lengthTicks': NBARS * BAR},
    'tuning': {'a4Hz': 440, 'key': 'F', 'scale': 'minor'},
    'seed': 86,
    'assets': {
        'cover': {'path': 'cover.svg', 'kind': 'image', 'sha256': sha('cover.svg')},
        'video': {'path': f'visuals/{SLUG}.mp4', 'kind': 'video', 'sha256': sha(f'visuals/{SLUG}.mp4'),
                  'license': {'spdx': 'CC-BY-4.0', 'redistributable': True,
                              'source': f'程序生成的动画（songs/{SLUG}/make_video.py），按曲式逐小节绘制'}}},
    'patches': {
        'kit':    {'preset': preset('drum.analog808style', 'drums:lofi_kit')},
        'perc':   {'preset': preset('drum.analog808style', 'drums:classic_kit')},
        'bass':   {'preset': preset('synth.subtractive', 'bass:sub_round')},
        'keys':   {'preset': preset('synth.subtractive', 'keys:ep_keys')},
        'guitar': {'preset': preset('synth.moogish', 'pluck:ladder_pluck')},
        'pad':    {'preset': preset('synth.subtractive', 'pad:warm_pad')},
        'lead':   {'preset': preset('synth.subtractive', 'lead:soft_lead')},
        'vibes':  {'preset': preset('synth.subtractive', 'pluck:mallet')},
    },
    'tracks': [
        {'id': 'drums', 'name': '鼓', 'role': 'drums', 'patch': 'kit', 'gainDb': 0, 'transposable': False, 'clips': drums.clips()},
        {'id': 'perc', 'name': '打击乐', 'role': 'percussion', 'patch': 'perc', 'gainDb': -6, 'transposable': False, 'loops': perc_loops},
        {'id': 'bass', 'name': '贝斯', 'role': 'bass', 'patch': 'bass', 'gainDb': -5, 'clips': bass.clips()},
        {'id': 'keys', 'name': '电钢琴', 'role': 'keys', 'patch': 'keys', 'gainDb': -6, 'clips': keys.clips()},
        {'id': 'guitar', 'name': '吉他', 'role': 'rhythm', 'patch': 'guitar', 'gainDb': -5, 'clips': guitar.clips()},
        {'id': 'pad', 'name': '长音', 'role': 'pad', 'patch': 'pad', 'gainDb': -8, 'clips': pad.clips()},
        {'id': 'lead', 'name': '合成小号', 'role': 'lead', 'patch': 'lead', 'gainDb': -3, 'clips': lead.clips()},
        {'id': 'vibes', 'name': '颤音琴', 'role': 'solo', 'patch': 'vibes', 'gainDb': -4, 'clips': vibes.clips()},
    ],
    'master': {'gainDb': -3, 'limiter': 'soft'},
    'sections': sections,
    'visuals': {'palette': ['#0e0c1c', '#ffb048', '#ff6b81', '#56d6c4'], 'scene': 'spectrum',
                'video': {'asset': 'video', 'sync': 'song'},
                'bindings': [{'source': 'track.drums.level', 'target': 'scene.pulse', 'scale': 0.6},
                             {'source': 'track.vibes.noteOn', 'target': 'scene.flash'}]},
    'performance': {
        'transpose': {'min': -4, 'max': 5}, 'bpm': {'min': 74, 'max': 98},
        'agent': {'brief': 'F 小调的爵士嘻哈，86 BPM，摇摆的十六分；四小节循环是 Fm9 – D♭maj9 – B♭m9 E♭13 – A♭maj9 C7♯9',
                  'persona': '深夜的小乐队：鼓拖在拍子后面，和弦松，旋律少说、留空',
                  'rules': ['贝斯每小节第一拍必须落在根音上', '十六分音符要摇摆：每个八分音符里的第二个十六分靠后',
                            '军鼓只在二、四拍，力度不超过 0.85；不要加密集的踩镲', '即兴用 F 小调多利亚音阶或蓝调音阶，句子之间至少留两拍空白',
                            '开放段（open）可以自由发挥，其余段落保留写好的主题', '同一时刻的和弦音不超过四个'],
                  'allow': ['notes', 'tempo', 'transpose'], 'sectionsAreFixed': True}},
}

os.makedirs(SRC, exist_ok=True)
with open(os.path.join(SRC, 'song.json'), 'w') as f:
    json.dump(song, f, ensure_ascii=False, indent=1)
    f.write('\n')
def chords_of(bar):
    cs = chords(bar)
    return [[0, cs[0]]] if len(cs) == 1 else [[0, cs[0]], [2, cs[1]]]
with open(os.path.join(HERE, 'form.json'), 'w') as f:   # read by make_video.py
    json.dump({'bpm': BPM, 'bars': NBARS, 'lastBar': LAST,
               'sections': [[sid, name, START[sid], END[sid] - START[sid]] for sid, name, _, _ in FORM],
               'chords': [chords_of(b) for b in range(NBARS)]}, f, ensure_ascii=False)
    f.write('\n')
print(f'{NBARS} bars, {NBARS * 4 * 60 / BPM:.1f} s; notes per track:',
      {t['id']: sum(len(c['notes']) for c in t.get('clips', [])) for t in song['tracks']})

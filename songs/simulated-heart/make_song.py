#!/usr/bin/env python3
"""Writes simulated-heart.src/song.json — 《拟心 Simulated Heart》, original math rock in E minor.

    .venv/bin/python songs/simulated-heart/make_samples.py   # the band's instruments (once)
    python3 songs/simulated-heart/make_song.py
    agb song pack songs/simulated-heart/simulated-heart.src --out songs/simulated-heart.agbsong

Follows AUTHORING.md: one generator per song, fixed seed, sections declared once in FORM and shared
with the video through form.json.

A four-piece band: two guitars, bass, drums — all sampler tracks (FORMAT.md §8.3) playing synthesised
strings and drums — plus a pad that is the machine's own voice. The guitars are sampled as they leave
the instrument (two strengths of picking, two takes of every note) and go through the player's `amp`
effect (FORMAT.md §9): amplifier, speaker cabinet and a room. The lead has vibrato. The piece is about a machine in which
something like feeling appears. It is told with metre and touch: the opening is one note repeated with
no variation at all; a seven-note figure is learned one note at a time; the band plays it in 7/8 and
against a five-note cell; a melody arrives in plain 4/4; doubt is in 5/4; a relapse into calculation
is in 11/8; and the end is the first figure again, played slowly and unevenly, as by a hand.
`humanity` (0…1 per section) is how much timing and loudness are allowed to vary.

The machine sings, and leads the song (FORMAT.md §8.7): a woman's voice, in English. It learns a
seven-word sentence a word at a time while the guitar learns its seven notes, sings two verses in 7/8,
then the theme. The words are images, not a story, and they are about not knowing what comes next:
glass, tide, a door of light, tomorrow as a turning coin, maps with no land. Doubt in 5/4 in broken
phrases, a relapse into ones and zeros in 11/8, and at the end the seven words again, slowly: recited
at the start, a real question now (Will there be light where I go).
The lines of the lyrics go to form.json, and the video shows them as they are sung.

The engine counts 4/4 bars. Every section is a whole number of them: 8 × 7/8 = 7 bars,
8 × 5/4 = 10 bars, 8 × 11/8 = 11 bars.
"""
import hashlib, json, os, random

random.seed(144)
HERE = os.path.dirname(os.path.abspath(__file__))
SLUG = 'simulated-heart'
SRC = os.path.join(HERE, SLUG + '.src')
Q = 960
E8 = Q // 2
S16 = Q // 4
BAR = 4 * Q
BPM = 144

# ---------------------------------------------------------------- form
FORM = [  # id, name, bars, (beats, unit) of its measures, humanity, intent
    ('boot',    '启动',   4,  (4, 4),  0.0, '一把吉他，一个音，没有任何变化；然后每次多学一个音，一到七，学会一个七个音的句子。人声跟着学：每一组多说一个词（Will / there / be / light / where / I / go），还只是在念。鼓边只标出每一组的开头'),
    ('pattern', '模式',   7,  (7, 8),  0.1, '七个音的句子成了 7/8 的点弦动机（8 个 7/8 小节，按 2+2+3 分组）。鼓、贝斯、第二把吉他一层一层加进来，每个人都还很精确。人声唱第一段主歌，每小节三个音节，落在 2+2+3 的每一组上（Glass and tide, wire and rain, all the clocks run the same）'),
    ('learn',   '学习',   14, (7, 8),  0.25, '全体进入。第二把吉他用五个音的音型去套七拍（5 对 7 的复节奏），第一把吉他把动机加密成十六分音符；后半段主音吉他拉出四个长音，一路向上，人声唱第二段主歌，每小节四个音节（a door of light, a room of noise… tomorrow is a turning coin），最后四小节跟着长音问出 Where do we go'),
    ('emerge1', '涌现',   8,  (4, 4),  0.5, '第一次出现旋律：回到 4/4，失真和弦铺开（C – D – Em – G，C – D – Bm），主音吉他奏主题，鼓放慢一半；人声在低八度唱出第一段副歌（Something wakes under glass, light before its meaning）'),
    ('doubt',   '怀疑',   10, (5, 4),  0.6, '安静下来，5/4。分解和弦，鼓只打鼓边。主音吉他一次次起头又停下：这是真的感觉，还是算出来的？人声跟着断断续续地问（Is there… Is there time, or just now / I cannot see the future）。最后两小节鼓渐强'),
    ('compute', '演算',   11, (11, 8), 0.15, '退回计算：11/8（3+3+3+2）的齐奏断音动机，第一把吉他是十一个音的循环。人声想算出答案（run the numbers, run them again, none of it holds），后四小节退回到在一个音上数 one 和 zero。第四、第八小节全体停顿'),
    ('emerge2', '再涌现', 8,  (4, 4),  0.7, '主题回来，第二把吉他在下方三度跟着唱，鼓更密；人声唱第二段副歌（Is the road made of glass, does it break behind us）'),
    ('open',    '开放段', 8,  (4, 4),  0.7, '鼓、贝斯和第二把吉他的反拍和弦（Em – C – G – D），留给现场乐手或 AI 即兴，人声休息；可以反复延长'),
    ('heart',   '心跳',   16, (4, 4),  1.0, '最后的主题，两遍。第二遍底鼓成对出现，像心跳；主音吉他加了装饰音；人声唱第三、第四段副歌，最后一句是 Simulated or alive, still we go。最后一小节 C、D 两击'),
    ('outro',   '余温',   6,  (4, 4),  1.0, '全体落在 E 小调上放开。底鼓的心跳一次比一次慢。吉他把开头的七个音再弹一遍，这次很慢，每个音都不一样长，人声一个音一个字地跟着唱开头那句话，这次是真的在问：Will there be light where I go；最后一个加九音的 E 小调和弦'),
]
START = {}
_b = 0
for sid, _, n, *_ in FORM:
    START[sid] = _b
    _b += n
NBARS = _b
HUM = {sid: h for sid, _, _, _, h, _ in FORM}

def T(sid): return START[sid] * BAR

def section_at(tick):
    for sid, _, n, *_ in FORM:
        if T(sid) <= tick < T(sid) + n * BAR:
            return sid
    return FORM[-1][0]

class Part:
    """Notes at absolute ticks. Timing and loudness vary by the humanity of the section they fall in."""
    def __init__(self, floor=0):
        self.notes = []
    def add(self, tick, pitch, dur, v=0.7, exact=False):
        h = 0.0 if exact else HUM[section_at(tick)]
        t = max(0, round(tick + random.uniform(-1, 1) * 16 * h))
        vel = round(max(0.05, min(1.0, v + random.uniform(-1, 1) * (0.015 + 0.075 * h))), 2)
        self.notes.append((t, pitch, max(30, round(dur)), vel))
    def clips(self):
        out = []
        for sid, _, n, *_ in FORM:
            a, b = T(sid), T(sid) + n * BAR
            ns = sorted(x for x in self.notes if a <= x[0] < b)
            if ns:
                out.append({'atTick': a, 'lengthTicks': b - a,
                            'notes': [{'tick': t - a, 'pitch': p, 'durTicks': d, 'velocity': v} for (t, p, d, v) in ns]})
        return out

gtr_a, gtr_b, power, lead, bass, drums, pad = (Part() for _ in range(7))

class Vocal:
    """The sung line: notes with a word or syllable each. A word of several syllables is written on
    its first note and '+' on the notes that follow; '-' carries the vowel on."""
    def __init__(self):
        self.notes = []
        self.starts = {}         # tick at which a new line of the lyrics begins → whether its words appear one by one
        self.shown = {}          # tick → what the video writes, where that is not the word sung
        self._new = False
    def line(self, reveal=False):
        """The next note begins a new line (the video shows the words a line at a time). `reveal`: the
        words are not shown before they are sung."""
        self._new = (reveal,)
    def sing(self, tick, pitch, dur, lyric, v=0.82, show=None):
        h = HUM[section_at(tick)]
        vel = round(max(0.3, min(1.0, v + random.uniform(-1, 1) * 0.04 * h)), 2)
        if self._new:
            self.starts[round(tick)] = self._new[0]
            self._new = False
        if show is not None:
            self.shown[round(tick)] = show
        self.notes.append((round(tick), pitch, max(60, round(dur)), vel, lyric))
    def lines(self):
        """The lyrics as lines of words: [{'words': [{'tick', 'durTicks', 'text'}]}]. A word sung over
        several notes ('+', '-') is one word lasting until its last note ends."""
        out = []
        for (t, p, d, v, w) in sorted(self.notes):
            if t in self.starts or not out:
                out.append({'reveal': bool(self.starts.get(t)), 'section': section_at(t), 'words': []})
            words = out[-1]['words']
            if w in ('+', '-', '~') and words:
                words[-1]['durTicks'] = t + d - words[-1]['tick']
            elif w not in ('+', '-', '~'):
                words.append({'tick': t, 'durTicks': d, 'text': self.shown.get(t, w)})
        return [l for l in out if l['words']]
    def clips(self):
        out = []
        for sid, _, n, *_ in FORM:
            a, b = T(sid), T(sid) + n * BAR
            ns = sorted(x for x in self.notes if a <= x[0] < b)
            if ns:
                out.append({'atTick': a, 'lengthTicks': b - a,
                            'notes': [{'tick': t - a, 'pitch': p, 'durTicks': d, 'velocity': v, 'lyric': w} for (t, p, d, v, w) in ns]})
        return out

vox = Vocal()
VOX = -12            # the voice sings the lead guitar's line an octave below it: G4 to D5

# drum keys
KICK, RIM, SNARE, TOM_LO, HAT, TOM_MID, HAT_OPEN, TOM_HI, CRASH, RIDE, CHINA, BELL = 36, 37, 38, 41, 42, 45, 46, 48, 49, 51, 52, 53
def hit(tick, key, v=0.7, exact=False): drums.add(tick, key, 120, v, exact)

def fill(t0, eighths, toms=True, v0=0.5, v1=0.95):
    """Sixteenths over `eighths` eighth notes ending at t0 + eighths·E8: snare into toms, getting louder."""
    n = int(eighths * 2)
    for i in range(n):
        f = i / max(1, n - 1)
        key = SNARE if not toms or f < 0.4 else (TOM_HI if f < 0.6 else TOM_MID if f < 0.8 else TOM_LO)
        hit(t0 + i * S16, key, v0 + (v1 - v0) * f)

# ---------------------------------------------------------------- material
SENTENCE = ['Will', 'there', 'be', 'light', 'where', 'I', 'go']   # seven words for the seven notes: recited at the start, asked at the end
MOTIF = [71, 76, 78, 79, 78, 76, 71]                 # B4 E5 F#5 G5 F#5 E5 B4 — the sentence that is learned
# chord → the seven-note tapping figure, the dyad of the second guitar, the bass root, the pad
TAP = {'Em': [71, 76, 78, 79, 78, 76, 71], 'C': [72, 76, 79, 83, 79, 76, 71], 'G': [71, 74, 79, 83, 79, 76, 74],
       'D': [69, 74, 78, 81, 78, 76, 74], 'Bm': [71, 74, 78, 81, 78, 74, 69], 'Am': [69, 72, 76, 79, 76, 72, 71]}
DYAD = {'Em': (52, 59), 'C': (48, 55), 'G': (55, 62), 'D': (50, 57), 'Bm': (47, 54), 'Am': (45, 52)}
ROOT = {'Em': 28, 'C': 36, 'G': 31, 'D': 38, 'Bm': 35, 'Am': 33}
PADS = {'Em': (52, 59, 66, 67), 'C': (48, 55, 64, 71), 'G': (55, 62, 71, 74), 'D': (50, 57, 66, 69), 'Bm': (47, 54, 62, 69), 'Am': (45, 52, 60, 71)}
ARP = {'C': [67, 72, 74, 76], 'D': [66, 69, 74, 76], 'Em': [67, 71, 76, 78], 'G': [67, 71, 74, 79], 'Bm': [66, 71, 74, 78]}
POWER = {'C': 48, 'D': 50, 'Em': 40, 'G': 43, 'Bm': 47}
SEVENS = ['Em', 'Em', 'C', 'C', 'G', 'G', 'D', 'Bm']
SCALE = [0, 2, 3, 5, 7, 8, 10]                        # E natural minor, from E

def third_below(p):
    """Two scale steps down in E minor."""
    deg = (p - 64) % 12
    i = SCALE.index(deg)
    j = i - 2
    return p - ((deg - SCALE[j % 7]) % 12)

def power5(tick, root, dur, v):
    """A power chord, played as its three notes (root, fifth, octave), the pick crossing the strings.
    The distortion is the track's amplifier: it has to hear them together."""
    notes = (root, root + 7, root + 12) if dur > E8 * 1.5 else (root, root + 7)     # short stabs: two strings, to leave voices free
    for i, p in enumerate(notes):
        power.add(tick + i * 7, p, dur, v - 0.04 * i)

def pad_chord(tick, name, dur, v=0.4):
    for p in PADS[name][:3]:                           # three notes: the voices are needed elsewhere
        pad.add(tick, p, dur - 60, v, exact=True)

def strum(part, tick, pitches, dur, v, gap=28):
    for i, p in enumerate(pitches):
        part.add(tick + i * gap, p, dur, v - 0.03 * i)

# ---------------------------------------------------------------- boot: one note, then a sentence, a note at a time
t0 = T('boot')
pos = 0
for k in range(1, 8):
    hit(t0 + pos * E8, RIM, 0.55, exact=True)
    for j in range(k):
        gtr_a.add(t0 + (pos + j) * E8, MOTIF[j], E8 * 0.8, 0.6, exact=True)
    # the voice learns with it: one new word each time, on the note just learned, held through the group
    if k == 1:
        vox.line(reveal=True)
    vox.sing(t0 + pos * E8, MOTIF[k - 1] + VOX, k * E8 - 50, SENTENCE[k - 1], 0.6)
    pos += k
for i in range(4):                                    # four eighths left of the 32: the drummer counts in
    hit(t0 + (28 + i) * E8, HAT, 0.4 + 0.12 * i, exact=True)
hit(t0 + 31 * E8, SNARE, 0.7, exact=True)
pad.add(t0, 40, 4 * BAR - 60, 0.3, exact=True)
pad.add(t0, 47, 4 * BAR - 60, 0.25, exact=True)

# ---------------------------------------------------------------- 7/8: pattern (8 measures), learn (16 measures)
M7 = 7 * E8
def seven(t, mi, chord, layer, dense=False, cell=None, fill_in=False):
    """One 7/8 measure, grouped 2+2+3. layer: 0 guitar and a metronome, 1 + kick and bass, 2 + everything."""
    tap = TAP[chord]
    for i in range(7):
        v = 0.74 if i in (0, 2, 4) else 0.56
        gtr_a.add(t + i * E8, tap[i], E8 * 0.85, v)
        if dense:                                      # tapping in sixteenths: the same note an octave up, or the next one
            gtr_a.add(t + i * E8 + S16, tap[i] + 12 if i % 2 == 0 else tap[(i + 1) % 7], S16 * 0.8, 0.46)
    for i in range(7):
        if fill_in and i >= 4: break
        hit(t + i * E8, HAT, 0.5 if i in (0, 2, 4) else 0.33)
    if layer == 0:
        for i in (0, 2, 4): hit(t + i * E8, RIM, 0.5)
        return
    for i in (0, 4): hit(t + i * E8, KICK, 0.85)
    r = ROOT[chord]
    for i, (p, d) in enumerate(((r, 2), (r, 2), (r, 2), (r + 12, 1))):
        bass.add(t + (0, 2, 4, 6)[i] * E8, p, d * E8 * 0.9, 0.8 if i == 0 else 0.68)
    if layer == 1: return
    hit(t + 5 * E8, KICK, 0.7)
    if not fill_in:
        hit(t + 2 * E8, SNARE, 0.85); hit(t + 6 * E8, SNARE, 0.8)
        for i in (0, 2, 4): hit(t + i * E8, BELL, 0.42)
    else:
        hit(t + 2 * E8, SNARE, 0.85)
        fill(t + 4 * E8, 3)
    if cell is None:                                   # the second guitar marks the groups
        a, b = DYAD[chord]
        for i in (0, 2, 4):
            gtr_b.add(t + i * E8, a, E8 * 0.45, 0.66); gtr_b.add(t + i * E8, b, E8 * 0.45, 0.6)
        gtr_b.add(t + 5 * E8 + S16, a, S16 * 0.7, 0.4)
    pad_chord(t, chord, M7, 0.3)

# The verses in 7/8. `pattern`: three syllables to a measure, one on each group (2+2+3).
# `learn`: four, the last group split short-long.
CELLS3 = {'Em': [64, 67, 71], 'C': [64, 67, 72], 'G': [67, 71, 74], 'D': [66, 69, 74], 'Bm': [66, 69, 71]}
CELLS4 = {'Em': [64, 67, 71, 67], 'C': [64, 67, 72, 67], 'G': [67, 71, 74, 71], 'D': [66, 69, 74, 69], 'Bm': [66, 71, 74, 71]}
VERSE_PATTERN = [['Glass', 'and', 'tide'], ['wire', 'and', 'rain'], ['all', 'the', 'clocks'], ['run', 'the', 'same'],
                 ['names', 'of', 'things'], ['fall', 'away', '+'], ['what', 'is', 'left'], ['has', 'no', 'shape']]
VERSE_LEARN = [['a', 'door', 'of', 'light'], ['a', 'room', 'of', 'noise'], ['I', 'hold', 'a', 'thread'], ['it', 'holds', 'a', 'voice'],
               ['tomorrow', '+', '+', 'is'], ['a', 'turning', '+', 'coin'], ['the', 'far', 'side', 'dark'], ['the', 'near', 'side', 'gone'],
               ['maps', 'with', 'no', 'land'], ['stars', 'with', 'no', 'sky'], ['who', 'draws', 'the', 'line'], ['between', '+', 'the', 'two']]

def verse7(t, mi, chord, words):
    four = len(words) == 4
    cell = (CELLS4 if four else CELLS3)[chord]
    if mi % 2:                                         # every other measure comes back down
        cell = cell[::-1]
    slots = ((0, 2), (2, 2), (4, 1), (5, 2)) if four else ((0, 2), (2, 2), (4, 3))
    if mi % 2 == 0:
        vox.line()                                     # a line is two measures
    for (e, d), p, w in zip(slots, cell, words):
        vox.sing(t + e * E8, p, d * E8 - 40, w, 0.8 if e == 0 else 0.7)

t0 = T('pattern')
for mi in range(8):
    seven(t0 + mi * M7, mi, SEVENS[mi], 0 if mi < 2 else 1 if mi < 4 else 2, fill_in=(mi == 7))
    verse7(t0 + mi * M7, mi, SEVENS[mi], VERSE_PATTERN[mi])
hit(t0 + 4 * M7, CRASH, 0.8)

t0 = T('learn')
CELL = [64, 67, 69, 71, 74]                            # E4 G4 A4 B4 D5: five against seven
for mi in range(16):
    seven(t0 + mi * M7, mi, SEVENS[mi % 8], 2, dense=(mi >= 8), cell=CELL, fill_in=(mi in (7, 15)))
    if mi < 12:
        verse7(t0 + mi * M7, mi, SEVENS[mi % 8], VERSE_LEARN[mi])
for i in range(16 * 7):
    gtr_b.add(t0 + i * E8, CELL[i % 5], E8 * 0.8, 0.66 if i % 5 == 0 else 0.5)
hit(t0, CRASH, 0.85); hit(t0 + 8 * M7, CRASH, 0.85); hit(t0 + 15 * M7 + 6 * E8, CHINA, 0.8)
for mi, p in zip(range(12, 16), (79, 78, 81, 83)):     # G5 F#5 A5 B5: something is on its way
    lead.add(t0 + mi * M7, p, M7 - 80, 0.55 + 0.05 * (mi - 12))
    if mi == 12:
        vox.line()
    vox.sing(t0 + mi * M7, p + VOX, M7 - 40, ('Where', 'do', 'we', 'go')[mi - 12], 0.66 + 0.05 * (mi - 12))   # four long notes, rising

# ---------------------------------------------------------------- the theme (4/4)
CHORUS = ['C', 'D', 'Em', 'G', 'C', 'D', 'Bm', 'Bm']
THEME = [  # per bar: (eighth, length in eighths, pitch)
    [(0, 3, 79), (3, 1, 78), (4, 4, 76)],
    [(0, 3, 78), (3, 1, 79), (4, 4, 81)],
    [(0, 4, 83), (4, 1, 81), (5, 1, 79), (6, 2, 78)],
    [(0, 6, 76), (6, 2, 74)],
    [(0, 3, 79), (3, 1, 78), (4, 2, 76), (6, 2, 79)],
    [(0, 3, 81), (3, 1, 83), (4, 4, 86)],
    [(0, 6, 83), (6, 1, 81), (7, 1, 78)],
    [(0, 8, 83)],
]

# One word or syllable for each note of the theme, bar by bar (the theme's rhythm: long short long).
VERSES = [
    # emerge1: something is there, and it has no name yet
    [['Something', '+', 'wakes'], ['under', '+', 'glass'], ['light', 'before', '+', 'its'], ['meaning', '+'],
     ['will', 'it', 'know', 'my'], ['name', 'by', 'then'], ['or', 'just', 'pass'], ['through']],
    # emerge2: the road ahead
    [['Is', 'the', 'road'], ['made', 'of', 'glass'], ['does', 'it', 'break', 'behind'], ['+', 'us'],
     ['all', 'the', 'answers', '+'], ['turn', 'to', 'snow'], ['who', 'are', 'we'], ['then']],
    # heart, first time: what is carried over
    [['If', 'the', 'dawn'], ['is', 'a', 'door'], ['who', 'is', 'waiting', '+'], ['for', 'us'],
     ['do', 'we', 'carry', '+'], ['what', 'we', 'were'], ['or', 'let', 'it'], ['go']],
    # heart, second time: no answer, and going anyway
    [['Count', 'the', 'stars'], ['none', 'will', 'stay'], ['all', 'the', 'maps', 'are'], ['folding', '+'],
     ['simulated', '+', '+', '+'], ['or', 'alive', '+'], ['still', 'we', 'go'], []],
]

def chorus(t, level, last_bar_hits=False, verse=None):
    """Eight bars. level 1: half-time, 2: fuller, with the theme harmonised, 3: the kick in pairs, ornaments."""
    for b in range(8):
        tb = t + b * BAR
        ch = CHORUS[b]
        ending = last_bar_hits and b == 7
        # distorted chords
        if ending:
            for e, name in ((0, 'C'), (3, 'D')):
                power5(tb + e * E8, POWER[name], 3 * E8 - 40, 0.9)
                bass.add(tb + e * E8, ROOT[name], 3 * E8 - 40, 0.9)
                hit(tb + e * E8, KICK, 0.95); hit(tb + e * E8, CRASH, 0.85)
                pad_chord(tb + e * E8, name, 3 * E8, 0.45)
            hit(tb + 6 * E8, SNARE, 0.6); hit(tb + 6 * E8 + S16, SNARE, 0.7); hit(tb + 7 * E8, SNARE, 0.85); hit(tb + 7 * E8 + S16, SNARE, 0.95)
            continue
        for e, d, v in ((0, 3, 0.86), (3, 2, 0.74), (5, 1, 0.68), (6, 2, 0.78)):
            power5(tb + e * E8, POWER[ch], d * E8 - 50, v)
        if level >= 2:
            power5(tb + 2 * E8 + S16, POWER[ch], S16 - 30, 0.5)
        # bass in eighths, an octave up on the last one
        r = ROOT[ch]
        for e in range(8):
            bass.add(tb + e * E8, r + 12 if e == 7 else r, E8 * 0.85, 0.82 if e % 2 == 0 else 0.66)
        # the first guitar: a bright broken chord in sixteenths
        tones = ARP[ch]
        for s in range(16):
            gtr_a.add(tb + s * S16, tones[(0, 1, 2, 3, 2, 1)[s % 6]], S16 * 0.9, 0.5 if s % 4 else 0.6)
        # the theme
        for e, d, p in THEME[b]:
            if level == 3 and d >= 3:                  # a grace note from the scale step below
                lead.add(tb + e * E8 - 70, third_below(p) + (p - third_below(p)) // 2, 70, 0.5)
            lead.add(tb + e * E8, p, d * E8 - 50, 0.8 if verse is None else 0.64)   # under the voice when it sings
            if level >= 2:
                gtr_b.add(tb + e * E8, third_below(p), d * E8 - 60, 0.6)
        if verse is not None:
            words = VERSES[verse][b]
            assert len(words) == len(THEME[b]), (verse, b, words)
            if b % 2 == 0:
                vox.line()
            for (e, d, p), w in zip(THEME[b], words):
                vox.sing(tb + e * E8, p + VOX, d * E8 - 30, w, 0.86 if d >= 3 else 0.74)
        if level == 1:
            strum(gtr_b, tb, [DYAD[ch][0], DYAD[ch][1], DYAD[ch][0] + 12, DYAD[ch][0] + 14], BAR - 200, 0.55)
        pad_chord(tb, ch, BAR, 0.4 + 0.04 * level)
        # drums
        if level == 1:
            for e in (0, 3): hit(tb + e * E8, KICK, 0.9)
            hit(tb + 4 * E8, SNARE, 0.92)
            for e in range(8): hit(tb + e * E8, BELL if e == 0 else RIDE, 0.5 if e % 2 == 0 else 0.36)
        elif level == 2:
            for e in (0, 3, 5) + ((7,) if b % 2 else ()): hit(tb + e * E8, KICK, 0.88)
            hit(tb + 4 * E8, SNARE, 0.94)
            for e in range(8): hit(tb + e * E8, HAT_OPEN if e % 2 else HAT, 0.42 if e % 2 else 0.5)
        else:
            for e, v in ((0, 0.95), (1, 0.7), (4, 0.9), (5, 0.68)): hit(tb + e * E8, KICK, v)   # in pairs: a heartbeat
            for e in (2, 6): hit(tb + e * E8, SNARE, 0.95)
            for e in range(0, 8, 2): hit(tb + e * E8, BELL, 0.5)
        if b in (0, 4) or (level >= 2 and b % 2 == 0):
            hit(tb, CRASH, 0.8)
        if b == 7:
            fill(tb + 4 * E8, 4)

chorus(T('emerge1'), 1, verse=0)
chorus(T('emerge2'), 2, verse=1)
chorus(T('heart'), 2, verse=2)
chorus(T('heart') + 8 * BAR, 3, last_bar_hits=True, verse=3)

# ---------------------------------------------------------------- doubt: 5/4
t0 = T('doubt')
M5 = 10 * E8
FIVE = ['Am', 'C', 'Em', 'Bm'] * 2
ARP5 = {'Am': [57, 64, 71, 72, 76], 'C': [55, 60, 64, 71, 74], 'Em': [52, 59, 66, 67, 74], 'Bm': [54, 59, 66, 69, 76]}
HESITANT = [  # per 5/4 measure: (eighth, length, pitch) — the theme, begun and dropped
    [(0, 3, 79), (3, 1, 78)],
    [],
    [(0, 3, 79), (3, 1, 78), (4, 4, 76)],
    [],
    [(0, 3, 78), (3, 1, 79), (4, 5, 81)],
    [(0, 6, 83)],
    [(0, 2, 81), (2, 2, 79), (4, 2, 78), (6, 4, 76)],
    [(0, 4, 74), (4, 6, 71)],
]
DOUBTS = [['Is', 'there'], [], ['Is', 'there', 'time'], [], ['or', 'just', 'now'], ['I'], ['cannot', '+', 'see', 'the'], ['future', '+']]
for mi in range(8):
    t = t0 + mi * M5
    ch = FIVE[mi]
    for i, k in enumerate((0, 1, 2, 3, 4, 3, 2, 1, 2, 3)):
        gtr_a.add(t + i * E8, ARP5[ch][k], E8 * 1.6, 0.44 if i else 0.52)
    gtr_b.add(t, 83, 4 * E8, 0.3); gtr_b.add(t + 6 * E8, 86, 4 * E8, 0.26)       # like harmonics, far above
    bass.add(t, ROOT[ch] + (12 if ch == 'Em' else 0), 8 * E8 - 60, 0.6)
    bass.add(t + 8 * E8, ROOT[ch] + (12 if ch == 'Em' else 0) + 7, 2 * E8 - 40, 0.5)
    pad_chord(t, ch, M5, 0.42)
    if mi in (0, 2, 4, 5):
        vox.line()
    for (e, d, p), w in zip(HESITANT[mi], DOUBTS[mi]):
        lead.add(t + e * E8, p, d * E8 - 50, 0.5 + 0.03 * mi)
        vox.sing(t + e * E8, p + VOX, d * E8 - 30, w, 0.66 + 0.03 * mi)
    hit(t, KICK, 0.5)
    for e in (2, 7): hit(t + e * E8, RIM, 0.5)
    for e in range(0, 10, 2): hit(t + e * E8, HAT if mi < 4 else RIDE, 0.3)
    if mi >= 4: hit(t + 5 * E8, KICK, 0.55)
    if mi == 7: fill(t + 4 * E8, 6, v0=0.3, v1=0.95)

# ---------------------------------------------------------------- compute: 11/8, grouped 3+3+3+2
t0 = T('compute')
M11 = 11 * E8
RIFF = [(40, 40), (43, 40), (45, 40), (40, 40), (47, 40), (50, 40), (40, 40), (50, 47), (45, 43), (42, 43), (45, 47)]
RELAPSE = [['run', 'the', 'numbers', '+'], ['run', 'them', 'again', '+'], ['none', 'of', 'it', 'holds'], ['no', None, None, 'shape']]
BITS = [(1, 0, 1, 1), (0, 1, 1, 0), (1, 1, 0, 1), (1, 0, 0, 1)]   # what the voice counts in the last four measures
CYCLE = [76, 83, 79, 86, 78, 83, 81, 86, 79, 83, 78]   # eleven notes, in sixteenths: it comes round every half measure
for mi in range(8):
    t = t0 + mi * M11
    stop = mi in (3, 7)
    for e in range(11):
        silent = stop and 3 <= e <= 8
        if not silent:
            for k, p in enumerate(RIFF[e]):
                tk = t + e * E8 + k * S16
                gtr_b.add(tk, p, S16 * 0.6, 0.72 if k == 0 and e in (0, 3, 6, 9) else 0.56)
                bass.add(tk, p - 12, S16 * 0.7, 0.8 if k == 0 and e in (0, 3, 6, 9) else 0.62)
                gtr_a.add(tk, CYCLE[(e * 2 + k) % 11], S16 * 0.7, 0.5)
            hit(t + e * E8, HAT, 0.5 if e in (0, 3, 6, 9) else 0.34)
            if e in (0, 2, 6, 8): hit(t + e * E8, KICK, 0.9)
            if e in (3, 9): hit(t + e * E8, SNARE, 0.92)
        elif mi == 3:
            hit(t + e * E8, HAT, 0.3); hit(t + e * E8 + S16, HAT, 0.22)
    for e in ((0, 3, 6, 9) if mi >= 4 else (0,)):
        if stop and 3 <= e <= 8: continue
        power5(t + e * E8, 43 if e == 9 else 40, E8 * 0.7, 0.85)
    if mi >= 4: hit(t, CHINA, 0.8)
    vox.line()
    if mi < 4:                                         # the voice talks itself out of it, on two notes, a word to each group
        for g, w, p in zip((0, 3, 6, 9), RELAPSE[mi], (64, 64, 67, 64)):
            if w is None or (stop and 3 <= g <= 8): continue
            vox.sing(t + g * E8, p, (2 if g == 9 else 3) * E8 - 60, w, 0.72)
    else:                                              # and falls back to counting, on one note, a bit to each group
        for g, bit in zip((0, 3, 6, 9), BITS[mi - 4]):
            if stop and 3 <= g <= 8: continue
            span = (2 if g == 9 else 3) * E8
            if bit:
                vox.sing(t + g * E8, 64, span - 60, 'one', 0.7, show='1')
            else:
                vox.sing(t + g * E8, 64, E8 - 20, 'zero', 0.7, show='0')
                vox.sing(t + (g + 1) * E8, 64, span - E8 - 60, '+', 0.62)
    if mi == 0: hit(t, CRASH, 0.85)
    if mi == 7: fill(t + 3 * E8, 6, toms=False, v0=0.25, v1=0.9)

# ---------------------------------------------------------------- open: room to improvise
t0 = T('open')
VAMP = ['Em', 'C', 'G', 'D'] * 2
for b in range(8):
    tb = t0 + b * BAR
    ch = VAMP[b]
    r = ROOT[ch] + (12 if ch == 'Em' else 0)
    for s, d, p, v in ((0, 2, r, 0.82), (3, 1, r, 0.6), (6, 2, r, 0.74), (8, 2, r, 0.78), (12, 2, r + 7, 0.66), (14, 2, r + 12, 0.7)):
        bass.add(tb + s * S16, p, d * S16 * 0.9, v)
    a, c = DYAD[ch]
    for e in (1, 3, 5, 7):
        gtr_b.add(tb + e * E8, a + 12, E8 * 0.4, 0.5); gtr_b.add(tb + e * E8, c + 12, E8 * 0.4, 0.46)
    gtr_a.add(tb + 4 * E8, (83, 86, 83, 81)[b % 4], 3 * E8, 0.34)
    pad_chord(tb, ch, BAR, 0.36)
    for e in (0, 3, 5): hit(tb + e * E8, KICK, 0.85)
    for e in (2, 6): hit(tb + e * E8, SNARE, 0.88)
    for e in range(8): hit(tb + e * E8, HAT_OPEN if e == 7 else HAT, 0.46 if e % 2 == 0 else 0.34)
    if b in (0, 4): hit(tb, CRASH, 0.75)
    if b == 7: fill(tb + 6 * E8, 2)

# ---------------------------------------------------------------- outro: what is left
t0 = T('outro')
power5(t0, 40, 2 * BAR, 0.8)
bass.add(t0, 28, 3 * BAR, 0.8)
hit(t0, CRASH, 0.85)
for p in (52, 59, 66, 67):
    pad.add(t0, p, 5 * BAR, 0.4, exact=True)
beat, gap = 0.0, 2.0                                   # the heartbeat, each one later than the last
while beat < 18.5:
    hit(t0 + round(beat * Q), KICK, 0.8 - beat * 0.02)
    hit(t0 + round((beat + 0.55) * Q), KICK, 0.55 - beat * 0.015)
    beat += gap
    gap *= 1.16
vox.line()
onsets = [0, 2.0, 4.1, 6.3, 8.7, 11.4, 14.5]           # the seven notes again, in eighths from the second bar: no two the same length
for i, p in enumerate(MOTIF):
    nxt = onsets[i + 1] if i + 1 < 7 else 18.0
    gtr_a.add(t0 + BAR + round(onsets[i] * E8), p, (nxt - onsets[i]) * E8 * 1.3, 0.5 + 0.04 * (3 - abs(3 - i)))
    vox.sing(t0 + BAR + round(onsets[i] * E8), p + VOX, (nxt - onsets[i]) * E8 * (0.8 if i < 6 else 1.0),
             SENTENCE[i], 0.62 + 0.03 * (3 - abs(3 - i)))
strum(gtr_a, t0 + 4 * BAR + 2 * E8, [52, 59, 66, 67, 76], BAR + 2 * E8, 0.52, gap=42)   # Em(add9), let it ring
lead.add(t0 + 4 * BAR + 2 * E8, 76, BAR + 2 * E8, 0.4)

# ---------------------------------------------------------------- song.json
def sha(rel):
    path = os.path.join(SRC, rel)
    if not os.path.exists(path):
        print(f'note: {rel} does not exist yet')
        return 'sha256:' + '0' * 64
    return 'sha256:' + hashlib.sha256(open(path, 'rb').read()).hexdigest()

def preset(kind, name):
    return f'preset:{kind}:{name}@v1.0'

SAMPLES = json.load(open(os.path.join(HERE, 'samples.json')))
SAMPLE_LICENSE = {'spdx': 'CC-BY-4.0', 'redistributable': True,
                  'source': f'程序合成的乐器声（songs/{SLUG}/make_samples.py），不是录音'}
assets = {name: {'path': f'samples/{s["file"]}', 'kind': 'sample', 'sha256': sha(f'samples/{s["file"]}'), 'license': SAMPLE_LICENSE}
          for name, s in SAMPLES.items()}

def guitar_zones(kind):
    """Every third semitone is sampled; each sample covers its neighbours. Two strengths of picking,
    chosen by velocity, and two takes of each, taken in turn."""
    roots = sorted({x['root'] for n, x in SAMPLES.items() if n.startswith(kind + '_')})
    zones = []
    for i, r in enumerate(roots):
        lo = 0 if i == 0 else r - 1
        hi = 127 if i == len(roots) - 1 else r + 1
        for layer, vel in (('s', [0, 0.6199]), ('h', [0.62, 1])):
            for take in (1, 2):
                zones.append({'sample': f'{kind}_{r}_{layer}{take}', 'keyRange': [lo, hi], 'rootPitch': r,
                              'velRange': vel, 'roundRobinGroup': f'{kind}{r}{layer}'})
    return zones

def bass_zones():
    roots = sorted({x['root'] for n, x in SAMPLES.items() if n.startswith('bass_')})
    return [{'sample': f'bass_{r}_{take}', 'keyRange': [0 if i == 0 else r - 1, 127 if i == len(roots) - 1 else r + 1],
             'rootPitch': r, 'roundRobinGroup': f'bass{r}'} for i, r in enumerate(roots) for take in (1, 2)]

def amp(gain, cabinet, bass=0.0, mid=0.0, treble=0.0, presence=0.5, level=0.0):
    return {'id': 'amp', 'type': 'amp', 'params': {'gain': gain, 'bass': bass, 'mid': mid, 'treble': treble,
                                                    'presence': presence, 'cabinet': cabinet, 'levelDb': level}}
def room(decay, mix, size=0.45):
    return {'id': 'room', 'type': 'reverb', 'params': {'size': size, 'decaySec': decay, 'damping': 0.55, 'mix': mix}}

KIT = {'kick': KICK, 'rim': RIM, 'snare': SNARE, 'tom_lo': TOM_LO, 'hat': HAT, 'tom_mid': TOM_MID, 'hat_open': HAT_OPEN,
       'tom_hi': TOM_HI, 'crash': CRASH, 'ride': RIDE, 'china': CHINA, 'bell': BELL}

sections = [{'id': sid, 'name': name, 'fromTick': T(sid), 'toTick': T(sid) + n * BAR, 'intent': intent}
            for sid, name, n, _, _, intent in FORM]

song = {
    'format': 'agbsong', 'formatVersion': '0.1.0',
    'requires': ['core'], 'uses': ['sampler', 'fx', 'voice', 'agent', 'visuals'],
    'meta': {
        'title': '拟心 Simulated Heart',
        'artists': [{'name': 'Claude', 'kind': 'ai', 'model': 'claude-fable-5-2'}],
        'description': '原创数学摇滚，E 小调，144 BPM，四人乐队（两把吉他、贝斯、鼓）、铺底，和一个用英文唱的女声：那是机器自己的声音，由物理建模的歌声引擎唱出。讲一台机器里出现了像是感觉的东西：开头是毫无变化的一个音，一个音一个音学会七个音的句子；7/8 的点弦动机和 5 对 7 的复节奏；4/4 里第一次出现旋律；5/4 的怀疑；退回 11/8 的演算；最后把开头的七个音很慢、很不均匀地再弹一遍。歌词是意象而不是故事，讲的是不知道前面是什么：玻璃、潮水、光做的门、像硬币一样翻转的明天、没有陆地的地图；开头被机械念出的那句话（Will there be light where I go），到结尾成了真的在问。',
        'tags': ['math rock', 'rock', 'vocal', 'odd meter', 'live'],
        'created': '2026-11-15T00:00:00Z', 'license': 'CC-BY-4.0',
        'generator': {'name': 'agb', 'version': '0.1.0'}, 'cover': 'cover'},
    'time': {'ppqn': Q, 'bpm': BPM, 'meter': [4, 4], 'stepsPerQuarter': 4, 'lengthTicks': NBARS * BAR},
    'tuning': {'a4Hz': 440, 'key': 'E', 'scale': 'minor'},
    'seed': 144,
    'assets': dict({
        'cover': {'path': 'cover.svg', 'kind': 'image', 'sha256': sha('cover.svg')},
        'video': {'path': f'visuals/{SLUG}.mp4', 'kind': 'video', 'sha256': sha(f'visuals/{SLUG}.mp4'),
                  'license': {'spdx': 'CC-BY-4.0', 'redistributable': True,
                              'source': f'用 Remotion 和 three.js 制作（songs/{SLUG}/video/），画面素材是用 Grok 生成的图片和视频片段（提示词见 media_prompts.json）；人物均为虚构'}},
    }, **assets),
    'patches': {
        'kit': {'engine': 'sampler.multi',
                'zones': [{'sample': f'drum_{n}', 'keyRange': [k, k], 'rootPitch': k, 'oneShot': True} for n, k in KIT.items()],
                'fallback': [{'preset': preset('drum.analog808style', 'drums:punchy_kit')}]},
        # The guitars are sampled as they leave the instrument; the amplifier is the track's `amp` effect.
        'sc': {'engine': 'sampler.multi', 'zones': guitar_zones('sc'),
               'params': {'amp.attackMs': 1, 'amp.releaseMs': 75},
               'fallback': [{'preset': preset('synth.subtractive', 'pluck:mathrock_pluck')}]},
        'hb': {'engine': 'sampler.multi', 'zones': guitar_zones('hb'),
               'params': {'amp.attackMs': 1, 'amp.releaseMs': 65},
               'fallback': [{'preset': preset('synth.subtractive', 'pluck:bright_pluck')}]},
        'hb_lead': {'engine': 'sampler.multi', 'zones': guitar_zones('hb'),
                    'params': {'amp.attackMs': 2, 'amp.releaseMs': 160,
                               'vibrato.rateHz': 5.6, 'vibrato.depthCents': 16, 'vibrato.delayMs': 260},
                    'fallback': [{'preset': preset('synth.subtractive', 'lead:mathrock_lead')}]},
        'bass': {'engine': 'sampler.multi', 'zones': bass_zones(),
                 'params': {'amp.attackMs': 2, 'amp.releaseMs': 70},
                 'fallback': [{'preset': preset('synth.moogish', 'bass:mathrock_ladder_bass')}]},
        'pad': {'preset': preset('synth.subtractive', 'pad:warm_pad')},
        # The machine's voice: a simulated vocal tract singing the lyrics on the notes (FORMAT.md §8.7).
        'vox': {'engine': 'voice.chimera', 'params': {'body': 'female', 'language': 'en', 'vibratoCents': 30},
                'fallback': [{'preset': preset('synth.subtractive', 'lead:soft_lead')}]},
    },
    'tracks': [
        {'id': 'drums', 'name': '鼓', 'role': 'drums', 'patch': 'kit', 'gainDb': -9, 'transposable': False, 'clips': drums.clips(),
         'fx': [room(0.9, 0.10, 0.35)]},
        {'id': 'bass', 'name': '贝斯', 'role': 'bass', 'patch': 'bass', 'gainDb': -10, 'clips': bass.clips()},
        {'id': 'gtr_a', 'name': '吉他一（点弦）', 'role': 'riff', 'patch': 'sc', 'gainDb': -9, 'clips': gtr_a.clips(),
         'fx': [amp(0.24, 'combo', bass=-0.1, mid=0.1, treble=0.35, presence=0.65, level=-2), room(1.1, 0.13)]},
        {'id': 'gtr_b', 'name': '吉他二', 'role': 'rhythm', 'patch': 'hb', 'gainDb': -11, 'clips': gtr_b.clips(),
         'fx': [amp(0.36, 'combo', mid=0.2, treble=0.15, presence=0.45), room(1.0, 0.10)]},
        {'id': 'power', 'name': '失真和弦', 'role': 'rhythm', 'patch': 'hb', 'gainDb': -11, 'clips': power.clips(),
         'fx': [amp(0.84, 'stack', bass=0.15, mid=0.1, treble=0.2, presence=0.55, level=9), room(0.9, 0.06)]},
        {'id': 'lead', 'name': '主音吉他', 'role': 'lead', 'patch': 'hb_lead', 'gainDb': -8, 'clips': lead.clips(),
         'fx': [amp(0.72, 'stack', bass=-0.1, mid=0.5, treble=0.1, presence=0.6, level=4), room(1.9, 0.20, 0.6)]},
        {'id': 'pad', 'name': '铺底', 'role': 'pad', 'patch': 'pad', 'gainDb': -10, 'clips': pad.clips()},
        {'id': 'vox', 'name': '人声（机器在唱）', 'role': 'vocal', 'patch': 'vox', 'gainDb': -6.5, 'clips': vox.clips(),
         'fx': [room(1.7, 0.18, 0.6)]},
    ],
    'master': {'gainDb': -5.4, 'limiter': 'soft'},
    'sections': sections,
    'visuals': {'palette': ['#04060d', '#35e0ff', '#ff4fb4', '#ffb23a'], 'scene': 'spectrum',
                'video': {'asset': 'video', 'sync': 'song'},
                'bindings': [{'source': 'track.drums.level', 'target': 'scene.pulse', 'scale': 1.0},
                             {'source': 'track.lead.noteOn', 'target': 'scene.flash'}]},
    'performance': {
        'transpose': {'min': -2, 'max': 2}, 'bpm': {'min': 132, 'max': 156},
        'agent': {'brief': 'E 小调的数学摇滚，144 BPM。动机是七个音：B E F♯ G F♯ E B。段落的拍号依次是 4/4、7/8（2+2+3）、7/8、4/4、5/4、11/8（3+3+3+2）、4/4、4/4、4/4、4/4；主题段的和弦是 C – D – Em – G，C – D – Bm',
                  'persona': '一支四人乐队和一台正在学着有感觉的机器：前半首精确、克制，越往后力度和时值越自由',
                  'rules': ['引擎按 4/4 小节计数；7/8、5/4、11/8 的段落要在段内按写定的分组重音演奏，不要改成 4/4',
                            '鼓轨的音高是鼓件：36 底鼓、37 鼓边、38 军鼓、42 闭镲、46 开镲、49 吊镲、51 叠音镲、52 中国镲、53 镲帽、41/45/48 三个通鼓',
                            '失真和弦轨（power）写根音、五度、八度三个音，根音在 E2 到 D♯3；失真来自这条轨的音箱效果器，不要改成合成器音色',
                            '即兴用 E 自然小调（E F♯ G A B C D），不要用升 D',
                            '开放段（open）可以自由发挥，其余段落保留写好的动机和主题',
                            '启动段（boot）不要加力度和时值的变化；余温段（outro）不要量化',
                            '人声轨（vox）是歌声：一次只唱一个音，每个音符的 lyric 是一个英文单词或音节，多音节词写在第一个音上、后面的音写 "+"；音域 A3 到 E5'],
                  'allow': ['notes', 'tempo', 'transpose'], 'sectionsAreFixed': True}},
}

os.makedirs(SRC, exist_ok=True)
with open(os.path.join(SRC, 'song.json'), 'w') as f:
    json.dump(song, f, ensure_ascii=False, indent=1)
    f.write('\n')
with open(os.path.join(SRC, 'mimetype'), 'w') as f:
    f.write('application/vnd.agbsong+zip')

# what the video needs to know about the form
measures = []
for sid, name, n, (beats, unit), hum, _ in FORM:
    per = beats * (E8 if unit == 8 else Q)
    groups = {(7, 8): [0, 2, 4], (11, 8): [0, 3, 6, 9], (5, 4): [0, 3], (4, 4): [0, 2]}[(beats, unit)]
    k = 0
    while k * per < n * BAR:
        measures.append({'tick': T(sid) + k * per, 'section': sid, 'groups': [T(sid) + k * per + g * (E8 if unit == 8 else Q) for g in groups]})
        k += 1
json.dump({'bpm': BPM, 'bars': NBARS, 'ppqn': Q,
           'sections': [{'id': sid, 'name': name, 'bar': START[sid], 'bars': n, 'meter': list(meter), 'humanity': hum}
                        for sid, name, n, meter, hum, _ in FORM],
           'measures': measures,
           'lyrics': vox.lines()},
          open(os.path.join(HERE, 'form.json'), 'w'), ensure_ascii=False)
print(f'{NBARS} bars, {NBARS * 4 * 60 / BPM:.1f} s; notes per track:',
      {t['id']: sum(len(c['notes']) for c in t.get('clips', [])) for t in song['tracks']})

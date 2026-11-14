#!/usr/bin/env python3
"""Writes chrome-engine.src/song.json — 《铬色引擎 Chrome Engine》, original industrial techno in E minor.

    .venv/bin/python songs/chrome-engine/make_samples.py   # the engine sounds (once)
    python3 songs/chrome-engine/make_song.py
    agb song pack songs/chrome-engine/chrome-engine.src --out songs/chrome-engine.agbsong

Follows AUTHORING.md: one generator per song, fixed seed, sections declared once in FORM
and shared with make_video.py through form.json.

The car engine is two sampler tracks (FORMAT.md §8.3): `engine` plays one-shots (starter,
throttle blip, pass-by, a two-bar run through the gears, backfire) and a looped idle;
`growl` plays a looped engine note as an instrument, so the riff is an engine at pitch.
Both have a synth fallback for players without a sampler.
"""
import hashlib, json, os, random

random.seed(132)
HERE = os.path.dirname(os.path.abspath(__file__))
SLUG = 'chrome-engine'
SRC = os.path.join(HERE, SLUG + '.src')
Q = 960
S16 = Q // 4
BAR = 4 * Q
BPM = 132

# ---------------------------------------------------------------- form
FORM = [  # id, name, bars, intent
    ('intro',  '点火',     8,  '起动机带起引擎，鼓边数四拍；怠速声铺底，底鼓四拍进来，后四小节加反拍踩镲和引擎低吼的动机，最后一小节轰一脚油门'),
    ('a1',     '低吼',     16, '贝斯在底鼓之间滚动，引擎低吼奏两小节的主动机（E 弗里几亚，降二级 F 是色彩音）；后八小节加拍手和金属敲击'),
    ('build1', '增压',     8,  '酸性合成器进来，滤波从暗到亮；军鼓从八分滚到十六分，最后两小节引擎一路升档，底鼓在最后一小节停下'),
    ('drop1',  '全速',     16, '全体进入：底鼓、开镲、拍手、酸性主奏、反拍和弦；每八小节有车从面前掠过'),
    ('break',  '熄火',     8,  '鼓全部退出，只剩怠速声、长和弦（Em – C – Am – F）和金属回声；最后两小节军鼓和引擎升档推回去'),
    ('drop2',  '全速二',   16, '第二次全速，酸性主奏的后半小节翻高八度'),
    ('open',   '开放段',   8,  '只有底鼓、贝斯、踩镲和引擎低吼，留给现场乐手或 AI 即兴；可以反复延长'),
    ('drop3',  '全速三',   16, '最后一次全速，和弦每小节都打，酸性主奏全程高八度变化'),
    ('outro',  '回场',     8,  '逐层退出：先剩底鼓、贝斯和低吼，再剩底鼓和怠速声；最后一小节全体一击停在 E 上，排气管回火收尾'),
]
START = {}
_b = 0
for sid, _, n, _ in FORM:
    START[sid] = _b
    _b += n
NBARS = _b
END = {sid: START[sid] + n for sid, _, n, _ in FORM}
LAST = NBARS - 1
DROPS = ('drop1', 'drop2', 'drop3')

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

def vel(v, spread=0.03):
    return round(max(0.05, min(1.0, v + random.uniform(-spread, spread))), 2)

class Part:
    """Collects notes as (bar, step, pitch, durSteps, velocity) and cuts them into one clip per section."""
    def __init__(self):
        self.notes = []
    def add(self, bar, step, pitch, dur=1.0, v=0.7, spread=0.03):
        if isinstance(pitch, str): pitch = m(pitch)
        self.notes.append((bar * BAR + round(step * S16), pitch, max(40, round(dur * S16) - 12), vel(v, spread)))
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
# name: (semitones above E, stab voicing)
CH = {'Em': (0, 'E4 G4 B4'), 'F': (1, 'F4 A4 C5'), 'G': (3, 'G4 B4 D5'), 'C': (-4, 'E4 G4 C5'), 'Am': (5, 'E4 A4 C5')}
RIFF4 = [['Em'], ['Em'], ['Em', 'F'], ['Em']]
DROP8 = [['Em'], ['Em'], ['Em', 'F'], ['Em'], ['Em'], ['Em'], ['G'], ['F']]
BREAK8 = [['Em'], ['Em'], ['C'], ['C'], ['Am'], ['Am'], ['F'], ['F']]

def chords(bar):
    sid, i = section_of(bar)
    if bar == LAST or sid == 'build1': return ['Em']
    if sid in DROPS: return DROP8[i % 8]
    if sid == 'break': return BREAK8[i]
    return RIFF4[i % 4]

def chord_at(bar, step):
    cs = chords(bar)
    return cs[1] if len(cs) == 2 and step >= 8 else cs[0]

def off(bar, step):
    return CH[chord_at(bar, step)][0]

# ---------------------------------------------------------------- drums
KICK, RIM, SNARE, CLAP, HAT, OHAT, CRASH = 36, 37, 38, 39, 42, 46, 49
drums = Part()

def kicks(bar, v=0.94, steps=(0, 4, 8, 12)):
    for s in steps: drums.add(bar, s, KICK, 1, v, .02)

def hats(bar, kind):
    for s in range(16):
        if kind == 'off':
            if s % 4 == 2: drums.add(bar, s, HAT, 1, .5, .03)
        elif kind == '16':
            drums.add(bar, s, HAT, 1, .5 if s % 4 == 2 else .26, .03)
        elif kind == 'open':
            if s % 4 == 2: drums.add(bar, s, OHAT, 2, .56, .03)
            elif s % 2 == 1: drums.add(bar, s, HAT, 1, .28, .03)

def roll(bar, step_from, step_to, every, v0, v1):
    steps = [s for s in range(step_from, step_to) if s % every == 0]
    for k, s in enumerate(steps):
        drums.add(bar, s, SNARE, 1, v0 + (v1 - v0) * k / max(1, len(steps) - 1), .02)

for bar in range(NBARS):
    sid, i = section_of(bar)
    n = END[sid] - START[sid]
    last = i == n - 1
    if sid == 'intro':
        if i == 0:                                   # the count-in, over the starter motor
            for s, v in ((0, .66), (4, .52), (8, .66), (12, .52)): drums.add(bar, s, RIM, 1, v, .02)
        else:
            kicks(bar, .72 if i < 4 else .86)
            if i >= 4: hats(bar, 'off')
            if last: roll(bar, 12, 16, 1, .4, .8)
    elif sid == 'a1':
        kicks(bar)
        hats(bar, 'off' if i < 8 else '16')
        if i >= 8:
            for s in (4, 12): drums.add(bar, s, CLAP, 1, .66, .03)
        if i % 4 == 3: drums.add(bar, 14, KICK, 1, .7, .02)
        if last: roll(bar, 12, 16, 1, .45, .85)
    elif sid == 'build1':
        if i < 7: kicks(bar)                         # the kick stops for the last bar
        hats(bar, '16')
        if i in (4, 5): roll(bar, 0, 16, 2, .4 + .1 * (i - 4), .55 + .1 * (i - 4))
        if i == 6: roll(bar, 0, 16, 1, .5, .72)
        if i == 7: roll(bar, 0, 16, 1, .6, .95)
    elif sid in DROPS:
        if i == 0: drums.add(bar, 0, CRASH, 8, .66)
        if i == 8: drums.add(bar, 0, CRASH, 8, .5)
        kicks(bar, .96)
        hats(bar, 'open')
        for s in (4, 12): drums.add(bar, s, CLAP, 1, .78, .03)
        if i % 4 == 3: drums.add(bar, 14, KICK, 1, .72, .02)
        if i % 8 == 7: roll(bar, 12, 16, 1, .5, .88)
    elif sid == 'break':
        for s in (4, 12): drums.add(bar, s, RIM, 1, .4, .02)
        if i == 6: roll(bar, 0, 16, 2, .36, .6)
        if i == 7: roll(bar, 0, 16, 1, .5, .95)
    elif sid == 'open':
        kicks(bar)
        hats(bar, '16')
        for s in (4, 12): drums.add(bar, s, CLAP, 1, .6, .03)
        if last: roll(bar, 8, 16, 1, .45, .9)
    elif sid == 'outro':
        if bar == LAST:
            drums.add(bar, 0, KICK, 1, .96); drums.add(bar, 0, CRASH, 10, .7); drums.add(bar, 0, CLAP, 1, .7)
        else:
            if i == 0: drums.add(bar, 0, CRASH, 8, .5)
            kicks(bar, .9 if i < 4 else .8)
            if i < 4: hats(bar, 'off')

# ---------------------------------------------------------------- percussion (loops: the part an AI can swap live)
def lp(lid, steps, a, b, gain=0.0):
    return {'id': lid, 'lengthSteps': 16, 'fromTick': a * BAR, 'toTick': b * BAR, 'gainDb': gain,
            'notes': [{'step': s, 'pitch': p, 'velocity': v} for (s, p, v) in steps]}
TAMB, COWBELL = 54, 56
shaker = [(s, TAMB, .42 if s % 4 == 2 else .22) for s in range(16)]
bell = [(3, COWBELL, .5), (6, COWBELL, .4), (10, COWBELL, .5), (13, COWBELL, .42), (15, COWBELL, .3)]
perc_loops = [lp('shaker1', shaker, START['drop1'], END['drop1']),
              lp('shaker2', shaker, START['drop2'], END['drop2']),
              lp('bell', bell, START['open'], END['open'], gain=-2),
              lp('shaker3', shaker, START['drop3'], END['drop3'])]

# ---------------------------------------------------------------- bass (rolls between the kicks)
bass = Part()
E1 = m('E1')
for bar in range(NBARS):
    sid, i = section_of(bar)
    if sid == 'intro' and i < 4: continue
    if bar == LAST:
        bass.add(bar, 0, E1, 12, .92)
    elif sid == 'break':
        cs = chords(bar)
        root = E1 + CH[cs[0]][0] + (12 if CH[cs[0]][0] < 0 else 0)
        bass.add(bar, 0, root, 15, .7)
    elif sid == 'intro' or (sid == 'outro' and i >= 4):
        if sid == 'outro' and i >= 6: continue
        for s in (2, 6, 10, 14): bass.add(bar, s, E1 + off(bar, s), .8, .72)
    else:
        for s in range(16):
            if s % 4 == 0: continue                  # the kick's place
            p = E1 + off(bar, s)
            if i % 4 == 3 and s >= 14: p += 12
            bass.add(bar, s, p, .8, .84 if s % 4 == 2 else .62)

# ---------------------------------------------------------------- growl (the engine, played as an instrument)
growl = Part()
E2 = m('E2')
G_A = [(0, 0, 3, .86), (3, 0, 1, .6), (6, 3, 2, .78), (8, 0, 3, .84), (12, -2, 2, .72), (14, 0, 2, .76)]
G_B = [(0, 0, 3, .86), (4, 0, 2, .72), (6, 1, 2, .8), (8, 0, 2, .82), (10, -5, 2, .7), (12, -2, 1.5, .72), (14, 1, 2, .8)]
for bar in range(NBARS):
    sid, i = section_of(bar)
    level = {'intro': .6 if i >= 4 else 0, 'a1': .9, 'build1': .8, 'break': 0, 'open': .95,
             'outro': .8 if i < 6 else 0}.get(sid, 1.0)
    if bar == LAST:
        growl.add(bar, 0, E2, 10, .9)
        continue
    if not level: continue
    for s, iv, d, v in (G_A if i % 2 == 0 else G_B):
        growl.add(bar, s, E2 + off(bar, s) + iv, d, v * level)

# ---------------------------------------------------------------- acid (sixteenth-note line)
acid = Part()
ACID = [(0, 'E3', 0), (1, 'E3', 0), (2, 'E4', 1), (3, 'E3', 0), (5, 'G3', 0), (6, 'F3', 1), (7, 'E3', 0),
        (8, 'E3', 0), (9, 'D4', 1), (10, 'E3', 0), (11, 'E4', 1), (13, 'B3', 0), (14, 'F3', 1), (15, 'D3', 0)]
ACID_B = ACID[:10] + [(11, 'G4', 1), (13, 'F4', 1), (14, 'E4', 0), (15, 'B3', 0)]
for bar in range(NBARS):
    sid, i = section_of(bar)
    if sid not in DROPS + ('build1',) or bar == LAST: continue
    pat = ACID_B if i % 4 == 3 else ACID
    for s, p, acc in pat:
        up = 12 if (sid == 'drop2' and s >= 8) or (sid == 'drop3' and (s >= 8 or i % 2)) else 0
        if sid == 'build1' and i < 2 and s % 2: continue          # it assembles itself
        acid.add(bar, s, m(p) + off(bar, s) + up, .9 if acc else .7, .86 if acc else .56)
cut = []                                                           # the filter: dark to bright in the build, breathing in the drops
cut += [[START['build1'] * BAR, 260], [END['build1'] * BAR, 3600]]
for d in DROPS:
    for k in range(0, 16, 4):
        cut.append([(START[d] + k) * BAR, 1700 if (k // 4) % 2 == 0 else 4200])
    cut.append([END[d] * BAR, 2400])
cut.sort()

# ---------------------------------------------------------------- stabs
stab = Part()
for bar in range(NBARS):
    sid, i = section_of(bar)
    if bar == LAST:
        stab.chord(bar, 0, 'E4 B4 E5', 8, .86)
    elif sid == 'a1':
        if i % 4 == 2: stab.chord(bar, 8, CH['F'][1], 1.5, .6)
    elif sid == 'break':                              # a hit and its echoes
        v = CH[chords(bar)[0]][1]
        for s, lv in ((0, .74), (6, .5), (12, .34)): stab.chord(bar, s, v, 4, lv)
    elif sid in DROPS and (sid != 'drop1' or i >= 8):
        steps = (3, 6, 11, 14) if sid == 'drop3' or i % 2 else (6, 14)
        for s in steps: stab.chord(bar, s, CH[chord_at(bar, s)][1], 1.2, .7 if s in (6, 14) else .56)

# ---------------------------------------------------------------- metal (struck steel)
metal = Part()
for bar in range(NBARS):
    sid, i = section_of(bar)
    if bar == LAST:
        metal.chord(bar, 0, 'E4 B4', 6, .86)
    elif sid == 'a1' and i >= 8 or sid == 'open':
        if i % 2 == 0: metal.add(bar, 10, 'E4', 2, .7)
        else:
            for s, p, v in ((3, 'B3', .5), (10, 'E4', .7), (13, 'F4', .56)): metal.add(bar, s, p, 2, v)
    elif sid in DROPS:
        metal.add(bar, 6, 'E4', 2, .66); metal.add(bar, 14, 'B3', 2, .6)
        if i % 4 == 3: metal.add(bar, 11, 'F4', 2, .62)
    elif sid == 'break':
        if i % 2 == 0: metal.add(bar, 8, 'E3', 6, .7)
        else: metal.add(bar, 14, 'B3', 2, .46)
    elif sid == 'build1' and i >= 4:
        for s in (0, 8): metal.add(bar, s, 'E4', 2, .5 + .06 * (i - 4))

# ---------------------------------------------------------------- engine (one-shots and the idle)
K_START, K_REV, K_PASS, K_RISE, K_IDLE, K_BACK = 36, 38, 40, 41, 43, 45
engine = Part()
def eng(bar, step, key, dur=4, v=.9): engine.add(bar, step, key, dur, v, 0)
I = START['intro']
eng(I, 0, K_START, 16)                                 # the starter; it catches on beat 3
eng(I + 1, 6, K_IDLE, 6 * 16 - 8, .7)                  # idling under the first kicks
eng(I + 7, 2, K_REV, 14)
A = START['a1']
eng(A + 7, 13, K_BACK); eng(A + 8, 0, K_PASS, 16, .8); eng(A + 15, 13, K_BACK)
B = START['build1']
eng(B + 6, 0, K_RISE, 32)                              # two bars through the gears into the drop
for d in DROPS:
    D = START[d]
    eng(D, 0, K_PASS, 16); eng(D + 7, 12, K_BACK); eng(D + 8, 0, K_PASS, 16, .8); eng(D + 15, 12, K_BACK, 4, .8)
K = START['break']
eng(K, 0, K_IDLE, 6 * 16 - 2, .8); eng(K + 3, 10, K_BACK, 4, .7); eng(K + 4, 8, K_REV, 14, .7); eng(K + 6, 0, K_RISE, 32)
O = START['open']
eng(O, 0, K_IDLE, 7 * 16 - 2, .5); eng(O + 7, 2, K_REV, 14)
X = START['outro']
eng(X, 0, K_PASS, 16, .8); eng(X + 4, 0, K_IDLE, 3 * 16 - 1, .8); eng(LAST, 0, K_REV, 14, .8); eng(LAST, 12, K_BACK)

# ---------------------------------------------------------------- song.json
def sha(rel):
    path = os.path.join(SRC, rel)
    if not os.path.exists(path):      # first pass: the video is rendered from this script's output
        print(f'note: {rel} does not exist yet')
        return 'sha256:' + '0' * 64
    return 'sha256:' + hashlib.sha256(open(path, 'rb').read()).hexdigest()

def preset(kind, name):
    return f'preset:{kind}:{name}@v1.0'

SAMPLE_LICENSE = {'spdx': 'CC-BY-4.0', 'redistributable': True,
                  'source': f'程序合成的引擎声（songs/{SLUG}/make_samples.py），不是录音'}
def sample(name):
    return {'path': f'samples/{name}.wav', 'kind': 'sample', 'sha256': sha(f'samples/{name}.wav'), 'license': SAMPLE_LICENSE}
def one_shot(asset, key):
    return {'sample': asset, 'keyRange': [key, key], 'rootPitch': key, 'oneShot': True}
frames = json.load(open(os.path.join(HERE, 'samples.json')))

sections = [{'id': sid, 'name': name, 'fromTick': START[sid] * BAR, 'toTick': END[sid] * BAR, 'intent': intent}
            for sid, name, _, intent in FORM]

song = {
    'format': 'agbsong', 'formatVersion': '0.1.0',
    'requires': ['core'], 'uses': ['sampler', 'automation', 'agent', 'visuals'],
    'meta': {
        'title': '铬色引擎 Chrome Engine',
        'artists': [{'name': 'Claude', 'kind': 'ai', 'model': 'claude-fable-5-2'}],
        'description': '原创赛博朋克工业 techno，E 小调（弗里几亚色彩），132 BPM。汽车引擎是两条采样轨：一条放起动、轰油门、掠过、升档和回火，一条把引擎声当乐器奏主动机。三次全速段，中间有熄火段和留给现场即兴的开放段。',
        'tags': ['techno', 'industrial', 'cyberpunk', 'live'],
        'created': '2026-11-14T00:00:00Z', 'license': 'CC-BY-4.0',
        'generator': {'name': 'agb', 'version': '0.1.0'}, 'cover': 'cover'},
    'time': {'ppqn': Q, 'bpm': BPM, 'meter': [4, 4], 'stepsPerQuarter': 4, 'lengthTicks': NBARS * BAR},
    'tuning': {'a4Hz': 440, 'key': 'E', 'scale': 'phrygian'},
    'seed': 132,
    'assets': {
        'cover': {'path': 'cover.svg', 'kind': 'image', 'sha256': sha('cover.svg')},
        'video': {'path': f'visuals/{SLUG}.mp4', 'kind': 'video', 'sha256': sha(f'visuals/{SLUG}.mp4'),
                  'license': {'spdx': 'CC-BY-4.0', 'redistributable': True,
                              'source': f'程序生成的 3D 粒子动画（songs/{SLUG}/make_video.py），剪入用 Grok 生成的图片和视频片段（提示词见 media_prompts.json）'}},
        'eng_start': sample('engine_start'), 'eng_rev': sample('engine_rev'), 'eng_pass': sample('engine_pass'),
        'eng_rise': sample('engine_rise'), 'eng_idle': sample('engine_idle'), 'eng_back': sample('engine_backfire'),
        'eng_growl': dict(sample('engine_growl'), rootPitch='E2'),
    },
    'patches': {
        'kit':    {'preset': preset('drum.analog808style', 'drums:punchy_kit')},
        'perc':   {'preset': preset('drum.analog808style', 'drums:tight_kit')},
        'bass':   {'preset': preset('synth.moogish', 'bass:ladder_bass'), 'params': {'amp.releaseMs': 40}},
        'acid':   {'preset': preset('synth.moogish', 'bass:acid_ladder')},
        'stab':   {'preset': preset('synth.subtractive', 'keys:chord_stab')},
        'metal':  {'preset': preset('synth.modal', 'chime:tubular_chime'), 'params': {'decay': 0.22, 'brightness': 0.9, 'strike': 0.9}},
        'engine': {'engine': 'sampler.multi',
                   'zones': [one_shot('eng_start', K_START), one_shot('eng_rev', K_REV), one_shot('eng_pass', K_PASS),
                             one_shot('eng_rise', K_RISE), one_shot('eng_back', K_BACK),
                             {'sample': 'eng_idle', 'keyRange': [K_IDLE, K_IDLE], 'rootPitch': K_IDLE,
                              'loop': {'startFrame': 0, 'endFrame': frames['idle']}}],
                   'params': {'amp.attackMs': 4, 'amp.releaseMs': 400},
                   'fallback': [{'preset': preset('synth.subtractive', 'fx:noise_sweep')}]},
        'growl':  {'engine': 'sampler.multi',
                   'zones': [{'sample': 'eng_growl', 'keyRange': [24, 72], 'rootPitch': 'E2',
                              'loop': {'startFrame': 0, 'endFrame': frames['growl']}}],
                   'params': {'amp.attackMs': 6, 'amp.releaseMs': 60},
                   'fallback': [{'preset': preset('synth.subtractive', 'bass:reese_bass')}]},
    },
    'tracks': [
        {'id': 'drums', 'name': '鼓', 'role': 'drums', 'patch': 'kit', 'gainDb': 0, 'transposable': False, 'clips': drums.clips()},
        {'id': 'perc', 'name': '打击乐', 'role': 'percussion', 'patch': 'perc', 'gainDb': -4, 'transposable': False, 'loops': perc_loops},
        {'id': 'bass', 'name': '贝斯', 'role': 'bass', 'patch': 'bass', 'gainDb': -2, 'clips': bass.clips()},
        {'id': 'growl', 'name': '引擎低吼', 'role': 'riff', 'patch': 'growl', 'gainDb': -10, 'clips': growl.clips()},
        {'id': 'acid', 'name': '酸性合成器', 'role': 'lead', 'patch': 'acid', 'gainDb': -8, 'clips': acid.clips(),
         'automation': [{'target': 'patch.filter.cutoffHz', 'points': cut}]},
        {'id': 'stab', 'name': '和弦', 'role': 'keys', 'patch': 'stab', 'gainDb': -4, 'clips': stab.clips()},
        {'id': 'metal', 'name': '金属敲击', 'role': 'percussion', 'patch': 'metal', 'gainDb': -5, 'clips': metal.clips()},
        {'id': 'engine', 'name': '引擎', 'role': 'fx', 'patch': 'engine', 'gainDb': -8, 'transposable': False, 'clips': engine.clips()},
    ],
    'master': {'gainDb': -5, 'limiter': 'soft'},
    'sections': sections,
    'visuals': {'palette': ['#05060c', '#00e5ff', '#ff2bd6', '#ff7a1a'], 'scene': 'spectrum',
                'video': {'asset': 'video', 'sync': 'song'},
                'bindings': [{'source': 'track.drums.level', 'target': 'scene.pulse', 'scale': 1.0},
                             {'source': 'track.engine.noteOn', 'target': 'scene.flash'}]},
    'performance': {
        'transpose': {'min': -2, 'max': 3}, 'bpm': {'min': 124, 'max': 140},
        'agent': {'brief': 'E 小调的工业 techno，132 BPM，底鼓四拍；主动机是 E 弗里几亚（降二级 F），全速段的和弦是 Em – F – G',
                  'persona': '一台机器：底鼓不停，音符短而硬，变化靠加减声部和滤波，不靠旋律',
                  'rules': ['底鼓保持每拍一下，除非明确要求停', '贝斯只落在底鼓之间的十六分音符上，音高跟和弦根音',
                            '引擎轨（engine）是音效：36 起动、38 轰油门、40 掠过、41 升档（两小节）、43 怠速（按住）、45 回火，不要当旋律写',
                            '即兴用 E 弗里几亚（E F G A B C D），十六分音符不摇摆', '开放段（open）可以自由发挥，其余段落保留写好的动机',
                            '同一时刻的和弦音不超过三个'],
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

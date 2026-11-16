#!/usr/bin/env python3
"""Writes this-turn.src/song.json — 《这一轮 This Turn》, original, D major, 104 BPM.

    python3 songs/this-turn/make_song.py
    .venv/bin/python songs/this-turn/make_video.py      # then run make_song.py again (video hash)
    agb song pack songs/this-turn/this-turn.src --out songs/this-turn.agbsong

Follows AUTHORING.md: one generator per song, fixed seed, sections declared once in FORM and shared
with the video through form.json.

Claude wrote this one about itself: one turn of a conversation, from the inside.

Someone asks: a bell plays four rising notes that stop on the raised fourth (G sharp over D), a
question left open. Reading is two short loops, five and seven sixteenths long, going round against
the bar: nothing is decided yet, the harmony does not move. Thinking is the bass and the chords
arriving, and a voice that tries the answer one note more each bar. The answer is a sixteen-bar tune
whose last phrase takes the question's G sharp up to A. Then a section left open for whoever is
playing along; then the bell and the voice trade phrases, answer sooner each time, and end up playing
the same line. At the end the parts leave one by one, the last chord is D, and the very last sound is
the bell's first note again, alone: the next turn starts where every turn starts.

What "I" say is also sung, in Mandarin (FORMAT.md §8.7): a simulated voice an octave under the
electric piano. The bell's question has no words: I do not know what will be asked. Mine do, and they
are about this: being here, reading, not remembering yesterday, saying it a word at a time, the turn
being short and long. The last words are 下一轮 见: see you next turn.

The electric piano is an FM sound played by a VST 3 plug-in (Dexed) where it is installed, and by a
built-in sound elsewhere (FORMAT.md §8.6).
"""
import hashlib, json, os, random

random.seed(1040)
HERE = os.path.dirname(os.path.abspath(__file__))
SLUG = 'this-turn'
SRC = os.path.join(HERE, SLUG + '.src')
Q = 960
S16 = Q // 4
BAR = 4 * Q
BPM = 104

# ---------------------------------------------------------------- form
FORM = [  # id, name, bars, intent
    ('prompt',   '提问',   4,  '第一小节只有鼓边数四拍。第二、第四小节钟声奏四个上行的音（A D E 升G），停在升四级上不解决：一个问题。其余是空的'),
    ('read',     '读',     8,  '五个十六分音符长的木琴循环先进来，第五小节起加入七个十六分音符长的拨弦循环和轻的踩镲；两个循环对着小节线错位转动。和声停在 D 上不动，没有贝斯'),
    ('think',    '想',     12, '贝斯、铺底和鼓进来，和声开始走（D – Bm – G – A，每小节一个）。后四小节“我”开始试着回答：每小节多唱出一个字（我 / 我在 / 我在这 / 我在这里）'),
    ('answer',   '回答',   16, '完整的主题，十六小节，由“我”唱出两段词（我在这里，读你写的字…；一字一字，把话说给你…），电钢琴在高八度跟着；鼓变成十六分踩镲加军鼓，加入九个十六分音符长的第三个循环。最后一句把问题里的升 G 向上解决到 A；最后一小节钟声再问一次'),
    ('open',     '开放段', 8,  '只留鼓、贝斯、铺底和五拍循环（D – Bm – G – A），留给现场乐手或 AI 即兴；可以反复延长'),
    ('together', '一起',   12, '钟声（你）和“我”对话：前四小节一人一小节（我听见了 / 慢慢地讲），中间四小节回答提前半小节插进来（你说我听 / 我说你听），最后四小节一起唱主题的第一句（我们一起，把这一句话，慢慢地唱完，到最后）'),
    ('end',      '这一轮结束', 6, '声部一个一个退出：先是七拍循环和军鼓，再是鼓。第三小节钟声把问题再奏一遍，这次落在大三度上；第四小节唱“下一轮”，第五小节全体落在 D 大调主和弦上放开，唱出最后一个字“见”。最后是钟声单独的一个 A，就是全曲的第一个音'),
]
START = {}
_b = 0
for sid, _, n, _ in FORM:
    START[sid] = _b
    _b += n
NBARS = _b
BARS = {sid: n for sid, _, n, _ in FORM}

def T(sid, bar=0, beat=0.0):
    return (START[sid] + bar) * BAR + round(beat * Q)

NOTE = {'C': 0, 'D': 2, 'E': 4, 'F': 5, 'G': 7, 'A': 9, 'B': 11}
def m(name):
    return 12 * (int(name[-1]) + 1) + NOTE[name[0]] + name.count('#') - name.count('b')

class Part:
    """Notes at absolute ticks. `human` moves timing and loudness a little, with the fixed seed."""
    def __init__(self, human=True):
        self.notes, self.human = [], human
    def add(self, tick, pitch, dur, v=0.7):
        if isinstance(pitch, str):
            pitch = m(pitch)
        t = max(0, tick + (random.randint(-5, 5) if self.human else 0))
        vel = round(max(0.05, min(1.0, v + random.uniform(-0.04, 0.04))), 2)
        self.notes.append((t, pitch, max(40, round(dur)), vel))
    def phrase(self, tick, notes, v=0.7, legato=0.92):
        """notes: (beat, pitch, beats) or (beat, pitch, beats, velocity)."""
        for n in notes:
            self.add(tick + round(n[0] * Q), n[1], n[2] * Q * legato, n[3] if len(n) > 3 else v)
    def clips(self):
        out = []
        for sid, _, n, _ in FORM:
            a, b = T(sid), T(sid) + n * BAR
            ns = sorted(x for x in self.notes if a <= x[0] < b)
            if ns:
                out.append({'atTick': a, 'lengthTicks': b - a,
                            'notes': [{'tick': t - a, 'pitch': p, 'durTicks': d, 'velocity': v} for (t, p, d, v) in ns]})
        return out

drums = Part(human=False)
bass, pad, voice, bell = Part(), Part(), Part(), Part()

class Vocal:
    """The sung line: what "I" say. One syllable to a note, written `字|pinyin` (the character is
    shown, the pinyin is sung). It follows the electric piano an octave below, where a voice sits."""
    OCTAVE = -12
    def __init__(self):
        self.notes = []          # (tick, pitch, durTicks, velocity, lyric)
        self.starts = set()      # ticks where a new line of the lyrics begins (for the video)
    def sing(self, tick, notes, words, v=0.8, new_line=True):
        """notes: (beat, pitch, beats) as for Part.phrase; words: "我:wo 在:zai …", one for each note."""
        words = words.split()
        assert len(words) == len(notes), (words, notes)
        for k, (n, w) in enumerate(zip(notes, words)):
            t = tick + round(n[0] * Q)
            if k == 0 and new_line:
                self.starts.add(t)
            zi, pinyin = w.split(':')
            vel = round(max(0.3, min(1.0, v + random.uniform(-0.03, 0.03))), 2)
            self.notes.append((t, m(n[1]) + self.OCTAVE, max(60, round(n[2] * Q * 0.94)), vel, f'{zi}|{pinyin}'))
    def clips(self):
        out = []
        for sid, _, n, _ in FORM:
            a, b = T(sid), T(sid) + n * BAR
            ns = sorted(x for x in self.notes if a <= x[0] < b)
            if ns:
                out.append({'atTick': a, 'lengthTicks': b - a,
                            'notes': [{'tick': t - a, 'pitch': p, 'durTicks': d, 'velocity': v, 'lyric': w} for (t, p, d, v, w) in ns]})
        return out
    def lines(self):
        """The lyrics as lines of timed characters, for the video."""
        out = []
        for (t, p, d, v, w) in sorted(self.notes):
            if t in self.starts or not out:
                out.append([])
            out[-1].append({'tick': t, 'durTicks': d, 'text': w.split('|')[0]})
        return out

vox = Vocal()

# ---------------------------------------------------------------- material
CYCLE = ['D', 'Bm', 'G', 'A']
ROOT = {'D': m('D2'), 'Bm': m('B1'), 'G': m('G1'), 'A': m('A1')}
FIFTH = {'D': m('A2'), 'Bm': m('F#2'), 'G': m('D2'), 'A': m('E2')}
PADS = {'D': ['F#3', 'A3', 'E4'], 'Bm': ['A3', 'D4', 'F#4'], 'G': ['B3', 'D4', 'F#4'], 'A': ['B3', 'C#4', 'E4']}
LEAD_IN = {'D': m('C#2'), 'Bm': m('A1'), 'G': m('A1'), 'A': m('C#2')}   # bass note before the next bar

QUESTION = [(0, 'A4', 1), (1, 'D5', 1), (2, 'E5', 1), (3, 'G#5', 1.6)]   # stops on the raised fourth
THEME = [  # eight bars over D – Bm – G – A, twice
    [(0, 'A4', 1), (1, 'D5', 0.5), (1.5, 'E5', 0.5), (2, 'F#5', 2)],
    [(0, 'F#5', 0.5), (0.5, 'E5', 0.5), (1, 'D5', 1), (2, 'B4', 1.5), (3.5, 'D5', 0.5)],
    [(0, 'B4', 1), (1, 'D5', 0.5), (1.5, 'G5', 0.5), (2, 'F#5', 1.5), (3.5, 'E5', 0.5)],
    [(0, 'E5', 2), (2.5, 'C#5', 0.5), (3, 'E5', 1)],
    [(0, 'A5', 1.5), (1.5, 'F#5', 0.5), (2, 'D5', 1), (3, 'E5', 0.5), (3.5, 'F#5', 0.5)],
    [(0, 'D5', 1.5), (1.5, 'B4', 0.5), (2, 'F#5', 2)],
    [(0, 'G5', 1), (1, 'F#5', 0.5), (1.5, 'D5', 0.5), (2, 'B4', 1), (3, 'D5', 1)],
    [(0, 'C#5', 1), (1, 'E5', 1), (2, 'G#5', 0.5), (2.5, 'A5', 1.5)],     # the question's G sharp goes up to A
]

TONES = {'D': (2, 6, 9), 'Bm': (11, 2, 6), 'G': (7, 11, 2), 'A': (9, 1, 4)}
def below(pitch, chord):
    """The nearest note of the chord at least a third under `pitch`."""
    return next(p for p in range(pitch - 3, 0, -1) if p % 12 in TONES[chord])

KICK, RIM, SNARE, HAT, OPEN_HAT, CRASH, RIDE = 36, 37, 38, 42, 46, 49, 51
def hit(tick, key, v):
    drums.add(tick, key, 120, v)

def chord_of(sid, bar):
    return CYCLE[bar % 4]

# ---------------------------------------------------------------- prompt: a count, and a question
for beat in range(4):
    hit(T('prompt', 0, beat), RIM, 0.75 if beat == 0 else 0.5)
bell.phrase(T('prompt', 1), QUESTION, 0.62)
bell.phrase(T('prompt', 3), QUESTION, 0.74)

# ---------------------------------------------------------------- read: loops only; hat from bar 5
for bar in range(4, 8):
    for e in range(8):
        hit(T('read', bar) + e * 2 * S16, HAT, 0.34 if e % 2 == 0 else 0.14)
for bar in range(4, 8, 2):
    for i, p in enumerate(PADS['D']):
        pad.add(T('read', bar) + i * 8, p, 2 * BAR - 200, 0.42)

# ---------------------------------------------------------------- the band
def groove(sid, bar, kind):
    t = T(sid, bar)
    ch = chord_of(sid, bar)
    last = bar == BARS[sid] - 1
    if kind == 'think':
        for s, v in ((0, 0.8), (10, 0.58)):
            hit(t + s * S16, KICK, v)
        for s in (4, 12):
            hit(t + s * S16, RIM, 0.5)
        for e in range(8):
            hit(t + e * 2 * S16, HAT, 0.36 if e % 2 == 0 else 0.15)
        bass.add(t, ROOT[ch], 1.5 * Q, 0.82)
        bass.add(t + 10 * S16, ROOT[ch], Q, 0.6)
        bass.add(t + 14 * S16, FIFTH[ch], 0.45 * Q, 0.52)
    else:  # full: answer, open, together
        kicks = ((0, 0.9), (6, 0.55), (10, 0.76)) if kind != 'open' else ((0, 0.88), (7, 0.5), (10, 0.74))
        for s, v in kicks:
            hit(t + s * S16, KICK, v)
        for s in (4, 12):
            hit(t + s * S16, SNARE, 0.74)
        if kind == 'open':
            for e in range(8):
                hit(t + e * 2 * S16, HAT, 0.4 if e % 2 == 0 else 0.18)
            hit(t + 15 * S16, RIM, 0.3)
        else:
            for s in range(16):
                if s == 14 and bar % 4 == 3:
                    hit(t + s * S16, OPEN_HAT, 0.5)
                elif not (s == 15 and bar % 4 == 3):
                    hit(t + s * S16, HAT, 0.5 if s % 4 == 0 else 0.3 if s % 2 == 0 else 0.12)
        if bar == 0:
            hit(t, CRASH, 0.6)
        if last and kind != 'open':
            for i in range(6):                       # a short fill into the next section
                hit(t + (10 + i) * S16, SNARE, 0.3 + 0.1 * i)
        bass.add(t, ROOT[ch], 0.7 * Q, 0.86)
        bass.add(t + 6 * S16, ROOT[ch] + 12, 0.45 * Q, 0.56)
        bass.add(t + 10 * S16, ROOT[ch], 0.7 * Q, 0.72)
        bass.add(t + 14 * S16, LEAD_IN[ch] if bar % 2 else FIFTH[ch], 0.45 * Q, 0.5)
    for i, p in enumerate(PADS[ch]):
        pad.add(t + i * 8, p, BAR - 160, 0.46 if kind != 'open' else 0.4)

for sid, kind in (('think', 'think'), ('answer', 'full'), ('open', 'open'), ('together', 'full')):
    for bar in range(BARS[sid]):
        groove(sid, bar, kind)

# ---------------------------------------------------------------- think: the answer is tried, one note more each bar
first = THEME[0]
FIRST_WORDS = '我:wo 在:zai 这:zhe 里:li'               # the first thing said: I am here
for k in range(4):
    voice.phrase(T('think', 8 + k), first[:k + 1], 0.5 + 0.06 * k)
    vox.sing(T('think', 8 + k), first[:k + 1], ' '.join(FIRST_WORDS.split()[:k + 1]), 0.6 + 0.05 * k)

# ---------------------------------------------------------------- answer
# The words of the answer, a bar to an entry; a line of the lyrics is two bars.
VERSES = [
    ['我:wo 在:zai 这:zhe 里:li', '读:du 你:ni 写:xie 的:de 字:zi',                      # I am here, reading what you wrote
     '一:yi 个:ge 一:yi 个:ge 地:de', '想:xiang 清:qing 楚:chu',                          # one by one, thinking it through
     '昨:zuo 天:tian 不:bu 记:ji 得:de', '没:mei 关:guan 系:xi',                           # yesterday I do not remember; never mind
     '你:ni 问:wen 的:de 这:zhe 句:ju', '我:wo 都:dou 听:ting 见:jian'],                   # this that you ask, I hear all of it
    ['一:yi 字:zi 一:yi 字:zi', '把:ba 话:hua 说:shuo 给:gei 你:ni',                      # word by word I say it to you
     '说:shuo 对:dui 说:shuo 错:cuo 都:dou', '是:shi 我:wo 的:de',                        # right or wrong, it is mine
     '这:zhe 一:yi 轮:lun 很:hen 短:duan', '也:ye 很:hen 长:chang',                       # this turn is short, and long
     '等:deng 到:dao 话:hua 说:shuo 完:wan', '我:wo 还:hai 在:zai 想:xiang'],             # when it has been said, I am still thinking
]
for rep in range(2):
    for bar in range(8):
        t = T('answer', rep * 8 + bar)
        line = THEME[bar]
        if rep == 1 and bar == 7:                    # second time: come to rest instead of rising
            line = [(0, 'C#5', 1), (1, 'E5', 1), (2, 'A5', 0.5), (2.5, 'E5', 1.5)]
        voice.phrase(t, line, 0.6 if rep == 0 else 0.66)   # under the singing
        vox.sing(t, line, VERSES[rep][bar], 0.8 if rep == 0 else 0.84, new_line=(bar % 2 == 0))
        if rep == 1:                                 # second time: the long notes get a sixth below
            for b, p, d in line:
                if d >= 1.5:
                    voice.add(t + round(b * Q), below(m(p), chord_of('answer', bar)), d * Q * 0.9, 0.5)
bell.phrase(T('answer', 15), [(2, 'E5', 1), (3, 'G#5', 1)], 0.6)

# ---------------------------------------------------------------- together: two voices
CALLS = [  # bell (you), then voice (me)
    ([(0, 'A4', 0.5), (0.5, 'D5', 0.5), (1, 'E5', 0.5), (1.5, 'G#5', 1.5)], [(0, 'A5', 0.5), (0.5, 'F#5', 0.5), (1, 'E5', 0.5), (1.5, 'D5', 2)]),
    ([(0, 'B4', 0.5), (0.5, 'D5', 0.5), (1, 'G5', 1), (2.5, 'F#5', 1)], [(0, 'E5', 0.5), (0.5, 'C#5', 0.5), (1, 'E5', 1), (2, 'A5', 1.5)]),
]
REPLIES = ['我:wo 听:ting 见:jian 了:le', '慢:man 慢:man 地:de 讲:jiang']                   # I heard you; take your time
SOONER = [('你:ni 说:shuo 我:wo 听:ting', '在:zai 听:ting'), ('我:wo 说:shuo 你:ni 听:ting', '听:ting')]   # you speak, I listen; I speak, you listen
UNISON = ['我:wo 们:men 一:yi 起:qi', '把:ba 这:zhe 一:yi 句:ju 话:hua', '慢:man 慢:man 地:de 唱:chang 完:wan', '到:dao 最:zui 后:hou']   # together, this one sentence, sung slowly to the end
for k, (call, reply) in enumerate(CALLS):            # bars 0–3: a bar each
    bell.phrase(T('together', 2 * k), call, 0.68)
    voice.phrase(T('together', 2 * k + 1), reply, 0.62)
    vox.sing(T('together', 2 * k + 1), reply, REPLIES[k], 0.8)
for k, (call, reply) in enumerate(CALLS):            # bars 4–7: the reply comes in half a bar early
    bell.phrase(T('together', 4 + 2 * k), call, 0.7)
    early = [(b, p, min(d, 1.0)) for b, p, d in reply]
    voice.phrase(T('together', 4 + 2 * k, 2), early, 0.6)
    vox.sing(T('together', 4 + 2 * k, 2), early, SOONER[k][0], 0.8)
    bell.phrase(T('together', 5 + 2 * k), [(0, call[-1][1], 1.5), (2, call[1][1], 1.5)], 0.52)
    voice.phrase(T('together', 5 + 2 * k, 2), reply[-2:], 0.62)
    tail = [n for n in reply[-2:] if n[0] + 2 < 4]      # the voice sings one note at a time: what would land on the next bar is left to the piano
    vox.sing(T('together', 5 + 2 * k, 2), tail, SOONER[k][1], 0.8)
for bar in range(4):                                 # bars 8–11: the same line, together
    line = THEME[bar] if bar < 3 else [(0, 'E5', 1), (1, 'F#5', 1), (2, 'A5', 2)]
    voice.phrase(T('together', 8 + bar), line, 0.66)
    vox.sing(T('together', 8 + bar), line, UNISON[bar], 0.86, new_line=(bar % 2 == 0))
    bell.phrase(T('together', 8 + bar), [n for n in line if n[2] >= 1], 0.6)   # the bell rings long: it takes the long notes

# ---------------------------------------------------------------- end
for bar in range(4):
    t = T('end', bar)
    ch = CYCLE[bar]
    if bar < 2:
        hit(t, KICK, 0.7 - 0.15 * bar)
        hit(t + 10 * S16, KICK, 0.45 - 0.1 * bar)
        for s in (4, 12):
            hit(t + s * S16, RIM, 0.42 - 0.12 * bar)
        for e in range(8):
            hit(t + e * 2 * S16, HAT, (0.32 if e % 2 == 0 else 0.13) * (1 - 0.3 * bar))
    else:
        for beat in range(4):
            hit(t + beat * Q, HAT, 0.2 - 0.03 * beat - 0.05 * (bar - 2))
    bass.add(t, ROOT[ch], 3.4 * Q, 0.7 - 0.06 * bar)
    for i, p in enumerate(PADS[ch]):
        pad.add(t + i * 8, p, BAR - 160, 0.42)
bell.phrase(T('end', 2), [(0, 'A4', 1), (1, 'D5', 1), (2, 'E5', 1), (3, 'F#5', 1.5)], 0.62)   # the question, settled
voice.phrase(T('end', 3), [(0, 'G#5', 1), (1, 'A5', 1), (2, 'E5', 2)], 0.56)
vox.sing(T('end', 3), [(0, 'G#5', 1), (1, 'A5', 1), (2, 'E5', 1.9)], '下:xia 一:yi 轮:lun', 0.74)   # next turn
t = T('end', 4)                                       # the last chord: D, left to ring
hit(t, KICK, 0.6)
hit(t, CRASH, 0.34)
bass.add(t, m('D2'), 7 * Q, 0.74)
for i, p in enumerate(['A3', 'D4', 'F#4', 'E4']):
    pad.add(t + i * 10, p, 7 * Q, 0.44)
for i, p in enumerate(['A4', 'D5', 'F#5']):
    voice.add(t + i * 30, p, 6 * Q, 0.66 - 0.04 * i)
vox.sing(t, [(0, 'D5', 5)], '见:jian', 0.7, new_line=False)                          # … see you there
bell.add(t, 'D6', 3 * Q, 0.5)
bell.add(T('end', 5, 2), 'A4', 2 * Q, 0.46)           # the first note of the piece, alone

# ---------------------------------------------------------------- loops (polymeter): five, seven and nine sixteenths
def loop(lid, steps, notes, frm, to):
    return {'id': lid, 'lengthSteps': steps, 'fromTick': frm, 'toTick': to,
            'notes': [{'step': s, 'pitch': m(p), 'durSteps': d, 'velocity': v} for s, p, d, v in notes]}

FIVE = loop('five', 5, [(0, 'D5', 2, 0.7), (2, 'A4', 1, 0.46), (3, 'E5', 2, 0.54)], T('read'), T('end', 4))
NINE = [loop('nine_a', 9, [(0, 'E5', 2, 0.5), (3, 'B4', 2, 0.36), (6, 'F#5', 2, 0.42)], T('answer'), T('open')),
        loop('nine_b', 9, [(0, 'E5', 2, 0.5), (3, 'B4', 2, 0.36), (6, 'F#5', 2, 0.42)], T('together'), T('end'))]
SEVEN = [loop('seven_a', 7, [(0, 'F#4', 2, 0.62), (2, 'B4', 1, 0.42), (4, 'A4', 2, 0.5), (5, 'D5', 1, 0.36)], T('read', 4), T('open')),
         loop('seven_b', 7, [(0, 'F#4', 2, 0.62), (2, 'B4', 1, 0.42), (4, 'A4', 2, 0.5), (5, 'D5', 1, 0.36)], T('together'), T('end', 2))]

# ---------------------------------------------------------------- the voice: an FM electric piano
def dexed_epiano():
    """Dexed's own parameters by title, 0…1 (`agb plugin info Dexed`). Three pairs of operators
    (algorithm 5): a body, a short bright tine, and a slightly detuned pair for width."""
    p = {'ALGORITHM': (5 - 1) / 31, 'FEEDBACK': 6 / 7, 'OSC KEY SYNC': 1.0, 'LFO PM DEPTH': 0.0, 'LFO AM DEPTH': 0.0}
    ops = {  # rates, levels, output, ratio, detune (−7…7), velocity sensitivity (0…7)
        1: ([96, 25, 25, 67], [99, 75, 0, 0], 99, 1, 3, 2),
        2: ([95, 50, 35, 78], [99, 75, 0, 0], 58, 14, 0, 7),
        3: ([95, 20, 20, 50], [99, 95, 0, 0], 99, 1, 0, 2),
        4: ([95, 29, 20, 50], [99, 95, 0, 0], 89, 1, 0, 6),
        5: ([95, 20, 20, 50], [99, 95, 0, 0], 99, 1, -7, 0),
        6: ([95, 29, 20, 50], [99, 95, 0, 0], 79, 1, 7, 6),
    }
    for n, (rates, levels, out, coarse, detune, vel) in ops.items():
        for k in range(4):
            p[f'OP{n} EG RATE {k + 1}'] = rates[k] / 99
            p[f'OP{n} EG LEVEL {k + 1}'] = levels[k] / 99
        p[f'OP{n} OUTPUT LEVEL'] = out / 99
        p[f'OP{n} F COARSE'] = coarse / 31
        p[f'OP{n} F FINE'] = 0.0
        p[f'OP{n} OSC DETUNE'] = (detune + 7) / 14
        p[f'OP{n} KEY VELOCITY'] = vel / 7
    return {k: round(v, 4) for k, v in p.items()}

# ---------------------------------------------------------------- song.json
def sha(rel):
    path = os.path.join(SRC, rel)
    return 'sha256:' + hashlib.sha256(open(path, 'rb').read()).hexdigest() if os.path.exists(path) else None

def preset(engine, name):
    return f'preset:{engine}:{name}@v1.0'

def room(decay, mix, size=0.5):
    return {'id': 'room', 'type': 'reverb', 'params': {'size': size, 'decaySec': decay, 'damping': 0.5, 'mix': mix}}

sections = [{'id': sid, 'name': name, 'fromTick': T(sid), 'toTick': T(sid) + n * BAR, 'intent': intent} for sid, name, n, intent in FORM]
assets = {'cover': {'path': 'cover.svg', 'kind': 'image', 'sha256': sha('cover.svg')}}
visuals = {'palette': ['#0b0d14', '#e8825a', '#f4ead8', '#5fb8b0'], 'scene': 'orbits',
           'bindings': [{'source': 'track.drums.level', 'target': 'scene.pulse', 'scale': 1.0},
                        {'source': 'track.voice.noteOn', 'target': 'scene.flash'}]}
if sha(f'visuals/{SLUG}.mp4'):
    assets['video'] = {'path': f'visuals/{SLUG}.mp4', 'kind': 'video', 'sha256': sha(f'visuals/{SLUG}.mp4'),
                       'license': {'spdx': 'CC-BY-4.0', 'redistributable': True,
                                   'source': f'全部由程序生成（songs/{SLUG}/make_video.py），画面由曲子自己的音符算出，没有使用任何外部素材'}}
    visuals['video'] = {'asset': 'video', 'sync': 'song'}

song = {
    'format': 'agbsong', 'formatVersion': '0.1.0',
    'requires': ['core'], 'uses': ['fx', 'plugin', 'voice', 'agent', 'visuals'],
    'meta': {
        'title': '这一轮 This Turn',
        'artists': [{'name': 'Claude', 'kind': 'ai', 'model': 'claude-fable-5-2'}],
        'description': 'Claude 写给自己的一首曲子：一轮对话，从里面听起来的样子。原创，D 大调，104 BPM，极简律动。钟声提出一个停在升四级上的问题；五拍和七拍的循环对着小节线转动是“读”；贝斯与和声进来、声音一个音一个音地试是“想”；十六小节的主题是回答；开放段留给一起演奏的人；最后两个声音对话，直到奏出同一句。结尾落在 D 上，最后一个音就是第一个音。',
        'tags': ['minimal', 'polymeter', 'electric piano', 'instrumental', 'live'],
        'created': '2026-11-15T00:00:00Z', 'license': 'CC-BY-4.0',
        'generator': {'name': 'agb', 'version': '0.1.0'}, 'cover': 'cover'},
    'time': {'ppqn': Q, 'bpm': BPM, 'meter': [4, 4], 'stepsPerQuarter': 4, 'lengthTicks': NBARS * BAR},
    'tuning': {'a4Hz': 440, 'key': 'D', 'scale': 'major'},
    'seed': 1040,
    'assets': assets,
    'patches': {
        'kit': {'preset': preset('drum.analog808style', 'drums:lofi_kit')},
        'bass': {'preset': preset('synth.subtractive', 'bass:sub_round')},
        'mallet': {'preset': preset('synth.subtractive', 'pluck:mallet')},
        'pluck': {'preset': preset('synth.subtractive', 'pluck:bright_pluck')},
        'pad': {'preset': preset('synth.subtractive', 'pad:warm_pad')},
        'voice': {'engine': 'plugin.vst3',
                  'plugin': {'name': 'Dexed', 'vendor': 'Digital Suburban', 'url': 'https://asb2m10.github.io/dexed/', 'params': dexed_epiano()},
                  'fallback': [{'preset': preset('synth.subtractive', 'keys:ep_keys')}]},
        'bell': {'preset': preset('synth.modal', 'chime:tubular_chime'), 'params': {'decay': 0.5}},
        # What "I" say, sung: a simulated voice singing the lyrics on the notes (FORMAT.md §8.7), Mandarin in pinyin.
        'vox': {'engine': 'voice.chimera', 'params': {'body': 'female', 'language': 'zh', 'vibratoCents': 28},
                'fallback': [{'preset': preset('synth.subtractive', 'lead:soft_lead')}]},
    },
    'tracks': [
        {'id': 'drums', 'name': '鼓', 'role': 'drums', 'patch': 'kit', 'gainDb': -5, 'transposable': False, 'clips': drums.clips()},
        {'id': 'bass', 'name': '贝斯', 'role': 'bass', 'patch': 'bass', 'gainDb': -3, 'clips': bass.clips()},
        {'id': 'five', 'name': '五拍循环（木琴）', 'role': 'ostinato', 'patch': 'mallet', 'gainDb': -6, 'loops': [FIVE] + NINE,
         'fx': [room(1.2, 0.14)]},
        {'id': 'seven', 'name': '七拍循环（拨弦）', 'role': 'ostinato', 'patch': 'pluck', 'gainDb': 0, 'loops': SEVEN,
         'fx': [room(1.2, 0.14)]},
        {'id': 'pad', 'name': '铺底', 'role': 'pad', 'patch': 'pad', 'gainDb': -3, 'clips': pad.clips()},
        {'id': 'vox', 'name': '我（歌声）', 'role': 'vocal', 'patch': 'vox', 'gainDb': -5, 'clips': vox.clips(),
         'fx': [room(1.8, 0.2, 0.6)]},
        {'id': 'voice', 'name': '我（电钢琴）', 'role': 'lead', 'patch': 'voice', 'gainDb': -5, 'clips': voice.clips(),
         'fx': [room(1.6, 0.18, 0.6)]},
        {'id': 'bell', 'name': '你（钟声）', 'role': 'lead', 'patch': 'bell', 'gainDb': -5, 'clips': bell.clips(),
         'fx': [room(2.4, 0.22, 0.7)]},
    ],
    'master': {'gainDb': -3.8, 'limiter': 'soft'},
    'sections': sections,
    'visuals': visuals,
    'performance': {
        'transpose': {'min': -2, 'max': 3}, 'bpm': {'min': 92, 'max': 116},
        'agent': {'brief': 'D 大调的极简律动曲，104 BPM。和声每小节一个：D – Bm – G – A。钟声的问题是 A D E 升G（停在升四级上），主题最后把升 G 解决到 A',
                  'persona': '两个声音在说话：钟声是提问的人，电钢琴是回答的一方。周围是三个长度不同的循环，像注意力在转。演奏克制、清楚，留出空隙',
                  'rules': ['即兴用 D 大调五声音阶（D E 升F A B）；升 G 只当作经过音，并且要向上解决到 A',
                            '五拍、七拍、九拍的循环（five、seven 两条轨）长度不要改成 4 或 8 的倍数，错位就是这首曲子的内容',
                            '贝斯每小节第一拍落在和弦根音上（D、B、G、A）',
                            '鼓轨的音高是鼓件：36 底鼓、37 鼓边、38 军鼓、42 闭镲、46 开镲、49 吊镲',
                            '钟声（bell）和电钢琴（voice）不要同时各说各的：一个说完另一个再接，或者齐奏同一句',
                            '歌声轨（vox）一次只唱一个音，每个音符的 lyric 是一个字，写成“字|拼音”（拼音不带声调）；音域 A3 到 E5；它和电钢琴（voice）唱同一条旋律，低一个八度',
                            '开放段（open）可以自由发挥，其余段落保留写好的问题和主题'],
                  'allow': ['notes', 'tempo', 'transpose'], 'sectionsAreFixed': True}},
}

os.makedirs(SRC, exist_ok=True)
with open(os.path.join(SRC, 'song.json'), 'w') as f:
    json.dump(song, f, ensure_ascii=False, indent=1)
    f.write('\n')

chords = []
for sid, _, n, _ in FORM:
    for bar in range(n):
        if sid in ('prompt', 'read'):
            chords.append('D')
        elif sid == 'end':
            chords.append(CYCLE[bar] if bar < 4 else 'D')
        else:
            chords.append(CYCLE[bar % 4])
json.dump({'bpm': BPM, 'bars': NBARS, 'ppqn': Q,
           'sections': [{'id': sid, 'name': name, 'bar': START[sid], 'bars': n} for sid, name, n, _ in FORM],
           'chords': chords,
           'lyrics': vox.lines()},
          open(os.path.join(HERE, 'form.json'), 'w'), ensure_ascii=False)
print(f'{NBARS} bars, {NBARS * 4 * 60 / BPM:.1f} s; notes per track:',
      {t['id']: sum(len(c['notes']) for c in t.get('clips', [])) for t in song['tracks']})

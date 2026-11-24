//! The "jam" page of the Android app: an instrument played with one finger on a pad, and a
//! four-track loop of what was played.
//!
//! Left to right on the pad is pitch, held to the notes of a key and a scale; bottom to top opens
//! a filter. A track has a program (one of the engine's presets), and a rhythm that turns a held
//! finger into notes in time (or, for a drum kit, into a beat). What is played can be recorded
//! into a loop of one to eight bars, track over track.
//!
//! It is made to be called from the audio output's own thread ([`Jam::render`]), which must never
//! wait: what the app asks for comes in through a queue that is emptied only when it is free, and
//! what the app shows is read from a few atomic values. The notes that are recorded are kept as
//! notes (not as sound), in beats, so the loop follows the tempo.

use agb_dsp::{EngineParams, ParamValue, Voice};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::Mutex;

pub const TRACKS: usize = 4;
const VOICES: usize = 8;
/// The sound is made this many frames at a time: notes start on these.
const STEP: usize = 64;

/// A sound to play: a preset of the engine, where on the keyboard the pad's left edge is, and
/// whether it is a drum kit (which plays beats, not pitches).
pub struct Program {
    pub name: &'static str,
    preset: &'static str,
    octave: i32,
    drums: bool,
    /// Made up for the presets' different loudness, so that the tracks sit together.
    gain: f32,
}

impl Program {
    pub fn is_drums(&self) -> bool {
        self.drums
    }
}

pub const PROGRAMS: &[Program] = &[
    Program { name: "锯齿主音", preset: "synth.subtractive:lead:saw_lead", octave: 4, drums: false, gain: 1.0 },
    Program { name: "方波主音", preset: "synth.subtractive:lead:square_lead", octave: 4, drums: false, gain: 1.0 },
    Program { name: "柔和主音", preset: "synth.subtractive:lead:soft_lead", octave: 4, drums: false, gain: 1.0 },
    Program { name: "梯形主音", preset: "synth.moogish:lead:ladder_lead", octave: 4, drums: false, gain: 1.0 },
    Program { name: "酸性", preset: "synth.moogish:bass:acid_ladder", octave: 2, drums: false, gain: 0.9 },
    Program { name: "梯形贝斯", preset: "synth.moogish:bass:ladder_bass", octave: 2, drums: false, gain: 0.8 },
    Program { name: "圆润低音", preset: "synth.subtractive:bass:sub_round", octave: 2, drums: false, gain: 0.8 },
    Program { name: "FM 贝斯", preset: "synth.fm:bass:solid_bass", octave: 2, drums: false, gain: 0.9 },
    Program { name: "Reese 贝斯", preset: "synth.subtractive:bass:reese_bass", octave: 2, drums: false, gain: 0.8 },
    Program { name: "FM 电钢琴", preset: "synth.fm:keys:tine_piano", octave: 3, drums: false, gain: 1.1 },
    Program { name: "玻璃钟", preset: "synth.fm:bell:glass_bell", octave: 4, drums: false, gain: 1.1 },
    Program { name: "木琴", preset: "synth.subtractive:pluck:mallet", octave: 3, drums: false, gain: 1.0 },
    Program { name: "拨弦", preset: "synth.subtractive:pluck:bright_pluck", octave: 3, drums: false, gain: 1.1 },
    Program { name: "风琴", preset: "synth.fm:keys:drawbar_organ", octave: 3, drums: false, gain: 1.2 },
    Program { name: "暖垫", preset: "synth.subtractive:pad:warm_pad", octave: 3, drums: false, gain: 1.4 },
    Program { name: "玻璃垫", preset: "synth.subtractive:pad:glass_pad", octave: 3, drums: false, gain: 1.3 },
    Program { name: "管钟", preset: "synth.modal:chime:tubular_chime", octave: 4, drums: false, gain: 1.0 },
    Program { name: "经典鼓", preset: "drum.analog808style:drums:classic_kit", octave: 0, drums: true, gain: 1.0 },
    Program { name: "重鼓", preset: "drum.analog808style:drums:punchy_kit", octave: 0, drums: true, gain: 1.0 },
    Program { name: "Trap 鼓", preset: "drum.analog808style:drums:trap_kit", octave: 0, drums: true, gain: 1.0 },
    Program { name: "Lo-fi 鼓", preset: "drum.analog808style:drums:lofi_kit", octave: 0, drums: true, gain: 1.0 },
];

/// What a held finger becomes: `per_beat` steps a beat, the steps of `mask` that sound (it goes
/// round), each for `gate` of a step, and for an arpeggio the scale steps above the note touched.
/// `per_beat` 0: the note sounds for as long as the finger is down.
pub struct Rhythm {
    pub name: &'static str,
    per_beat: u32,
    mask: &'static [u8],
    gate: f64,
    climb: &'static [i32],
}

pub const RHYTHMS: &[Rhythm] = &[
    Rhythm { name: "连奏", per_beat: 0, mask: &[1], gate: 1.0, climb: &[0] },
    Rhythm { name: "四分", per_beat: 1, mask: &[1], gate: 0.8, climb: &[0] },
    Rhythm { name: "八分", per_beat: 2, mask: &[1], gate: 0.7, climb: &[0] },
    Rhythm { name: "十六分", per_beat: 4, mask: &[1], gate: 0.6, climb: &[0] },
    Rhythm { name: "三连音", per_beat: 3, mask: &[1], gate: 0.7, climb: &[0] },
    Rhythm { name: "附点", per_beat: 4, mask: &[1, 0, 0, 1, 0, 0, 1, 0], gate: 1.6, climb: &[0] },
    Rhythm { name: "切分", per_beat: 4, mask: &[1, 0, 1, 1, 0, 1, 0, 1], gate: 0.7, climb: &[0] },
    Rhythm { name: "琶音上行", per_beat: 4, mask: &[1], gate: 0.7, climb: &[0, 2, 4, 7] },
    Rhythm { name: "琶音往返", per_beat: 4, mask: &[1], gate: 0.7, climb: &[0, 2, 4, 7, 4, 2] },
];

pub const SCALES: &[(&str, &[i32])] = &[
    ("大调", &[0, 2, 4, 5, 7, 9, 11]),
    ("小调", &[0, 2, 3, 5, 7, 8, 10]),
    ("大调五声", &[0, 2, 4, 7, 9]),
    ("小调五声", &[0, 3, 5, 7, 10]),
    ("布鲁斯", &[0, 3, 5, 6, 7, 10]),
    ("多利亚", &[0, 2, 3, 5, 7, 9, 10]),
    ("弗里几亚", &[0, 1, 3, 5, 7, 8, 10]),
    ("混合利底亚", &[0, 2, 4, 5, 7, 9, 10]),
    ("和声小调", &[0, 2, 3, 5, 7, 8, 11]),
    ("半音", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
];

/// Beats a drum kit plays while the pad is held, from the left of the pad to the right: sparse to
/// busy. Sixteen steps a bar for the kick, the snare, the closed hat and one more sound.
const BEATS: &[[u16; 4]] = &[
    [0b0000_0001_0000_0001, 0, 0, 0],
    [0b0001_0001_0001_0001, 0, 0, 0],
    [0b0000_0001_0000_0001, 0b0001_0000_0001_0000, 0b0100_0100_0100_0100, 0],
    [0b0001_0001_0001_0001, 0b0001_0000_0001_0000, 0b0100_0100_0100_0100, 0],
    [0b0000_0101_0000_0001, 0b0001_0000_0001_0000, 0b0101_0101_0101_0101, 0],
    [0b0100_0001_0010_0001, 0b0001_0000_0001_0000, 0b0101_0101_0101_0101, 0b1000_0000_0000_0000],
    [0b0010_0101_0100_1001, 0b1001_0000_0001_0000, 0b1111_1111_1111_1111, 0b0000_0100_0000_0000],
    [0b0101_0101_0101_0101, 0b1011_0000_1001_0010, 0b1111_1111_1111_1111, 0b0100_0000_0100_0000],
];
/// The four sounds of [`BEATS`] as notes of the kit: kick, snare, closed hat, clap; and the open hat.
const BEAT_NOTES: [u8; 4] = [36, 38, 42, 39];
const OPEN_HAT: u8 = 46;

#[derive(Clone, Copy)]
enum What {
    On(u8, f32),
    Off(u8),
    /// The pad's height, 0…1.
    Filter(f32),
}

/// Something that was played, at a beat of the loop. `pass`: the turn of the loop it was recorded in.
#[derive(Clone, Copy)]
struct Event {
    beat: f64,
    what: What,
    pass: u64,
}

struct Sounding {
    voice: Box<dyn Voice>,
    pitch: u8,
    held: bool,
}

struct Track {
    program: usize,
    params: Option<EngineParams>,
    voices: Vec<Sounding>,
    events: Vec<Event>,
    volume: f32,
    mute: bool,
    // the filter: two poles, its opening following the pad
    aim: f32,
    open: f32,
    low: f32,
    band: f32,
    seed: u64,
}

impl Track {
    fn new(program: usize) -> Track {
        let mut t = Track { program: usize::MAX, params: None, voices: Vec::with_capacity(VOICES), events: vec![], volume: 0.8, mute: false, aim: 1.0, open: 1.0, low: 0.0, band: 0.0, seed: 1 };
        t.set_program(program);
        t
    }

    fn set_program(&mut self, program: usize) {
        let program = program.min(PROGRAMS.len() - 1);
        if program == self.program {
            return;
        }
        self.program = program;
        let p = &PROGRAMS[program];
        let id = format!("preset:{}@v1.0", p.preset);
        self.params = agb_presets::get(&id).and_then(|preset| EngineParams::resolve(&preset.engine, &[&preset.parameters, &BTreeMap::<String, ParamValue>::new()]).ok());
        for v in self.voices.iter_mut() {
            v.held = false;
            v.voice.note_off(v.pitch, 0);
        }
    }

    fn on(&mut self, pitch: u8, velocity: f32, sample_rate: u32) {
        let Some(params) = &self.params else { return };
        if self.voices.len() >= VOICES {
            // the oldest goes: a new note is never refused
            self.voices.remove(0);
        }
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let mut voice = params.make_voice(sample_rate, self.seed);
        voice.set_velocity(velocity);
        voice.note_on(pitch, velocity * PROGRAMS[self.program].gain, 0);
        self.voices.push(Sounding { voice, pitch, held: true });
    }

    fn off(&mut self, pitch: u8) {
        for v in self.voices.iter_mut().filter(|v| v.held && v.pitch == pitch) {
            v.held = false;
            v.voice.note_off(pitch, 0);
        }
    }

    fn all_off(&mut self) {
        for v in self.voices.iter_mut().filter(|v| v.held) {
            v.held = false;
            v.voice.note_off(v.pitch, 0);
        }
    }

    /// Add the track's sound to `mix`. Returns its peak.
    fn render(&mut self, mix: &mut [f32], scratch: &mut [f32], sample_rate: f32) -> f32 {
        let buf = &mut scratch[..mix.len()];
        buf.fill(0.0);
        for v in self.voices.iter_mut() {
            v.voice.render(buf);
        }
        self.voices.retain(|v| v.voice.is_active());
        // The pad's height opens the filter: from 120 Hz at the bottom to wide open at the top.
        let mut peak = 0f32;
        let gain = if self.mute { 0.0 } else { self.volume };
        for (x, out) in buf.iter().zip(mix.iter_mut()) {
            self.open += (self.aim - self.open) * 0.002;
            let y = if self.open > 0.985 {
                *x
            } else {
                let hz = 120.0 * (150.0f32).powf(self.open);
                let f = (2.0 * (std::f32::consts::PI * hz / sample_rate).sin()).min(1.2);
                // a state-variable low-pass with a little resonance, so that the sweep is heard
                self.low += f * self.band;
                let high = *x - self.low - 0.55 * self.band;
                self.band += f * high;
                self.low
            };
            let y = if y.is_finite() { y } else { 0.0 };
            if !y.is_finite() || self.low.abs() > 8.0 {
                self.low = 0.0;
                self.band = 0.0;
            }
            let y = y * gain;
            peak = peak.max(y.abs());
            *out += y;
        }
        peak
    }
}

/// What the app asks for. Applied at the start of the next block of sound.
pub enum Ask {
    Touch { x: f32, y: f32, down: bool },
    Track(usize),
    Program(usize, usize),
    Rhythm(usize),
    Key(i32),
    Scale(usize),
    Bpm(f32),
    Bars(u32),
    Record(bool),
    Play(bool),
    Click(bool),
    Clear(usize),
    Mute(usize, bool),
    Volume(usize, f32),
}

/// What the app shows, read without waiting.
#[derive(Default)]
pub struct Shown {
    /// Where the loop is, 0…1 (as the bits of an f32).
    pub phase: AtomicU32,
    /// The note that is sounding under the finger, or −1.
    pub note: AtomicI32,
    pub level: [AtomicU32; TRACKS],
    pub filled: [AtomicBool; TRACKS],
    pub recording: AtomicBool,
    pub playing: AtomicBool,
}

pub struct Jam {
    sample_rate: u32,
    tracks: Vec<Track>,
    track: usize,
    rhythm: usize,
    key: i32,
    scale: usize,
    bpm: f64,
    bars: u32,
    /// Beats since the loop was started; the loop is `bars × 4` of them long.
    beat: f64,
    recording: bool,
    playing: bool,
    click: bool,
    click_phase: f32,
    click_level: f32,
    click_hz: f32,
    // the finger
    down: bool,
    x: f32,
    y: f32,
    /// The note that sounds for as long as the finger is down (no rhythm).
    held: Option<u8>,
    /// Notes started by the rhythm, with the beat at which each ends.
    gated: Vec<(u8, f64)>,
    last_step: i64,
    last_filter: f32,
    scratch: Vec<f32>,
    mix: Vec<f32>,
}

// SAFETY: a `Jam` is only ever used by one thread at a time (it lives in a mutex), and its voices,
// which the engine does not mark as sendable, are plain numbers with no ties to the thread that
// made them.
unsafe impl Send for Jam {}

impl Jam {
    pub fn new(sample_rate: u32) -> Jam {
        let tracks = vec![Track::new(0), Track::new(5), Track::new(9), Track::new(17)];
        Jam {
            sample_rate,
            tracks,
            track: 0,
            rhythm: 0,
            key: 0,
            scale: 3,
            bpm: 110.0,
            bars: 4,
            beat: 0.0,
            recording: false,
            playing: true,
            click: false,
            click_phase: 0.0,
            click_level: 0.0,
            click_hz: 0.0,
            down: false,
            x: 0.5,
            y: 0.5,
            held: None,
            gated: vec![],
            last_step: -1,
            last_filter: -1.0,
            scratch: vec![0.0; STEP],
            mix: vec![0.0; STEP],
        }
    }

    fn loop_beats(&self) -> f64 {
        self.bars as f64 * 4.0
    }

    /// The note under the finger: `x` across two octaves of the scale from the program's lowest
    /// note, `climb` scale steps further up.
    fn pitch_at(&self, x: f32, climb: i32) -> u8 {
        let steps = SCALES[self.scale].1;
        let n = steps.len() as i32;
        let across = 2 * n;
        let k = (x.clamp(0.0, 1.0) * across as f32).round() as i32 + climb;
        let octave = PROGRAMS[self.tracks[self.track].program].octave;
        let note = 12 * (octave + 1) + self.key + 12 * k.div_euclid(n) + steps[k.rem_euclid(n) as usize];
        note.clamp(0, 127) as u8
    }

    /// How many notes the pad has from left to right (for whoever draws it).
    pub fn columns(scale: usize) -> usize {
        2 * SCALES[scale.min(SCALES.len() - 1)].1.len() + 1
    }

    fn pass(&self) -> u64 {
        (self.beat / self.loop_beats()) as u64
    }

    fn at(&self) -> f64 {
        self.beat % self.loop_beats()
    }

    /// Play on the track that is being played, and write it down if the loop is recording.
    fn play(&mut self, what: What) {
        let (sr, track, beat, pass) = (self.sample_rate, self.track, self.at(), self.pass());
        let t = &mut self.tracks[track];
        match what {
            What::On(p, v) => t.on(p, v, sr),
            What::Off(p) => t.off(p),
            What::Filter(y) => t.aim = y,
        }
        if self.recording {
            t.events.push(Event { beat, what, pass });
        }
    }

    fn take(&mut self, ask: Ask) {
        match ask {
            Ask::Touch { x, y, down } => {
                self.x = x.clamp(0.0, 1.0);
                self.y = y.clamp(0.0, 1.0);
                if down && !self.down {
                    self.last_step = -1; // the rhythm starts with the finger's next step
                }
                self.down = down;
            }
            Ask::Track(i) => {
                if i < TRACKS && i != self.track {
                    self.release();
                    self.track = i;
                }
            }
            Ask::Program(i, p) => {
                if i < TRACKS {
                    if i == self.track {
                        self.release();
                    }
                    self.tracks[i].set_program(p);
                }
            }
            Ask::Rhythm(r) => {
                self.release();
                self.rhythm = r.min(RHYTHMS.len() - 1);
            }
            Ask::Key(k) => self.key = k.rem_euclid(12),
            Ask::Scale(s) => self.scale = s.min(SCALES.len() - 1),
            Ask::Bpm(b) => self.bpm = (b as f64).clamp(40.0, 220.0),
            Ask::Bars(b) => {
                let b = b.clamp(1, 8);
                if b != self.bars {
                    self.release();
                    // what lies beyond a shorter loop is kept, and heard again in a longer one
                    self.bars = b;
                    self.beat = self.at();
                    for t in self.tracks.iter_mut() {
                        t.all_off();
                        for e in t.events.iter_mut() {
                            e.pass = 0;
                        }
                    }
                }
            }
            Ask::Record(on) => {
                self.recording = on;
                if on {
                    self.playing = true;
                }
            }
            Ask::Play(on) => {
                self.playing = on;
                if !on {
                    self.recording = false;
                    for (i, t) in self.tracks.iter_mut().enumerate() {
                        if i != self.track {
                            t.all_off();
                        }
                    }
                } else {
                    self.beat = 0.0;
                    self.last_step = -1;
                }
            }
            Ask::Click(on) => self.click = on,
            Ask::Clear(i) => {
                if i < TRACKS {
                    self.tracks[i].events.clear();
                    if i != self.track {
                        self.tracks[i].all_off();
                    }
                }
            }
            Ask::Mute(i, on) => {
                if i < TRACKS {
                    self.tracks[i].mute = on;
                }
            }
            Ask::Volume(i, v) => {
                if i < TRACKS {
                    self.tracks[i].volume = v.clamp(0.0, 1.2);
                }
            }
        }
    }

    /// The finger's notes end (it was lifted, or what it plays has changed).
    fn release(&mut self) {
        if let Some(p) = self.held.take() {
            self.play(What::Off(p));
        }
        for (p, _) in std::mem::take(&mut self.gated) {
            self.play(What::Off(p));
        }
    }

    /// What the finger does in the block that begins at `self.beat` and is `beats` long.
    fn finger(&mut self, beats: f64) {
        let drums = PROGRAMS[self.tracks[self.track].program].drums;
        // notes of the rhythm that have had their time
        let now = self.beat;
        let mut i = 0;
        while i < self.gated.len() {
            if self.gated[i].1 <= now {
                let (p, _) = self.gated.remove(i);
                self.play(What::Off(p));
            } else {
                i += 1;
            }
        }
        if !self.down {
            if let Some(p) = self.held.take() {
                self.play(What::Off(p));
            }
            return;
        }
        if (self.y - self.last_filter).abs() > 0.01 {
            self.last_filter = self.y;
            self.play(What::Filter(self.y));
        }
        let rhythm = &RHYTHMS[self.rhythm];
        if drums {
            // a beat: which one by where the finger is from left to right, how hard by its height
            let step = (self.beat * 4.0).floor() as i64;
            let ends = ((self.beat + beats) * 4.0).floor() as i64;
            if step != self.last_step || ends != step {
                let s = if step != self.last_step { step } else { ends };
                if s != self.last_step {
                    self.last_step = s;
                    let beat = &BEATS[((self.x * BEATS.len() as f32) as usize).min(BEATS.len() - 1)];
                    let bit = 1u16 << (s.rem_euclid(16) as u32);
                    for (lane, mask) in beat.iter().enumerate() {
                        if mask & bit == 0 {
                            continue;
                        }
                        // high on the pad the off-beat hats open
                        let note = if lane == 2 && self.y > 0.75 && s.rem_euclid(4) == 2 { OPEN_HAT } else { BEAT_NOTES[lane] };
                        let accent = if s.rem_euclid(4) == 0 { 1.0 } else { 0.72 };
                        self.play(What::On(note, (0.55 + 0.45 * self.y) * accent));
                        self.gated.push((note, self.beat + 0.2));
                    }
                }
            }
        } else if rhythm.per_beat == 0 {
            let p = self.pitch_at(self.x, 0);
            if self.held != Some(p) {
                if let Some(old) = self.held.take() {
                    self.play(What::Off(old));
                }
                self.play(What::On(p, 0.85));
                self.held = Some(p);
            }
        } else {
            let per = rhythm.per_beat as f64;
            let step = ((self.beat + beats) * per).floor() as i64;
            if step != self.last_step {
                self.last_step = step;
                if rhythm.mask[step.rem_euclid(rhythm.mask.len() as i64) as usize] != 0 {
                    let climb = rhythm.climb[step.rem_euclid(rhythm.climb.len() as i64) as usize];
                    let p = self.pitch_at(self.x, climb);
                    self.play(What::On(p, 0.85));
                    self.gated.push((p, self.beat + rhythm.gate / per));
                }
            }
        }
    }

    /// Play what was recorded in the block that begins at `self.beat` and is `beats` long.
    fn recorded(&mut self, beats: f64) {
        let length = self.loop_beats();
        let (from, pass, sr) = (self.at(), self.pass(), self.sample_rate);
        let to = from + beats;
        // where the loop turns, nothing that was held hangs over (before the first notes of the next turn start)
        if to >= length {
            for (i, t) in self.tracks.iter_mut().enumerate() {
                if !(i == self.track && self.down) {
                    t.all_off();
                }
            }
        }
        for (i, t) in self.tracks.iter_mut().enumerate() {
            let live = i == self.track && self.down;
            for k in 0..t.events.len() {
                let e = t.events[k];
                if e.beat >= length {
                    continue;
                }
                // in this block before the loop turns, or after it has (then it is the next turn's)
                let turn = if e.beat >= from && e.beat < to {
                    pass
                } else if to > length && e.beat < to - length {
                    pass + 1
                } else {
                    continue;
                };
                // not what is being recorded in this turn of the loop: that was just heard
                if self.recording && i == self.track && e.pass >= turn {
                    continue;
                }
                match e.what {
                    What::On(p, v) => t.on(p, v, sr),
                    What::Off(p) => t.off(p),
                    // the finger that is on the pad now has the filter
                    What::Filter(y) => {
                        if !live {
                            t.aim = y
                        }
                    }
                }
            }
        }
    }

    /// Make the next `out.len()` samples (one channel). `asks`: what the app has asked for since
    /// the last call. `shown`: what it reads back.
    pub fn render(&mut self, out: &mut [f32], asks: &mut Vec<Ask>, shown: &Shown) {
        for a in asks.drain(..) {
            self.take(a);
        }
        let sr = self.sample_rate as f32;
        let mut peaks = [0f32; TRACKS];
        for block in out.chunks_mut(STEP) {
            let beats = block.len() as f64 * self.bpm / (60.0 * self.sample_rate as f64);
            self.finger(beats);
            if self.playing {
                self.recorded(beats);
            }
            let mix = &mut self.mix[..block.len()];
            mix.fill(0.0);
            for (i, t) in self.tracks.iter_mut().enumerate() {
                peaks[i] = peaks[i].max(t.render(mix, &mut self.scratch, sr));
            }
            // the click: on every beat while recording or when asked for, higher on the first of a bar
            if self.click || self.recording {
                let before = self.beat.floor();
                if (self.beat + beats).floor() > before || self.beat == 0.0 {
                    let n = (self.beat + beats).floor() as i64;
                    self.click_level = 0.25;
                    self.click_hz = if n.rem_euclid(4) == 0 { 1760.0 } else { 880.0 };
                }
            }
            for (x, y) in mix.iter().zip(block.iter_mut()) {
                let mut v = *x;
                if self.click_level > 0.0005 {
                    self.click_phase += self.click_hz / sr;
                    v += (self.click_phase * std::f32::consts::TAU).sin() * self.click_level;
                    self.click_level *= 0.9985;
                }
                // never over full scale: gently above 0.7
                let a = v.abs();
                *y = if a <= 0.7 { v } else { v.signum() * (0.7 + 0.3 * ((a - 0.7) / (a - 0.7 + 0.3))) };
            }
            self.beat += beats;
        }
        shown.phase.store(((self.at() / self.loop_beats()) as f32).to_bits(), Ordering::Relaxed);
        let note = self.held.or(self.gated.last().map(|g| g.0)).filter(|_| self.down);
        shown.note.store(note.map_or(-1, |p| p as i32), Ordering::Relaxed);
        for (i, peak) in peaks.iter().enumerate() {
            let was = f32::from_bits(shown.level[i].load(Ordering::Relaxed));
            shown.level[i].store(peak.max(was * 0.8).to_bits(), Ordering::Relaxed);
            shown.filled[i].store(!self.tracks[i].events.is_empty(), Ordering::Relaxed);
        }
        shown.recording.store(self.recording, Ordering::Relaxed);
        shown.playing.store(self.playing, Ordering::Relaxed);
    }
}

/// The one instrument of the app, the queue of what it is asked, and what it shows.
pub struct Shared {
    pub jam: Mutex<Option<Jam>>,
    pub asks: Mutex<Vec<Ask>>,
    pub shown: Shown,
}

impl Shared {
    pub fn ask(&self, a: Ask) {
        self.asks.lock().unwrap_or_else(|e| e.into_inner()).push(a);
    }

    /// From the audio output's thread: never waits (if the app holds the queue this moment, what
    /// it asks is taken at the next call).
    pub fn render(&self, out: &mut [f32]) {
        let Ok(mut jam) = self.jam.try_lock() else {
            out.fill(0.0);
            return;
        };
        let Some(jam) = jam.as_mut() else {
            out.fill(0.0);
            return;
        };
        let mut none = Vec::new();
        match self.asks.try_lock() {
            Ok(mut asks) => jam.render(out, &mut asks, &self.shown),
            Err(_) => jam.render(out, &mut none, &self.shown),
        }
    }
}

pub static SHARED: Shared = Shared {
    jam: Mutex::new(None),
    asks: Mutex::new(Vec::new()),
    shown: Shown {
        phase: AtomicU32::new(0),
        note: AtomicI32::new(-1),
        level: [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)],
        filled: [AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false)],
        recording: AtomicBool::new(false),
        playing: AtomicBool::new(true),
    },
};

#[cfg(test)]
mod tests {
    use super::*;

    fn run(jam: &mut Jam, asks: Vec<Ask>, seconds: f32) -> Vec<f32> {
        let shown = Shown::default();
        let mut out = vec![0f32; (48000.0 * seconds) as usize];
        let mut asks = asks;
        jam.render(&mut out, &mut asks, &shown);
        out
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    #[test]
    fn every_program_has_its_preset() {
        for p in PROGRAMS {
            assert!(agb_presets::get(&format!("preset:{}@v1.0", p.preset)).is_some(), "{}", p.preset);
        }
    }

    #[test]
    fn the_pad_plays_notes_of_the_scale() {
        let mut jam = Jam::new(48000);
        jam.take(Ask::Scale(0)); // C major, from C4 on the first program
        jam.take(Ask::Key(0));
        let notes: Vec<u8> = (0..=14).map(|k| jam.pitch_at(k as f32 / 14.0, 0)).collect();
        assert_eq!(notes, [60, 62, 64, 65, 67, 69, 71, 72, 74, 76, 77, 79, 81, 83, 84]);
        jam.take(Ask::Key(2));
        assert_eq!(jam.pitch_at(0.0, 0), 62, "D");
        assert_eq!(Jam::columns(0), 15);
    }

    #[test]
    fn a_finger_sounds_and_stops() {
        let mut jam = Jam::new(48000);
        let silent = run(&mut jam, vec![], 0.2);
        assert!(rms(&silent) < 1e-6);
        let held = run(&mut jam, vec![Ask::Touch { x: 0.5, y: 1.0, down: true }], 0.5);
        assert!(rms(&held[4800..]) > 0.01, "{}", rms(&held));
        assert!(held.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        let after = run(&mut jam, vec![Ask::Touch { x: 0.5, y: 1.0, down: false }], 1.5);
        assert!(rms(&after[48000..]) < 1e-3, "released: {}", rms(&after[48000..]));
        // low on the pad the filter is closed: darker, so quieter for a bright sound
        let mut dark = Jam::new(48000);
        let low = run(&mut dark, vec![Ask::Touch { x: 0.5, y: 0.0, down: true }], 0.5);
        assert!(rms(&low[4800..]) < 0.7 * rms(&held[4800..]), "{} against {}", rms(&low[4800..]), rms(&held[4800..]));
    }

    #[test]
    fn what_is_recorded_comes_round_again() {
        let mut jam = Jam::new(48000);
        jam.take(Ask::Bars(1));
        jam.take(Ask::Bpm(120.0)); // a bar is two seconds
        jam.take(Ask::Click(false));
        jam.take(Ask::Play(true));
        // record a note in the first half second
        let mut first = run(&mut jam, vec![Ask::Record(true), Ask::Touch { x: 0.3, y: 1.0, down: true }], 0.5);
        first.extend(run(&mut jam, vec![Ask::Touch { x: 0.3, y: 1.0, down: false }], 1.5));
        // the loop turns: the note is heard again with no finger on the pad (and no click)
        let again = run(&mut jam, vec![Ask::Record(false)], 2.0);
        assert!(rms(&again[2400..21600]) > 0.01, "replayed: {}", rms(&again[2400..21600]));
        assert!(rms(&again[72000..]) < 2e-3, "and ends where it ended: {}", rms(&again[72000..]));
        let cleared = run(&mut jam, vec![Ask::Clear(0)], 2.0);
        assert!(rms(&cleared[24000..]) < 1e-3);
    }

    #[test]
    fn a_rhythm_and_a_kit_play_in_time() {
        let mut jam = Jam::new(48000);
        jam.take(Ask::Bpm(120.0));
        jam.take(Ask::Program(0, 11)); // a mallet: each note is a stroke
        jam.take(Ask::Rhythm(2)); // eighths: four notes a second
        let out = run(&mut jam, vec![Ask::Touch { x: 0.5, y: 1.0, down: true }], 2.0);
        // loud at the start of each eighth, quieter before the next: count the rises
        let level: Vec<f32> = out.chunks(480).map(rms).collect();
        let mut rises = 0;
        let mut last = 0usize;
        for i in 2..level.len() {
            // a stroke: well above what it was two hundredths of a second ago, and not the same stroke again
            if level[i] > 1.6 * level[i - 2] && level[i] > 0.02 && i - last > 5 {
                rises += 1;
                last = i;
            }
        }
        assert!((4..=9).contains(&rises), "{rises} strokes heard in two seconds of eighths");

        let mut kit = Jam::new(48000);
        kit.take(Ask::Track(3)); // the drum kit
        let beat = run(&mut kit, vec![Ask::Touch { x: 0.45, y: 0.6, down: true }], 2.0);
        assert!(rms(&beat) > 0.01 && beat.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
    }
}

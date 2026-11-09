//! `drum.analog808style`: parametric analog-*style* drum voices (original
//! model, inspired by public descriptions of classic analog drum circuits;
//! no samples, not a recreation). Instrument chosen by MIDI pitch (GM-like):
//!
//! | pitch | instrument |
//! |-------|------------|
//! | 36 | BD bass drum |
//! | 35 | punch kick (shorter, harder) |
//! | 37 | RS rim shot |
//! | 38, 40 | SD snare |
//! | 39 | CP clap |
//! | 42, 44 | CH closed hat (chokes OH) |
//! | 46 | OH open hat |
//! | 41, 43, 45, 47, 48, 50 | toms low → high |
//! | 49, 52, 55, 57 | CY crash cymbal |
//! | 51, 59 | ride |
//! | 53 | ride bell |
//! | 54 | tambourine |
//! | 56 | CB cowbell |
//! | 60, 61 | bongo high / low |
//! | 62, 63, 64 | conga mute / open / low |
//! | 70, 82 | shaker / maracas |
//! | 75, 76 | claves |
//! | other | RS |

use crate::common::{FilterMode, NumParams, NumSpec, Svf};
use crate::detmath::{db_to_gain, decay_coef, sin_turns, tanh_d, Rng};
use crate::{ParamMeta, ParamValue, Voice};

pub const DRUM_TABLE: &[NumSpec] = &[
    ("bd.tune", "Hz", 30.0, 120.0, 50.0),
    ("bd.decayMs", "ms", 50.0, 2000.0, 450.0),
    ("bd.pitchEnv", "ratio", 0.0, 8.0, 2.5),
    ("bd.click", "ratio", 0.0, 1.0, 0.3),
    ("bd.drive", "ratio", 0.0, 1.0, 0.3),
    ("sd.tune", "Hz", 100.0, 400.0, 185.0),
    ("sd.decayMs", "ms", 30.0, 1000.0, 180.0),
    ("sd.snappy", "ratio", 0.0, 1.0, 0.6),
    ("hh.tone", "Hz", 2000.0, 15000.0, 8000.0),
    ("hh.decayMs", "ms", 10.0, 500.0, 45.0),
    ("oh.decayMs", "ms", 50.0, 3000.0, 380.0),
    ("cp.decayMs", "ms", 50.0, 1500.0, 220.0),
    ("tom.decayMs", "ms", 50.0, 2000.0, 320.0),
    ("cy.decayMs", "ms", 200.0, 5000.0, 1500.0),
    ("gainDb", "dB", -60.0, 6.0, -3.0),
];
const BD_TUNE: usize = 0;
const BD_DECAY: usize = 1;
const BD_PENV: usize = 2;
const BD_CLICK: usize = 3;
const BD_DRIVE: usize = 4;
const SD_TUNE: usize = 5;
const SD_DECAY: usize = 6;
const SD_SNAPPY: usize = 7;
const HH_TONE: usize = 8;
const HH_DECAY: usize = 9;
const OH_DECAY: usize = 10;
const CP_DECAY: usize = 11;
const TOM_DECAY: usize = 12;
const CY_DECAY: usize = 13;
const GAIN: usize = 14;

#[derive(Clone, Debug, PartialEq)]
pub struct DrumParams {
    pub n: NumParams,
}

impl Default for DrumParams {
    fn default() -> Self {
        DrumParams { n: NumParams::new(DRUM_TABLE) }
    }
}

impl DrumParams {
    pub fn describe() -> Vec<ParamMeta> {
        NumParams::metas(DRUM_TABLE)
    }
    pub fn set(&mut self, path: &str, v: &ParamValue) -> Result<(), String> {
        if self.n.set(path, v)? {
            Ok(())
        } else {
            Err(format!("unknown param {path:?} for drum.analog808style"))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bd,
    Punch,
    Ride,
    RideBell,
    Tamb,
    Bongo(u8),
    Conga(u8),
    Shaker,
    Claves,
    Rs,
    Sd,
    Cp,
    Ch,
    Oh,
    Tom(u8),
    Cy,
    Cb,
}

pub fn kind_for_pitch(p: u8) -> Kind {
    match p {
        36 => Kind::Bd,
        35 => Kind::Punch,
        51 | 59 => Kind::Ride,
        53 => Kind::RideBell,
        54 => Kind::Tamb,
        60 => Kind::Bongo(0),
        61 => Kind::Bongo(1),
        62 => Kind::Conga(0),
        63 => Kind::Conga(1),
        64 => Kind::Conga(2),
        70 | 82 => Kind::Shaker,
        75 | 76 => Kind::Claves,
        37 => Kind::Rs,
        38 | 40 => Kind::Sd,
        39 => Kind::Cp,
        42 | 44 => Kind::Ch,
        46 => Kind::Oh,
        41 => Kind::Tom(0),
        43 => Kind::Tom(1),
        45 => Kind::Tom(2),
        47 => Kind::Tom(3),
        48 => Kind::Tom(4),
        50 => Kind::Tom(5),
        49 | 52 | 55 | 57 => Kind::Cy,
        56 => Kind::Cb,
        _ => Kind::Rs,
    }
}

const METAL: [f64; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];
const TOMS: [f64; 6] = [80.0, 98.0, 118.0, 140.0, 165.0, 196.0];

#[derive(Clone, Debug)]
pub struct DrumVoice {
    p: DrumParams,
    sr: f64,
    seed: u64,
    rng: Rng,
    kind: Kind,
    amp: f64,
    t: u64,
    ph: [f64; 8],
    env: f64,
    env_c: f64,
    env2: f64,
    env2_c: f64,
    penv: f64,
    penv_c: f64,
    f1: Svf,
    f2: Svf,
    active: bool,
}

impl DrumVoice {
    pub fn new(p: DrumParams, sample_rate: u32, seed: u64) -> Self {
        DrumVoice {
            p,
            sr: sample_rate as f64,
            seed,
            rng: Rng::new(seed),
            kind: Kind::Bd,
            amp: 0.0,
            t: 0,
            ph: [0.0; 8],
            env: 0.0,
            env_c: 0.0,
            env2: 0.0,
            env2_c: 0.0,
            penv: 0.0,
            penv_c: 0.0,
            f1: Svf::default(),
            f2: Svf::default(),
            active: false,
        }
    }

    fn metal(&mut self, scale: f64) -> f64 {
        let mut s = 0.0;
        for (i, f) in METAL.iter().enumerate() {
            self.ph[i] += f * scale / self.sr;
            if self.ph[i] >= 1.0 {
                self.ph[i] -= 1.0;
            }
            s += if self.ph[i] < 0.5 { 1.0 } else { -1.0 };
        }
        s / 6.0
    }

    fn sine(&mut self, i: usize, hz: f64) -> f64 {
        self.ph[i] += hz / self.sr;
        if self.ph[i] >= 1.0 {
            self.ph[i] -= 1.0;
        }
        sin_turns(self.ph[i])
    }

    fn sample(&mut self) -> f64 {
        let v: [f64; 15] = self.p.n.v[..15].try_into().expect("drum table has 15 params");
        let sr = self.sr;
        let ms = |x: f64| x * sr / 1000.0;
        let t = self.t as f64;
        let out = match self.kind {
            Kind::Bd => {
                let hz = v[BD_TUNE] * (1.0 + v[BD_PENV] * self.penv);
                let body = self.sine(0, hz) * self.env;
                let click = if t < ms(3.0) { self.rng.bipolar() * (1.0 - t / ms(3.0)) * v[BD_CLICK] } else { 0.0 };
                let x = body + click * 0.5;
                tanh_d(x * (1.0 + 2.0 * v[BD_DRIVE])) / (1.0 + 0.5 * v[BD_DRIVE])
            }
            Kind::Punch => {
                let hz = v[BD_TUNE] * 1.25 * (1.0 + (v[BD_PENV] + 2.0) * self.penv);
                let body = self.sine(0, hz) * self.env;
                let click = if t < ms(4.0) { self.rng.bipolar() * (1.0 - t / ms(4.0)) } else { 0.0 };
                tanh_d((body + 0.6 * click) * 2.2) * 0.8
            }
            Kind::Ride | Kind::RideBell => {
                let m = self.metal(1.3) * 0.6 + self.rng.bipolar() * 0.15;
                let shimmer = self.f2.process(self.f1.process(m, FilterMode::Bp), FilterMode::Hp);
                let bell = 0.3 * (self.sine(6, 1720.0) + self.sine(7, 2580.0));
                let bw = if self.kind == Kind::RideBell { 1.0 } else { 0.25 };
                (shimmer * 1.4 + bell * bw * self.env2) * self.env
            }
            Kind::Tamb => {
                // jingles: bright noise in a few quick bursts + decay
                let burst = ms(14.0);
                let e = if t < 3.0 * burst { 0.6 + 0.4 * (1.0 - (t - (t / burst).floor() * burst) / burst) } else { 1.0 };
                let n = self.f2.process(self.f1.process(self.rng.bipolar(), FilterMode::Bp), FilterMode::Hp);
                n * e * self.env * 0.7
            }
            Kind::Bongo(i) => {
                let base = [420.0, 300.0][i as usize];
                let hz = base * (1.0 + 0.25 * self.penv);
                let slap = if t < ms(3.0) { self.rng.bipolar() * 0.5 } else { 0.0 };
                (self.sine(0, hz) + slap) * self.env * 0.55
            }
            Kind::Conga(i) => {
                let base = [340.0, 230.0, 180.0][i as usize];
                let hz = base * (1.0 + 0.12 * self.penv);
                let skin = 0.25 * self.sine(1, hz * 1.58) * self.env2;
                let slap = if t < ms(2.5) { self.rng.bipolar() * 0.4 } else { 0.0 };
                (self.sine(0, hz) + skin + slap) * self.env * 0.5
            }
            Kind::Shaker => {
                let a = (t / ms(12.0)).min(1.0); // soft attack
                self.f1.process(self.rng.bipolar(), FilterMode::Hp) * a * self.env * 1.5
            }
            Kind::Claves => (0.7 * self.sine(0, 2500.0) + 0.3 * self.sine(1, 1700.0)) * self.env,
            Kind::Tom(i) => {
                let hz = TOMS[i as usize] * (1.0 + 0.6 * self.penv);
                self.sine(0, hz) * self.env
            }
            Kind::Sd => {
                let f = v[SD_TUNE] * (1.0 + 0.3 * self.penv);
                let tone = 0.6 * self.sine(0, f) + 0.4 * self.sine(1, f * 1.62);
                let n = self.f1.process(self.rng.bipolar(), FilterMode::Hp);
                tone * self.env2 * (1.0 - 0.5 * v[SD_SNAPPY]) + n * self.env * v[SD_SNAPPY] * 0.8
            }
            Kind::Rs => {
                let x = 0.6 * self.sine(0, 1700.0) + 0.4 * self.sine(1, 480.0);
                let click = if t < ms(1.0) { self.rng.bipolar() } else { 0.0 };
                (x + 0.3 * click) * self.env
            }
            Kind::Cp => {
                // three quick bursts, then the tail
                let burst = ms(10.0);
                let e = if t < 3.0 * burst {
                    let local = t - (t / burst).floor() * burst;
                    1.0 - local / burst
                } else {
                    self.env
                };
                self.f1.process(self.rng.bipolar(), FilterMode::Bp) * e * 2.0
            }
            Kind::Ch | Kind::Oh | Kind::Cy => {
                let scale = if self.kind == Kind::Cy { 0.75 } else { 1.0 };
                let m = self.metal(scale) * 0.7 + self.rng.bipolar() * 0.3;
                let b = self.f1.process(m, FilterMode::Bp);
                self.f2.process(b, FilterMode::Hp) * self.env * 2.0
            }
            Kind::Cb => {
                self.ph[1] += 587.0 / self.sr;
                self.ph[2] += 845.0 / self.sr;
                for i in 1..3 {
                    if self.ph[i] >= 1.0 {
                        self.ph[i] -= 1.0;
                    }
                }
                let sq = |p: f64| if p < 0.5 { 1.0 } else { -1.0 };
                let x = 0.5 * (sq(self.ph[1]) + sq(self.ph[2]));
                self.f1.process(x, FilterMode::Bp) * self.env
            }
        };
        self.env *= self.env_c;
        self.env2 *= self.env2_c;
        self.penv *= self.penv_c;
        self.t += 1;
        out
    }
}

impl Voice for DrumVoice {
    fn note_on(&mut self, pitch: u8, velocity: f32, _tick: u64) {
        let v = self.p.n.v.clone();
        let sr = self.sr;
        self.kind = kind_for_pitch(pitch);
        self.amp = velocity as f64 * db_to_gain(v[GAIN]);
        self.rng = Rng::new(self.seed);
        self.t = 0;
        self.ph = [0.0; 8];
        self.env = 1.0;
        self.env2 = 1.0;
        self.penv = 1.0;
        self.penv_c = decay_coef(40.0, sr);
        let (d1, d2) = match self.kind {
            Kind::Bd => (v[BD_DECAY], v[BD_DECAY]),
            Kind::Tom(_) => (v[TOM_DECAY], v[TOM_DECAY]),
            Kind::Sd => (v[SD_DECAY], v[SD_DECAY] * 0.6),
            Kind::Rs => (30.0, 30.0),
            Kind::Cp => (v[CP_DECAY], v[CP_DECAY]),
            Kind::Ch => (v[HH_DECAY], v[HH_DECAY]),
            Kind::Oh => (v[OH_DECAY], v[OH_DECAY]),
            Kind::Cy => (v[CY_DECAY], v[CY_DECAY]),
            Kind::Cb => (140.0, 140.0),
            Kind::Punch => (v[BD_DECAY] * 0.5, v[BD_DECAY] * 0.5),
            Kind::Ride => (v[CY_DECAY] * 1.4, v[CY_DECAY] * 0.5),
            Kind::RideBell => (v[CY_DECAY], v[CY_DECAY] * 0.8),
            Kind::Tamb => (220.0, 220.0),
            Kind::Bongo(_) => (140.0, 140.0),
            Kind::Conga(i) => ([90.0, 260.0, 330.0][i as usize], 120.0),
            Kind::Shaker => (90.0, 90.0),
            Kind::Claves => (45.0, 45.0),
        };
        self.env_c = decay_coef(d1, sr);
        self.env2_c = decay_coef(d2, sr);
        match self.kind {
            Kind::Sd => self.f1.set(1800.0, 0.1, sr),
            Kind::Cp => self.f1.set(1150.0, 0.45, sr),
            Kind::Ch | Kind::Oh => {
                self.f1.set(v[HH_TONE], 0.3, sr);
                self.f2.set(v[HH_TONE] * 0.7, 0.1, sr);
            }
            Kind::Cy => {
                self.f1.set(v[HH_TONE] * 0.8, 0.2, sr);
                self.f2.set(v[HH_TONE] * 0.5, 0.1, sr);
            }
            Kind::Cb => self.f1.set(2600.0, 0.5, sr),
            Kind::Ride | Kind::RideBell => {
                self.f1.set(v[HH_TONE] * 0.9, 0.25, sr);
                self.f2.set(v[HH_TONE] * 0.45, 0.1, sr);
            }
            Kind::Tamb => {
                self.f1.set(7500.0, 0.5, sr);
                self.f2.set(4000.0, 0.1, sr);
            }
            Kind::Shaker => self.f1.set(5500.0, 0.2, sr),
            _ => {}
        }
        self.active = true;
    }
    /// One-shots ignore note-off.
    fn note_off(&mut self, _pitch: u8, _tick: u64) {}
    fn set_param(&mut self, path: &str, value: &ParamValue) -> Result<(), String> {
        self.p.set(path, value)
    }
    fn render(&mut self, out: &mut [f32]) {
        for s in out.iter_mut() {
            if !self.active {
                return;
            }
            *s += (self.sample() * self.amp) as f32;
            if self.env < 1e-4 && self.env2 < 1e-4 && self.t > (self.sr * 0.04) as u64 {
                self.active = false;
            }
        }
    }
    fn reset(&mut self) {
        self.active = false;
    }
    fn is_active(&self) -> bool {
        self.active
    }
    fn level(&self) -> f32 {
        (self.env * self.amp) as f32
    }
    fn describe_params(&self) -> Vec<ParamMeta> {
        DrumParams::describe()
    }
    fn choke_group(&self) -> Option<u8> {
        matches!(self.kind, Kind::Ch | Kind::Oh).then_some(1)
    }
    fn chokes_others(&self) -> bool {
        self.kind == Kind::Ch
    }
}

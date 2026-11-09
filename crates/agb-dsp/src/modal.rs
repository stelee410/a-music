//! `synth.modal`: modal (additive) synthesis of struck resonant bodies —
//! singing bowls, crystal bowls, tuning forks, tubular chimes, bells, gongs.
//!
//! Each body is a set of inharmonic partials. Every partial is rendered as a
//! *pair* of slightly detuned modes (real bowls split each mode into two
//! nearly-degenerate modes), which produces the characteristic slow beating
//! ("wah-wah"). Higher partials decay faster; a short filtered noise burst
//! models the mallet strike. Original model; no samples.

use crate::common::{enum_meta, NumParams, NumSpec};
use crate::detmath::{db_to_gain, decay_coef, midi_to_hz, sin_turns, Rng};
use crate::{ParamMeta, ParamValue, Voice};

pub const MODAL_TABLE: &[NumSpec] = &[
    ("freqHz", "Hz", 0.0, 4000.0, 0.0),
    ("decay", "x", 0.05, 4.0, 1.0),
    ("brightness", "ratio", 0.0, 1.0, 0.5),
    ("beat", "x", 0.0, 3.0, 1.0),
    ("strike", "ratio", 0.0, 1.0, 0.4),
    ("gainDb", "dB", -60.0, 6.0, -6.0),
];
const FREQ: usize = 0;
const DECAY: usize = 1;
const BRIGHT: usize = 2;
const BEAT: usize = 3;
const STRIKE: usize = 4;
const GAIN: usize = 5;

const MODELS: &[&str] = &["bowl", "crystal", "fork", "chime", "bell", "gong"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    Bowl,
    Crystal,
    Fork,
    Chime,
    Bell,
    Gong,
}

/// (frequency ratio, amplitude, T60 seconds at decay=1, beat Hz at beat=1)
type Partial = (f64, f64, f64, f64);

impl Model {
    fn partials(self) -> &'static [Partial] {
        match self {
            // Himalayan bowl: strongly inharmonic, long-ringing, audible beating.
            Model::Bowl => &[
                (1.0, 1.0, 22.0, 0.9),
                (2.71, 0.62, 14.0, 1.7),
                (5.15, 0.36, 8.0, 2.6),
                (8.27, 0.20, 5.0, 3.1),
                (12.07, 0.11, 3.0, 4.0),
                (16.6, 0.06, 2.0, 4.7),
            ],
            // Quartz crystal bowl: almost pure, very long, slow shimmer.
            Model::Crystal => &[(1.0, 1.0, 30.0, 0.35), (2.02, 0.10, 12.0, 0.6), (3.03, 0.04, 6.0, 0.8)],
            // Tuning fork: pure fundamental, a quick "clang" mode at ~6.27x.
            Model::Fork => &[(1.0, 1.0, 18.0, 0.0), (6.27, 0.22, 0.35, 0.0), (17.55, 0.05, 0.08, 0.0)],
            // Free-free bar (tubular chime) modes.
            Model::Chime => &[
                (1.0, 1.0, 7.0, 0.25),
                (2.756, 0.75, 4.5, 0.4),
                (5.404, 0.45, 2.5, 0.6),
                (8.933, 0.25, 1.4, 0.8),
                (13.34, 0.12, 0.8, 1.0),
            ],
            // Church-bell partials: hum, prime, minor-third tierce, quint, nominal…
            Model::Bell => &[
                (0.5, 0.55, 12.0, 0.4),
                (1.0, 0.8, 8.0, 0.6),
                (1.183, 0.6, 6.0, 0.8),
                (1.5, 0.35, 5.0, 0.7),
                (2.0, 0.7, 4.0, 1.0),
                (2.5, 0.25, 2.5, 1.2),
                (3.0, 0.2, 2.0, 1.4),
                (4.07, 0.12, 1.2, 1.6),
            ],
            // Gong: dense inharmonic cluster, long wash.
            Model::Gong => &[
                (1.0, 1.0, 14.0, 0.5),
                (1.47, 0.7, 12.0, 0.9),
                (1.98, 0.6, 10.0, 1.3),
                (2.44, 0.5, 9.0, 1.7),
                (2.91, 0.42, 7.0, 2.1),
                (3.6, 0.35, 6.0, 2.6),
                (4.17, 0.28, 5.0, 3.0),
                (5.03, 0.2, 4.0, 3.4),
                (6.12, 0.14, 3.0, 3.9),
                (7.4, 0.09, 2.0, 4.3),
            ],
        }
    }

    fn strike_hz(self) -> f64 {
        match self {
            Model::Fork => 3000.0,
            Model::Crystal => 1500.0,
            Model::Chime | Model::Bell => 4000.0,
            _ => 2200.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModalParams {
    pub model: Model,
    pub n: NumParams,
}

impl Default for ModalParams {
    fn default() -> Self {
        ModalParams { model: Model::Bowl, n: NumParams::new(MODAL_TABLE) }
    }
}

impl ModalParams {
    pub fn describe() -> Vec<ParamMeta> {
        let mut v = vec![enum_meta("model", MODELS, "bowl")];
        v.extend(NumParams::metas(MODAL_TABLE));
        v
    }

    pub fn set(&mut self, path: &str, v: &ParamValue) -> Result<(), String> {
        if path == "model" {
            let ParamValue::Str(s) = v else { return Err(format!("model expects one of {MODELS:?}")) };
            self.model = match s.as_str() {
                "bowl" => Model::Bowl,
                "crystal" => Model::Crystal,
                "fork" => Model::Fork,
                "chime" => Model::Chime,
                "bell" => Model::Bell,
                "gong" => Model::Gong,
                _ => return Err(format!("model {s:?} not in {MODELS:?}")),
            };
            return Ok(());
        }
        if self.n.set(path, v)? {
            Ok(())
        } else {
            Err(format!("unknown param {path:?} for synth.modal"))
        }
    }
}

#[derive(Clone, Debug)]
struct Mode {
    dt_a: f64,
    dt_b: f64,
    ph_a: f64,
    ph_b: f64,
    amp: f64,
    coef: f64,
}

#[derive(Clone, Debug)]
pub struct ModalVoice {
    p: ModalParams,
    sr: f64,
    seed: u64,
    rng: Rng,
    modes: Vec<Mode>,
    amp: f64,
    strike_env: f64,
    strike_coef: f64,
    bp_lp: f64,
    bp_hp: f64,
    bp_a: f64,
    active: bool,
    counter: u32,
}

impl ModalVoice {
    pub fn new(p: ModalParams, sample_rate: u32, seed: u64) -> Self {
        ModalVoice {
            p,
            sr: sample_rate as f64,
            seed,
            rng: Rng::new(seed),
            modes: vec![],
            amp: 0.0,
            strike_env: 0.0,
            strike_coef: 0.0,
            bp_lp: 0.0,
            bp_hp: 0.0,
            bp_a: 0.0,
            active: false,
            counter: 0,
        }
    }
}

impl Voice for ModalVoice {
    fn note_on(&mut self, pitch: u8, velocity: f32, _tick: u64) {
        let v = self.p.n.v.clone();
        let f0 = if v[FREQ] > 0.0 { v[FREQ] } else { midi_to_hz(pitch) };
        let nyq = self.sr * 0.45;
        let bright = v[BRIGHT];
        let strike = v[STRIKE];
        self.modes = self
            .p
            .model
            .partials()
            .iter()
            .enumerate()
            .filter_map(|(k, &(ratio, a, t60, beat))| {
                let f = f0 * ratio;
                if f >= nyq {
                    return None;
                }
                // brightness tilts the spectrum; a harder strike excites upper partials more
                let tilt = if k == 0 { 1.0 } else { (0.35 + 1.3 * bright) * (0.7 + 0.6 * strike) };
                let bh = beat * v[BEAT] * 0.5;
                Some(Mode {
                    dt_a: (f - bh) / self.sr,
                    dt_b: (f + bh) / self.sr,
                    ph_a: 0.0,
                    ph_b: 0.25 * k as f64,
                    amp: a * tilt.min(1.6) * 0.5,
                    coef: decay_coef(t60 * v[DECAY] * 1000.0, self.sr),
                })
            })
            .collect();
        self.amp = velocity as f64 * db_to_gain(v[GAIN]) * 0.6;
        self.strike_env = strike;
        self.strike_coef = decay_coef(12.0 + 30.0 * (1.0 - strike), self.sr);
        self.bp_a = (2.0 * std::f64::consts::PI * self.p.model.strike_hz() / self.sr).min(0.9);
        self.bp_lp = 0.0;
        self.bp_hp = 0.0;
        self.rng = Rng::new(self.seed);
        self.active = true;
        self.counter = 0;
    }

    /// Struck bodies ring on; note-off is ignored.
    fn note_off(&mut self, _pitch: u8, _tick: u64) {}

    fn set_param(&mut self, path: &str, value: &ParamValue) -> Result<(), String> {
        self.p.set(path, value)
    }

    fn render(&mut self, out: &mut [f32]) {
        for s in out.iter_mut() {
            if !self.active {
                return;
            }
            let mut x = 0.0;
            for m in self.modes.iter_mut() {
                x += (sin_turns(m.ph_a) + sin_turns(m.ph_b)) * m.amp;
                m.ph_a += m.dt_a;
                if m.ph_a >= 1.0 {
                    m.ph_a -= 1.0;
                }
                m.ph_b += m.dt_b;
                if m.ph_b >= 1.0 {
                    m.ph_b -= 1.0;
                }
                m.amp *= m.coef;
            }
            if self.strike_env > 1e-4 {
                // band-limited mallet click
                let w = self.rng.bipolar();
                self.bp_lp += self.bp_a * (w - self.bp_lp);
                let hp = self.bp_lp - self.bp_hp;
                self.bp_hp += 0.02 * (self.bp_lp - self.bp_hp);
                x += hp * self.strike_env * 0.6;
                self.strike_env *= self.strike_coef;
            }
            *s += (x * self.amp) as f32;
            self.counter = self.counter.wrapping_add(1);
            if self.counter % 256 == 0 {
                let total: f64 = self.modes.iter().map(|m| m.amp).sum::<f64>() + self.strike_env;
                if total < 1e-4 {
                    self.active = false;
                }
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
        (self.modes.iter().map(|m| m.amp).sum::<f64>() * self.amp) as f32
    }

    fn describe_params(&self) -> Vec<ParamMeta> {
        ModalParams::describe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spectrum_peak_near(buf: &[f32], sr: f64, hz: f64) -> f64 {
        // Goertzel magnitude at hz over the first 0.5 s
        let n = (sr * 0.5) as usize;
        let w = 2.0 * std::f64::consts::PI * hz / sr;
        let (mut s1, mut s2) = (0.0, 0.0);
        for &x in &buf[..n] {
            let s0 = x as f64 + 2.0 * w.cos() * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2).sqrt() / n as f64
    }

    #[test]
    fn bowl_is_inharmonic_and_fork_is_pure() {
        let sr = 48000.0;
        let render = |model: &str, freq: f64| {
            let mut p = ModalParams::default();
            p.set("model", &ParamValue::Str(model.into())).unwrap();
            p.set("freqHz", &ParamValue::Num(freq)).unwrap();
            let mut v = ModalVoice::new(p, 48000, 1);
            v.note_on(60, 1.0, 0);
            let mut b = vec![0.0f32; 48000];
            v.render(&mut b);
            b
        };
        let bowl = render("bowl", 200.0);
        let fund = spectrum_peak_near(&bowl, sr, 200.0);
        let p2 = spectrum_peak_near(&bowl, sr, 542.0); // 2.71x
        let harmonic = spectrum_peak_near(&bowl, sr, 400.0); // 2x: should be weak
        assert!(fund > 0.01 && p2 > 0.2 * fund && harmonic < 0.3 * p2, "{fund} {p2} {harmonic}");

        let fork = render("fork", 528.0);
        let f = spectrum_peak_near(&fork, sr, 528.0);
        let second = spectrum_peak_near(&fork, sr, 1056.0);
        assert!(f > 0.01 && second < 0.05 * f, "fork should be pure: {f} {second}");
        assert!(fork.iter().all(|x| x.is_finite() && x.abs() < 1.5));
    }
}

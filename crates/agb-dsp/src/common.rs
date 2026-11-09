//! Shared building blocks: numeric parameter tables, ADSR, band-limited
//! oscillator, TPT state-variable filter and a ladder-inspired 4-pole filter.

use crate::detmath::{tan_turns, tanh_d, Rng};
use crate::waveform::Wave;
use crate::{ParamKind, ParamMeta, ParamValue};
use serde_json::json;

/// `(path, unit, min, max, default)`
pub type NumSpec = (&'static str, &'static str, f64, f64, f64);

/// Numeric parameter block backed by a static table.
#[derive(Clone, Debug, PartialEq)]
pub struct NumParams {
    pub table: &'static [NumSpec],
    pub v: Vec<f64>,
}

impl NumParams {
    pub fn new(table: &'static [NumSpec]) -> Self {
        NumParams { table, v: table.iter().map(|t| t.4).collect() }
    }

    pub fn index(&self, path: &str) -> Option<usize> {
        self.table.iter().position(|t| t.0 == path)
    }

    /// `Ok(false)` if `path` is not in this table.
    pub fn set(&mut self, path: &str, v: &ParamValue) -> Result<bool, String> {
        let Some(i) = self.index(path) else { return Ok(false) };
        let (_, _, lo, hi, _) = self.table[i];
        let n = match v {
            ParamValue::Num(n) => *n,
            ParamValue::Str(_) => return Err(format!("param {path} expects a number")),
        };
        if !n.is_finite() || n < lo || n > hi {
            return Err(format!("param {path}={n} out of range {lo}..={hi}"));
        }
        self.v[i] = n;
        Ok(true)
    }

    pub fn metas(table: &'static [NumSpec]) -> Vec<ParamMeta> {
        table
            .iter()
            .map(|&(path, unit, min, max, d)| ParamMeta {
                path,
                unit,
                kind: ParamKind::Continuous,
                min: Some(min),
                max: Some(max),
                default: json!(d),
                safe_max: if path == "gainDb" { Some(0.0) } else { None },
                choices: None,
            })
            .collect()
    }
}

pub fn enum_meta(path: &'static str, choices: &[&'static str], default: &str) -> ParamMeta {
    ParamMeta {
        path,
        unit: "enum",
        kind: ParamKind::Discrete,
        min: None,
        max: None,
        default: json!(default),
        safe_max: None,
        choices: Some(choices.to_vec()),
    }
}

/// Linear ADSR (same shape as `synth.waveform`).
#[derive(Clone, Debug, Default)]
pub struct Adsr {
    pub stage: u8, // 0 idle, 1 attack, 2 decay, 3 sustain, 4 release
    pub level: f64,
    rel_step: f64,
}

impl Adsr {
    pub fn trigger(&mut self) {
        self.stage = 1;
        self.level = 0.0;
    }
    pub fn release(&mut self, release_samples: f64) {
        if self.stage == 0 || self.stage == 4 {
            return;
        }
        self.stage = 4;
        self.rel_step = if release_samples < 1.0 { f64::INFINITY } else { self.level / release_samples };
    }
    pub fn is_idle(&self) -> bool {
        self.stage == 0
    }
    /// One sample. Times in samples.
    pub fn next(&mut self, a: f64, d: f64, s: f64) -> f64 {
        match self.stage {
            1 => {
                self.level = if a < 1.0 { 1.0 } else { self.level + 1.0 / a };
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = 2;
                }
            }
            2 => {
                self.level = if d < 1.0 { s } else { self.level - (1.0 - s) / d };
                if self.level <= s {
                    self.level = s;
                    self.stage = 3;
                }
            }
            3 => self.level = s,
            4 => {
                self.level -= self.rel_step;
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = 0;
                }
            }
            _ => {}
        }
        self.level
    }
}

fn poly_blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

/// PolyBLEP oscillator.
#[derive(Clone, Debug, Default)]
pub struct Osc {
    pub phase: f64,
    pub dt: f64,
}

impl Osc {
    pub fn set_freq(&mut self, hz: f64, sr: f64) {
        self.dt = (hz / sr).clamp(0.0, 0.49);
    }
    pub fn next(&mut self, wave: Wave, pw: f64, rng: &mut Rng) -> f64 {
        let t = self.phase;
        let dt = self.dt;
        let v = match wave {
            Wave::Sine => crate::detmath::sin_turns(t),
            Wave::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
            Wave::Square => {
                let mut v = if t < pw { 1.0 } else { -1.0 };
                v += poly_blep(t, dt);
                let t2 = t + 1.0 - pw;
                v -= poly_blep(t2 - t2.floor(), dt);
                v
            }
            Wave::Tri => {
                if t < 0.5 {
                    4.0 * t - 1.0
                } else {
                    3.0 - 4.0 * t
                }
            }
            Wave::Noise => rng.bipolar(),
        };
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        v
    }
}

/// Zero-delay-feedback (TPT) state-variable filter.
#[derive(Clone, Debug, Default)]
pub struct Svf {
    ic1: f64,
    ic2: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    k: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterMode {
    Lp,
    Hp,
    Bp,
}

impl Svf {
    /// `res` 0..1 maps to Q ≈ 0.5 … 12.
    pub fn set(&mut self, cutoff_hz: f64, res: f64, sr: f64) {
        let fc = cutoff_hz.clamp(10.0, sr * 0.45);
        let g = tan_turns(fc / (2.0 * sr));
        self.k = 2.0 * (1.0 - 0.96 * res.clamp(0.0, 1.0));
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }
    pub fn process(&mut self, v0: f64, mode: FilterMode) -> f64 {
        let v3 = v0 - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        match mode {
            FilterMode::Lp => v2,
            FilterMode::Bp => v1,
            FilterMode::Hp => v0 - self.k * v1 - v2,
        }
    }
}

/// Ladder-*inspired* 24 dB/oct low-pass: four TPT one-pole stages with
/// saturated global feedback. Not a model of any specific circuit.
#[derive(Clone, Debug, Default)]
pub struct Ladder {
    s: [f64; 4],
    g: f64,
    k: f64,
    y4: f64,
}

impl Ladder {
    pub fn set(&mut self, cutoff_hz: f64, res: f64, sr: f64) {
        let fc = cutoff_hz.clamp(10.0, sr * 0.45);
        let g = tan_turns(fc / (2.0 * sr));
        self.g = g / (1.0 + g);
        self.k = 3.9 * res.clamp(0.0, 1.0);
    }
    pub fn process(&mut self, x: f64, drive: f64) -> f64 {
        let dg = 1.0 + 3.0 * drive;
        let mut u = tanh_d(x * dg - self.k * self.y4) ;
        for s in self.s.iter_mut() {
            let v = (u - *s) * self.g;
            let y = v + *s;
            *s = y + v;
            u = y;
        }
        self.y4 = u;
        // passband compensation for resonance loss, undo drive gain
        u * (1.0 + 0.5 * self.k) / dg.sqrt()
    }
}

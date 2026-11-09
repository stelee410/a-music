//! `synth.waveform`: basic oscillator (sine/saw/square/tri/noise + PWM) with a
//! linear ADSR. Saw and square use PolyBLEP band-limiting.

use crate::detmath::{db_to_gain, midi_to_hz, sin_turns, Rng};
use crate::{ParamKind, ParamMeta, ParamValue, Voice};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Wave {
    Sine,
    Saw,
    Square,
    Tri,
    Noise,
}

impl Wave {
    fn parse(s: &str) -> Option<Wave> {
        Some(match s {
            "sine" => Wave::Sine,
            "saw" => Wave::Saw,
            "square" => Wave::Square,
            "tri" => Wave::Tri,
            "noise" => Wave::Noise,
            _ => return None,
        })
    }
}

/// Resolved parameters (§9.4).
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WaveformParams {
    pub wave: Wave,
    pub pulse_width: f64,
    pub attack_ms: f64,
    pub decay_ms: f64,
    pub sustain: f64,
    pub release_ms: f64,
    pub gain_db: f64,
}

impl Default for WaveformParams {
    fn default() -> Self {
        WaveformParams {
            wave: Wave::Saw,
            pulse_width: 0.5,
            attack_ms: 5.0,
            decay_ms: 200.0,
            sustain: 0.7,
            release_ms: 100.0,
            gain_db: -6.0,
        }
    }
}

const WAVES: &[&str] = &["sine", "saw", "square", "tri", "noise"];

pub fn describe_params() -> Vec<ParamMeta> {
    let c = |path, unit, min, max, default: f64| ParamMeta {
        path,
        unit,
        kind: ParamKind::Continuous,
        min: Some(min),
        max: Some(max),
        default: json!(default),
        safe_max: None,
        choices: None,
    };
    vec![
        ParamMeta {
            path: "osc.wave",
            unit: "enum",
            kind: ParamKind::Discrete,
            min: None,
            max: None,
            default: json!("saw"),
            safe_max: None,
            choices: Some(WAVES.to_vec()),
        },
        c("osc.pulseWidth", "ratio", 0.01, 0.99, 0.5),
        c("amp.attackMs", "ms", 0.0, 5000.0, 5.0),
        c("amp.decayMs", "ms", 0.0, 10000.0, 200.0),
        c("amp.sustain", "ratio", 0.0, 1.0, 0.7),
        c("amp.releaseMs", "ms", 0.0, 10000.0, 100.0),
        ParamMeta { safe_max: Some(0.0), ..c("gainDb", "dB", -60.0, 6.0, -6.0) },
    ]
}

impl WaveformParams {
    /// Set one parameter by path, validating type and range.
    pub fn set(&mut self, path: &str, v: &ParamValue) -> Result<(), String> {
        let num = |v: &ParamValue| match v {
            ParamValue::Num(n) => Ok(*n),
            ParamValue::Str(_) => Err(format!("param {path} expects a number")),
        };
        let meta = describe_params().into_iter().find(|m| m.path == path);
        let Some(meta) = meta else {
            return Err(format!("unknown param {path:?} for synth.waveform"));
        };
        if path == "osc.wave" {
            let ParamValue::Str(s) = v else {
                return Err("param osc.wave expects one of sine/saw/square/tri/noise".into());
            };
            self.wave = Wave::parse(s).ok_or_else(|| format!("osc.wave {s:?} not in {WAVES:?}"))?;
            return Ok(());
        }
        let n = num(v)?;
        if !n.is_finite() || n < meta.min.unwrap() || n > meta.max.unwrap() {
            return Err(format!(
                "param {path}={n} out of range {}..={}",
                meta.min.unwrap(),
                meta.max.unwrap()
            ));
        }
        match path {
            "osc.pulseWidth" => self.pulse_width = n,
            "amp.attackMs" => self.attack_ms = n,
            "amp.decayMs" => self.decay_ms = n,
            "amp.sustain" => self.sustain = n,
            "amp.releaseMs" => self.release_ms = n,
            "gainDb" => self.gain_db = n,
            _ => unreachable!(),
        }
        Ok(())
    }

    /// Defaults overlaid by each map in order (e.g. preset, then track overrides).
    pub fn resolve(layers: &[&BTreeMap<String, ParamValue>]) -> Result<Self, String> {
        let mut p = WaveformParams::default();
        for m in layers {
            for (k, v) in *m {
                p.set(k, v)?;
            }
        }
        Ok(p)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// One monophonic voice of `synth.waveform`.
#[derive(Clone, Debug)]
pub struct WaveformVoice {
    params: WaveformParams,
    sample_rate: f64,
    seed: u64,
    rng: Rng,
    phase: f64,
    dt: f64,
    amp: f64,
    pitch: u8,
    stage: Stage,
    env: f64,
    release_step: f64,
}

/// PolyBLEP residual for a discontinuity at phase 0.
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

impl WaveformVoice {
    pub fn new(params: WaveformParams, sample_rate: u32, seed: u64) -> Self {
        WaveformVoice {
            params,
            sample_rate: sample_rate as f64,
            seed,
            rng: Rng::new(seed),
            phase: 0.0,
            dt: 0.0,
            amp: 0.0,
            pitch: 0,
            stage: Stage::Idle,
            env: 0.0,
            release_step: 0.0,
        }
    }

    fn ms_to_samples(&self, ms: f64) -> f64 {
        (ms * self.sample_rate / 1000.0).max(0.0)
    }

    fn osc(&mut self) -> f64 {
        let t = self.phase;
        let dt = self.dt;
        match self.params.wave {
            Wave::Sine => sin_turns(t),
            Wave::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
            Wave::Square => {
                let pw = self.params.pulse_width;
                let mut v = if t < pw { 1.0 } else { -1.0 };
                v += poly_blep(t, dt);
                let t2 = t + 1.0 - pw;
                v -= poly_blep(t2 - t2.floor(), dt);
                v
            }
            Wave::Tri => {
                let v = 4.0 * t - 1.0;
                if t < 0.5 {
                    v
                } else {
                    3.0 - 4.0 * t
                }
            }
            Wave::Noise => self.rng.bipolar(),
        }
    }

    fn env_step(&mut self) -> f64 {
        let p = &self.params;
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                let n = self.ms_to_samples(p.attack_ms);
                if n < 1.0 {
                    self.env = 1.0;
                } else {
                    self.env += 1.0 / n;
                }
                if self.env >= 1.0 {
                    self.env = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                let n = self.ms_to_samples(p.decay_ms);
                let s = p.sustain;
                if n < 1.0 {
                    self.env = s;
                } else {
                    self.env -= (1.0 - s) / n;
                }
                if self.env <= s {
                    self.env = s;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {
                self.env = p.sustain;
            }
            Stage::Release => {
                self.env -= self.release_step;
                if self.env <= 0.0 {
                    self.env = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.env
    }

    /// Apply a sustain level of zero → a decaying "one-shot" ends naturally.
    fn finished_without_release(&self) -> bool {
        self.stage == Stage::Sustain && self.params.sustain <= 0.0
    }
}

impl Voice for WaveformVoice {
    fn note_on(&mut self, pitch: u8, velocity: f32, _tick: u64) {
        self.pitch = pitch;
        let hz = midi_to_hz(pitch);
        self.dt = (hz / self.sample_rate).min(0.5);
        self.phase = 0.0;
        self.amp = velocity as f64 * db_to_gain(self.params.gain_db);
        self.env = 0.0;
        self.stage = Stage::Attack;
        self.rng = Rng::new(self.seed);
    }

    fn note_off(&mut self, _pitch: u8, _tick: u64) {
        if matches!(self.stage, Stage::Idle | Stage::Release) {
            return;
        }
        let n = self.ms_to_samples(self.params.release_ms);
        self.stage = Stage::Release;
        self.release_step = if n < 1.0 { f64::INFINITY } else { self.env / n };
    }

    fn set_param(&mut self, path: &str, value: &ParamValue) -> Result<(), String> {
        self.params.set(path, value)
    }

    fn render(&mut self, out: &mut [f32]) {
        for s in out.iter_mut() {
            if self.stage == Stage::Idle {
                return;
            }
            let e = self.env_step();
            let v = self.osc() * e * self.amp;
            *s += v as f32;
            self.phase += self.dt;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
            if self.finished_without_release() {
                self.stage = Stage::Idle;
            }
        }
    }

    fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.env = 0.0;
        self.phase = 0.0;
    }

    fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }

    fn level(&self) -> f32 {
        (self.env * self.amp) as f32
    }

    fn describe_params(&self) -> Vec<ParamMeta> {
        describe_params()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(p: WaveformParams, on: usize, total: usize) -> Vec<f32> {
        let mut v = WaveformVoice::new(p, 48000, 1);
        v.note_on(57, 1.0, 0);
        let mut buf = vec![0.0f32; total];
        v.render(&mut buf[..on]);
        v.note_off(57, 0);
        v.render(&mut buf[on..]);
        buf
    }

    #[test]
    fn all_waves_bounded_and_release_to_silence() {
        for w in ["sine", "saw", "square", "tri", "noise"] {
            let mut p = WaveformParams::default();
            p.set("osc.wave", &ParamValue::Str(w.into())).unwrap();
            let b = run(p, 4800, 4800 + 9600);
            let peak = b.iter().fold(0.0f32, |a, x| a.max(x.abs()));
            assert!(peak > 0.1 && peak < 1.0, "{w} peak {peak}");
            // release 100ms = 4800 samples; afterwards silent
            assert!(b[4800 + 4900..].iter().all(|x| *x == 0.0), "{w} not silent after release");
        }
    }

    #[test]
    fn param_validation() {
        let mut p = WaveformParams::default();
        assert!(p.set("osc.wave", &ParamValue::Str("triangle".into())).is_err());
        assert!(p.set("amp.sustain", &ParamValue::Num(1.5)).is_err());
        assert!(p.set("nope", &ParamValue::Num(1.0)).is_err());
        assert!(p.set("amp.attackMs", &ParamValue::Str("x".into())).is_err());
        assert!(p.set("osc.pulseWidth", &ParamValue::Num(0.2)).is_ok());
    }

    #[test]
    fn deterministic() {
        let a = run(WaveformParams::default(), 1000, 6000);
        let b = run(WaveformParams::default(), 1000, 6000);
        assert_eq!(a, b);
    }
}

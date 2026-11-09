//! `synth.subtractive` (2 OSC + noise → SVF → amp) and `synth.moogish`
//! (same voice, ladder-inspired 24 dB low-pass with drive). "moogish" is a
//! descriptive label only — not a recreation of any manufacturer's product.

use crate::common::{enum_meta, Adsr, FilterMode, Ladder, NumParams, NumSpec, Osc, Svf};
use crate::detmath::{db_to_gain, exp2, midi_to_hz, Rng};
use crate::waveform::Wave;
use crate::{ParamMeta, ParamValue, Voice};

pub const SUB_TABLE: &[NumSpec] = &[
    ("osc.pulseWidth", "ratio", 0.01, 0.99, 0.5),
    ("osc2.level", "ratio", 0.0, 1.0, 0.0),
    ("osc2.semitones", "st", -24.0, 24.0, 0.0),
    ("osc2.detuneCents", "cents", -100.0, 100.0, 7.0),
    ("noise.level", "ratio", 0.0, 1.0, 0.0),
    ("filter.cutoffHz", "Hz", 20.0, 20000.0, 1800.0),
    ("filter.resonance", "ratio", 0.0, 1.0, 0.2),
    ("filter.envAmount", "oct", -8.0, 8.0, 2.5),
    ("filter.keyTrack", "ratio", 0.0, 1.0, 0.3),
    ("filter.drive", "ratio", 0.0, 1.0, 0.2),
    ("filter.velAmount", "oct", 0.0, 4.0, 1.0),
    ("fenv.attackMs", "ms", 0.0, 5000.0, 2.0),
    ("fenv.decayMs", "ms", 0.0, 10000.0, 300.0),
    ("fenv.sustain", "ratio", 0.0, 1.0, 0.2),
    ("fenv.releaseMs", "ms", 0.0, 10000.0, 200.0),
    ("amp.attackMs", "ms", 0.0, 5000.0, 3.0),
    ("amp.decayMs", "ms", 0.0, 10000.0, 300.0),
    ("amp.sustain", "ratio", 0.0, 1.0, 0.7),
    ("amp.releaseMs", "ms", 0.0, 10000.0, 150.0),
    ("gainDb", "dB", -60.0, 6.0, -8.0),
];

const PW: usize = 0;
const O2_LEVEL: usize = 1;
const O2_SEMI: usize = 2;
const O2_DET: usize = 3;
const NOISE: usize = 4;
const CUTOFF: usize = 5;
const RES: usize = 6;
const ENV_AMT: usize = 7;
const KEYTRK: usize = 8;
const DRIVE: usize = 9;
const VEL_AMT: usize = 10;
const FA: usize = 11;
const FD: usize = 12;
const FS: usize = 13;
const FR: usize = 14;
const AA: usize = 15;
const AD: usize = 16;
const AS: usize = 17;
const AR: usize = 18;
const GAIN: usize = 19;

const WAVES: &[&str] = &["sine", "saw", "square", "tri", "noise"];

#[derive(Clone, Debug, PartialEq)]
pub struct SubParams {
    /// true = `synth.moogish` (ladder), false = `synth.subtractive` (SVF).
    pub ladder: bool,
    pub wave: Wave,
    pub wave2: Wave,
    pub mode: FilterMode,
    pub n: NumParams,
}

fn parse_wave(path: &str, v: &ParamValue) -> Result<Wave, String> {
    let ParamValue::Str(s) = v else { return Err(format!("{path} expects one of {WAVES:?}")) };
    Ok(match s.as_str() {
        "sine" => Wave::Sine,
        "saw" => Wave::Saw,
        "square" => Wave::Square,
        "tri" => Wave::Tri,
        "noise" => Wave::Noise,
        _ => return Err(format!("{path} {s:?} not in {WAVES:?}")),
    })
}

impl SubParams {
    pub fn new(ladder: bool) -> Self {
        let mut n = NumParams::new(SUB_TABLE);
        if ladder {
            n.v[CUTOFF] = 900.0;
            n.v[RES] = 0.35;
            n.v[ENV_AMT] = 3.0;
            n.v[DRIVE] = 0.4;
        }
        SubParams { ladder, wave: Wave::Saw, wave2: Wave::Saw, mode: FilterMode::Lp, n }
    }

    pub fn describe(ladder: bool) -> Vec<ParamMeta> {
        let mut v = vec![enum_meta("osc.wave", WAVES, "saw"), enum_meta("osc2.wave", WAVES, "saw")];
        if !ladder {
            v.push(enum_meta("filter.type", &["lp", "hp", "bp"], "lp"));
        }
        let mut nums = NumParams::metas(SUB_TABLE);
        if ladder {
            let d = SubParams::new(true);
            for (m, val) in nums.iter_mut().zip(d.n.v.iter()) {
                m.default = serde_json::json!(val);
            }
        }
        v.extend(nums);
        v
    }

    pub fn set(&mut self, path: &str, v: &ParamValue) -> Result<(), String> {
        match path {
            "osc.wave" => self.wave = parse_wave(path, v)?,
            "osc2.wave" => self.wave2 = parse_wave(path, v)?,
            "filter.type" if !self.ladder => {
                let ParamValue::Str(s) = v else { return Err("filter.type expects lp|hp|bp".into()) };
                self.mode = match s.as_str() {
                    "lp" => FilterMode::Lp,
                    "hp" => FilterMode::Hp,
                    "bp" => FilterMode::Bp,
                    _ => return Err(format!("filter.type {s:?} not in [lp, hp, bp]")),
                };
            }
            _ => {
                if !self.n.set(path, v)? {
                    let engine = if self.ladder { "synth.moogish" } else { "synth.subtractive" };
                    return Err(format!("unknown param {path:?} for {engine}"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct SubVoice {
    p: SubParams,
    sr: f64,
    seed: u64,
    rng: Rng,
    o1: Osc,
    o2: Osc,
    svf: Svf,
    ladder: Ladder,
    amp_env: Adsr,
    f_env: Adsr,
    amp: f64,
    pitch: u8,
    vel: f64,
    counter: u32,
}

impl SubVoice {
    pub fn new(p: SubParams, sample_rate: u32, seed: u64) -> Self {
        SubVoice {
            p,
            sr: sample_rate as f64,
            seed,
            rng: Rng::new(seed),
            o1: Osc::default(),
            o2: Osc::default(),
            svf: Svf::default(),
            ladder: Ladder::default(),
            amp_env: Adsr::default(),
            f_env: Adsr::default(),
            amp: 0.0,
            pitch: 60,
            vel: 0.0,
            counter: 0,
        }
    }
    fn ms(&self, i: usize) -> f64 {
        self.p.n.v[i] * self.sr / 1000.0
    }
    fn set_pitch(&mut self) {
        let v = &self.p.n.v;
        let f1 = midi_to_hz(self.pitch);
        let f2 = f1 * exp2((v[O2_SEMI] + v[O2_DET] / 100.0) / 12.0);
        self.o1.set_freq(f1, self.sr);
        self.o2.set_freq(f2, self.sr);
    }
    fn update_filter(&mut self, fenv: f64) {
        let v = &self.p.n.v;
        let oct = fenv * v[ENV_AMT]
            + v[KEYTRK] * (self.pitch as f64 - 60.0) / 12.0
            + v[VEL_AMT] * (self.vel - 0.8);
        let fc = v[CUTOFF] * exp2(oct);
        if self.p.ladder {
            self.ladder.set(fc, v[RES], self.sr);
        } else {
            self.svf.set(fc, v[RES], self.sr);
        }
    }
}

impl Voice for SubVoice {
    fn note_on(&mut self, pitch: u8, velocity: f32, _tick: u64) {
        self.pitch = pitch;
        // `velocity` already includes layer/track gains; keep a 0..1 copy for filter velocity.
        self.vel = (velocity as f64).clamp(0.0, 1.0);
        self.amp = velocity as f64 * db_to_gain(self.p.n.v[GAIN]);
        self.rng = Rng::new(self.seed);
        self.o1.phase = 0.0;
        self.o2.phase = 0.25;
        self.set_pitch();
        self.amp_env.trigger();
        self.f_env.trigger();
        self.counter = 0;
    }
    fn note_off(&mut self, _pitch: u8, _tick: u64) {
        let ar = self.ms(AR);
        let fr = self.ms(FR);
        self.amp_env.release(ar);
        self.f_env.release(fr);
    }
    fn set_param(&mut self, path: &str, value: &ParamValue) -> Result<(), String> {
        self.p.set(path, value)?;
        if path.starts_with("osc2.") {
            self.set_pitch();
        }
        Ok(())
    }
    fn render(&mut self, out: &mut [f32]) {
        let (aa, ad, ar_s) = (self.ms(AA), self.ms(AD), self.p.n.v[AS]);
        let (fa, fd, fs) = (self.ms(FA), self.ms(FD), self.p.n.v[FS]);
        for s in out.iter_mut() {
            if self.amp_env.is_idle() {
                return;
            }
            let fe = self.f_env.next(fa, fd, fs);
            if self.counter % 8 == 0 {
                self.update_filter(fe);
            }
            self.counter = self.counter.wrapping_add(1);
            let v = &self.p.n.v;
            let (pw, l2, nl, drive) = (v[PW], v[O2_LEVEL], v[NOISE], v[DRIVE]);
            let mut x = self.o1.next(self.p.wave, pw, &mut self.rng) * (1.0 - 0.5 * l2);
            if l2 > 0.0 {
                x += self.o2.next(self.p.wave2, pw, &mut self.rng) * l2;
            }
            if nl > 0.0 {
                x += self.rng.bipolar() * nl;
            }
            let y = if self.p.ladder {
                self.ladder.process(x, drive)
            } else {
                self.svf.process(crate::detmath::tanh_d(x * (1.0 + 2.0 * drive)), self.p.mode)
            };
            let e = self.amp_env.next(aa, ad, ar_s);
            *s += (y * e * self.amp) as f32;
            if self.amp_env.stage == 3 && ar_s <= 0.0 {
                self.amp_env.stage = 0;
            }
        }
    }
    fn reset(&mut self) {
        self.amp_env = Adsr::default();
        self.f_env = Adsr::default();
    }
    fn is_active(&self) -> bool {
        !self.amp_env.is_idle()
    }
    fn level(&self) -> f32 {
        (self.amp_env.level * self.amp) as f32
    }
    fn describe_params(&self) -> Vec<ParamMeta> {
        SubParams::describe(self.p.ladder)
    }
}

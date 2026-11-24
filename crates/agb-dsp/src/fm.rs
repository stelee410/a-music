//! `synth.fm`: four sine operators that modulate one another (SPEC §9.5).
//!
//! An operator is a sine oscillator with its own envelope, at a ratio of the note's frequency. In
//! an `algorithm` some operators are heard (carriers) and the others bend the phase or the
//! frequency of the one below them (modulators): the harder a modulator is driven, the brighter the
//! carrier. With every modulator's envelope shorter than its carrier's, a note starts bright and
//! mellows as it rings: electric pianos, bells, plucked basses, glassy pads.
//!
//! `modType` says what a modulator does to its carrier, and is never left unsaid:
//!
//! - `pm` — it is added to the carrier's phase. A modulator at `level` 1 moves the phase by up to
//!   [`INDEX`] radians. The sound keeps its brightness whatever the ratios (what most "FM" synths do).
//! - `fm` — it is added to the carrier's frequency: at `level` 1 the frequency swings by up to
//!   [`INDEX`] times the carrier's own. With modulator and carrier at the same ratio this is as bright
//!   as `pm`; a modulator at a higher ratio brightens less, one at a lower ratio more.
//!
//! Operator 4 can modulate itself (`feedback`); the amount is limited so that it stays a tone.
//! Original design; it does not read DX7 patches.

use crate::common::{enum_meta, NumParams, NumSpec};
use crate::detmath::{db_to_gain, decay_coef, exp2, midi_to_hz, sin_turns};
use crate::{ParamMeta, ParamValue, Voice};

/// Radians (in `pm`) by which a modulator at level 1 moves its carrier's phase.
pub const INDEX: f64 = 12.0;
/// Radians by which operator 4 at full `feedback` moves its own phase.
const FEEDBACK: f64 = 1.5;
const OPS: usize = 4;

pub const FM_TABLE: &[NumSpec] = &[
    ("feedback", "ratio", 0.0, 1.0, 0.0),
    // How much sooner high notes die away (and low notes ring longer): 1 halves the times every octave.
    ("keyScale", "ratio", 0.0, 1.0, 0.5),
    ("gainDb", "dB", -60.0, 6.0, -9.0),
    ("op1.ratio", "x", 0.125, 32.0, 1.0),
    ("op1.detuneCents", "cents", -100.0, 100.0, 0.0),
    ("op1.level", "ratio", 0.0, 1.0, 1.0),
    ("op1.attackMs", "ms", 0.0, 5000.0, 2.0),
    ("op1.decayMs", "ms", 1.0, 30000.0, 2600.0),
    ("op1.sustain", "ratio", 0.0, 1.0, 0.0),
    ("op1.releaseMs", "ms", 1.0, 10000.0, 160.0),
    ("op1.velocity", "ratio", 0.0, 1.0, 0.25),
    ("op2.ratio", "x", 0.125, 32.0, 1.0),
    ("op2.detuneCents", "cents", -100.0, 100.0, 0.0),
    ("op2.level", "ratio", 0.0, 1.0, 0.1),
    ("op2.attackMs", "ms", 0.0, 5000.0, 2.0),
    ("op2.decayMs", "ms", 1.0, 30000.0, 900.0),
    ("op2.sustain", "ratio", 0.0, 1.0, 0.0),
    ("op2.releaseMs", "ms", 1.0, 10000.0, 160.0),
    ("op2.velocity", "ratio", 0.0, 1.0, 0.85),
    ("op3.ratio", "x", 0.125, 32.0, 1.0),
    ("op3.detuneCents", "cents", -100.0, 100.0, 0.0),
    ("op3.level", "ratio", 0.0, 1.0, 0.55),
    ("op3.attackMs", "ms", 0.0, 5000.0, 2.0),
    ("op3.decayMs", "ms", 1.0, 30000.0, 2200.0),
    ("op3.sustain", "ratio", 0.0, 1.0, 0.0),
    ("op3.releaseMs", "ms", 1.0, 10000.0, 160.0),
    ("op3.velocity", "ratio", 0.0, 1.0, 0.35),
    ("op4.ratio", "x", 0.125, 32.0, 14.0),
    ("op4.detuneCents", "cents", -100.0, 100.0, 0.0),
    ("op4.level", "ratio", 0.0, 1.0, 0.05),
    ("op4.attackMs", "ms", 0.0, 5000.0, 1.0),
    ("op4.decayMs", "ms", 1.0, 30000.0, 140.0),
    ("op4.sustain", "ratio", 0.0, 1.0, 0.0),
    ("op4.releaseMs", "ms", 1.0, 10000.0, 80.0),
    ("op4.velocity", "ratio", 0.0, 1.0, 1.0),
];
const FEEDBACK_AT: usize = 0;
const KEY_SCALE: usize = 1;
const GAIN: usize = 2;
const FIRST_OP: usize = 3;
const PER_OP: usize = 8;
const RATIO: usize = 0;
const DETUNE: usize = 1;
const LEVEL: usize = 2;
const ATTACK: usize = 3;
const DECAY: usize = 4;
const SUSTAIN: usize = 5;
const RELEASE: usize = 6;
const VELOCITY: usize = 7;

const MOD_TYPES: &[&str] = &["pm", "fm"];
const ALGORITHMS: &[&str] = &["pairs", "stack", "branch", "fan", "organ"];

/// Which operator modulates which, and which are heard. Operators are numbered 1…4 in the
/// parameters and 0…3 here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    /// 2 → 1 and 4 → 3: two voices of one modulator and one carrier each.
    Pairs,
    /// 4 → 3 → 2 → 1: one carrier, as bright as it gets.
    Stack,
    /// (4 → 3) and 2, both → 1.
    Branch,
    /// 4 → 1, 2 and 3: one modulator, three carriers.
    Fan,
    /// No modulation: four sines (drawbars).
    Organ,
}

impl Algorithm {
    /// For each operator, the operators that modulate it.
    fn inputs(self) -> [&'static [usize]; OPS] {
        match self {
            Algorithm::Pairs => [&[1], &[], &[3], &[]],
            Algorithm::Stack => [&[1], &[2], &[3], &[]],
            Algorithm::Branch => [&[1, 2], &[], &[3], &[]],
            Algorithm::Fan => [&[3], &[3], &[3], &[]],
            Algorithm::Organ => [&[], &[], &[], &[]],
        }
    }

    fn carriers(self) -> &'static [usize] {
        match self {
            Algorithm::Pairs => &[0, 2],
            Algorithm::Stack | Algorithm::Branch => &[0],
            Algorithm::Fan => &[0, 1, 2],
            Algorithm::Organ => &[0, 1, 2, 3],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FmParams {
    /// `true`: modulators change the carrier's frequency (`fm`); `false`: its phase (`pm`).
    pub frequency: bool,
    pub algorithm: Algorithm,
    pub n: NumParams,
}

impl Default for FmParams {
    /// An electric piano: a mellow pair for the body, and a short high modulator for the tine.
    fn default() -> Self {
        FmParams { frequency: false, algorithm: Algorithm::Pairs, n: NumParams::new(FM_TABLE) }
    }
}

impl FmParams {
    pub fn describe() -> Vec<ParamMeta> {
        let mut v = vec![enum_meta("modType", MOD_TYPES, "pm"), enum_meta("algorithm", ALGORITHMS, "pairs")];
        v.extend(NumParams::metas(FM_TABLE));
        v
    }

    pub fn set(&mut self, path: &str, v: &ParamValue) -> Result<(), String> {
        match path {
            "modType" => {
                let ParamValue::Str(s) = v else { return Err(format!("modType expects one of {MOD_TYPES:?}")) };
                self.frequency = match s.as_str() {
                    "pm" => false,
                    "fm" => true,
                    _ => return Err(format!("modType {s:?} not in {MOD_TYPES:?}")),
                };
                Ok(())
            }
            "algorithm" => {
                let ParamValue::Str(s) = v else { return Err(format!("algorithm expects one of {ALGORITHMS:?}")) };
                self.algorithm = match s.as_str() {
                    "pairs" => Algorithm::Pairs,
                    "stack" => Algorithm::Stack,
                    "branch" => Algorithm::Branch,
                    "fan" => Algorithm::Fan,
                    "organ" => Algorithm::Organ,
                    _ => return Err(format!("algorithm {s:?} not in {ALGORITHMS:?}")),
                };
                Ok(())
            }
            _ if self.n.set(path, v)? => Ok(()),
            _ => Err(format!("unknown param {path:?} for synth.fm")),
        }
    }

    fn op(&self, k: usize, field: usize) -> f64 {
        self.n.v[FIRST_OP + k * PER_OP + field]
    }
}

/// One operator while a note sounds.
#[derive(Clone, Debug, Default)]
struct Op {
    /// Turns per sample.
    dt: f64,
    phase: f64,
    /// Output level: for a carrier its loudness, for a modulator radians.
    level: f64,
    /// 0 silent, 1 rising, 2 falling to the sustain level, 3 released.
    stage: u8,
    env: f64,
    attack_step: f64,
    decay: f64,
    sustain: f64,
    release: f64,
    /// What it puts out: sine × envelope × level.
    out: f64,
    /// Sine × envelope now and one sample ago, for the feedback (the mean of the two keeps it from
    /// ringing at half the sample rate).
    raw: f64,
    raw_before: f64,
}

impl Op {
    fn step_env(&mut self) -> f64 {
        match self.stage {
            1 => {
                self.env += self.attack_step;
                if self.env >= 1.0 {
                    self.env = 1.0;
                    self.stage = 2;
                }
            }
            2 => self.env = self.sustain + (self.env - self.sustain) * self.decay,
            3 => {
                self.env *= self.release;
                if self.env < 1e-5 {
                    self.env = 0.0;
                    self.stage = 0;
                }
            }
            _ => {}
        }
        self.env
    }
}

#[derive(Clone, Debug)]
pub struct FmVoice {
    p: FmParams,
    sr: f64,
    ops: [Op; OPS],
    amp: f64,
    feedback: f64,
    /// The note's own velocity, when the renderer has said it.
    velocity: Option<f32>,
    active: bool,
    counter: u32,
}

impl FmVoice {
    pub fn new(p: FmParams, sample_rate: u32, _seed: u64) -> Self {
        FmVoice { p, sr: sample_rate as f64, ops: Default::default(), amp: 0.0, feedback: 0.0, velocity: None, active: false, counter: 0 }
    }

    fn heard(&self) -> f64 {
        self.p.algorithm.carriers().iter().map(|&c| self.ops[c].env * self.ops[c].level).sum()
    }
}

impl Voice for FmVoice {
    fn note_on(&mut self, pitch: u8, amp: f32, _tick: u64) {
        let f0 = midi_to_hz(pitch);
        let velocity = self.velocity.unwrap_or(amp).clamp(0.0, 1.0) as f64;
        // times of high notes are shorter, of low notes longer (middle C as written)
        let times = exp2((60.0 - pitch as f64) / 12.0 * self.p.n.v[KEY_SCALE]).clamp(0.1, 8.0);
        let carriers = self.p.algorithm.carriers();
        for k in 0..OPS {
            let hz = f0 * self.p.op(k, RATIO) * exp2(self.p.op(k, DETUNE) / 1200.0);
            // a soft note drives an operator less, by as much as its `velocity` says
            let touch = 1.0 - self.p.op(k, VELOCITY) * (1.0 - velocity);
            let carrier = carriers.contains(&k);
            // an operator above what the sample rate can carry is left out
            let level = if hz >= self.sr * 0.45 { 0.0 } else { self.p.op(k, LEVEL) * touch * if carrier { 1.0 } else { INDEX } };
            let attack = self.p.op(k, ATTACK) * self.sr / 1000.0;
            self.ops[k] = Op {
                dt: hz / self.sr,
                phase: 0.0,
                level,
                stage: 1,
                env: 0.0,
                attack_step: if attack < 1.0 { 1.0 } else { 1.0 / attack },
                decay: decay_coef(self.p.op(k, DECAY) * times, self.sr),
                sustain: self.p.op(k, SUSTAIN),
                release: decay_coef(self.p.op(k, RELEASE) * times.min(2.0), self.sr),
                out: 0.0,
                raw: 0.0,
                raw_before: 0.0,
            };
        }
        // several carriers at full level are no louder than one
        self.amp = amp as f64 * db_to_gain(self.p.n.v[GAIN]) / (carriers.len() as f64).sqrt();
        self.feedback = self.p.n.v[FEEDBACK_AT] * FEEDBACK;
        self.active = true;
        self.counter = 0;
    }

    fn note_off(&mut self, _pitch: u8, _tick: u64) {
        for o in self.ops.iter_mut() {
            if o.stage != 0 {
                o.stage = 3;
            }
        }
    }

    fn set_param(&mut self, path: &str, value: &ParamValue) -> Result<(), String> {
        self.p.set(path, value)
    }

    fn render(&mut self, out: &mut [f32]) {
        if !self.active {
            return;
        }
        const TURN: f64 = 1.0 / (2.0 * std::f64::consts::PI);
        let inputs = self.p.algorithm.inputs();
        let carriers = self.p.algorithm.carriers();
        let frequency = self.p.frequency;
        for s in out.iter_mut() {
            // from the top down: a modulator is always numbered higher than what it modulates
            for k in (0..OPS).rev() {
                let mut bend = 0.0; // radians
                for &m in inputs[k] {
                    bend += self.ops[m].out;
                }
                let o = &mut self.ops[k];
                let own = if k == OPS - 1 { self.feedback * 0.5 * (o.raw + o.raw_before) } else { 0.0 };
                let e = o.step_env();
                let y = if frequency {
                    let y = sin_turns(o.phase + own * TURN);
                    o.phase += o.dt * (1.0 + bend);
                    y
                } else {
                    let y = sin_turns(o.phase + (bend + own) * TURN);
                    o.phase += o.dt;
                    y
                };
                o.phase -= o.phase.floor();
                o.raw_before = o.raw;
                o.raw = y * e;
                o.out = o.raw * o.level;
            }
            let mut x = 0.0;
            for &c in carriers {
                x += self.ops[c].out;
            }
            *s += (x * self.amp) as f32;
            self.counter = self.counter.wrapping_add(1);
            if self.counter % 64 == 0 {
                // over when nothing heard is left: released, or decayed away with no sustain
                let ringing = carriers.iter().any(|&c| {
                    let o = &self.ops[c];
                    o.stage == 1 || (o.stage != 0 && o.env * o.level > 1e-4)
                });
                if !ringing {
                    self.active = false;
                    return;
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
        (self.heard() * self.amp) as f32
    }

    fn describe_params(&self) -> Vec<ParamMeta> {
        FmParams::describe()
    }

    fn set_velocity(&mut self, velocity: f32) {
        self.velocity = Some(velocity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(set: &[(&str, ParamValue)]) -> FmParams {
        let mut p = FmParams::default();
        for (k, v) in set {
            p.set(k, v).unwrap();
        }
        p
    }

    fn play(p: FmParams, pitch: u8, velocity: f32, held: usize, total: usize) -> (Vec<f32>, bool) {
        let mut v = FmVoice::new(p, 48000, 1);
        v.set_velocity(velocity);
        v.note_on(pitch, velocity, 0);
        let mut b = vec![0f32; total];
        v.render(&mut b[..held]);
        v.note_off(pitch, 0);
        v.render(&mut b[held..]);
        (b, v.is_active())
    }

    /// Goertzel magnitude at `hz` over `buf`.
    fn at(buf: &[f32], hz: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * hz / 48000.0;
        let (mut s1, mut s2) = (0.0, 0.0);
        for &x in buf {
            let s0 = x as f64 + 2.0 * w.cos() * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2).max(0.0).sqrt() / buf.len() as f64
    }

    #[test]
    fn a_note_sounds_at_its_pitch_and_ends() {
        let (b, active) = play(FmParams::default(), 69, 0.8, 24000, 96000);
        assert!(!active, "released: over within a second and a half");
        assert!(b.iter().all(|x| x.is_finite() && x.abs() < 1.0));
        let peak = b.iter().fold(0f32, |a, x| a.max(x.abs()));
        assert!(peak > 0.05, "peak {peak}");
        let seg = &b[2400..21600];
        assert!(at(seg, 440.0) > 5.0 * at(seg, 466.2), "A4, not B flat");
        assert!(at(seg, 880.0) > 0.01 * at(seg, 440.0), "the modulator adds harmonics");
        // with no sustain a note that is held ends by itself too
        let (_, active) = play(FmParams::default(), 96, 0.8, 48000 * 12, 48000 * 12);
        assert!(!active, "a held note decays away");
    }

    #[test]
    fn a_harder_touch_is_brighter() {
        let bright = |velocity: f32| {
            let (b, _) = play(FmParams::default(), 57, velocity, 12000, 12000);
            let seg = &b[1200..12000];
            (at(seg, 440.0) + at(seg, 660.0) + at(seg, 880.0)) / at(seg, 220.0)
        };
        assert!(bright(1.0) > 1.5 * bright(0.2), "{} against {}", bright(1.0), bright(0.2));
    }

    #[test]
    fn organ_has_no_sidebands_and_stack_has_many() {
        let level = |k: usize, v: f64| (["op1.level", "op2.level", "op3.level", "op4.level"][k], ParamValue::Num(v));
        let sustain = |k: usize| (["op1.sustain", "op2.sustain", "op3.sustain", "op4.sustain"][k], ParamValue::Num(1.0));
        let ratio4 = ("op4.ratio", ParamValue::Num(1.0));
        let organ = params(&[("algorithm", ParamValue::Str("organ".into())), level(0, 1.0), level(1, 0.0), level(2, 0.0), level(3, 0.0), sustain(0)]);
        let (b, _) = play(organ, 57, 1.0, 24000, 24000);
        assert!(at(&b[2400..], 440.0) < 0.01 * at(&b[2400..], 220.0), "a sine");
        let stack = params(&[
            ("algorithm", ParamValue::Str("stack".into())),
            level(0, 1.0), level(1, 0.3), level(2, 0.3), level(3, 0.3),
            sustain(0), sustain(1), sustain(2), sustain(3), ratio4.clone(),
        ]);
        let (b, _) = play(stack, 57, 1.0, 24000, 24000);
        let upper: f64 = (2..=8).map(|h| at(&b[2400..], 220.0 * h as f64)).sum();
        assert!(upper > 0.5 * at(&b[2400..], 220.0), "the harmonics above are there: {upper}");
    }

    #[test]
    fn fm_and_pm_are_told_apart() {
        // a modulator at three times its carrier's frequency: in `fm` it brightens a third as much
        let set = |kind: &str| {
            params(&[
                ("modType", ParamValue::Str(kind.into())),
                ("algorithm", ParamValue::Str("stack".into())),
                ("op2.ratio", ParamValue::Num(3.0)),
                ("op2.level", ParamValue::Num(0.1)),
                ("op2.sustain", ParamValue::Num(1.0)),
                ("op1.sustain", ParamValue::Num(1.0)),
                ("op3.level", ParamValue::Num(0.0)),
                ("op4.level", ParamValue::Num(0.0)),
            ])
        };
        let side = |kind: &str| {
            let (b, _) = play(set(kind), 57, 1.0, 24000, 24000);
            at(&b[2400..], 880.0) / at(&b[2400..], 220.0)
        };
        let (pm, fm) = (side("pm"), side("fm"));
        assert!(pm > 2.0 * fm && fm > 0.05, "pm {pm}, fm {fm}");
        assert!(FmParams::default().set("modType", &ParamValue::Str("both".into())).is_err());
        assert!(FmParams::describe().iter().any(|m| m.path == "modType"));
    }

    #[test]
    fn feedback_stays_a_tone() {
        let p = params(&[
            ("algorithm", ParamValue::Str("organ".into())),
            ("feedback", ParamValue::Num(1.0)),
            ("op1.level", ParamValue::Num(0.0)),
            ("op2.level", ParamValue::Num(0.0)),
            ("op3.level", ParamValue::Num(0.0)),
            ("op4.level", ParamValue::Num(1.0)),
            ("op4.ratio", ParamValue::Num(1.0)),
            ("op4.sustain", ParamValue::Num(1.0)),
        ]);
        let (b, _) = play(p, 57, 1.0, 24000, 24000);
        let seg = &b[2400..];
        assert!(seg.iter().all(|x| x.is_finite() && x.abs() < 1.0));
        assert!(at(seg, 220.0) > 3.0 * at(seg, 233.1), "still at its pitch");
        assert!(at(seg, 440.0) > 0.05 * at(seg, 220.0), "a saw-like tone: the octave is there");
    }
}

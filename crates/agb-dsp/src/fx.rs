//! Track effects (FORMAT.md §9): what a whole track is played through.
//!
//! Two are implemented: `amp`, a guitar amplifier and its speaker cabinet, and `reverb`, a room.
//! Both are deterministic: only `+ - * /`, `sqrt` and the functions of [`crate::detmath`].
//!
//! The amplifier is the reason this module exists. A distorted guitar is not the sum of distorted
//! notes: the notes of a chord are added *before* they are clipped, and what the clipping makes of
//! their sum (difference tones, the way a chord "growls") is most of the sound. So the distortion
//! cannot be baked into samples; it has to be applied to the track.

use crate::detmath::{exp2, sin_turns, tanh_d};
use serde_json::Value;

/// The effect types this build renders.
pub const FX_TYPES: &[&str] = &["amp", "reverb"];

pub trait Fx {
    /// Process a mono buffer in place.
    fn process(&mut self, buf: &mut [f32]);
}

/// An effect from its `{ "type", "params", "bypass" }` description, or `None` when it is bypassed or
/// not one of [`FX_TYPES`].
pub fn build(spec: &Value, sample_rate: f64) -> Option<Box<dyn Fx>> {
    if spec.get("bypass").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let params = spec.get("params").cloned().unwrap_or(Value::Null);
    match spec.get("type").and_then(Value::as_str)? {
        "amp" => Some(Box::new(Amp::new(&params, sample_rate))),
        "reverb" => Some(Box::new(Reverb::new(&params, sample_rate))),
        _ => None,
    }
}

fn num(p: &Value, key: &str, default: f64, lo: f64, hi: f64) -> f64 {
    p.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(default).clamp(lo, hi)
}

fn db(x: f64) -> f64 {
    exp2(x / 20.0 * std::f64::consts::LOG2_10)
}

// ---------------------------------------------------------------- biquad (RBJ cookbook)

#[derive(Clone, Copy, Debug, Default)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn norm(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Biquad { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, z1: 0.0, z2: 0.0 }
    }
    fn trig(freq: f64, sr: f64) -> (f64, f64) {
        let t = (freq / sr).clamp(1e-5, 0.49);
        (sin_turns(t), sin_turns(t + 0.25))
    }
    fn lowpass(freq: f64, q: f64, sr: f64) -> Self {
        let (s, c) = Self::trig(freq, sr);
        let al = s / (2.0 * q);
        Self::norm((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + al, -2.0 * c, 1.0 - al)
    }
    fn highpass(freq: f64, q: f64, sr: f64) -> Self {
        let (s, c) = Self::trig(freq, sr);
        let al = s / (2.0 * q);
        Self::norm((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + al, -2.0 * c, 1.0 - al)
    }
    fn peak(freq: f64, q: f64, gain_db: f64, sr: f64) -> Self {
        let (s, c) = Self::trig(freq, sr);
        let a = db(gain_db / 2.0);
        let al = s / (2.0 * q);
        Self::norm(1.0 + al * a, -2.0 * c, 1.0 - al * a, 1.0 + al / a, -2.0 * c, 1.0 - al / a)
    }
    fn shelf(freq: f64, gain_db: f64, sr: f64, high: bool) -> Self {
        let (s, c) = Self::trig(freq, sr);
        let a = db(gain_db / 2.0);
        let al = s / 2.0 * std::f64::consts::SQRT_2;
        let r = 2.0 * a.sqrt() * al;
        if high {
            Self::norm(
                a * ((a + 1.0) + (a - 1.0) * c + r),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
                a * ((a + 1.0) + (a - 1.0) * c - r),
                (a + 1.0) - (a - 1.0) * c + r,
                2.0 * ((a - 1.0) - (a + 1.0) * c),
                (a + 1.0) - (a - 1.0) * c - r,
            )
        } else {
            Self::norm(
                a * ((a + 1.0) - (a - 1.0) * c + r),
                2.0 * a * ((a - 1.0) - (a + 1.0) * c),
                a * ((a + 1.0) - (a - 1.0) * c - r),
                (a + 1.0) + (a - 1.0) * c + r,
                -2.0 * ((a - 1.0) + (a + 1.0) * c),
                (a + 1.0) + (a - 1.0) * c - r,
            )
        }
    }
    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

// ---------------------------------------------------------------- amp

const OS: usize = 4; // the clipping runs at four times the sample rate, so that what it adds above
const TAPS: usize = 48; // half the sample rate is filtered out instead of folding back as hiss

/// A guitar amplifier and speaker cabinet.
///
/// `gain` 0…1 (clean … high gain), `bass` / `mid` / `treble` −1…1, `presence` 0…1,
/// `cabinet` "combo" | "stack" | "none", `levelDb`.
pub struct Amp {
    // into the preamp
    tight: Biquad,
    push: Biquad,
    pre: f64,
    bias: f64,
    stage2: f64,
    // oversampling
    fir: [f64; TAPS],
    up_hist: [f64; TAPS / OS],
    up_pos: usize,
    down_hist: [f64; TAPS],
    down_pos: usize,
    inter: f64,
    inter_coef: f64,
    // after the preamp
    block: Biquad,
    tone: [Biquad; 3],
    power: f64,
    cab: Vec<Biquad>,
    level: f64,
}

impl Amp {
    pub fn new(p: &Value, sr: f64) -> Self {
        let gain = num(p, "gain", 0.3, 0.0, 1.0);
        let bass = num(p, "bass", 0.0, -1.0, 1.0);
        let mid = num(p, "mid", 0.0, -1.0, 1.0);
        let treble = num(p, "treble", 0.0, -1.0, 1.0);
        let presence = num(p, "presence", 0.5, 0.0, 1.0);
        let cabinet = p.get("cabinet").and_then(Value::as_str).unwrap_or("combo");

        // Windowed-sinc low-pass at the original Nyquist, for both directions of the oversampling.
        let mut fir = [0.0; TAPS];
        let centre = (TAPS as f64 - 1.0) / 2.0;
        let mut sum = 0.0;
        for (i, h) in fir.iter_mut().enumerate() {
            let x = (i as f64 - centre) / OS as f64 * 0.92;
            let sinc = if x.abs() < 1e-9 { 1.0 } else { sin_turns(x / 2.0) / (std::f64::consts::PI * x) };
            let w = 0.5 - 0.5 * sin_turns(i as f64 / (TAPS as f64 - 1.0) + 0.25); // Hann
            *h = sinc * w;
            sum += *h;
        }
        for h in fir.iter_mut() {
            *h /= sum;
        }

        let cab = match cabinet {
            "none" => vec![],
            // a closed 4×12: a lower thump, darker on top
            "stack" => vec![
                Biquad::highpass(78.0, 0.95, sr),
                Biquad::peak(105.0, 1.2, 4.5, sr),
                Biquad::peak(430.0, 1.1, -5.0, sr),
                Biquad::peak(1250.0, 1.6, -2.0, sr),
                Biquad::peak(2300.0, 1.5, 5.0 + 3.0 * presence, sr),
                Biquad::peak(3700.0, 2.2, 2.0 + 4.0 * presence, sr),
                Biquad::lowpass(4700.0, 0.85, sr),
                Biquad::lowpass(5200.0, 0.7, sr),
                Biquad::lowpass(9000.0, 0.7, sr),
            ],
            // an open-backed 1×12 or 2×12: lighter in the bass, more air
            _ => vec![
                Biquad::highpass(92.0, 0.8, sr),
                Biquad::peak(130.0, 1.0, 3.0, sr),
                Biquad::peak(480.0, 1.0, -3.5, sr),
                Biquad::peak(2600.0, 1.3, 4.0 + 3.0 * presence, sr),
                Biquad::peak(4200.0, 2.0, 1.5 + 4.0 * presence, sr),
                Biquad::lowpass(5600.0, 0.8, sr),
                Biquad::lowpass(6400.0, 0.7, sr),
                Biquad::lowpass(11000.0, 0.7, sr),
            ],
        };
        Amp {
            // more gain needs less bass going in, or the low strings turn to mud
            tight: Biquad::highpass(70.0 + 110.0 * gain, 0.7, sr),
            push: Biquad::peak(780.0, 0.7, 5.0 * gain, sr),
            pre: db(4.0 + 40.0 * gain),
            bias: 0.12 + 0.1 * gain,
            stage2: 1.0 + 7.0 * gain,
            fir,
            up_hist: [0.0; TAPS / OS],
            up_pos: 0,
            down_hist: [0.0; TAPS],
            down_pos: 0,
            inter: 0.0,
            inter_coef: 1.0 - exp2(-(6500.0 / (sr * OS as f64)) * std::f64::consts::TAU * std::f64::consts::LOG2_E),
            block: Biquad::highpass(25.0, 0.7, sr),
            tone: [
                Biquad::shelf(140.0, 9.0 * bass, sr, false),
                Biquad::peak(650.0, 0.7, 9.0 * mid, sr),
                Biquad::shelf(3200.0, 9.0 * treble, sr, true),
            ],
            power: 1.0 + 1.5 * gain,
            cab,
            // the more it is driven the louder it gets: take that back, so that `gain` is a tone control
            level: db(num(p, "levelDb", 0.0, -40.0, 20.0) - 3.0 - 9.0 * gain),
        }
    }

    #[inline]
    fn clip(&mut self, x: f64) -> f64 {
        // first stage: a valve, which clips one side before the other
        let a = tanh_d(x * self.pre + self.bias) - tanh_d(self.bias);
        // what couples it to the next stage loses the top
        self.inter += self.inter_coef * (a - self.inter);
        // second stage, symmetric
        tanh_d(self.inter * self.stage2)
    }
}

impl Fx for Amp {
    fn process(&mut self, buf: &mut [f32]) {
        let n_up = TAPS / OS;
        for o in buf.iter_mut() {
            let mut x = self.push.run(self.tight.run(*o as f64));
            if !x.is_finite() {
                x = 0.0;
            }
            // up by four
            self.up_hist[self.up_pos] = x;
            let mut y = 0.0;
            for phase in 0..OS {
                let mut u = 0.0;
                for k in 0..n_up {
                    u += self.fir[k * OS + phase] * self.up_hist[(self.up_pos + n_up - k) % n_up];
                }
                let c = self.clip(u * OS as f64);
                // down by four: filter, keep the last of every four
                self.down_hist[self.down_pos] = c;
                if phase == OS - 1 {
                    for j in 0..TAPS {
                        y += self.fir[j] * self.down_hist[(self.down_pos + TAPS - j) % TAPS];
                    }
                }
                self.down_pos = (self.down_pos + 1) % TAPS;
            }
            self.up_pos = (self.up_pos + 1) % n_up;

            y = self.block.run(y);
            for t in self.tone.iter_mut() {
                y = t.run(y);
            }
            y = tanh_d(y * self.power) / self.power; // the power valves give a little too
            for c in self.cab.iter_mut() {
                y = c.run(y);
            }
            *o = (y * self.level) as f32;
        }
    }
}

// ---------------------------------------------------------------- reverb

/// A room: eight damped comb filters in parallel into four all-pass filters (after Schroeder and
/// Moorer; the delay lengths are the well-known Freeverb set). `size` 0…1, `decaySec`, `damping`
/// 0…1, `mix` 0…1.
pub struct Reverb {
    combs: Vec<(Vec<f64>, usize, f64, f64)>, // line, position, feedback, low-pass state
    allpasses: Vec<(Vec<f64>, usize)>,
    damp: f64,
    mix: f64,
    cut: Biquad,
}

impl Reverb {
    pub fn new(p: &Value, sr: f64) -> Self {
        let size = num(p, "size", 0.5, 0.0, 1.0);
        let decay = num(p, "decaySec", 1.2, 0.1, 20.0);
        let scale = sr / 44100.0 * (0.6 + 0.9 * size);
        let combs = [1116.0, 1188.0, 1277.0, 1356.0, 1422.0, 1491.0, 1557.0, 1617.0]
            .iter()
            .map(|len: &f64| {
                let n = ((len * scale) as usize).max(8);
                // fall by 60 dB in `decay` seconds
                let feedback = exp2(-3.0 * std::f64::consts::LOG2_10 * (n as f64 / sr) / decay);
                (vec![0.0; n], 0, feedback, 0.0)
            })
            .collect();
        let allpasses = [556.0, 441.0, 341.0, 225.0].iter().map(|len: &f64| (vec![0.0; ((len * scale) as usize).max(4)], 0)).collect();
        Reverb {
            combs,
            allpasses,
            damp: num(p, "damping", 0.5, 0.0, 1.0) * 0.8,
            mix: num(p, "mix", 0.2, 0.0, 1.0),
            cut: Biquad::highpass(160.0, 0.7, sr), // no rumble in the room
        }
    }
}

impl Fx for Reverb {
    fn process(&mut self, buf: &mut [f32]) {
        for o in buf.iter_mut() {
            let dry = *o as f64;
            let x = self.cut.run(if dry.is_finite() { dry } else { 0.0 }) * 0.06;
            let mut wet = 0.0;
            for (line, pos, feedback, lp) in self.combs.iter_mut() {
                let y = line[*pos];
                *lp = y * (1.0 - self.damp) + *lp * self.damp;
                line[*pos] = x + *lp * *feedback;
                *pos = (*pos + 1) % line.len();
                wet += y;
            }
            for (line, pos) in self.allpasses.iter_mut() {
                let b = line[*pos];
                let y = b - wet;
                line[*pos] = wet + b * 0.5;
                *pos = (*pos + 1) % line.len();
                wet = y;
            }
            *o = (dry * (1.0 - self.mix * 0.35) + wet * self.mix * 2.2) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sine(freq: f64, amp: f64, n: usize) -> Vec<f32> {
        (0..n).map(|i| (amp * sin_turns(freq * i as f64 / 48000.0)) as f32).collect()
    }
    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
    }
    /// Level of one frequency in a buffer (Goertzel-like correlation).
    fn level_at(x: &[f32], freq: f64) -> f64 {
        let (mut s, mut c) = (0.0, 0.0);
        for (i, v) in x.iter().enumerate() {
            let t = freq * i as f64 / 48000.0;
            s += *v as f64 * sin_turns(t);
            c += *v as f64 * sin_turns(t + 0.25);
        }
        (s * s + c * c).sqrt() * 2.0 / x.len() as f64
    }

    #[test]
    fn unknown_and_bypassed_effects_are_not_built() {
        assert!(build(&json!({"type": "chorus"}), 48000.0).is_none());
        assert!(build(&json!({"type": "amp", "bypass": true}), 48000.0).is_none());
        assert!(build(&json!({"type": "amp"}), 48000.0).is_some());
    }

    /// The point of the amplifier: two notes through it make tones that neither note has.
    #[test]
    fn amp_makes_difference_tones_of_a_chord() {
        let n = 48000;
        let (a, b) = (sine(220.0, 0.25, n), sine(330.0, 0.25, n)); // a fifth
        let mut chord: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
        let before = level_at(&chord[n / 2..], 110.0);
        Amp::new(&json!({"gain": 0.8, "cabinet": "none"}), 48000.0).process(&mut chord);
        let after = level_at(&chord[n / 2..], 110.0); // 330 − 220
        assert!(before < 1e-4, "{before}");
        assert!(after > 0.01, "difference tone at 110 Hz: {after}");
        assert!(chord.iter().all(|v| v.is_finite() && v.abs() < 4.0));
    }

    /// Driven hard, a quiet note and a loud one come out nearly as loud: the amp compresses.
    #[test]
    fn amp_compresses_when_driven() {
        let run = |amp: f64| {
            let mut x = sine(196.0, amp, 24000);
            Amp::new(&json!({"gain": 0.9}), 48000.0).process(&mut x);
            rms(&x[12000..])
        };
        let (quiet, loud) = (run(0.05), run(0.5));
        assert!(loud / quiet < 2.5, "20 dB in, {:.1} dB out", 20.0 * (loud / quiet).log10());
    }

    /// The cabinet takes the top off: clipping a low note makes harmonics far up, a speaker does not pass them.
    #[test]
    fn cabinet_removes_the_fizz() {
        let run = |cab: &str| {
            let mut x = sine(110.0, 0.4, 48000);
            Amp::new(&json!({"gain": 0.9, "cabinet": cab}), 48000.0).process(&mut x);
            level_at(&x[24000..], 110.0 * 81.0) / level_at(&x[24000..], 110.0 * 3.0) // 8910 Hz against 330 Hz
        };
        assert!(run("stack") < run("none") * 0.2, "stack {} none {}", run("stack"), run("none"));
    }

    #[test]
    fn reverb_rings_after_the_sound_stops_and_dies_away() {
        let mut x = sine(440.0, 0.5, 4800);
        x.extend(vec![0.0; 48000 * 3]);
        Reverb::new(&json!({"decaySec": 1.0, "mix": 0.5}), 48000.0).process(&mut x);
        let soon = rms(&x[9600..14400]);
        let late = rms(&x[48000 * 2 + 4800..48000 * 2 + 9600]);
        assert!(soon > 1e-4, "{soon}");
        assert!(late < soon * 0.02, "soon {soon} late {late}");
    }

    #[test]
    fn deterministic() {
        let run = || {
            let mut x = sine(147.0, 0.3, 9600);
            build(&json!({"type": "amp", "params": {"gain": 0.7}}), 48000.0).unwrap().process(&mut x);
            build(&json!({"type": "reverb", "params": {}}), 48000.0).unwrap().process(&mut x);
            x
        };
        assert_eq!(run(), run());
    }
}

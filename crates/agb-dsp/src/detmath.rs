//! Deterministic math.
//!
//! Platform `libm` implementations of `sin`/`exp`/`powf` may differ in the last
//! ulp between operating systems, which would break golden-hash comparisons
//! (SPEC §10.2). These functions use only IEEE-754 `+ - * /`, `floor` and bit
//! manipulation, which are exactly specified, so results are identical on
//! every target (Rust never auto-fuses multiply-add).

const TAU: f64 = std::f64::consts::TAU;
const LN2: f64 = std::f64::consts::LN_2;
const LOG2_10: f64 = std::f64::consts::LOG2_10;

/// `sin(2π·t)` for any finite `t` (t measured in turns).
pub fn sin_turns(t: f64) -> f64 {
    let t = t - t.floor(); // [0, 1)
    // Fold to u ∈ [-0.25, 0.25] using sin(2π(½−t)) = sin(2πt) and periodicity.
    let u = if t < 0.25 {
        t
    } else if t < 0.75 {
        0.5 - t
    } else {
        t - 1.0
    };
    let x = TAU * u; // [-π/2, π/2]
    let x2 = x * x;
    // Taylor series to x^15 (truncation error < 1e-10 on [-π/2, π/2]).
    let p = 1.0
        - x2 / 6.0
            * (1.0
                - x2 / 20.0
                    * (1.0
                        - x2 / 42.0
                            * (1.0
                                - x2 / 72.0
                                    * (1.0 - x2 / 110.0 * (1.0 - x2 / 156.0 * (1.0 - x2 / 210.0))))));
    x * p
}

/// `2^x`.
pub fn exp2(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x >= 1024.0 {
        return f64::INFINITY;
    }
    if x < -1022.0 {
        return 0.0;
    }
    let n = x.floor();
    let f = x - n; // [0, 1)
    let y = f * LN2; // [0, ln2)
    // e^y Taylor to y^13: error < 1e-15 on [0, ln2).
    let mut term = 1.0;
    let mut sum = 1.0;
    let mut k = 1.0;
    while k <= 13.0 {
        term = term * y / k;
        sum += term;
        k += 1.0;
    }
    let bits = ((n as i64 + 1023) as u64) << 52;
    sum * f64::from_bits(bits)
}

/// Decibels → linear amplitude.
pub fn db_to_gain(db: f64) -> f64 {
    exp2(db / 20.0 * LOG2_10)
}

/// Linear amplitude → dBFS (uses std `log10`; only for reporting, never for PCM).
pub fn gain_to_db(g: f64) -> Option<f64> {
    if g > 0.0 {
        Some(20.0 * g.log10())
    } else {
        None
    }
}

/// MIDI note → Hz (A4 = 440).
pub fn midi_to_hz(m: u8) -> f64 {
    440.0 * exp2((m as f64 - 69.0) / 12.0)
}

/// xorshift64* PRNG — tiny, seedable, deterministic.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        // splitmix64 scramble so small seeds are well distributed; never zero.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Rng(if z == 0 { 1 } else { z })
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform in [-1, 1).
    pub fn bipolar(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64) / ((1u64 << 52) as f64) - 1.0
    }
}

/// FNV-1a 64 — stable string hash for seeding.
pub fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_accuracy() {
        for i in 0..10_000 {
            let t = i as f64 / 10_000.0 * 3.0 - 1.0;
            let err = (sin_turns(t) - (TAU * t).sin()).abs();
            assert!(err < 1e-9, "t={t} err={err}");
        }
    }

    #[test]
    fn exp2_accuracy() {
        for i in 0..10_000 {
            let x = i as f64 / 100.0 - 50.0;
            let rel = (exp2(x) - x.exp2()).abs() / x.exp2();
            assert!(rel < 1e-13, "x={x} rel={rel}");
        }
        assert_eq!(exp2(0.0), 1.0);
        assert_eq!(exp2(3.0), 8.0);
    }

    #[test]
    fn db_and_pitch() {
        assert!((db_to_gain(-6.0) - 0.501187).abs() < 1e-6);
        assert!((db_to_gain(0.0) - 1.0).abs() < 1e-15);
        assert!((midi_to_hz(69) - 440.0).abs() < 1e-9);
        assert!((midi_to_hz(81) - 880.0).abs() < 1e-9);
    }

    #[test]
    fn rng_deterministic() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..100 {
            let v = a.bipolar();
            assert_eq!(v, b.bipolar());
            assert!((-1.0..1.0).contains(&v));
        }
        assert_ne!(Rng::new(7).next_u64(), Rng::new(8).next_u64());
    }
}

/// `tan(2π·t)` for t in (-0.25, 0.25).
pub fn tan_turns(t: f64) -> f64 {
    sin_turns(t) / sin_turns(t + 0.25)
}

/// Deterministic soft saturator (Padé tanh approximation, exact ±1 beyond |x|=3).
pub fn tanh_d(x: f64) -> f64 {
    if x >= 3.0 {
        1.0
    } else if x <= -3.0 {
        -1.0
    } else {
        let x2 = x * x;
        x * (27.0 + x2) / (27.0 + 9.0 * x2)
    }
}

/// Per-sample multiplier giving −60 dB after `ms` milliseconds.
pub fn decay_coef(ms: f64, sample_rate: f64) -> f64 {
    let n = (ms * sample_rate / 1000.0).max(1.0);
    exp2(-9.965_784_284_662_087 / n) // log2(1000)
}

#[cfg(test)]
mod tests_ext {
    use super::*;

    #[test]
    fn tan_and_tanh() {
        for i in 1..240 {
            let t = i as f64 / 1000.0;
            let want = (std::f64::consts::TAU * t).tan();
            assert!(((tan_turns(t) - want) / want).abs() < 1e-8, "t={t}");
        }
        for i in -400..400 {
            let x = i as f64 / 100.0;
            assert!((tanh_d(x) - x.tanh()).abs() < 0.03, "x={x}");
        }
        let c = decay_coef(1000.0, 48000.0);
        let mut v = 1.0;
        for _ in 0..48000 {
            v *= c;
        }
        assert!((v - 0.001).abs() < 1e-6);
    }
}

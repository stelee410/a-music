//! Time model (SPEC §5.2) — pure functions.

pub fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

pub fn lcm(a: u64, b: u64) -> u64 {
    if a == 0 || b == 0 {
        return 0;
    }
    a / gcd(a, b) * b
}

/// LCM of all values (1 for an empty list).
pub fn lcm_all(values: impl IntoIterator<Item = u64>) -> u64 {
    values.into_iter().fold(1, lcm)
}

/// `ppqn / stepsPerQuarter`; `None` if not an exact division.
pub fn ticks_per_step(ppqn: u32, steps_per_quarter: u32) -> Option<u64> {
    if steps_per_quarter == 0 || ppqn == 0 || ppqn % steps_per_quarter != 0 {
        return None;
    }
    Some((ppqn / steps_per_quarter) as u64)
}

/// `60 / (bpm * ppqn)`
pub fn sec_per_tick(bpm: f64, ppqn: u32) -> f64 {
    60.0 / (bpm * ppqn as f64)
}

/// `round(originSample + tick * sampleRate * secPerTick)` (round half away from zero).
pub fn tick_to_sample(tick: u64, bpm: f64, ppqn: u32, sample_rate: u32, origin_sample: u64) -> u64 {
    let s = origin_sample as f64 + tick as f64 * sample_rate as f64 * sec_per_tick(bpm, ppqn);
    s.round() as u64
}

/// One 4/4 bar in ticks (`stepsPerQuarter * 4` steps).
pub fn bar_ticks(ppqn: u32) -> u64 {
    ppqn as u64 * 4
}

/// Smallest `origin + k*period >= t`.
pub fn ceil_to_grid(t: u64, origin: u64, period: u64) -> u64 {
    if t <= origin {
        return origin;
    }
    let d = t - origin;
    origin + d.div_ceil(period) * period
}

/// Piecewise-constant tempo: maps ticks ↔ samples across tempo changes.
/// With no changes it reproduces [`tick_to_sample`] bit-for-bit.
#[derive(Clone, Debug, PartialEq)]
pub struct TempoMap {
    ppqn: u32,
    sample_rate: u32,
    /// (start tick, start sample (unrounded), bpm)
    segs: Vec<(u64, f64, f64)>,
}

impl TempoMap {
    pub fn new(base_bpm: f64, points: &[(u64, f64)], ppqn: u32, sample_rate: u32) -> Self {
        let mut pts = points.to_vec();
        pts.sort_by_key(|p| p.0);
        let mut segs = vec![(0u64, 0.0f64, base_bpm)];
        for (t, b) in pts {
            let last = *segs.last().unwrap();
            if t == last.0 {
                segs.last_mut().unwrap().2 = b;
                continue;
            }
            let start = last.1 + (t - last.0) as f64 * sample_rate as f64 * sec_per_tick(last.2, ppqn);
            segs.push((t, start, b));
        }
        TempoMap { ppqn, sample_rate, segs }
    }

    fn seg_for_tick(&self, tick: u64) -> (u64, f64, f64) {
        *self.segs.iter().rev().find(|s| s.0 <= tick).unwrap_or(&self.segs[0])
    }

    pub fn tick_to_sample(&self, tick: u64) -> u64 {
        let (t0, s0, bpm) = self.seg_for_tick(tick);
        (s0 + (tick - t0) as f64 * self.sample_rate as f64 * sec_per_tick(bpm, self.ppqn)).round() as u64
    }

    pub fn sample_to_tick(&self, sample: f64) -> f64 {
        let (t0, s0, bpm) = *self.segs.iter().rev().find(|s| s.1 <= sample).unwrap_or(&self.segs[0]);
        t0 as f64 + (sample - s0) / (self.sample_rate as f64 * sec_per_tick(bpm, self.ppqn))
    }

    pub fn bpm_at(&self, tick: u64) -> f64 {
        self.seg_for_tick(tick).2
    }

    /// `[tick, startSample, bpm]` per segment (for clients mapping playback position).
    pub fn segments(&self) -> Vec<(u64, f64, f64)> {
        self.segs.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempo_map() {
        let flat = TempoMap::new(120.0, &[], 960, 48000);
        for t in [0u64, 1, 480, 134_400, 999_999] {
            assert_eq!(flat.tick_to_sample(t), tick_to_sample(t, 120.0, 960, 48000, 0));
        }
        // 120 BPM for one bar (3840 ticks = 2 s = 96000 samples), then 60 BPM.
        let m = TempoMap::new(120.0, &[(3840, 60.0)], 960, 48000);
        assert_eq!(m.tick_to_sample(3840), 96_000);
        assert_eq!(m.tick_to_sample(7680), 96_000 + 192_000);
        assert!((m.sample_to_tick(96_000.0 + 96_000.0) - 5760.0).abs() < 1e-9);
        assert_eq!(m.bpm_at(100), 120.0);
        assert_eq!(m.bpm_at(4000), 60.0);
    }

    /// T_clock_lcm
    #[test]
    fn t_clock_lcm() {
        assert_eq!(lcm_all([7, 5, 8]), 280);
        assert_eq!(280 / 7, 40);
        assert_eq!(280 / 5, 56);
        assert_eq!(280 / 8, 35);
        let tps = ticks_per_step(960, 2).unwrap();
        assert_eq!(tps, 480);
        assert_eq!(280 * tps, 134_400);
    }

    #[test]
    fn conversions() {
        assert_eq!(ticks_per_step(960, 3), Some(320));
        assert_eq!(ticks_per_step(960, 7), None);
        assert_eq!(ticks_per_step(960, 0), None);
        // 120 BPM, 960 PPQN, 48 kHz → 25 samples per tick.
        assert_eq!(tick_to_sample(1, 120.0, 960, 48000, 0), 25);
        assert_eq!(tick_to_sample(134_400, 120.0, 960, 48000, 0), 3_360_000);
        assert!((sec_per_tick(120.0, 960) - 1.0 / 1920.0).abs() < 1e-15);
        assert_eq!(bar_ticks(960), 3840);
        assert_eq!(ceil_to_grid(0, 0, 3840), 0);
        assert_eq!(ceil_to_grid(1, 0, 3840), 3840);
        assert_eq!(ceil_to_grid(3840, 0, 3840), 3840);
        assert_eq!(gcd(12, 18), 6);
    }
}

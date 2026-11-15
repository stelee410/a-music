//! `sampler.multi` (FORMAT.md §8.3): plays recorded audio.
//!
//! A patch is a list of zones; a note sounds every zone whose key and velocity
//! range contain it, repitched from the zone's root pitch by resampling
//! (linear interpolation). One-shot zones play to their end and ignore
//! note-off; other zones are held, looping between their loop points if they
//! have any, and released with the amplitude envelope.
//!
//! The zones reach the engine as one string parameter, `zones`: a JSON array
//! written by the song player, which knows where the audio files are on disk.
//! Files are decoded once per process and shared between voices.
//!
//! Not implemented: `pan` (the engine is mono), loop crossfades, choke and
//! round-robin groups.

use crate::common::{NumParams, NumSpec};
use crate::detmath::{db_to_gain, exp2, sin_turns};
use crate::{ParamKind, ParamMeta, ParamValue, Voice};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub const SAMPLER_TABLE: &[NumSpec] = &[
    ("amp.attackMs", "ms", 0.0, 5000.0, 2.0),
    ("amp.releaseMs", "ms", 1.0, 10000.0, 80.0),
    ("gainDb", "dB", -60.0, 12.0, 0.0),
    // A slow wavering of the pitch that sets in after the note has begun: a held note played by a hand.
    ("vibrato.rateHz", "Hz", 0.1, 12.0, 5.5),
    ("vibrato.depthCents", "cents", 0.0, 100.0, 0.0),
    ("vibrato.delayMs", "ms", 0.0, 5000.0, 250.0),
];
const ATTACK: usize = 0;
const RELEASE: usize = 1;
const GAIN: usize = 2;
const VIB_RATE: usize = 3;
const VIB_DEPTH: usize = 4;
const VIB_DELAY: usize = 5;

/// Longest file the sampler will load (seconds, at the file's own rate).
const MAX_SECONDS: u64 = 120;

/// One decoded audio file, mixed down to mono.
pub struct Sample {
    pub data: Vec<f32>,
    pub rate: u32,
}

impl std::fmt::Debug for Sample {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sample({} frames at {} Hz)", self.data.len(), self.rate)
    }
}

fn cache() -> &'static Mutex<HashMap<String, Arc<Sample>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Sample>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Decode a WAV file (16/24/32-bit integer or float, any channel count) to mono.
pub fn load_sample(path: &str) -> Result<Arc<Sample>, String> {
    if let Some(s) = cache().lock().unwrap().get(path) {
        return Ok(s.clone());
    }
    let mut r = hound::WavReader::open(path).map_err(|e| format!("sample {path}: {e}"))?;
    let spec = r.spec();
    if spec.channels == 0 || spec.sample_rate < 8000 || spec.sample_rate > 192_000 {
        return Err(format!("sample {path}: unsupported format ({} ch, {} Hz)", spec.channels, spec.sample_rate));
    }
    if r.duration() as u64 > MAX_SECONDS * spec.sample_rate as u64 {
        return Err(format!("sample {path}: longer than {MAX_SECONDS} s"));
    }
    let flat: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>().map_err(|e| format!("sample {path}: {e}"))?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample.clamp(8, 32) - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>().map_err(|e| format!("sample {path}: {e}"))?
        }
    };
    let ch = spec.channels as usize;
    let data: Vec<f32> = flat.chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
    if data.is_empty() {
        return Err(format!("sample {path}: no audio"));
    }
    let s = Arc::new(Sample { data, rate: spec.sample_rate });
    cache().lock().unwrap().insert(path.to_string(), s.clone());
    Ok(s)
}

/// A zone as the song player writes it into the `zones` parameter.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct ZoneSpec {
    path: String,
    #[serde(default)]
    lo: u8,
    #[serde(default = "top_key")]
    hi: u8,
    #[serde(default = "middle_c")]
    root: u8,
    #[serde(default)]
    tune_cents: f64,
    #[serde(default)]
    gain_db: f64,
    #[serde(default)]
    vel_lo: f64,
    #[serde(default = "one")]
    vel_hi: f64,
    #[serde(default)]
    one_shot: bool,
    #[serde(default)]
    start: u64,
    #[serde(default)]
    end: Option<u64>,
    #[serde(default)]
    loop_start: Option<u64>,
    #[serde(default)]
    loop_end: Option<u64>,
    /// Round-robin group: of the zones of one group that fit a note, one is played.
    #[serde(default)]
    rr: Option<String>,
}

fn top_key() -> u8 {
    127
}
fn middle_c() -> u8 {
    60
}
fn one() -> f64 {
    1.0
}

#[derive(Clone, Debug)]
struct Zone {
    sample: Arc<Sample>,
    lo: u8,
    hi: u8,
    root: u8,
    tune_cents: f64,
    gain: f64,
    vel_lo: f64,
    vel_hi: f64,
    one_shot: bool,
    start: usize,
    end: usize,
    /// Loop points in frames, already clamped to `start..end`.
    looped: Option<(usize, usize)>,
    rr: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SamplerParams {
    pub n: NumParams,
    /// The `zones` parameter as given (identity of the loaded zones).
    spec: String,
    zones: Arc<Vec<Zone>>,
}

impl PartialEq for SamplerParams {
    fn eq(&self, o: &Self) -> bool {
        self.n == o.n && self.spec == o.spec
    }
}

impl Default for SamplerParams {
    fn default() -> Self {
        SamplerParams { n: NumParams::new(SAMPLER_TABLE), spec: String::new(), zones: Arc::new(vec![]) }
    }
}

impl SamplerParams {
    pub fn describe() -> Vec<ParamMeta> {
        let mut v = vec![ParamMeta {
            path: "zones",
            unit: "json",
            kind: ParamKind::Discrete,
            min: None,
            max: None,
            default: json!(""),
            safe_max: None,
            choices: None,
        }];
        v.extend(NumParams::metas(SAMPLER_TABLE));
        v
    }

    pub fn set(&mut self, path: &str, v: &ParamValue) -> Result<(), String> {
        if path == "zones" {
            let ParamValue::Str(s) = v else { return Err("zones expects a JSON array in a string".into()) };
            let specs: Vec<ZoneSpec> = serde_json::from_str(s).map_err(|e| format!("zones: {e}"))?;
            let mut zones = Vec::with_capacity(specs.len());
            for z in specs {
                let sample = load_sample(&z.path)?;
                let frames = sample.data.len();
                let start = (z.start as usize).min(frames - 1);
                let end = z.end.map_or(frames, |e| (e as usize).clamp(start + 1, frames));
                let looped = match (z.loop_start, z.loop_end) {
                    (Some(a), Some(b)) => {
                        let a = (a as usize).clamp(start, end - 1);
                        let b = (b as usize).clamp(a + 1, end);
                        (b - a >= 8).then_some((a, b))
                    }
                    _ => None,
                };
                zones.push(Zone {
                    sample,
                    lo: z.lo,
                    hi: z.hi,
                    root: z.root,
                    tune_cents: z.tune_cents,
                    gain: db_to_gain(z.gain_db),
                    vel_lo: z.vel_lo,
                    vel_hi: z.vel_hi,
                    one_shot: z.one_shot,
                    start,
                    end,
                    looped,
                    rr: z.rr,
                });
            }
            self.spec = s.clone();
            self.zones = Arc::new(zones);
            return Ok(());
        }
        if self.n.set(path, v)? {
            Ok(())
        } else {
            Err(format!("unknown param {path:?} for sampler.multi"))
        }
    }
}

/// One zone being played.
#[derive(Clone, Debug)]
struct Layer {
    zone: usize,
    pos: f64,
    step: f64,
    gain: f64,
    done: bool,
}

#[derive(Clone, Debug)]
pub struct SamplerVoice {
    p: SamplerParams,
    sr: f64,
    layers: Vec<Layer>,
    amp: f64,
    env: f64,
    attack_step: f64,
    release_step: f64,
    /// 0 idle, 1 attack, 2 held, 3 release
    stage: u8,
    held_layers: bool,
    seed: u64,
    /// The note's velocity when the renderer gave it (see `Voice::set_velocity`).
    velocity: Option<f64>,
    /// Samples since the note began, and the current pitch ratio of the vibrato.
    age: u64,
    bend: f64,
}

impl SamplerVoice {
    pub fn new(p: SamplerParams, sample_rate: u32, seed: u64) -> Self {
        SamplerVoice { p, sr: sample_rate as f64, layers: vec![], amp: 0.0, env: 0.0, attack_step: 1.0, release_step: 1.0, stage: 0, held_layers: false, seed, velocity: None, age: 0, bend: 1.0 }
    }
}

impl Voice for SamplerVoice {
    fn set_velocity(&mut self, velocity: f32) {
        self.velocity = Some(velocity as f64);
    }

    fn note_on(&mut self, pitch: u8, velocity: f32, _tick: u64) {
        let vel = velocity as f64;
        let touch = self.velocity.unwrap_or(vel); // which layer: by how hard the note is played, not by how loud the track is
        let zones = self.p.zones.clone();
        let fits: Vec<usize> = zones
            .iter()
            .enumerate()
            .filter(|(_, z)| pitch >= z.lo && pitch <= z.hi && touch >= z.vel_lo && touch <= z.vel_hi)
            .map(|(i, _)| i)
            .collect();
        // Round robin: of the zones of one group, play one. Which one follows from the note's seed, so
        // the same note in the same place always gets the same take, and neighbours get different ones.
        let chosen: Vec<usize> = fits
            .iter()
            .copied()
            .filter(|&i| match &zones[i].rr {
                None => true,
                Some(g) => {
                    let group: Vec<usize> = fits.iter().copied().filter(|&j| zones[j].rr.as_ref() == Some(g)).collect();
                    group[(self.seed >> 7) as usize % group.len()] == i
                }
            })
            .collect();
        self.age = 0;
        self.bend = 1.0;
        self.layers = chosen
            .into_iter()
            .map(|i| (i, &zones[i]))
            .map(|(i, z)| Layer {
                zone: i,
                pos: z.start as f64,
                step: exp2((pitch as f64 - z.root as f64) / 12.0 + z.tune_cents / 1200.0) * z.sample.rate as f64 / self.sr,
                gain: z.gain,
                done: false,
            })
            .collect();
        self.held_layers = self.layers.iter().any(|l| !zones[l.zone].one_shot);
        let v = &self.p.n.v;
        self.amp = vel * db_to_gain(v[GAIN]);
        self.attack_step = 1.0 / (v[ATTACK] * 0.001 * self.sr).max(1.0);
        self.release_step = 1.0 / (v[RELEASE] * 0.001 * self.sr).max(1.0);
        self.env = 0.0;
        self.stage = if self.layers.is_empty() { 0 } else { 1 };
    }

    /// One-shot zones play to their end; held zones are released.
    fn note_off(&mut self, _pitch: u8, _tick: u64) {
        if self.stage != 0 && self.held_layers {
            let zones = self.p.zones.clone();
            if self.layers.iter().all(|l| !zones[l.zone].one_shot) {
                self.stage = 3;
            } else {
                // Mixed: stop the held zones, let the one-shots finish.
                for l in self.layers.iter_mut().filter(|l| !zones[l.zone].one_shot) {
                    l.done = true;
                }
                self.held_layers = false;
            }
        }
    }

    fn set_param(&mut self, path: &str, value: &ParamValue) -> Result<(), String> {
        self.p.set(path, value)
    }

    fn render(&mut self, out: &mut [f32]) {
        if self.stage == 0 {
            return;
        }
        let zones = self.p.zones.clone();
        let (rate, depth, delay) = (self.p.n.v[VIB_RATE], self.p.n.v[VIB_DEPTH], self.p.n.v[VIB_DELAY] * 0.001);
        for o in out.iter_mut() {
            if depth > 0.0 && self.age % 32 == 0 {
                let t = self.age as f64 / self.sr;
                let onset = ((t - delay) / 0.35).clamp(0.0, 1.0);
                self.bend = exp2(depth * onset * sin_turns(rate * t) / 1200.0);
            }
            self.age += 1;
            match self.stage {
                1 => {
                    self.env += self.attack_step;
                    if self.env >= 1.0 {
                        self.env = 1.0;
                        self.stage = 2;
                    }
                }
                3 => {
                    self.env -= self.release_step;
                    if self.env <= 0.0 {
                        self.env = 0.0;
                        self.stage = 0;
                        return;
                    }
                }
                _ => {}
            }
            let mut s = 0.0f64;
            let mut alive = false;
            for l in self.layers.iter_mut().filter(|l| !l.done) {
                let z = &zones[l.zone];
                let d = &z.sample.data;
                let looped = if z.one_shot { None } else { z.looped };
                if let Some((a, b)) = looped {
                    while l.pos >= b as f64 {
                        l.pos -= (b - a) as f64;
                    }
                }
                let i = l.pos as usize;
                let next = match looped {
                    Some((a, b)) if i + 1 >= b => a,
                    _ => i + 1,
                };
                if next >= z.end {
                    l.done = true;
                    continue;
                }
                let f = l.pos - i as f64;
                let mut x = d[i] as f64 * (1.0 - f) + d[next] as f64 * f;
                // A short fade at the very end of the audio, so that a file cut mid-wave does not click.
                let left = (z.end - i) as f64 / l.step;
                if looped.is_none() && left < 96.0 {
                    x *= left / 96.0;
                }
                s += x * l.gain;
                l.pos += l.step * self.bend;
                alive = true;
            }
            if !alive {
                self.stage = 0;
                return;
            }
            *o += (s * self.amp * self.env) as f32;
        }
    }

    fn reset(&mut self) {
        self.layers.clear();
        self.stage = 0;
        self.env = 0.0;
    }

    fn is_active(&self) -> bool {
        self.stage != 0
    }

    fn level(&self) -> f32 {
        (self.amp * self.env) as f32
    }

    fn describe_params(&self) -> Vec<ParamMeta> {
        SamplerParams::describe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(dir: &std::path::Path, name: &str, frames: usize, hz: f64) -> String {
        let path = dir.join(name);
        let spec = hound::WavSpec { channels: 1, sample_rate: 48000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..frames {
            let v = (i as f64 * hz / 48000.0 * std::f64::consts::TAU).sin();
            w.write_sample((v * 20000.0) as i16).unwrap();
        }
        w.finalize().unwrap();
        path.to_string_lossy().into_owned()
    }

    fn params(zones: serde_json::Value) -> SamplerParams {
        let mut p = SamplerParams::default();
        p.set("zones", &ParamValue::Str(zones.to_string())).unwrap();
        p
    }

    fn crossings(b: &[f32]) -> usize {
        b.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
    }

    #[test]
    fn one_shot_plays_to_the_end_and_repitches() {
        let d = std::env::temp_dir().join(format!("agb-sampler-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let path = wav(&d, "a.wav", 4800, 480.0);
        let p = params(json!([{ "path": path, "root": 60, "oneShot": true }]));

        // At the root pitch: 0.1 s of 480 Hz = 48 cycles, then silence.
        let mut v = SamplerVoice::new(p.clone(), 48000, 0);
        v.note_on(60, 1.0, 0);
        v.note_off(60, 0); // ignored by a one-shot
        let mut b = vec![0.0f32; 9600];
        v.render(&mut b);
        assert!(!v.is_active());
        assert!((47..=49).contains(&crossings(&b)), "{}", crossings(&b));
        assert!(b[6000..].iter().all(|x| *x == 0.0));

        // An octave up: the same cycles in half the time.
        let mut v = SamplerVoice::new(p, 48000, 0);
        v.note_on(72, 1.0, 0);
        let mut b = vec![0.0f32; 9600];
        v.render(&mut b);
        assert!((47..=49).contains(&crossings(&b[..2500])));
        assert!(b[2500..].iter().all(|x| *x == 0.0));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn held_zone_loops_until_released_and_respects_key_range() {
        let d = std::env::temp_dir().join(format!("agb-sampler-test2-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let path = wav(&d, "b.wav", 4800, 480.0);
        let p = params(json!([{ "path": path, "lo": 36, "hi": 47, "root": 36, "loopStart": 0, "loopEnd": 4800 }]));

        let mut v = SamplerVoice::new(p.clone(), 48000, 0);
        v.note_on(36, 0.8, 0);
        let mut b = vec![0.0f32; 48000];
        v.render(&mut b);
        assert!(v.is_active(), "a looped zone keeps sounding while held");
        assert!(b[40000..].iter().any(|x| x.abs() > 0.1));
        v.note_off(36, 0);
        let mut tail = vec![0.0f32; 48000];
        v.render(&mut tail);
        assert!(!v.is_active());

        // Outside the key range nothing sounds.
        let mut v = SamplerVoice::new(p, 48000, 0);
        v.note_on(60, 0.8, 0);
        assert!(!v.is_active());

        assert!(SamplerParams::default().set("zones", &ParamValue::Str(r#"[{"path":"/nonexistent.wav"}]"#.into())).is_err());
        std::fs::remove_dir_all(&d).ok();
    }
}

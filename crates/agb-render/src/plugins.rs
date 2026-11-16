//! Tracks and effects played by VST 3 plug-ins.
//!
//! A plug-in keeps its state between blocks, and what it produces need not be the same twice (free
//! running LFOs, random phases). So while a piece is played bar by bar the plug-in is not run again
//! from a few seconds earlier, as the built-in voices are: its instance is kept, and each bar goes on
//! from where the last one stopped. Any other render starts the plug-in afresh.

use agb_dsp::PluginParams;
use agb_plugin::{Instance, NoteEvent};
use agb_protocol::{AgbError, ErrorCode};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};

/// A note for a plug-in, in samples from the start of the piece.
pub(crate) struct Note<'a> {
    pub on: u64,
    pub off: u64,
    pub pitch: u8,
    pub velocity: f32,
    /// Layer × track gain, applied to what the plug-in puts out from this note on.
    pub gain: f32,
    pub spec: &'a PluginParams,
}

struct Live {
    inst: Instance,
    name: String,
    uid: Option<String>,
    sample_rate: u32,
    /// The preset file loaded, with its size and date.
    preset: Option<String>,
    /// Parameter values applied so far.
    values: BTreeMap<String, f64>,
    /// The sample up to which this instance has played without a break.
    pos: Option<u64>,
    /// Notes started and not yet released: (start sample, pitch).
    held: Vec<(u64, u8)>,
    gain: f32,
    /// What the instance put out lately, ending at `pos`: the track's effects are run again from a
    /// few seconds before each bar and need to hear what led up to it.
    history: Vec<f32>,
}

/// How much of a plug-in's output is remembered, in seconds.
const HISTORY_SECONDS: usize = 60;

fn lives() -> &'static Mutex<HashMap<String, Live>> {
    static L: OnceLock<Mutex<HashMap<String, Live>>> = OnceLock::new();
    L.get_or_init(Default::default)
}

fn err(e: String) -> AgbError {
    AgbError::new(ErrorCode::AssetMissing, e)
}

fn preset_key(spec: &PluginParams) -> Option<String> {
    spec.preset.as_ref().map(|p| {
        let m = std::fs::metadata(p).ok();
        format!("{p}|{}|{:?}", m.as_ref().map_or(0, |m| m.len()), m.and_then(|m| m.modified().ok()))
    })
}

fn differs(live: &Live, spec: &PluginParams) -> bool {
    live.values != spec.values || live.preset != preset_key(spec)
}

/// Bring the instance to the sound `spec` describes.
fn apply(live: &mut Live, spec: &PluginParams) -> Result<(), AgbError> {
    let key = preset_key(spec);
    if live.preset != key {
        if let Some(path) = &spec.preset {
            let bytes = std::fs::read(path).map_err(|e| err(format!("plug-in preset {path}: {e}")))?;
            live.inst.load_preset(&bytes).map_err(|e| err(format!("plug-in preset {path}: {e}")))?;
        }
        live.preset = key;
        live.values.clear();
    }
    for (k, v) in &spec.values {
        if live.values.get(k) != Some(v) {
            let id = live.inst.param_id(k).ok_or_else(|| {
                err(format!("plug-in {:?} has no parameter {k:?} (list them with `agb plugin info {}`)", spec.name, spec.name))
            })?;
            live.inst.set_param(id, *v);
            live.values.insert(k.clone(), *v);
        }
    }
    Ok(())
}

/// The instance kept under `key`, replaced when it is another plug-in or holds settings `spec` no
/// longer has (those cannot be taken back).
fn instance<'a>(map: &'a mut HashMap<String, Live>, key: &str, spec: &PluginParams, instrument: bool, sample_rate: u32) -> Result<&'a mut Live, AgbError> {
    let stale = map.get(key).is_some_and(|l| {
        l.name != spec.name
            || l.uid != spec.uid
            || l.sample_rate != sample_rate
            || (l.preset.is_some() && spec.preset.is_none())
            || l.values.keys().any(|k| !spec.values.contains_key(k))
    });
    if stale {
        map.remove(key);
    }
    if !map.contains_key(key) {
        let inst = Instance::open(&spec.name, spec.uid.as_deref(), instrument, sample_rate).map_err(err)?;
        map.insert(
            key.to_string(),
            Live { inst, name: spec.name.clone(), uid: spec.uid.clone(), sample_rate, preset: None, values: BTreeMap::new(), pos: None, held: vec![], gain: 1.0, history: vec![] },
        );
    }
    Ok(map.get_mut(key).unwrap())
}

/// Whether a plug-in instance is kept for this track (it may still be ringing).
pub(crate) fn has_track(track_id: &str) -> bool {
    lives().lock().unwrap_or_else(|e| e.into_inner()).contains_key(&format!("i:{track_id}"))
}

/// Forget every plug-in instance and singer: the next render starts them afresh.
pub fn forget_plugins() {
    lives().lock().unwrap_or_else(|e| e.into_inner()).clear();
    crate::voices::forget();
}

/// Add a plug-in track's sound to `stem`, whose first sample is sample `base` of the piece.
///
/// With `keep` (playing bar by bar: only `[keep, end)` will be heard) the instance goes on from where
/// it stopped if that is exactly `keep`; otherwise it starts afresh at `base`. `resumable` says that
/// the next call may go on from the end of this one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_track(
    track_id: &str,
    notes: &[Note],
    stem: &mut [f32],
    base: u64,
    keep: Option<u64>,
    resumable: bool,
    sample_rate: u32,
    tempo_bpm: f64,
) -> Result<(), AgbError> {
    let mut map = lives().lock().unwrap_or_else(|e| e.into_inner());
    let key = format!("i:{track_id}");
    let end = base + stem.len() as u64;
    let live = match notes.first() {
        Some(n) => instance(&mut map, &key, n.spec, true, sample_rate)?,
        None => match map.get_mut(&key) {
            // No notes here, but what was played before may still be sounding.
            Some(l) if keep.is_some() && l.pos == keep => l,
            Some(l) => {
                l.pos = None;
                return Ok(());
            }
            None => return Ok(()),
        },
    };
    let cont = keep.is_some() && live.pos == keep;
    let start = if cont { keep.unwrap() } else { base };
    if cont {
        // The stem before `keep` is what was played then.
        let hist_start = start - (live.history.len() as u64).min(start);
        let from = base.max(hist_start);
        let n = (start - from) as usize;
        let h = (from - hist_start) as usize;
        let s0 = (from - base) as usize;
        for (x, y) in stem[s0..s0 + n].iter_mut().zip(&live.history[h..h + n]) {
            *x += *y;
        }
    } else {
        live.history.clear();
        live.inst.reset();
        live.held.clear();
        if let Some(n) = notes.first() {
            apply(live, n.spec)?;
            live.gain = n.gain;
        }
    }

    // (offset, 0 = off / 1 = on, note index or pitch of a note that is no longer in the score)
    let mut line: Vec<(usize, u8, usize)> = vec![];
    for h in &live.held {
        if !notes.iter().any(|n| n.on == h.0 && n.pitch == h.1 && n.off >= start) {
            line.push((0, 0, usize::MAX - h.1 as usize));
        }
    }
    for (i, n) in notes.iter().enumerate() {
        let started = n.on >= start && n.on < end;
        if started {
            line.push(((n.on - start) as usize, 1, i));
        }
        if n.off >= start && n.off < end && (started || live.held.contains(&(n.on, n.pitch))) {
            line.push(((n.off - start) as usize, 0, i));
        }
    }
    line.sort_unstable();

    let mut io = vec![0f32; (end - start) as usize];
    let mut gains: Vec<(usize, f32)> = vec![(0, live.gain)];
    let mut events: Vec<NoteEvent> = vec![];
    let mut seg = 0usize;
    for (offset, kind, i) in line {
        if kind == 0 {
            let (pitch, on) = if i < notes.len() { (notes[i].pitch, Some(notes[i].on)) } else { ((usize::MAX - i) as u8, None) };
            events.push(NoteEvent { offset: offset - seg, on: false, pitch, velocity: 0.0, id: -1 });
            match on {
                Some(on) => live.held.retain(|h| *h != (on, pitch)),
                None => live.held.retain(|h| h.1 != pitch),
            }
            continue;
        }
        let n = &notes[i];
        if differs(live, n.spec) {
            // The sound changes from this note on.
            live.inst.process(&mut io[seg..offset], &events, tempo_bpm, start + seg as u64);
            events.clear();
            seg = offset;
            apply(live, n.spec)?;
        }
        events.push(NoteEvent { offset: offset - seg, on: true, pitch: n.pitch, velocity: n.velocity, id: -1 });
        live.held.push((n.on, n.pitch));
        gains.push((offset, n.gain));
    }
    live.inst.process(&mut io[seg..], &events, tempo_bpm, start + seg as u64);

    let out = &mut stem[(start - base) as usize..];
    let mut g = 0usize;
    for (i, x) in io.iter().enumerate() {
        while g + 1 < gains.len() && gains[g + 1].0 <= i {
            g += 1;
        }
        let y = x * gains[g].1;
        out[i] += y;
        if resumable {
            live.history.push(y);
        }
    }
    let keep_len = HISTORY_SECONDS * sample_rate as usize;
    if live.history.len() > 2 * keep_len {
        let cut = live.history.len() - keep_len;
        live.history.drain(..cut);
    }
    if !resumable {
        live.history.clear();
    }
    live.gain = gains.last().unwrap().1;
    live.pos = resumable.then_some(end);
    Ok(())
}

/// Run a stem through a plug-in effect, in place. Always from the start of the stem.
pub(crate) fn render_fx(track_id: &str, index: usize, spec: &PluginParams, stem: &mut [f32], base: u64, sample_rate: u32, tempo_bpm: f64) -> Result<(), AgbError> {
    let mut map = lives().lock().unwrap_or_else(|e| e.into_inner());
    let live = instance(&mut map, &format!("f:{track_id}:{index}"), spec, false, sample_rate)?;
    if !live.inst.has_audio_input() {
        return Err(err(format!("plug-in {:?} takes no audio, so it cannot be used as an effect", spec.name)));
    }
    live.inst.reset();
    apply(live, spec)?;
    live.inst.process(stem, &[], tempo_bpm, base);
    Ok(())
}

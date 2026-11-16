//! Tracks sung by the simulated voice (`voice.chimera`).
//!
//! One singer per track, kept between the bars of a piece that is being played: a voice is in the
//! middle of a word at a bar line, so each bar goes on from where the last one stopped (as the
//! plug-ins do). The singer works in blocks of 128 samples counted from where it started, whatever
//! the size of the pieces it is asked for, so a piece played bar by bar is the piece rendered whole.

use agb_dsp::VoiceParams;
use agb_protocol::AgbError;
use agb_voice::{Singer, BLOCK, LEGATO_GAP};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// What the voice puts out at full velocity, against the other engines.
const LEVEL: f32 = 1.3;
/// How much of a singer's output is remembered for the track's effects, in seconds.
const HISTORY_SECONDS: usize = 60;

/// A sung note, in samples from the start of the piece.
pub(crate) struct Note<'a> {
    pub on: u64,
    pub off: u64,
    pub pitch: u8,
    /// velocity × layer gain × track gain
    pub amp: f32,
    pub lyric: Option<&'a str>,
    pub spec: &'a VoiceParams,
}

struct Live {
    singer: Singer,
    body: String,
    vibrato: f64,
    sample_rate: u32,
    /// The singer has rendered up to this sample (a whole number of blocks from where it started).
    rendered: u64,
    /// What it rendered beyond the end of the last piece asked for: the start of the next.
    ahead: Vec<f32>,
    /// The sample up to which its sound has been given out without a break.
    pos: Option<u64>,
    /// When the note being held ends, unless another begins first.
    release_at: Option<u64>,
    /// Start of the last note given to the singer (a note is begun before its beat, so this may be
    /// later than what has been rendered).
    last_on: Option<u64>,
    gain: f32,
    history: Vec<f32>,
}

fn lives() -> &'static Mutex<HashMap<String, Live>> {
    static L: OnceLock<Mutex<HashMap<String, Live>>> = OnceLock::new();
    L.get_or_init(Default::default)
}

pub(crate) fn has_track(track_id: &str) -> bool {
    lives().lock().unwrap_or_else(|e| e.into_inner()).contains_key(track_id)
}

pub(crate) fn forget() {
    lives().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Add a sung track to `stem`, whose first sample is sample `base` of the piece. `notes` are all
/// the track's notes from the start of the piece, in order (a word's syllables are spread over the
/// notes after it, so the words before matter). `keep` and `resumable` as for the plug-ins.
pub(crate) fn render_track(track_id: &str, notes: &[Note], stem: &mut [f32], base: u64, keep: Option<u64>, resumable: bool, sample_rate: u32) -> Result<(), AgbError> {
    let mut map = lives().lock().unwrap_or_else(|e| e.into_inner());
    let end = base + stem.len() as u64;
    let Some(spec) = notes.first().map(|n| n.spec) else {
        // No notes, but the last one may still be dying away.
        match map.get_mut(track_id) {
            Some(l) if keep.is_some() && l.pos == keep => {}
            Some(l) => {
                l.pos = None;
                return Ok(());
            }
            None => return Ok(()),
        }
        return render(map.get_mut(track_id).unwrap(), &[], &[], stem, base, keep.unwrap(), end, resumable);
    };
    let stale = map.get(track_id).is_some_and(|l| l.body != spec.body || l.vibrato != spec.vibrato_cents || l.sample_rate != sample_rate);
    let cont = !stale && keep.is_some() && map.get(track_id).is_some_and(|l| l.pos == keep);
    let start = if cont { keep.unwrap() } else { base };
    if !cont {
        map.insert(
            track_id.to_string(),
            Live {
                singer: Singer::new(sample_rate, &spec.body, spec.vibrato_cents),
                body: spec.body.clone(),
                vibrato: spec.vibrato_cents,
                sample_rate,
                rendered: start,
                ahead: vec![],
                pos: None,
                release_at: None,
                last_on: None,
                gain: 1.0,
                history: vec![],
            },
        );
    }
    let lyrics: Vec<Option<&str>> = notes.iter().map(|n| n.lyric).collect();
    let syllables = agb_voice::syllables(&spec.language, &lyrics);
    render(map.get_mut(track_id).unwrap(), notes, &syllables, stem, base, start, end, resumable)
}

#[allow(clippy::too_many_arguments)]
fn render(
    live: &mut Live,
    notes: &[Note],
    syllables: &[(agb_voice::Syllable, bool)],
    stem: &mut [f32],
    base: u64,
    start: u64,
    end: u64,
    resumable: bool,
) -> Result<(), AgbError> {
    let gap = (LEGATO_GAP * live.sample_rate as f64) as u64;
    // what led up to `start`, for the track's effects
    if start > base {
        let have = (live.history.len() as u64).min(start - base);
        let from = (start - have - base) as usize;
        let h = live.history.len() - have as usize;
        for (x, y) in stem[from..from + have as usize].iter_mut().zip(&live.history[h..]) {
            *x += *y;
        }
    }
    let mut out = vec![0f32; (end - start) as usize];
    // what was already rendered past the end of the piece before
    let n = live.ahead.len().min(out.len());
    out[..n].copy_from_slice(&live.ahead[..n]);
    let mut ahead = live.ahead.split_off(n);
    // Each note is started early by the time its consonants take, so that its vowel is on the beat.
    let mut begin: Vec<u64> = Vec::with_capacity(notes.len());
    for (i, n) in notes.iter().enumerate() {
        let prev = i.checked_sub(1).filter(|j| notes[*j].off + gap >= n.on).map(|j| (&syllables[j].0, notes[j].pitch as f64));
        let lead = live.singer.lead(&syllables[i].0, n.pitch as f64, syllables[i].1, prev);
        let at = n.on.saturating_sub((lead * live.sample_rate as f64) as u64);
        begin.push(at.max(begin.last().map_or(0, |b| b + BLOCK as u64)));
    }
    // go on after the last note already started (its start may lie before `start`: it was begun early)
    let mut next = match live.last_on {
        Some(on) => notes.partition_point(|n| n.on <= on),
        None => notes.partition_point(|n| n.on < start),
    };
    let mut t = live.rendered;
    let mut block = [0f32; BLOCK];
    while t < end {
        while next < notes.len() && begin[next] <= t {
            let n = &notes[next];
            live.last_on = Some(n.on);
            let (syl, melisma) = &syllables[next];
            live.singer.note_on(syl, n.pitch as f64, *melisma);
            live.gain = n.amp * LEVEL;
            live.release_at = Some(n.off + gap);
            next += 1;
        }
        if live.release_at.is_some_and(|r| r <= t) {
            live.singer.note_off();
            live.release_at = None;
        }
        block.fill(0.0);
        live.singer.render(&mut block);
        for (i, x) in block.iter().enumerate() {
            let at = t + i as u64;
            let y = x * live.gain;
            if at < end {
                out[(at - start) as usize] = y;
            } else {
                ahead.push(y);
            }
        }
        t += BLOCK as u64;
    }
    live.rendered = t;
    live.ahead = ahead;
    for (x, y) in stem[(start - base) as usize..].iter_mut().zip(&out) {
        *x += *y;
    }
    if resumable {
        live.history.extend_from_slice(&out);
        let keep_len = HISTORY_SECONDS * live.sample_rate as usize;
        if live.history.len() > 2 * keep_len {
            let cut = live.history.len() - keep_len;
            live.history.drain(..cut);
        }
    } else {
        live.history.clear();
    }
    live.pos = resumable.then_some(end);
    Ok(())
}

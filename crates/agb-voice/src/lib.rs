//! A singing voice (FORMAT.md §8.7): the track's notes carry lyrics, and a simulated vocal tract
//! sings them.
//!
//! The voice is CHIMERA by Jameson Hodge (MIT, <https://github.com/jamesondh/chimera-vocal-synth>):
//! lungs, vocal folds and a vocal tract computed as an airway, no samples. This crate turns the
//! lyrics written on notes into the syllables that engine sings, and plays them note by note.
//!
//! Languages: English (the CMU Pronouncing Dictionary, `data/`), Korean (Hangul) and Russian
//! (Cyrillic) through CHIMERA's own front ends; Mandarin written in pinyin through [`pinyin`];
//! anything else as IPA between slashes.

mod pinyin;

use chimera::anatomy::human::{BodyMix, HumanBody, BODY_CHILD, BODY_FEMALE, BODY_MALE};
use chimera::control::score::Gesture;
use chimera::phonetics::english::EnglishG2P;
use chimera::phonetics::ipa::tokenize_ipa;
use chimera::phonetics::korean::Korean;
use chimera::phonetics::russian::Russian;
use chimera::phonetics::sing::{plan_melisma, plan_note_off, plan_note_on, LegatoFrom, MelismaOptions, NoteOnOptions};
use chimera::phonetics::syllables::{read_lyrics, syllabify_segments, English};
use chimera::phonetics::Segment;
pub use chimera::phonetics::Syllable;
use chimera::voice::human_voice::{oversample_for, HumanVoice, SourcePatch};
use std::io::Read;
use std::sync::OnceLock;

/// Engine id of a track sung by this voice.
pub const ENGINE_ID: &str = "voice.chimera";
/// The engine renders in blocks of this many samples; notes start on a block.
pub const BLOCK: usize = 128;
/// A note that begins within this long after the one before it ended is sung without a break.
pub const LEGATO_GAP: f64 = 0.06;

pub const BODIES: &[&str] = &["male", "female", "child"];
pub const LANGUAGES: &[&str] = &["en", "zh"];

fn english() -> &'static EnglishG2P {
    static G: OnceLock<EnglishG2P> = OnceLock::new();
    G.get_or_init(|| {
        chimera::init();
        let mut g = EnglishG2P::new();
        let mut text = String::new();
        // Without the dictionary the words are still read, by letter-to-sound rules.
        if flate2::read::GzDecoder::new(&include_bytes!("../data/cmudict.txt.gz")[..]).read_to_string(&mut text).is_ok() {
            g.load_dict(&text);
        }
        g
    })
}

fn body(name: &str) -> (HumanBody, BodyMix) {
    match name {
        "female" => (BODY_FEMALE, BodyMix::FEMALE),
        "child" => (BODY_CHILD, BodyMix::CHILD),
        _ => (BODY_MALE, BodyMix::MALE),
    }
}

/// The vowel sung where a note has no words: "ah".
fn ah() -> Syllable {
    Syllable { text: "a".into(), nucleus: Segment::new("ɑ"), word_end: true, ..Default::default() }
}

/// One word → its syllables.
fn word(language: &str, text: &str) -> Vec<Syllable> {
    let t = text.trim();
    // "/…/": the sounds themselves, in IPA
    if let Some(ipa) = t.strip_prefix('/').and_then(|r| r.strip_suffix('/')) {
        return syllabify_segments(&tokenize_ipa(ipa))
            .into_iter()
            .map(|s| Syllable { text: ipa.to_string(), onset: s.onset, nucleus: s.nucleus, coda: s.coda, ..Default::default() })
            .collect();
    }
    let g = english();
    match language {
        "zh" => read_lyrics(t, &[&pinyin::Pinyin, &Korean, &Russian]),
        _ => read_lyrics(t, &[&English(g), &Korean, &Russian]),
    }
}

/// The part of a lyric that is sung: of `shown|sung`, the second; but when only the first can be
/// read (the two were written the other way round), the first.
fn sung_part<'a>(language: &str, lyric: &'a str) -> &'a str {
    match lyric.rsplit_once('|') {
        None => lyric.trim(),
        Some((a, b)) => {
            let (a, b) = (a.trim(), b.trim());
            if !matches!(b, "" | "-" | "~" | "+") && word(language, b).is_empty() && !word(language, a).is_empty() { a } else { b }
        }
    }
}

/// What a lyric shows to someone reading along: the word (of `shown|sung`, the shown side). Nothing
/// for the notes that only carry a word on (`+`, `-`, `~`), for bare IPA and for notes without words.
pub fn shown_part<'a>(language: &str, lyric: &'a str) -> Option<&'a str> {
    let marker = |t: &str| matches!(t, "" | "-" | "~" | "+");
    let shown = match lyric.rsplit_once('|') {
        None => lyric.trim(),
        Some((a, b)) => {
            let (a, b) = (a.trim(), b.trim());
            if sung_part(language, lyric) == a && !marker(b) { b } else { a }
        }
    };
    let ipa = shown.len() > 1 && shown.starts_with('/') && shown.ends_with('/');
    (!marker(shown) && !ipa).then_some(shown)
}

/// What each note of a track sings, from the lyric written on it (notes in the order they are sung).
///
/// - a word: its first syllable; `+` on the notes after it: the word's next syllables
///   (`"an-y-thing"` with hyphens also splits a word by hand);
/// - `-` or `~`: the vowel before goes on at this note's pitch (a melisma);
/// - `字|zi`: what is shown, then what is sung;
/// - `/…/`: IPA;
/// - nothing: "ah" on the first note, and after that the vowel before goes on.
///
/// The second value says whether the note is a melisma.
pub fn syllables(language: &str, lyrics: &[Option<&str>]) -> Vec<(Syllable, bool)> {
    let mut out: Vec<(Syllable, bool)> = Vec::with_capacity(lyrics.len());
    let mut rest: Vec<Syllable> = vec![]; // the syllables of a word still to be sung, reversed
    for l in lyrics {
        let l = l.map(str::trim).unwrap_or("");
        let sung = sung_part(language, l);
        let held = |out: &Vec<(Syllable, bool)>| match out.last() {
            Some((prev, _)) => (Syllable { text: "~".into(), onset: vec![], nucleus: prev.nucleus.clone(), coda: vec![], melisma: Some(true), ..Default::default() }, true),
            None => (ah(), false),
        };
        let next = match sung {
            "" | "-" | "~" => {
                rest.clear();
                held(&out)
            }
            "+" => match rest.pop() {
                Some(s) => (s, false),
                None => held(&out),
            },
            w => {
                let mut syls = word(language, w);
                syls.reverse();
                rest = syls;
                match rest.pop() {
                    Some(s) => (s, false),
                    None => held(&out), // nothing in it could be read
                }
            }
        };
        out.push(next);
    }
    out
}

/// Lyrics that no front end could read (they are sung as the vowel before, or "ah").
pub fn unreadable<'a>(language: &str, lyrics: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    lyrics
        .filter(|l| {
            let sung = sung_part(language, l);
            !matches!(sung, "" | "-" | "~" | "+") && word(language, sung).is_empty()
        })
        .collect()
}

struct Held {
    syl: Syllable,
    midi: f64,
    tag: f64,
}

/// Brings a voice simulated at two thirds of the output rate up to the output rate: three
/// samples for every two (a low-pass at the upper end of what the voice has, 32 taps a sample).
struct Upsampler {
    /// The filter at three times the simulated rate, 96 taps.
    taps: [f32; 96],
    /// The last 32 samples of the block before.
    tail: [f32; 32],
}

impl Upsampler {
    fn new() -> Upsampler {
        let mut taps = [0f32; 96];
        // windowed sinc: cut at 0.46 of the simulated rate, i.e. 0.46 / 3 of the filter's
        let fc = 0.46 / 3.0;
        for (n, h) in taps.iter_mut().enumerate() {
            let x = n as f64 - 47.5;
            let sinc = (2.0 * std::f64::consts::PI * fc * x).sin() / (std::f64::consts::PI * x);
            let w = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * (n as f64 + 0.5) / 96.0).cos() + 0.08 * (4.0 * std::f64::consts::PI * (n as f64 + 0.5) / 96.0).cos();
            *h = (3.0 * sinc * w) as f32;
        }
        Upsampler { taps, tail: [0.0; 32] }
    }

    /// `out` has three samples for every two of `inp`; `inp` is a whole block.
    fn run(&mut self, inp: &[f32], out: &mut [f32]) {
        let at = |i: isize| if i < 0 { self.tail[(32 + i) as usize] } else { inp[i as usize] };
        for (m, y) in out.iter_mut().enumerate() {
            let n = 2 * m as isize; // where this sample is at three times the simulated rate
            let mut i = n.div_euclid(3);
            let mut t = (n - 3 * i) as usize;
            let mut acc = 0f32;
            while t < 96 {
                acc += self.taps[t] * at(i);
                t += 3;
                i -= 1;
            }
            *y = acc;
        }
        let n = inp.len();
        self.tail.copy_from_slice(&inp[n - 32..]);
    }
}

/// One singer. Notes are given as they come; the sound is rendered in between.
pub struct Singer {
    voice: HumanVoice,
    mix: BodyMix,
    vibrato: f64,
    held: Option<Held>,
    tag: f64,
    /// For a singer simulated at two thirds of the output rate.
    up: Option<Upsampler>,
}

impl Singer {
    /// `body`: `male`, `female` or `child`. `vibrato`: depth in cents on held vowels.
    pub fn new(sample_rate: u32, body_name: &str, vibrato: f64) -> Singer {
        chimera::init();
        let (b, mix) = body(body_name);
        let fs = sample_rate as f64;
        let mut voice = HumanVoice::new(fs, b, oversample_for(fs));
        voice.set_source(&SourcePatch { voiced: Some(true), pressure: Some(9.0), tension: Some(0.55), ..Default::default() });
        Singer { voice, mix, vibrato: vibrato.clamp(0.0, 200.0), held: None, tag: 2.0, up: None }
    }

    /// The same singer at about half the cost, for slow machines: the voice is simulated at two
    /// thirds of `sample_rate` (so it has nothing above a third of it: 16 kHz at 48 kHz) and
    /// brought up to it. It works in blocks of [`Singer::block`] samples, not [`BLOCK`].
    pub fn light(sample_rate: u32, body_name: &str, vibrato: f64) -> Singer {
        let mut s = Singer::new(sample_rate * 2 / 3, body_name, vibrato);
        s.up = Some(Upsampler::new());
        s
    }

    /// The singer renders in blocks of this many samples; notes start on a block.
    pub fn block(&self) -> usize {
        if self.up.is_some() {
            BLOCK * 3 / 2
        } else {
            BLOCK
        }
    }

    fn at_now(&self, gs: Vec<Gesture>) -> Vec<Gesture> {
        let now = self.voice.clock + 0.006;
        gs.into_iter().map(|g| Gesture { t0: g.t0 + now, t1: g.t1 + now, ..g }).collect()
    }

    /// How long before its vowel a note has to be started so that the vowel falls on the beat: the
    /// time its consonants take (and, sung straight on from `prev`, that note's ending). People
    /// sing this way; a note started on the beat sounds late.
    pub fn lead(&self, syl: &Syllable, midi: f64, melisma: bool, prev: Option<(&Syllable, f64)>) -> f64 {
        if melisma && prev.is_some() {
            return 0.0;
        }
        let legato_from = prev.map(|(s, m)| LegatoFrom { syl: s.clone(), midi: m });
        let gs = plan_note_on(syl, midi, &NoteOnOptions { body: self.mix, vibrato: self.vibrato, tag: 0.0, legato_from });
        // the last gesture is the vowel's vibrato, which the planner starts 0.22 s into the vowel
        gs.last().map_or(0.0, |g| (g.t0 - 0.22 + 0.006).clamp(0.0, 0.6))
    }

    /// Whether a note is being held.
    pub fn is_holding(&self) -> bool {
        self.held.is_some()
    }

    /// Start a note. A note still held runs into it without a break (its coda, then this onset).
    pub fn note_on(&mut self, syl: &Syllable, midi: f64, melisma: bool) {
        if melisma {
            if let Some(h) = self.held.as_mut() {
                // the same vowel at a new pitch
                let tag = h.tag;
                h.midi = midi;
                self.voice.release(tag + 1.0);
                let gs = self.at_now(plan_melisma(midi, &MelismaOptions { vibrato: self.vibrato, tag }));
                self.voice.schedule(&gs);
                return;
            }
        }
        let legato_from = self.held.take().map(|h| {
            self.voice.release(h.tag);
            self.voice.release(h.tag + 1.0);
            LegatoFrom { syl: h.syl, midi: h.midi }
        });
        let tag = self.tag;
        self.tag += 2.0;
        let gs = self.at_now(plan_note_on(syl, midi, &NoteOnOptions { body: self.mix, vibrato: self.vibrato, tag, legato_from }));
        self.voice.schedule(&gs);
        self.held = Some(Held { syl: syl.clone(), midi, tag });
    }

    /// End the note being held: its coda is sung and the voice stops.
    pub fn note_off(&mut self) {
        if let Some(h) = self.held.take() {
            self.voice.release(h.tag);
            self.voice.release(h.tag + 1.0);
            let gs = self.at_now(plan_note_off(&h.syl, h.midi, self.mix));
            self.voice.schedule(&gs);
        }
    }

    /// Render the next `out.len()` samples (a multiple of [`Singer::block`] keeps renders
    /// repeatable whatever the size of the pieces they are made in; a light singer renders whole
    /// blocks only).
    pub fn render(&mut self, out: &mut [f32]) {
        if let Some(up) = self.up.as_mut() {
            // whole blocks only: three samples out for every two simulated
            let mut inner = [0f32; BLOCK];
            for block in out.chunks_exact_mut(BLOCK * 3 / 2) {
                inner.fill(0.0);
                self.voice.render(&mut inner);
                up.run(&inner, block);
            }
        } else {
            for block in out.chunks_mut(BLOCK) {
                self.voice.render(block);
            }
        }
        for x in out.iter_mut() {
            if !x.is_finite() {
                *x = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ipa(s: &Syllable) -> String {
        let seg = |v: &[Segment]| v.iter().map(|x| x.ipa.as_str()).collect::<String>();
        format!("{}.{}.{}", seg(&s.onset), s.nucleus.ipa, seg(&s.coda))
    }

    #[test]
    fn words_spread_over_notes() {
        let s = syllables("en", &[Some("anything"), Some("+"), Some("+"), Some("-"), None, Some("now")]);
        assert_eq!(s.len(), 6);
        assert_eq!(s.iter().map(|x| x.1).collect::<Vec<_>>(), [false, false, false, true, true, false]);
        assert_eq!(ipa(&s[5].0), "n.aʊ.");
        assert_ne!(ipa(&s[0].0), ipa(&s[1].0));
        // a first note without words is "ah"; what is shown is not what is sung
        assert_eq!(ipa(&syllables("en", &[None])[0].0), ".ɑ.");
        assert_eq!(ipa(&syllables("zh", &[Some("我|wo")])[0].0), "w.o.");
        assert_eq!(ipa(&syllables("zh", &[Some("wo|我")])[0].0), "w.o.", "written the other way round");
        assert_eq!(ipa(&syllables("en", &[Some("/la/")])[0].0), "l.a.");
        assert_eq!(unreadable("zh", ["wo", "我", "+", "xyzq"].into_iter()), ["我", "xyzq"]);
    }

    #[test]
    fn a_light_singer_sings_the_same_note() {
        let mut v = Singer::light(48000, "female", 30.0);
        assert_eq!(v.block(), 192);
        let s = syllables("en", &[Some("la")]);
        let mut out = vec![0f32; 192 * 500];
        v.render(&mut out[..192 * 25]);
        v.note_on(&s[0].0, 69.0, false);
        v.render(&mut out[192 * 25..192 * 275]);
        v.note_off();
        v.render(&mut out[192 * 275..]);
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        assert!(rms(&out[..4000]) < 1e-3, "silent before the note");
        assert!((0.2..0.8).contains(&rms(&out[20000..50000])), "as loud as the full singer: {}", rms(&out[20000..50000]));
        assert!(rms(&out[90000..]) < 0.01, "over after the note");
        let seg = &out[24000..40000];
        let corr = |lag: usize| seg[lag..].iter().zip(seg).map(|(a, b)| a * b).sum::<f32>();
        let best = (80..140).max_by(|a, b| corr(*a).total_cmp(&corr(*b))).unwrap();
        assert!((best as f64 - 48000.0 / 440.0).abs() < 3.0, "period {best} samples");
        // nothing above what the simulation has: no step from one sample to the next is large
        let step = seg.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
        assert!(step < 0.6, "smooth: {step}");
    }

    #[test]
    fn a_sung_note_sounds_at_its_pitch_and_stops() {
        let mut v = Singer::new(48000, "female", 30.0);
        let s = syllables("en", &[Some("la")]);
        let mut out = vec![0f32; 48000 * 2];
        v.render(&mut out[..4800]);
        v.note_on(&s[0].0, 69.0, false);
        v.render(&mut out[4800..52800]);
        v.note_off();
        v.render(&mut out[52800..]);
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        assert!(rms(&out[..4000]) < 1e-3, "silent before the note");
        assert!(rms(&out[20000..50000]) > 0.02, "sounding: {}", rms(&out[20000..50000]));
        assert!(rms(&out[90000..]) < 0.01, "over after the note: {}", rms(&out[90000..]));
        // the pitch: A4, by the strongest period near 109 samples
        let seg = &out[24000..40000];
        let corr = |lag: usize| seg[lag..].iter().zip(seg).map(|(a, b)| a * b).sum::<f32>();
        let best = (80..140).max_by(|a, b| corr(*a).total_cmp(&corr(*b))).unwrap();
        assert!((best as f64 - 48000.0 / 440.0).abs() < 3.0, "period {best} samples");
    }
}

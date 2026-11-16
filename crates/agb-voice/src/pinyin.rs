//! Mandarin written in pinyin → sung syllables. Tones are not sung: the melody is the pitch.
//!
//! `wo`, `zhe4`, `nǚ`, `lv`, `xi'an`: tone digits and tone marks are dropped, `v` is `ü`.

use chimera::phonetics::syllables::FrontEnd;
use chimera::phonetics::{Segment, Syllable};

pub struct Pinyin;

fn plain(c: char) -> char {
    match c {
        'ā' | 'á' | 'ǎ' | 'à' => 'a',
        'ē' | 'é' | 'ě' | 'è' => 'e',
        'ī' | 'í' | 'ǐ' | 'ì' => 'i',
        'ō' | 'ó' | 'ǒ' | 'ò' => 'o',
        'ū' | 'ú' | 'ǔ' | 'ù' => 'u',
        'ǖ' | 'ǘ' | 'ǚ' | 'ǜ' | 'ü' => 'v',
        c => c.to_ascii_lowercase(),
    }
}

fn is_pinyin_letter(c: char) -> bool {
    plain(c).is_ascii_lowercase()
}

const INITIALS: &[&str] = &["zh", "ch", "sh", "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x", "r", "z", "c", "s", "y", "w"];

/// A consonant: its IPA and, for a stop or affricate, whether it is aspirated (Mandarin tells
/// b/p, d/t, g/k, j/q, zh/ch, z/c apart by aspiration alone).
fn initial(i: &str) -> Option<Segment> {
    let (ipa, asp) = match i {
        "b" => ("p", Some(false)),
        "p" => ("p", Some(true)),
        "m" => ("m", None),
        "f" => ("f", None),
        "d" => ("t", Some(false)),
        "t" => ("t", Some(true)),
        "n" => ("n", None),
        "l" => ("l", None),
        "g" => ("k", Some(false)),
        "k" => ("k", Some(true)),
        "h" => ("x", None),
        "j" => ("tɕ", Some(false)),
        "q" => ("tɕ", Some(true)),
        "x" => ("ɕ", None),
        "zh" => ("tʂ", Some(false)),
        "ch" => ("tʂ", Some(true)),
        "sh" => ("ʂ", None),
        "r" => ("ʐ", None),
        "z" => ("ts", Some(false)),
        "c" => ("ts", Some(true)),
        "s" => ("s", None),
        _ => return None,
    };
    Some(Segment { ipa: ipa.into(), aspirated: asp, ..Default::default() })
}

/// A final (with y/w spellings already undone) → glide before the vowel, vowel, consonant after.
fn final_parts(f: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match f {
        "a" => ("", "a", ""),
        "o" => ("", "o", ""),
        "e" => ("", "ɤ", ""),
        "i" => ("", "i", ""),
        "u" => ("", "u", ""),
        "v" => ("", "y", ""),
        "ai" => ("", "aɪ", ""),
        "ei" => ("", "eɪ", ""),
        "ao" => ("", "aʊ", ""),
        "ou" => ("", "oʊ", ""),
        "an" => ("", "a", "n"),
        "en" => ("", "ə", "n"),
        "ang" => ("", "ɑ", "ŋ"),
        "eng" => ("", "ə", "ŋ"),
        "ong" => ("", "ʊ", "ŋ"),
        "er" => ("", "ɝ", ""),
        "ia" => ("j", "a", ""),
        "ie" => ("j", "ɛ", ""),
        "iao" => ("j", "aʊ", ""),
        "iu" | "iou" => ("j", "oʊ", ""),
        "ian" => ("j", "ɛ", "n"),
        "in" => ("", "i", "n"),
        "iang" => ("j", "ɑ", "ŋ"),
        "ing" => ("", "i", "ŋ"),
        "iong" => ("j", "ʊ", "ŋ"),
        "ua" => ("w", "a", ""),
        "uo" => ("w", "o", ""),
        "uai" => ("w", "aɪ", ""),
        "ui" | "uei" => ("w", "eɪ", ""),
        "uan" => ("w", "a", "n"),
        "un" | "uen" => ("w", "ə", "n"),
        "uang" => ("w", "ɑ", "ŋ"),
        "ueng" => ("w", "ə", "ŋ"),
        "ve" | "ue" => ("ɥ", "ɛ", ""),
        "van" => ("ɥ", "ɛ", "n"),
        "vn" => ("", "y", "n"),
        _ => return None,
    })
}

/// One pinyin syllable (letters only, lower case, `v` for ü), or None when it is not one.
fn syllable(s: &str) -> Option<Syllable> {
    let ini = INITIALS.iter().find(|i| s.starts_with(**i)).copied().unwrap_or("");
    let mut fin = s[ini.len()..].to_string();
    let mut onset: Vec<Segment> = vec![];
    match ini {
        // y and w only spell a syllable that starts with i, ü or u
        "y" => {
            fin = match fin.as_str() {
                "i" | "in" | "ing" => fin,
                "u" => "v".into(),
                "ue" => "ve".into(),
                "uan" => "van".into(),
                "un" => "vn".into(),
                f => format!("i{f}"),
            };
        }
        "w" => {
            if fin != "u" {
                fin = format!("u{fin}");
            }
        }
        // after j, q, x the letter u is ü
        "j" | "q" | "x" => {
            fin = match fin.as_str() {
                "u" => "v".into(),
                "ue" => "ve".into(),
                "uan" => "van".into(),
                "un" => "vn".into(),
                _ => fin,
            };
            onset.push(initial(ini)?);
        }
        "" => {}
        i => onset.push(initial(i)?),
    }
    if fin.is_empty() {
        return None;
    }
    let (glide, mut vowel, coda) = final_parts(&fin)?;
    // zi ci si, zhi chi shi ri: the vowel is the consonant held, not [i]
    if fin == "i" && matches!(ini, "z" | "c" | "s" | "zh" | "ch" | "sh" | "r") {
        vowel = "ɨ";
    }
    // bo po mo fo are said [wo]
    let glide = if fin == "o" && matches!(ini, "b" | "p" | "m" | "f") { "w" } else { glide };
    if !glide.is_empty() {
        onset.push(Segment::new(glide));
    }
    Some(Syllable {
        text: s.replace('v', "ü"),
        onset,
        nucleus: Segment::new(vowel),
        coda: if coda.is_empty() { vec![] } else { vec![Segment::new(coda)] },
        ..Default::default()
    })
}

/// Split a run of letters into pinyin syllables: the longest syllable first, as long as what is
/// left can still be read (`xian` is one syllable, `xi'an` two).
fn split(s: &str) -> Option<Vec<Syllable>> {
    if s.is_empty() {
        return Some(vec![]);
    }
    for n in (1..=s.len().min(6)).rev() {
        if let Some(first) = syllable(&s[..n]) {
            if let Some(mut rest) = split(&s[n..]) {
                let mut out = vec![first];
                out.append(&mut rest);
                return Some(out);
            }
        }
    }
    None
}

impl FrontEnd for Pinyin {
    fn starts(&self, c: char) -> bool {
        is_pinyin_letter(c)
    }
    fn continues(&self, c: char) -> bool {
        is_pinyin_letter(c) || c.is_ascii_digit() || c == '\'' || c == '-'
    }
    fn word(&self, word: &str) -> Vec<Syllable> {
        let mut out = vec![];
        for part in word.split(|c: char| c == '\'' || c == '-' || c.is_ascii_digit()).filter(|p| !p.is_empty()) {
            let letters: String = part.chars().map(plain).collect();
            match split(&letters) {
                Some(mut s) => out.append(&mut s),
                None => return vec![], // not pinyin: the word is not sung as something else
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn say(w: &str) -> String {
        Pinyin
            .word(w)
            .iter()
            .map(|s| {
                let seg = |v: &[Segment]| v.iter().map(|x| format!("{}{}", x.ipa, if x.aspirated == Some(true) { "ʰ" } else { "" })).collect::<String>();
                format!("{}{}{}", seg(&s.onset), s.nucleus.ipa, seg(&s.coda))
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn syllables_are_read() {
        assert_eq!(say("wo"), "wo");
        assert_eq!(say("zai4"), "tsaɪ");
        assert_eq!(say("zhe"), "tʂɤ");
        assert_eq!(say("li"), "li");
        assert_eq!(say("qing"), "tɕʰiŋ");
        assert_eq!(say("xue"), "ɕɥɛ");
        assert_eq!(say("yuan"), "ɥɛn");
        assert_eq!(say("yu"), "y");
        assert_eq!(say("nǚ"), "ny");
        assert_eq!(say("shi"), "ʂɨ");
        assert_eq!(say("si"), "sɨ");
        assert_eq!(say("bo"), "pwo");
        assert_eq!(say("guang"), "kwɑŋ");
        assert_eq!(say("you"), "joʊ");
        assert_eq!(say("wen"), "wən");
        assert_eq!(say("er"), "ɝ");
        assert_eq!(say("jiu"), "tɕjoʊ");
        // several syllables in one word, and the apostrophe that separates them
        assert_eq!(say("nihao"), "ni xaʊ");
        assert_eq!(say("xian"), "ɕjɛn");
        assert_eq!(say("xi'an"), "ɕi an");
        assert_eq!(say("tian1kong1"), "tʰjɛn kʰʊŋ");
        // not pinyin
        assert_eq!(say("the"), "");
        assert_eq!(say("xyz"), "");
    }
}

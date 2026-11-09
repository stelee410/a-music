//! Pitch notation. Scientific pitch: `C4` = MIDI 60, `A4` = 69, `C-1` = 0.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `"E1"`-style note name or a MIDI number 0–127.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Pitch {
    Midi(u8),
    Name(String),
}

impl Pitch {
    pub fn to_midi(&self) -> Result<u8, String> {
        match self {
            Pitch::Midi(m) if *m <= 127 => Ok(*m),
            Pitch::Midi(m) => Err(format!("MIDI pitch {m} out of range 0..=127")),
            Pitch::Name(s) => parse_pitch(s),
        }
    }
}

/// Parse `C#3`, `Db2`, `E1`, `C-1` … into a MIDI note number.
pub fn parse_pitch(s: &str) -> Result<u8, String> {
    // A MIDI number written as a string ("65") is unambiguous: accept it.
    if let Ok(m) = s.trim().parse::<u8>() {
        return if m <= 127 { Ok(m) } else { Err(format!("MIDI pitch {m} out of range 0..=127")) };
    }
    let err = || format!("invalid pitch name {s:?} (expected e.g. \"E1\", \"C#3\", \"Bb2\")");
    let mut chars = s.chars();
    let letter = chars.next().ok_or_else(err)?.to_ascii_uppercase();
    let base: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return Err(err()),
    };
    let rest: &str = chars.as_str();
    let (acc, oct_str) = match rest.chars().next() {
        Some('#') => (1, &rest[1..]),
        Some('b') => (-1, &rest[1..]),
        _ => (0, rest),
    };
    let octave: i32 = oct_str.parse().map_err(|_| err())?;
    let midi = (octave + 1) * 12 + base + acc;
    if !(0..=127).contains(&midi) {
        return Err(format!("pitch {s:?} -> MIDI {midi} out of range 0..=127"));
    }
    Ok(midi as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(parse_pitch("C4").unwrap(), 60);
        assert_eq!(parse_pitch("A4").unwrap(), 69);
        assert_eq!(parse_pitch("C-1").unwrap(), 0);
        assert_eq!(parse_pitch("E1").unwrap(), 28);
        assert_eq!(parse_pitch("B1").unwrap(), 35);
        assert_eq!(parse_pitch("C2").unwrap(), 36);
        assert_eq!(parse_pitch("E3").unwrap(), 52);
        assert_eq!(parse_pitch("C#3").unwrap(), 49);
        assert_eq!(parse_pitch("Bb2").unwrap(), 46);
        assert_eq!(parse_pitch("G9").unwrap(), 127);
        assert_eq!(parse_pitch("65").unwrap(), 65);
        assert!(parse_pitch("200").is_err());
        assert!(parse_pitch("G#9").is_err());
        assert!(parse_pitch("H2").is_err());
        assert!(parse_pitch("").is_err());
        assert!(parse_pitch("C").is_err());
    }

    #[test]
    fn untagged_json() {
        let p: Pitch = serde_json::from_str("36").unwrap();
        assert_eq!(p.to_midi().unwrap(), 36);
        let p: Pitch = serde_json::from_str("\"E1\"").unwrap();
        assert_eq!(p.to_midi().unwrap(), 28);
    }
}

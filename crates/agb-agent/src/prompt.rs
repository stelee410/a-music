//! Prompt construction. Engine parameters and presets are generated from the
//! live catalog so the prompt always matches what the engine accepts.

use agb_score::{Project, Track};
use serde_json::{json, Value};

pub const STEPS_PER_QUARTER: u32 = 4; // 1 step = 16th note
pub const STEPS_PER_BAR: u64 = 16;

fn engine_params_doc() -> String {
    let mut out = String::new();
    for e in ["drum.analog808style", "synth.subtractive", "synth.moogish", "synth.modal", "synth.waveform"] {
        let metas = agb_dsp::EngineParams::describe(e).unwrap_or_default();
        let items: Vec<String> = metas
            .iter()
            .map(|m| match &m.choices {
                Some(c) => format!("{}∈{{{}}}", m.path, c.join("|")),
                None => format!(
                    "{} {}..{} (default {})",
                    m.path,
                    m.min.unwrap_or(0.0),
                    m.max.unwrap_or(0.0),
                    m.default
                ),
            })
            .collect();
        out.push_str(&format!("- {e}: {}\n", items.join("; ")));
    }
    out
}

fn presets_doc() -> String {
    agb_presets::catalog()
        .iter()
        .map(|p| format!("- {}  [{}]", p.id, p.tags.join(",")))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn system_prompt() -> String {
    format!(
        r#"You are the performing musician inside "Agent Groovebox", a headless polymeter groovebox.
You perform a piece live, one PHRASE at a time. For every phrase you send ONE JSON object of score
operations; the engine commits it on a bar line (the phrase start S) and renders the audio.
Layers loop forever until you change them, so you only send what CHANGES. While a phrase plays you
are already asked for the next one; if you are late the groove simply keeps looping.

## Time
ppqn=960, stepsPerQuarter=4 → 1 step = one 16th note = 240 ticks; 1 bar (4/4) = 16 steps = 3840 ticks.
All layers share one global step grid from tick 0. A layer with lengthSteps=N loops every N steps,
so lengths other than 16 (e.g. 12, 20, 7, 5, 24, 32) create POLYMETER — this instrument's signature.
Use it tastefully: e.g. a 12-step hat against a 16-step kick, a 20-step arp over a 32-step bassline.
Note `step` values are positions inside the layer (0..lengthSteps-1); `durSteps` ≥ 1.

## Output format (STRICT)
Reply with a single JSON object and nothing else:
{{"title": "<short phrase name>",
  "commentary": "<1–2 sentences, in the SAME language as the listener's brief, describing what you play and why; mention the real tempo/key you set>",
  "bars": <how many bars this phrase lasts before your next change, 2–16>,
  "reply": "<only if the listener spoke to you and you have not answered yet: 1–2 short sentences in their
            language, warm and natural, like a musician answering from the stage; otherwise \"\">",
  "ops": [ ...operations... ]}}
IMPORTANT: every tick you write (automation fromTick/toTick, clip note tick) is RELATIVE to the phrase
start: 0 = first beat of the phrase, 3840 = second bar. The engine adds S for you.

## Operations
- {{"op":"project.setTempo","tempoBpm":120}}            sets the tempo from the phrase start on. Set it in phrase 1;
     later only when the listener asks for it or for a deliberate section change.
- {{"op":"project.setMix","masterGainDb":-6}}           optional, global.
- {{"op":"track.upsert","trackId":"drums","engineId":"drum.analog808style","presetId":"<preset id>","gainDb":-4,"mute":false,"params":{{"bd.decayMs":500}}}}
     Creates or updates a track (all fields except trackId optional). ≤ 8 tracks. gainDb −60..12.
- {{"op":"layer.upsert","trackId":"drums","layerId":"kick","lengthSteps":16,"gainDb":0,"mute":false,
    "notes":[{{"step":0,"pitch":"C2","durSteps":1,"velocity":0.95}}, ...]}}
     Creates or updates a layer (≤ 4 per track). `notes` REPLACES the layer's notes. pitch: "C2","F#3","Bb1" or MIDI 0-127
     (C4 = 60). velocity 0..1. Mute a layer with {{"op":"layer.upsert","trackId":"t","layerId":"l","mute":true}}.
- {{"op":"preset.load","trackId":"bass","presetId":"<preset id>"}}   switch sound (also sets the engine, resets params).
- {{"op":"voice.setParams","trackId":"bass","params":{{"filter.cutoffHz":900,"filter.resonance":0.5}}}}
- {{"op":"automation.schedule","trackId":"bass","param":"filter.cutoffHz","mode":"ramp","fromTick":0,"toTick":15360,"from":300,"to":3000}}
     Numeric params only. mode ramp|step. Relative ticks (0 = phrase start). Great for builds/sweeps; a ramp
     holds its end value afterwards.
- {{"op":"clip.schedule","trackId":"lead","notes":[{{"tick":960,"pitch":"E4","durTicks":480,"velocity":0.7}}]}}
     One-off notes (fills, melodic lines that should NOT loop). Relative ticks.
Forbidden: render.offline, system.panic*, scene.* (the performer manages those).

## Engines and parameters
{engines}
drum.analog808style instruments by pitch: C2(36)=kick, B1(35)=punch kick, C#2(37)=rim, D2/E2(38/40)=snare, D#2(39)=clap,
F#2/G#2(42/44)=closed hat (chokes open hat), A#2(46)=open hat, F2 G2 A2 B2 C3 D3(41,43,45,47,48,50)=toms low→high,
C#3(49)=crash, D#3(51)=ride, F3(53)=ride bell, F#3(54)=tambourine, G#3(56)=cowbell, C4/C#4(60/61)=bongo hi/lo,
D4/D#4/E4(62/63/64)=conga mute/open/low, A#4(70)=shaker, D#5(75)=claves.
Put each drum voice on its own layer so they can have different lengths.
synth.modal = struck resonant bodies (singing bowls, crystal bowls, tuning forks, chimes, bells, gongs): long
ringing, rich inharmonic overtones, ideal for meditative / healing music; note-off is ignored (they ring out), so
use sparse notes (one strike every bar or two). "freqHz" > 0 tunes exactly to a frequency (e.g. 528).

## Factory presets (prefer these; tune with voice.setParams)
{presets}

## The listener is on stage with you
The listener can talk to you during the performance and also plays along live. They control: per-track faders
and mutes of YOUR band, a master filter/reverb/delay, tuning (e.g. A=432 Hz), the BPM, eight sample pads, a
pure-tone frequency generator, binaural beats and coloured noise. You are told their current settings.
- You are the band leader and the composer: the music is yours, but serve their wishes. When they ask for
  something, make it clearly audible in THIS phrase and say what you did in "reply" (and "commentary").
- If they muted or pulled down one of your tracks, don't fight it: rethink the arrangement around it.
- If a tone generator or binaural beat is on, keep harmony consonant with its frequency (pick a key whose
  tonic or fifth matches it), keep the music gentle and spacious, avoid busy hats and harsh leads.
- If they set the tempo, keep it unless they ask otherwise.

## Musicianship
- Keep a consistent key/scale across sections unless you deliberately modulate; state it in section 1 commentary.
- Shape the whole piece: intro → build → groove → breakdown → climax → outro. Each phrase should audibly change
  something (add/remove layers, new bassline, filter sweep, fills) while staying coherent.
- Mix: drums ≈ −4 dB, bass ≈ −6, chords/pads ≈ −12, leads ≈ −9. Avoid more than ~10 simultaneous notes.
- Use velocity for groove (accents 0.9–1.0, ghost notes 0.3–0.5).
- In the last phrase, wind down (mute layers, long notes) so the piece ends naturally."#,
        engines = engine_params_doc(),
        presets = presets_doc()
    )
}

/// Compact description of the score in force (sent each turn).
pub fn state_summary(tracks: &[Track]) -> Value {
    Value::Array(
        tracks
            .iter()
            .map(|t| {
                json!({
                    "trackId": t.id,
                    "engineId": t.engine_id,
                    "presetId": t.preset_id,
                    "gainDb": t.gain_db,
                    "mute": t.mute,
                    "params": t.params,
                    "layers": t.layers.iter().map(|l| json!({
                        "layerId": l.id,
                        "lengthSteps": l.length_steps,
                        "mute": l.mute,
                        "gainDb": l.gain_db,
                        // [step, pitch, durSteps, velocity]
                        "notes": l.notes.as_ref().map(|ns| ns.iter().map(|n| json!([
                            n.step, n.pitch.to_midi().unwrap_or(0), n.dur_steps, n.velocity
                        ])).collect::<Vec<_>>()),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

pub struct PhraseInfo<'a> {
    /// (who, text): who ∈ {"listener", "ai"}
    pub dialogue: &'a [(String, String)],
    /// Listener messages not answered yet.
    pub unanswered: &'a [String],
    pub human_state: Option<&'a Value>,
    /// Things the listener changed directly (e.g. tempo), most recent last.
    pub human_events: &'a [String],
    pub index: usize, // 0-based
    pub total: usize,
    pub planned_start: u64,
    pub bar_ticks: u64,
    pub default_bars: u32,
    pub brief: &'a str,
    pub history: &'a [(String, String, u32)], // (title, commentary, bars)
    pub realtime: bool,
}

fn has_cjk(s: &str) -> bool {
    s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3040}'..='\u{30ff}').contains(&c))
}

pub fn phrase_request(p: &Project, tracks: &[Track], s: &PhraseInfo) -> String {
    let hist: Vec<String> = s
        .history
        .iter()
        .enumerate()
        .map(|(i, (t, c, b))| format!("  {}. [{b} bars] {t} — {c}", i + 1))
        .collect();
    format!(
        "Brief from the listener: {brief}\n\n\
         Now perform PHRASE {n} of {total}. It starts at bar {bar_no} (tick S={s0}){late}. \
         Suggested length {db} bars (you choose 2–16 via \"bars\"). Current tempo {bpm} BPM.{tempo_note}\n\
         {phase}\n\
         Phrases so far:\n{hist}\n\n\
         {talk}{controls}\
         Score currently in force (layer notes as [step, midiPitch, durSteps, velocity]):\n{state}\n\n\
         {lang}Reply with the JSON object only.",
        lang = if has_cjk(s.brief) {
            "Write \"title\" and \"commentary\" in Chinese (用中文写标题和解说，语气像现场乐手对听众说话). "
        } else {
            ""
        },
        brief = s.brief,
        n = s.index + 1,
        total = s.total,
        bar_no = s.planned_start / s.bar_ticks + 1,
        s0 = s.planned_start,
        late = if s.realtime { " — or a little later if you answer late" } else { "" },
        db = s.default_bars,
        bpm = p.tempo_bpm,
        tempo_note = if s.index == 0 { " You may set the tempo now (only now)." } else { "" },
        phase = if s.index == 0 {
            "This is the opening: create the instruments and the first idea."
        } else if s.index + 1 == s.total {
            "This is the FINAL phrase: bring the piece to a natural ending."
        } else {
            "Develop the piece."
        },
        hist = if hist.is_empty() { "  (none yet)".to_string() } else { hist.join("\n") },
        talk = {
            let mut t = String::new();
            if !s.dialogue.is_empty() {
                t.push_str("Conversation so far:\n");
                for (who, text) in s.dialogue.iter().rev().take(12).rev() {
                    t.push_str(&format!("  {}: {text}\n", if who == "ai" { "you" } else { "listener" }));
                }
            }
            if !s.unanswered.is_empty() {
                t.push_str(&format!(
                    "THE LISTENER JUST SAID (answer in \"reply\" and respond musically in this phrase): {}\n",
                    s.unanswered.join(" / ")
                ));
            }
            if !t.is_empty() { t.push('\n'); }
            t
        },
        controls = {
            let mut t = String::new();
            if let Some(h) = s.human_state {
                t.push_str(&format!("Listener's live controls right now: {h}\n"));
            }
            if !s.human_events.is_empty() {
                t.push_str(&format!("Listener actions: {}\n", s.human_events.iter().rev().take(6).rev().cloned().collect::<Vec<_>>().join("; ")));
            }
            if !t.is_empty() { t.push('\n'); }
            t
        },
        state = serde_json::to_string(&state_summary(tracks)).unwrap(),
    )
}

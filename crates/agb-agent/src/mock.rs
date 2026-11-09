//! Deterministic offline "performer" (no network). Used by tests and by
//! `agb perform --mock` as a demo / fallback. It speaks the same JSON reply
//! format as a real LLM, so it exercises the full pipeline.

use crate::llm::{Llm, Msg, Reply};
use serde_json::{json, Value};

#[derive(Default, Clone)]
pub struct MockPerformer {
    /// If set, the first reply of each section is invalid (tests the retry loop).
    pub fail_first: bool,
    pub attempts: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<usize, u32>>>,
    /// Artificial latency per reply.
    pub delay_ms: u64,
}

fn parse_section(text: &str) -> (usize, usize, u64) {
    let num_after = |key: &str| -> Option<u64> {
        let i = text.find(key)? + key.len();
        text[i..].trim_start().split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
    };
    let n = num_after("PHRASE ").unwrap_or(1) as usize;
    let total = num_after(" of ").unwrap_or(1) as usize;
    (n, total, 0)
}

fn notes(v: &[(u32, &str, u32, f64)]) -> Value {
    Value::Array(v.iter().map(|(s, p, d, vel)| json!({"step": s, "pitch": p, "durSteps": d, "velocity": vel})).collect())
}

fn section_ops(n: usize, total: usize, start: u64) -> (String, String, Vec<Value>) {
    let bass_a = notes(&[(0, "A1", 3, 0.95), (4, "A1", 1, 0.6), (6, "C2", 2, 0.8), (10, "E2", 2, 0.85), (14, "G1", 2, 0.7)]);
    let bass_b = notes(&[(0, "F1", 3, 0.95), (4, "F1", 1, 0.6), (6, "A1", 2, 0.8), (10, "C2", 2, 0.85), (14, "E1", 2, 0.7)]);
    if n == 1 {
        return (
            "Opening".into(),
            "A minor, 118 BPM: four-on-the-floor kick against a 12-step hat cycle.".into(),
            vec![
                json!({"op":"project.setTempo","tempoBpm":118.0}),
                json!({"op":"track.upsert","trackId":"drums","engineId":"drum.analog808style","presetId":"preset:drum.analog808style:drums:tight_kit@v1.0","gainDb":-4.0}),
                json!({"op":"layer.upsert","trackId":"drums","layerId":"kick","lengthSteps":16,"notes":notes(&[(0,"C2",1,1.0),(4,"C2",1,0.9),(8,"C2",1,0.95),(12,"C2",1,0.9)])}),
                json!({"op":"layer.upsert","trackId":"drums","layerId":"hat","lengthSteps":12,"notes":notes(&[(2,"F#2",1,0.7),(5,"F#2",1,0.4),(8,"F#2",1,0.8),(11,"A#2",1,0.5)])}),
                json!({"op":"track.upsert","trackId":"bass","presetId":"preset:synth.moogish:bass:acid_ladder@v1.0","engineId":"synth.moogish","gainDb":-7.0}),
                json!({"op":"layer.upsert","trackId":"bass","layerId":"line","lengthSteps":16,"notes":bass_a}),
            ],
        );
    }
    if n == total {
        return (
            "Outro".into(),
            "Drums drop out; the pad holds A minor while the bass fades.".into(),
            vec![
                json!({"op":"layer.upsert","trackId":"drums","layerId":"kick","mute":true}),
                json!({"op":"layer.upsert","trackId":"drums","layerId":"hat","mute":true}),
                json!({"op":"automation.schedule","trackId":"bass","param":"filter.cutoffHz","mode":"ramp","fromTick":start,"toTick":start+3840*2,"from":350.0,"to":120.0}),
                json!({"op":"track.upsert","trackId":"pad","engineId":"synth.subtractive","presetId":"preset:synth.subtractive:pad:warm_pad@v1.0","gainDb":-12.0}),
                json!({"op":"layer.upsert","trackId":"pad","layerId":"chord","lengthSteps":32,"notes":notes(&[(0,"A3",28,0.6),(0,"C4",28,0.55),(0,"E4",28,0.55)])}),
            ],
        );
    }
    match n % 3 {
        2 => (
            "Build".into(),
            "Warm pad enters on a 32-step cycle; bass filter opens over two bars.".into(),
            vec![
                json!({"op":"track.upsert","trackId":"pad","engineId":"synth.subtractive","presetId":"preset:synth.subtractive:pad:warm_pad@v1.0","gainDb":-12.0}),
                json!({"op":"layer.upsert","trackId":"pad","layerId":"chord","lengthSteps":32,"notes":notes(&[(0,"A3",14,0.6),(0,"C4",14,0.55),(0,"E4",14,0.55),(16,"F3",14,0.6),(16,"A3",14,0.55),(16,"C4",14,0.55)])}),
                json!({"op":"automation.schedule","trackId":"bass","param":"filter.cutoffHz","mode":"ramp","fromTick":start,"toTick":start+3840*2,"from":350.0,"to":1400.0}),
                json!({"op":"layer.upsert","trackId":"drums","layerId":"clap","lengthSteps":16,"notes":notes(&[(4,"D#2",1,0.85),(12,"D#2",1,0.9)])}),
            ],
        ),
        0 => (
            "Groove".into(),
            "The bassline moves to F, a 20-step arp starts phasing against the bar.".into(),
            vec![
                json!({"op":"layer.upsert","trackId":"bass","layerId":"line","notes":bass_b}),
                json!({"op":"track.upsert","trackId":"arp","engineId":"synth.subtractive","presetId":"preset:synth.subtractive:arp:arp_square@v1.0","gainDb":-13.0}),
                json!({"op":"layer.upsert","trackId":"arp","layerId":"seq","lengthSteps":20,"notes":notes(&[(0,"A4",1,0.7),(3,"E5",1,0.5),(6,"C5",1,0.6),(10,"G4",1,0.5),(13,"E5",1,0.6),(16,"D5",1,0.5)])}),
            ],
        ),
        _ => (
            "Variation".into(),
            "Back to A, a tom fill closes the phrase.".into(),
            vec![
                json!({"op":"layer.upsert","trackId":"bass","layerId":"line","notes":bass_a}),
                json!({"op":"clip.schedule","trackId":"drums","notes":[
                    {"tick":start+3840*3+2880,"pitch":"D3","durTicks":240,"velocity":0.8},
                    {"tick":start+3840*3+3120,"pitch":"B2","durTicks":240,"velocity":0.8},
                    {"tick":start+3840*3+3360,"pitch":"G2","durTicks":240,"velocity":0.85},
                    {"tick":start+3840*3+3600,"pitch":"F2","durTicks":240,"velocity":0.9}]}),
            ],
        ),
    }
}

impl Llm for MockPerformer {
    fn fork(&self) -> Box<dyn Llm + Send> {
        Box::new(self.clone())
    }

    fn chat(&mut self, messages: &[Msg]) -> Result<Reply, String> {
        let first_user = messages.iter().find(|m| m.role == "user").map(|m| m.content.as_str()).unwrap_or("");
        let (n, total, start) = parse_section(first_user);
        if self.delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(self.delay_ms));
        }
        let a = {
            let mut m = self.attempts.lock().unwrap();
            let a = m.entry(n).or_insert(0);
            *a += 1;
            *a
        };
        if self.fail_first && a == 1 {
            // Invalid op on purpose: engine must reject it, agent must retry.
            return Ok(Reply {
                content: r#"{"title":"oops","commentary":"bad","ops":[{"op":"layer.upsert","trackId":"ghost","layerId":"x"}]}"#.into(),
                model: "mock".into(),
                elapsed_ms: 0,
            });
        }
        let (mut title, mut commentary, mut ops) = section_ops(n, total, start);
        // Respond to the listener: acknowledge and thin out the drums.
        let mut reply = String::new();
        if let Some(i) = first_user.find("THE LISTENER JUST SAID") {
            let said = first_user[i..].split_once(':').map(|x| x.1).unwrap_or("").lines().next().unwrap_or("").trim();
            reply = format!("收到：{said}");
            title = "Answer".into();
            commentary = "Hats out, softer kick, as asked.".into();
            ops = vec![json!({"op":"layer.upsert","trackId":"drums","layerId":"hat","mute":true})];
        }
        Ok(Reply {
            content: json!({"title": title, "commentary": commentary, "bars": 4, "reply": reply, "ops": ops}).to_string(),
            model: "mock".into(),
            elapsed_ms: 0,
        })
    }
}

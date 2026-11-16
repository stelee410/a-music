//! AI performer: drives the engine through JSON transactions using an LLM
//! (agentllm gateway, OpenAI-compatible) and streams rendered audio sections.

pub mod jam;
pub mod llm;
pub mod mock;
pub mod perform;
pub mod prompt;

pub use jam::{jam, JamConfig, SongContext, SongSection};
pub use llm::{HttpLlm, Llm, Msg, Reply};
pub use mock::MockPerformer;
pub use perform::{parse_command, perform, Command, PerformConfig, PerformState};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn cfg(root: &std::path::Path, phrases: u32) -> PerformConfig {
        let mut c = PerformConfig::new(root, "dark minimal techno");
        c.phrases = phrases;
        c.default_bars = 4;
        c.seed = 1;
        c.max_retries = 2;
        c
    }

    fn no_cmds() -> std::sync::mpsc::Receiver<Command> {
        std::sync::mpsc::channel().1
    }

    fn read(p: &std::path::Path) -> Vec<f32> {
        hound::WavReader::open(p).unwrap().samples::<f32>().map(|s| s.unwrap()).collect()
    }

    #[test]
    fn mock_performance_end_to_end() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("perf");
        let mut events: Vec<Value> = vec![];
        let llm = MockPerformer { fail_first: true, ..Default::default() };
        let st = perform(&cfg(&root, 4), Box::new(llm), no_cmds(), &mut |e| events.push(e)).unwrap();
        assert_eq!(st.phrases.len(), 4);
        assert!(st.phrases.iter().all(|s| s.applied), "{:#?}", st.phrases);
        // each phrase first rejected (UNKNOWN_TRACK), then applied
        let rejected: Vec<&Value> = events.iter().filter(|e| e["type"] == "txn_rejected").collect();
        assert_eq!(rejected.len(), 4);
        assert!(rejected.iter().all(|e| e["code"] == "UNKNOWN_TRACK"));
        // phrases start on consecutive 4-bar boundaries, exactly as planned in batch mode
        let ticks: Vec<u64> = st.phrases.iter().map(|p| p.start_tick).collect();
        assert_eq!(ticks, vec![0, 15_360, 30_720, 46_080]);
        let chunks: Vec<&Value> = events.iter().filter(|e| e["type"] == "chunk").collect();
        assert_eq!(chunks.len(), 16, "one chunk per bar");
        let per_bar = (4.0 * 60.0 / 118.0 * 48000.0_f64).round() as i64;
        let first = chunks[0]["samples"].as_i64().unwrap();
        assert!((first - per_bar).abs() <= 1);
        assert!(chunks[15]["last"].as_bool().unwrap() && chunks[15]["samples"].as_i64().unwrap() > per_bar);
        assert_eq!(events.last().unwrap()["type"], "done");
        assert!(root.join("renders/full/mix.wav").is_file());
        assert!(root.join("renders/full/stems/drums.wav").is_file());

        // Seamless: concatenated chunks == one cumulative render of the same span.
        let mut cat = vec![];
        for c in &chunks[..15] {
            cat.extend(read(std::path::Path::new(c["wav"].as_str().unwrap())));
        }
        let p = agb_engine::ProjectStore::new(&root).load().unwrap();
        let full = agb_render::render(
            &p,
            &agb_render::RenderRequest {
                from_tick: 0,
                to_tick: chunks[15]["endTick"].as_u64().unwrap(),
                tail_ms: 0,
                seed: 1,
                stems: false,
                format: agb_protocol::SampleFormat::F32,
            },
        )
        .unwrap();
        assert_eq!(&full.mix[..cat.len()], &cat[..], "chunks must join seamlessly");

        // Continue: two more phrases appended after the rendered audio.
        let mut c2 = cfg(&root, 2);
        c2.continue_existing = true;
        let st = perform(&c2, Box::new(MockPerformer::default()), no_cmds(), &mut |_| {}).unwrap();
        assert_eq!(st.phrases.len(), 6);
        assert_eq!(st.phrases[4].start_tick, 61_440);
    }

    /// Slow AI in realtime mode: audio keeps flowing, changes land late on bar lines.
    #[test]
    fn realtime_slow_ai_keeps_looping() {
        #[derive(Clone)]
        struct Slow(MockPerformer);
        impl Llm for Slow {
            fn fork(&self) -> Box<dyn Llm + Send> {
                Box::new(self.clone())
            }
            fn chat(&mut self, m: &[Msg]) -> Result<Reply, String> {
                let first = m.iter().any(|x| x.content.contains("PHRASE 1 of"));
                if !first {
                    std::thread::sleep(std::time::Duration::from_millis(2600));
                }
                self.0.chat(m)
            }
        }
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("rt");
        let mut c = cfg(&root, 2);
        c.realtime = true;
        c.default_bars = 2;
        c.lookahead_bars = 1;
        // Tempo 400 BPM would be ideal for a fast test; the mock sets 118 → 1 bar ≈ 2.03 s.
        let mut events = vec![];
        let t0 = std::time::Instant::now();
        let st = perform(&c, Box::new(Slow(MockPerformer::default())), no_cmds(), &mut |e| events.push(e)).unwrap();
        assert_eq!(st.phrases.len(), 2);
        // Mock asks for 4-bar phrases; the reply for phrase 2 arrives after ~2.6 s, well
        // before bar 5, so it lands on time.
        assert_eq!(st.phrases[1].start_tick, 4 * 3840);
        // Wall clock pacing: rendering 8+ bars took real time (≥ ~6 bars ≈ 12 s minus lookahead).
        assert!(t0.elapsed().as_secs_f64() > 8.0, "{:?}", t0.elapsed());
        assert!(events.iter().any(|e| e["type"] == "change" && e["lateBars"] == 0));
    }

    /// Listener chat cancels the pending phrase, the AI answers and the change lands on
    /// the next free bar; a human tempo change enters the tempo map; finish ends early.
    #[test]
    fn interactive_chat_tempo_finish() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("live");
        let mut c = cfg(&root, 8);
        c.realtime = true;
        c.lookahead_bars = 1;
        let (tx, rx) = std::sync::mpsc::channel();
        let llm = MockPerformer { delay_ms: 300, ..Default::default() };
        let mut events: Vec<Value> = vec![];
        let mut sent = 0;
        let st = perform(&c, Box::new(llm), rx, &mut |e| {
            if e["type"] == "chunk" {
                sent += 1;
                match sent {
                    2 => tx.send(Command::Chat("安静一点".into())).unwrap(),
                    4 => tx.send(Command::SetTempo(90.0)).unwrap(),
                    5 => tx.send(Command::HumanState(serde_json::json!({"binaural": {"carrierHz": 200, "beatHz": 6}}))).unwrap(),
                    6 => tx.send(Command::Finish).unwrap(),
                    _ => {}
                }
            }
            events.push(e);
        })
        .unwrap();
        // AI answered the listener
        assert!(events.iter().any(|e| e["type"] == "chat" && e["who"] == "ai" && e["text"].as_str().unwrap().contains("安静一点")));
        let ans = events.iter().find(|e| e["type"] == "change" && e["answer"] == true).expect("answer change");
        assert_eq!(ans["title"], "Answer");
        // human tempo change committed on a bar line and in the tempo map
        let hc = events.iter().find(|e| e["type"] == "human_change").expect("tempo change");
        assert_eq!(hc["tick"].as_u64().unwrap() % 3840, 0);
        let p = agb_engine::ProjectStore::new(&root).load().unwrap();
        assert_eq!(p.tempo_map.last().unwrap().bpm, 90.0);
        assert!(events.iter().any(|e| e["type"] == "chunk" && e["tempoBpm"] == 90.0));
        // finish: far fewer than 8 phrases, conversation persisted
        assert!(events.iter().any(|e| e["type"] == "finishing"));
        assert!(st.phrases.len() < 8, "{}", st.phrases.len());
        assert!(st.dialogue.iter().any(|l| l.who == "listener") && st.dialogue.iter().any(|l| l.who == "ai"));
        assert!(events.iter().any(|e| e["type"] == "chunk" && !e["stems"].as_array().unwrap().is_empty()));
    }

    #[test]
    fn command_parsing() {
        assert_eq!(parse_command(r#"{"type":"chat","text":" hi "}"#), Some(Command::Chat("hi".into())));
        assert_eq!(parse_command(r#"{"type":"chat","text":"  "}"#), None);
        assert_eq!(parse_command(r#"{"type":"set_tempo","bpm":88}"#), Some(Command::SetTempo(88.0)));
        assert_eq!(parse_command(r#"{"type":"finish"}"#), Some(Command::Finish));
        assert_eq!(parse_command(r#"{"type":"pause"}"#), Some(Command::Pause));
        assert_eq!(parse_command(r#"{"type":"resume"}"#), Some(Command::Resume));
        assert!(matches!(parse_command(r#"{"type":"human_state","state":{"a":1}}"#), Some(Command::HumanState(_))));
        assert_eq!(parse_command("garbage"), None);
    }

    #[test]
    fn refuses_existing_project_and_forbidden_ops() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("perf");
        agb_engine::ProjectStore::new(&root).init(None).unwrap();
        assert!(perform(&cfg(&root, 1), Box::new(MockPerformer::default()), no_cmds(), &mut |_| {}).is_err());

        struct Bad;
        impl Llm for Bad {
            fn fork(&self) -> Box<dyn Llm + Send> {
                Box::new(Bad)
            }
            fn chat(&mut self, _: &[Msg]) -> Result<Reply, String> {
                Ok(Reply { content: r#"{"ops":[{"op":"system.panic"}]}"#.into(), model: "bad".into(), elapsed_ms: 0 })
            }
        }
        let root2 = d.path().join("perf2");
        let mut ev = vec![];
        let r = perform(&cfg(&root2, 1), Box::new(Bad), no_cmds(), &mut |e| ev.push(e));
        assert!(r.is_err(), "no usable opening phrase");
        assert!(ev.iter().any(|e| e["code"] == "BAD_REPLY"));
    }

    #[test]
    fn relative_ticks_are_shifted() {
        let v: Value = serde_json::from_str(r#"{"ops":[
            {"op":"automation.schedule","trackId":"b","param":"x","mode":"ramp","fromTick":0,"toTick":3840,"from":0,"to":1},
            {"op":"clip.schedule","trackId":"b","notes":[{"tick":960,"pitch":"C3","durTicks":10}]}]}"#).unwrap();
        let ops = perform::prepare_ops(&v, 1000).unwrap();
        assert_eq!(ops[0]["fromTick"], 1000);
        assert_eq!(ops[0]["toTick"], 4840);
        assert_eq!(ops[1]["notes"][0]["tick"], 1960);
        let tempo: Value = serde_json::from_str(r#"{"ops":[{"op":"project.setTempo","tempoBpm":90}]}"#).unwrap();
        assert!(perform::prepare_ops(&tempo, 7680).is_ok(), "tempo map: AI may change tempo later");
        let bad: Value = serde_json::from_str(r#"{"ops":[{"op":"system.panic"}]}"#).unwrap();
        assert!(perform::prepare_ops(&bad, 0).is_err());
    }

    #[test]
    fn system_prompt_lists_engines_and_presets() {
        let s = prompt::system_prompt();
        for needle in ["drum.analog808style", "filter.cutoffHz", "preset:synth.moogish:bass:acid_ladder@v1.0", "bd.decayMs", "RELATIVE"] {
            assert!(s.contains(needle), "{needle}");
        }
    }
}

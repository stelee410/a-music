//! OpenAI-compatible chat client (agentllm gateway) with model fallback.
//!
//! The LLM is only ever called from the control plane, *before* a transaction
//! is committed (SPEC §1). Rendering never touches the network.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

pub const DEFAULT_BASE_URL: &str = "https://agentllm.linkyun.co";
pub const DEFAULT_MODELS: &[&str] = &["c-music-express", "gemini-3-flash-preview", "gpt-5-mini", "deepseek-v4-pro"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Msg {
    pub role: String,
    pub content: String,
}

impl Msg {
    pub fn system(c: impl Into<String>) -> Self {
        Msg { role: "system".into(), content: c.into() }
    }
    pub fn user(c: impl Into<String>) -> Self {
        Msg { role: "user".into(), content: c.into() }
    }
    pub fn assistant(c: impl Into<String>) -> Self {
        Msg { role: "assistant".into(), content: c.into() }
    }
}

/// A chat completion: content plus the model that actually answered.
#[derive(Clone, Debug)]
pub struct Reply {
    pub content: String,
    pub model: String,
    pub elapsed_ms: u128,
}

pub trait Llm {
    fn chat(&mut self, messages: &[Msg]) -> Result<Reply, String>;
    /// An independent handle for a concurrent request.
    fn fork(&self) -> Box<dyn Llm + Send>;
}

#[derive(Clone, Debug)]
pub struct HttpLlm {
    pub base_url: String,
    pub api_key: String,
    /// Tried in order until one answers.
    pub models: Vec<String>,
    pub timeout: Duration,
    pub max_tokens: u32,
}

impl HttpLlm {
    pub fn new(base_url: Option<String>, api_key: String, models: Vec<String>) -> Self {
        HttpLlm {
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            api_key,
            models: if models.is_empty() { DEFAULT_MODELS.iter().map(|s| s.to_string()).collect() } else { models },
            timeout: Duration::from_secs(150),
            max_tokens: 8000,
        }
    }

    pub fn endpoint(&self) -> String {
        let b = self.base_url.trim_end_matches('/');
        if b.ends_with("/v1") {
            format!("{b}/chat/completions")
        } else {
            format!("{b}/v1/chat/completions")
        }
    }

    fn call(&self, model: &str, messages: &[Msg]) -> Result<String, String> {
        let agent = ureq::AgentBuilder::new().timeout(self.timeout).build();
        let mut body = json!({ "model": model, "messages": messages, "max_tokens": self.max_tokens });
        if !model.starts_with("gpt-5") {
            body["temperature"] = json!(0.9);
        }
        let resp = agent
            .post(&self.endpoint())
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .send_json(body);
        let v: Value = match resp {
            Ok(r) => r.into_json().map_err(|e| format!("{model}: bad JSON body: {e}"))?,
            Err(ureq::Error::Status(code, r)) => {
                let t = r.into_string().unwrap_or_default();
                return Err(format!("{model}: HTTP {code}: {}", t.chars().take(300).collect::<String>()));
            }
            Err(e) => return Err(format!("{model}: {e}")),
        };
        let content = v["choices"][0]["message"]["content"].as_str().unwrap_or("").trim().to_string();
        if content.is_empty() {
            return Err(format!(
                "{model}: empty reply (finish_reason={})",
                v["choices"][0]["finish_reason"].as_str().unwrap_or("?")
            ));
        }
        Ok(content)
    }
}

impl Llm for HttpLlm {
    fn fork(&self) -> Box<dyn Llm + Send> {
        Box::new(self.clone())
    }

    fn chat(&mut self, messages: &[Msg]) -> Result<Reply, String> {
        let mut errors = vec![];
        for (i, m) in self.models.clone().iter().enumerate() {
            let t0 = std::time::Instant::now();
            match self.call(m, messages) {
                Ok(content) => {
                    // Prefer the model that worked from now on.
                    if i > 0 {
                        let good = self.models.remove(i);
                        self.models.insert(0, good);
                    }
                    return Ok(Reply { content, model: m.clone(), elapsed_ms: t0.elapsed().as_millis() });
                }
                Err(e) => errors.push(e),
            }
        }
        Err(format!("all models failed: {}", errors.join(" | ")))
    }
}

/// Extract the first top-level JSON object from an LLM reply, repairing
/// common transport damage:
/// * markdown fences / prose around the object;
/// * raw line breaks injected *inside* strings (dropped — valid JSON never
///   contains raw control characters in strings; some gateways insert `\n`
///   at stream-chunk boundaries, even mid-word);
/// * line breaks injected *between* two scalar characters outside strings
///   (`96\n0` → `960`; valid JSON never has adjacent scalars);
/// * trailing commas before `}` / `]`.
pub fn extract_json(text: &str) -> Result<Value, String> {
    let Some(start) = text.find('{') else { return Err("no JSON object in reply".into()) };
    let chars: Vec<char> = text[start..].chars().collect();
    let is_scalar = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+');
    let mut out = String::with_capacity(chars.len());
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &ch) in chars.iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
                out.push(ch);
            } else if ch == '\\' {
                esc = true;
                out.push(ch);
            } else if ch == '"' {
                in_str = false;
                out.push(ch);
            } else if ch == '\t' {
                out.push_str("\\t");
            } else if (ch as u32) >= 0x20 {
                out.push(ch);
            } // other control characters (\n, \r, …) are dropped
            continue;
        }
        match ch {
            '\n' | '\r' => {
                let prev = out.chars().last();
                let next = chars[i + 1..].iter().find(|c| **c != '\n' && **c != '\r');
                if prev.is_some_and(is_scalar) && next.is_some_and(|c| is_scalar(*c)) {
                    continue; // re-join a split number / literal
                }
                out.push(' ');
                continue;
            }
            '"' => in_str = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                let trimmed = out.trim_end().len();
                if out[..trimmed].ends_with(',') {
                    out.truncate(trimmed - 1);
                }
                depth -= 1;
            }
            _ => {}
        }
        out.push(ch);
        if depth == 0 {
            return serde_json::from_str(&out).map_err(|e| format!("reply JSON invalid: {e}"));
        }
    }
    Err("unterminated JSON object in reply".into())
}

/// Read the API key: explicit file → `AGENTLLM_API_KEY` env.
pub fn load_api_key(file: Option<&std::path::Path>) -> Result<String, String> {
    if let Some(f) = file {
        let k = std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
        let k = k.trim().to_string();
        if k.is_empty() {
            return Err(format!("{} is empty", f.display()));
        }
        return Ok(k);
    }
    std::env::var("AGENTLLM_API_KEY")
        .map(|k| k.trim().to_string())
        .ok()
        .filter(|k| !k.is_empty())
        .ok_or_else(|| "no API key: set AGENTLLM_API_KEY or pass --api-key-file".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_noise() {
        let v = extract_json("Sure!\n```json\n{\"a\": \"}{\", \"b\": {\"c\": 1}}\n```\nbye").unwrap();
        assert_eq!(v["b"]["c"], 1);
        assert_eq!(v["a"], "}{");
        assert!(extract_json("nothing here").is_err());
        let v = extract_json("{\"c\": \"line1\nline2\", \"ops\": [1, 2,],}").unwrap();
        assert_eq!(v["c"], "line1line2");
        assert_eq!(v["ops"][1], 2);
        // Real damage seen from the gateway: newlines injected at chunk boundaries.
        let damaged = "{\"title\n\": \"rainy\", \"ops\": [{\"op\": \"project.\nsetTempo\", \"tempoBpm\": 8\n4},\n {\"op\": \"track.upsert\", \"\ntrackId\": \"drums\", \"gainDb\":\n -4, \"mute\": fal\nse}]}";
        let v = extract_json(damaged).unwrap();
        assert_eq!(v["title"], "rainy");
        assert_eq!(v["ops"][0]["op"], "project.setTempo");
        assert_eq!(v["ops"][0]["tempoBpm"], 84);
        assert_eq!(v["ops"][1]["trackId"], "drums");
        assert_eq!(v["ops"][1]["gainDb"], -4);
        assert_eq!(v["ops"][1]["mute"], false);
        assert!(extract_json("{\"a\": ").is_err());
    }

    #[test]
    fn endpoint_forms() {
        let mut l = HttpLlm::new(Some("https://x.y".into()), "k".into(), vec![]);
        assert_eq!(l.endpoint(), "https://x.y/v1/chat/completions");
        l.base_url = "https://x.y/v1/".into();
        assert_eq!(l.endpoint(), "https://x.y/v1/chat/completions");
        assert_eq!(l.models[0], "c-music-express");
    }
}

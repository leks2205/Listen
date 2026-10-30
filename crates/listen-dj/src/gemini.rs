//! Minimal Gemini client (Interactions API) for the two calls the DJ makes.
//! The API key travels in the `x-goog-api-key` header and is never logged.

use crate::persona::Persona;
use crate::prompt::{self, Plan, PlanRequest, Turn};
use base64::Engine;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1beta/interactions";

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DjError {
    /// 429: out of quota. `daily` = per-day (free tier) limit, else per-minute; `retry_after` from the server.
    #[error("Gemini quota exhausted ({})", if *.daily { "daily limit" } else { "rate limit" })]
    Quota { daily: bool, retry_after: Option<Duration> },
    #[error("the Gemini API key was rejected")]
    Auth,
    #[error("no Gemini API key set")]
    NoKey,
    #[error("Gemini: {0}")]
    Api(String),
    #[error("network: {0}")]
    Network(String),
}

/// Token usage of one call (for the log and cost estimates).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub thought: u64,
}

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} in / {} out / {} thinking tokens", self.input, self.output, self.thought)
    }
}

#[derive(Clone)]
pub struct Gemini {
    http: reqwest::Client,
    key: String,
    endpoint: String,
}

impl std::fmt::Debug for Gemini {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gemini").finish_non_exhaustive()
    }
}

impl Gemini {
    pub fn new(http: reqwest::Client, key: impl Into<String>) -> Result<Self, DjError> {
        let key = key.into().trim().to_string();
        if key.is_empty() {
            return Err(DjError::NoKey);
        }
        Ok(Self { http, key, endpoint: ENDPOINT.into() })
    }

    /// Points the client at another endpoint (tests).
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    /// Asks the text model for the next set and the break line.
    pub async fn plan(&self, model: &str, req: &PlanRequest<'_>) -> Result<(Plan, Usage), DjError> {
        let body = json!({
            "model": model,
            "system_instruction": prompt::system_instruction(req.persona, &req.show),
            "input": prompt::user_prompt(req),
            "response_format": {
                "type": "text",
                "mime_type": "application/json",
                "schema": prompt::response_schema(req.persona, req.set_min, req.set_max),
            },
            // No sampling parameters: ignored since 3.6 Flash, and newer models reject them (400).
            "generation_config": { "thinking_level": "minimal" },
            "store": false,
        });
        let started = Instant::now();
        let reply = self.post(&body, Duration::from_secs(45)).await?;
        let usage = usage(&reply);
        let text = outputs(&reply, "text").filter_map(|c| c["text"].as_str()).collect::<String>();
        let plan = prompt::parse_plan(&text, req.candidates.len(), req.set_min, req.set_max)
            .map_err(DjError::Api)?;
        tracing::info!(
            "AI DJ plan from {model} in {:.1}s ({usage}): {} songs, mood “{}”",
            started.elapsed().as_secs_f32(),
            plan.picks.len(),
            plan.mood
        );
        Ok((plan, usage))
    }

    /// Speaks the break (one voice, or host and co-host in one request). Returns WAV bytes.
    /// `say` lists (word, how to pronounce it) hints for lines that contain the word.
    pub async fn speak(
        &self,
        model: &str,
        persona: &Persona,
        turns: &[Turn],
        say: &[(&str, &str)],
    ) -> Result<(Vec<u8>, Usage), DjError> {
        let body = speech_body(model, persona, turns, say);
        let started = Instant::now();
        let reply = self.post(&body, Duration::from_secs(90)).await?;
        let usage = usage(&reply);
        let audio = outputs(&reply, "audio")
            .filter_map(|c| c["data"].as_str())
            .last()
            .ok_or_else(|| DjError::Api("no audio in the reply".into()))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(audio.trim())
            .map_err(|e| DjError::Api(format!("bad audio data: {e}")))?;
        tracing::info!(
            "AI DJ speech from {model} in {:.1}s ({usage}, {} KiB)",
            started.elapsed().as_secs_f32(),
            bytes.len() / 1024
        );
        Ok((bytes, usage))
    }

    async fn post(&self, body: &Value, timeout: Duration) -> Result<Value, DjError> {
        let resp = self
            .http
            .post(&self.endpoint)
            .header("x-goog-api-key", &self.key)
            .timeout(timeout)
            .json(body)
            .send()
            .await
            .map_err(|e| DjError::Network(e.without_url().to_string()))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| DjError::Network(e.without_url().to_string()))?;
        if status.is_success() {
            let reply: Value =
                serde_json::from_str(&text).map_err(|e| DjError::Api(format!("unreadable reply: {e}")))?;
            if let Some(s) = reply["status"].as_str().filter(|s| *s != "completed") {
                return Err(DjError::Api(format!("interaction {s}")));
            }
            return Ok(reply);
        }
        Err(classify_error(status.as_u16(), &text))
    }
}

/// The TTS request for these turns (see `speak`).
pub fn speech_body(model: &str, persona: &Persona, turns: &[Turn], say: &[(&str, &str)]) -> Value {
    let cohost = persona.cohost.as_ref();
    let is_cohost = |t: &Turn| cohost.is_some_and(|c| t.speaker.eq_ignore_ascii_case(c.name));
    let two_voices = cohost.is_some() && turns.iter().any(is_cohost) && turns.iter().any(|t| !is_cohost(t));
    let content: Vec<Value> = turns
        .iter()
        .map(|t| {
            let (name, base) = match cohost {
                Some(c) if is_cohost(t) => (c.name, c.style),
                _ => (persona.host, persona.style),
            };
            let mut style =
                if t.style.trim().is_empty() { base.to_string() } else { format!("{base}; {}", t.style) };
            let text = t.text.to_lowercase();
            for (word, how) in say.iter().filter(|(w, h)| !w.is_empty() && !h.is_empty()) {
                if text.contains(&word.to_lowercase()) {
                    style.push_str(&format!("; \"{word}\" is pronounced {how}"));
                }
            }
            let mut meta = json!({ "type": "speech_metadata", "style": style });
            if two_voices {
                meta["speaker"] = json!(name);
            }
            json!({ "type": "text", "text": t.text, "annotations": [meta] })
        })
        .collect();
    let speech_config = match cohost {
        Some(c) if two_voices => json!({
            "speakers": [
                { "speaker": persona.host, "voice": persona.voice },
                { "speaker": c.name, "voice": c.voice },
            ],
        }),
        // One speaker: the co-host alone gets her own voice.
        Some(c) if turns.iter().all(is_cohost) => json!([{ "voice": c.voice }]),
        _ => json!([{ "voice": persona.voice }]),
    };
    json!({
        "model": model,
        "input": [{ "type": "user_input", "content": content }],
        "response_format": { "type": "audio", "mime_type": "audio/wav" },
        "generation_config": { "speech_config": speech_config },
        "store": false,
    })
}

/// Content items of the model's output steps with this `type`.
fn outputs<'a>(reply: &'a Value, kind: &'a str) -> impl Iterator<Item = &'a Value> + 'a {
    reply["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["type"].as_str().is_none_or(|t| t == "model_output"))
        .flat_map(|s| s["content"].as_array().into_iter().flatten())
        .filter(move |c| c["type"] == kind)
}

fn usage(reply: &Value) -> Usage {
    let u = &reply["usage"];
    Usage {
        input: u["total_input_tokens"].as_u64().unwrap_or(0),
        output: u["total_output_tokens"].as_u64().unwrap_or(0),
        thought: u["total_thought_tokens"].as_u64().unwrap_or(0),
    }
}

/// Maps an error reply to a [`DjError`].
pub fn classify_error(status: u16, body: &str) -> DjError {
    let json: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let err = &json["error"];
    let message = err["message"].as_str().unwrap_or(body).chars().take(300).collect::<String>();
    let api_status = err["status"].as_str().unwrap_or_default();
    if status == 429 || api_status == "RESOURCE_EXHAUSTED" {
        let daily = body.contains("PerDay") || body.contains("per day");
        let retry_after = err["details"]
            .as_array()
            .into_iter()
            .flatten()
            .find_map(|d| d["retryDelay"].as_str())
            .and_then(|s| s.trim_end_matches('s').parse::<f64>().ok())
            .map(Duration::from_secs_f64);
        return DjError::Quota { daily, retry_after };
    }
    if status == 401
        || status == 403
        || body.contains("API_KEY_INVALID")
        || message.contains("API key not valid")
    {
        return DjError::Auth;
    }
    DjError::Api(format!("HTTP {status}: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_quota_errors() {
        let daily = r#"{"error":{"code":429,"message":"You exceeded your current quota","status":"RESOURCE_EXHAUSTED",
            "details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaId":"GenerateRequestsPerDayPerProjectPerModel-FreeTier"}]},
                       {"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"41s"}]}}"#;
        assert_eq!(
            classify_error(429, daily),
            DjError::Quota { daily: true, retry_after: Some(Duration::from_secs(41)) }
        );
        let minute = r#"{"error":{"code":429,"status":"RESOURCE_EXHAUSTED","message":"rate","details":[{"violations":[{"quotaId":"GenerateRequestsPerMinutePerProjectPerModel-FreeTier"}]}]}}"#;
        assert_eq!(classify_error(429, minute), DjError::Quota { daily: false, retry_after: None });
        let key = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT","details":[{"reason":"API_KEY_INVALID"}]}}"#;
        assert_eq!(classify_error(400, key), DjError::Auth);
        assert!(matches!(classify_error(500, "oops"), DjError::Api(m) if m.contains("500")));
    }

    #[test]
    fn reads_text_and_audio_outputs() {
        let reply = json!({
            "status": "completed",
            "steps": [
                {"type": "thought", "content": [{"type": "text", "text": "hmm"}]},
                {"type": "model_output", "content": [{"type": "text", "text": "{\"a\":"}, {"type": "text", "text": "1}"}]},
                {"type": "model_output", "content": [{"type": "audio", "mime_type": "audio/wav", "data": "UklGRg=="}]}
            ],
            "usage": {"total_input_tokens": 7, "total_output_tokens": 20, "total_thought_tokens": 3}
        });
        let text: String = outputs(&reply, "text").filter_map(|c| c["text"].as_str()).collect();
        assert_eq!(text, "{\"a\":1}");
        assert_eq!(outputs(&reply, "audio").count(), 1);
        assert_eq!(usage(&reply), Usage { input: 7, output: 20, thought: 3 });
    }

    #[test]
    fn speech_requests_for_one_or_two_hosts() {
        let turn =
            |who: &str, text: &str| Turn { speaker: who.into(), text: text.into(), style: "excited".into() };
        let duo = Persona::by_id("hamburg_duo");
        let body = speech_body("m", duo, &[turn("Hauke", "Moin!"), turn("Merle", "Moin zurück!")], &[]);
        assert!(body["generation_config"]["speech_config"].get("mode").is_none(), "the API rejects `mode`");
        assert_eq!(body["generation_config"]["speech_config"]["speakers"][1]["voice"], "Laomedeia");
        assert_eq!(body["input"][0]["content"][1]["annotations"][0]["speaker"], "Merle");
        // The host alone: a plain single-voice request.
        let body = speech_body("m", duo, &[turn("Hauke", "Moin!")], &[]);
        assert_eq!(body["generation_config"]["speech_config"][0]["voice"], "Fenrir");
        assert!(body["input"][0]["content"][0]["annotations"][0].get("speaker").is_none());
        // Solo personas ignore speaker names.
        let body = speech_body("m", Persona::by_id("calm"), &[turn("", "Ruhig.")], &[]);
        assert_eq!(body["generation_config"]["speech_config"][0]["voice"], "Kore");
        // Lines naming a word with a pronunciation get the hint.
        let style = |b: &Value| b["input"][0]["content"][0]["annotations"][0]["style"].to_string();
        let say = [("Kiel", "Keel")];
        assert!(
            !style(&speech_body("m", Persona::by_id("calm"), &[turn("", "Ruhig.")], &say)).contains("Keel")
        );
        let body = speech_body("m", Persona::by_id("calm"), &[turn("", "Greetings to kiel.")], &say);
        assert!(style(&body).contains("\\\"Kiel\\\" is pronounced Keel"), "{}", style(&body));
    }

    #[test]
    fn empty_key_is_rejected() {
        assert_eq!(Gemini::new(reqwest::Client::new(), "  ").unwrap_err(), DjError::NoKey);
    }
}

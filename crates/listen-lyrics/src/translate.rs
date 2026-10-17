//! Online translation (Feishin's `lyric-translate.ts`): the whole lyrics text goes to
//! Microsoft Azure Translator or Google Cloud Translation in one request, with the
//! user's own API key, and comes back as text with the same line breaks.

use serde_json::{Value, json};
use std::time::Duration;

pub const PROVIDERS: [&str; 2] = ["Microsoft Azure", "Google Cloud"];

/// Target languages offered in the settings: (code, name).
pub const LANGUAGES: &[(&str, &str)] = &[
    ("ar", "Arabic"),
    ("bg", "Bulgarian"),
    ("zh-Hans", "Chinese (Simplified)"),
    ("zh-Hant", "Chinese (Traditional)"),
    ("cs", "Czech"),
    ("da", "Danish"),
    ("nl", "Dutch"),
    ("en", "English"),
    ("fi", "Finnish"),
    ("fr", "French"),
    ("de", "German"),
    ("el", "Greek"),
    ("he", "Hebrew"),
    ("hi", "Hindi"),
    ("hu", "Hungarian"),
    ("id", "Indonesian"),
    ("it", "Italian"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("nb", "Norwegian"),
    ("pl", "Polish"),
    ("pt", "Portuguese"),
    ("ro", "Romanian"),
    ("ru", "Russian"),
    ("sk", "Slovak"),
    ("es", "Spanish"),
    ("sv", "Swedish"),
    ("th", "Thai"),
    ("tr", "Turkish"),
    ("uk", "Ukrainian"),
    ("vi", "Vietnamese"),
];

/// A request, ready to send.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub url: String,
    /// Extra headers (name, value).
    pub headers: Vec<(&'static str, String)>,
    pub body: Value,
}

/// The request for `provider`, or `None` for an unknown provider or a missing key / language.
pub fn request(provider: &str, key: &str, target: &str, text: &str) -> Option<Request> {
    if key.trim().is_empty() || target.trim().is_empty() {
        return None;
    }
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    match provider {
        "Microsoft Azure" => Some(Request {
            url: format!(
                "https://api.cognitive.microsofttranslator.com/translate?api-version=3.0&to={}",
                enc(target)
            ),
            headers: vec![("Ocp-Apim-Subscription-Key", key.trim().to_string())],
            body: json!([{ "Text": text }]),
        }),
        "Google Cloud" => Some(Request {
            url: format!(
                "https://translation.googleapis.com/language/translate/v2?target={}&key={}",
                enc(target),
                enc(key.trim())
            ),
            headers: vec![],
            body: json!({ "format": "text", "q": text }),
        }),
        _ => None,
    }
}

/// The translated text out of a response body.
pub fn parse_response(provider: &str, body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let text = match provider {
        "Microsoft Azure" => v[0]["translations"][0]["text"].as_str()?,
        "Google Cloud" => v["data"]["translations"][0]["translatedText"].as_str()?,
        _ => return None,
    };
    Some(text.to_string())
}

/// Lays translated text back over the original lines (line for line; a missing or
/// identical line gives `None`).
pub fn split_lines(original: &[String], translated: &str) -> Vec<Option<String>> {
    let mut parts = translated.split('\n');
    original
        .iter()
        .map(|o| {
            let t = parts.next().map(str::trim).unwrap_or_default();
            (!t.is_empty() && t != o.trim()).then(|| t.to_string())
        })
        .collect()
}

/// Translates the lines; `None` when it fails or is not configured.
pub async fn translate(
    http: &reqwest::Client,
    provider: &str,
    key: &str,
    target: &str,
    lines: &[String],
) -> Option<Vec<Option<String>>> {
    let req = request(provider, key, target, &lines.join("\n"))?;
    let mut r = http.post(&req.url).timeout(Duration::from_secs(15)).json(&req.body);
    for (k, v) in &req.headers {
        r = r.header(*k, v);
    }
    let resp = r.send().await.ok()?;
    if !resp.status().is_success() {
        tracing::warn!("{provider} translation answered {}", resp.status());
        return None;
    }
    let text = parse_response(provider, &resp.text().await.ok()?)?;
    Some(split_lines(lines, &text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_azure_request() {
        let r = request("Microsoft Azure", " k ", "de", "a\nb").unwrap();
        assert_eq!(r.url, "https://api.cognitive.microsofttranslator.com/translate?api-version=3.0&to=de");
        assert_eq!(r.headers, [("Ocp-Apim-Subscription-Key", "k".to_string())]);
        assert_eq!(r.body, json!([{"Text": "a\nb"}]));
    }

    #[test]
    fn builds_the_google_request() {
        let r = request("Google Cloud", "k&y", "zh-Hans", "a").unwrap();
        assert_eq!(
            r.url,
            "https://translation.googleapis.com/language/translate/v2?target=zh-Hans&key=k%26y"
        );
        assert_eq!(r.body, json!({"format": "text", "q": "a"}));
        assert!(r.headers.is_empty());
    }

    #[test]
    fn nothing_without_provider_key_or_language() {
        assert!(request("", "k", "de", "a").is_none());
        assert!(request("Google Cloud", " ", "de", "a").is_none());
        assert!(request("Google Cloud", "k", "", "a").is_none());
        assert!(request("Bing", "k", "de", "a").is_none());
    }

    #[test]
    fn parses_responses() {
        assert_eq!(
            parse_response("Microsoft Azure", r#"[{"translations":[{"text":"Hallo\nTschüss","to":"de"}]}]"#)
                .unwrap(),
            "Hallo\nTschüss"
        );
        assert_eq!(
            parse_response("Google Cloud", r#"{"data":{"translations":[{"translatedText":"Hola"}]}}"#)
                .unwrap(),
            "Hola"
        );
        assert!(parse_response("Google Cloud", r#"{"error":{"code":403}}"#).is_none());
        assert!(parse_response("Microsoft Azure", "oops").is_none());
    }

    #[test]
    fn lines_map_back_by_index() {
        let orig: Vec<String> = ["Hello", "", "Same", "Bye"].map(String::from).to_vec();
        let out = split_lines(&orig, "Hallo\n\nSame\nTschüss");
        assert_eq!(out, [Some("Hallo".into()), None, None, Some("Tschüss".into())]);
        // Fewer lines back than sent.
        assert_eq!(split_lines(&orig, "Hallo"), [Some("Hallo".into()), None, None, None]);
    }

    #[test]
    fn languages_include_the_default() {
        assert!(LANGUAGES.iter().any(|(c, _)| *c == "en"));
    }
}

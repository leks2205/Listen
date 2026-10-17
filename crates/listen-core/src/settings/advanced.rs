//! Advanced tab: updates, log level and settings export / import. Secrets live in the keyring,
//! not in `Settings`; export and import still skip any key that looks like one.

use super::*;

/// Which releases the updater offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    /// Only full releases.
    #[default]
    Stable,
    /// Also pre-releases (GitHub "pre-release").
    Beta,
}

impl UpdateChannel {
    pub const ALL: [Self; 2] = [Self::Stable, Self::Beta];

    pub fn label(self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Beta => "Beta",
        }
    }
}

impl std::fmt::Display for UpdateChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// How much `listen.log` records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    /// Everything Listen's own crates say (the default, to help with bug reports).
    #[default]
    Debug,
}

impl LogLevel {
    pub const ALL: [Self; 2] = [Self::Info, Self::Debug];

    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Debug => "Debug",
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AdvancedSettings {
    /// Look for a new version when Listen starts.
    pub update_check_on_start: bool,
    pub update_channel: UpdateChannel,
    /// A version (tag) the user chose to skip.
    pub skipped_version: String,
    /// No automatic update dialog before this time (unix seconds): "Remind later".
    pub update_remind_after: i64,
    pub log_level: LogLevel,
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            update_check_on_start: true,
            update_channel: UpdateChannel::Stable,
            skipped_version: String::new(),
            update_remind_after: 0,
            log_level: LogLevel::Debug,
        }
    }
}

/// `format` marker of an exported file.
pub const EXPORT_FORMAT: &str = "listen-settings";

/// Whether a settings key holds a secret.
pub fn is_secret_key(key: &str) -> bool {
    let k = key.to_lowercase();
    ["password", "passwd", "secret", "token", "cookie", "api_key", "apikey", "credential", "lastfm_key"]
        .iter()
        .any(|s| k.contains(s))
}

fn strip_secrets(v: &mut Value) {
    match v {
        Value::Object(map) => {
            map.retain(|k, _| !is_secret_key(k));
            map.values_mut().for_each(strip_secrets);
        }
        Value::Array(a) => a.iter_mut().for_each(strip_secrets),
        _ => {}
    }
}

/// The settings as a JSON export file (without secrets).
pub fn export_json(settings: &Settings, app_version: &str) -> String {
    let mut value = serde_json::to_value(settings).unwrap_or(Value::Null);
    strip_secrets(&mut value);
    let file = serde_json::json!({ "format": EXPORT_FORMAT, "version": app_version, "settings": value });
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// What an import did.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportReport {
    pub settings: Settings,
    /// Number of values taken over.
    pub applied: usize,
    /// One line per value that was refused: `path: reason`.
    pub issues: Vec<String>,
}

fn apply(root: &mut Value, path: &mut Vec<String>, incoming: &Value, report: &mut (usize, Vec<String>)) {
    let mut candidate = root.clone();
    let mut slot = &mut candidate;
    for k in path.iter() {
        match slot.get_mut(k.as_str()) {
            Some(next) => slot = next,
            None => return,
        }
    }
    *slot = incoming.clone();
    match serde_json::from_value::<Settings>(candidate.clone()) {
        Ok(_) => {
            *root = candidate;
            report.0 += 1;
        }
        Err(e) => {
            let mut current = &*root;
            for k in path.iter() {
                current = current.get(k.as_str()).unwrap_or(&Value::Null);
            }
            if let (Value::Object(inc), Value::Object(_)) = (incoming, current) {
                for (k, v) in inc {
                    path.push(k.clone());
                    // An unknown nested key is ignored by serde; only known ones can fail.
                    let known = path.iter().try_fold(&*root, |c, k| c.get(k.as_str())).is_some();
                    if known {
                        apply(root, path, v, report);
                    }
                    path.pop();
                }
            } else {
                report.1.push(format!("{}: {e}", path.join(".")));
            }
        }
    }
}

/// Imports a settings export onto `base` key by key; values that don't fit are reported and skipped.
pub fn import_json(base: &Settings, json: &str) -> Result<ImportReport, String> {
    let file: Value = serde_json::from_str(json).map_err(|e| format!("not valid JSON: {e}"))?;
    let Value::Object(top) = &file else { return Err("not a settings file".into()) };
    let incoming = match top.get("settings") {
        Some(s) if top.get("format").and_then(Value::as_str) == Some(EXPORT_FORMAT) => s,
        _ if top.get("format").is_some() => return Err("not a Listen settings export".into()),
        _ if top.contains_key("general") && top.contains_key("playback") => {
            return Err("this is a Feishin export; use \"Import from Feishin\"".into());
        }
        _ => &file,
    };
    let Value::Object(incoming) = incoming else { return Err("\"settings\" is not an object".into()) };
    let mut root = serde_json::to_value(base).map_err(|e| e.to_string())?;
    let mut report = (0usize, Vec::new());
    for (k, v) in incoming {
        if is_secret_key(k) {
            report.1.push(format!("{k}: secrets are not imported"));
            continue;
        }
        if root.get(k).is_none() {
            report.1.push(format!("{k}: unknown setting"));
            continue;
        }
        let mut v = v.clone();
        strip_secrets(&mut v);
        apply(&mut root, &mut vec![k.clone()], &v, &mut report);
    }
    let mut settings: Settings = serde_json::from_value(root).map_err(|e| e.to_string())?;
    settings.sidebar.add_missing_items();
    for (action, key) in Settings::default().hotkeys {
        settings.hotkeys.entry(action).or_insert(key);
    }
    Ok(ImportReport { settings, applied: report.0, issues: report.1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_import_round_trip() {
        let mut s = Settings::default();
        s.advanced.update_channel = UpdateChannel::Beta;
        s.advanced.log_level = LogLevel::Info;
        s.history.enabled = false;
        let text = export_json(&s, "0.28.0");
        let r = import_json(&Settings::default(), &text).unwrap();
        assert!(r.issues.is_empty(), "{:?}", r.issues);
        assert_eq!(r.settings, s);
    }

    #[test]
    fn bad_value_is_reported_and_the_rest_is_kept() {
        let json = r#"{"format":"listen-settings","version":"1","settings":{
            "history":{"enabled":"banana","retention":"oneyear"},
            "advanced":{"update_channel":"beta"},
            "nonsense":1}}"#;
        let r = import_json(&Settings::default(), json).unwrap();
        assert_eq!(r.settings.advanced.update_channel, UpdateChannel::Beta);
        assert_eq!(r.settings.history.retention, HistoryRetention::OneYear);
        assert!(r.settings.history.enabled, "bad value keeps the old one");
        assert!(r.issues.iter().any(|i| i.starts_with("history.enabled")), "{:?}", r.issues);
        assert!(r.issues.iter().any(|i| i.starts_with("nonsense")), "{:?}", r.issues);
    }

    #[test]
    fn bare_object_and_wrong_files() {
        let r = import_json(&Settings::default(), r#"{"advanced":{"log_level":"info"}}"#).unwrap();
        assert_eq!(r.settings.advanced.log_level, LogLevel::Info);
        assert!(import_json(&Settings::default(), "[1]").is_err());
        assert!(import_json(&Settings::default(), "nope").is_err());
        let feishin = import_json(&Settings::default(), r#"{"general":{},"playback":{}}"#).unwrap_err();
        assert!(feishin.contains("Feishin"));
        assert!(import_json(&Settings::default(), r#"{"format":"other","settings":{}}"#).is_err());
    }

    #[test]
    fn secrets_are_neither_exported_nor_imported() {
        assert!(is_secret_key("github_token") && is_secret_key("ApiKey") && !is_secret_key("theme"));
        assert!(is_secret_key("lastfm_key") && is_secret_key("translation_api_key"));
        let mut v =
            serde_json::json!({"a": {"api_key": "x", "ok": 1}, "password": "p", "l": [{"token": "t"}]});
        strip_secrets(&mut v);
        assert_eq!(v, serde_json::json!({"a": {"ok": 1}, "l": [{}]}));
        let r = import_json(&Settings::default(), r#"{"token":"abc","history":{"enabled":false}}"#).unwrap();
        assert!(!r.settings.history.enabled);
        assert!(r.issues.iter().any(|i| i.contains("secrets")));
        let text = export_json(&Settings::default(), "1");
        assert!(!text.to_lowercase().contains("password"));
    }

    #[test]
    fn old_config_without_advanced_loads_with_defaults() {
        let s: Settings = toml::from_str("[history]\nenabled = false\n").unwrap();
        assert_eq!(s.advanced, AdvancedSettings::default());
        assert!(s.advanced.update_check_on_start);
    }
}

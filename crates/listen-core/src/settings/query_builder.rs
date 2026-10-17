//! Smart-playlist builder settings (Feishin `queryBuilder.tag`): extra tags the
//! rule builder offers beside the standard fields.

use serde::{Deserialize, Serialize};

/// One custom field: a label, an input type and the tag (field key) in the rules.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct QueryTag {
    pub label: String,
    /// `string`, `number`, `boolean`, `date`, `dateRange` or `playlist`.
    pub kind: String,
    pub tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct QueryBuilder {
    pub tags: Vec<QueryTag>,
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn imports_feishin_custom_fields_and_old_files_still_load() {
        let json = r#"{"queryBuilder": {"tag": [
            {"label": "Moods", "type": "string", "value": "mood"},
            {"label": "Skipped", "type": "number", "value": "skips"},
            {"label": "No tag", "type": "string", "value": ""}]}}"#;
        let s = import_feishin(&Settings::default(), json).unwrap();
        assert_eq!(s.query_builder.tags.len(), 2);
        assert_eq!(
            s.query_builder.tags[1],
            QueryTag { label: "Skipped".into(), kind: "number".into(), tag: "skips".into() }
        );
        // A settings file without the section keeps working.
        let old: Settings = toml::from_str("[window]\n").unwrap();
        assert!(old.query_builder.tags.is_empty());
        let round: Settings = toml::from_str(&toml::to_string(&s).unwrap()).unwrap();
        assert_eq!(round.query_builder, s.query_builder);
    }
}

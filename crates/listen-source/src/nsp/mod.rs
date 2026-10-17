//! Navidrome smart playlists (`.nsp` rules JSON, e.g. `{"all":[{"contains":{"title":"x"}}],"sort":"-year"}`).
//!
//! - [`Nsp`]: the wire format.
//! - [`fields`]: fields and operators per field type (Feishin's `NDSongQueryFields`).
//! - [`query`]: the editable rule tree of the builder, converted from / to [`Nsp`].

pub mod fields;
pub mod query;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// A comparison operator of a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Is,
    IsNot,
    Gt,
    Lt,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    InTheRange,
    Before,
    After,
    InTheLast,
    NotInTheLast,
    InPlaylist,
    NotInPlaylist,
    IsMissing,
    IsPresent,
    /// An operator this version does not know; kept so editing never loses a rule.
    Other(String),
}

impl Op {
    pub const ALL: [Op; 17] = [
        Op::Is,
        Op::IsNot,
        Op::Gt,
        Op::Lt,
        Op::Contains,
        Op::NotContains,
        Op::StartsWith,
        Op::EndsWith,
        Op::InTheRange,
        Op::Before,
        Op::After,
        Op::InTheLast,
        Op::NotInTheLast,
        Op::InPlaylist,
        Op::NotInPlaylist,
        Op::IsMissing,
        Op::IsPresent,
    ];

    /// The key in the JSON.
    pub fn key(&self) -> &str {
        match self {
            Op::Is => "is",
            Op::IsNot => "isNot",
            Op::Gt => "gt",
            Op::Lt => "lt",
            Op::Contains => "contains",
            Op::NotContains => "notContains",
            Op::StartsWith => "startsWith",
            Op::EndsWith => "endsWith",
            Op::InTheRange => "inTheRange",
            Op::Before => "before",
            Op::After => "after",
            Op::InTheLast => "inTheLast",
            Op::NotInTheLast => "notInTheLast",
            Op::InPlaylist => "inPlaylist",
            Op::NotInPlaylist => "notInPlaylist",
            Op::IsMissing => "isMissing",
            Op::IsPresent => "isPresent",
            Op::Other(k) => k,
        }
    }

    pub fn parse(key: &str) -> Op {
        Op::ALL.iter().find(|o| o.key() == key).cloned().unwrap_or_else(|| Op::Other(key.to_string()))
    }
}

/// `field <op> value`; the JSON is `{"<op>": {"<field>": <value>}}`.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub op: Op,
    pub field: String,
    pub value: Value,
}

/// How a group combines its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupKind {
    #[default]
    All,
    Any,
}

impl GroupKind {
    pub fn key(self) -> &'static str {
        match self {
            GroupKind::All => "all",
            GroupKind::Any => "any",
        }
    }
}

impl GroupKind {
    pub const BOTH: [GroupKind; 2] = [GroupKind::All, GroupKind::Any];
}

impl std::fmt::Display for GroupKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            GroupKind::All => "Match all",
            GroupKind::Any => "Match any",
        })
    }
}

/// A child of a group.
#[derive(Debug, Clone, PartialEq)]
pub enum Cond {
    Rule(Rule),
    Group(Group),
}

/// `{"all": [...]}` or `{"any": [...]}`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Group {
    pub kind: GroupKind,
    pub items: Vec<Cond>,
}

/// A whole smart-playlist definition: the root group plus sort and limit.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Nsp {
    pub root: Group,
    /// Navidrome's `sort`: fields, comma separated, each optionally prefixed `+` / `-`.
    pub sort: Option<String>,
    /// Legacy `order` ("asc" / "desc") of an unprefixed `sort`.
    pub order: Option<String>,
    pub limit: Option<u64>,
    pub limit_percent: Option<f64>,
    /// Keys this version does not know (`offset`, ...), written back unchanged.
    pub extra: Map<String, Value>,
}

fn group_key(obj: &Map<String, Value>) -> Option<GroupKind> {
    if obj.contains_key("all") {
        Some(GroupKind::All)
    } else if obj.contains_key("any") {
        Some(GroupKind::Any)
    } else {
        None
    }
}

impl Cond {
    pub fn to_value(&self) -> Value {
        match self {
            Cond::Group(g) => g.to_value(),
            Cond::Rule(r) => {
                let mut field = Map::new();
                field.insert(r.field.clone(), r.value.clone());
                let mut obj = Map::new();
                obj.insert(r.op.key().to_string(), Value::Object(field));
                Value::Object(obj)
            }
        }
    }

    pub fn from_value(v: &Value) -> Result<Cond, String> {
        let obj = v.as_object().ok_or("a rule must be an object")?;
        if group_key(obj).is_some() {
            return Group::from_value(v).map(Cond::Group);
        }
        let mut entries = obj.iter();
        let (op, body) = entries.next().ok_or("an empty rule")?;
        if entries.next().is_some() {
            return Err(format!("rule with several operators ({op}, ...)"));
        }
        let body = body.as_object().ok_or_else(|| format!("`{op}` needs an object"))?;
        let mut fields = body.iter();
        let (field, value) = fields.next().ok_or_else(|| format!("`{op}` has no field"))?;
        if fields.next().is_some() {
            return Err(format!("`{op}` compares several fields"));
        }
        Ok(Cond::Rule(Rule { op: Op::parse(op), field: field.clone(), value: value.clone() }))
    }
}

impl Group {
    pub fn to_value(&self) -> Value {
        let mut obj = Map::new();
        obj.insert(
            self.kind.key().to_string(),
            Value::Array(self.items.iter().map(Cond::to_value).collect()),
        );
        Value::Object(obj)
    }

    pub fn from_value(v: &Value) -> Result<Group, String> {
        let obj = v.as_object().ok_or("a group must be an object")?;
        let kind = group_key(obj).ok_or("a group needs `all` or `any`")?;
        let items = obj[kind.key()].as_array().ok_or_else(|| format!("`{}` must be a list", kind.key()))?;
        Ok(Group { kind, items: items.iter().map(Cond::from_value).collect::<Result<_, _>>()? })
    }
}

impl Nsp {
    pub fn to_value(&self) -> Value {
        let Value::Object(mut obj) = self.root.to_value() else { unreachable!() };
        if let Some(s) = &self.sort {
            obj.insert("sort".into(), s.clone().into());
        }
        if let Some(o) = &self.order {
            obj.insert("order".into(), o.clone().into());
        }
        if let Some(l) = self.limit {
            obj.insert("limit".into(), l.into());
        }
        if let Some(p) = self.limit_percent {
            obj.insert("limitPercent".into(), number(p));
        }
        for (k, v) in &self.extra {
            obj.entry(k.clone()).or_insert_with(|| v.clone());
        }
        Value::Object(obj)
    }

    pub fn from_value(v: &Value) -> Result<Nsp, String> {
        let obj = v.as_object().ok_or("the rules must be an object")?;
        let root = Group::from_value(v)?;
        let mut nsp = Nsp { root, ..Default::default() };
        for (k, val) in obj {
            match k.as_str() {
                "all" | "any" if k == nsp.root.kind.key() => {}
                "sort" => nsp.sort = val.as_str().filter(|s| !s.is_empty()).map(str::to_string),
                "order" => nsp.order = val.as_str().filter(|s| !s.is_empty()).map(str::to_string),
                "limit" => {
                    nsp.limit = val.as_u64().or_else(|| val.as_f64().map(|f| f as u64)).filter(|l| *l > 0)
                }
                "limitPercent" => nsp.limit_percent = val.as_f64().filter(|p| *p > 0.0),
                _ => {
                    nsp.extra.insert(k.clone(), val.clone());
                }
            }
        }
        Ok(nsp)
    }

    /// Parses the JSON text Navidrome stores in a playlist's `rules`.
    pub fn from_json(text: &str) -> Result<Nsp, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        Nsp::from_value(&v)
    }

    pub fn to_json(&self) -> String {
        self.to_value().to_string()
    }
}

/// A float as an integer JSON number when it has no fraction.
pub fn number(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 9e15 { Value::from(x as i64) } else { Value::from(x) }
}

macro_rules! via_value {
    ($ty:ty) => {
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                self.to_value().serialize(s)
            }
        }
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let v = Value::deserialize(d)?;
                <$ty>::from_value(&v).map_err(serde::de::Error::custom)
            }
        }
    };
}
via_value!(Nsp);
via_value!(Group);
via_value!(Cond);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// One rule per operator, with a value of the shape Navidrome expects.
    fn every_operator() -> Value {
        json!({
            "all": [
                {"is": {"artist": "Beatles"}},
                {"isNot": {"genre": "Pop"}},
                {"gt": {"rating": 3}},
                {"lt": {"year": 2000}},
                {"contains": {"title": "love"}},
                {"notContains": {"comment": "live"}},
                {"startsWith": {"album": "The"}},
                {"endsWith": {"album": "Road"}},
                {"inTheRange": {"year": [1960, 1970]}},
                {"before": {"releasedate": "2001-01-01"}},
                {"after": {"dateadded": "2020-05-17"}},
                {"inTheLast": {"lastplayed": 30}},
                {"notInTheLast": {"dateloved": 90}},
                {"inPlaylist": {"id": "pl-1"}},
                {"notInPlaylist": {"id": "pl-2"}},
                {"isMissing": {"lyrics": true}},
                {"isPresent": {"hascoverart": true}},
                {"is": {"loved": true}},
                {"any": [
                    {"is": {"genre": "Rock"}},
                    {"all": [{"gt": {"playcount": 5}}, {"lt": {"bpm": 140.5}}]}
                ]}
            ],
            "sort": "-year,+title",
            "limit": 100
        })
    }

    #[test]
    fn round_trips_every_operator_and_nested_groups() {
        let json = every_operator();
        let nsp: Nsp = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(nsp.root.kind, GroupKind::All);
        assert_eq!(nsp.root.items.len(), 19);
        let Cond::Group(any) = &nsp.root.items[18] else { panic!("nested group") };
        assert_eq!(any.kind, GroupKind::Any);
        let Cond::Group(inner) = &any.items[1] else { panic!("second level") };
        assert_eq!((inner.kind, inner.items.len()), (GroupKind::All, 2));
        // Every known operator is typed, none falls back to `Other`.
        let ops: Vec<&Op> = nsp
            .root
            .items
            .iter()
            .filter_map(|c| if let Cond::Rule(r) = c { Some(&r.op) } else { None })
            .collect();
        assert!(ops.iter().all(|o| !matches!(o, Op::Other(_))));
        for known in Op::ALL {
            assert!(ops.contains(&&known), "{known:?} missing in the sample");
        }
        assert_eq!((nsp.sort.as_deref(), nsp.limit), (Some("-year,+title"), Some(100)));
        assert_eq!(serde_json::to_value(&nsp).unwrap(), json);
        // And through text.
        assert_eq!(Nsp::from_json(&nsp.to_json()).unwrap(), nsp);
    }

    #[test]
    fn any_root_percent_limit_and_unknown_keys_survive() {
        let json = json!({"any": [{"is": {"loved": true}}], "limitPercent": 12.5, "offset": 3,
            "order": "desc", "sort": "title"});
        let nsp: Nsp = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(nsp.root.kind, GroupKind::Any);
        assert_eq!((nsp.limit_percent, nsp.order.as_deref()), (Some(12.5), Some("desc")));
        assert_eq!(nsp.extra["offset"], 3);
        assert_eq!(serde_json::to_value(&nsp).unwrap(), json);
    }

    #[test]
    fn unknown_operator_is_kept() {
        let json = json!({"all": [{"matchesRegex": {"title": "^a"}}]});
        let nsp: Nsp = serde_json::from_value(json.clone()).unwrap();
        let Cond::Rule(r) = &nsp.root.items[0] else { panic!() };
        assert_eq!(r.op, Op::Other("matchesRegex".into()));
        assert_eq!(serde_json::to_value(&nsp).unwrap(), json);
    }

    #[test]
    fn malformed_rules_are_errors() {
        for bad in [
            json!([]),
            json!({"sort": "title"}),
            json!({"all": {"is": {"a": 1}}}),
            json!({"all": [{"is": {"a": 1}, "isNot": {"b": 2}}]}),
            json!({"all": [{"is": {}}]}),
            json!({"all": [{"is": 5}]}),
        ] {
            assert!(serde_json::from_value::<Nsp>(bad.clone()).is_err(), "{bad}");
        }
    }
}

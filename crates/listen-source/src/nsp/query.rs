//! The rule tree the query builder edits (Feishin `QueryBuilderGroup`). Values are input text;
//! [`Query::to_nsp`] converts them to rules JSON by field type. Groups list rules before sub-groups.

use super::fields::{self, Kind};
use super::{Cond, Group, GroupKind, Nsp, Op, Rule, number};
use serde_json::{Map, Value};

/// A field the builder offers: a standard tag or a custom one from the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldOpt {
    pub label: String,
    pub key: String,
    pub kind: Kind,
    pub custom: bool,
}

impl std::fmt::Display for FieldOpt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// The fields: custom ones (`label`, kind name, tag) first, as Feishin's "Custom tags", then standard.
/// Custom fields without a tag are left out.
pub fn field_options<'a>(custom: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>) -> Vec<FieldOpt> {
    let mut out: Vec<FieldOpt> = custom
        .into_iter()
        .filter(|(_, _, tag)| !tag.trim().is_empty())
        .map(|(label, kind, tag)| FieldOpt {
            label: if label.trim().is_empty() { tag.trim().to_string() } else { label.trim().to_string() },
            key: tag.trim().to_string(),
            kind: Kind::parse(kind),
            custom: true,
        })
        .collect();
    out.extend(fields::FIELDS.iter().map(|f| FieldOpt {
        label: f.label.to_string(),
        key: f.key.to_string(),
        kind: f.kind,
        custom: false,
    }));
    out
}

/// The sort choices: random, then every field (Feishin `sortOptions`).
pub fn sort_options(fields: &[FieldOpt]) -> Vec<FieldOpt> {
    let mut out =
        vec![FieldOpt { label: "Random".into(), key: "random".into(), kind: Kind::String, custom: false }];
    out.extend(fields.iter().filter(|f| !f.custom).cloned());
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct QRule {
    pub field: String,
    pub kind: Kind,
    /// An id of [`fields::operators`]; empty: none picked yet.
    pub op: String,
    /// The value (or the lower end of a range) as typed.
    pub a: String,
    /// The upper end of a range.
    pub b: String,
}

impl QRule {
    pub fn empty() -> Self {
        Self {
            field: String::new(),
            kind: Kind::String,
            op: String::new(),
            a: String::new(),
            b: String::new(),
        }
    }

    /// Which input the rule's value gets.
    pub fn input(&self) -> Input {
        match (self.kind, self.op.as_str()) {
            (_, "isMissing" | "isPresent") => Input::Bool,
            (_, "inTheRangeDate") => Input::DateRange,
            (_, "inTheRange") => Input::NumberRange,
            (Kind::Boolean, _) => Input::Bool,
            (Kind::Date, "beforeDate" | "afterDate") => Input::Date,
            (Kind::Number, _) | (Kind::Date, "inTheLast" | "notInTheLast") => Input::Number,
            (Kind::Playlist, _) => Input::Playlist,
            _ => Input::Text,
        }
    }
}

/// The kind of input a rule's value needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Text,
    Number,
    /// `YYYY-MM-DD`.
    Date,
    Bool,
    NumberRange,
    DateRange,
    Playlist,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QGroup {
    pub kind: GroupKind,
    pub rules: Vec<QRule>,
    pub groups: Vec<QGroup>,
}

impl QGroup {
    fn new(kind: GroupKind) -> Self {
        Self { kind, rules: vec![QRule::empty()], groups: vec![] }
    }

    fn at(&mut self, path: &[usize]) -> Option<&mut QGroup> {
        match path.split_first() {
            None => Some(self),
            Some((i, rest)) => self.groups.get_mut(*i)?.at(rest),
        }
    }

    pub fn get(&self, path: &[usize]) -> Option<&QGroup> {
        match path.split_first() {
            None => Some(self),
            Some((i, rest)) => self.groups.get(*i)?.get(rest),
        }
    }

    /// Rules and groups in this group and below.
    pub fn count(&self) -> usize {
        self.rules.len() + self.groups.iter().map(QGroup::count).sum::<usize>()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LimitMode {
    #[default]
    Count,
    Percent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortEntry {
    pub field: String,
    pub desc: bool,
}

/// A whole smart-playlist definition as edited.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub root: QGroup,
    pub sort: Vec<SortEntry>,
    pub limit_mode: LimitMode,
    pub limit: String,
    pub limit_percent: String,
    extra: Map<String, Value>,
    /// What "Reset to default" returns to: the loaded rules, or the empty query.
    initial: QGroup,
}

impl Default for Query {
    fn default() -> Self {
        let root = QGroup::new(GroupKind::All);
        Self {
            initial: root.clone(),
            root,
            // Feishin's create form starts sorted by album.
            sort: vec![SortEntry { field: "album".into(), desc: false }],
            limit_mode: LimitMode::Count,
            limit: String::new(),
            limit_percent: String::new(),
            extra: Map::new(),
        }
    }
}

fn is_date(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
        && matches!(text[5..7].parse::<u32>(), Ok(1..=12))
        && matches!(text[8..10].parse::<u32>(), Ok(1..=31))
}

fn parse_number(label: &str, text: &str) -> Result<Value, String> {
    let t = text.trim();
    if t.is_empty() {
        return Err(format!("{label}: enter a number."));
    }
    t.replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|x| x.is_finite())
        .map(number)
        .ok_or_else(|| format!("{label}: \"{t}\" is not a number."))
}

fn parse_date(label: &str, text: &str) -> Result<Value, String> {
    let t = text.trim();
    if is_date(t) { Ok(t.into()) } else { Err(format!("{label}: enter a date as YYYY-MM-DD.")) }
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

impl QRule {
    fn to_cond(&self, label: &str) -> Result<Cond, String> {
        let api = fields::api_operator(&self.op);
        let value = match self.input() {
            Input::Bool => Value::Bool(self.a != "false"),
            Input::Number => parse_number(label, &self.a)?,
            Input::Date => parse_date(label, &self.a)?,
            Input::NumberRange => {
                Value::Array(vec![parse_number(label, &self.a)?, parse_number(label, &self.b)?])
            }
            Input::DateRange => Value::Array(vec![parse_date(label, &self.a)?, parse_date(label, &self.b)?]),
            Input::Text | Input::Playlist => {
                if self.a.is_empty() {
                    return Err(format!("{label}: enter a value."));
                }
                Value::String(self.a.clone())
            }
        };
        Ok(Cond::Rule(Rule { op: Op::parse(api), field: self.field.clone(), value }))
    }

    fn from_rule(r: &Rule, known: &[FieldOpt]) -> QRule {
        let kind = known.iter().find(|f| f.key == r.field).map(|f| f.kind).unwrap_or(match r.value {
            Value::Bool(_) => Kind::Boolean,
            Value::Number(_) => Kind::Number,
            _ => Kind::String,
        });
        let dateish = |v: &Value| v.as_str().is_some_and(|s| is_date(s.trim()));
        let mut op = r.op.key().to_string();
        // Feishin shows the date-picker version when the value is a date.
        if kind == Kind::Date {
            match (&r.op, &r.value) {
                (Op::Before, v) if dateish(v) => op = "beforeDate".into(),
                (Op::After, v) if dateish(v) => op = "afterDate".into(),
                (Op::InTheRange, Value::Array(p)) if p.len() == 2 && p.iter().all(dateish) => {
                    op = "inTheRangeDate".into()
                }
                _ => {}
            }
        }
        let (a, b) = match &r.value {
            Value::Array(p) => {
                (p.first().map(text_of).unwrap_or_default(), p.get(1).map(text_of).unwrap_or_default())
            }
            v => (text_of(v), String::new()),
        };
        QRule { field: r.field.clone(), kind, op, a, b }
    }
}

impl QGroup {
    fn to_group(&self, known: &[FieldOpt]) -> Result<Group, String> {
        let mut items = Vec::new();
        for r in self.rules.iter().filter(|r| !r.field.is_empty() && !r.op.is_empty()) {
            let label = known.iter().find(|f| f.key == r.field).map(|f| f.label.as_str()).unwrap_or(&r.field);
            items.push(r.to_cond(label)?);
        }
        for g in &self.groups {
            items.push(Cond::Group(g.to_group(known)?));
        }
        Ok(Group { kind: self.kind, items })
    }

    fn from_group(g: &Group, known: &[FieldOpt]) -> QGroup {
        let mut out = QGroup { kind: g.kind, rules: vec![], groups: vec![] };
        for c in &g.items {
            match c {
                Cond::Rule(r) => out.rules.push(QRule::from_rule(r, known)),
                Cond::Group(g) => out.groups.push(QGroup::from_group(g, known)),
            }
        }
        out
    }
}

fn parse_sort(sort: Option<&str>, order: Option<&str>) -> Vec<SortEntry> {
    let default_desc = order.is_some_and(|o| o.eq_ignore_ascii_case("desc"));
    let Some(sort) = sort.filter(|s| !s.trim().is_empty()) else { return vec![] };
    sort.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| match p.strip_prefix('-') {
            Some(f) => SortEntry { field: f.to_string(), desc: true },
            None => SortEntry { field: p.strip_prefix('+').unwrap_or(p).to_string(), desc: default_desc },
        })
        .collect()
}

impl Query {
    /// Loads existing rules; `known` are the fields the builder offers.
    pub fn from_nsp(nsp: &Nsp, known: &[FieldOpt]) -> Query {
        let root = QGroup::from_group(&nsp.root, known);
        let (limit_mode, limit, limit_percent) = match nsp.limit_percent {
            Some(p) => (
                LimitMode::Percent,
                nsp.limit.map(|l| l.to_string()).unwrap_or_default(),
                text_of(&number(p)),
            ),
            None => (LimitMode::Count, nsp.limit.map(|l| l.to_string()).unwrap_or_default(), String::new()),
        };
        Query {
            initial: root.clone(),
            root,
            sort: parse_sort(nsp.sort.as_deref(), nsp.order.as_deref()),
            limit_mode,
            limit,
            limit_percent,
            extra: nsp.extra.clone(),
        }
    }

    /// The rules to save, without rules lacking a field or operator; `Err` names a value that doesn't fit.
    pub fn to_nsp(&self, known: &[FieldOpt]) -> Result<Nsp, String> {
        let root = self.root.to_group(known)?;
        let sort: Vec<String> = self
            .sort
            .iter()
            .filter(|e| !e.field.is_empty())
            .map(|e| format!("{}{}", if e.desc { '-' } else { '+' }, e.field))
            .collect();
        let (limit, limit_percent) = match self.limit_mode {
            LimitMode::Count => (self.parse_limit(&self.limit)?, None),
            LimitMode::Percent => {
                let t = self.limit_percent.trim();
                let p = if t.is_empty() {
                    None
                } else {
                    let p: f64 = t
                        .replace(',', ".")
                        .parse()
                        .map_err(|_| format!("Limit: \"{t}\" is not a number."))?;
                    if !(0.0..=100.0).contains(&p) {
                        return Err("Limit: a percentage is between 0 and 100.".into());
                    }
                    (p > 0.0).then_some(p)
                };
                (None, p)
            }
        };
        Ok(Nsp {
            root,
            // The direction is in the prefixes, so the legacy `order` goes.
            sort: Some(if sort.is_empty() { "+dateAdded".to_string() } else { sort.join(",") }),
            order: None,
            limit,
            limit_percent,
            extra: self.extra.clone(),
        })
    }

    fn parse_limit(&self, text: &str) -> Result<Option<u64>, String> {
        let t = text.trim();
        if t.is_empty() {
            return Ok(None);
        }
        let n: u64 = t.parse().map_err(|_| format!("Limit: \"{t}\" is not a whole number."))?;
        Ok((n > 0).then_some(n))
    }

    // ---- edits -----------------------------------------------------------

    pub fn add_rule(&mut self, path: &[usize]) {
        if let Some(g) = self.root.at(path) {
            g.rules.push(QRule::empty());
        }
    }

    /// A new "match any" group with one empty rule (Feishin `handleAddRuleGroup`).
    pub fn add_group(&mut self, path: &[usize]) {
        if let Some(g) = self.root.at(path) {
            g.groups.push(QGroup::new(GroupKind::Any));
        }
    }

    /// Removes the group at `path`; the root cannot be removed.
    pub fn remove_group(&mut self, path: &[usize]) {
        let Some((last, parent)) = path.split_last() else { return };
        if let Some(g) = self.root.at(parent)
            && *last < g.groups.len()
        {
            g.groups.remove(*last);
        }
    }

    pub fn remove_rule(&mut self, path: &[usize], index: usize) {
        if let Some(g) = self.root.at(path)
            && index < g.rules.len()
        {
            g.rules.remove(index);
        }
    }

    pub fn set_kind(&mut self, path: &[usize], kind: GroupKind) {
        if let Some(g) = self.root.at(path) {
            g.kind = kind;
        }
    }

    fn rule(&mut self, path: &[usize], index: usize) -> Option<&mut QRule> {
        self.root.at(path)?.rules.get_mut(index)
    }

    /// Picking a field clears the operator and the value (Feishin `handleChangeField`).
    pub fn set_field(&mut self, path: &[usize], index: usize, field: &FieldOpt) {
        if let Some(r) = self.rule(path, index) {
            *r = QRule { field: field.key.clone(), kind: field.kind, ..QRule::empty() };
        }
    }

    /// Picking an operator keeps the value when it still fits the new input.
    pub fn set_op(&mut self, path: &[usize], index: usize, op: &str) {
        if let Some(r) = self.rule(path, index) {
            let before = r.input();
            r.op = op.to_string();
            if r.input() != before {
                r.a.clear();
                r.b.clear();
            }
        }
    }

    pub fn set_a(&mut self, path: &[usize], index: usize, text: String) {
        if let Some(r) = self.rule(path, index) {
            r.a = text;
        }
    }

    pub fn set_b(&mut self, path: &[usize], index: usize, text: String) {
        if let Some(r) = self.rule(path, index) {
            r.b = text;
        }
    }

    /// "Reset to default": the rules as loaded.
    pub fn reset(&mut self) {
        self.root = self.initial.clone();
    }

    /// "Clear filters": one empty rule.
    pub fn clear(&mut self) {
        self.root = QGroup::new(GroupKind::All);
    }

    pub fn add_sort(&mut self) {
        self.sort.push(SortEntry { field: String::new(), desc: false });
    }

    pub fn remove_sort(&mut self, index: usize) {
        if index < self.sort.len() && self.sort.len() > 1 {
            self.sort.remove(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn known() -> Vec<FieldOpt> {
        field_options([("Moods", "string", "mood2"), ("Skipped", "number", "skips"), ("", "string", " ")])
    }

    fn field(key: &str) -> FieldOpt {
        known().into_iter().find(|f| f.key == key).unwrap()
    }

    fn q(json: Value) -> Query {
        Query::from_nsp(&Nsp::from_value(&json).unwrap(), &known())
    }

    #[test]
    fn custom_fields_come_first_and_blank_tags_are_dropped() {
        let k = known();
        assert_eq!((k[0].label.as_str(), k[0].custom, k[0].kind), ("Moods", true, Kind::String));
        assert_eq!((k[1].key.as_str(), k[1].kind), ("skips", Kind::Number));
        assert!(!k.iter().any(|f| f.key.trim().is_empty()));
        assert_eq!(k.len(), 2 + fields::FIELDS.len());
        assert_eq!(sort_options(&k)[0].key, "random");
        assert!(sort_options(&k).iter().all(|f| !f.custom));
    }

    #[test]
    fn loads_and_saves_the_same_rules() {
        let json = json!({"all": [
            {"is": {"loved": true}},
            {"gt": {"rating": 3}},
            {"inTheRange": {"year": [1960, 1970]}},
            {"inTheLast": {"lastplayed": 30}},
            {"before": {"releasedate": "2001-01-01"}},
            {"inTheRange": {"dateadded": ["2020-01-01", "2020-12-31"]}},
            {"isMissing": {"lyrics": true}},
            {"inPlaylist": {"id": "pl"}},
            {"contains": {"mood2": "chill"}},
            {"any": [{"is": {"genre": "Rock"}}, {"all": [{"lt": {"bpm": 140.5}}]}]}
        ], "sort": "-year,+title", "limit": 50, "offset": 2});
        let query = q(json.clone());
        let r = &query.root.rules;
        assert_eq!((r[0].kind, r[0].input()), (Kind::Boolean, Input::Bool));
        assert_eq!(r[2].input(), Input::NumberRange);
        assert_eq!((r[3].kind, r[3].input()), (Kind::Date, Input::Number));
        assert_eq!((r[4].op.as_str(), r[4].input()), ("beforeDate", Input::Date));
        assert_eq!((r[5].op.as_str(), r[5].input()), ("inTheRangeDate", Input::DateRange));
        assert_eq!(r[6].input(), Input::Bool);
        assert_eq!(r[7].input(), Input::Playlist);
        assert_eq!(query.root.groups[0].kind, GroupKind::Any);
        assert_eq!(query.root.groups[0].groups.len(), 1);
        assert_eq!(
            query.sort,
            [
                SortEntry { field: "year".into(), desc: true },
                SortEntry { field: "title".into(), desc: false }
            ]
        );
        // The saved JSON lists rules first, then groups: the same as the input here.
        let back = query.to_nsp(&known()).unwrap();
        assert_eq!(back.to_value(), json);
    }

    #[test]
    fn legacy_order_becomes_prefixes() {
        let query = q(json!({"all": [], "sort": "year,title", "order": "desc"}));
        assert!(query.sort.iter().all(|e| e.desc));
        let back = query.to_nsp(&known()).unwrap();
        assert_eq!((back.sort.as_deref(), back.order), (Some("-year,-title"), None));
    }

    #[test]
    fn percent_limit_round_trips() {
        let query = q(json!({"all": [], "limitPercent": 10, "sort": "+title"}));
        assert_eq!((query.limit_mode, query.limit_percent.as_str()), (LimitMode::Percent, "10"));
        let back = query.to_nsp(&known()).unwrap();
        assert_eq!((back.limit, back.limit_percent), (None, Some(10.0)));
        let mut bad = query.clone();
        bad.limit_percent = "150".into();
        assert!(bad.to_nsp(&known()).is_err());
    }

    #[test]
    fn field_type_decides_input_and_values() {
        let mut query = Query::default();
        let p: &[usize] = &[];
        query.set_field(p, 0, &field("year"));
        query.set_op(p, 0, "gt");
        assert_eq!(query.root.rules[0].input(), Input::Number);
        query.set_a(p, 0, "1990".into());
        assert_eq!(
            query.to_nsp(&known()).unwrap().root.items[0],
            Cond::Rule(Rule { op: Op::Gt, field: "year".into(), value: json!(1990) })
        );
        // Switching the field clears operator and value; a text field takes text.
        query.set_field(p, 0, &field("title"));
        assert_eq!(query.root.rules[0].op, "");
        query.set_op(p, 0, "contains");
        query.set_a(p, 0, "love".into());
        assert_eq!(query.root.rules[0].input(), Input::Text);
        assert_eq!(query.to_nsp(&known()).unwrap().root.items.len(), 1);
        // The operator switches a number to a range, and a date to a date picker.
        query.set_field(p, 0, &field("year"));
        query.set_op(p, 0, "inTheRange");
        assert_eq!(query.root.rules[0].input(), Input::NumberRange);
        query.set_field(p, 0, &field("dateadded"));
        query.set_op(p, 0, "afterDate");
        assert_eq!(query.root.rules[0].input(), Input::Date);
        query.set_a(p, 0, "2020-02-30x".into());
        assert!(query.to_nsp(&known()).unwrap_err().contains("YYYY-MM-DD"));
        query.set_a(p, 0, "2020-02-03".into());
        let nsp = query.to_nsp(&known()).unwrap();
        let Cond::Rule(r) = &nsp.root.items[0] else { panic!() };
        assert_eq!((r.op.clone(), r.value.clone()), (Op::After, json!("2020-02-03")));
        // Boolean fields send booleans.
        query.set_field(p, 0, &field("loved"));
        query.set_op(p, 0, "is");
        query.set_a(p, 0, "false".into());
        let nsp = query.to_nsp(&known()).unwrap();
        let Cond::Rule(r) = &nsp.root.items[0] else { panic!() };
        assert_eq!(r.value, json!(false));
        // Garbage numbers are reported by the field's label.
        query.set_field(p, 0, &field("skips"));
        query.set_op(p, 0, "lt");
        query.set_a(p, 0, "many".into());
        assert!(query.to_nsp(&known()).unwrap_err().starts_with("Skipped"));
    }

    #[test]
    fn tree_edits() {
        let mut query = Query::default();
        query.add_rule(&[]);
        query.add_group(&[]);
        query.add_group(&[0]);
        query.add_rule(&[0, 0]);
        assert_eq!(query.root.rules.len(), 2);
        assert_eq!(query.root.get(&[0, 0]).unwrap().rules.len(), 2);
        assert_eq!(query.root.groups[0].kind, GroupKind::Any);
        query.set_kind(&[0, 0], GroupKind::All);
        assert_eq!(query.root.get(&[0, 0]).unwrap().kind, GroupKind::All);
        query.remove_rule(&[], 0);
        assert_eq!(query.root.rules.len(), 1);
        query.remove_group(&[0, 0]);
        assert!(query.root.get(&[0, 0]).is_none());
        query.remove_group(&[]);
        query.clear();
        assert_eq!(query.root.count(), 1);
        // Empty rules are skipped; the sort falls back to the date added.
        query.sort.clear();
        let nsp = query.to_nsp(&known()).unwrap();
        assert!(nsp.root.items.is_empty());
        assert_eq!(nsp.sort.as_deref(), Some("+dateAdded"));
        // Reset returns to the loaded rules.
        let mut loaded = q(json!({"all": [{"is": {"loved": true}}]}));
        loaded.clear();
        loaded.reset();
        assert_eq!(loaded.root.rules.len(), 1);
        assert_eq!(loaded.root.rules[0].field, "loved");
    }
}

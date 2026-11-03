//! The entry editor, drawn from the session's field table so every kind gets one without a
//! line of its own here.
//!
//! A secret field starts empty and is written only if something is typed into it, so opening
//! an entry to fix its title never puts its password into this process. Ctrl+R loads the
//! current value into the box for editing.

use serde_json::{Map, Value, json};
use zeroize::Zeroizing;

use crate::engine::{Detail, Engine, FieldRow, Secret};
use crate::input::Input;

/// What a row edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Slot {
    /// The title.
    Title,
    /// A field of the kind.
    Field,
    /// Web addresses, comma separated.
    Sites,
    /// Tags, comma separated.
    Tags,
    /// Notes.
    Notes,
    /// A one-time code's link or seed.
    Otp,
}

/// One row.
#[derive(Debug)]
pub struct Row {
    pub slot: Slot,
    pub key: String,
    pub label: String,
    /// `text`, `password`, `hidden`, `multiline`, `toggle`, `choice`, `number`, `date`, …
    pub input: String,
    pub secret: bool,
    pub choices: Vec<String>,
    pub value: Input,
    /// For a toggle.
    pub flag: bool,
    /// For a secret: whether it has a value now, to say "unchanged" rather than "empty".
    pub had_value: bool,
    /// Whether the secret's current value was loaded into the box.
    pub loaded: bool,
    original: Zeroizing<String>,
    original_flag: bool,
}

impl Row {
    fn new(slot: Slot, key: &str, label: &str, input: &str) -> Self {
        Self {
            slot,
            key: key.to_owned(),
            label: label.to_owned(),
            input: input.to_owned(),
            secret: false,
            choices: Vec::new(),
            value: Input::new(),
            flag: false,
            had_value: false,
            loaded: false,
            original: Zeroizing::new(String::new()),
            original_flag: false,
        }
    }

    fn with(mut self, text: &str) -> Self {
        self.value.set(text);
        self.original = Zeroizing::new(text.to_owned());
        self
    }

    /// Whether it opens on several lines.
    pub fn multiline(&self) -> bool {
        self.input == "multiline" || self.slot == Slot::Notes
    }

    fn changed(&self) -> bool {
        if self.input == "toggle" {
            self.flag != self.original_flag
        } else if self.secret {
            !self.value.is_empty() && !(self.loaded && self.original.as_str() == self.value.text())
        } else {
            self.value.text() != self.original.as_str()
        }
    }
}

/// Days since 1970 for a calendar date, by Howard Hinnant's method.
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The calendar date of a day since 1970.
pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { yoe + era * 400 + 1 } else { yoe + era * 400 }, m, d)
}

/// `YYYY-MM-DD` for milliseconds since 1970.
pub fn date_text(ms: u64) -> String {
    let days = i64::try_from(ms / 86_400_000).unwrap_or(0);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn parse_date(text: &str) -> Option<u64> {
    let mut parts = text.trim().splitn(3, '-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    u64::try_from(days_from_civil(y, m, d) * 86_400_000).ok()
}

fn site_text(site: &Value) -> Option<String> {
    let (kind, value) = site.as_object()?.iter().next()?;
    matches!(kind.as_str(), "Url" | "Domain" | "Host").then(|| value.as_str().map(str::to_owned))?
}

fn shown_text(field: &FieldRow) -> String {
    match &field.value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) if field.input == "date" => n.as_u64().map(date_text).unwrap_or_default(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// The editor.
#[derive(Debug)]
pub struct Editor {
    /// The entry, or `None` for a new one.
    pub id: Option<String>,
    pub kind: String,
    pub rows: Vec<Row>,
    pub at: usize,
    /// Whether typed secrets are shown.
    pub show: bool,
    pub error: Option<String>,
    /// Sites this editor does not show, such as Android apps, kept as they were.
    kept_sites: Vec<Value>,
    original_sites: Vec<Value>,
}

impl Editor {
    /// A new entry of a kind, with the kind's fields from the session's table.
    pub fn new(kind: &str, fields: &[FieldRow]) -> Self {
        let mut editor = Self {
            id: None,
            kind: kind.to_owned(),
            rows: Vec::new(),
            at: 0,
            show: false,
            error: None,
            kept_sites: Vec::new(),
            original_sites: Vec::new(),
        };
        editor.rows.push(Row::new(Slot::Title, "title", "Title", "text"));
        for field in fields {
            editor.rows.push(Self::field_row(field));
        }
        editor.common_rows(None);
        editor
    }

    /// An existing entry.
    pub fn edit(detail: &Detail) -> Self {
        let mut editor = Self {
            id: Some(detail.row.id.clone()),
            kind: detail.row.kind.clone(),
            rows: Vec::new(),
            at: 0,
            show: false,
            error: None,
            kept_sites: detail.sites.iter().filter(|s| site_text(s).is_none()).cloned().collect(),
            original_sites: detail.sites.clone(),
        };
        editor.rows.push(Row::new(Slot::Title, "title", "Title", "text").with(&detail.row.title));
        for field in &detail.fields {
            let mut row = Self::field_row(field);
            if !field.secret {
                if field.input == "toggle" {
                    row.flag = field.value.as_ref().and_then(Value::as_bool).unwrap_or(false);
                    row.original_flag = row.flag;
                } else {
                    row = row.with(&shown_text(field));
                }
            }
            row.had_value = field.has_value;
            editor.rows.push(row);
        }
        editor.common_rows(Some(detail));
        editor
    }

    fn field_row(field: &FieldRow) -> Row {
        let mut row = Row::new(Slot::Field, &field.key, &field.label, &field.input);
        row.secret = field.secret;
        row.choices.clone_from(&field.choices);
        row.value.multiline = field.input == "multiline";
        if field.input == "choice"
            && let Some(first) = field.choices.first()
        {
            row = row.with(first);
        }
        row
    }

    fn common_rows(&mut self, detail: Option<&Detail>) {
        let kind = self.kind.clone();
        if kind == "login" {
            let sites: Vec<String> = detail.map(|d| d.sites.iter().filter_map(site_text).collect()).unwrap_or_default();
            self.rows.push(Row::new(Slot::Sites, "sites", "Websites", "url").with(&sites.join(", ")));
        }
        if kind == "login" || kind == "otp" {
            let mut row = Row::new(Slot::Otp, "otp", "One-time code", "hidden");
            row.secret = true;
            row.had_value = detail.is_some_and(|d| d.otp.is_some());
            self.rows.push(row);
        }
        let tags = detail.map(|d| d.row.tags.join(", ")).unwrap_or_default();
        self.rows.push(Row::new(Slot::Tags, "tags", "Tags", "text").with(&tags));
        if kind != "note" {
            let mut row = Row::new(Slot::Notes, "notes", "Notes", "multiline");
            row.secret = true;
            row.value.multiline = true;
            row.had_value = detail.is_some_and(|d| d.has_notes);
            self.rows.push(row);
        }
    }

    /// The row with the keyboard.
    pub fn row(&mut self) -> &mut Row {
        let at = self.at.min(self.rows.len().saturating_sub(1));
        &mut self.rows[at]
    }

    /// Loads the current value of a secret row into its box.
    pub fn load_secret(&mut self, engine: &mut Engine) -> Result<(), String> {
        let id = self.id.clone().ok_or("nothing saved to load yet")?;
        let row = self.row();
        if !row.secret || !row.had_value || row.slot == Slot::Otp {
            return Ok(());
        }
        let what = match row.slot {
            Slot::Notes => Secret::Notes,
            _ => Secret::Field(row.key.clone()),
        };
        let value = engine.reveal(&what, &id)?;
        row.value.set(&value);
        row.original = value;
        row.loaded = true;
        Ok(())
    }

    fn sites(&self, text: &str) -> Vec<Value> {
        let mut out = self.kept_sites.clone();
        for part in text.split([',', '\n']).map(str::trim).filter(|s| !s.is_empty()) {
            let same = self.original_sites.iter().find(|s| site_text(s).as_deref() == Some(part));
            out.push(match same {
                Some(original) => original.clone(),
                None if part.contains("://") => json!({"Url": part}),
                None => json!({"Domain": part.to_lowercase()}),
            });
        }
        out
    }

    fn plain_value(row: &Row) -> Result<Value, String> {
        let text = row.value.text().trim();
        Ok(match row.input.as_str() {
            "toggle" => json!(row.flag),
            "number" if text.is_empty() => Value::Null,
            "number" => json!(text.parse::<u64>().map_err(|_| format!("{} must be a whole number", row.label))?),
            "date" if text.is_empty() => Value::Null,
            "date" => json!(parse_date(text).ok_or_else(|| format!("{} must look like 2026-12-31", row.label))?),
            _ => json!(row.value.text()),
        })
    }

    /// Writes what changed. Returns the entry's id.
    pub fn save(&mut self, engine: &mut Engine) -> Result<String, String> {
        let title = self.rows.iter().find(|r| r.slot == Slot::Title).map(|r| r.value.text().trim().to_owned()).unwrap_or_default();
        if title.is_empty() {
            return Err("give it a title".to_owned());
        }
        let mut plain = Map::new();
        for row in self.rows.iter().filter(|r| r.slot == Slot::Field && !r.secret) {
            if self.id.is_none() || row.changed() {
                let value = Self::plain_value(row)?;
                // A new entry's empty fields are left at their defaults.
                if self.id.is_some() || !(value.is_null() || value == json!("") || value == json!(false)) {
                    plain.insert(row.key.clone(), value);
                }
            }
        }
        let sites_row = self.rows.iter().find(|r| r.slot == Slot::Sites);
        let sites = sites_row.map(|r| self.sites(r.value.text())).unwrap_or_default();
        let tags_row = self.rows.iter().find(|r| r.slot == Slot::Tags);
        let tags: Vec<String> = tags_row
            .map(|r| r.value.text().split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned).collect())
            .unwrap_or_default();

        let id = match &self.id {
            None => {
                let id = engine.create(&self.kind, &title, &plain, &sites)?;
                self.id = Some(id.clone());
                if !tags.is_empty() {
                    engine.set_common(&id, json!({"tags": tags}))?;
                }
                id
            }
            Some(id) => {
                let id = id.clone();
                let mut common = Map::new();
                if self.rows.iter().any(|r| r.slot == Slot::Title && r.changed()) {
                    common.insert("title".into(), json!(title));
                }
                if tags_row.is_some_and(Row::changed) {
                    common.insert("tags".into(), json!(tags));
                }
                if sites_row.is_some_and(Row::changed) {
                    common.insert("sites".into(), json!(sites));
                }
                if !common.is_empty() {
                    engine.set_common(&id, Value::Object(common))?;
                }
                engine.set_plain(&id, &plain)?;
                id
            }
        };
        for row in self.rows.iter().filter(|r| r.secret && r.changed()) {
            let value = row.value.text().as_bytes();
            match row.slot {
                Slot::Notes => engine.set_secret(&id, "notes", value)?,
                Slot::Otp => engine.set_otp(&id, Some(value))?,
                _ => engine.set_secret(&id, &row.key, value)?,
            }
        }
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip() {
        for ms in [0u64, 951_782_400_000, 1_767_139_200_000, 4_102_358_400_000] {
            assert_eq!(parse_date(&date_text(ms)), Some(ms));
        }
        assert_eq!(date_text(1_767_139_200_000), "2025-12-31");
        assert_eq!(parse_date("2026-13-01"), None);
    }

    #[test]
    fn sites_the_editor_cannot_show_are_kept() {
        let detail = Detail {
            row: crate::engine::Row { id: "x".into(), kind: "login".into(), title: "T".into(), ..Default::default() },
            sites: vec![json!({"Host": "a.example.com"}), json!({"AndroidApp": {"package": "p", "cert_sha256": null}})],
            ..Detail::default()
        };
        let editor = Editor::edit(&detail);
        let sites = editor.sites("a.example.com, https://b.example");
        assert_eq!(sites[0]["AndroidApp"]["package"], "p");
        assert_eq!(sites[1], json!({"Host": "a.example.com"}), "the original kind of match stays");
        assert_eq!(sites[2], json!({"Url": "https://b.example"}));
    }
}

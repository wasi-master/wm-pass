//! The entry editor, drawn from the session's field table so every kind gets one without a
//! line of its own here.
//!
//! A secret field's value stays in the vault until the box is typed in: the first key loads
//! it, so it is edited like any other text, and opening an entry to fix its title never puts
//! its password into this process. A note's text is loaded straight away, as it is what a
//! note is opened for.

use serde_json::{Map, Value, json};
use zeroize::Zeroizing;

use crate::engine::{Detail, Engine, FieldRow, Folder, Secret};
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
    /// Android apps, by package name, comma separated.
    Apps,
    /// Tags, comma separated.
    Tags,
    /// Notes.
    Notes,
    /// A one-time code's link or seed.
    Otp,
    /// A field the user added, with a name of its own.
    Custom,
    /// A login's security question and its answer.
    Question,
    /// A button that adds a field.
    AddCustom,
    /// A button that adds a security question.
    AddQuestion,
    /// The folder it is in.
    Folder,
    /// Its color.
    Color,
    /// Whether it is a favorite.
    Favourite,
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
    /// What each choice saves as, when that is not its words: a folder's id, a color's hue.
    pub choice_keys: Vec<String>,
    /// Values people type often, offered as the field is filled.
    pub suggestions: Vec<String>,
    pub value: Input,
    /// For a toggle.
    pub flag: bool,
    /// For a secret: whether it has a value now, to say "unchanged" rather than "empty".
    pub had_value: bool,
    /// Whether the secret's current value was loaded into the box.
    pub loaded: bool,
    /// A custom field's name or a security question, which is typed like the value.
    pub name: Input,
    /// A custom field's or question's place in the entry, `None` for one added here.
    pub index: Option<usize>,
    /// A custom field's kind as saved: `text`, `hidden`, `url`, `email`, `phone`, `date`, `toggle`.
    pub custom_kind: String,
    original: Zeroizing<String>,
    original_flag: bool,
    original_name: String,
    original_secret: bool,
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
            choice_keys: Vec::new(),
            suggestions: Vec::new(),
            value: Input::new(),
            flag: false,
            had_value: false,
            loaded: false,
            name: Input::new(),
            index: None,
            custom_kind: String::new(),
            original: Zeroizing::new(String::new()),
            original_flag: false,
            original_name: String::new(),
            original_secret: false,
        }
    }

    fn with(mut self, text: &str) -> Self {
        self.value.set(text);
        self.original = Zeroizing::new(text.to_owned());
        self
    }

    fn named(slot: Slot, index: Option<usize>, name: &str, secret: bool, had_value: bool) -> Self {
        let label = if slot == Slot::Question { "Question" } else { "Field" };
        let mut row = Self::new(slot, "", label, "text");
        row.name.set(name);
        row.original_name = name.to_owned();
        row.index = index;
        row.secret = secret;
        row.original_secret = secret;
        row.had_value = had_value;
        row
    }

    /// Whether it opens on several lines.
    pub fn multiline(&self) -> bool {
        self.input == "multiline" || self.slot == Slot::Notes
    }

    /// Whether it has a name of its own to type, beside its value.
    pub fn is_named(&self) -> bool {
        matches!(self.slot, Slot::Custom | Slot::Question)
    }

    /// The offered value that what was typed starts, if any.
    pub fn suggestion(&self) -> Option<String> {
        let typed = self.value.text();
        if typed.is_empty() || self.secret {
            return None;
        }
        let lower = typed.to_lowercase();
        self.suggestions.iter().find(|s| s.to_lowercase().starts_with(&lower) && s.len() > typed.len()).cloned()
    }

    /// What a choice row saves: its key, or `None` for the first choice, which means none.
    fn choice_key(&self) -> Option<String> {
        let at = self.choices.iter().position(|c| c == self.value.text())?;
        self.choice_keys.get(at).filter(|k| !k.is_empty()).cloned()
    }

    /// Whether it is a button rather than a value.
    pub fn is_action(&self) -> bool {
        matches!(self.slot, Slot::AddCustom | Slot::AddQuestion)
    }

    /// Whether a value is saved that is not in the box yet.
    pub fn unloaded(&self) -> bool {
        self.secret && self.had_value && !self.loaded
    }

    fn value_changed(&self) -> bool {
        if self.input == "toggle" {
            self.flag != self.original_flag
        } else if self.secret && !self.loaded {
            !self.value.is_empty()
        } else {
            self.value.text() != self.original.as_str()
        }
    }

    fn changed(&self) -> bool {
        match self.slot {
            Slot::AddCustom | Slot::AddQuestion => false,
            Slot::Custom | Slot::Question if self.index.is_none() => !self.name.text().trim().is_empty() || !self.value.is_empty(),
            Slot::Custom | Slot::Question => {
                self.value_changed() || self.name.text().trim() != self.original_name || self.secret != self.original_secret
            }
            _ => self.value_changed(),
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
    /// Whether the cursor is in a named row's name rather than its value.
    pub on_name: bool,
    /// Whether typed secrets are shown.
    pub show: bool,
    pub error: Option<String>,
    /// Set by Esc over unsaved changes, until the next key answers it.
    pub confirm_discard: bool,
    /// How strong the password under the cursor is, 0 to 4.
    pub strength: Option<u8>,
    /// Custom fields taken away, by their place in the entry.
    removed_custom: Vec<usize>,
    /// Sites this editor does not show, such as patterns, kept as they were.
    kept_sites: Vec<Value>,
    original_sites: Vec<Value>,
}

fn app_package(site: &Value) -> Option<&str> {
    site.get("AndroidApp")?.get("package")?.as_str()
}

impl Editor {
    fn empty(id: Option<String>, kind: &str) -> Self {
        Self {
            id,
            kind: kind.to_owned(),
            rows: Vec::new(),
            at: 0,
            on_name: false,
            show: false,
            error: None,
            confirm_discard: false,
            strength: None,
            removed_custom: Vec::new(),
            kept_sites: Vec::new(),
            original_sites: Vec::new(),
        }
    }

    /// A new entry of a kind, with the kind's fields from the session's table.
    pub fn new(kind: &str, fields: &[FieldRow]) -> Self {
        let mut editor = Self::empty(None, kind);
        editor.rows.push(Row::new(Slot::Title, "title", "Title", "text"));
        for field in fields {
            editor.rows.push(Self::field_row(field));
        }
        editor.common_rows(None);
        editor
    }

    /// An existing entry.
    pub fn edit(detail: &Detail) -> Self {
        let mut editor = Self::empty(Some(detail.row.id.clone()), &detail.row.kind);
        editor.kept_sites = detail.sites.iter().filter(|s| site_text(s).is_none() && app_package(s).is_none()).cloned().collect();
        editor.original_sites.clone_from(&detail.sites);
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

    /// An existing entry, with what it is opened for already in the box: a note's text.
    pub fn open(detail: &Detail, engine: &mut Engine) -> Self {
        let mut editor = Self::edit(detail);
        if let Some(at) = editor.rows.iter().position(|r| r.key == "body" && r.unloaded()) {
            let keep = editor.at;
            editor.at = at;
            if let Err(e) = editor.load_secret(engine) {
                editor.error = Some(e);
            }
            editor.at = keep;
        }
        editor
    }

    fn field_row(field: &FieldRow) -> Row {
        let mut row = Row::new(Slot::Field, &field.key, &field.label, &field.input);
        row.secret = field.secret;
        row.choices.clone_from(&field.choices);
        row.suggestions.clone_from(&field.suggestions);
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
            let apps: Vec<&str> = detail.map(|d| d.sites.iter().filter_map(app_package).collect()).unwrap_or_default();
            self.rows.push(Row::new(Slot::Apps, "apps", "Android apps", "text").with(&apps.join(", ")));
        }
        if kind == "login" || kind == "otp" {
            let mut row = Row::new(Slot::Otp, "otp", "One-time code", "hidden");
            row.secret = true;
            row.had_value = detail.is_some_and(|d| d.otp.is_some());
            self.rows.push(row);
        }
        if kind != "note" {
            let mut row = Row::new(Slot::Notes, "notes", "Notes", "multiline");
            row.secret = true;
            row.value.multiline = true;
            row.had_value = detail.is_some_and(|d| d.has_notes);
            self.rows.push(row);
        }
        for custom in detail.map(|d| d.custom_fields.as_slice()).unwrap_or_default() {
            let hidden = custom.kind == "hidden";
            let mut row = Row::named(Slot::Custom, Some(custom.index), &custom.name, hidden, custom.has_value);
            row.custom_kind.clone_from(&custom.kind);
            if !hidden {
                row = row.with(custom.value.as_deref().unwrap_or_default());
            }
            self.rows.push(row);
        }
        self.rows.push(Row::new(Slot::AddCustom, "add_custom", "Add a field", "text"));
        if kind == "login" {
            for (index, question) in detail.map(|d| d.questions.as_slice()).unwrap_or_default().iter().enumerate() {
                self.rows.push(Row::named(Slot::Question, Some(index), question, true, true));
            }
            self.rows.push(Row::new(Slot::AddQuestion, "add_question", "Add a security question", "text"));
        }
        let tags = detail.map(|d| d.row.tags.join(", ")).unwrap_or_default();
        self.rows.push(Row::new(Slot::Tags, "tags", "Tags", "text").with(&tags));
    }

    /// Adds where it is kept and how it looks: folder before the tags, color and favorite after.
    pub fn organize(&mut self, folders: &[Folder], detail: Option<&Detail>) {
        let Some(tags_at) = self.rows.iter().position(|r| r.slot == Slot::Tags) else { return };
        let mut sorted: Vec<&Folder> = folders.iter().collect();
        sorted.sort_by_key(|f| f.name.to_lowercase());
        let mut folder = Row::new(Slot::Folder, "folder", "Folder", "choice");
        folder.choices = std::iter::once("No folder".to_owned()).chain(sorted.iter().map(|f| f.name.clone())).collect();
        folder.choice_keys = std::iter::once(String::new()).chain(sorted.iter().map(|f| f.id.clone())).collect();
        let now = detail.and_then(|d| d.row.folder.as_deref()).and_then(|id| folder.choice_keys.iter().position(|k| k == id)).unwrap_or(0);
        let shown = folder.choices[now].clone();
        self.rows.insert(tags_at, folder.with(&shown));

        let mut color = Row::new(Slot::Color, "color", "Color", "choice");
        color.choices = std::iter::once("The site's own".to_owned()).chain(crate::app::HUES.iter().map(|h| h.1.to_owned())).collect();
        color.choice_keys = std::iter::once(String::new()).chain(crate::app::HUES.iter().map(|h| h.0.to_owned())).collect();
        let now = detail.and_then(|d| d.row.color.as_deref()).and_then(|hue| color.choice_keys.iter().position(|k| k == hue)).unwrap_or(0);
        let shown = color.choices[now].clone();
        self.rows.push(color.with(&shown));

        let mut favourite = Row::new(Slot::Favourite, "favourite", "Favorite", "toggle");
        favourite.flag = detail.is_some_and(|d| d.row.favourite);
        favourite.original_flag = favourite.flag;
        self.rows.push(favourite);
    }

    /// Rates the password under the cursor.
    pub fn rate(&mut self, engine: &mut Engine) {
        let row = &self.rows[self.at.min(self.rows.len() - 1)];
        self.strength = if row.input == "password" && !row.value.is_empty() && !self.on_name {
            engine.strength(row.value.text()).ok().map(|s| s.score)
        } else {
            None
        };
    }

    /// The row with the keyboard.
    pub fn row(&mut self) -> &mut Row {
        let at = self.at.min(self.rows.len().saturating_sub(1));
        &mut self.rows[at]
    }

    /// The heading drawn above row `n`, where a group starts.
    pub fn heading(&self, n: usize) -> Option<&'static str> {
        let group = |slot: &Slot| match slot {
            Slot::Custom | Slot::AddCustom => Some("More fields"),
            Slot::Question | Slot::AddQuestion => Some("Security questions"),
            Slot::Folder | Slot::Tags | Slot::Color | Slot::Favourite => Some("Organize"),
            _ => None,
        };
        let this = group(&self.rows.get(n)?.slot)?;
        (n == 0 || group(&self.rows[n - 1].slot) != Some(this)).then_some(this)
    }

    /// Whether anything was changed.
    pub fn dirty(&self) -> bool {
        !self.removed_custom.is_empty() || self.rows.iter().any(Row::changed)
    }

    /// Moves to the next place: a named row's value after its name, then the next row.
    pub fn forward(&mut self) {
        let row = &self.rows[self.at];
        if row.is_named() && self.on_name {
            self.on_name = false;
        } else if self.at + 1 < self.rows.len() {
            self.at += 1;
            self.on_name = self.rows[self.at].is_named() && self.rows[self.at].name.is_empty();
        }
    }

    /// Moves to the place before: a named row's name before its value, then the row above.
    pub fn back(&mut self) {
        let row = &self.rows[self.at];
        if row.is_named() && !self.on_name {
            self.on_name = true;
        } else if self.at > 0 {
            self.at -= 1;
            self.on_name = false;
        }
    }

    /// Moves a whole row.
    pub fn step(&mut self, down: bool) {
        self.at = if down { (self.at + 1).min(self.rows.len() - 1) } else { self.at.saturating_sub(1) };
        self.on_name = self.rows[self.at].is_named() && self.rows[self.at].name.is_empty();
    }

    /// Adds a custom field, or a security question for `question`, and puts the cursor on its name.
    pub fn add(&mut self, question: bool) {
        let (button, slot) = if question { (Slot::AddQuestion, Slot::Question) } else { (Slot::AddCustom, Slot::Custom) };
        let Some(at) = self.rows.iter().position(|r| r.slot == button) else { return };
        let mut row = Row::named(slot, None, "", question, false);
        row.loaded = true;
        row.custom_kind = if question { String::new() } else { "text".to_owned() };
        self.rows.insert(at, row);
        self.at = at;
        self.on_name = true;
        self.error = None;
    }

    /// Takes away the custom field under the cursor. Returns what happened, to say.
    pub fn remove(&mut self) -> Result<&'static str, String> {
        let row = &self.rows[self.at];
        match (&row.slot, row.index) {
            (Slot::Custom, index) => {
                if let Some(index) = index {
                    self.removed_custom.push(index);
                }
                self.rows.remove(self.at);
                self.at = self.at.min(self.rows.len() - 1);
                self.on_name = false;
                Ok("Field taken away. Esc leaves it as it was")
            }
            (Slot::Question, None) => {
                self.rows.remove(self.at);
                self.on_name = false;
                Ok("Question taken away")
            }
            (Slot::Question, Some(_)) => Err("a saved question stays; change its words or answer instead".to_owned()),
            _ => Err("only a field you added can be taken away".to_owned()),
        }
    }

    /// Hides a custom field's value or shows it again. A saved hidden value is loaded first.
    pub fn toggle_hidden(&mut self, engine: &mut Engine) -> Result<bool, String> {
        if self.rows[self.at].slot != Slot::Custom {
            return Err("only a field you added can be hidden or shown".to_owned());
        }
        if self.rows[self.at].unloaded() {
            self.load_secret(engine)?;
        }
        let row = self.row();
        row.secret = !row.secret;
        row.loaded = true;
        Ok(row.secret)
    }

    /// Loads the current value of a secret row into its box.
    pub fn load_secret(&mut self, engine: &mut Engine) -> Result<(), String> {
        let id = self.id.clone();
        let row = self.row();
        if !row.unloaded() || row.slot == Slot::Otp {
            return Ok(());
        }
        let id = id.ok_or("nothing saved to load yet")?;
        let what = match (&row.slot, row.index) {
            (Slot::Notes, _) => Secret::Notes,
            (Slot::Custom, Some(index)) => Secret::Custom(index),
            (Slot::Question, Some(index)) => Secret::Question(index),
            _ => Secret::Field(row.key.clone()),
        };
        let value = engine.reveal(&what, &id)?;
        row.value.set(&value);
        row.original = value;
        row.loaded = true;
        Ok(())
    }

    fn sites(&self, text: &str, apps: &str) -> Vec<Value> {
        let mut out = self.kept_sites.clone();
        for part in text.split([',', '\n']).map(str::trim).filter(|s| !s.is_empty()) {
            let same = self.original_sites.iter().find(|s| site_text(s).as_deref() == Some(part));
            out.push(match same {
                Some(original) => original.clone(),
                None if part.contains("://") => json!({"Url": part}),
                None => json!({"Domain": part.to_lowercase()}),
            });
        }
        for package in apps.split([',', '\n', ' ']).map(str::trim).filter(|s| !s.is_empty()) {
            // The certificate it was saved with stays with it.
            let same = self.original_sites.iter().find(|s| app_package(s) == Some(package));
            out.push(same.cloned().unwrap_or_else(|| json!({"AndroidApp": {"package": package, "cert_sha256": null}})));
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
        for row in self.rows.iter().filter(|r| r.is_named() && r.changed()) {
            if row.name.text().trim().is_empty() {
                return Err(if row.slot == Slot::Question { "a security question needs its words" } else { "give each new field a name" }.to_owned());
            }
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
        let apps_row = self.rows.iter().find(|r| r.slot == Slot::Apps);
        let sites = self.sites(sites_row.map_or("", |r| r.value.text()), apps_row.map_or("", |r| r.value.text()));
        let tags_row = self.rows.iter().find(|r| r.slot == Slot::Tags);
        let tags: Vec<String> = tags_row
            .map(|r| r.value.text().split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned).collect())
            .unwrap_or_default();

        // Folder, color and favorite go with the other shared parts.
        let mut organized = Map::new();
        for row in self.rows.iter().filter(|r| matches!(r.slot, Slot::Folder | Slot::Color | Slot::Favourite)) {
            if self.id.is_some() && !row.changed() {
                continue;
            }
            let value = if row.slot == Slot::Favourite { json!(row.flag) } else { json!(row.choice_key()) };
            if self.id.is_some() || !(value.is_null() || value == json!(false)) {
                organized.insert(row.key.clone(), value);
            }
        }
        let id = match &self.id {
            None => {
                let id = engine.create(&self.kind, &title, &plain, &sites)?;
                self.id = Some(id.clone());
                if !tags.is_empty() {
                    organized.insert("tags".into(), json!(tags));
                }
                if !organized.is_empty() {
                    engine.set_common(&id, Value::Object(organized))?;
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
                if sites_row.is_some_and(Row::changed) || apps_row.is_some_and(Row::changed) {
                    common.insert("sites".into(), json!(sites));
                }
                common.extend(organized);
                if !common.is_empty() {
                    engine.set_common(&id, Value::Object(common))?;
                }
                engine.set_plain(&id, &plain)?;
                id
            }
        };
        for row in self.rows.iter().filter(|r| r.secret && !r.is_named() && r.changed()) {
            let value = row.value.text().as_bytes();
            match row.slot {
                Slot::Notes => engine.set_secret(&id, "notes", value)?,
                Slot::Otp => engine.set_otp(&id, Some(value))?,
                _ => engine.set_secret(&id, &row.key, value)?,
            }
        }
        // A renamed hidden field keeps its value, so it is read before it is written again.
        for at in 0..self.rows.len() {
            if self.rows[at].is_named() && self.rows[at].changed() && self.rows[at].unloaded() {
                let keep = self.at;
                self.at = at;
                let loaded = self.load_secret(engine);
                self.at = keep;
                loaded?;
            }
        }
        // Changed fields are written at the places they were read from, before any removal
        // moves the ones after it up.
        for row in self.rows.iter().filter(|r| r.is_named() && r.changed()) {
            let name = row.name.text().trim();
            let value = row.value.text().as_bytes();
            if row.slot == Slot::Question {
                engine.set_question(&id, row.index, name, value)?;
            } else {
                let kind = match (row.secret, row.custom_kind.as_str()) {
                    (true, _) => "hidden",
                    (false, "hidden" | "") => "text",
                    (false, kind) => kind,
                };
                engine.set_custom(&id, row.index, name, kind, value)?;
            }
        }
        let mut removed = std::mem::take(&mut self.removed_custom);
        removed.sort_unstable_by(|a, b| b.cmp(a));
        for index in removed {
            engine.remove_custom(&id, index)?;
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
        let apps = editor.rows.iter().find(|r| r.slot == Slot::Apps).expect("apps row");
        assert_eq!(apps.value.text(), "p", "an app shows to be edited");
        let sites = editor.sites("a.example.com, https://b.example", "p, com.example.new");
        assert_eq!(sites[0], json!({"Host": "a.example.com"}), "the original kind of match stays");
        assert_eq!(sites[1], json!({"Url": "https://b.example"}));
        assert_eq!(sites[2]["AndroidApp"]["package"], "p");
        assert_eq!(sites[3]["AndroidApp"]["package"], "com.example.new");
        assert_eq!(editor.sites("", "").len(), 0, "an app taken out of the box goes");
    }
}

//! The open vault's screen state: what is listed, what is picked, what is revealed.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use ratatui::widgets::ListState;
use zeroize::Zeroizing;

use crate::engine::{Attachment, Code, Detail, Engine, Folder, Health, Row, Secret};
use crate::generator::Generator;
use crate::glyphs::Glyph;
use crate::input::Input;

/// The tabs along the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Items,
    Codes,
    Generator,
    Health,
}

impl Tab {
    pub const ALL: [Self; 4] = [Self::Items, Self::Codes, Self::Generator, Self::Health];

    pub fn name(self) -> &'static str {
        match self {
            Self::Items => "Items",
            Self::Codes => "Codes",
            Self::Generator => "Generator",
            Self::Health => "Health",
        }
    }
}

/// Which pane has the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Sidebar,
    List,
    Detail,
}

/// How the list is ordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    /// Best match first, as search ranks it.
    Relevance,
    Title,
    Modified,
}

impl Sort {
    pub fn arg(self) -> &'static str {
        match self {
            Self::Relevance => "",
            Self::Title => "title",
            Self::Modified => "modified",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Relevance => "best match",
            Self::Title => "title",
            Self::Modified => "recently changed",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Relevance => Self::Title,
            Self::Title => Self::Modified,
            Self::Modified => Self::Relevance,
        }
    }
}

/// A line in the sidebar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub icon: Glyph,
    pub label: String,
    pub query: String,
    pub count: usize,
    /// A heading, not a filter.
    pub heading: bool,
}

impl Section {
    fn new(icon: Glyph, label: impl Into<String>, query: impl Into<String>, count: usize) -> Self {
        Self { icon, label: label.into(), query: query.into(), count, heading: false }
    }

    fn heading(label: &str) -> Self {
        Self { icon: Glyph::All, label: label.to_owned(), query: String::new(), count: 0, heading: true }
    }
}

/// The kinds, as the sidebar names them.
pub const KINDS: [(&str, &str); 12] = [
    ("login", "Logins"),
    ("card", "Cards"),
    ("identity", "Identities"),
    ("note", "Notes"),
    ("otp", "Authenticator"),
    ("ssh", "SSH keys"),
    ("api", "API keys"),
    ("database", "Databases"),
    ("licence", "Licences"),
    ("recovery", "Recovery codes"),
    ("wifi", "Wi-Fi"),
    ("document", "Documents"),
];

/// A kind's name, singular, for headings.
pub fn kind_name(kind: &str) -> &'static str {
    match kind {
        "login" => "Login",
        "card" => "Card",
        "identity" => "Identity",
        "note" => "Note",
        "otp" => "Authenticator",
        "ssh" => "SSH key",
        "api" => "API key",
        "database" => "Database",
        "licence" => "Licence",
        "recovery" => "Recovery codes",
        "wifi" => "Wi-Fi",
        "document" => "Document",
        _ => "Item",
    }
}

fn quoted(value: &str) -> String {
    if value.contains(char::is_whitespace) { format!("\"{value}\"") } else { value.to_owned() }
}

/// A line in the detail pane.
#[derive(Clone, Debug)]
pub enum Line {
    /// A heading between groups.
    Heading(String),
    /// A value, plain or secret.
    Value {
        label: String,
        /// The value, or `None` for a secret not revealed.
        text: Option<String>,
        secret: Option<Secret>,
        has_value: bool,
    },
    /// The live one-time code.
    Code,
    /// An attachment.
    File(Attachment),
    /// A past value.
    Past { index: usize, field: String, at_ms: u64 },
}

impl Line {
    /// Whether the cursor stops on it.
    pub fn selectable(&self) -> bool {
        !matches!(self, Self::Heading(_))
    }
}

/// Where each part of the screen was last drawn, for the mouse.
#[derive(Clone, Copy, Debug, Default)]
pub struct Areas {
    pub tabs: [Rect; 4],
    pub sidebar: Rect,
    pub list: Rect,
    pub detail: Rect,
    pub codes: Rect,
    /// Columns in the codes grid.
    pub code_cols: u16,
    pub health: Rect,
}

/// The vault screen.
#[derive(Debug)]
pub struct VaultUi {
    pub engine: Box<Engine>,
    pub tab: Tab,
    pub pane: Pane,
    pub sections: Vec<Section>,
    pub section: usize,
    pub search: Input,
    pub searching: bool,
    pub sort: Sort,
    pub rows: Vec<Row>,
    pub list: ListState,
    pub detail: Option<Detail>,
    pub lines: Vec<Line>,
    pub line: usize,
    pub revealed: HashMap<Secret, (Zeroizing<String>, Instant)>,
    /// The code shown in the detail pane.
    pub code: Option<Code>,
    pub codes: Vec<Code>,
    pub code_at: usize,
    pub code_filter: Input,
    pub filtering_codes: bool,
    pub generator: Generator,
    pub health: Option<Health>,
    pub health_group: usize,
    pub health_at: usize,
    pub titles: HashMap<String, (String, String)>,
    pub folders: Vec<Folder>,
    pub shelf_open: bool,
    pub total: usize,
    pub areas: Areas,
    last_pull: Instant,
    last_codes: Instant,
}

/// The health report's groups, with the search word that lists each.
pub const HEALTH_GROUPS: [(&str, &str); 7] = [
    ("Weak passwords", "weak"),
    ("Reused passwords", "reused"),
    ("Not changed in a year", "old"),
    ("No two-factor code", ""),
    ("Plain http addresses", ""),
    ("Duplicates", "duplicate"),
    ("Expiring soon", ""),
];

impl VaultUi {
    pub fn new(engine: Box<Engine>) -> Self {
        let mut ui = Self {
            engine,
            tab: Tab::Items,
            pane: Pane::List,
            sections: Vec::new(),
            section: 0,
            search: Input::new(),
            searching: false,
            sort: Sort::Title,
            rows: Vec::new(),
            list: ListState::default(),
            detail: None,
            lines: Vec::new(),
            line: 0,
            revealed: HashMap::new(),
            code: None,
            codes: Vec::new(),
            code_at: 0,
            code_filter: Input::new(),
            filtering_codes: false,
            generator: Generator::default(),
            health: None,
            health_group: 0,
            health_at: 0,
            titles: HashMap::new(),
            folders: Vec::new(),
            shelf_open: false,
            total: 0,
            areas: Areas::default(),
            last_pull: Instant::now(),
            last_codes: Instant::now() - Duration::from_secs(5),
        };
        ui.reload();
        ui
    }

    /// The selected row.
    pub fn current(&self) -> Option<&Row> {
        self.list.selected().and_then(|at| self.rows.get(at))
    }

    /// The selected row's id.
    pub fn current_id(&self) -> Option<String> {
        self.current().map(|r| r.id.clone())
    }

    /// Whether the trash is what is listed.
    pub fn in_trash(&self) -> bool {
        self.sections.get(self.section).is_some_and(|s| s.query == "trashed")
    }

    fn query(&self) -> String {
        let base = self.sections.get(self.section).map(|s| s.query.as_str()).unwrap_or("");
        format!("{base} {}", self.search.text()).trim().to_owned()
    }

    /// Reads everything again, keeping the selection where it can.
    pub fn reload(&mut self) {
        let keep = self.current_id();
        let all = self.engine.list("", "title").unwrap_or_default();
        self.total = all.len();
        self.titles = all.iter().map(|r| (r.id.clone(), (r.title.clone(), r.kind.clone()))).collect();
        self.folders = self.engine.folders().unwrap_or_default();
        self.build_sections(&all);
        self.refresh_list(keep);
    }

    fn build_sections(&mut self, all: &[Row]) {
        let label = self.sections.get(self.section).map(|s| s.query.clone());
        let count = |f: &dyn Fn(&Row) -> bool| all.iter().filter(|r| f(r)).count();
        let mut out = vec![
            Section::new(Glyph::All, "All items", "", all.len()),
            Section::new(Glyph::Favourite, "Favourites", "favourite", count(&|r| r.favourite)),
            Section::new(Glyph::Codes, "With codes", "has:otp", count(&|r| r.has_otp)),
            Section::new(Glyph::Passkey, "Passkeys", "has:passkey", count(&|r| r.has_passkey)),
        ];
        out.retain(|s| s.count > 0 || s.query.is_empty());
        let kinds: Vec<Section> = KINDS
            .iter()
            .map(|(kind, name)| Section::new(Glyph::Kind(kind), *name, format!("kind:{kind}"), count(&|r| r.kind == *kind)))
            .filter(|s| s.count > 0)
            .collect();
        if !kinds.is_empty() {
            out.push(Section::heading("Kinds"));
            out.extend(kinds);
        }
        if !self.folders.is_empty() {
            out.push(Section::heading("Folders"));
            let mut folders = self.folders.clone();
            folders.sort_by_key(|f| f.name.to_lowercase());
            for folder in &folders {
                let n = count(&|r| r.folder.as_deref() == Some(folder.id.as_str()));
                out.push(Section::new(Glyph::Folder, folder.name.clone(), format!("folder:{}", quoted(&folder.name.to_lowercase())), n));
            }
        }
        let mut tags: BTreeMap<String, usize> = BTreeMap::new();
        for tag in all.iter().flat_map(|r| &r.tags) {
            *tags.entry(tag.clone()).or_default() += 1;
        }
        if !tags.is_empty() {
            out.push(Section::heading("Tags"));
            for (tag, n) in tags {
                out.push(Section::new(Glyph::Tag, tag.clone(), format!("tag:{}", quoted(&tag.to_lowercase())), n));
            }
        }
        out.push(Section::heading("More"));
        if self.shelf_open {
            let hidden = self.engine.list("hidden", "").map(|r| r.len()).unwrap_or(0);
            out.push(Section::new(Glyph::Hidden, "Hidden shelf", "hidden", hidden));
        }
        let trashed = self.engine.list("trashed", "").map(|r| r.len()).unwrap_or(0);
        out.push(Section::new(Glyph::Trash, "Trash", "trashed", trashed));
        self.section = label.and_then(|q| out.iter().position(|s| !s.heading && s.query == q)).unwrap_or(0);
        self.sections = out;
    }

    /// Runs the list query again.
    pub fn refresh_list(&mut self, keep: Option<String>) {
        let query = self.query();
        self.rows = self.engine.list(&query, self.sort.arg()).unwrap_or_default();
        let at = keep
            .and_then(|id| self.rows.iter().position(|r| r.id == id))
            .or_else(|| self.list.selected().map(|at| at.min(self.rows.len().saturating_sub(1))))
            .filter(|_| !self.rows.is_empty())
            .or(if self.rows.is_empty() { None } else { Some(0) });
        self.list.select(at);
        self.load_detail();
    }

    /// Reads the selected entry.
    pub fn load_detail(&mut self) {
        self.revealed.clear();
        let Some(id) = self.current_id() else {
            self.detail = None;
            self.lines.clear();
            self.code = None;
            return;
        };
        let same = self.detail.as_ref().is_some_and(|d| d.row.id == id);
        self.detail = self.engine.detail(&id).ok();
        self.build_lines();
        if !same {
            self.line = self.lines.iter().position(Line::selectable).unwrap_or(0);
        } else {
            self.line = self.line.min(self.lines.len().saturating_sub(1));
        }
        self.refresh_code();
    }

    fn build_lines(&mut self) {
        let Some(detail) = &self.detail else {
            self.lines.clear();
            return;
        };
        let mut lines = Vec::new();
        for field in &detail.fields {
            if !field.has_value && field.input != "toggle" {
                continue;
            }
            let text = if field.secret {
                None
            } else {
                Some(match &field.value {
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(serde_json::Value::Bool(b)) => if *b { "Yes" } else { "No" }.to_owned(),
                    Some(serde_json::Value::Number(n)) if field.input == "date" => {
                        n.as_u64().map(crate::editor::date_text).unwrap_or_default()
                    }
                    Some(other) => other.to_string(),
                    None => String::new(),
                })
            };
            lines.push(Line::Value {
                label: field.label.clone(),
                text,
                secret: field.secret.then(|| Secret::Field(field.key.clone())),
                has_value: field.has_value,
            });
        }
        if detail.otp.is_some() {
            lines.push(Line::Code);
        }
        let sites: Vec<String> = detail
            .sites
            .iter()
            .filter_map(|s| {
                let (kind, value) = s.as_object()?.iter().next()?;
                Some(match kind.as_str() {
                    "AndroidApp" => format!("app {}", value["package"].as_str().unwrap_or("")),
                    "Regex" => format!("pattern {}", value.as_str().unwrap_or("")),
                    _ => value.as_str()?.to_owned(),
                })
            })
            .collect();
        for (n, site) in sites.into_iter().enumerate() {
            lines.push(Line::Value {
                label: if n == 0 { "Website".to_owned() } else { String::new() },
                text: Some(site),
                secret: None,
                has_value: true,
            });
        }
        if let Some(alias) = &detail.alias {
            lines.push(Line::Value { label: "Alias".into(), text: Some(alias.clone()), secret: None, has_value: true});
        }
        if !detail.custom_fields.is_empty() || !detail.questions.is_empty() {
            lines.push(Line::Heading("More fields".into()));
        }
        for field in &detail.custom_fields {
            lines.push(Line::Value {
                label: field.name.clone(),
                text: field.value.clone(),
                secret: (field.kind == "hidden").then_some(Secret::Custom(field.index)),
                has_value: field.has_value,
            });
        }
        for (index, question) in detail.questions.iter().enumerate() {
            lines.push(Line::Value {
                label: question.clone(),
                text: None,
                secret: Some(Secret::Question(index)),
                has_value: true,
            });
        }
        if detail.has_notes && detail.row.kind != "note" {
            lines.push(Line::Heading("Notes".into()));
            lines.push(Line::Value { label: "Notes".into(), text: None, secret: Some(Secret::Notes), has_value: true});
        }
        if !detail.passkeys.is_empty() {
            lines.push(Line::Heading("Passkeys".into()));
            for key in &detail.passkeys {
                lines.push(Line::Value {
                    label: key.rp_id.clone(),
                    text: Some(key.user_name.clone()),
                    secret: None,
                    has_value: true,
                });
            }
        }
        if !detail.attachment_list.is_empty() {
            lines.push(Line::Heading("Files".into()));
            lines.extend(detail.attachment_list.iter().cloned().map(Line::File));
        }
        if !detail.history.is_empty() {
            lines.push(Line::Heading("Earlier values".into()));
            for past in detail.history.iter().rev() {
                lines.push(Line::Past { index: past.index, field: past.field.clone(), at_ms: past.at_ms });
            }
        }
        self.lines = lines;
    }

    /// The code shown in the detail pane, fresh.
    pub fn refresh_code(&mut self) {
        self.code = match (&self.detail, self.current_id()) {
            (Some(d), Some(id)) if d.otp.is_some() => self.engine.codes(Some(&[id])).ok().and_then(|mut c| c.pop()),
            _ => None,
        };
    }

    /// Every live code, filtered.
    pub fn refresh_codes(&mut self) {
        let filter = self.code_filter.text().to_lowercase();
        let mut codes = self.engine.codes(None).unwrap_or_default();
        codes.retain(|c| {
            filter.is_empty()
                || c.title.to_lowercase().contains(&filter)
                || c.otp.issuer.to_lowercase().contains(&filter)
                || c.otp.account.to_lowercase().contains(&filter)
        });
        codes.sort_by_key(|c| c.title.to_lowercase());
        self.codes = codes;
        self.code_at = self.code_at.min(self.codes.len().saturating_sub(1));
    }

    /// The selected detail line's secret, revealed, if any.
    pub fn revealed_text(&self, secret: &Secret) -> Option<&str> {
        self.revealed.get(secret).map(|(v, _)| v.as_str())
    }

    /// Shows or hides a secret.
    pub fn toggle_reveal(&mut self, secret: Secret) -> Result<(), String> {
        if self.revealed.remove(&secret).is_some() {
            return Ok(());
        }
        let id = self.current_id().ok_or("nothing is picked")?;
        let value = self.engine.reveal(&secret, &id)?;
        self.revealed.insert(secret, (value, Instant::now()));
        Ok(())
    }

    /// Hides secrets shown longer than `keep`.
    pub fn expire(&mut self, keep: Duration) -> bool {
        let before = self.revealed.len();
        self.revealed.retain(|_, (_, at)| at.elapsed() < keep);
        before != self.revealed.len()
    }

    /// Moves the list cursor.
    pub fn move_list(&mut self, by: isize) {
        if self.rows.is_empty() {
            return;
        }
        let at = self.list.selected().unwrap_or(0).cast_signed();
        let next = (at + by).clamp(0, self.rows.len().cast_signed() - 1).cast_unsigned();
        if Some(next) != self.list.selected() {
            self.list.select(Some(next));
            self.load_detail();
        }
    }

    /// Moves the sidebar cursor, stepping over headings.
    pub fn move_section(&mut self, by: isize) {
        let len = self.sections.len().cast_signed();
        let mut at = self.section.cast_signed();
        loop {
            at += by.signum();
            if at < 0 || at >= len {
                return;
            }
            if !self.sections[at.cast_unsigned()].heading {
                break;
            }
        }
        self.section = at.cast_unsigned();
        self.list.select(Some(0));
        self.refresh_list(None);
    }

    /// Moves the detail cursor, stepping over headings.
    pub fn move_line(&mut self, by: isize) {
        let len = self.lines.len().cast_signed();
        let mut at = self.line.cast_signed();
        loop {
            at += by.signum();
            if at < 0 || at >= len {
                return;
            }
            if self.lines[at.cast_unsigned()].selectable() {
                break;
            }
        }
        self.line = at.cast_unsigned();
    }

    /// Housekeeping, about four times a second.
    pub fn tick(&mut self, reveal_for: Duration) -> (bool, Option<String>) {
        let mut redraw = self.expire(reveal_for);
        let mut news = None;
        if self.last_pull.elapsed() >= Duration::from_secs(3) {
            self.last_pull = Instant::now();
            match self.engine.pull() {
                Ok(true) => {
                    self.reload();
                    redraw = true;
                    news = Some("Changes made elsewhere were merged in".to_owned());
                }
                Ok(false) => {}
                Err(message) => news = Some(message),
            }
        }
        if self.last_codes.elapsed() >= Duration::from_secs(1) {
            self.last_codes = Instant::now();
            if self.tab == Tab::Codes {
                self.refresh_codes();
                redraw = true;
            } else if self.code.is_some() && self.tab == Tab::Items {
                self.refresh_code();
                redraw = true;
            }
        }
        (redraw, news)
    }

    /// The ids in a health group.
    pub fn health_ids(&self, group: usize) -> Vec<String> {
        let Some(h) = &self.health else { return Vec::new() };
        match group {
            0 => h.weak.clone(),
            1 => h.reused.iter().flatten().cloned().collect(),
            2 => h.old.clone(),
            3 => h.missing_two_factor.clone(),
            4 => h.insecure_url.clone(),
            5 => h.duplicates.iter().flatten().cloned().collect(),
            6 => h.expiring.clone(),
            _ => Vec::new(),
        }
    }

    /// Shows an entry in the Items tab, whatever was filtered before.
    pub fn show_entry(&mut self, id: &str) {
        self.tab = Tab::Items;
        self.section = 0;
        self.search.clear();
        self.refresh_list(Some(id.to_owned()));
        self.pane = Pane::Detail;
    }
}

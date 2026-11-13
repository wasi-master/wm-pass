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
    Settings,
}

impl Tab {
    pub const ALL: [Self; 5] = [Self::Items, Self::Codes, Self::Generator, Self::Health, Self::Settings];

    pub fn name(self) -> &'static str {
        match self {
            Self::Items => "Items",
            Self::Codes => "Codes",
            Self::Generator => "Generator",
            Self::Health => "Health",
            Self::Settings => "Settings",
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
    /// For a folder, its id. `folder:` in a query finds every folder whose name holds the
    /// words, so "Work" would list "Workshop" too and two folders called "Mail" would list as
    /// one; the rows are kept to this folder by id.
    pub folder: Option<String>,
}

impl Section {
    fn new(icon: Glyph, label: impl Into<String>, query: impl Into<String>, count: usize) -> Self {
        Self { icon, label: label.into(), query: query.into(), count, heading: false, folder: None }
    }

    fn heading(label: &str) -> Self {
        Self { icon: Glyph::All, label: label.to_owned(), query: String::new(), count: 0, heading: true, folder: None }
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
    ("licence", "Licenses"),
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
        "licence" => "License",
        "recovery" => "Recovery codes",
        "wifi" => "Wi-Fi",
        "document" => "Document",
        _ => "Item",
    }
}

/// A kind's name inside a sentence: "New login", but "New SSH key" and "New Wi-Fi", whose
/// capitals are part of the name.
pub fn kind_in_text(kind: &str) -> String {
    let name = kind_name(kind);
    let first_word = name.split(' ').next().unwrap_or(name);
    if first_word.chars().skip(1).any(char::is_uppercase) {
        return name.to_owned();
    }
    let mut chars = name.chars();
    chars.next().map(|first| first.to_lowercase().chain(chars).collect()).unwrap_or_default()
}

pub fn quoted(value: &str) -> String {
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
    /// Another item this one lists under Linked, by id.
    Link(String),
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
    pub tabs: [Rect; 5],
    pub settings: Rect,
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
    /// Items marked with space, for acting on together. Kept while the same section is listed.
    pub marked: Vec<String>,
    /// The section the marks were made in.
    marked_in: String,
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
    /// What the Ignored group lists, as `check:id`, kept in step with the settings.
    pub health_ignored: Vec<String>,
    /// The setting picked in the Settings tab.
    pub settings_at: usize,
    /// The vault's own settings, read when the Settings tab opens.
    pub info: Option<crate::engine::VaultInfo>,
    pub titles: HashMap<String, (String, String)>,
    pub folders: Vec<Folder>,
    pub shelf_open: bool,
    pub total: usize,
    pub areas: Areas,
    last_pull: Instant,
    /// Set when changes from elsewhere came in, until the app has taken their settings.
    pub merged: bool,
    /// What the vault has to tell its owner, kept on the status line until it is resolved.
    pub notices: Vec<String>,
    last_codes: Instant,
}

/// The health report's groups, with the search word that lists each and the check's name, the
/// one every app files an ignored item under. The last group lists what was ignored.
pub const HEALTH_GROUPS: [(&str, &str, &str); 8] = [
    ("Weak passwords", "weak", "weak"),
    ("Reused passwords", "reused", "reused"),
    ("Not changed in a year", "old", "old"),
    ("No two-factor code", "", "twofactor"),
    ("Plain http addresses", "", "insecure"),
    ("Duplicates", "duplicate", "duplicates"),
    ("Expiring soon", "", "expiring"),
    ("Ignored", "", ""),
];

/// The group that lists ignored items.
pub const IGNORED_GROUP: usize = HEALTH_GROUPS.len() - 1;

/// A check's name as the Health tab shows it.
pub fn check_label(check: &str) -> &str {
    HEALTH_GROUPS.iter().find(|group| group.2 == check && !check.is_empty()).map_or(check, |group| group.0)
}

/// One line of a health group: an item, or the heading over items sharing one password.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HealthLine {
    Heading(String),
    /// An item, with the check it was ignored for in the Ignored group.
    Item { id: String, check: Option<String> },
}

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
            marked: Vec::new(),
            marked_in: String::new(),
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
            health_ignored: Vec::new(),
            settings_at: 0,
            info: None,
            titles: HashMap::new(),
            folders: Vec::new(),
            shelf_open: false,
            total: 0,
            areas: Areas::default(),
            last_pull: Instant::now(),
            merged: false,
            notices: Vec::new(),
            last_codes: Instant::now() - Duration::from_secs(5),
        };
        ui.engine.name_device(&crate::join::device_name());
        ui.reload();
        ui.notices = ui.engine.notices();
        ui
    }

    /// The selected row.
    pub fn current(&self) -> Option<&Row> {
        self.list.selected().and_then(|at| self.rows.get(at))
    }

    /// The selected row's id.
    /// The tag picked in the sidebar, if a tag is what is picked.
    pub fn picked_tag(&self) -> Option<String> {
        let section = self.sections.get(self.section)?;
        (section.icon == Glyph::Tag && !section.heading).then(|| section.label.clone())
    }

    pub fn current_id(&self) -> Option<String> {
        self.current().map(|r| r.id.clone())
    }

    /// What a command acts on: the marked items, or else the selected one.
    pub fn targets(&self) -> Vec<String> {
        if self.marked.is_empty() { self.current_id().into_iter().collect() } else { self.marked.clone() }
    }

    /// Marks the selected row, or unmarks it, and steps down to the next.
    pub fn toggle_mark(&mut self) {
        let Some(id) = self.current_id() else { return };
        self.marked_in = self.section_query();
        if let Some(at) = self.marked.iter().position(|m| *m == id) {
            self.marked.remove(at);
        } else {
            self.marked.push(id);
        }
        self.move_list(1);
    }

    /// Marks every row listed, or, when they all are already, none.
    pub fn mark_all(&mut self) {
        let all = !self.rows.is_empty() && self.rows.iter().all(|r| self.marked.contains(&r.id));
        if all {
            self.marked.clear();
        } else {
            self.marked_in = self.section_query();
            for row in &self.rows {
                if !self.marked.contains(&row.id) {
                    self.marked.push(row.id.clone());
                }
            }
        }
    }

    pub fn is_marked(&self, id: &str) -> bool {
        self.marked.iter().any(|m| m == id)
    }

    fn section_query(&self) -> String {
        self.sections.get(self.section).map(|s| s.query.clone()).unwrap_or_default()
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
        // An item trashed, deleted or merged away is no longer marked.
        let rows: Vec<&str> = self.rows.iter().map(|r| r.id.as_str()).collect();
        self.marked.retain(|id| self.titles.contains_key(id) || rows.contains(&id.as_str()));
    }

    fn build_sections(&mut self, all: &[Row]) {
        let label = self.sections.get(self.section).map(|s| (s.query.clone(), s.folder.clone()));
        let count = |f: &dyn Fn(&Row) -> bool| all.iter().filter(|r| f(r)).count();
        let mut out = vec![
            Section::new(Glyph::All, "All items", "", all.len()),
            Section::new(Glyph::Favourite, "Favorites", "favourite", count(&|r| r.favourite)),
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
                let mut section = Section::new(Glyph::Folder, folder.name.clone(), format!("folder:{}", quoted(&folder.name.to_lowercase())), n);
                section.folder = Some(folder.id.clone());
                out.push(section);
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
        self.section = label.and_then(|(q, f)| out.iter().position(|s| !s.heading && s.query == q && s.folder == f)).unwrap_or(0);
        self.sections = out;
    }

    /// Runs the list query again.
    pub fn refresh_list(&mut self, keep: Option<String>) {
        if !self.marked.is_empty() && self.marked_in != self.section_query() {
            self.marked.clear();
        }
        let query = self.query();
        self.rows = self.engine.list(&query, self.sort.arg()).unwrap_or_default();
        if let Some(folder) = self.sections.get(self.section).and_then(|s| s.folder.as_deref()) {
            self.rows.retain(|r| r.folder.as_deref() == Some(folder));
        }
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
        // Websites first, then apps by the names people know them by.
        let mut sites: Vec<(bool, String)> = detail
            .sites
            .iter()
            .filter_map(|s| {
                let (kind, value) = s.as_object()?.iter().next()?;
                Some(match kind.as_str() {
                    "AndroidApp" => {
                        let package = value["package"].as_str().unwrap_or("");
                        let text = match wmpass_core::related::app_name(package) {
                            Some(name) => format!("{name} ({package})"),
                            None => package.to_owned(),
                        };
                        (true, text)
                    }
                    "Regex" => (false, format!("pattern {}", value.as_str().unwrap_or(""))),
                    _ => (false, value.as_str()?.to_owned()),
                })
            })
            .collect();
        sites.sort_by_key(|(app, _)| *app);
        let mut last = None;
        for (app, site) in sites {
            let label = if last == Some(app) {
                String::new()
            } else if app {
                "App".to_owned()
            } else {
                "Website".to_owned()
            };
            last = Some(app);
            lines.push(Line::Value { label, text: Some(site), secret: None, has_value: true });
        }
        // The page the site keeps for changing the password, to copy into a browser.
        if let Some(url) = &detail.change_password {
            lines.push(Line::Value { label: "Change password".into(), text: Some(url.clone()), secret: None, has_value: true });
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
        // Linked items that are still there; one deleted for good drops out.
        let links: Vec<Line> = detail.links.iter().filter(|id| self.titles.contains_key(*id)).map(|id| Line::Link(id.clone())).collect();
        if !links.is_empty() {
            lines.push(Line::Heading("Linked".into()));
            lines.extend(links);
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
                    self.merged = true;
                    self.reload();
                    redraw = true;
                    let notices = self.engine.notices();
                    // Something the merge brought that was not said yet is the news; otherwise
                    // that the merge happened.
                    news = Some(
                        notices
                            .iter()
                            .find(|notice| !self.notices.contains(notice))
                            .cloned()
                            .unwrap_or_else(|| "Changes made elsewhere were merged in".to_owned()),
                    );
                    self.notices = notices;
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

    /// How many groups the Health tab lists: Ignored only when something is.
    pub fn health_groups(&self) -> usize {
        if self.health_ignored.is_empty() { IGNORED_GROUP } else { HEALTH_GROUPS.len() }
    }

    /// The items in a health group, in the order the cursor moves through them.
    pub fn health_ids(&self, group: usize) -> Vec<String> {
        self.health_items(group).into_iter().map(|(id, _)| id).collect()
    }

    /// The items in a health group, each with the check it was ignored for in the Ignored group.
    pub fn health_items(&self, group: usize) -> Vec<(String, Option<String>)> {
        self.health_lines(group)
            .into_iter()
            .filter_map(|line| match line {
                HealthLine::Item { id, check } => Some((id, check)),
                HealthLine::Heading(_) => None,
            })
            .collect()
    }

    /// The lines of a health group. Reused passwords come one group per password, each under a
    /// heading, so each group is one password to change.
    pub fn health_lines(&self, group: usize) -> Vec<HealthLine> {
        let item = |id: &String| HealthLine::Item { id: id.clone(), check: None };
        if group == IGNORED_GROUP {
            return self
                .health_ignored
                .iter()
                .filter_map(|pair| pair.split_once(':'))
                .map(|(check, id)| HealthLine::Item { id: id.to_owned(), check: Some(check.to_owned()) })
                .collect();
        }
        let Some(h) = &self.health else { return Vec::new() };
        match group {
            0 => h.weak.iter().map(item).collect(),
            1 => h
                .reused
                .iter()
                .flat_map(|set| {
                    std::iter::once(HealthLine::Heading(format!("{} items share one password", set.len()))).chain(set.iter().map(item))
                })
                .collect(),
            2 => h.old.iter().map(item).collect(),
            3 => h.missing_two_factor.iter().map(item).collect(),
            4 => h.insecure_url.iter().map(item).collect(),
            5 => h.duplicates.iter().flatten().map(item).collect(),
            6 => h.expiring.iter().map(item).collect(),
            _ => Vec::new(),
        }
    }

    /// Shows an entry in the Items tab, whatever was filtered before. Returns false, changing
    /// nothing, when it is not in the list, such as an item deleted for good: the first item
    /// listed must not stand in for it.
    pub fn show_entry(&mut self, id: &str) -> bool {
        if self.engine.detail(id).is_err() {
            return false;
        }
        self.tab = Tab::Items;
        self.section = 0;
        self.search.clear();
        self.refresh_list(Some(id.to_owned()));
        if self.current_id().as_deref() != Some(id) {
            // In the trash, or on the shelf: look there.
            for query in ["trashed", "hidden"] {
                if let Some(at) = self.sections.iter().position(|s| s.query == query) {
                    self.section = at;
                    self.refresh_list(Some(id.to_owned()));
                    if self.current_id().as_deref() == Some(id) {
                        break;
                    }
                }
            }
        }
        if self.current_id().as_deref() != Some(id) {
            self.section = 0;
            self.refresh_list(None);
            return false;
        }
        self.pane = Pane::Detail;
        true
    }
}

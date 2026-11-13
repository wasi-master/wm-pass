//! Everything drawn. No state changes here except where things were drawn, for the mouse,
//! and the list's scroll position, which ratatui keeps in its state.

use std::cell::RefCell;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};

use crate::app::{App, COMMANDS, Level, Overlay, Screen, palette_matches};
use crate::door::{self, Door, Field};
use crate::editor::{Editor, Slot, date_text};
use crate::engine::{Code, Secret};
use crate::generator::Knob;
use crate::glyphs::Glyph;
use crate::input::Input;
use crate::settings::{self, Look, Setting};
use crate::join::{QUIET, Qr};
use crate::link::{LinkUi, Role, Stage};
use crate::theme::Theme;
use crate::vault::{self, HEALTH_GROUPS, Line as DLine, Pane, Tab, VaultUi};

const MASK: &str = "••••••••••••";

/// Something on screen a click lands on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    /// Does what this key does.
    Key(KeyEvent),
    DoorField(Field),
    Recent(usize),
    Section(usize),
    Row(usize),
    Line(usize),
    Code(usize),
    Knob(usize),
    HealthGroup(usize),
    HealthEntry(usize),
    Setting(usize),
    PaletteRow(usize),
    PickRow(usize),
    EditorRow(usize),
    /// A custom field's name, or a security question's words.
    EditorName(usize),
    LinkDevice(usize),
    /// An overlay's own area, so a click on it reaches nothing beneath.
    Cover,
}

/// Where things were drawn this frame, in drawing order, and where the overlay's begin.
type Hits = (Vec<(Rect, Hit)>, Option<usize>);

thread_local! {
    /// Whether the last QR code drawn used big cells.
    static DREW_BIG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// How far the keys list could scroll in the last frame.
    static HELP_MOST: std::cell::Cell<u16> = const { std::cell::Cell::new(0) };
    static HITS: RefCell<Hits> = const { RefCell::new((Vec::new(), None)) };
}

/// Notes that a click inside `rect` means `hit`. Later notes sit on top of earlier ones.
fn hit(rect: Rect, what: Hit) {
    if rect.width > 0 && rect.height > 0 {
        HITS.with(|h| h.borrow_mut().0.push((rect, what)));
    }
}

/// What a click at `at` lands on: the topmost thing drawn there. With an overlay open, only
/// what it drew counts, and `Some(None)` means the click fell outside it.
pub fn hit_at(hits: &[(Rect, Hit)], overlay_from: Option<usize>, at: Position) -> Option<Option<Hit>> {
    let from = overlay_from.unwrap_or(0).min(hits.len());
    let found = hits[from..].iter().rev().find(|(r, _)| r.contains(at)).map(|(_, h)| h.clone());
    match (overlay_from, found) {
        (Some(_), found) => Some(found),
        (None, Some(found)) => Some(Some(found)),
        (None, None) => None,
    }
}

/// The key a hint names, when a click can press it.
fn key_of(name: &str) -> Option<KeyEvent> {
    let code = match name {
        "Enter" => KeyCode::Enter,
        "Esc" => KeyCode::Esc,
        "Tab" => KeyCode::Tab,
        "Space" => KeyCode::Char(' '),
        _ => {
            if let Some(rest) = name.strip_prefix('^')
                && let [ch] = rest.chars().collect::<Vec<_>>()[..]
            {
                return Some(KeyEvent::new(KeyCode::Char(ch.to_ascii_lowercase()), KeyModifiers::CONTROL));
            }
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) => KeyCode::Char(ch),
                _ => return None,
            }
        }
    };
    Some(KeyEvent::new(code, KeyModifiers::NONE))
}

/// As many hints, from the first, as fit in `width` whole: a hint cut off part way, as
/// "Esc qui", reads as a different key.
fn fitting<'k, 'a>(theme: &Theme, keys: &'k [(&'a str, &'a str)], width: u16) -> &'k [(&'a str, &'a str)] {
    let mut n = keys.len();
    while n > 0 && key_hint(theme, &keys[..n]).width() > usize::from(width) {
        n -= 1;
    }
    &keys[..n]
}

/// A row of key hints, each one a button. Hints that do not fit whole are left off.
fn hint_bar(frame: &mut Frame, area: Rect, theme: &Theme, keys: &[(&str, &str)], align: Alignment) {
    let keys = fitting(theme, keys, area.width);
    let line = key_hint(theme, keys);
    let width = (line.width() as u16).min(area.width);
    let start = match align {
        Alignment::Left => area.x,
        Alignment::Center => area.x + (area.width - width) / 2,
        Alignment::Right => area.right() - width,
    };
    frame.render_widget(Paragraph::new(line), Rect { x: start, width, height: 1, ..area });
    let mut x = start;
    for (n, (key, what)) in keys.iter().enumerate() {
        if n > 0 {
            x += 2;
        }
        let w = (key.chars().count() + 1 + what.chars().count()) as u16;
        if let Some(event) = key_of(key) {
            hit(Rect { x, y: area.y, width: w.min(area.right().saturating_sub(x)), height: 1 }, Hit::Key(event));
        }
        x += w;
    }
}

/// Draws a frame.
pub fn draw(frame: &mut Frame, app: &mut App) {
    HITS.with(|h| *h.borrow_mut() = (Vec::new(), None));
    draw_all(frame, app);
    let (hits, from) = HITS.with(|h| std::mem::take(&mut *h.borrow_mut()));
    app.hits = hits;
    app.overlay_from = from;
    app.link_drew_big = DREW_BIG.get();
}

fn draw_all(frame: &mut Frame, app: &mut App) {
    let theme = app.theme;
    let area = frame.area();
    frame.render_widget(Block::new().style(theme.base()), area);
    if area.width < 40 || area.height < 12 {
        let text = Paragraph::new("Make the window a little bigger").style(theme.dim()).alignment(Alignment::Center).wrap(Wrap { trim: true });
        frame.render_widget(text, centered(area, area.width, 2));
        return;
    }
    let lock_in = app.lock_in();
    let clip_left = app.clip.seconds_left();
    match &mut app.screen {
        Screen::Door(door) => draw_door(frame, area, door, &theme, &app.prefs.recent),
        Screen::Vault(ui) => {
            let around = Around { lock_in, clip_left, toast: app.toast.as_ref(), prefs: &app.prefs };
            draw_vault(frame, area, ui, &theme, &around);
        }
    }
    if let Screen::Door(_) = app.screen
        && let Some(toast) = &app.toast
    {
        let bottom = Rect { y: area.bottom() - 1, height: 1, ..area };
        frame.render_widget(Paragraph::new(toast_line(toast, &theme)).alignment(Alignment::Center), bottom);
    }
    if let Some(overlay) = &app.overlay {
        HITS.with(|h| {
            let mut h = h.borrow_mut();
            h.1 = Some(h.0.len());
        });
        let ui = match &app.screen {
            Screen::Vault(ui) => Some(&**ui),
            Screen::Door(_) => None,
        };
        draw_overlay(frame, area, overlay, &theme, ui);
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

fn panel<'a>(theme: &Theme, title: impl Into<Line<'a>>, focused: bool) -> Block<'a> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.border(focused))
        .title(title)
        .style(theme.panel())
}

fn key_hint<'a>(theme: &Theme, keys: &[(&'a str, &'a str)]) -> Line<'a> {
    let mut spans = Vec::new();
    for (n, (key, what)) in keys.iter().enumerate() {
        if n > 0 {
            spans.push(Span::styled("  ", theme.faint()));
        }
        spans.push(Span::styled(*key, theme.key()));
        spans.push(Span::styled(format!(" {what}"), theme.faint()));
    }
    Line::from(spans)
}

/// A bar of `width` cells, `ratio` of it filled.
fn bar<'a>(width: u16, ratio: f64, filled: Color, theme: &Theme) -> Vec<Span<'a>> {
    let width = usize::from(width);
    let full = ((ratio.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    vec![
        Span::styled("━".repeat(full), Style::new().fg(filled)),
        Span::styled("─".repeat(width - full), Style::new().fg(theme.line)),
    ]
}

/// A strength meter of five segments.
fn meter<'a>(score: u8, theme: &Theme) -> Vec<Span<'a>> {
    let colour = theme.score(score);
    (0..5u8)
        .map(|n| {
            let on = n <= score;
            Span::styled(if n == 4 { "▰" } else { "▰ " }, Style::new().fg(if on { colour } else { theme.line }))
        })
        .collect()
}

fn score_word(score: u8) -> &'static str {
    match score {
        0 => "Very weak",
        1 => "Weak",
        2 => "Fair",
        3 => "Strong",
        _ => "Very strong",
    }
}

fn toast_line<'a>(toast: &crate::app::Toast, theme: &Theme) -> Line<'a> {
    let (icon, colour) = match toast.level {
        Level::Info => ("●", theme.info),
        Level::Good => ("✓", theme.ok),
        Level::Warn => ("!", theme.warn),
        Level::Bad => ("✕", theme.danger),
    };
    Line::from(vec![
        Span::styled(format!(" {icon} "), Style::new().fg(colour).add_modifier(Modifier::BOLD)),
        Span::styled(toast.text.clone(), Style::new().fg(theme.text)),
    ])
}

fn shield(theme: &Theme) -> Vec<Line<'static>> {
    const ART: [&str; 6] = ["▗▄▄▄▄▄▄▄▄▄▄▖", "▐██████████▌", "▐████▀▀████▌", "▐████  ████▌", " ▜████████▛ ", "   ▀▜██▛▀   "];
    ART.iter()
        .enumerate()
        .map(|(n, row)| {
            let t = n as f32 / (ART.len() - 1) as f32;
            Line::from(Span::styled(*row, Style::new().fg(theme.gradient(t)))).centered()
        })
        .collect()
}

fn brand_word(theme: &Theme) -> Line<'static> {
    let word = "W M   P A S S";
    let len = word.chars().count().max(2) - 1;
    let spans: Vec<Span> = word
        .chars()
        .enumerate()
        .map(|(n, ch)| Span::styled(ch.to_string(), Style::new().fg(theme.gradient(n as f32 / len as f32)).add_modifier(Modifier::BOLD)))
        .collect();
    Line::from(spans).centered()
}

/// How a text box looks.
struct BoxLook<'a> {
    label: &'a str,
    secret: bool,
    focused: bool,
    placeholder: &'a str,
}

/// What a one-line box shows of `input` in `width` cells, and how many cells in its cursor is.
/// A long line scrolls to keep the cursor in sight, and wide letters count two cells.
fn visible(input: &Input, secret: bool, width: usize) -> (String, usize) {
    let (line, col) = input.cursor_line_col();
    let row = input.text().split('\n').nth(line).unwrap_or("");
    if secret {
        // Each letter shows as one dot, however wide it is.
        let before: String = input.text().chars().take(input.cursor()).collect();
        let at = before.rsplit('\n').next().map_or(0, |l| l.chars().count());
        return crate::input::scrolled(&"•".repeat(row.chars().count()), at, width);
    }
    crate::input::scrolled(row, col, width)
}

/// Draws a text box; returns where the cursor goes.
fn text_box(frame: &mut Frame, area: Rect, theme: &Theme, input: &Input, look: &BoxLook<'_>) -> Option<Position> {
    let BoxLook { label, secret, focused, placeholder } = *look;
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.border(focused))
        .title(Span::styled(format!(" {label} "), if focused { theme.brand() } else { theme.dim() }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let (row, at) = visible(input, secret, usize::from(inner.width));
    let text = if input.is_empty() {
        Line::from(Span::styled(placeholder.to_owned(), theme.faint()))
    } else {
        Line::from(Span::styled(row, Style::new().fg(theme.text)))
    };
    frame.render_widget(Paragraph::new(text), inner);
    focused.then(|| Position::new(inner.x + at as u16, inner.y))
}

fn draw_door(frame: &mut Frame, area: Rect, door: &Door, theme: &Theme, recent: &[std::path::PathBuf]) {
    let fields = door.fields();
    let extra = if door.mode == door::Mode::Create { 2 } else { 0 };
    let height = (8 + 3 * fields.len() as u16 + 6 + extra).min(area.height);
    let card = centered(area, 64, height);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.line))
        .style(theme.panel());
    let inner = block.inner(card).inner(Margin::new(2, 0));
    frame.render_widget(block, card);

    let mut y = inner.y;
    let logo = shield(theme);
    let logo_h = (logo.len() as u16).min(inner.height);
    frame.render_widget(Paragraph::new(logo), Rect { y, height: logo_h, ..inner });
    y += logo_h;
    frame.render_widget(Paragraph::new(brand_word(theme)), Rect { y, height: 1, ..inner });
    y += 1;
    let subtitle = match door.mode {
        door::Mode::Unlock if door::place_label(door.path.text()).is_some() => "Unlock the WM Pass app's vault".to_owned(),
        door::Mode::Unlock => {
            let name = std::path::Path::new(door.path.text())
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "your vault".into());
            format!("Unlock {name}")
        }
        door::Mode::Create => "A new vault, on this device only".to_owned(),
    };
    frame.render_widget(Paragraph::new(Line::from(Span::styled(subtitle, theme.dim())).centered()), Rect { y, height: 1, ..inner });
    y += 2;

    let mut cursor = None;
    for field in &fields {
        if y + 3 > inner.bottom() {
            break;
        }
        let rect = Rect { y, height: 3, ..inner };
        let focused = door.focus == *field && door.recent.is_none() && door.busy.is_none();
        let (label, input, secret, hint) = match field {
            Field::Path => ("Vault file", &door.path, false, door::default_new_path().to_string_lossy().into_owned()),
            Field::Name => ("Name", &door.name, false, "Personal".to_owned()),
            Field::Password => ("Password", &door.password, true, String::new()),
            Field::Confirm => ("Password again", &door.confirm, true, String::new()),
            Field::KeyFile => ("Key file", &door.key_file, false, "Path to the key file".to_owned()),
        };
        // The app's folder reads as what it is until the box is picked to change it.
        let shown;
        let input = match door::place_label(input.text()) {
            Some(place) if *field == Field::Path && !focused => {
                shown = Input::with(place);
                &shown
            }
            _ => input,
        };
        if let Some(at) = text_box(frame, rect, theme, input, &BoxLook { label, secret, focused, placeholder: &hint }) {
            cursor = Some(at);
        }
        hit(rect, Hit::DoorField(*field));
        y += 3;
        if *field == Field::Password
            && door.mode == door::Mode::Create
            && let Some((score, time)) = &door.strength
        {
            let mut spans = meter(*score, theme);
            spans.push(Span::styled(format!("  {}", score_word(*score)), Style::new().fg(theme.score(*score))));
            spans.push(Span::styled(format!(" · {time} to guess"), theme.faint()));
            frame.render_widget(Paragraph::new(Line::from(spans)), Rect { y, height: 1, x: inner.x + 1, width: inner.width - 1 });
            y += 1;
        }
    }
    y += 1;
    let status: Line = if let Some(busy) = door.busy {
        Line::from(Span::styled(format!("◐ {busy}"), theme.brand())).centered()
    } else if let Some(error) = &door.error {
        Line::from(Span::styled(format!("✕ {error}"), Style::new().fg(theme.danger))).centered()
    } else if let Some(notice) = &door.notice {
        Line::from(Span::styled(notice.clone(), Style::new().fg(theme.info))).centered()
    } else {
        Line::default()
    };
    if y < inner.bottom() {
        frame.render_widget(Paragraph::new(status).wrap(Wrap { trim: true }), Rect { y, height: 2.min(inner.bottom() - y), ..inner });
    }
    let hints = match door.mode {
        door::Mode::Unlock => {
            // In the order they matter, as a narrow window shows only the first few.
            let mut keys = vec![("Enter", "unlock"), ("Tab", "next"), ("Esc", "quit"), ("^N", "new vault")];
            if !recent.is_empty() {
                keys.push(("^R", "recent"));
            }
            keys.extend([("^G", "from another device"), ("^B", "from a backup"), ("^F", "key file")]);
            keys
        }
        door::Mode::Create => vec![("Enter", "next"), ("Tab", "move"), ("Esc", "back")],
    };
    let below = Rect { y: card.bottom().min(area.bottom() - 1), height: 1, ..area };
    if below.y < area.bottom() {
        hint_bar(frame, below, theme, &hints, Alignment::Center);
    }
    if let Some(at) = door.recent {
        let height = (recent.len() as u16 + 2).min(area.height - 2);
        let popup = centered(area, 70, height);
        frame.render_widget(Clear, popup);
        let items: Vec<ListItem> = recent
            .iter()
            .map(|p| {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let dir = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
                ListItem::new(Line::from(vec![
                    Span::styled(format!("◆ {name}"), Style::new().fg(theme.text).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("  {dir}"), theme.faint()),
                ]))
            })
            .collect();
        let mut state = ratatui::widgets::ListState::default().with_selected(Some(at));
        let list = List::new(items)
            .block(panel(theme, " Recent vaults ", true))
            .highlight_style(theme.selected(true))
            .highlight_symbol("▌");
        frame.render_stateful_widget(list, popup, &mut state);
        hit(popup, Hit::Cover);
        for n in 0..recent.len() {
            let y = popup.y + 1 + (n - state.offset().min(n)) as u16;
            if n >= state.offset() && y < popup.bottom() - 1 {
                hit(Rect { x: popup.x + 1, y, width: popup.width - 2, height: 1 }, Hit::Recent(n));
            }
        }
    } else if let Some(at) = cursor {
        frame.set_cursor_position(at);
    }
}

/// What the vault screen shows besides the vault: timers, a message, the settings.
struct Around<'a> {
    lock_in: Option<std::time::Duration>,
    clip_left: Option<u64>,
    toast: Option<&'a crate::app::Toast>,
    prefs: &'a crate::store::Prefs,
}

fn draw_vault(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme, around: &Around<'_>) {
    let Around { lock_in, clip_left, toast, prefs } = *around;
    let [top, body, bottom] = Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(area);
    draw_top(frame, top, ui, theme, lock_in, clip_left);
    let cursor = match ui.tab {
        Tab::Items => draw_items(frame, body, ui, theme),
        Tab::Codes => draw_codes(frame, body, ui, theme),
        Tab::Generator => {
            draw_generator(frame, body, ui, theme);
            None
        }
        Tab::Health => {
            draw_health(frame, body, ui, theme);
            None
        }
        Tab::Settings => {
            draw_settings(frame, body, ui, theme, prefs);
            None
        }
    };
    draw_bottom(frame, bottom, ui, theme, toast);
    if let Some(at) = cursor {
        frame.set_cursor_position(at);
    }
}

fn draw_top(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme, lock_in: Option<std::time::Duration>, clip_left: Option<u64>) {
    frame.render_widget(Block::new().style(Style::new().bg(theme.surface)), area);
    let mut x = area.x + 1;
    let brand = Line::from(vec![
        Span::styled(theme.g.get(Glyph::Brand), Style::new().fg(theme.primary)),
        Span::styled(" WM", Style::new().fg(theme.gradient(0.2)).add_modifier(Modifier::BOLD)),
        Span::styled(" Pass", Style::new().fg(theme.gradient(0.9)).add_modifier(Modifier::BOLD)),
    ]);
    let width = brand.width() as u16;
    frame.render_widget(Paragraph::new(brand), Rect { x, width, ..area });
    x += width + 3;
    for (n, tab) in Tab::ALL.iter().enumerate() {
        let icon = theme.g.get(Glyph::Tab(n));
        let label = if icon.is_empty() { format!(" {} {} ", n + 1, tab.name()) } else { format!(" {} {icon} {} ", n + 1, tab.name()) };
        let w = label.chars().count() as u16;
        if x + w > area.right() {
            break;
        }
        let rect = Rect { x, width: w, ..area };
        let style = if *tab == ui.tab { theme.chip() } else { theme.dim() };
        frame.render_widget(Paragraph::new(Span::styled(label, style)), rect);
        ui.areas.tabs[n] = rect;
        hit(rect, Hit::Key(KeyEvent::new(KeyCode::Char(char::from(b'1' + n as u8)), KeyModifiers::NONE)));
        x += w + 1;
    }
    let mut right = vec![Span::styled(ui.engine.name.clone(), Style::new().fg(theme.text).add_modifier(Modifier::BOLD))];
    if ui.engine.read_only {
        right.push(Span::styled(" read-only", Style::new().fg(theme.warn)));
    }
    if ui.shelf_open {
        right.push(Span::styled(format!("  {} shelf open", theme.g.get(Glyph::Hidden)), Style::new().fg(theme.warn)));
    }
    if let Some(left) = clip_left {
        right.push(Span::styled(format!("  {} {left}s", theme.g.get(Glyph::Clipboard)), Style::new().fg(theme.accent)));
    }
    if let Some(left) = lock_in {
        let secs = left.as_secs();
        let colour = if secs < 60 { theme.warn } else { theme.low };
        right.push(Span::styled(format!("  {} {}:{:02}", theme.g.get(Glyph::Timer), secs / 60, secs % 60), Style::new().fg(colour)));
    }
    right.push(Span::raw(" "));
    let line = Line::from(right).right_aligned();
    let width = (line.width() as u16).min(area.width.saturating_sub(x - area.x));
    frame.render_widget(Paragraph::new(line), Rect { x: area.right() - width, width, ..area });
}

fn draw_bottom(frame: &mut Frame, area: Rect, ui: &VaultUi, theme: &Theme, toast: Option<&crate::app::Toast>) {
    frame.render_widget(Block::new().style(Style::new().bg(theme.surface)), area);
    let hints: Vec<(&str, &str)> = match ui.tab {
        Tab::Items if ui.searching => vec![("Enter", "done"), ("Esc", "clear")],
        Tab::Items if ui.in_trash() => vec![("u", "restore"), ("D", "delete for good"), ("?", "keys")],
        Tab::Items => match ui.pane {
            Pane::Detail => vec![("r", "show"), ("y", "copy line"), ("e", "edit"), (":", "commands"), ("?", "keys")],
            _ if !ui.marked.is_empty() => vec![("Space", "mark"), ("Esc", "unmark"), ("d", "trash"), ("m", "folder"), (":", "more"), ("?", "keys")],
            _ => vec![("/", "search"), ("c", "copy"), ("u", "user"), ("Space", "mark"), ("n", "new"), (":", "commands"), ("?", "keys")],
        },
        Tab::Codes => vec![("Enter", "copy"), ("/", "filter"), ("o", "open item"), ("?", "keys")],
        Tab::Generator => vec![("←→", "change"), ("Enter", "again"), ("c", "copy"), ("?", "keys")],
        Tab::Health => vec![("←→", "group"), ("Enter", "open"), ("i", "ignore"), ("r", "check again"), ("?", "keys")],
        Tab::Settings => vec![("↑↓", "move"), ("←→", "change"), ("Enter", "open"), ("?", "keys")],
    };
    let left = match toast {
        Some(toast) => toast_line(toast, theme),
        // What the vault has to say stays in view between messages, until it is dealt with.
        None if !ui.notices.is_empty() => Line::from(Span::styled(format!(" {}", ui.notices.join("  ")), Style::new().fg(theme.warn))),
        None => {
            let shown = ui.rows.len();
            let text = if ui.tab == Tab::Items && !ui.marked.is_empty() {
                format!(" {} marked of {shown}", ui.marked.len())
            } else if ui.tab == Tab::Items {
                if shown == ui.total { format!(" {shown} items") } else { format!(" {shown} of {} items", ui.total) }
            } else {
                String::new()
            };
            Line::from(Span::styled(text, theme.faint()))
        }
    };
    // A message comes first: the hints give way to it, whole ones at a time, with a gap
    // between the two.
    let room = area.width.saturating_sub(left.width() as u16 + 3);
    let hints = fitting(theme, &hints, room);
    let hint_w = if hints.is_empty() { 0 } else { key_hint(theme, hints).width() as u16 + 1 };
    frame.render_widget(Paragraph::new(left), Rect { width: area.width.saturating_sub(hint_w), ..area });
    if hint_w > 1 {
        hint_bar(frame, Rect { x: area.right() - hint_w, width: hint_w - 1, ..area }, theme, hints, Alignment::Right);
    }
}

fn draw_items(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme) -> Option<Position> {
    let narrow = area.width < 76;
    let show_sidebar = area.width >= 110 || ui.pane == Pane::Sidebar;
    let mut constraints = Vec::new();
    if show_sidebar {
        constraints.push(Constraint::Length(26));
    }
    if narrow {
        constraints.push(Constraint::Min(0));
    } else {
        constraints.push(Constraint::Percentage(42));
        constraints.push(Constraint::Min(30));
    }
    let parts = Layout::horizontal(constraints).split(area);
    let mut at = 0;
    ui.areas.sidebar = Rect::default();
    ui.areas.list = Rect::default();
    ui.areas.detail = Rect::default();
    if show_sidebar {
        draw_sidebar(frame, parts[0], ui, theme);
        at = 1;
    }
    if narrow {
        if ui.pane == Pane::Detail {
            draw_detail(frame, parts[at], ui, theme);
            None
        } else if !show_sidebar || at < parts.len() {
            draw_list(frame, parts[at], ui, theme)
        } else {
            None
        }
    } else {
        let cursor = draw_list(frame, parts[at], ui, theme);
        draw_detail(frame, parts[at + 1], ui, theme);
        cursor
    }
}

fn draw_sidebar(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme) {
    ui.areas.sidebar = area;
    let focused = ui.pane == Pane::Sidebar;
    let block = panel(theme, Line::from(Span::styled(" Vault ", theme.dim())), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = usize::from(inner.width);
    let lines: Vec<Line> = ui
        .sections
        .iter()
        .enumerate()
        .map(|(n, s)| {
            if s.heading {
                return Line::from(Span::styled(format!(" {}", s.label.to_uppercase()), theme.faint().add_modifier(Modifier::BOLD)));
            }
            let picked = n == ui.section;
            let count = s.count.to_string();
            let label: String = s.label.chars().take(width.saturating_sub(count.len() + 5)).collect();
            let pad = width.saturating_sub(label.chars().count() + count.len() + 5);
            let base = if picked { theme.selected(focused) } else { Style::new() };
            let icon_style = if picked { base.fg(theme.primary) } else { Style::new().fg(theme.low) };
            Line::from(vec![
                Span::styled(if picked { "▌" } else { " " }, base.fg(theme.primary)),
                Span::styled(format!("{} ", theme.g.get(s.icon)), icon_style),
                Span::styled(format!("{label}{}", " ".repeat(pad)), if picked { base } else { Style::new().fg(theme.mid) }),
                Span::styled(format!(" {count} "), if picked { base.fg(theme.accent) } else { theme.faint() }),
            ])
        })
        .collect();
    let scroll = ui.section.saturating_sub(usize::from(inner.height.saturating_sub(2))) as u16;
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
    for (n, section) in ui.sections.iter().enumerate().skip(usize::from(scroll)) {
        let y = inner.y + (n - usize::from(scroll)) as u16;
        if y >= inner.bottom() {
            break;
        }
        if !section.heading {
            hit(Rect { y, height: 1, ..inner }, Hit::Section(n));
        }
    }
}

fn draw_list(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme) -> Option<Position> {
    let focused = ui.pane == Pane::List || ui.searching;
    let section = ui.sections.get(ui.section).map(|s| s.label.clone()).unwrap_or_default();
    let mut heading = vec![Span::styled(format!(" {section} "), theme.brand())];
    if !ui.marked.is_empty() {
        heading.push(Span::styled(format!("{} marked ", ui.marked.len()), Style::new().fg(theme.primary).add_modifier(Modifier::BOLD)));
    }
    let block = panel(theme, Line::from(heading), focused)
        .title_bottom(Line::from(Span::styled(format!(" {} ", ui.sort.name()), theme.faint())).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [search, list_area] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
    ui.areas.list = Rect { y: list_area.y - 1, height: list_area.height + 1, ..list_area };

    // The search box, part of the list.
    let mut cursor = None;
    let prompt = Span::styled(format!(" {} ", theme.g.get(Glyph::Search)), if ui.searching { theme.brand() } else { theme.faint() });
    let prompt_w = prompt.width() as u16;
    let (shown, at) = visible(&ui.search, false, usize::from(search.width.saturating_sub(prompt_w)));
    let typed = if ui.search.is_empty() && !ui.searching {
        Span::styled("Search   tag:work  has:otp  weak  kind:card", theme.faint())
    } else {
        Span::styled(shown, Style::new().fg(theme.text))
    };
    frame.render_widget(Paragraph::new(Line::from(vec![prompt, typed])), Rect { height: 1, ..search });
    let rule = "─".repeat(usize::from(search.width));
    frame.render_widget(Paragraph::new(Span::styled(rule, Style::new().fg(theme.line))), Rect { y: search.y + 1, height: 1, ..search });
    if ui.searching {
        cursor = Some(Position::new(search.x + prompt_w + at as u16, search.y));
    }

    if ui.rows.is_empty() {
        let text = if ui.total == 0 {
            vec![
                Line::default(),
                Line::from(Span::styled("Nothing here yet", theme.dim())).centered(),
                Line::default(),
            ]
        } else {
            // "No matches" only when something was searched for; an empty trash or a section
            // with nothing in it is not a search that failed.
            let empty = if !ui.search.is_empty() {
                "No matches"
            } else if ui.in_trash() {
                "The trash is empty"
            } else {
                "Nothing in this section"
            };
            vec![Line::default(), Line::from(Span::styled(empty, theme.dim())).centered()]
        };
        frame.render_widget(Paragraph::new(text), list_area);
        if ui.total == 0 && list_area.height > 4 {
            hint_bar(frame, Rect { y: list_area.y + 3, height: 1, ..list_area }, theme, &[("n", "new item"), (":", "import")], Alignment::Center);
        }
        return cursor;
    }
    let width = usize::from(list_area.width.saturating_sub(2));
    // Only the rows in sight are laid out: the list is drawn several times a second, and
    // for every mouse move, and a vault of thousands would lay out thousands each time.
    let in_sight = (usize::from(list_area.height) / 2).max(1);
    let picked = ui.list.selected().map(|n| n.min(ui.rows.len() - 1));
    let mut offset = ui.list.offset().min(ui.rows.len().saturating_sub(in_sight));
    if let Some(picked) = picked {
        if picked < offset {
            offset = picked;
        } else if picked >= offset + in_sight {
            offset = picked + 1 - in_sight;
        }
    }
    let end = (offset + in_sight + 1).min(ui.rows.len());
    let items: Vec<ListItem> = ui.rows[offset..end]
        .iter()
        .map(|row| {
            let colour = theme.label(row.color.as_deref(), &row.kind);
            let mut badges = String::new();
            if row.pinned {
                badges.push(' ');
                badges.push_str(theme.g.get(Glyph::Pinned));
            }
            if row.favourite {
                badges.push(' ');
                badges.push_str(theme.g.get(Glyph::Favourite));
            }
            if row.has_otp {
                badges.push(' ');
                badges.push_str(theme.g.get(Glyph::Codes));
            }
            if row.has_passkey {
                badges.push(' ');
                badges.push_str(theme.g.get(Glyph::Passkey));
            }
            if row.attachments > 0 {
                badges.push(' ');
                badges.push_str(theme.g.get(Glyph::Attachment));
            }
            let badge_w = badges.chars().count();
            let title_w = width.saturating_sub(badge_w + 3);
            let mut title: String = row.title.chars().take(title_w).collect();
            if row.title.chars().count() > title_w && title_w > 1 {
                title.pop();
                title.push('…');
            }
            let pad = width.saturating_sub(title.chars().count() + badge_w + 2);
            // A marked row wears a tick where its kind's icon was.
            let lead = if ui.is_marked(&row.id) {
                Span::styled("✓ ", Style::new().fg(theme.primary).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(format!("{} ", theme.g.kind(&row.kind)), Style::new().fg(colour))
            };
            let first = Line::from(vec![
                lead,
                Span::styled(title, Style::new().fg(theme.text).add_modifier(Modifier::BOLD)),
                Span::raw(" ".repeat(pad)),
                Span::styled(badges, Style::new().fg(theme.warn)),
            ]);
            let mut sub = row.subtitle.clone();
            if let Some(site) = &row.site
                && !sub.contains(site.as_str())
            {
                if !sub.is_empty() {
                    sub.push_str(" · ");
                }
                sub.push_str(site);
            }
            if !row.tags.is_empty() {
                let tags: Vec<String> = row.tags.iter().map(|t| format!("#{t}")).collect();
                sub.push_str("  ");
                sub.push_str(&tags.join(" "));
            }
            let sub: String = sub.chars().take(width.saturating_sub(2)).collect();
            let second = Line::from(Span::styled(format!("  {sub}"), theme.faint()));
            ListItem::new(Text::from(vec![first, second]))
        })
        .collect();
    let list = List::new(items)
        .highlight_style(theme.selected(focused))
        .highlight_symbol(Span::styled("▌", Style::new().fg(theme.primary)))
        .repeat_highlight_symbol(true);
    let mut window = ratatui::widgets::ListState::default().with_selected(picked.map(|n| n - offset));
    frame.render_stateful_widget(list, list_area, &mut window);
    *ui.list.offset_mut() = offset;
    let first = offset;
    for n in first..ui.rows.len() {
        let y = list_area.y + ((n - first) * 2) as u16;
        if y >= list_area.bottom() {
            break;
        }
        hit(Rect { y, height: 2.min(list_area.bottom() - y), ..list_area }, Hit::Row(n));
    }
    if ui.rows.len() * 2 > usize::from(list_area.height) {
        let mut state = ScrollbarState::new(ui.rows.len()).position(ui.list.selected().unwrap_or(0));
        let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some(" "))
            .thumb_style(Style::new().fg(theme.line));
        frame.render_stateful_widget(bar, Rect { x: area.right() - 1, ..list_area }, &mut state);
    }
    cursor
}

fn chip<'a>(text: String, fg: Color, theme: &Theme) -> Span<'a> {
    if theme.colourless() {
        Span::styled(format!("[{text}]"), Style::new())
    } else {
        Span::styled(format!(" {text} "), Style::new().fg(fg).bg(theme.raised))
    }
}

fn human_size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.0} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

fn spaced_code(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    match chars.len() {
        6 => format!("{} {}", chars[..3].iter().collect::<String>(), chars[3..].iter().collect::<String>()),
        8 => format!("{} {}", chars[..4].iter().collect::<String>(), chars[4..].iter().collect::<String>()),
        _ => code.to_owned(),
    }
}

fn code_lines<'a>(code: &Code, theme: &Theme, width: u16) -> Vec<Line<'a>> {
    let period = code.otp.period.unwrap_or(30).max(1);
    let left = code.seconds_left.unwrap_or(0);
    let colour = if left <= 5 { theme.danger } else if left <= 10 { theme.warn } else { theme.primary };
    let mut first = vec![match &code.code {
        Some(value) => Span::styled(spaced_code(value.as_str()), Style::new().fg(colour).add_modifier(Modifier::BOLD)),
        None => Span::styled("press Enter for the next code", theme.faint()),
    }];
    if code.code.is_some() {
        first.push(Span::styled(format!("   {left}s"), theme.faint()));
    }
    if let Some(next) = &code.next_code {
        first.push(Span::styled(format!("   next {}", spaced_code(next.as_str())), theme.faint()));
    }
    let mut out = vec![Line::from(first)];
    if code.code.is_some() {
        out.push(Line::from(bar(width, f64::from(left) / f64::from(period), colour, theme)));
    }
    out
}

fn draw_detail(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme) {
    ui.areas.detail = area;
    let focused = ui.pane == Pane::Detail;
    let Some(detail) = &ui.detail else {
        let block = panel(theme, "", focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let text = vec![
            Line::default(),
            Line::from(Span::styled("Pick an item to see it here", theme.faint())).centered(),
        ];
        frame.render_widget(Paragraph::new(text), inner);
        return;
    };
    let row = &detail.row;
    let colour = theme.label(row.color.as_deref(), &row.kind);
    let block = panel(theme, Line::from(Span::styled(format!(" {} ", vault::kind_name(&row.kind)), Style::new().fg(colour))), focused);
    let inner = block.inner(area).inner(Margin::new(1, 0));
    frame.render_widget(block, area);

    // Heading: icon, title, flags; then site and tags.
    let mut head = vec![
        Span::styled(format!("{} ", theme.g.kind(&row.kind)), Style::new().fg(colour).add_modifier(Modifier::BOLD)),
        Span::styled(row.title.clone(), Style::new().fg(theme.text).add_modifier(Modifier::BOLD)),
    ];
    if row.favourite {
        head.push(Span::styled(format!("  {}", theme.g.get(Glyph::Favourite)), Style::new().fg(theme.warn)));
    }
    if row.pinned {
        head.push(Span::styled(format!("  {} pinned", theme.g.get(Glyph::Pinned)), theme.faint()));
    }
    if row.trashed {
        head.push(Span::styled("  in the trash", Style::new().fg(theme.danger)));
    }
    let mut lines: Vec<Line> = vec![Line::from(head)];
    let mut chips = Vec::new();
    if let Some(folder) = row.folder.as_ref().and_then(|id| ui.folders.iter().find(|f| f.id == *id)) {
        chips.push(chip(format!("▸ {}", folder.name), theme.mid, theme));
        chips.push(Span::raw(" "));
    }
    for tag in &row.tags {
        chips.push(chip(format!("#{tag}"), theme.accent, theme));
        chips.push(Span::raw(" "));
    }
    if !chips.is_empty() {
        lines.push(Line::from(chips));
    }
    lines.push(Line::from(Span::styled("─".repeat(usize::from(inner.width)), Style::new().fg(theme.line))));
    let head_rows = lines.len();

    let label_w = 16usize;
    let mut starts = Vec::new();
    for (n, line) in ui.lines.iter().enumerate() {
        starts.push(lines.len());
        let picked = focused && n == ui.line;
        let base = if picked { theme.selected(true) } else { Style::new() };
        let marker = Span::styled(if picked { "› " } else { "  " }, base.fg(theme.primary));
        let pad_label = |label: &str| -> String {
            let short: String = label.chars().take(label_w - 1).collect();
            format!("{short:<label_w$}")
        };
        match line {
            DLine::Heading(title) => {
                lines.push(Line::default());
                lines.push(Line::from(Span::styled(title.to_uppercase(), theme.faint().add_modifier(Modifier::BOLD))));
            }
            DLine::Value { label, text, secret, has_value } => {
                let revealed = secret.as_ref().and_then(|s| ui.revealed_text(s));
                let label_span = Span::styled(pad_label(label), base.fg(theme.mid));
                let value_style = base.fg(theme.text);
                let (value, hint) = match (secret, revealed, text) {
                    (Some(_), Some(shown), _) => (Some(shown.to_owned()), Some("r hide")),
                    (Some(_), None, _) if *has_value => (None, Some("r show · y copy")),
                    (Some(_), None, _) => (Some(String::new()), None),
                    (None, _, Some(text)) => (Some(text.clone()), None),
                    (None, _, None) => (Some(String::new()), None),
                };
                match value {
                    None => {
                        let mut spans = vec![marker, label_span, Span::styled(MASK, base.fg(theme.low))];
                        if picked && let Some(hint) = hint {
                            spans.push(Span::styled(format!("   {hint}"), base.fg(theme.low)));
                        }
                        lines.push(Line::from(spans));
                    }
                    Some(value) => {
                        let revealed_style = if revealed.is_some() { base.fg(theme.accent).add_modifier(Modifier::BOLD) } else { value_style };
                        let mut first = true;
                        let mut rows: Vec<&str> = value.lines().collect();
                        if rows.is_empty() {
                            rows.push("");
                        }
                        for part in rows {
                            let (m, l) = if first { (marker.clone(), label_span.clone()) } else { (Span::styled("  ", base), Span::styled(" ".repeat(label_w), base)) };
                            let mut spans = vec![m, l, Span::styled(part.to_owned(), revealed_style)];
                            if first && picked && let Some(hint) = hint {
                                spans.push(Span::styled(format!("   {hint}"), base.fg(theme.low)));
                            }
                            lines.push(Line::from(spans));
                            first = false;
                        }
                    }
                }
            }
            DLine::Code => {
                let label_span = Span::styled(pad_label("One-time code"), base.fg(theme.mid));
                match &ui.code {
                    Some(code) => {
                        let width = inner.width.saturating_sub(label_w as u16 + 4).min(30);
                        for (k, mut l) in code_lines(code, theme, width).into_iter().enumerate() {
                            let prefix = if k == 0 { vec![marker.clone(), label_span.clone()] } else { vec![Span::raw("  "), Span::raw(" ".repeat(label_w))] };
                            let mut spans = prefix;
                            spans.append(&mut l.spans);
                            lines.push(Line::from(spans).style(if k == 0 { base } else { Style::new() }));
                        }
                    }
                    None => lines.push(Line::from(vec![marker, label_span, Span::styled("…", theme.faint())])),
                }
            }
            DLine::Link(id) => {
                let (title, kind) = ui.titles.get(id).cloned().unwrap_or_default();
                lines.push(Line::from(vec![
                    marker,
                    Span::styled(format!("{} ", theme.g.kind(&kind)), base.fg(theme.label(None, &kind))),
                    Span::styled(title, base.fg(theme.text)),
                    Span::styled(if picked { "   Enter open" } else { "" }, base.fg(theme.low)),
                ]));
            }
            DLine::File(file) => lines.push(Line::from(vec![
                marker,
                Span::styled(format!("{} ", theme.g.get(Glyph::Attachment)), base.fg(theme.accent)),
                Span::styled(file.name.clone(), base.fg(theme.text)),
                Span::styled(format!("  {}", human_size(file.size)), base.fg(theme.low)),
                Span::styled(if picked { "   o save" } else { "" }, base.fg(theme.low)),
            ])),
            DLine::Past { index, field, at_ms } => {
                let shown = ui.revealed_text(&Secret::History(*index));
                // The date after the value: in the label column it was cut to "2026-1".
                let mut name = field.chars();
                let name: String = name.next().map(|c| c.to_uppercase().chain(name).collect()).unwrap_or_default();
                let value = shown.map_or(MASK.chars().count(), |v| v.chars().count());
                // Whole or not at all: a narrow pane would cut it to "2026-11-".
                let date = format!("  {}", date_text(*at_ms));
                let room = usize::from(inner.width) >= 2 + label_w + value + date.chars().count();
                lines.push(Line::from(vec![
                    marker,
                    Span::styled(pad_label(&name), base.fg(theme.mid)),
                    match shown {
                        Some(v) => Span::styled(v.to_owned(), base.fg(theme.accent)),
                        None => Span::styled(MASK, base.fg(theme.low)),
                    },
                    Span::styled(if room { date } else { String::new() }, base.fg(theme.low)),
                ]));
            }
        }
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        format!("Made {} · changed {}", date_text(detail.created_ms), date_text(row.modified_ms)),
        theme.faint(),
    )));

    // Keep the picked line in sight.
    let height = usize::from(inner.height);
    let at = starts.get(ui.line).copied().unwrap_or(0);
    let scroll = (at + 2).saturating_sub(height);
    let scroll = if scroll > 0 { scroll.max(head_rows.min(scroll)) } else { 0 };
    let total = lines.len();
    frame.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), inner);
    for (n, start) in starts.iter().enumerate() {
        if !ui.lines[n].selectable() || *start < scroll {
            continue;
        }
        let end = starts.get(n + 1).copied().unwrap_or(total.saturating_sub(2)).max(start + 1);
        let y = inner.y + (start - scroll) as u16;
        if y >= inner.bottom() {
            break;
        }
        let height = ((end - start) as u16).min(inner.bottom() - y);
        hit(Rect { y, height, ..inner }, Hit::Line(n));
    }
}

fn draw_codes(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme) -> Option<Position> {
    let block = panel(theme, Line::from(Span::styled(" One-time codes ", theme.brand())), true);
    let inner = block.inner(area).inner(Margin::new(1, 0));
    frame.render_widget(block, area);
    let [filter, grid] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
    let prompt = Span::styled(format!("{} ", theme.g.get(Glyph::Search)), if ui.filtering_codes { theme.brand() } else { theme.faint() });
    let prompt_w = prompt.width() as u16;
    let (shown, at) = visible(&ui.code_filter, false, usize::from(filter.width.saturating_sub(prompt_w)));
    let typed = if ui.code_filter.is_empty() && !ui.filtering_codes {
        Span::styled("Filter by name", theme.faint())
    } else {
        Span::styled(shown, Style::new().fg(theme.text))
    };
    frame.render_widget(Paragraph::new(Line::from(vec![prompt, typed])), Rect { height: 1, ..filter });
    let filter_cursor = ui.filtering_codes.then(|| Position::new(filter.x + prompt_w + at as u16, filter.y));
    ui.areas.codes = grid;
    if ui.codes.is_empty() {
        // With a filter typed, the codes are there but none is called that.
        let (head, tip) = if ui.code_filter.is_empty() {
            ("No one-time codes yet", "Edit a login and paste its otpauth:// link or seed")
        } else {
            ("No codes match", "Esc clears the filter")
        };
        let text = vec![
            Line::default(),
            Line::from(Span::styled(head, theme.dim())).centered(),
            Line::from(Span::styled(tip, theme.faint())).centered(),
        ];
        frame.render_widget(Paragraph::new(text), grid);
        return filter_cursor;
    }
    let card_w = 36u16;
    let cols = (grid.width / card_w).max(1);
    ui.areas.code_cols = cols;
    let card_w = grid.width / cols;
    let card_h = 5u16;
    let rows_fit = (grid.height / card_h).max(1);
    let row_of = (ui.code_at / usize::from(cols)) as u16;
    let first_row = row_of.saturating_sub(rows_fit - 1);
    for (n, code) in ui.codes.iter().enumerate() {
        let (r, c) = ((n / usize::from(cols)) as u16, (n % usize::from(cols)) as u16);
        if r < first_row || r >= first_row + rows_fit {
            continue;
        }
        let rect = Rect { x: grid.x + c * card_w, y: grid.y + (r - first_row) * card_h, width: card_w.saturating_sub(1), height: card_h };
        let picked = n == ui.code_at;
        let title = Line::from(vec![
            Span::styled(format!(" {} ", theme.g.get(Glyph::Codes)), Style::new().fg(theme.accent)),
            Span::styled(format!("{} ", code.title), Style::new().fg(theme.text).add_modifier(Modifier::BOLD)),
        ]);
        let mut block = Block::bordered()
            .border_type(if picked { BorderType::Thick } else { BorderType::Rounded })
            .border_style(theme.border(picked))
            .title(title);
        if picked {
            block = block.title_bottom(Line::from(Span::styled(" Enter copies ", theme.faint())).right_aligned());
        }
        let body = block.inner(rect).inner(Margin::new(1, 0));
        frame.render_widget(block, rect);
        hit(rect, Hit::Code(n));
        let mut lines = code_lines(code, theme, body.width);
        let who = [code.otp.account.as_str(), code.site.as_deref().unwrap_or("")]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" · ");
        lines.push(Line::from(Span::styled(who, theme.faint())));
        frame.render_widget(Paragraph::new(lines), body);
    }
    filter_cursor
}

fn coloured_value<'a>(value: &str, theme: &Theme) -> Line<'a> {
    let spans: Vec<Span> = value
        .chars()
        .map(|ch| {
            let colour = if theme.colourless() {
                Color::Reset
            } else if ch.is_ascii_digit() {
                theme.info
            } else if ch.is_alphabetic() {
                theme.text
            } else if ch == ' ' || ch == '-' {
                theme.low
            } else {
                theme.warn
            };
            Span::styled(ch.to_string(), Style::new().fg(colour).add_modifier(Modifier::BOLD))
        })
        .collect();
    Line::from(spans)
}

fn draw_generator(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme) {
    let [left, right] = Layout::horizontal([Constraint::Length(34), Constraint::Min(30)]).areas(area);
    let generator = &ui.generator;
    let block = panel(theme, Line::from(Span::styled(" Settings ", theme.brand())), true);
    let inner = block.inner(left).inner(Margin::new(1, 1));
    frame.render_widget(block, left);
    let knobs = generator.knobs();
    let lines: Vec<Line> = knobs
        .iter()
        .enumerate()
        .map(|(n, knob)| {
            let (label, value) = generator.describe(*knob);
            let picked = n == generator.at;
            let base = if picked { theme.selected(true) } else { Style::new() };
            let toggle = matches!(knob, Knob::Lower | Knob::Upper | Knob::Digits | Knob::Symbols | Knob::Ambiguous | Knob::Capitalise | Knob::WordDigit);
            let shown = if toggle {
                if value == "on" { Span::styled("● on ", base.fg(theme.primary)) } else { Span::styled("○ off", base.fg(theme.low)) }
            } else {
                Span::styled(format!("‹ {value} ›"), base.fg(if picked { theme.accent } else { theme.text }))
            };
            let width = usize::from(inner.width).saturating_sub(label.chars().count() + shown.width() + 2);
            Line::from(vec![
                Span::styled(if picked { "▌" } else { " " }, base.fg(theme.primary)),
                Span::styled(format!("{label}{}", " ".repeat(width)), base.fg(theme.mid)),
                shown,
                Span::styled(" ", base),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
    for n in 0..knobs.len() {
        let y = inner.y + n as u16;
        if y < inner.bottom() {
            hit(Rect { y, height: 1, ..inner }, Hit::Knob(n));
        }
    }

    let block = panel(theme, Line::from(Span::styled(format!(" {} ", generator.mode.name()), theme.brand())), false);
    let inner = block.inner(right).inner(Margin::new(2, 1));
    frame.render_widget(block, right);
    let mut lines = vec![Line::default()];
    let value = generator.value.as_str();
    let width = usize::from(inner.width.max(1));
    let chars: Vec<char> = value.chars().collect();
    for chunk in chars.chunks(width) {
        let text: String = chunk.iter().collect();
        lines.push(coloured_value(&text, theme).centered());
    }
    lines.push(Line::default());
    let bits = generator.bits;
    let bar_w = inner.width.saturating_sub(18).min(40);
    let mut entropy = vec![Span::styled(format!("{bits:>5.0} bits  "), theme.dim())];
    let bits_colour = if bits < 50.0 { theme.danger } else if bits < 80.0 { theme.warn } else { theme.primary };
    entropy.extend(bar(bar_w, bits / 128.0, bits_colour, theme));
    lines.push(Line::from(entropy).centered());
    if let Some(strength) = &generator.strength {
        let mut spans = meter(strength.score, theme);
        spans.push(Span::styled(format!("  {}", score_word(strength.score)), Style::new().fg(theme.score(strength.score))));
        lines.push(Line::from(spans).centered());
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled("Offline, slow hash  ", theme.faint()),
            Span::styled(strength.crack_slow_hash.clone(), Style::new().fg(theme.text)),
        ]).centered());
        lines.push(Line::from(vec![
            Span::styled("Offline, fast hash  ", theme.faint()),
            Span::styled(strength.crack_fast_hash.clone(), Style::new().fg(theme.text)),
        ]).centered());
        if let Some(warning) = &strength.warning {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(warning.clone(), Style::new().fg(theme.warn))).centered());
        }
    }
    let used = lines.len() as u16;
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    if used + 1 < inner.height {
        hint_bar(frame, Rect { y: inner.y + used + 1, height: 1, ..inner }, theme, &[("Enter", "make another"), ("c", "copy"), ("Shift ←→", "faster")], Alignment::Center);
    }
}

fn draw_health(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme) {
    ui.areas.health = area;
    let Some(health) = &ui.health else {
        frame.render_widget(Paragraph::new("Checking…").style(theme.dim()), area);
        return;
    };
    let [top, rest] = Layout::vertical([Constraint::Length(5), Constraint::Min(0)]).areas(area);
    let block = panel(theme, Line::from(Span::styled(" Health ", theme.brand())), false);
    let inner = block.inner(top).inner(Margin::new(2, 0));
    frame.render_widget(block, top);
    let score = health.score;
    let colour = match score {
        0..50 => theme.danger,
        50..80 => theme.warn,
        _ => theme.primary,
    };
    let verdict = match score {
        0..50 => "Your vault needs work",
        50..80 => "Your vault could be better",
        90.. => "Your vault is in great shape",
        _ => "Your vault is in good shape",
    };
    let bar_w = inner.width.saturating_sub(4);
    let lines = vec![
        Line::from(vec![
            Span::styled(format!("{score}"), Style::new().fg(colour).add_modifier(Modifier::BOLD)),
            Span::styled(" / 100  ", theme.faint()),
            Span::styled(verdict, Style::new().fg(theme.text)),
        ]),
        Line::from(bar(bar_w, f64::from(score) / 100.0, colour, theme)),
        Line::from(Span::styled("Checked on this device. Nothing is sent anywhere.", theme.faint())),
    ];
    frame.render_widget(Paragraph::new(lines), inner);

    let [groups, entries] = Layout::horizontal([Constraint::Length(34), Constraint::Min(20)]).areas(rest);
    let block = panel(theme, Line::from(Span::styled(" Findings ", theme.dim())), true);
    let inner = block.inner(groups).inner(Margin::new(1, 0));
    frame.render_widget(block, groups);
    let shown = ui.health_groups();
    let lines: Vec<Line> = HEALTH_GROUPS
        .iter()
        .take(shown)
        .enumerate()
        .map(|(n, (label, _, _))| {
            let count = ui.health_ids(n).len();
            let picked = n == ui.health_group;
            let base = if picked { theme.selected(true) } else { Style::new() };
            let ignored = n == vault::IGNORED_GROUP;
            let badge_colour = if ignored { theme.low } else if count == 0 { theme.ok } else if n <= 1 { theme.danger } else { theme.warn };
            let badge = if count == 0 && !ignored { "✓".to_owned() } else { count.to_string() };
            let pad = usize::from(inner.width).saturating_sub(label.chars().count() + badge.chars().count() + 3);
            Line::from(vec![
                Span::styled(if picked { "▌" } else { " " }, base.fg(theme.primary)),
                Span::styled(format!("{label}{}", " ".repeat(pad)), base.fg(if ignored { theme.low } else { theme.text })),
                Span::styled(format!(" {badge} "), base.fg(badge_colour).add_modifier(Modifier::BOLD)),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
    for n in 0..shown {
        let y = inner.y + n as u16;
        if y < inner.bottom() {
            hit(Rect { y, height: 1, ..inner }, Hit::HealthGroup(n));
        }
    }

    let rows = ui.health_lines(ui.health_group);
    let title = HEALTH_GROUPS.get(ui.health_group).map_or("", |g| g.0);
    let block = panel(theme, Line::from(Span::styled(format!(" {title} "), theme.dim())), false);
    let inner = block.inner(entries).inner(Margin::new(1, 0));
    frame.render_widget(block, entries);
    if rows.is_empty() {
        frame.render_widget(Paragraph::new(Line::from(Span::styled("Nothing to fix here", Style::new().fg(theme.ok)))), inner);
        return;
    }
    // Each line with the item number it stands for, so the cursor and clicks skip headings.
    let mut numbered = Vec::with_capacity(rows.len());
    let mut item = 0;
    for row in &rows {
        match row {
            vault::HealthLine::Heading(_) => numbered.push(None),
            vault::HealthLine::Item { .. } => {
                numbered.push(Some(item));
                item += 1;
            }
        }
    }
    let picked_line = numbered.iter().position(|n| *n == Some(ui.health_at)).unwrap_or(0);
    let skip = picked_line.saturating_sub(usize::from(inner.height.saturating_sub(1)));
    let hint = if ui.health_group == vault::IGNORED_GROUP { "   i counts it again" } else { "   Enter opens, i ignores" };
    let lines: Vec<Line> = rows
        .iter()
        .zip(&numbered)
        .skip(skip)
        .map(|(row, n)| match row {
            vault::HealthLine::Heading(text) => Line::from(Span::styled(format!(" {text}"), theme.dim())),
            vault::HealthLine::Item { id, check } => {
                let (title, kind) = ui.titles.get(id).cloned().unwrap_or_else(|| ("A deleted item".into(), String::new()));
                let picked = *n == Some(ui.health_at);
                let base = if picked { theme.selected(true) } else { Style::new() };
                let indent = if ui.health_group == 1 { "  " } else { "" };
                let mut spans = vec![
                    Span::styled(if picked { "▌" } else { " " }, base.fg(theme.primary)),
                    Span::styled(format!("{indent}{} ", theme.g.kind(&kind)), base.fg(theme.label(None, &kind))),
                    Span::styled(title, base.fg(theme.text)),
                ];
                if let Some(check) = check {
                    spans.push(Span::styled(format!("  {}", vault::check_label(check)), base.fg(theme.low)));
                }
                spans.push(Span::styled(if picked { hint } else { "" }, base.fg(theme.low)));
                Line::from(spans)
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
    for (line, n) in numbered.iter().enumerate().skip(skip) {
        let y = inner.y + (line - skip) as u16;
        if y >= inner.bottom() {
            break;
        }
        if let Some(n) = n {
            hit(Rect { y, height: 1, ..inner }, Hit::HealthEntry(*n));
        }
    }
}

/// What a setting is set to now, as the list shows it.
fn setting_value(setting: Setting, ui: &VaultUi, theme: &Theme, prefs: &crate::store::Prefs) -> (String, Option<bool>) {
    let info = ui.info.as_ref();
    match setting {
        Setting::Theme => {
            let mut name = theme.name.to_owned();
            if let Some(first) = name.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            (name, None)
        }
        Setting::Icons => (String::new(), Some(theme.g.nerd)),
        Setting::LockAfter => (settings::minutes(prefs.lock_minutes), None),
        Setting::ClipSeconds => (settings::seconds(prefs.clip_seconds), None),
        Setting::RevealSeconds => (settings::seconds(prefs.reveal_seconds), None),
        Setting::Shelf => (if ui.shelf_open { "open" } else { "closed" }.to_owned(), Some(ui.shelf_open)),
        Setting::VaultName => (ui.engine.name.clone(), None),
        Setting::TrashDays => (
            info.map_or_else(String::new, |i| if i.trash_days == 0 { "never".to_owned() } else { format!("{} days", i.trash_days) }),
            None,
        ),
        Setting::HistoryLimit => (
            info.map_or_else(String::new, |i| if i.history_limit == 0 { "all".to_owned() } else { i.history_limit.to_string() }),
            None,
        ),
        Setting::SyncSettings => (String::new(), Some(prefs.sync_settings)),
        Setting::SkipLocal => (String::new(), Some(prefs.health_skip_local)),
        Setting::IntoApp if ui.engine.app_folder().is_some() => ("already there".to_owned(), None),
        Setting::Password | Setting::Import | Setting::Export | Setting::AddDevice | Setting::IntoApp => (String::new(), None),
    }
}

fn draw_settings(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme, prefs: &crate::store::Prefs) {
    ui.areas.settings = area;
    let list = settings::all();
    let at = ui.settings_at.min(list.len() - 1);
    let wide = area.width >= 96;
    let (left, right) = if wide {
        let [l, r] = Layout::horizontal([Constraint::Length((area.width * 11 / 20).min(66)), Constraint::Min(30)]).areas(area);
        (l, Some(r))
    } else {
        (area, None)
    };
    let block = panel(theme, Line::from(Span::styled(format!(" {} Settings ", theme.g.get(Glyph::Tab(4))).replace("  ", " "), theme.brand())), true);
    let inner = block.inner(left).inner(Margin::new(1, 1));
    frame.render_widget(block, left);
    // Without room for the side panel, the picked setting's words go along the bottom.
    let (list_area, about_area) = if wide || inner.height < 12 {
        (inner, None)
    } else {
        let [l, a] = Layout::vertical([Constraint::Min(0), Constraint::Length(4)]).areas(inner);
        (l, Some(a))
    };

    // Every line: a heading, a gap, or a setting.
    let mut lines: Vec<(Line, Option<usize>)> = Vec::new();
    for (n, setting) in list.iter().enumerate() {
        if let Some(heading) = settings::heading_before(n) {
            if n > 0 {
                lines.push((Line::default(), None));
            }
            lines.push((Line::from(Span::styled(heading.to_uppercase(), theme.brand())), None));
        }
        let picked = n == at;
        let base = if picked { theme.selected(true) } else { Style::new() };
        let (value, flag) = setting_value(*setting, ui, theme, prefs);
        let shown: Vec<Span> = match (setting.look(), flag) {
            (Look::Toggle, Some(true)) => vec![Span::styled("● on  ", base.fg(theme.primary).add_modifier(Modifier::BOLD))],
            (Look::Toggle, _) => vec![Span::styled("○ off  ", base.fg(theme.low))],
            (Look::Steps, _) if picked => vec![
                Span::styled("‹ ", base.fg(theme.low)),
                Span::styled(value, base.fg(theme.accent)),
                Span::styled(" ›", base.fg(theme.low)),
            ],
            (Look::Steps, _) => vec![Span::styled(format!("{value}  "), base.fg(theme.text))],
            (Look::Action, _) if value.is_empty() => vec![Span::styled("›", base.fg(if picked { theme.accent } else { theme.low }))],
            (Look::Action, _) => vec![Span::styled(format!("{value}  ›"), base.fg(if picked { theme.text } else { theme.mid }))],
        };
        let shown_w: usize = shown.iter().map(Span::width).sum();
        let label = setting.label();
        let gap = usize::from(list_area.width).saturating_sub(label.chars().count() + shown_w + 3);
        let mut spans = vec![
            Span::styled(if picked { "▌" } else { " " }, base.fg(theme.primary)),
            Span::styled(format!("{label}{}", " ".repeat(gap)), base.fg(if picked { theme.text } else { theme.mid })),
            Span::styled(" ", base),
        ];
        spans.extend(shown);
        spans.push(Span::styled(" ", base));
        lines.push((Line::from(spans), Some(n)));
    }
    let picked_line = lines.iter().position(|(_, n)| *n == Some(at)).unwrap_or(0);
    let height = usize::from(list_area.height);
    // Keep the group's heading in sight above the picked line when there is room.
    let first = (picked_line + 3).saturating_sub(height).min(lines.len().saturating_sub(height));
    for (row, (line, n)) in lines.iter().skip(first).take(height).enumerate() {
        let rect = Rect { y: list_area.y + row as u16, height: 1, ..list_area };
        frame.render_widget(Paragraph::new(line.clone()), rect);
        if let Some(n) = n {
            hit(rect, Hit::Setting(*n));
        }
    }

    let setting = list[at];
    let mut about = vec![Line::from(Span::styled(setting.about(), Style::new().fg(theme.text)))];
    let app_timer = matches!(setting, Setting::LockAfter | Setting::ClipSeconds | Setting::RevealSeconds) && ui.engine.app_folder().is_some();
    if app_timer {
        about.push(Line::from(Span::styled("This vault is the WM Pass app's, so the app's own timer is taken each time it opens.", Style::new().fg(theme.warn))));
    }
    let keys: Vec<(&str, &str)> = match setting.look() {
        Look::Toggle => vec![("Enter", "turn on or off")],
        Look::Steps => vec![("←→", "change"), ("Enter", "type a value")],
        Look::Action => vec![("Enter", "open")],
    };
    match (right, about_area) {
        (Some(right), _) => {
            let block = panel(theme, Line::from(Span::styled(format!(" {} ", setting.label()), theme.brand())), false);
            let inner = block.inner(right).inner(Margin::new(2, 1));
            frame.render_widget(block, right);
            let mut text = about;
            text.push(Line::default());
            let used = text.len() as u16;
            frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), inner);
            // The vault itself, at the foot.
            // The file's own name is the part that tells vaults apart, so a long path loses
            // its start, not its end.
            let path = ui.engine.path().display().to_string();
            let room = usize::from(inner.width.saturating_sub(7));
            let count = path.chars().count();
            let path = if count > room && room > 1 { format!("…{}", path.chars().skip(count - (room - 1)).collect::<String>()) } else { path };
            let mut foot = vec![
                Line::from(Span::styled("THIS VAULT", theme.brand())),
                Line::from(vec![Span::styled("Name   ", theme.faint()), Span::styled(ui.engine.name.clone(), Style::new().fg(theme.text))]),
                Line::from(vec![Span::styled("Items  ", theme.faint()), Span::styled(ui.info.as_ref().map_or(ui.total, |i| i.entries).to_string(), Style::new().fg(theme.text))]),
                Line::from(vec![Span::styled("File   ", theme.faint()), Span::styled(path, Style::new().fg(theme.mid))]),
                Line::from(vec![Span::styled("App    ", theme.faint()), Span::styled(format!("WM Pass {}", env!("CARGO_PKG_VERSION")), Style::new().fg(theme.mid))]),
            ];
            if ui.engine.read_only {
                foot.push(Line::from(Span::styled("Read-only: changes to the vault are turned off", Style::new().fg(theme.warn))));
            }
            let foot_h = foot.len() as u16;
            if inner.height > foot_h + 6 {
                let rect = Rect { y: inner.bottom() - foot_h, height: foot_h, ..inner };
                frame.render_widget(Paragraph::new(foot).wrap(Wrap { trim: false }), rect);
                hint_bar(frame, Rect { y: rect.y - 2, height: 1, ..inner }, theme, &keys, Alignment::Left);
            } else if inner.height > used + 3 {
                hint_bar(frame, Rect { y: inner.y + used + 2, height: 1, ..inner }, theme, &keys, Alignment::Left);
            }
        }
        (None, Some(about_area)) => {
            frame.render_widget(Paragraph::new(about).style(theme.faint()).wrap(Wrap { trim: true }), Rect { height: about_area.height.saturating_sub(1), ..about_area });
            hint_bar(frame, Rect { y: about_area.bottom() - 1, height: 1, ..about_area }, theme, &keys, Alignment::Right);
        }
        (None, None) => {}
    }
}

fn popup(frame: &mut Frame, area: Rect, width: u16, height: u16, theme: &Theme, title: &str, danger: bool) -> Rect {
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);
    let colour = if danger { theme.danger } else { theme.primary };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(colour))
        .title(Span::styled(format!(" {title} "), Style::new().fg(colour).add_modifier(Modifier::BOLD)))
        .style(Style::new().bg(theme.raised).fg(theme.text));
    let inner = block.inner(rect).inner(Margin::new(1, 0));
    frame.render_widget(block, rect);
    hit(rect, Hit::Cover);
    inner
}

fn draw_overlay(frame: &mut Frame, area: Rect, overlay: &Overlay, theme: &Theme, ui: Option<&VaultUi>) {
    match overlay {
        Overlay::Help { scroll } => draw_help(frame, area, theme, *scroll),
        Overlay::Palette { input, at } => {
            let found = palette_matches(input.text());
            let height = (found.len() as u16 + 4).min(area.height - 4).max(6);
            let inner = popup(frame, area, 60, height, theme, "Commands", false);
            let (shown, col) = visible(input, false, usize::from(inner.width.saturating_sub(2)));
            let line = Line::from(vec![Span::styled("› ", theme.brand()), Span::styled(shown, Style::new().fg(theme.text))]);
            frame.render_widget(Paragraph::new(line), Rect { height: 1, ..inner });
            frame.set_cursor_position(Position::new(inner.x + 2 + col as u16, inner.y));
            let list_area = Rect { y: inner.y + 2, height: inner.height.saturating_sub(2), ..inner };
            let width = usize::from(list_area.width);
            let skip = at.saturating_sub(usize::from(list_area.height.saturating_sub(1)));
            let lines: Vec<Line> = found
                .iter()
                .enumerate()
                .skip(skip)
                .map(|(n, (_, label, key))| {
                    let picked = n == *at;
                    let base = if picked { theme.selected(true) } else { Style::new() };
                    let pad = width.saturating_sub(label.chars().count() + key.chars().count() + 3);
                    Line::from(vec![
                        Span::styled(if picked { "▌" } else { " " }, base.fg(theme.primary)),
                        Span::styled(format!("{label}{}", " ".repeat(pad)), base.fg(theme.text)),
                        Span::styled(format!(" {key} "), base.fg(theme.accent)),
                    ])
                })
                .collect();
            if found.is_empty() {
                frame.render_widget(Paragraph::new(Line::from(Span::styled(" No command matches that", theme.dim()))), list_area);
            }
            frame.render_widget(Paragraph::new(lines), list_area);
            for n in skip..found.len() {
                let y = list_area.y + (n - skip) as u16;
                if y >= list_area.bottom() {
                    break;
                }
                hit(Rect { y, height: 1, ..list_area }, Hit::PaletteRow(n));
            }
        }
        Overlay::Confirm { title, body, danger, .. } => {
            let inner = popup(frame, area, 60, 9, theme, title, *danger);
            let colour = if *danger { theme.danger } else { theme.primary };
            let lines = vec![
                Line::default(),
                Line::from(Span::styled(body.clone(), Style::new().fg(theme.text))),
            ];
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), Rect { height: inner.height - 2, ..inner });
            let buttons = Line::from(vec![
                Span::styled(" y ", Style::new().bg(colour).fg(theme.on_primary).add_modifier(Modifier::BOLD)),
                Span::styled(" Yes    ", Style::new().fg(theme.text)),
                Span::styled(" n ", Style::new().bg(theme.line).fg(theme.text).add_modifier(Modifier::BOLD)),
                Span::styled(" No", Style::new().fg(theme.text)),
            ])
            .right_aligned();
            let row = Rect { y: inner.bottom() - 1, height: 1, ..inner };
            frame.render_widget(Paragraph::new(buttons), row);
            // " y  Yes    " is 11 cells, " n  No" 6, flush right.
            let yes = Rect { x: row.right().saturating_sub(17), width: 9, ..row };
            let no = Rect { x: row.right().saturating_sub(6), width: 6, ..row };
            hit(yes, Hit::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)));
            hit(no, Hit::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)));
        }
        Overlay::Prompt { title, hint, input, secret, .. } => {
            let inner = popup(frame, area, 70, 8, theme, title, false);
            frame.render_widget(Paragraph::new(Span::styled(hint.clone(), theme.dim())).wrap(Wrap { trim: true }), Rect { height: 2, ..inner });
            let look = BoxLook { label: "", secret: *secret, focused: true, placeholder: "" };
            if let Some(at) = text_box(frame, Rect { y: inner.y + 2, height: 3, ..inner }, theme, input, &look) {
                frame.set_cursor_position(at);
            }
            hint_bar(frame, Rect { y: inner.bottom() - 1, height: 1, ..inner }, theme, &[("Enter", "done"), ("Esc", "cancel")], Alignment::Right);
        }
        Overlay::Pick { title, items, at, then } => {
            let colors = matches!(then, crate::app::PickFor::Color(_));
            let height = (items.len() as u16 + 4).min(area.height - 2);
            let inner = popup(frame, area, 64, height, theme, title, false);
            let skip = at.saturating_sub(usize::from(inner.height.saturating_sub(3)));
            let lines: Vec<Line> = items
                .iter()
                .enumerate()
                .skip(skip)
                .map(|(n, (_, label))| {
                    let picked = n == *at;
                    let base = if picked { theme.selected(true) } else { Style::new() };
                    let width = usize::from(inner.width).saturating_sub(label.chars().count() + 1);
                    let mut line = vec![Span::styled(if picked { "▌" } else { " " }, base.fg(theme.primary))];
                    if colors {
                        // Each color shows itself; the first, the site's own, shows the login's.
                        let id = &items[n].0;
                        let swatch = theme.label((!id.is_empty()).then_some(id.as_str()), "login");
                        line.push(Span::styled("● ", base.fg(swatch)));
                    }
                    let width = width.saturating_sub(if colors { 2 } else { 0 });
                    line.push(Span::styled(format!("{label}{}", " ".repeat(width)), base.fg(theme.text)));
                    Line::from(line)
                })
                .collect();
            let rows = Rect { y: inner.y + 1, height: inner.height.saturating_sub(2), ..inner };
            frame.render_widget(Paragraph::new(lines), rows);
            for n in skip..items.len() {
                let y = rows.y + (n - skip) as u16;
                if y >= rows.bottom() {
                    break;
                }
                hit(Rect { y, height: 1, ..rows }, Hit::PickRow(n));
            }
            hint_bar(frame, Rect { y: inner.bottom() - 1, height: 1, ..inner }, theme, &[("↑↓", "move"), ("Enter", "pick"), ("Esc", "cancel")], Alignment::Right);
        }
        Overlay::Import(preview) => {
            let inner = popup(frame, area, 70, (area.height - 4).min(22), theme, "Import", false);
            let mut lines = vec![
                Line::default(),
                Line::from(vec![
                    Span::styled(format!("{} ", preview.count), theme.brand()),
                    Span::styled(
                        format!("{} found in a {} file", if preview.count == 1 { "item" } else { "items" }, preview.format),
                        Style::new().fg(theme.text),
                    ),
                ]),
                Line::default(),
            ];
            let room = usize::from(inner.height.saturating_sub(9));
            for title in preview.titles.iter().take(room) {
                lines.push(Line::from(vec![Span::styled("  • ", theme.faint()), Span::styled(title.clone(), Style::new().fg(theme.mid))]));
            }
            // Counted from what this window shows, which can be fewer than the titles sent.
            let listed = preview.titles.len().min(room);
            if preview.count > listed {
                lines.push(Line::from(Span::styled(format!("  and {} more", preview.count - listed), theme.faint())));
            }
            for note in preview.skipped.iter().take(3) {
                lines.push(Line::from(Span::styled(format!("! {note}"), Style::new().fg(theme.warn))));
            }
            lines.push(Line::default());
            lines.push(Line::from(Span::styled("Items already in the vault are left as they are.", theme.faint())));
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), Rect { height: inner.height - 1, ..inner });
            hint_bar(frame, Rect { y: inner.bottom() - 1, height: 1, ..inner }, theme, &[("y", "import"), ("n", "cancel")], Alignment::Right);
        }
        Overlay::Editor(editor) => draw_editor(frame, area, editor, theme, ui),
        Overlay::Link(link) => draw_link(frame, area, link, theme),
        Overlay::Restore(ui) => draw_restore(frame, area, ui, theme),
        Overlay::WifiCode { ssid, qr } => draw_wifi_code(frame, area, ssid, qr, theme),
    }
}

/// A saved app's package, with the name people know it by when there is one.
fn app_names(packages: &str) -> String {
    packages
        .split([',', ' '])
        .filter(|p| !p.is_empty())
        .map(|p| match wmpass_core::related::app_name(p) {
            Some(name) => format!("{name} ({p})"),
            None => p.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn draw_editor(frame: &mut Frame, area: Rect, editor: &Editor, theme: &Theme, ui: Option<&VaultUi>) {
    // The title as saved, from the item the editor has open: the one picked in the list can be
    // another.
    let title = match (&editor.id, ui.and_then(|u| u.detail.as_ref())) {
        (Some(id), Some(detail)) if detail.row.id == *id => format!("{}  Edit {}", theme.g.kind(&editor.kind), detail.row.title),
        (Some(_), _) => format!("{}  Edit {}", theme.g.kind(&editor.kind), vault::kind_in_text(&editor.kind)),
        _ => format!("{}  New {}", theme.g.kind(&editor.kind), vault::kind_in_text(&editor.kind)),
    };
    let width = area.width.saturating_sub(8).min(96);
    let height = area.height.saturating_sub(2);
    let inner = popup(frame, area, width, height, theme, &title, false);
    let [body, error, foot] = Layout::vertical([Constraint::Min(0), Constraint::Length(1), Constraint::Length(1)]).areas(inner);

    // Each row is a label and a box; a multiline box gets more height when picked, and a group
    // of added fields or questions gets a heading above it.
    let longest = editor
        .rows
        .iter()
        .filter(|r| !r.is_action())
        .map(|r| if r.is_named() { r.name.text().chars().count() + 2 } else { r.label.chars().count() })
        .max()
        .unwrap_or(10);
    let label_w = (longest as u16 + 3).clamp(16, 28).min(body.width / 3);
    let heights: Vec<u16> = editor
        .rows
        .iter()
        .enumerate()
        .map(|(n, r)| {
            let own = if r.multiline() && n == editor.at { 6 } else { 1 };
            own + 1 + u16::from(editor.heading(n).is_some())
        })
        .collect();
    let at = editor.at.min(heights.len() - 1);
    let mut first = 0;
    while first < at && heights[first..=at].iter().sum::<u16>() > body.height {
        first += 1;
    }
    let mut y = body.y + 1;
    let mut cursor = None;
    let mut last_drawn = first;
    for (n, row) in editor.rows.iter().enumerate().skip(first) {
        let h = heights[n] - 1 - u16::from(editor.heading(n).is_some());
        if let Some(heading) = editor.heading(n) {
            if y + 1 + h > body.bottom() {
                break;
            }
            let rule = "─".repeat(usize::from(body.width.saturating_sub(heading.chars().count() as u16 + 4)));
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(format!(" {heading} "), theme.brand()),
                    Span::styled(rule, theme.faint()),
                ])),
                Rect { x: body.x, y, width: body.width, height: 1 },
            );
            y += 1;
        }
        if y + h > body.bottom() {
            break;
        }
        last_drawn = n;
        let picked = n == editor.at;
        hit(Rect { x: body.x, y, width: body.width, height: h }, Hit::EditorRow(n));
        if row.is_action() {
            let style = if picked { Style::new().fg(theme.primary).bg(theme.select).add_modifier(Modifier::BOLD) } else { Style::new().fg(theme.primary) };
            let text = format!("  + {}", row.label);
            let w = (text.chars().count() as u16 + 2).min(body.width);
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(if picked { "▌" } else { " " }, Style::new().fg(theme.primary)),
                    Span::styled(text, style),
                    Span::styled(" ", style),
                ])),
                Rect { x: body.x, y, width: w + 1, height: 1 },
            );
            y += h + 1;
            continue;
        }
        let label_style = if picked { theme.brand() } else { theme.dim() };
        frame.render_widget(Span::styled(if picked { "▌" } else { " " }, Style::new().fg(theme.primary)), Rect { x: body.x, y, width: 1, height: 1 });
        let label_box = Rect { x: body.x + 1, y, width: label_w.saturating_sub(2), height: 1 };
        if row.is_named() {
            // The name is a box of its own.
            let on = picked && editor.on_name;
            let style = if on { Style::new().bg(theme.select).fg(theme.text) } else { Style::new().bg(theme.surface).fg(theme.mid) };
            frame.render_widget(Block::new().style(style), label_box);
            let text: Text = if row.name.is_empty() {
                Text::from(Span::styled(if row.slot == Slot::Question { "Question" } else { "Name" }, theme.faint()))
            } else {
                Text::from(Span::styled(row.name.text().to_owned(), style))
            };
            let col = row.name.cursor_cells() as u16;
            let inner_w = label_box.width.saturating_sub(2);
            let hscroll = if on { col.saturating_sub(inner_w.saturating_sub(1)) } else { 0 };
            let text_box = Rect { x: label_box.x + 1, width: inner_w, ..label_box };
            frame.render_widget(Paragraph::new(text).scroll((0, hscroll)), text_box);
            hit(label_box, Hit::EditorName(n));
            if on {
                cursor = Some(Position::new(text_box.x + (col - hscroll).min(inner_w.saturating_sub(1)), y));
            }
        } else {
            let label: String = row.label.chars().take(usize::from(label_box.width)).collect();
            frame.render_widget(Span::styled(label, label_style), label_box);
        }
        let field = Rect { x: body.x + label_w, y, width: body.width.saturating_sub(label_w), height: h };
        let on_value = picked && !editor.on_name;
        let bg = if on_value { Style::new().bg(theme.select) } else { Style::new().bg(theme.surface) };
        frame.render_widget(Block::new().style(bg), field);
        let text_area = Rect { x: field.x + 1, width: field.width.saturating_sub(2), ..field };
        let suggest_hint = format!("such as {}", row.suggestions.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
        let masked = |count: usize| Span::styled("•".repeat(count.min(64)), Style::new().fg(theme.text));
        let shown: Text = match row.input.as_str() {
            "toggle" => Text::from(Line::from(if row.flag {
                Span::styled("● on", Style::new().fg(theme.primary).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("○ off", Style::new().fg(theme.low))
            })),
            "choice" => Text::from(Line::from(vec![
                Span::styled("‹ ", theme.faint()),
                Span::styled(row.value.text().to_owned(), Style::new().fg(theme.text)),
                Span::styled(" ›", theme.faint()),
            ])),
            _ if row.unloaded() && row.value.is_empty() => {
                let hint = match row.slot {
                    Slot::Otp => "  saved · paste an otpauth:// link or seed to replace",
                    _ if on_value => "  saved · type to edit, ^R to show",
                    _ => "  saved",
                };
                Text::from(Line::from(vec![Span::styled(MASK, theme.dim()), Span::styled(hint, theme.faint())]))
            }
            _ if row.secret && !editor.show && !row.value.is_empty() => {
                let lines: Vec<Line> = row.value.text().split('\n').map(|l| Line::from(masked(l.chars().count().max(1)))).collect();
                Text::from(lines)
            }
            _ if row.value.is_empty() => {
                let placeholder = match (&row.slot, row.secret) {
                    (Slot::Otp, _) => "otpauth://… link or base32 seed",
                    (Slot::Sites, ..) => "github.com, https://example.com/login",
                    (Slot::Apps, ..) => "com.example.app",
                    (Slot::Tags, ..) => "work, family",
                    (Slot::Question, _) => "the answer",
                    (Slot::Custom, true) => "hidden value",
                    _ if !row.suggestions.is_empty() => &suggest_hint,
                    _ if row.input == "date" => "2026-12-31",
                    _ if row.input == "password" => "^G makes a strong one",
                    _ => "",
                };
                Text::from(Span::styled(placeholder, theme.faint()))
            }
            _ if row.slot == Slot::Apps && !on_value => Text::from(app_names(row.value.text())).style(Style::new().fg(theme.text)),
            _ => Text::from(row.value.text().to_owned()).style(Style::new().fg(theme.text)),
        };
        let (line, col) = row.value.cursor_line_col();
        let scroll = (line as u16).saturating_sub(h.saturating_sub(1));
        let hscroll = (col as u16).saturating_sub(text_area.width.saturating_sub(1));
        let hscroll = if row.multiline() { 0 } else { hscroll };
        frame.render_widget(Paragraph::new(shown).scroll((scroll, hscroll)), text_area);
        // The rest of an offered value, faint after the cursor, and how strong a password is.
        if on_value
            && !row.multiline()
            && row.value.cursor() == row.value.text().chars().count()
            && let Some(offer) = row.suggestion()
        {
            let rest: String = offer.chars().skip(row.value.text().chars().count()).collect();
            let x = text_area.x + (col as u16).saturating_sub(hscroll);
            let w = (rest.chars().count() as u16 + 9).min(text_area.right().saturating_sub(x));
            frame.render_widget(
                Paragraph::new(Line::from(vec![Span::styled(rest, theme.faint()), Span::styled("  → take", theme.faint())])),
                Rect { x, y: text_area.y, width: w, height: 1 },
            );
        }
        if on_value
            && row.input == "password"
            && let Some(score) = editor.strength
        {
            let mut spans = meter(score, theme);
            spans.push(Span::styled(format!(" {}", score_word(score)), Style::new().fg(theme.score(score))));
            let line = Line::from(spans);
            let w = line.width() as u16;
            if text_area.width > w + 20 {
                let at = Rect { x: text_area.right() - w, y: text_area.y, width: w, height: 1 };
                frame.render_widget(Block::new().style(Style::new().bg(theme.select)), Rect { x: at.x - 1, width: w + 1, ..at });
                frame.render_widget(Paragraph::new(line), at);
            }
        }
        if on_value && !matches!(row.input.as_str(), "toggle" | "choice") && !(row.unloaded() && row.value.is_empty()) {
            let cx = col as u16 - hscroll;
            cursor = Some(Position::new(text_area.x + cx.min(text_area.width.saturating_sub(1)), text_area.y + (line as u16 - scroll)));
        } else if on_value && row.unloaded() {
            cursor = Some(Position::new(text_area.x, text_area.y));
        }
        y += h + 1;
    }
    // Where the list goes on past the box.
    let more_above = first > 0;
    let more_below = last_drawn + 1 < editor.rows.len();
    if more_above || more_below {
        let text = match (more_above, more_below) {
            (true, true) => "↑↓ more",
            (true, false) => "↑ more",
            _ => "↓ more",
        };
        let w = text.chars().count() as u16;
        frame.render_widget(Span::styled(text, theme.faint()), Rect { x: body.right().saturating_sub(w), y: body.y, width: w, height: 1 });
    }
    if editor.confirm_discard {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Discard your changes?  ", Style::new().fg(theme.warn).add_modifier(Modifier::BOLD)),
                Span::styled("y", theme.key()),
                Span::styled(" discard  ", theme.dim()),
                Span::styled("^S", theme.key()),
                Span::styled(" save  ", theme.dim()),
                Span::styled("any other key", theme.key()),
                Span::styled(" keep editing", theme.dim()),
            ])),
            error,
        );
    } else if let Some(e) = &editor.error {
        frame.render_widget(Paragraph::new(Span::styled(format!("✕ {e}"), Style::new().fg(theme.danger))), error);
    }
    let row = &editor.rows[at];
    let mut keys = vec![("^S", "save"), ("Esc", "cancel")];
    if row.is_action() {
        keys.insert(0, ("Enter", "add"));
    } else {
        keys.insert(0, ("Tab", "next"));
    }
    if row.slot == Slot::Custom {
        keys.insert(1, ("^O", if row.secret { "unhide" } else { "hide" }));
        keys.insert(1, ("^X", "remove"));
    } else if row.slot == Slot::Question && row.index.is_none() {
        keys.insert(1, ("^X", "remove"));
    } else if !row.is_action() {
        keys.insert(1, ("^N", "add field"));
    }
    if row.input == "password" {
        keys.insert(1, ("^G", "generate"));
    }
    if row.secret && !row.is_action() {
        keys.insert(1, ("^R", if editor.show { "hide" } else { "show" }));
    }
    if matches!(row.input.as_str(), "toggle" | "choice") {
        keys.insert(1, ("Space", "change"));
    }
    if row.multiline() {
        keys.insert(1, ("Enter", "new line"));
    }
    hint_bar(frame, foot, theme, &keys, Alignment::Right);
    if let Some(at) = cursor {
        frame.set_cursor_position(at);
    }
}

/// The keys list. Two columns when they fit, one otherwise; either way it scrolls when the
/// window is too short for all of it, as 80 by 24 is.
fn draw_help(frame: &mut Frame, area: Rect, theme: &Theme, scroll: u16) {
    let inner = popup(frame, area, 96, (area.height - 2).min(36), theme, "Keys", false);
    let groups: [(&str, &[(&str, &str)]); 7] = [
        ("Anywhere", &[("1-5", "go to a tab"), (": or ^K", "commands"), ("^L", "lock now"), ("^T", "next theme"), ("?", "this list"), ("q", "quit")]),
        ("Moving", &[("↑↓ or j k", "move"), ("←→ or h l", "pane"), ("Tab", "next pane"), ("g G", "top, bottom"), ("Enter", "open"), ("s", "sort the list")]),
        ("Search", &[("/", "search"), ("Esc", "clear"), ("tag:work", "by tag"), ("kind:card", "by kind"), ("has:otp", "with codes"), ("weak reused old", "health")]),
        ("In the editor", &[("Tab ⇧Tab", "next, back"), ("^S Esc", "save, cancel"), ("^N ^X", "add or remove a field"), ("^O", "hide a field's value"), ("^G ^R", "generate, show"), ("→", "take the value offered")]),
        ("Item", &[("c", "copy password"), ("u", "copy username"), ("t", "copy code"), ("r", "show password"), ("e n", "edit, new"), ("d", "trash"), ("f p", "favorite, pin"), ("m + -", "folder, tags"), ("a", "attach a file"), ("w", "Wi-Fi QR code"), ("Space", "mark, to act on several")]),
        ("In an item", &[("r Space", "show or hide"), ("y Enter", "copy the line"), ("o", "save a file")]),
        ("Elsewhere", &[("u D", "trash: restore, delete for good"), ("i r", "Health: ignore, check again"), ("R D", "a sidebar tag: rename, delete"), ("←→ Enter", "Settings: change, open")]),
    ];
    let column = |groups: &[(&str, &[(&str, &str)])]| -> Vec<Line<'static>> {
        // Keys as wide as the widest in the column, so the words have the rest of it.
        let key_w = groups.iter().flat_map(|(_, keys)| keys.iter()).map(|(key, _)| key.chars().count()).max().unwrap_or(0) + 2;
        let mut lines = Vec::new();
        for (title, keys) in groups {
            lines.push(Line::from(Span::styled(title.to_uppercase(), theme.brand())));
            for (key, what) in *keys {
                lines.push(Line::from(vec![Span::styled(format!("  {key:<key_w$}"), theme.key()), Span::styled((*what).to_owned(), Style::new().fg(theme.text))]));
            }
            lines.push(Line::default());
        }
        lines
    };
    let (left, right) = (column(&groups[..4]), column(&groups[4..]));
    let widest = |lines: &[Line]| lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let two = widest(&left) + 2 <= inner.width / 2 && widest(&right) <= inner.width - inner.width / 2;
    let body = Rect { height: inner.height.saturating_sub(1), ..inner };
    let tall = if two { left.len().max(right.len()) } else { left.len() + right.len() };
    let most = u16::try_from(tall).unwrap_or(u16::MAX).saturating_sub(body.height);
    HELP_MOST.set(most);
    let scroll = scroll.min(most);
    if two {
        let half = inner.width / 2;
        frame.render_widget(Paragraph::new(left).scroll((scroll, 0)), Rect { width: half, ..body });
        frame.render_widget(Paragraph::new(right).scroll((scroll, 0)), Rect { x: inner.x + half, width: inner.width - half, ..body });
    } else {
        let mut lines = left;
        lines.extend(right);
        frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), body);
    }
    let foot = if most > 0 {
        let more = if scroll < most { "↓ more" } else { "↑ back" };
        Line::from(vec![Span::styled(format!("{more}  "), theme.brand()), Span::styled("↑↓ PgUp PgDn scroll  Esc close", theme.faint())]).centered()
    } else {
        Line::from(Span::styled(
            format!("Copied secrets clear themselves. Every change is saved at once. {} commands under :", COMMANDS.len()),
            theme.faint(),
        ))
        .centered()
    };
    frame.render_widget(Paragraph::new(foot), Rect { y: inner.bottom() - 1, height: 1, ..inner });
}

/// How far the keys list could scroll when last drawn.
pub fn help_most() -> u16 {
    HELP_MOST.get()
}

/// Digits five cells tall, for the six to compare.
const BIG: [[&str; 5]; 10] = [
    ["███", "█ █", "█ █", "█ █", "███"],
    [" █ ", "██ ", " █ ", " █ ", "███"],
    ["███", "  █", "███", "█  ", "███"],
    ["███", "  █", "███", "  █", "███"],
    ["█ █", "█ █", "███", "  █", "  █"],
    ["███", "█  ", "███", "  █", "███"],
    ["███", "█  ", "███", "█ █", "███"],
    ["███", "  █", "  █", " █ ", " █ "],
    ["███", "█ █", "███", "█ █", "███"],
    ["███", "█ █", "███", "  █", "███"],
];

/// Six digits in large type, each cell doubled across so they keep their shape.
fn big_digits<'a>(code: &str, colour: Color) -> Vec<Line<'a>> {
    (0..5)
        .map(|row| {
            let mut text = String::new();
            for (n, ch) in code.chars().enumerate() {
                if n == 3 {
                    text.push_str("    ");
                } else if n > 0 {
                    text.push_str("  ");
                }
                let glyph = ch.to_digit(10).map_or("   ", |d| BIG[d as usize][row]);
                for c in glyph.chars() {
                    text.push(c);
                    text.push(c);
                }
            }
            Line::from(Span::styled(text, Style::new().fg(colour))).centered()
        })
        .collect()
}

/// How big a QR code draws: two cells across and one down per module, or half that height
/// with half blocks.
fn qr_size(qr: &Qr, big: bool) -> (u16, u16) {
    let side = qr.width as u16 + 2 * QUIET as u16;
    if big { (side * 2, side) } else { (side, side.div_ceil(2)) }
}

/// Draws a QR code in fixed black and white, whatever the theme. Big codes paint whole cells,
/// which leave no seam in the line spacing; small ones use a half block only where the two
/// halves of a cell differ, so filled areas stay solid there too.
fn draw_qr(frame: &mut Frame, at: Rect, qr: &Qr, big: bool) {
    let (dark, light) = (Color::Indexed(16), Color::Indexed(231));
    let buf = frame.buffer_mut();
    let side = qr.width as isize + 2 * QUIET;
    let (w, h) = qr_size(qr, big);
    for row in 0..h.min(at.height) {
        for col in 0..w.min(at.width) {
            let cell = &mut buf[(at.x + col, at.y + row)];
            if big {
                let (x, y) = ((col / 2) as isize - QUIET, row as isize - QUIET);
                cell.set_symbol(" ").set_bg(if qr.dark(x, y) { dark } else { light });
            } else {
                let (x, y) = (col as isize - QUIET, row as isize * 2 - QUIET);
                let top = qr.dark(x, y);
                let bottom = y + 1 < side - QUIET && qr.dark(x, y + 1);
                if top == bottom {
                    cell.set_symbol(" ").set_bg(if top { dark } else { light });
                } else {
                    cell.set_symbol("▀").set_fg(if top { dark } else { light }).set_bg(if bottom { dark } else { light });
                }
            }
        }
    }
}

fn button<'a>(key: &'a str, label: &'a str, colour: Color, theme: &Theme) -> Vec<Span<'a>> {
    vec![
        Span::styled(format!(" {key} "), Style::new().bg(colour).fg(theme.on_primary).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {label} "), Style::new().bg(theme.surface).fg(theme.text)),
    ]
}

/// Two buttons side by side, centred, each one clickable.
fn answer_buttons(frame: &mut Frame, area: Rect, theme: &Theme, yes: &str, no: &str) {
    let mut spans = button("y", yes, theme.primary, theme);
    spans.push(Span::raw("    "));
    spans.extend(button("n", no, theme.danger, theme));
    let line = Line::from(spans);
    let width = line.width() as u16;
    let x = area.x + area.width.saturating_sub(width) / 2;
    frame.render_widget(Paragraph::new(line), Rect { x, width: width.min(area.width), height: 1, ..area });
    let yes_w = (yes.chars().count() + 5) as u16;
    let no_w = (no.chars().count() + 5) as u16;
    hit(Rect { x, width: yes_w, height: 1, ..area }, Hit::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)));
    hit(Rect { x: x + yes_w + 4, width: no_w, height: 1, ..area }, Hit::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)));
}

/// A Wi-Fi network's code, as big as the terminal allows, with what it gives away under it.
fn draw_wifi_code(frame: &mut Frame, area: Rect, ssid: &str, qr: &Qr, theme: &Theme) {
    let note = "Point another phone's camera at this to join the network. The password is in the code, so anyone who can see it can read it.";
    let (bw, bh) = qr_size(qr, true);
    let big = bw + 4 <= area.width && bh + 8 <= area.height;
    let (w, h) = qr_size(qr, big);
    let width = (w + 4).max(48).min(area.width);
    let height = (h + 7).min(area.height);
    let title = if ssid.is_empty() { "Wi-Fi network" } else { ssid };
    let inner = popup(frame, area, width, height, theme, title, false);
    let qr_area = Rect { x: inner.x + inner.width.saturating_sub(w) / 2, y: inner.y + 1, width: w.min(inner.width), height: h.min(inner.height) };
    draw_qr(frame, qr_area, qr, big);
    let below = Rect { y: qr_area.bottom() + 1, height: inner.bottom().saturating_sub(qr_area.bottom() + 1), ..inner };
    let lines = vec![
        Line::from(Span::styled(note, theme.dim())),
        Line::from(vec![Span::styled(" Esc ", theme.brand()), Span::styled(" close", theme.faint())]),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }).centered(), below);
}

fn draw_restore(frame: &mut Frame, area: Rect, ui: &crate::restore::RestoreUi, theme: &Theme) {
    use crate::restore::{Field as F, Stage as S};
    let rows = ui.fields().len() as u16;
    let inner = popup(frame, area, 76, (rows + 10).max(16), theme, "Restore from a backup location", false);
    let [body, foot] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
    let body = body.inner(Margin::new(1, 0));
    let text = Style::new().fg(theme.text);
    let mut lines: Vec<Line> = Vec::new();
    let mut cursor = None;
    let keys: Vec<(&str, &str)> = match &ui.stage {
        S::Form | S::Busy(_) => {
            lines.push(Line::from(Span::styled("Copies are encrypted; the vault opens with its master password.", theme.dim())));
            lines.push(Line::default());
            let label_w = 14usize;
            let value_w = usize::from(body.width).saturating_sub(label_w + 3).max(4);
            for field in ui.fields() {
                let focused = ui.focus == field && matches!(ui.stage, S::Form);
                let mark = Span::styled(if focused { "▌ " } else { "  " }, theme.brand());
                let label = Span::styled(format!("{:<label_w$} ", field.label(ui.kind)), if focused { theme.brand() } else { theme.dim() });
                let value = match ui.input(field) {
                    None => {
                        let kinds: Vec<Span> = crate::restore::Kind::ALL
                            .iter()
                            .flat_map(|k| {
                                let on = *k == ui.kind;
                                let style = if on { Style::new().fg(theme.on_primary).bg(theme.primary).add_modifier(Modifier::BOLD) } else { theme.dim() };
                                [Span::styled(format!(" {} ", k.label()), style), Span::raw(" ")]
                            })
                            .collect();
                        lines.push(Line::from([vec![mark, label], kinds].concat()));
                        continue;
                    }
                    Some(input) if input.is_empty() => Span::styled(field.hint().to_owned(), theme.faint()),
                    Some(input) => Span::styled(visible(input, field.secret(), value_w).0, text),
                };
                if focused && let Some(input) = ui.input(field) {
                    let col = visible(input, field.secret(), value_w).1;
                    cursor = Some(Position::new(body.x + (label_w + 3 + col) as u16, body.y + lines.len() as u16));
                }
                lines.push(Line::from(vec![mark, label, value]));
            }
            lines.push(Line::default());
            if let S::Busy(what) = &ui.stage {
                lines.push(Line::from(Span::styled(format!("{} {what}", ui.spinner()), theme.brand())));
                vec![("Esc", "stop")]
            } else {
                if let Some(error) = &ui.error {
                    lines.push(Line::from(Span::styled(format!("✕ {error}"), Style::new().fg(theme.danger))));
                }
                let mut keys = vec![("Tab", "next"), ("^S", "look for copies"), ("Esc", "back")];
                if ui.focus == F::Kind {
                    keys.insert(0, ("←→", "kind"));
                }
                keys
            }
        }
        S::Copies { list, at } => {
            lines.push(Line::from(Span::styled(format!("{} {} there, newest first. Pick one to restore.", list.len(), if list.len() == 1 { "copy" } else { "copies" }), text)));
            if let Some(key) = &ui.server_key {
                lines.push(Line::from(vec![Span::styled("Server key ", theme.dim()), Span::styled(key.clone(), Style::new().fg(theme.accent))]));
                lines.push(Line::from(Span::styled("Check that it matches your server's before going on.", theme.faint())));
            }
            lines.push(Line::default());
            let room = usize::from(body.height).saturating_sub(lines.len() + 3).max(1);
            let skip = at.saturating_sub(room - 1);
            for (n, copy) in list.iter().enumerate().skip(skip).take(room) {
                let picked = n == *at;
                let base = if picked { theme.selected(true) } else { Style::new() };
                let row = crate::restore::describe(copy);
                let pad = usize::from(body.width).saturating_sub(row.chars().count() + 2);
                lines.push(Line::from(vec![Span::styled(if picked { "▌ " } else { "  " }, base.fg(theme.primary)), Span::styled(format!("{row}{}", " ".repeat(pad)), base.fg(theme.text))]));
            }
            if let Some(error) = &ui.error {
                lines.push(Line::default());
                lines.push(Line::from(Span::styled(format!("✕ {error}"), Style::new().fg(theme.danger))));
            }
            vec![("↑↓", "move"), ("Enter", "restore"), ("Esc", "back")]
        }
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
    if let Some(at) = cursor {
        frame.set_cursor_position(at);
    }
    hint_bar(frame, foot, theme, &keys, Alignment::Right);
}

/// `text` as lines no wider than `width`, broken between words.
fn wrapped(text: &str, width: u16, style: Style) -> Vec<Line<'static>> {
    let width = usize::from(width.max(10));
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(Line::from(Span::styled(std::mem::take(&mut line), style)));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    lines.push(Line::from(Span::styled(line, style)));
    lines
}

fn draw_link(frame: &mut Frame, area: Rect, link: &LinkUi, theme: &Theme) {
    let title = match link.role {
        Role::Receive => "Get your vault from another device",
        Role::Send => "Add a new device",
    };
    // The whole screen: what is behind it has nothing to do with this.
    let inner = popup(frame, area, area.width, area.height, theme, title, false);
    let [body, foot] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
    let body = body.inner(Margin::new(1, 0));
    let faint = |s: String| Line::from(Span::styled(s, theme.faint()));
    let mut keys: Vec<(&str, &str)> = Vec::new();

    match &link.stage {
        Stage::Starting => {
            frame.render_widget(Paragraph::new(Line::from(Span::styled(format!("{} Getting ready…", link.spinner()), theme.brand()))), body);
            keys.push(("Esc", "stop"));
        }
        Stage::Waiting { code, address } => {
            let qr = code.as_deref().and_then(Qr::new);
            // The code goes left and the steps right when both fit, else the steps go under.
            let text_w = 50u16;
            let mut steps = body;
            if let Some(qr) = &qr {
                let fits = |big: bool| {
                    let (w, h) = qr_size(qr, big);
                    (w + text_w + 3 <= body.width && h <= body.height) || (w <= body.width && h + 12 <= body.height)
                };
                let big = link.big.unwrap_or_else(|| fits(true));
                let (w, h) = qr_size(qr, big);
                let qr_area = if w + text_w + 3 <= body.width {
                    steps = Rect { x: body.x + w + 3, width: body.width - w - 3, ..body };
                    Rect { x: body.x, y: body.y + body.height.saturating_sub(h) / 2, width: w, height: h.min(body.height) }
                } else {
                    steps = Rect { y: body.y + h + 1, height: body.height.saturating_sub(h + 1), ..body };
                    Rect { x: body.x + body.width.saturating_sub(w) / 2, y: body.y, width: w.min(body.width), height: h.min(body.height) }
                };
                draw_qr(frame, qr_area, qr, big);
                DREW_BIG.set(big);
                keys.push(("c", "copy the code"));
                keys.push(("z", if big { "smaller code" } else { "bigger code" }));
            }
            let mut lines = vec![
                Line::from(vec![Span::styled("1  ", theme.brand()), Span::styled("On the device that has your vault, open", Style::new().fg(theme.text))]),
                Line::from(vec![Span::raw("   "), Span::styled("Settings › Sync and accounts › Add a new device", Style::new().fg(theme.accent).add_modifier(Modifier::BOLD))]),
                Line::default(),
                Line::from(vec![Span::styled("2  ", theme.brand()), Span::styled("Then do one of these:", Style::new().fg(theme.text))]),
            ];
            if qr.is_some() {
                lines.push(Line::from(vec![Span::styled("   ▸ ", theme.brand()), Span::styled("Scan this code", Style::new().fg(theme.text).add_modifier(Modifier::BOLD)), Span::styled("  with a phone's camera", theme.faint())]));
            }
            lines.push(Line::from(vec![Span::styled("   ▸ ", theme.brand()), Span::styled("Find it on this network", Style::new().fg(theme.text).add_modifier(Modifier::BOLD)), Span::styled("  and compare six digits", theme.faint())]));
            if let Some(code) = code {
                lines.push(Line::from(vec![Span::styled("   ▸ ", theme.brand()), Span::styled("Paste its code", Style::new().fg(theme.text).add_modifier(Modifier::BOLD)), Span::styled("  c copies it here:", theme.faint())]));
                let room = usize::from(steps.width.saturating_sub(5)).max(10);
                let chars: Vec<char> = code.chars().collect();
                for chunk in chars.chunks(room) {
                    lines.push(Line::from(Span::styled(format!("     {}", chunk.iter().collect::<String>()), Style::new().fg(theme.mid))));
                }
            }
            lines.push(Line::default());
            let left = link.seconds_left();
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", link.spinner()), theme.brand()),
                Span::styled("Waiting for your other device", Style::new().fg(theme.text)),
                Span::styled(format!("  {}:{:02} left", left / 60, left % 60), Style::new().fg(if left < 20 { theme.warn } else { theme.low })),
            ]));
            let here = match address {
                Some(address) => format!("This computer: {} · {address}", crate::join::device_name()),
                None => format!("This computer: {} · no network address for a code", crate::join::device_name()),
            };
            lines.push(faint(here));
            lines.push(Line::default());
            lines.push(faint("Both need the same network. The vault comes over encrypted and opens only with its master password.".into()));
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), steps);
            keys.push(("Esc", "stop"));
        }
        Stage::Searching => {
            // Wrapped by hand, since the list and the box below are placed by line count.
            let mut lines = wrapped("On the new device, choose I already have a vault, then Get it from another device.", body.width, Style::new().fg(theme.text));
            lines.extend(wrapped("It shows a code and waits. Pick it below, or paste the text under its code.", body.width, theme.dim()));
            lines.extend([
                Line::default(),
                Line::from(vec![
                    Span::styled("Devices waiting on this network  ", theme.brand()),
                    Span::styled(link.spinner(), theme.faint()),
                ]),
            ]);
            let list_y = body.y + lines.len() as u16;
            if link.found.is_empty() {
                lines.push(faint("  Looking… none yet. Both need the same network.".into()));
            }
            for (n, address) in link.found.iter().enumerate() {
                let picked = n == link.at;
                let base = if picked { theme.selected(true) } else { Style::new() };
                lines.push(Line::from(vec![
                    Span::styled(if picked { "▌ " } else { "  " }, base.fg(theme.primary)),
                    Span::styled(format!("{} ", theme.g.get(Glyph::Brand)), base.fg(theme.accent)),
                    Span::styled(format!("New device at {address}"), base.fg(theme.text).add_modifier(Modifier::BOLD)),
                    Span::styled(if picked { "   Enter sends, after six digits match" } else { "" }, base.fg(theme.low)),
                ]));
            }
            for n in 0..link.found.len() {
                hit(Rect { x: body.x, y: list_y + n as u16, width: body.width, height: 1 }, Hit::LinkDevice(n));
            }
            lines.push(Line::default());
            lines.push(Line::from(Span::styled("Or paste its code", theme.brand())));
            let used = lines.len() as u16;
            frame.render_widget(Paragraph::new(lines), body);
            let boxed = Rect { y: body.y + used, height: 3, ..body };
            if boxed.bottom() <= body.bottom() {
                let look = BoxLook { label: "Code", secret: false, focused: true, placeholder: "wmpass-pair:1:…  (paste with your terminal's paste keys)" };
                if let Some(at) = text_box(frame, boxed, theme, &link.code, &look) {
                    frame.set_cursor_position(at);
                }
            }
            keys.extend([("↑↓", "pick"), ("Enter", "send"), ("Esc", "stop")]);
        }
        Stage::Busy(what) => {
            let lines = vec![Line::default(), Line::from(Span::styled(format!("{} {what}", link.spinner()), theme.brand())).centered()];
            frame.render_widget(Paragraph::new(lines), body);
            keys.push(("Esc", "stop"));
        }
        Stage::Digits { code, peer } => {
            let mut lines = vec![
                Line::default(),
                Line::from(Span::styled("Do both screens show these six digits?", Style::new().fg(theme.text).add_modifier(Modifier::BOLD))).centered(),
                Line::default(),
            ];
            if body.width >= 60 && body.height >= 14 {
                lines.extend(big_digits(code, theme.accent));
            } else {
                lines.push(Line::from(Span::styled(crate::join::spaced(code), Style::new().fg(theme.accent).add_modifier(Modifier::BOLD))).centered());
            }
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(format!("The other device is {peer}"), theme.dim())).centered());
            lines.push(Line::default());
            let used = lines.len() as u16;
            frame.render_widget(Paragraph::new(lines), body);
            if used < body.height {
                answer_buttons(frame, Rect { y: body.y + used, height: 1, ..body }, theme, "They match", "They differ");
            }
            keys.extend([("y", "match"), ("n", "differ"), ("Esc", "stop")]);
        }
        Stage::Received { path, from } => {
            let lines = vec![
                Line::default(),
                Line::from(Span::styled("✓ Your vault is here", Style::new().fg(theme.ok).add_modifier(Modifier::BOLD))).centered(),
                Line::default(),
                Line::from(Span::styled(format!("It came from {from}, and is kept in {}.", path.display()), Style::new().fg(theme.text))).centered(),
                Line::from(Span::styled("The two are paired. Unlock it with the vault's master password.", theme.dim())).centered(),
            ];
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), body);
            keys.push(("Enter", "unlock it"));
        }
        Stage::Sent { to } => {
            let lines = vec![
                Line::default(),
                Line::from(Span::styled(format!("✓ Sent to {to}"), Style::new().fg(theme.ok).add_modifier(Modifier::BOLD))).centered(),
                Line::default(),
                Line::from(Span::styled("Unlock it there with this vault's master password.", Style::new().fg(theme.text))).centered(),
            ];
            frame.render_widget(Paragraph::new(lines), body);
            keys.push(("Enter", "done"));
        }
        Stage::Failed(problem) => {
            let lines = vec![
                Line::default(),
                Line::from(Span::styled(format!("✕ {problem}"), Style::new().fg(theme.danger))).centered(),
            ];
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), body);
            keys.extend([("r", "try again"), ("Esc", "close")]);
        }
    }
    hint_bar(frame, foot, theme, &keys, Alignment::Right);
}

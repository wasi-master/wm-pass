//! The app driven by keys and drawn into a buffer, as a person would see it.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;

use crate::app::{App, Overlay, Screen};
use crate::link::{LinkUi, Role, Stage};
use crate::engine;

const PASSWORD: &str = "Tr0ub4dor-and-3-horses";

fn vault(dir: &std::path::Path) -> Box<engine::Engine> {
    let mut engine = engine::create(&dir.join("t.wmvault"), "Personal", b"master-pw", 3).expect("create");
    let mut plain = serde_json::Map::new();
    plain.insert("username".into(), json!("octo"));
    let id = engine.create("login", "GitHub", &plain, &[json!({"Domain": "github.com"})]).expect("login");
    engine.set_secret(&id, "password", PASSWORD.as_bytes()).expect("password");
    engine.set_otp(&id, Some(b"otpauth://totp/GitHub:octo?secret=JBSWY3DPEHPK3PXP&issuer=GitHub")).expect("otp");
    engine.set_common(&id, json!({"tags": ["dev"], "favourite": true})).expect("tags");
    engine.create("note", "Wi-Fi at home", &serde_json::Map::new(), &[]).expect("note");
    Box::new(engine)
}

fn screen(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal.draw(|frame| crate::draw::draw(frame, app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn press(app: &mut App, code: KeyCode) {
    app.key_for_test(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ctrl(app: &mut App, ch: char) {
    app.key_for_test(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL));
}

fn typed(app: &mut App, text: &str) {
    for ch in text.chars() {
        press(app, KeyCode::Char(ch));
    }
}

#[test]
fn a_secret_shows_only_when_asked_and_hides_on_lock() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let shown = screen(&mut app, 140, 40);
    assert!(shown.contains("GitHub") && shown.contains("Wi-Fi at home"), "{shown}");
    assert!(shown.contains("octo"));
    assert!(!shown.contains(PASSWORD), "a password is never drawn unasked");

    // The list is sorted by title, so GitHub is first and picked.
    press(&mut app, KeyCode::Char('r'));
    let revealed = screen(&mut app, 140, 40);
    assert!(revealed.contains(PASSWORD), "{revealed}");
    press(&mut app, KeyCode::Char('r'));
    assert!(!screen(&mut app, 140, 40).contains(PASSWORD), "r hides it again");

    press(&mut app, KeyCode::Char('r'));
    ctrl(&mut app, 'l');
    assert!(matches!(app.screen, Screen::Door(_)));
    let door = screen(&mut app, 140, 40);
    assert!(!door.contains(PASSWORD) && !door.contains("GitHub"));
    assert!(door.contains("Unlock t.wmvault"), "{door}");
}

#[test]
fn every_tab_and_size_draws() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    for tab in ['1', '2', '3', '4', '5'] {
        press(&mut app, KeyCode::Char(tab));
        for (w, h) in [(160, 48), (100, 30), (70, 24), (44, 14), (30, 8)] {
            let shown = screen(&mut app, w, h);
            assert!(!shown.contains(PASSWORD), "tab {tab} at {w}x{h}");
        }
    }
    press(&mut app, KeyCode::Char('2'));
    let codes = screen(&mut app, 120, 30);
    assert!(codes.contains("GitHub") && codes.contains("octo"), "{codes}");
    press(&mut app, KeyCode::Char('3'));
    let generator = screen(&mut app, 120, 30);
    assert!(generator.contains("bits"), "{generator}");
    press(&mut app, KeyCode::Char('?'));
    assert!(screen(&mut app, 120, 40).contains("copy password"));
}

#[test]
fn a_new_login_from_the_keyboard() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('n'));
    assert!(matches!(app.overlay, Some(Overlay::Pick { .. })));
    press(&mut app, KeyCode::Enter);
    assert!(matches!(app.overlay, Some(Overlay::Editor(_))));
    typed(&mut app, "Mastodon");
    press(&mut app, KeyCode::Tab);
    typed(&mut app, "me@example.social");
    press(&mut app, KeyCode::Tab);
    ctrl(&mut app, 'g');
    press(&mut app, KeyCode::Tab);
    typed(&mut app, "mastodon.social");
    ctrl(&mut app, 's');
    assert!(app.overlay.is_none(), "saving closes the editor");

    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let row = ui.current().cloned().expect("picked");
    assert_eq!(row.title, "Mastodon");
    assert_eq!(row.subtitle, "me@example.social");
    assert!(row.has_password, "^G filled the password");
    assert_eq!(row.site.as_deref(), Some("mastodon.social"));

    // It is in the file, not just in memory.
    let path = ui.engine.path().to_owned();
    let engine::Unlock::Opened(mut again, _) = engine::unlock(&path, b"master-pw", None, 4).expect("opens") else {
        panic!("did not open");
    };
    assert_eq!(again.list("mastodon", "").expect("list").len(), 1);
}

#[test]
fn search_trash_and_restore() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('/'));
    typed(&mut app, "tag:dev");
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert_eq!(ui.rows.len(), 1);

    press(&mut app, KeyCode::Char('d'));
    assert!(matches!(app.overlay, Some(Overlay::Confirm { .. })));
    press(&mut app, KeyCode::Char('y'));
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert!(ui.rows.is_empty(), "gone from the list");
    let trash = ui.sections.iter().position(|s| s.query == "trashed").expect("trash");
    ui.search.clear();
    ui.section = trash;
    ui.refresh_list(None);
    assert_eq!(ui.rows.len(), 1);
    press(&mut app, KeyCode::Char('u'));
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert!(ui.rows.is_empty(), "brought back out of the trash");
    assert_eq!(ui.total, 2);
}

#[test]
fn the_palette_finds_and_runs() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char(':'));
    typed(&mut app, "health");
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert_eq!(ui.tab, crate::vault::Tab::Health);
    assert!(ui.health.is_some());
    assert!(screen(&mut app, 120, 30).contains("/ 100"));
}

#[test]
fn the_palette_sets_how_long_the_clipboard_keeps_a_secret() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    assert_eq!(app.prefs.clip_seconds, 60);
    press(&mut app, KeyCode::Char(':'));
    typed(&mut app, "clear the clipboard");
    press(&mut app, KeyCode::Enter);
    assert!(screen(&mut app, 120, 30).contains("Clear the clipboard after"));
    for _ in 0..2 {
        press(&mut app, KeyCode::Backspace);
    }
    typed(&mut app, "45");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.prefs.clip_seconds, 45);
    assert!(screen(&mut app, 120, 30).contains("cleared after 45 seconds"));
}

#[test]
fn nerd_font_icons_from_the_palette() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    assert!(screen(&mut app, 140, 40).contains('◉'), "plain icons by default");
    press(&mut app, KeyCode::Char(':'));
    typed(&mut app, "nerd");
    press(&mut app, KeyCode::Enter);
    let shown = screen(&mut app, 140, 40);
    assert!(shown.contains('\u{f084}') && !shown.contains('◉'), "{shown}");
    app.command(crate::app::Command::Theme);
    assert!(app.theme.g.nerd, "a new theme keeps the icons");
}

#[test]
fn editing_leaves_secrets_alone_unless_typed() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('e'));
    let Some(Overlay::Editor(editor)) = &app.overlay else { panic!("no editor") };
    assert!(editor.rows.iter().filter(|r| r.secret).all(|r| r.value.is_empty()), "no secret is loaded to edit a title");
    ctrl(&mut app, 'e');
    typed(&mut app, " Enterprise");
    ctrl(&mut app, 's');
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let id = ui.current_id().expect("picked");
    assert_eq!(ui.current().expect("row").title, "GitHub Enterprise");
    let password = ui.engine.reveal(&engine::Secret::Field("password".into()), &id).expect("reveal");
    assert_eq!(password.as_str(), PASSWORD);
    assert!(ui.detail.as_ref().expect("detail").history.is_empty(), "no revision for an untouched password");
}

/// Writes each screen as a coloured HTML page, for looking at the design without a terminal.
/// Run with `WMPASS_TUI_SNAPSHOTS=<dir> cargo test -p wmpass-tui snapshots -- --ignored`.
#[test]
#[ignore = "writes files; run by hand"]
fn snapshots() {
    use ratatui::style::{Color, Modifier};
    let Some(out) = std::env::var_os("WMPASS_TUI_SNAPSHOTS").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&out).expect("dir");
    let colour = |c: Color, fallback: &str| match c {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => fallback.to_owned(),
    };
    let html = |app: &mut App, w: u16, h: u16| -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("terminal");
        terminal.draw(|frame| crate::draw::draw(frame, app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let mut page = String::from("<html><head><meta charset=\"utf-8\"></head><body style=\"margin:0;background:#0E1513\"><pre style=\"font:14px/1.25 'FiraCode Nerd Font Mono','FiraCode Nerd Font','JetBrains Mono',Menlo,monospace;margin:12px\">");
        for y in 0..h {
            for x in 0..w {
                let cell = &buffer[(x, y)];
                let bold = if cell.modifier.contains(Modifier::BOLD) { "font-weight:bold;" } else { "" };
                let symbol = match cell.symbol() {
                    "<" => "&lt;",
                    ">" => "&gt;",
                    "&" => "&amp;",
                    other => other,
                };
                page.push_str(&format!(
                    "<span style=\"color:{};background:{};{bold}\">{symbol}</span>",
                    colour(cell.fg, "#DDE4E1"),
                    colour(cell.bg, "#0E1513"),
                ));
            }
            page.push('\n');
        }
        page.push_str("</pre></body></html>");
        page
    };
    let dir = tempfile::tempdir().expect("dir");
    let mut engine = vault(dir.path());
    for (title, user, site) in [("Proton Mail", "me@proton.me", "proton.me"), ("Amazon", "shopper@example.com", "amazon.com"), ("Bank of Somewhere", "acct-1234", "bank.example")] {
        let mut plain = serde_json::Map::new();
        plain.insert("username".into(), json!(user));
        let id = engine.create("login", title, &plain, &[json!({"Domain": site})]).expect("login");
        engine.set_secret(&id, "password", b"password1").expect("weak");
    }
    let mut plain = serde_json::Map::new();
    plain.insert("holder".into(), json!("Ada Lovelace"));
    plain.insert("issuer".into(), json!("Visa"));
    let card = engine.create("card", "Travel card", &plain, &[]).expect("card");
    engine.set_secret(&card, "number", b"4111111111111111").expect("number");
    let mut app = App::over(engine);
    std::fs::write(out.join("items.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Char('l'));
    press(&mut app, KeyCode::Char('r'));
    std::fs::write(out.join("detail.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Char('2'));
    std::fs::write(out.join("codes.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Char('3'));
    std::fs::write(out.join("generator.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Char('4'));
    std::fs::write(out.join("health.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Char('5'));
    std::fs::write(out.join("settings.html"), html(&mut app, 150, 40)).expect("write");
    std::fs::write(out.join("settings-narrow.html"), html(&mut app, 80, 30)).expect("write");
    press(&mut app, KeyCode::Char('1'));
    press(&mut app, KeyCode::Char('e'));
    std::fs::write(out.join("editor.html"), html(&mut app, 150, 40)).expect("write");
    ctrl(&mut app, 'n');
    typed(&mut app, "Recovery email");
    press(&mut app, KeyCode::Tab);
    typed(&mut app, "backup@example.com");
    std::fs::write(out.join("editor-custom.html"), html(&mut app, 150, 40)).expect("write");
    {
        let Some(Overlay::Editor(editor)) = &mut app.overlay else { panic!("no editor") };
        editor.at = editor.rows.iter().position(|r| r.key == "password").expect("password");
    }
    typed(&mut app, "x");
    std::fs::write(out.join("editor-password.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Esc);
    std::fs::write(out.join("editor-discard.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Char('y'));
    press(&mut app, KeyCode::Char(':'));
    std::fs::write(out.join("palette.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Esc);
    std::fs::write(out.join("narrow.html"), html(&mut app, 70, 30)).expect("write");
    app.theme.g.nerd = true;
    std::fs::write(out.join("nerd.html"), html(&mut app, 150, 40)).expect("write");
    app.theme.g.nerd = false;
    ctrl(&mut app, 'l');
    std::fs::write(out.join("door.html"), html(&mut app, 150, 40)).expect("write");
}

fn click(app: &mut App, x: u16, y: u16) {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    app.mouse_for_test(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
}

/// Where a piece of text was drawn.
fn find(shown: &str, text: &str) -> (u16, u16) {
    for (y, line) in shown.lines().enumerate() {
        if let Some(at) = line.find(text) {
            return (line[..at].chars().count() as u16, y as u16);
        }
    }
    panic!("{text} is not on screen:\n{shown}");
}

#[test]
fn the_join_code_draws_in_solid_cells_that_scan() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let code = "wmpass-pair:1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:1800000000000:MTkyLjE2OC4xLjQ6NTEyMzQ".to_owned();
    app.overlay = Some(Overlay::Link(Box::new(LinkUi::at_stage(
        Role::Receive,
        Stage::Waiting { code: Some(code.clone()), address: Some("192.168.1.4:51234".into()) },
    ))));
    let mut terminal = Terminal::new(TestBackend::new(200, 60)).expect("terminal");
    terminal.draw(|frame| crate::draw::draw(frame, &mut app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let (black, white) = (ratatui::style::Color::Indexed(16), ratatui::style::Color::Indexed(231));
    let mut painted = 0;
    for cell in buffer.content() {
        if cell.bg == black || cell.bg == white {
            painted += 1;
            assert_eq!(cell.symbol(), " ", "a big code paints whole cells, with no half blocks to leave seams");
        }
    }
    let qr = crate::join::Qr::new(&code).expect("qr");
    let side = qr.width + 2 * crate::join::QUIET as usize;
    assert_eq!(painted, side * side * 2);
    let shown = screen(&mut app, 200, 60);
    assert!(shown.contains("Find it on this network"), "the network way is offered too");
    assert!(shown.contains("Paste its code"));
    // Small terminals get the half-block code instead, which still fits.
    let shown = screen(&mut app, 100, 40);
    assert!(shown.contains("▀"));
}

#[test]
fn digits_are_answered_with_a_click() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    app.overlay = Some(Overlay::Link(Box::new(LinkUi::at_stage(
        Role::Send,
        Stage::Digits { code: "123456".into(), peer: "Pixel".into() },
    ))));
    let shown = screen(&mut app, 120, 40);
    assert!(shown.contains("Pixel"));
    let (x, y) = find(&shown, "They differ");
    // With nothing on the network behind it the answer goes nowhere, but the screen moves on.
    click(&mut app, x, y);
    let Some(Overlay::Link(link)) = &app.overlay else { panic!("the screen closed") };
    assert_eq!(link.stage, Stage::Digits { code: "123456".into(), peer: "Pixel".into() });
    // Esc in the hint bar closes it.
    let shown = screen(&mut app, 120, 40);
    let (x, y) = find(&shown, "Esc stop");
    click(&mut app, x, y);
    assert!(app.overlay.is_none());
}

#[test]
fn the_mouse_picks_tabs_rows_and_palette_commands() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let shown = screen(&mut app, 140, 40);
    let (x, y) = find(&shown, "Wi-Fi at home");
    click(&mut app, x, y);
    let Screen::Vault(ui) = &app.screen else { panic!() };
    assert_eq!(ui.current().map(|r| r.title.as_str()), Some("Wi-Fi at home"));
    let (x, y) = find(&shown, "Generator");
    click(&mut app, x, y);
    let Screen::Vault(ui) = &app.screen else { panic!() };
    assert_eq!(ui.tab, crate::vault::Tab::Generator);
    // The hint bar's keys are buttons.
    press(&mut app, KeyCode::Char(':'));
    typed(&mut app, "go to");
    let shown = screen(&mut app, 140, 40);
    let (x, y) = find(&shown, "Go to Health");
    click(&mut app, x, y);
    assert!(app.overlay.is_none());
    let Screen::Vault(ui) = &app.screen else { panic!() };
    assert_eq!(ui.tab, crate::vault::Tab::Health);
    // A click outside the palette closes it.
    press(&mut app, KeyCode::Char(':'));
    screen(&mut app, 140, 40);
    click(&mut app, 1, 39);
    assert!(app.overlay.is_none());
}

#[test]
#[ignore = "prints a screen to look at"]
fn print_join_screens() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let code = "wmpass-pair:1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:1800000000000:MTkyLjE2OC4xLjQ6NTEyMzQ".to_owned();
    app.overlay = Some(Overlay::Link(Box::new(LinkUi::at_stage(Role::Receive, Stage::Waiting { code: Some(code), address: Some("192.168.1.4:51234".into()) }))));
    println!("{}", screen(&mut app, 150, 45));
    let mut link = LinkUi::at_stage(Role::Send, Stage::Searching);
    link.found = vec!["192.168.1.20:5123".parse().unwrap()];
    app.overlay = Some(Overlay::Link(Box::new(link)));
    println!("{}", screen(&mut app, 110, 30));
    app.overlay = Some(Overlay::Link(Box::new(LinkUi::at_stage(Role::Send, Stage::Digits { code: "482913".into(), peer: "Pixel 9".into() }))));
    println!("{}", screen(&mut app, 110, 30));
}

#[test]
fn health_groups_reused_passwords_ignores_per_check_and_leaves_out_local_logins() {
    let dir = tempfile::tempdir().expect("dir");
    let mut engine = vault(dir.path());
    let mut weak = |title: &str, site: &str| {
        let id = engine.create("login", title, &serde_json::Map::new(), &[json!({"Url": site})]).expect("login");
        engine.set_secret(&id, "password", b"password1").expect("password");
        id
    };
    let mail = weak("Mail", "https://mail.example.com");
    weak("Shop", "https://shop.example.org");
    weak("Router", "http://192.168.1.1");
    let mut app = App::over(engine);
    press(&mut app, KeyCode::Char('4'));

    // Local addresses are left out by default; the reused ones come as one group per password.
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert_eq!(ui.health_ids(0).len(), 2, "the router's login is left out");
    assert!(matches!(&ui.health_lines(1)[0], crate::vault::HealthLine::Heading(text) if text == "2 items share one password"));
    assert_eq!(ui.health_groups(), crate::vault::IGNORED_GROUP, "no Ignored group until something is");
    ui.health_group = 1;
    let reused = screen(&mut app, 140, 30);
    assert!(reused.contains("2 items share one password"), "{reused}");

    // Ignoring one leaves it out of that check only, and lists it under Ignored.
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    ui.health_group = 0;
    ui.health_at = ui.health_ids(0).iter().position(|id| *id == mail).expect("mail is weak");
    press(&mut app, KeyCode::Char('i'));
    assert_eq!(app.prefs.health_ignored, vec![format!("weak:{mail}")]);
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert!(!ui.health_ids(0).contains(&mail));
    assert!(ui.health_ids(1).contains(&mail), "still reused");
    assert_eq!(ui.health_groups(), crate::vault::HEALTH_GROUPS.len());
    ui.health_group = crate::vault::IGNORED_GROUP;
    ui.health_at = 0;
    let ignored = screen(&mut app, 140, 30);
    assert!(ignored.contains("Mail") && ignored.contains("Weak passwords"), "{ignored}");

    // And it counts again from there.
    press(&mut app, KeyCode::Char('i'));
    assert!(app.prefs.health_ignored.is_empty());
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert!(ui.health_ids(0).contains(&mail));
    assert!(ui.health_group < crate::vault::IGNORED_GROUP, "the empty Ignored group goes");

    app.command(crate::app::Command::SkipLocal);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert_eq!(ui.health_ids(0).len(), 3, "the router's login counts again");
}

#[test]
fn a_login_shows_where_to_change_its_password() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let shown = screen(&mut app, 160, 40);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    let line = ui.lines.iter().find_map(|line| match line {
        crate::vault::Line::Value { label, text, .. } if label == "Change password" => text.clone(),
        _ => None,
    });
    assert_eq!(line.as_deref(), Some("https://github.com/.well-known/change-password"), "{:?}", ui.lines);
    assert!(shown.contains("Change password"), "{shown}");
}

/// Picks the row with this title in the list.
fn pick(app: &mut App, title: &str) {
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let at = ui.rows.iter().position(|r| r.title == title).expect("row");
    ui.list.select(Some(at));
    ui.load_detail();
}

#[test]
fn apps_show_by_name_and_wifi_shows_its_code() {
    let dir = tempfile::tempdir().expect("dir");
    let mut engine = vault(dir.path());
    engine
        .create("login", "Chat", &serde_json::Map::new(), &[json!({"AndroidApp": {"package": "com.facebook.orca", "cert_sha256": null}})])
        .expect("login");
    let mut plain = serde_json::Map::new();
    plain.insert("ssid".into(), json!("Home"));
    plain.insert("security".into(), json!("wpa"));
    let wifi = engine.create("wifi", "Home network", &plain, &[]).expect("wifi");
    engine.set_secret(&wifi, "password", b"hunter22").expect("password");
    let mut app = App::over(engine);

    pick(&mut app, "Chat");
    let shown = screen(&mut app, 140, 40);
    assert!(shown.contains("Messenger (com.facebook.orca)"), "{shown}");
    press(&mut app, KeyCode::Char('w'));
    assert!(app.overlay.is_none(), "only a Wi-Fi network has a code");

    pick(&mut app, "Home network");
    press(&mut app, KeyCode::Char('w'));
    let Some(Overlay::WifiCode { ssid, qr }) = &app.overlay else { panic!("no code") };
    assert_eq!(ssid, "Home");
    assert_eq!(qr.width, qrcode::QrCode::new(b"WIFI:T:WPA;S:Home;P:hunter22;;").expect("qr").width());
    let shown = screen(&mut app, 160, 60);
    assert!(shown.contains("join the network"), "{shown}");
    assert!(!shown.contains("hunter22"), "the password is only in the code");
    press(&mut app, KeyCode::Esc);
    assert!(app.overlay.is_none());
}

#[test]
fn a_tag_is_renamed_and_deleted_from_the_sidebar() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let pick_tag = |app: &mut App, tag: &str| {
        let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
        ui.pane = crate::vault::Pane::Sidebar;
        ui.section = ui.sections.iter().position(|s| s.label == tag && !s.heading).expect("tag in the sidebar");
    };
    pick_tag(&mut app, "dev");
    press(&mut app, KeyCode::Char('R'));
    assert!(matches!(app.overlay, Some(Overlay::Prompt { .. })), "R asks for the new name");
    for _ in 0..3 {
        press(&mut app, KeyCode::Backspace);
    }
    typed(&mut app, "code");
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let row = ui.engine.list("", "title").expect("list").into_iter().find(|r| r.title == "GitHub").expect("row");
    assert_eq!(row.tags, vec!["code".to_owned()]);
    assert!(ui.picked_tag().as_deref() == Some("code"), "the sidebar follows the new name");

    press(&mut app, KeyCode::Char('D'));
    assert!(matches!(app.overlay, Some(Overlay::Confirm { .. })), "D asks first");
    press(&mut app, KeyCode::Char('y'));
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let row = ui.engine.list("", "title").expect("list").into_iter().find(|r| r.title == "GitHub").expect("row");
    assert!(row.tags.is_empty(), "the tag came off, the item stayed");
}

#[test]
fn marked_items_are_acted_on_together() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char(' '));
    assert!(screen(&mut app, 140, 40).contains("2 marked"));

    let run = |app: &mut App, words: &str| {
        press(app, KeyCode::Char(':'));
        typed(app, words);
        press(app, KeyCode::Enter);
    };
    run(&mut app, "add a tag");
    typed(&mut app, "work");
    press(&mut app, KeyCode::Enter);
    run(&mut app, "favorite or");
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert!(ui.rows.iter().all(|r| r.tags.contains(&"work".to_owned())), "tagged both");
    assert!(ui.rows.iter().all(|r| r.favourite), "one was a favorite, so both became one");
    assert_eq!(ui.marked.len(), 2, "still marked");

    run(&mut app, "color");
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert!(ui.rows.iter().all(|r| r.color.as_deref() == Some("25")), "both red");
    assert!(ui.marked.is_empty(), "a color clears the marks, as on the phone");

    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char(' '));
    run(&mut app, "link the marked");
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let ids: Vec<String> = ui.rows.iter().map(|r| r.id.clone()).collect();
    for (n, id) in ids.iter().enumerate() {
        let other = &ids[1 - n];
        assert_eq!(ui.engine.detail(id).expect("detail").links, vec![other.clone()]);
    }

    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Esc);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert!(ui.marked.is_empty(), "Esc unmarks");
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('d'));
    press(&mut app, KeyCode::Char('y'));
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert!(ui.rows.is_empty() && ui.marked.is_empty(), "both in the trash");
}

/// Moves the editor's cursor to the first row that `wanted` picks.
fn editor_to(app: &mut App, wanted: impl Fn(&crate::editor::Row) -> bool) {
    let Some(Overlay::Editor(editor)) = &mut app.overlay else { panic!("no editor") };
    editor.at = editor.rows.iter().position(wanted).expect("row");
    editor.on_name = false;
}

fn current_detail(app: &mut App) -> engine::Detail {
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let id = ui.current_id().expect("picked");
    ui.engine.detail(&id).expect("detail")
}

#[test]
fn custom_fields_apps_and_questions_are_edited_in_place() {
    use crate::editor::Slot;
    let dir = tempfile::tempdir().expect("dir");
    let mut engine = vault(dir.path());
    let site = json!({"AndroidApp": {"package": "com.facebook.orca", "cert_sha256": "ab:cd"}});
    let id = engine.create("login", "Chat", &serde_json::Map::new(), &[json!({"Domain": "messenger.com"}), site]).expect("login");
    engine.set_custom(&id, None, "Account number", "text", b"12345").expect("custom");
    engine.set_custom(&id, None, "Recovery PIN", "hidden", b"9999").expect("hidden");
    engine.set_custom(&id, None, "Old", "url", b"https://old.example").expect("third");
    engine.set_question(&id, None, "First pet?", b"Rex").expect("question");
    let mut app = App::over(engine);
    pick(&mut app, "Chat");
    press(&mut app, KeyCode::Char('e'));

    let shown = screen(&mut app, 140, 50);
    assert!(shown.contains("Account number") && shown.contains("12345"), "a custom field shows its value\n{shown}");
    assert!(shown.contains("Messenger (com.facebook.orca)"), "an app shows by name\n{shown}");
    assert!(shown.contains("More fields") && shown.contains("Security questions") && shown.contains("First pet?"), "{shown}");
    assert!(!shown.contains("9999") && !shown.contains("Rex"), "hidden values stay hidden until edited");

    // A plain custom field takes typing like any other.
    editor_to(&mut app, |r| r.slot == Slot::Custom && r.name.text() == "Account number");
    typed(&mut app, "-6");
    // A hidden one is loaded by the first key, then edited.
    editor_to(&mut app, |r| r.slot == Slot::Custom && r.name.text() == "Recovery PIN");
    press(&mut app, KeyCode::Backspace);
    typed(&mut app, "8");
    // Renaming the third keeps its kind; it is then taken away.
    editor_to(&mut app, |r| r.slot == Slot::Custom && r.name.text() == "Old");
    ctrl(&mut app, 'x');
    // A new field: the cursor starts in its name, Tab goes to its value.
    ctrl(&mut app, 'n');
    typed(&mut app, "Member since");
    press(&mut app, KeyCode::Tab);
    typed(&mut app, "2019");
    // The question's answer is changed; the apps lose one and gain one.
    editor_to(&mut app, |r| r.slot == Slot::Question);
    ctrl(&mut app, 'u');
    typed(&mut app, "Max");
    editor_to(&mut app, |r| r.slot == Slot::Apps);
    ctrl(&mut app, 'e');
    typed(&mut app, ", org.example.chat");
    ctrl(&mut app, 's');
    assert!(app.overlay.is_none(), "saved");

    let detail = current_detail(&mut app);
    let customs: Vec<(String, String, Option<String>)> =
        detail.custom_fields.iter().map(|c| (c.name.clone(), c.kind.clone(), c.value.clone())).collect();
    assert_eq!(
        customs,
        vec![
            ("Account number".into(), "text".into(), Some("12345-6".into())),
            ("Recovery PIN".into(), "hidden".into(), None),
            ("Member since".into(), "text".into(), Some("2019".into())),
        ]
    );
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert_eq!(ui.engine.reveal(&engine::Secret::Custom(1), &id).expect("pin").as_str(), "9998");
    assert_eq!(ui.engine.reveal(&engine::Secret::Question(0), &id).expect("answer").as_str(), "Max");
    let apps: Vec<&serde_json::Value> = detail.sites.iter().filter(|s| s.get("AndroidApp").is_some()).collect();
    assert_eq!(apps.len(), 2);
    assert_eq!(apps[0]["AndroidApp"]["cert_sha256"], "ab:cd", "a kept app keeps its certificate");
    assert_eq!(apps[1]["AndroidApp"]["package"], "org.example.chat");
    assert!(detail.sites.iter().any(|s| s["Domain"] == "messenger.com"));
}

#[test]
fn a_saved_password_is_edited_not_replaced_and_a_note_opens_with_its_text() {
    let dir = tempfile::tempdir().expect("dir");
    let mut engine = vault(dir.path());
    let note = engine.list("", "title").expect("list").into_iter().find(|r| r.kind == "note").expect("note").id;
    engine.set_secret(&note, "body", b"router: admin / sticker on the back").expect("body");
    let mut app = App::over(engine);
    press(&mut app, KeyCode::Char('e'));
    editor_to(&mut app, |r| r.key == "password");
    let shown = screen(&mut app, 140, 40);
    assert!(shown.contains("saved") && !shown.contains(PASSWORD), "{shown}");
    typed(&mut app, "!");
    ctrl(&mut app, 's');
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let id = ui.current_id().expect("picked");
    let password = ui.engine.reveal(&engine::Secret::Field("password".into()), &id).expect("reveal");
    assert_eq!(password.as_str(), format!("{PASSWORD}!"), "typing adds to what was there");

    pick(&mut app, "Wi-Fi at home");
    press(&mut app, KeyCode::Char('e'));
    let Some(Overlay::Editor(editor)) = &app.overlay else { panic!("no editor") };
    let body = editor.rows.iter().find(|r| r.key == "body").expect("body");
    assert_eq!(body.value.text(), "router: admin / sticker on the back");
    assert!(!editor.dirty(), "loading is not a change");
}

#[test]
fn leaving_the_editor_with_changes_asks_first() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('e'));
    press(&mut app, KeyCode::Esc);
    assert!(app.overlay.is_none(), "nothing changed, nothing to ask");
    press(&mut app, KeyCode::Char('e'));
    typed(&mut app, "!");
    press(&mut app, KeyCode::Esc);
    assert!(screen(&mut app, 140, 40).contains("Discard your changes?"));
    press(&mut app, KeyCode::Char('n'));
    assert!(matches!(app.overlay, Some(Overlay::Editor(_))), "any other key goes back to editing");
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Char('y'));
    assert!(app.overlay.is_none());
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert_eq!(ui.current().expect("row").title, "GitHub");
}

#[test]
fn settings_has_a_tab_of_its_own() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('5'));
    let shown = screen(&mut app, 140, 40);
    for words in ["Settings", "Theme", "Lock after", "Clear the clipboard after", "Vault name", "Empty the trash after", "Import from another app", "THIS VAULT"] {
        assert!(shown.contains(words), "{words}\n{shown}");
    }
    assert!(screen(&mut app, 80, 30).contains("Lock after"), "narrow too");

    let to = |app: &mut App, label: &str| {
        let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
        ui.settings_at = crate::settings::all().iter().position(|s| s.label() == label).expect("setting");
    };
    to(&mut app, "Lock after");
    assert_eq!(app.prefs.lock_minutes, 5);
    press(&mut app, KeyCode::Right);
    assert_eq!(app.prefs.lock_minutes, 10);
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Left);
    assert_eq!(app.prefs.lock_minutes, 2);

    to(&mut app, "Empty the trash after");
    press(&mut app, KeyCode::Enter);
    ctrl(&mut app, 'u');
    typed(&mut app, "45");
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert_eq!(ui.engine.info().expect("info").trash_days, 45);

    to(&mut app, "Vault name");
    press(&mut app, KeyCode::Enter);
    ctrl(&mut app, 'u');
    typed(&mut app, "Family");
    press(&mut app, KeyCode::Enter);
    assert!(screen(&mut app, 140, 40).contains("Family"));

    to(&mut app, "Settings follow the vault");
    let before = app.prefs.sync_settings;
    press(&mut app, KeyCode::Enter);
    assert_ne!(app.prefs.sync_settings, before);
}

#[test]
fn the_vault_password_is_changed_from_settings() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('5'));
    {
        let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
        ui.settings_at = crate::settings::all().iter().position(|s| *s == crate::settings::Setting::Password).expect("setting");
    }
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "not-it");
    press(&mut app, KeyCode::Enter);
    assert!(app.overlay.is_none(), "a wrong password ends it");
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "master-pw");
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "a brand new pass");
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "a brand new pass");
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert!(ui.engine.verify(b"a brand new pass").expect("verify"));
    assert!(!ui.engine.verify(b"master-pw").expect("verify"));
    let path = ui.engine.path().to_owned();
    ctrl(&mut app, 'l');
    assert!(matches!(engine::unlock(&path, b"a brand new pass", None, 3), Ok(engine::Unlock::Opened(..))));
}

#[test]
fn the_editor_files_colors_and_favors_an_item() {
    use crate::editor::Slot;
    let dir = tempfile::tempdir().expect("dir");
    let mut engine = vault(dir.path());
    let work = engine.add_folder("Work").expect("folder");
    let mut app = App::over(engine);
    pick(&mut app, "Wi-Fi at home");
    press(&mut app, KeyCode::Char('e'));
    editor_to(&mut app, |r| r.slot == Slot::Folder);
    press(&mut app, KeyCode::Right);
    editor_to(&mut app, |r| r.slot == Slot::Color);
    press(&mut app, KeyCode::Right);
    editor_to(&mut app, |r| r.slot == Slot::Favourite);
    press(&mut app, KeyCode::Char(' '));
    ctrl(&mut app, 's');
    let detail = current_detail(&mut app);
    assert_eq!(detail.row.folder.as_deref(), Some(work.as_str()));
    assert_eq!(detail.row.color.as_deref(), Some(crate::app::HUES[0].0));
    assert!(detail.row.favourite);

    // Opened again, it shows what it was saved with, and nothing counts as changed.
    press(&mut app, KeyCode::Char('e'));
    let Some(Overlay::Editor(editor)) = &app.overlay else { panic!("no editor") };
    assert_eq!(editor.rows.iter().find(|r| r.slot == Slot::Folder).expect("folder").value.text(), "Work");
    assert!(!editor.dirty());
}

#[test]
fn a_database_engine_is_offered_as_it_is_typed() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('n'));
    let Some(Overlay::Pick { items, at, .. }) = &mut app.overlay else { panic!("no pick") };
    *at = items.iter().position(|i| i.0 == "database").expect("database");
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "Main db");
    press(&mut app, KeyCode::Tab);
    typed(&mut app, "Postg");
    assert!(screen(&mut app, 140, 40).contains("reSQL"), "the rest is offered");
    press(&mut app, KeyCode::Right);
    let Some(Overlay::Editor(editor)) = &app.overlay else { panic!("no editor") };
    assert_eq!(editor.rows[1].value.text(), "PostgreSQL");
}

#[test]
fn linked_items_show_and_open() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let note = {
        let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
        let github = ui.current_id().expect("picked");
        let note = ui.rows.iter().find(|r| r.kind == "note").expect("note").id.clone();
        ui.engine.set_common(&github, json!({"links": [note]})).expect("link");
        ui.reload();
        ui.load_detail();
        note
    };
    let shown = screen(&mut app, 140, 40);
    assert!(shown.contains("LINKED"), "{shown}");
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    ui.pane = crate::vault::Pane::Detail;
    ui.line = ui.lines.iter().position(|l| matches!(l, crate::vault::Line::Link(_))).expect("link line");
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert_eq!(ui.current_id().as_deref(), Some(note.as_str()));
}

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
    for tab in ['1', '2', '3', '4'] {
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
    press(&mut app, KeyCode::Char('1'));
    press(&mut app, KeyCode::Char('e'));
    std::fs::write(out.join("editor.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Esc);
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

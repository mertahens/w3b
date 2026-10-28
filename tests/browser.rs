//! End-to-end behaviour of the browser logic: keys, cursor, links, history,
//! search and the status line. Driven through FakeFetcher, so no network.
mod common;

use common::{char_key, ctrl, fake_app, fixture, key, press, type_str};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use w3b::app::Mode;
use w3b::error::NetError;
use w3b::FakeFetcher;

const HOME: &str = "https://test.local/home";

fn home_fetcher() -> FakeFetcher {
    FakeFetcher::new()
        .with_html(HOME, 200, &fixture("article.html"))
        .with_html("https://test.local/second", 200, "<h1>Second</h1><p>other page</p>")
        .with_html("https://test.local/gone", 404, "<h1>Gone</h1><p>no page here</p>")
        .with_error("https://test.local/dns", NetError::Dns, "no such host")
}

#[test]
fn start_page_renders_and_status_is_200() {
    let app = fake_app(home_fetcher(), HOME, 70, 24);
    assert_eq!(app.status().text(), "200 OK");
    assert_eq!(app.title(), "Design notes - w3b");
    assert_eq!(app.url(), HOME);
    assert_eq!(app.percent(), 0);
    assert!(app.rows().len() > 5, "the fixture should produce several rows");
    assert!(!app.is_loading());
}

#[test]
fn cursor_moves_and_the_page_scrolls_under_it() {
    let mut app = fake_app(home_fetcher(), HOME, 30, 8);
    assert_eq!(app.view.cursor.row, 0);
    for _ in 0..30 {
        app.handle_key(key(KeyCode::Char('j')));
    }
    assert_eq!(app.view.cursor.row, app.rows().len() - 1, "cursor clamps to the last row");
    assert_eq!(app.percent(), 100);
    assert!(app.view.scroll > 0, "the view must have scrolled to follow the cursor");

    for _ in 0..40 {
        app.handle_key(key(KeyCode::Char('k')));
    }
    assert_eq!(app.view.cursor.row, 0);
    assert_eq!(app.view.scroll, 0);
    assert_eq!(app.percent(), 0);
}

#[test]
fn gg_and_capital_g_jump_to_the_ends() {
    let mut app = fake_app(home_fetcher(), HOME, 40, 10);
    app.handle_key(key(KeyCode::Char('g')));
    assert!(app.pending_g, "first g waits for a second one");
    app.handle_key(key(KeyCode::Char('g')));
    assert_eq!(app.view.cursor.row, 0);
    app.handle_key(key(KeyCode::Char('G')));
    assert_eq!(app.view.cursor.row, app.rows().len() - 1);
}

#[test]
fn the_g_menu_waits_for_a_command_and_can_be_cancelled() {
    let mut app = fake_app(home_fetcher(), HOME, 40, 10);
    press(&mut app, KeyCode::Char('g'));
    assert!(app.goto_menu_open(), "a lone g opens the menu");
    assert!(app.message.as_ref().expect("hint message").text.contains('g'));
    press(&mut app, KeyCode::Esc);
    assert!(!app.goto_menu_open(), "Escape closes the menu");
    assert!(app.message.is_none(), "cancelling leaves no message behind");
}

#[test]
fn ge_goes_to_the_end_of_the_page() {
    let mut app = fake_app(home_fetcher(), HOME, 40, 10);
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('e'));
    assert_eq!(app.view.cursor.row, app.rows().len() - 1);
    assert!(!app.goto_menu_open(), "the menu closes when the command runs");
}

#[test]
fn gu_lets_you_type_a_url_in_the_status_line() {
    let mut app = fake_app(home_fetcher(), HOME, 40, 10);
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('u'));
    assert_eq!(app.mode, Mode::Open, "the url prompt is open");
    type_str(&mut app, "https://test.local/second");
    press(&mut app, KeyCode::Enter);
    app.settle();
    assert_eq!(app.page.url, "https://test.local/second");
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn gh_opens_the_home_page() {
    let mut app = fake_app(home_fetcher(), "https://test.local/second", 40, 10);
    app.config.behavior.home_url = HOME.to_string();
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('h'));
    app.settle();
    assert_eq!(app.page.url, HOME);
}

#[test]
fn an_unknown_command_after_g_says_so_and_closes_the_menu() {
    let mut app = fake_app(home_fetcher(), HOME, 40, 10);
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('x'));
    let message = app.message.clone().expect("a message about gx");
    assert!(message.error, "an unknown command is an error");
    assert!(message.text.contains("gx"), "{}", message.text);
    assert!(!app.goto_menu_open());
    // The key is not passed through to normal handling either.
    assert_eq!(app.view.cursor.row, 0);
}

#[test]
fn tab_jumps_between_links_and_enter_follows_one() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    // The first link in the fixture points at /second.
    app.handle_key(key(KeyCode::Tab));
    let (row, col) = {
        let p = app.page.layout.links.first().expect("fixture has links");
        (p.row, p.start)
    };
    assert_eq!(app.view.cursor.row, row, "Tab lands on the first link");
    assert!(app.page.link_at(row, col).is_some());

    app.handle_key(key(KeyCode::Enter));
    // Navigation is asynchronous; wait for it the way the event loop would.
    app.settle();
    assert_eq!(app.url(), "https://test.local/second");
    // the /second fixture has no <title>, so the last path segment is used
    assert_eq!(app.title(), "second");
    assert_eq!(app.status().text(), "200 OK");
    assert!(app.plain_text().contains("other page"));
}

#[test]
fn history_back_and_forward() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    app.open("https://test.local/second");
    assert_eq!(app.url(), "https://test.local/second");
    assert_eq!(app.history.len(), 2);

    app.handle_key(key(KeyCode::Backspace));
    app.settle();
    assert_eq!(app.url(), HOME, "Backspace goes back");
    app.handle_key(key(KeyCode::Char(']')));
    app.settle();
    assert_eq!(app.url(), "https://test.local/second", "']' goes forward");
    app.handle_key(key(KeyCode::Char(']')));
    assert_eq!(
        app.message.as_ref().map(|m| m.text.as_str()),
        Some("no next page"),
        "walking past the end says so"
    );
}

#[test]
fn missing_page_shows_404_and_keeps_the_error_body() {
    let app = fake_app(home_fetcher(), "https://test.local/gone", 70, 24);
    assert_eq!(app.status().text(), "404 NOT FOUND");
    assert_eq!(app.status().kind(), w3b::status::StatusKind::ClientError);
    assert!(app.plain_text().contains("no page here"), "error page bodies are rendered");
}

#[test]
fn network_failure_becomes_an_error_page() {
    let app = fake_app(home_fetcher(), "https://test.local/dns", 70, 24);
    assert_eq!(app.status().text(), "ERROR DNS");
    let text = app.plain_text();
    assert!(text.contains("https://test.local/dns"), "{text}");
    assert!(text.contains("no such host"), "{text}");
}

#[test]
fn opening_a_url_by_typing_it_searches_when_it_is_not_a_url() {
    let mut app = fake_app(
        home_fetcher().with_html("https://duckduckgo.com/?q=rust+browser", 200, "<h1>results</h1>"),
        HOME,
        70,
        24,
    );
    app.handle_key(key(KeyCode::Char(':')));
    assert!(matches!(app.mode, Mode::Open));
    type_str(&mut app, "rust browser");
    app.handle_key(key(KeyCode::Enter));
    app.settle();
    assert_eq!(app.url(), "https://duckduckgo.com/?q=rust+browser");
    assert_eq!(app.status().text(), "200 OK");
}

#[test]
fn search_highlights_and_n_walks_the_hits() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    app.handle_key(key(KeyCode::Char('/')));
    assert!(matches!(app.mode, Mode::Search));
    type_str(&mut app, "item");
    app.handle_key(key(KeyCode::Enter));
    assert!(!app.matches.is_empty(), "the fixture contains 'item'");
    let first = app.view.cursor.row;
    app.handle_key(key(KeyCode::Char('n')));
    assert_ne!(app.view.cursor.row, first, "n jumps to the next hit");
    let before = app.view.cursor.row;
    app.handle_key(key(KeyCode::Char('N')));
    assert!(app.view.cursor.row < before || app.view.cursor.row == first, "N goes back or wraps");
}

#[test]
fn searching_for_something_absent_reports_it() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    app.handle_key(key(KeyCode::Char('/')));
    type_str(&mut app, "zzz-nothing");
    app.handle_key(key(KeyCode::Enter));
    let msg = app.message.as_ref().map(|m| m.text.clone()).unwrap_or_default();
    assert!(msg.contains("no match"), "{msg}");
    assert!(app.message.as_ref().map(|m| m.error) == Some(true));
}

#[test]
fn wrap_toggle_relayouts_without_refetching() {
    let mut app = fake_app(home_fetcher(), HOME, 30, 24);
    let wrapped_rows = app.rows().len();
    app.handle_key(key(KeyCode::Char('w')));
    assert!(!app.config.behavior.wrap);
    let unwrapped_rows = app.rows().len();
    assert!(unwrapped_rows < wrapped_rows, "unwrapped text needs fewer rows");
    assert!(app.page.layout.width > 30, "long rows are kept instead of being cut");

    // Walk to the first row that does not fit, then scroll sideways with the cursor.
    for _ in 0..30 {
        if app.row_width(app.view.cursor.row) > app.viewport.width {
            break;
        }
        app.handle_key(key(KeyCode::Char('j')));
    }
    let wide = app.row_width(app.view.cursor.row);
    assert!(wide > app.viewport.width, "found a row wider than the terminal");
    for _ in 0..wide {
        app.handle_key(key(KeyCode::Char('l')));
    }
    assert!(app.view.cursor.col >= app.viewport.width, "the cursor walked past the right edge");
    assert!(app.view.hscroll > 0, "the view panned sideways to follow the cursor");
    assert!(app.view.hscroll + app.viewport.width > app.view.cursor.col, "the cursor stayed visible");
    for _ in 0..wide {
        app.handle_key(key(KeyCode::Char('h')));
    }
    assert_eq!(app.view.cursor.col, 0);
    assert_eq!(app.view.hscroll, 0, "scrolling back to the left returns to the line start");
}

#[test]
fn resize_relayouts_the_same_document() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let before = app.rows().len();
    app.resize(20, 24);
    assert!(app.rows().len() > before, "narrower terminal, more rows");
    assert_eq!(app.viewport.width, 20);
    assert_eq!(app.url(), HOME, "no refetch on resize");
}

#[test]
fn q_quits_and_esc_leaves_help() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    app.handle_key(key(KeyCode::Char('i')));
    assert!(matches!(app.mode, Mode::Help));
    app.handle_key(key(KeyCode::Esc));
    assert!(matches!(app.mode, Mode::Normal));
    let action = app.handle_key(key(KeyCode::Char('q')));
    assert_eq!(action, w3b::Action::Quit);
    assert!(app.quit);
}

#[test]
fn key_release_events_are_ignored() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let mut ev = KeyEvent::new(KeyCode::Char('G'), ratatui::crossterm::event::KeyModifiers::NONE);
    ev.kind = KeyEventKind::Release;
    app.handle_key(ev);
    assert_eq!(app.view.cursor.row, 0, "a release event must not move the cursor");
}

#[test]
fn y_shows_the_target_under_the_cursor() {
    use w3b::page::Follow;
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let index = app.page.links.iter().position(|l| matches!(l.follow, Follow::Url(_))).expect("fixture link");
    let (row, start) = {
        let p = app.page.layout.links.iter().find(|p| p.link == index).expect("placed link");
        (p.row, p.start)
    };
    app.view.cursor.row = row;
    app.view.cursor.col = start;
    app.handle_key(key(KeyCode::Char('y')));
    let Follow::Url(target) = &app.page.links[index].follow else { unreachable!() };
    assert_eq!(app.message.as_ref().unwrap().text, format!("yanked: {target}"));
}

#[test]
fn visiting_a_link_marks_it_visited() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    assert!(app.visited.contains(HOME));
    app.open("https://test.local/second");
    assert!(app.visited.contains("https://test.local/second"));
}

#[test]
fn control_keys_scroll_by_pages() {
    let mut app = fake_app(home_fetcher(), HOME, 40, 10);
    app.handle_key(KeyEvent::new(KeyCode::Char('d'), ratatui::crossterm::event::KeyModifiers::CONTROL));
    let half = app.view.cursor.row;
    assert!(half > 0 && half < app.rows().len() - 1);
    app.handle_key(key(KeyCode::Char('G')));
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), ratatui::crossterm::event::KeyModifiers::CONTROL));
    assert!(app.view.cursor.row < app.rows().len() - 1);
}

#[test]
fn mouse_click_follows_a_link() {
    use ratatui::crossterm::event::{MouseEvent, MouseEventKind};
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let p = *app.page.layout.links.first().unwrap();
    // The screen keeps `page_padding_x` blank columns on the left, so a layout
    // column is that many columns further right on screen.
    let ev = MouseEvent {
        kind: MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left),
        column: (p.start + app.padding_x()) as u16,
        row: p.row as u16,
        modifiers: KeyModifiers::NONE,
    };
    app.handle_mouse(ev);
    app.settle();
    assert_eq!(app.url(), "https://test.local/second");
}

#[test]
fn one_wheel_notch_moves_one_line() {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let wheel = |kind| MouseEvent { kind, column: 4, row: 4, modifiers: KeyModifiers::NONE };
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    app.handle_mouse(wheel(MouseEventKind::ScrollDown));
    assert_eq!(app.view.cursor.row, 1, "one notch, one line");
    app.handle_mouse(wheel(MouseEventKind::ScrollDown));
    assert_eq!(app.view.cursor.row, 2);
    app.handle_mouse(wheel(MouseEventKind::ScrollUp));
    assert_eq!(app.view.cursor.row, 1, "and back up one line");
    // A click still jumps to the row that was clicked, wheel included.
    app.handle_mouse(wheel(MouseEventKind::Down(MouseButton::Left)));
    assert_eq!(app.view.cursor.row, 4, "a click moves where it was clicked");
    app.handle_mouse(wheel(MouseEventKind::ScrollUp));
    assert_eq!(app.view.cursor.row, 3);
}

#[test]
fn unknown_urls_in_the_fake_fetcher_report_bad_url() {
    let app = fake_app(FakeFetcher::new(), "https://nowhere.local/", 70, 24);
    assert!(app.status().text().starts_with("ERROR"), "{}", app.status().text());
    assert!(app.plain_text().contains("https://nowhere.local/"));
}

#[test]
fn anchor_links_jump_within_the_page() {
    let html = "<html><head><title>Anchors</title></head><body>\
         <p><a href=\"#tips\">jump to tips</a> or <a href=\"#nope\">nowhere</a></p>\
         <p>filler one</p><p>filler two</p><p>filler three</p>\
         <h2 id=\"tips\">Tips</h2><p>tip content</p></body></html>";
    let fetcher = FakeFetcher::new().with_html("https://test.local/anchors", 200, html);
    let mut app = fake_app(fetcher, "https://test.local/anchors", 60, 16);

    // The cursor opens on the first link; standing on it and pressing Enter is
    // all a same-document jump needs.
    let p = app.page.layout.links[0];
    app.view.cursor.row = p.row;
    app.view.cursor.col = p.start;
    let start = app.view.cursor.row;
    press(&mut app, KeyCode::Enter);

    let landed = app.view.cursor.row;
    assert!(landed > start, "the cursor should move down to the anchored block");
    let row_text: String =
        app.page.layout.rows[landed].runs.iter().map(|r| r.text.as_str()).collect::<Vec<_>>().concat();
    assert!(row_text.contains("Tips"), "landed on {row_text:?}");
    assert_eq!(app.message.as_ref().map(|m| m.text.as_str()), Some("anchor #tips"));
    assert!(!app.is_loading(), "a same-document jump must not fetch");
    assert_eq!(app.url(), "https://test.local/anchors", "the url does not change");

    // The cursor is below both links now, so walk backwards to the second one.
    press(&mut app, KeyCode::BackTab); // the #nope link
    let before = app.view.cursor.row;
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.message.as_ref().map(|m| m.text.as_str()), Some("no #nope on this page"));
    assert_eq!(app.message.as_ref().map(|m| m.error), Some(true));
    assert_eq!(app.view.cursor.row, before, "a missing anchor leaves the cursor alone");
}

// --- typing into form fields ------------------------------------------------

const FORM: &str = "https://test.local/form";

fn form_app(html: &str) -> w3b::app::App {
    fake_app(FakeFetcher::new().with_html(FORM, 200, html), FORM, 60, 16)
}

fn row_text(app: &w3b::app::App, row: usize) -> String {
    app.rows()[row].runs.iter().map(|r| r.text.as_str()).collect::<String>()
}

fn page_text(app: &w3b::app::App) -> String {
    (0..app.rows().len()).map(|i| row_text(app, i)).collect::<Vec<_>>().join("\n")
}

/// Park the cursor on the last cell of a field, the way a user would.
fn point_at_field(app: &mut w3b::app::App, index: usize) {
    let (row, end) = app.page.layout.field_end(index).expect("the field is on screen");
    app.view.cursor.row = row;
    app.view.cursor.col = end - 1;
}

#[test]
fn enter_opens_the_field_under_the_cursor_for_typing() {
    let mut app = form_app("<h1>Search</h1><p>q <input name=q value=ann></p>");
    point_at_field(&mut app, 0);
    assert_eq!(app.field_under_cursor(), Some(0));

    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode, Mode::Field);
    assert_eq!(app.editing, Some(0));
    assert!(app.message.as_ref().expect("a note").text.contains("typing in q"));

    type_str(&mut app, "nab");
    assert_eq!(app.page.field_value(0), Some("annnab"));
    assert!(page_text(&app).contains("[ annnab ]"), "{}", page_text(&app));

    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.editing, None);
    assert_eq!(app.page.field_value(0), Some("annnab"), "Enter keeps what was typed");
}

#[test]
fn editing_a_field_is_append_only_and_leaves_the_page_alone() {
    let mut app = form_app("<p>one</p><p>two</p><p>three</p><p>q <input></p>");
    point_at_field(&mut app, 0);
    let scroll = app.view.scroll;

    press(&mut app, KeyCode::Enter);
    // j and k type instead of scrolling while a field has the caret.
    type_str(&mut app, "jk");
    assert_eq!(app.page.field_value(0), Some("jk"));
    assert_eq!(app.view.scroll, scroll, "the page did not scroll");

    press(&mut app, KeyCode::Backspace);
    assert_eq!(app.page.field_value(0), Some("j"));
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.page.field_value(0), Some("j"));
}

#[test]
fn a_password_field_shows_dots_but_keeps_its_value() {
    let mut app = form_app("<p><input type=password></p>");
    point_at_field(&mut app, 0);
    press(&mut app, KeyCode::Enter);
    type_str(&mut app, "s3cr3t");
    assert_eq!(app.page.field_value(0), Some("s3cr3t"));
    let dots = "\u{2022}".repeat(6);
    assert!(page_text(&app).contains(&format!("[ {dots} ]")), "{}", page_text(&app));
    assert!(!page_text(&app).contains("s3cr3t"), "the password is not echoed");
    app.handle_key(ctrl(KeyCode::Char('u')));
    assert_eq!(app.page.field_value(0), Some(""));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn a_field_being_edited_is_left_behind_when_the_page_changes() {
    let mut app = fake_app(
        FakeFetcher::new().with_html(FORM, 200, "<p><a href=\"/second\">next</a> <input></p>").with_html(
            "https://test.local/second",
            200,
            "<h1>Second</h1><p>other page</p>",
        ),
        FORM,
        60,
        16,
    );
    point_at_field(&mut app, 0);
    press(&mut app, KeyCode::Enter);
    type_str(&mut app, "half typed");
    assert_eq!(app.mode, Mode::Field);

    // Any navigation drops the edit: values belong to the page they came from.
    app.navigate("https://test.local/second");
    app.settle();
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.editing, None);
    assert!(app.page.doc.fields.is_empty(), "the new page has no fields");
}

#[test]
fn y_without_a_link_yanks_the_page_url() {
    let mut app = form_app("<h1>Plain</h1><p>nothing to click here</p>");
    app.view.cursor.row = 1;
    app.view.cursor.col = 0;
    assert_eq!(app.link_under_cursor(), None);
    let action = press(&mut app, KeyCode::Char('y'));
    assert_eq!(action, w3b::Action::Yank(FORM.to_string()));
    assert_eq!(app.message.as_ref().expect("a note").text, format!("yanked: {FORM}"));
}

#[test]
fn a_loading_page_shows_a_spinner_that_turns() {
    let mut app = fake_app(
        FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")).with_html(
            "https://test.local/slow",
            200,
            "<h1>Slow</h1><p>later</p>",
        ),
        HOME,
        70,
        24,
    );
    app.navigate("https://test.local/slow");
    assert!(app.is_loading());
    let first = app.spinner_glyph();
    app.tick();
    assert_ne!(first, app.spinner_glyph(), "the animation advances");
    assert_eq!(app.load_time_text().as_deref(), Some("\u{2026}"), "the clock is still running");
    app.settle();
    assert!(!app.is_loading());
    let took = app.load_time_text().expect("a finished fetch reports its time");
    assert!(took.ends_with("ms") || took.ends_with('s'), "{took}");
}

#[test]
fn g_o_and_g_i_walk_the_history() {
    let fetcher = FakeFetcher::new()
        .with_html(HOME, 200, "<h1>Home</h1><p><a href=\"/second\">onward</a></p>")
        .with_html("https://test.local/second", 200, "<h1>Second</h1><p>other page</p>");
    let mut app = fake_app(fetcher, HOME, 60, 16);
    assert_eq!(app.url(), HOME);

    // Out: `Tab` to the link, Enter to follow it, then `g o` to come back.
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Enter);
    app.settle();
    assert_eq!(app.url(), "https://test.local/second");

    press(&mut app, KeyCode::Char('g'));
    assert!(app.goto_menu_open());
    press(&mut app, KeyCode::Char('o'));
    app.settle();
    assert_eq!(app.url(), HOME, "g o went back");

    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('i'));
    app.settle();
    assert_eq!(app.url(), "https://test.local/second", "g i went forward");

    // At the far end of history `g i` says so rather than doing something odd.
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('i'));
    assert_eq!(app.message.as_ref().expect("a note").text, "no next page");
    assert!(!app.goto_menu_open(), "the menu closed either way");
}

#[test]
fn tab_walks_links_and_fields_in_reading_order() {
    let html =
        "<p><a href=\"/first\">first link</a> then <input name=one> then <a href=\"/last\">last link</a></p>";
    let mut app = form_app(html);
    assert_eq!(app.page.doc.fields.len(), 1);
    assert_eq!(app.link_under_cursor(), Some(0), "the cursor opens on the first link");

    press(&mut app, KeyCode::Tab);
    assert_eq!(app.field_under_cursor(), Some(0), "tab reached the field");
    let field_spot = (app.view.cursor.row, app.view.cursor.col);

    press(&mut app, KeyCode::Tab);
    assert_eq!(app.link_under_cursor(), Some(1), "tab reached the last link");
    assert!(field_spot < (app.view.cursor.row, app.view.cursor.col), "walking forward");

    // Past the last item there is nowhere to go, and it says so.
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.message.as_ref().expect("a note").text, "no more links or fields");

    // Back over the field, and Enter starts typing in it.
    press(&mut app, KeyCode::BackTab);
    assert_eq!(app.field_under_cursor(), Some(0), "shift-tab came back to the field");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode, Mode::Field);
    type_str(&mut app, "typed");
    assert_eq!(app.page.field_value(0), Some("typed"));
    press(&mut app, KeyCode::Esc);

    // Walking off the front reports it too.
    let mut back = form_app(html);
    back.view.cursor.row = field_spot.0;
    back.view.cursor.col = field_spot.1 - 1;
    press(&mut back, KeyCode::BackTab);
    assert_eq!(back.link_under_cursor(), Some(0));
    press(&mut back, KeyCode::BackTab);
    assert_eq!(back.message.as_ref().expect("a note").text, "no more links or fields");
}

#[test]
fn load_times_are_formatted_by_size() {
    let mut app = form_app("<h1>Timed</h1>");
    for (ms, want) in [(0, "0 ms"), (999, "999 ms"), (1000, "1.0s"), (1500, "1.5s"), (60_000, "60.0s")] {
        app.page.load_ms = Some(ms);
        assert_eq!(app.load_time_text().as_deref(), Some(want), "{ms} ms");
    }
    app.page.load_ms = None;
    assert_eq!(app.load_time_text(), None, "a page w3b wrote itself has no time");
}

#[test]
fn ctrl_c_quits_from_a_field_but_q_only_types() {
    let mut app = form_app("<p><input name=\"n\"></p><p><a href=\"/x\">x</a></p>");
    // The cursor opens on the field, so Enter starts typing straight away.
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode, Mode::Field);
    app.handle_key(char_key('q'));
    assert!(!app.quit, "q is just a letter while typing");
    assert_eq!(app.field_value(app.editing.expect("the field")), "q");
    app.handle_key(ctrl(KeyCode::Char('c')));
    assert!(app.quit, "Ctrl-c works in every mode");
}

#[test]
fn the_cursor_keeps_its_column_past_the_end_of_a_short_line() {
    let html = "<p>short</p><p>a much longer line of text that goes on for a while</p>";
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 40, 24);
    let short = app.row_width(0);
    assert!(short < 10, "the first row should be short: {short}");

    for _ in 0..(short + 10) {
        app.handle_key(key(KeyCode::Char('l')));
    }
    assert!(app.view.cursor.col > short, "the cursor walked into the blank right of the text");
    assert!(app.view.cursor.col <= app.content_width(), "and stayed on the page");

    let col = app.view.cursor.col;
    app.handle_key(key(KeyCode::Char('j')));
    assert_eq!(app.view.cursor.col, col, "moving down kept the wanted column");
    app.handle_key(key(KeyCode::Char('k')));
    assert_eq!(app.view.cursor.col, col, "and moving back up did too");

    for _ in 0..80 {
        app.handle_key(key(KeyCode::Char('l')));
    }
    assert_eq!(app.view.cursor.col, app.content_width(), "the far edge of the page is the limit");
}

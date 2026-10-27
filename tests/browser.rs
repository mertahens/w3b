//! End-to-end behaviour of the browser logic: keys, cursor, links, history,
//! search and the status line. Driven through FakeFetcher, so no network.
mod common;

use common::{fake_app, fixture, key, press, type_str};
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
    let ev =
        MouseEvent { kind: MouseEventKind::ScrollDown, column: 0, row: 0, modifiers: KeyModifiers::NONE };
    app.handle_mouse(ev);
    assert_eq!(app.view.cursor.row, 3, "scroll wheel moves three lines");

    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
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

    let start = app.view.cursor.row;
    press(&mut app, KeyCode::Tab); // the #tips link
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

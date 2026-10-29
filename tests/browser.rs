//! End-to-end behaviour of the browser logic: keys, cursor, links, history,
//! search and the status line. Driven through FakeFetcher, so no network.
mod common;

use common::{
    cell, cell_of, char_key, click, ctrl, drag, fake_app, fixture, key, mouse, page_cell, press, type_str,
};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
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
fn a_home_address_written_without_a_scheme_is_read_as_an_address() {
    // The config is written by hand, so `home_url = "test.local/home"` happens.
    let mut app = fake_app(home_fetcher(), "https://test.local/second", 40, 10);
    app.config.behavior.home_url = "test.local/home".into();
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('h'));
    app.settle();
    assert_eq!(app.page.url, HOME);
}

#[test]
fn a_home_address_that_names_nothing_is_a_message_and_not_a_panic() {
    let mut app = fake_app(home_fetcher(), HOME, 40, 10);
    app.config.behavior.home_url = String::new();
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('h'));
    let message = app.message.clone().expect("a message about the empty home address");
    assert!(message.error, "{}", message.text);
    assert_eq!(app.page.url, HOME, "the page being read is left alone");
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
    app.handle_key(key(KeyCode::Char('?')));
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
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let p = *app.page.layout.links.first().unwrap();
    // The screen keeps `page_padding_x` blank columns on the left, so a layout
    // column is that many columns further right on screen.
    let at = cell(&app, p.row, p.start);
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at.1, at.0));
    assert_eq!(app.url(), HOME, "the press waits: only the release can tell a click from a drag");
    assert_eq!((app.view.cursor.row, app.view.cursor.col), (p.row, p.start), "it still moves the cursor");
    app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), at.1, at.0));
    app.settle();
    assert_eq!(app.url(), "https://test.local/second");
}

#[test]
fn the_wheel_scrolls_the_page_and_not_the_cursor() {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let wheel = |kind| MouseEvent { kind, column: 4, row: 4, modifiers: KeyModifiers::NONE };
    // A tall page in a short window: there is somewhere to scroll to.
    let mut app = fake_app(home_fetcher(), HOME, 70, 10);
    // Park the cursor in the middle of the window, away from both edges.
    for _ in 0..5 {
        app.handle_key(key(KeyCode::Char('j')));
    }
    assert_eq!((app.view.scroll, app.view.cursor.row), (0, 5));

    // One notch, one line of page - and the cursor keeps the line it was on.
    for _ in 0..2 {
        app.handle_mouse(wheel(MouseEventKind::ScrollDown));
    }
    assert_eq!(app.view.scroll, 2, "two notches, two lines of page");
    assert_eq!(app.view.cursor.row, 5, "the cursor does not ride along");

    app.handle_mouse(wheel(MouseEventKind::ScrollUp));
    assert_eq!((app.view.scroll, app.view.cursor.row), (1, 5));

    // Scroll its line past the top of the window and the cursor stops at the
    // margin: from there the highlight holds one screen row while the text runs
    // under it, and what Enter and y act on is always a line that is on screen.
    for _ in 0..5 {
        app.handle_mouse(wheel(MouseEventKind::ScrollDown));
    }
    assert_eq!(app.view.scroll, 6);
    assert_eq!(
        app.view.cursor.row,
        app.view.scroll + app.config.behavior.scroll_margin,
        "parked inside the window's edge, margin included"
    );

    // A click still moves the cursor to the row that was clicked.
    app.handle_mouse(wheel(MouseEventKind::Down(MouseButton::Left)));
    assert_eq!(app.view.cursor.row, 10, "a click moves where it was clicked");

    // However hard the wheel is turned, the cursor stays inside the page.
    for _ in 0..400 {
        app.handle_mouse(wheel(MouseEventKind::ScrollDown));
    }
    assert_eq!(app.view.scroll, app.page.row_count() - app.page_height());
    assert!(app.view.cursor.row >= app.view.scroll);
    assert!(app.view.cursor.row < app.view.scroll + app.page_height());

    // At the top of the page the wheel has nothing left to do.
    let mut top = fake_app(home_fetcher(), HOME, 70, 10);
    for _ in 0..5 {
        top.handle_mouse(wheel(MouseEventKind::ScrollUp));
    }
    assert_eq!((top.view.scroll, top.view.cursor.row), (0, 0), "the page does not scroll off the top");
}

#[test]
fn a_wheel_then_a_key_does_not_drag_the_page_back() {
    use ratatui::crossterm::event::{MouseEvent, MouseEventKind};
    let wheel = |kind| MouseEvent { kind, column: 4, row: 4, modifiers: KeyModifiers::NONE };
    let mut app = fake_app(home_fetcher(), HOME, 70, 10);
    for _ in 0..4 {
        app.handle_mouse(wheel(MouseEventKind::ScrollDown));
    }
    let (scrolled, parked) = (app.view.scroll, app.view.cursor.row);
    assert_eq!(parked, scrolled + app.config.behavior.scroll_margin);

    // The wheel parked the cursor where the keys keep it, so the first `j` after a
    // scroll moves the cursor one line instead of dragging the page back up.
    app.handle_key(key(KeyCode::Char('j')));
    assert_eq!(app.view.cursor.row, parked + 1, "one key, one line");
    assert_eq!(app.view.scroll, scrolled, "the page does not jump back the margin");
}

// --- marking text: `v`, the mouse, and `y` --------------------------------

#[test]
fn dragging_the_mouse_marks_text_and_y_takes_it() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let here = cell_of(&app, "Design notes");
    drag(&mut app, here, (here.0, here.1 + 5));
    assert_eq!(app.mode, Mode::Visual, "a drag leaves the text marked");
    assert_eq!(app.selected_text().as_deref(), Some("Design"));
    assert_eq!(app.visual_status().as_deref(), Some("6 selected"));
    assert_eq!(press(&mut app, KeyCode::Char('y')), w3b::Action::Yank("Design".to_string()));
    assert_eq!(app.message.as_ref().map(|m| m.text.as_str()), Some("yanked 6 characters"));
    assert_eq!(app.mode, Mode::Visual, "yanking does not end the mark");
}

#[test]
fn a_drag_across_a_link_marks_it_instead_of_following_it() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let here = cell_of(&app, "second page");
    drag(&mut app, here, (here.0, here.1 + 5));
    app.settle();
    assert_eq!(app.url(), HOME, "dragging over a link is marking, not following");
    assert_eq!(app.selected_text().as_deref(), Some("second"));
}

#[test]
fn a_click_after_a_drag_drops_the_mark() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let here = cell_of(&app, "Design notes");
    drag(&mut app, here, (here.0, here.1 + 5));
    assert!(app.view.visual.is_some());
    let elsewhere = cell_of(&app, "Code");
    click(&mut app, elsewhere);
    assert!(app.view.visual.is_none(), "a click somewhere else asks for something else");
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn the_wheel_while_marking_keeps_the_mark_and_only_moves_its_far_end() {
    use ratatui::crossterm::event::{MouseEvent, MouseEventKind};
    let wheel = |kind| MouseEvent { kind, column: 4, row: 4, modifiers: KeyModifiers::NONE };
    // A short window: the wheel has somewhere to go and a reason to park the cursor.
    let mut app = fake_app(home_fetcher(), HOME, 70, 10);
    let here = cell_of(&app, "first item");
    drag(&mut app, here, (here.0, here.1 + 9));
    let (marked, text) = (app.view.visual, app.selected_text());

    // The marked line is still on screen, so the wheel moves the page alone.
    app.handle_mouse(wheel(MouseEventKind::ScrollDown));
    assert_eq!(app.view.visual, marked);
    assert_eq!(app.selected_text(), text, "a mark is a place on the page, not on the screen");

    // Turn it far enough that the marked line slides out of the window and the cursor
    // is parked inside it: the mark stretches with the cursor, as every other move
    // does, so `y` still copies from where the button went down to where it stopped.
    for _ in 0..4 {
        app.handle_mouse(wheel(MouseEventKind::ScrollDown));
    }
    assert_eq!(app.view.visual, marked, "the end the button went down on stays put");
    assert_eq!(app.view.cursor.row, app.view.scroll + app.config.behavior.scroll_margin);
    assert!(
        app.selected_text().unwrap().starts_with("first item"),
        "the mark still starts at the marked word"
    );
    assert_eq!(app.mode, Mode::Visual, "scrolling does not end the mark");
}

#[test]
fn v_marks_text_and_the_cursor_keys_move_its_far_end() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let (row, col) = page_cell(&app, "bold");
    app.move_cursor_to((row, col));
    press(&mut app, KeyCode::Char('v'));
    assert_eq!(app.mode, Mode::Visual);
    assert_eq!(
        app.view.visual,
        Some(w3b::app::Anchor { row, col, linewise: false, far: None }),
        "the press is one end and the cursor the other"
    );
    for _ in 0..3 {
        press(&mut app, KeyCode::Right);
    }
    assert_eq!(app.selected_text().as_deref(), Some("bold"), "the cell under the cursor belongs to it");
    assert_eq!(app.visual_status().as_deref(), Some("4 selected"));
    assert_eq!(press(&mut app, KeyCode::Char('y')), w3b::Action::Yank("bold".to_string()));
    assert_eq!(app.message.as_ref().map(|m| m.text.as_str()), Some("yanked 4 characters"));
}

#[test]
fn capital_v_takes_whole_lines() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let (row, _) = page_cell(&app, "first item");
    app.move_cursor_to((row, 0));
    press(&mut app, KeyCode::Char('V'));
    press(&mut app, KeyCode::Down);
    let text = app.selected_text().expect("a mark is open");
    assert_eq!(text.lines().count(), 2, "two rows, end to end");
    assert!(text.contains("first item") && text.contains("second item"), "{text:?}");
    assert_eq!(press(&mut app, KeyCode::Char('y')), w3b::Action::Yank(text));
    assert_eq!(app.message.as_ref().map(|m| m.text.as_str()), Some("yanked 2 lines"));
}

#[test]
fn v_and_capital_v_switch_between_characters_and_whole_lines() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let mark = |app: &w3b::App| match app.view.visual {
        None => "none".to_string(),
        Some(a) => format!(
            "{} {}",
            if a.linewise { "lines" } else { "chars" },
            if a.far.is_some() { "standing" } else { "travelling" }
        ),
    };
    press(&mut app, KeyCode::Char('v'));
    assert_eq!(mark(&app), "chars travelling");
    press(&mut app, KeyCode::Char('V'));
    assert_eq!(mark(&app), "lines travelling", "the same key pair as vim");
    press(&mut app, KeyCode::Char('v'));
    assert_eq!(mark(&app), "chars travelling");
    // The other key of the pair is only ever about what is taken, so it cannot lose
    // the mark: it switches kind and leaves the two ends where they are.
    press(&mut app, KeyCode::Char('V'));
    assert_eq!(mark(&app), "lines travelling");
    press(&mut app, KeyCode::Char('v'));
    assert_eq!(mark(&app), "chars travelling");
    // The same key is the one that stands the mark up and picks it back down.
    press(&mut app, KeyCode::Char('v'));
    assert_eq!(mark(&app), "chars standing");
    press(&mut app, KeyCode::Char('V'));
    assert_eq!(mark(&app), "lines standing", "and the pair still switches kind");
    press(&mut app, KeyCode::Char('v'));
    assert_eq!(mark(&app), "chars standing");
    press(&mut app, KeyCode::Char('v'));
    assert_eq!(mark(&app), "chars travelling", "the same key picks the mark back up");
    press(&mut app, KeyCode::Char('v'));
    assert_eq!(mark(&app), "chars standing");
    press(&mut app, KeyCode::Esc);
    assert_eq!(mark(&app), "none", "Esc is what drops a mark for good");
}

#[test]
fn v_a_second_time_leaves_the_mark_on_the_page() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let (row, col) = page_cell(&app, "bold");
    app.move_cursor_to((row, col));
    press(&mut app, KeyCode::Char('v'));
    for _ in 0..3 {
        press(&mut app, KeyCode::Char('l'));
    }
    assert_eq!(app.selected_text().as_deref(), Some("bold"));

    // The second `v` is done marking: the cursor is free again, and the mark keeps the
    // end it had reached rather than going away with it.
    press(&mut app, KeyCode::Char('v'));
    assert_eq!(app.mode, Mode::Normal, "the cursor is free");
    assert!(app.marked(), "the selection stays there");
    assert_eq!(app.selected_text().as_deref(), Some("bold"), "still the same four cells");
    assert_eq!(app.visual_status().as_deref(), Some("4 selected"));

    // The cursor goes off on its own and the mark does not follow it.
    for _ in 0..5 {
        press(&mut app, KeyCode::Char('j'));
    }
    assert_eq!(app.view.cursor.row, row + 5, "and it really did go");
    assert_eq!(app.selected_text().as_deref(), Some("bold"), "where it was is where it stays");

    // Still the mark, and not the line the cursor has stopped on, that y takes.
    assert_eq!(press(&mut app, KeyCode::Char('y')), w3b::Action::Yank("bold".to_string()));

    // And the keys the mark held while it travelled are released with it: the pane is
    // not a reason to lose it either.
    press(&mut app, KeyCode::Char('?'));
    assert_eq!(app.mode, Mode::Help, "a standing mark does not hold the page keys");
    press(&mut app, KeyCode::Char('x'));
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.selected_text().as_deref(), Some("bold"), "the pane gave it back");

    // `v` picks it up again: the far end is the cursor, wherever the cursor has got to.
    press(&mut app, KeyCode::Char('v'));
    assert_eq!(app.mode, Mode::Visual);
    assert_eq!(app.view.visual.map(|a| a.far), Some(None));
    assert_eq!(app.visual_ends(), Some((row, col, row + 5, app.view.cursor.col)));

    // Esc drops a standing mark as well, which is how y gets back to the cursor.
    press(&mut app, KeyCode::Char('v'));
    press(&mut app, KeyCode::Esc);
    assert!(!app.marked());
    assert_eq!(app.mode, Mode::Normal);
    assert!(matches!(press(&mut app, KeyCode::Char('y')), w3b::Action::Yank(url) if url == HOME));
}

#[test]
fn a_drag_mark_turned_into_lines_takes_the_whole_rows() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let from = cell_of(&app, "second page");
    let to = cell_of(&app, "anchor");
    drag(&mut app, from, to);
    assert_eq!(app.view.visual.map(|a| a.linewise), Some(false), "a drag marks characters");
    assert_eq!(app.selected_text().as_deref().map(|t| t.lines().count()), Some(2));
    press(&mut app, KeyCode::Char('V'));
    let text = app.selected_text().expect("a mark is open");
    assert_eq!(text.lines().count(), 2);
    assert!(
        text.starts_with("Some bold"),
        "rows are taken from their own beginning, not from where the drag began: {text:?}"
    );
    assert!(text.ends_with("anchor."), "and to their end: {text:?}");
}

#[test]
fn a_mark_that_the_cursor_walks_past_is_still_the_same_mark() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let (row, _) = page_cell(&app, "Code");
    app.move_cursor_to((row, 0));
    press(&mut app, KeyCode::Char('v'));
    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Up);
    // The cursor is now above the anchor, and the two ends simply swap places.
    assert_eq!(app.visual_ends(), Some((row - 2, 0, row, 0)));
    let above = app.selected_text().expect("a mark is open");
    assert!(above.ends_with('C') && above.contains("width nicely"), "{above:?}");
    for _ in 0..3 {
        press(&mut app, KeyCode::Down);
    }
    assert_eq!(app.visual_ends(), Some((row, 0, row + 1, 0)), "the far end followed the cursor");
}

#[test]
fn gg_in_a_mark_stretches_it_to_the_top_of_the_page() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    for _ in 0..6 {
        press(&mut app, KeyCode::Char('j'));
    }
    let here = app.view.cursor;
    press(&mut app, KeyCode::Char('v'));
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('g'));
    assert_eq!(app.view.cursor.row, 0, "the jump still works while marking");
    assert_eq!(app.view.visual.map(|a| (a.row, a.col)), Some((here.row, here.col)));
    assert_eq!(app.visual_status().map(|s| s.ends_with("selected")), Some(true));
    // What the mark does not offer while marking is the page-changing half of the menu.
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('r'));
    assert!(!app.is_loading(), "no reload from inside a mark");
    assert_eq!(app.mode, Mode::Visual);
}

#[test]
fn a_mark_silences_the_keys_that_would_leave_the_page() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    press(&mut app, KeyCode::Char('v'));
    for c in ['o', ':', '/', 'n', 'N', 'r', 'i', '?', ']', '['] {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Backspace);
    assert_eq!(app.mode, Mode::Visual, "still marking: no prompt, no pane, no reload");
    assert!(!app.is_loading());
    assert_eq!(app.url(), HOME);
    assert_eq!(app.input, "", "no prompt was opened");
}

#[test]
fn esc_leaves_a_mark_and_leaves_the_cursor_where_it_stopped() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let (row, col) = page_cell(&app, "bold");
    app.move_cursor_to((row, col));
    press(&mut app, KeyCode::Char('v'));
    for _ in 0..2 {
        press(&mut app, KeyCode::Char('l'));
    }
    let here = app.view.cursor;
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.view.visual, None);
    assert_eq!(app.view.cursor, here, "leaving the mark does not move the cursor");
}

#[test]
fn a_blank_mark_says_so_rather_than_yanking_nothing() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let blank = app.rows().iter().position(|r| r.is_blank()).expect("the page has a blank row");
    app.move_cursor_to((blank, 0));
    press(&mut app, KeyCode::Char('v'));
    assert_eq!(press(&mut app, KeyCode::Char('y')), w3b::Action::None, "there is nothing to copy");
    let message = app.message.as_ref().expect("a note about it");
    assert!(message.error, "and it is an error: nothing was copied");
    assert_eq!(message.text, "the selection is blank");
    assert_eq!(app.mode, Mode::Visual, "the mark stays open to be moved");
}

#[test]
fn a_mark_belongs_to_the_page_it_was_made_on() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let here = cell_of(&app, "Design notes");
    drag(&mut app, here, (here.0, here.1 + 3));
    app.open("https://test.local/second");
    assert_eq!(app.url(), "https://test.local/second");
    assert!(app.view.visual.is_none(), "the new page does not inherit a mark");
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn a_re_layout_drops_the_mark() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);
    let here = cell_of(&app, "second item");
    drag(&mut app, here, (here.0, here.1 + 20));
    press(&mut app, KeyCode::Char('w'));
    assert!(app.view.visual.is_none(), "the text moved to other rows and columns");
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.message.as_ref().map(|m| m.text.as_str()), Some("wrap off"));
}

#[test]
fn a_press_lands_after_a_note_rather_than_keeping_it() {
    let mut app = fake_app(home_fetcher(), HOME, 70, 24);

    // Standing on text rather than a link, y takes the page and says so.
    let action = press(&mut app, KeyCode::Char('y'));
    assert!(matches!(action, w3b::Action::Yank(url) if url == HOME));
    assert!(app.message.is_some(), "the note is up");

    // The wheel only moves the page, so the note is left alone.
    app.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, 5));
    assert!(app.message.is_some(), "scrolling is not asking for anything");

    // The left button is: whatever the last action reported is done with, and the
    // status line goes back to what the page is showing.
    let at = cell_of(&app, "Design notes");
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at.1, at.0));
    assert!(app.message.is_none());
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
fn enter_in_a_field_sends_the_get_form_it_belongs_to() {
    let sent = "https://test.local/search?q=annnab&sort=newest";
    let mut app = fake_app(
        FakeFetcher::new()
            .with_html(
                FORM,
                200,
                "<h1>Search</h1><form action=/search><input name=q value=ann><input name=sort value=newest></form>",
            )
            .with_html(sent, 200, "<h1>results for annnab</h1>"),
        FORM,
        60,
        16,
    );
    point_at_field(&mut app, 0);
    press(&mut app, KeyCode::Enter);
    assert!(
        app.message.as_ref().expect("a note").text.contains("Enter submits"),
        "the note says what Enter does"
    );

    type_str(&mut app, "nab");
    press(&mut app, KeyCode::Enter);
    app.settle();
    assert_eq!(app.page.url, sent, "what was typed and what was already there are both in the query");
    assert!(page_text(&app).contains("results for annnab"), "{}", page_text(&app));
    assert_eq!(app.mode, Mode::Normal);

    // The form page is still behind the result, exactly as after a link.
    press(&mut app, KeyCode::Char('['));
    app.settle();
    assert_eq!(app.page.url, FORM, "back returns to the form");
}

#[test]
fn a_form_w3b_will_not_send_is_reported_and_stays_put() {
    let mut app = form_app("<form action=/send method=post><input name=q value=ann></form>");
    point_at_field(&mut app, 0);
    press(&mut app, KeyCode::Enter);
    type_str(&mut app, "typed");
    press(&mut app, KeyCode::Enter);
    app.settle();

    assert_eq!(app.mode, Mode::Normal, "the typing is finished either way");
    assert_eq!(app.page.url, FORM, "nothing was fetched");
    let note = app.message.as_ref().expect("a note").text.clone();
    assert!(note.contains("post") && note.contains("GET"), "{note}");
    assert_eq!(app.page.field_value(0), Some("anntyped"), "what was typed stays on the page");
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

/// Park the cursor on the first image or media label on the page.
fn point_at_media(app: &mut w3b::app::App) {
    let (row, col) = app
        .page
        .layout
        .rows
        .iter()
        .enumerate()
        .find_map(|(row, line)| line.runs.iter().find(|run| run.media.is_some()).map(|run| (row, run.start)))
        .expect("a media label on the page");
    app.view.cursor.row = row;
    app.view.cursor.col = col;
}

#[test]
fn y_on_an_image_copies_where_the_image_came_from() {
    let mut app = form_app("<h1>Plate</h1><p>see <img src=\"/pics/logo.png?v=2\" alt=\"the logo\"></p>");
    point_at_media(&mut app);
    assert_eq!(app.media_under_cursor(), Some(0), "standing on the label");
    let action = press(&mut app, KeyCode::Char('y'));
    let target = "https://test.local/pics/logo.png?v=2";
    assert_eq!(
        action,
        w3b::Action::Yank(target.to_string()),
        "the address, finished off with the page's own"
    );
    assert_eq!(app.message.as_ref().expect("a note").text, format!("yanked: {target}"));
}

#[test]
fn y_on_a_video_or_a_sound_copies_the_clip() {
    let mut app = form_app("<p><video src=\"/talks/talk.mp4\" title=\"the talk\"></video></p>");
    point_at_media(&mut app);
    let action = press(&mut app, KeyCode::Char('y'));
    let target = "https://test.local/talks/talk.mp4";
    assert_eq!(action, w3b::Action::Yank(target.to_string()));
    let shown = page_text(&app);
    assert!(shown.contains("[video: the talk]"), "the clip is labelled for the eye:\n{shown}");
}

#[test]
fn an_image_written_inside_a_link_yanks_the_link_it_was_wrapped_in() {
    let mut app = form_app("<p><a href=\"/full.png\"><img src=\"/thumb.png\" alt=\"t\"></a></p>");
    point_at_media(&mut app);
    assert_eq!(app.link_under_cursor(), Some(0), "the label sits on a link as well");
    let action = press(&mut app, KeyCode::Char('y'));
    assert_eq!(
        action,
        w3b::Action::Yank("https://test.local/full.png".to_string()),
        "what a click would use comes first"
    );
}

#[test]
fn an_image_that_names_no_address_yanks_the_page_as_before() {
    let mut app = form_app("<p><img alt=\"painted by hand\"></p>");
    point_at_media(&mut app);
    assert_eq!(app.media_under_cursor(), Some(0));
    assert_eq!(app.page.media_target(0), None, "the tag names no address");
    let action = press(&mut app, KeyCode::Char('y'));
    assert_eq!(action, w3b::Action::Yank(FORM.to_string()));
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
    assert_eq!(app.message.as_ref().expect("a note").text, "no more links, fields or disclosures");

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
    assert_eq!(back.message.as_ref().expect("a note").text, "no more links, fields or disclosures");
}

/// Which row of the laid-out page holds this text.
fn row_of(app: &w3b::App, needle: &str) -> usize {
    (0..app.rows().len())
        .find(|n| app.rows()[*n].runs.iter().map(|r| r.text.as_str()).collect::<String>().contains(needle))
        .expect("the line is on the page")
}

#[test]
fn enter_opens_a_details_and_closes_it_again() {
    let html = "<p>see <a href=\"/x\">a link</a></p>\
        <details><summary>Anything at all</summary><p>Body text</p></details><p>after</p>";
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 60, 16);
    let closed = app.plain_text();
    assert!(closed.contains("+ Anything at all"), "the summary line is on the page: {closed}");
    assert!(!closed.contains("Body text"), "what it holds is not: {closed}");

    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.disclosure_under_cursor(), Some(0), "tab walked to the summary line");
    let line = row_of(&app, "+ Anything at all");

    press(&mut app, KeyCode::Enter);
    let open = app.plain_text();
    assert!(open.contains("Body text"), "the content went into the lines below: {open}");
    assert!(open.contains("- Anything at all"), "and the marker says it can be closed");
    assert_eq!(app.message.as_ref().expect("a note").text, "opened Anything at all");
    assert_eq!(row_of(&app, "- Anything at all"), line, "the summary keeps its row, so the cursor does too");
    assert_eq!(app.view.cursor.row, line, "and the cursor really did not move");

    press(&mut app, KeyCode::Enter);
    assert_eq!(app.plain_text(), closed, "Enter again took the content away");
    assert_eq!(app.message.as_ref().expect("a note").text, "closed Anything at all");
}

#[test]
fn a_details_written_open_shows_its_content_and_enter_shuts_it() {
    let html = "<details open><summary>Anything</summary><p>Body text</p></details>";
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 40, 12);
    let open = app.plain_text();
    assert!(open.contains("- Anything"), "open says so: {open}");
    assert!(open.contains("Body text"), "and shows what it holds");

    press(&mut app, KeyCode::Tab);
    assert_eq!(app.disclosure_under_cursor(), Some(0));
    press(&mut app, KeyCode::Enter);
    let closed = app.plain_text();
    assert!(!closed.contains("Body text"), "Enter closed it: {closed}");
    assert!(closed.contains("+ Anything"), "and the marker turned back");
}

#[test]
fn a_hidden_disclosure_is_nowhere_to_be_found() {
    // What a closed <details> holds is off the page: not a target for Tab, and not
    // something search can land on.
    let html = "<details><summary>Anything</summary><p><a href=\"/deep\">deep link</a></p></details>";
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 40, 12);
    assert_eq!(
        app.page.links.iter().filter(|l| l.href.contains("/deep")).count(),
        1,
        "it is in the document"
    );
    assert!(app.page.layout.links.is_empty(), "but its link is not on the page");
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.disclosure_under_cursor(), Some(0), "tab reaches the summary instead");

    press(&mut app, KeyCode::Enter);
    assert!(!app.page.layout.links.is_empty(), "opening it puts the link on the page");
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.link_under_cursor().map(|l| app.page.links[l].href.clone()), Some("/deep".to_string()));
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

#[test]
fn a_json_body_can_be_searched_like_any_other_text() {
    const API: &str = "https://test.local/api.json";
    let json = r#"{"results":[{"title":"first one"},{"title":"second one"}],"count":2}"#;
    let mut app = fake_app(FakeFetcher::new().with_type(API, 200, "application/json", json), API, 60, 14);
    app.handle_key(key(KeyCode::Char('/')));
    assert!(matches!(app.mode, Mode::Search));
    type_str(&mut app, "title");
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.matches.len(), 2, "the key appears once in each object: {:?}", app.matches);
    let first = app.view.cursor.row;
    app.handle_key(key(KeyCode::Char('n')));
    assert_ne!(app.view.cursor.row, first, "n walks to the next hit");
}

#[test]
fn a_json_body_the_server_broke_is_still_shown() {
    const API: &str = "https://test.local/api.json";
    let app = fake_app(FakeFetcher::new().with_type(API, 200, "application/json", "{\"a\":1,}"), API, 60, 14);
    let text = page_text(&app);
    assert!(text.contains("{\"a\":1,}"), "the bytes are shown as sent: {text}");
    let notice = app.page.notice.clone().unwrap_or_default();
    assert!(notice.contains("does not parse"), "{notice}");
}

#[test]
fn a_markdown_body_is_a_page_and_not_a_dump_of_its_source() {
    const DOC: &str = "https://test.local/README.md";
    let md = "# Reader\n\nSome *words* with a [link](/next).\n\n* one\n* two\n";
    let app =
        fake_app(FakeFetcher::new().with_type(DOC, 200, "text/markdown; charset=utf-8", md), DOC, 40, 16);
    let text = page_text(&app);
    assert!(text.contains("Reader"), "{text}");
    assert!(text.contains("Some words with a link."), "the markup is gone: {text}");
    assert!(!text.contains('*'), "no asterisk is left on the page: {text}");
    assert!(text.contains('•'), "the list has markers: {text}");
    assert!(!app.page.file_view, "a converted body is a page");
    assert!(app.page.layout.links.iter().any(|_| true), "the link is a link");
}

#[test]
fn a_plain_text_body_at_a_dot_md_address_is_read_as_markdown() {
    // The address a raw file is served from ends in `.md` while the server calls
    // the body `text/plain`, which is what a code host really does.
    const DOC: &str = "https://raw.test.local/repo/main/README.md";
    let md = "# Reader\n\n* one\n* two\n";
    let app = fake_app(FakeFetcher::new().with_type(DOC, 200, "text/plain; charset=utf-8", md), DOC, 40, 16);
    let text = page_text(&app);
    assert!(text.contains('•'), "the bullets were made: {text}");
    assert!(!text.contains("* one"), "the source is not shown: {text}");
    assert!(!app.page.file_view);
}

#[test]
fn a_plain_text_file_stays_the_text_it_is() {
    // A `.txt` is not promoted, so a file that merely mentions markdown syntax
    // keeps showing it.
    const DOC: &str = "https://test.local/notes.txt";
    let md = "# Reader\n\n* one\n* two\n";
    let app = fake_app(FakeFetcher::new().with_type(DOC, 200, "text/plain; charset=utf-8", md), DOC, 40, 16);
    let text = page_text(&app);
    assert!(text.contains("* one"), "the source is shown as written: {text}");
    assert!(!text.contains('•'), "nothing was converted: {text}");
    assert!(app.page.file_view, "a text file is its own text");
}

#[test]
fn a_markdown_table_of_contents_jumps_to_its_heading() {
    const DOC: &str = "https://test.local/guide.md";
    let md = format!(
        "# Guide\n\nStart at [tips](#tips).\n\n{}## Tips\n\ntip content\n",
        "filler one\n\nfiller two\n\n".repeat(4)
    );
    let mut app = fake_app(FakeFetcher::new().with_type(DOC, 200, "text/markdown", &md), DOC, 40, 12);
    let placement = app.page.layout.links[0];
    app.view.cursor.row = placement.row;
    app.view.cursor.col = placement.start;
    let start = app.view.cursor.row;
    press(&mut app, KeyCode::Enter);
    assert!(app.view.cursor.row > start, "the jump goes down the page");
    let row: String = app.page.layout.rows[app.view.cursor.row]
        .runs
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .concat();
    assert!(row.contains("Tips"), "landed on {row:?}");
    assert!(!app.is_loading(), "a jump inside the page fetches nothing");
}

#[test]
fn markdown_can_be_searched_like_any_other_text() {
    const DOC: &str = "https://test.local/CHANGELOG.md";
    let md = "# Notes\n\nAdded a thing.\n\nFixed another thing.\n";
    let mut app = fake_app(FakeFetcher::new().with_type(DOC, 200, "text/markdown", md), DOC, 40, 12);
    app.handle_key(key(KeyCode::Char('/')));
    type_str(&mut app, "thing");
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.matches.len(), 2, "both lines are hits: {:?}", app.matches);
}

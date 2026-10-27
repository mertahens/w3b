//! Rendering tests against ratatui's TestBackend: what actually lands in the
//! terminal buffer, including the status line and the styles.
mod common;

use common::{fake_app, fixture, key, type_str};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::KeyCode;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use w3b::render;
use w3b::FakeFetcher;

const HOME: &str = "https://test.local/home";

/// Draw the whole UI and return the terminal buffer plus the terminal itself.
fn draw(app: &w3b::App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal.draw(|f| render::draw_all(app, f.area(), f.buffer_mut())).expect("draw");
    buffer_text(terminal.backend().buffer())
}

fn buffer_text(buf: &Buffer) -> String {
    let area = buf.area;
    let mut out = String::new();
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            line.push_str(buf.cell((x, y)).expect("cell").symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

#[test]
fn full_screen_snapshot() {
    let app = fake_app(FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")), HOME, 62, 16);
    insta::assert_snapshot!("screen", draw(&app, 62, 16));
}

#[test]
fn status_line_holds_status_url_title_and_percent() {
    let app = fake_app(FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")), HOME, 70, 12);
    let screen = draw(&app, 70, 12);
    let status = screen.lines().last().unwrap_or_default();
    assert!(status.contains("200 OK"), "{status}");
    assert!(status.contains(HOME), "{status}");
    assert!(status.contains("Design notes"), "{status}");
    assert!(status.contains('%'), "{status}");
}

#[test]
fn status_line_for_errors_and_network_failures() {
    let app = fake_app(FakeFetcher::new().with_html(HOME, 404, "<h1>nope</h1>"), HOME, 60, 8);
    let status = draw(&app, 60, 8).lines().last().unwrap_or_default().to_string();
    assert!(status.contains("404 NOT FOUND"), "{status}");

    let app = fake_app(
        FakeFetcher::new().with_error(HOME, w3b::error::NetError::Tls, "bad certificate"),
        HOME,
        60,
        8,
    );
    let status = draw(&app, 60, 8).lines().last().unwrap_or_default().to_string();
    assert!(status.contains("ERROR TLS"), "{status}");
}

#[test]
fn loading_state_shows_the_word_loading() {
    let config = w3b::config::built_in_defaults();
    let theme = w3b::Theme::build(&config).unwrap();
    let mut app = w3b::App::empty(
        config,
        theme,
        std::sync::Arc::new(FakeFetcher::new()),
        HOME,
        w3b::Viewport { width: 60, height: 8 },
    );
    app.navigate(HOME);
    assert!(app.is_loading());
    let status = draw(&app, 60, 8).lines().last().unwrap_or_default().to_string();
    assert!(status.contains("LOADING"), "{status}");
}

#[test]
fn prompt_line_appears_while_typing_a_url() {
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, "<p>x</p>"), HOME, 60, 8);
    app.handle_key(key(KeyCode::Char(':')));
    type_str(&mut app, "example.com/path");
    let screen = draw(&app, 60, 8);
    insta::assert_snapshot!("prompt", screen);
    assert!(screen.contains(":open example.com/path"), "{screen}");
    // status line moves down one row because the prompt takes a row
    assert!(screen.lines().last().unwrap_or_default().contains("200 OK"));
}

#[test]
fn help_overlay_covers_the_page() {
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")), HOME, 60, 20);
    app.handle_key(key(KeyCode::Char('i')));
    let screen = draw(&app, 60, 20);
    insta::assert_snapshot!("help", screen);
    assert!(screen.contains("w3b"), "{screen}");
    assert!(screen.contains("Tab"), "the help lists the keys");
}

#[test]
fn page_colors_are_never_used() {
    // A page covered in CSS color must still render in the configured colors.
    let html = "<p style=\"color:red;background:blue\"><span style=\"color:#00ff00\">green text</span> <b>bold</b></p>";
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 40, 8);
    app.view.cursor.row = 3; // keep row 0 clear of the cursor line style
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    terminal.draw(|f| render::draw_all(&app, f.area(), f.buffer_mut())).unwrap();
    let buf = terminal.backend().buffer();
    let style = buf.cell((1, 0)).expect("cell").style();
    // None and Color::Reset both mean "the terminal's own colour".
    let plain = |c: Option<Color>| c.filter(|c| *c != Color::Reset);
    assert_eq!(plain(style.fg), plain(app.theme.text.fg), "text keeps the configured fg");
    assert_eq!(plain(style.bg), plain(app.theme.text.bg), "the page cannot set a background");
}

#[test]
fn links_and_the_cursor_are_styled_as_configured() {
    let fetcher = FakeFetcher::new().with_html(
        HOME,
        200,
        r#"<p>go to <a href="https://test.local/x">the docs</a></p>"#,
    );
    let mut app = fake_app(fetcher, HOME, 40, 8);
    app.view.cursor.row = 3;
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    terminal.draw(|f| render::draw_all(&app, f.area(), f.buffer_mut())).unwrap();
    let buf = terminal.backend().buffer();

    // "go to the docs": the link starts at layout column 6 of row 0, and the
    // screen keeps `page_padding_x` blank columns before the text.
    let pad = app.padding_x() as u16;
    let link_cell = buf.cell((6 + pad, 0)).expect("link cell");
    assert_eq!(link_cell.symbol(), "t");
    let link_style = link_cell.style();
    if app.theme.link.add_modifier.contains(Modifier::UNDERLINED) {
        assert!(
            link_style.add_modifier.contains(Modifier::UNDERLINED),
            "links are underlined from the config"
        );
    }
    assert_eq!(link_style.fg, app.theme.link.fg, "link colour comes from the config");

    // the cursor cell is the one under the cursor
    app.view.cursor.row = 0;
    app.view.cursor.col = 6;
    terminal.draw(|f| render::draw_all(&app, f.area(), f.buffer_mut())).unwrap();
    let cur = terminal.backend().buffer().cell((6 + pad, 0)).expect("cursor cell").style();
    assert_eq!(cur.fg, app.theme.cursor.fg);
    assert_eq!(cur.bg, app.theme.cursor.bg);
}

#[test]
fn visited_links_use_the_visited_style() {
    let fetcher = FakeFetcher::new()
        .with_html(HOME, 200, r#"<p><a href="https://test.local/x">docs</a></p>"#)
        .with_html("https://test.local/x", 200, "<p>read me</p>");
    let mut app = fake_app(fetcher, HOME, 40, 8);
    app.open("https://test.local/x");
    app.open(HOME);
    app.view.cursor.row = 3;
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    terminal.draw(|f| render::draw_all(&app, f.area(), f.buffer_mut())).unwrap();
    let x = app.padding_x() as u16;
    let style = terminal.backend().buffer().cell((x, 0)).expect("cell").style();
    assert_eq!(style.fg, app.theme.visited.fg, "visited links look different");
}

#[test]
fn search_matches_are_highlighted() {
    let mut app =
        fake_app(FakeFetcher::new().with_html(HOME, 200, "<p>find the needle here</p>"), HOME, 40, 8);
    app.handle_key(key(KeyCode::Char('/')));
    type_str(&mut app, "needle");
    app.handle_key(key(KeyCode::Enter));
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    terminal.draw(|f| render::draw_all(&app, f.area(), f.buffer_mut())).unwrap();
    let buf = terminal.backend().buffer();
    let x = (9 + app.padding_x()) as u16;
    assert_eq!(buf.cell((x, 0)).expect("n").symbol(), "n");
    assert_eq!(buf.cell((x, 0)).unwrap().style().bg, app.theme.search_match.bg, "hit is highlighted");
}

#[test]
fn a_monospace_line_is_grey_right_across() {
    let html = "<p>plain <code>ls -l</code></p>";
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 40, 8);
    app.view.cursor.row = 3;
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    terminal.draw(|f| render::draw_all(&app, f.area(), f.buffer_mut())).unwrap();
    let buf = terminal.backend().buffer();
    let first = buf.cell((app.padding_x() as u16, 0)).expect("cell").style();
    assert_eq!(first.bg, app.theme.monospace.bg, "the text itself is on the grey block");
    // Far past the end of the text the background must still be there.
    let tail = buf.cell((30, 0)).expect("cell");
    assert_eq!(tail.symbol(), " ");
    assert_eq!(tail.style().bg, app.theme.monospace.bg, "the grey spans the whole line");
}

#[test]
fn an_ordinary_line_keeps_the_text_background() {
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, "<p>no code here</p>"), HOME, 40, 8);
    app.view.cursor.row = 3;
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    terminal.draw(|f| render::draw_all(&app, f.area(), f.buffer_mut())).unwrap();
    let buf = terminal.backend().buffer();
    let plain = |c: Option<Color>| c.filter(|c| *c != Color::Reset);
    assert_eq!(plain(buf.cell((0, 0)).unwrap().style().bg), plain(app.theme.text.bg));
}

#[test]
fn a_hovered_link_gets_its_own_background() {
    let fetcher = FakeFetcher::new().with_html(
        HOME,
        200,
        r#"<p>go to <a href="https://test.local/x">the docs</a></p>"#,
    );
    let mut app = fake_app(fetcher, HOME, 40, 8);
    app.view.cursor.row = 0;
    app.view.cursor.col = 7; // inside "the docs"
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    terminal.draw(|f| render::draw_all(&app, f.area(), f.buffer_mut())).unwrap();
    let style = terminal.backend().buffer().cell((8 + app.padding_x() as u16, 0)).expect("cell").style();
    assert_eq!(style.bg, app.theme.hovered_link.bg, "the hovered link is highlighted");
}

#[test]
fn the_page_is_padded_from_the_screen_edge() {
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, "<p>indented</p>"), HOME, 40, 8);
    assert_eq!(app.padding_x(), 1, "one column of padding by default");
    let screen = draw(&app, 40, 8);
    let line = screen.lines().next().unwrap_or_default();
    assert!(line.starts_with(' '), "the first column stays blank: {line:?}");
    assert!(line[1..].starts_with("indented"), "text starts after the padding: {line:?}");
    app.config.behavior.page_padding_x = 4;
    let wider = draw(&app, 40, 8);
    let line = wider.lines().next().unwrap_or_default();
    assert!(line[3..].starts_with(" "), "4 columns are blank: {line:?}");
    assert!(line[4..].starts_with("indented"), "{line:?}");
    app.config.behavior.page_padding_x = 0;
    let flush = draw(&app, 40, 8);
    assert!(flush.lines().next().unwrap_or_default().starts_with("indented"));
}

#[test]
fn hidden_elements_never_reach_the_screen() {
    let html = "<p>visible</p><style>.ad { display: none }</style><p class=ad>sponsored</p>";
    let app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 40, 8);
    let screen = draw(&app, 40, 8);
    assert!(screen.contains("visible"), "{screen}");
    assert!(!screen.contains("sponsored"), "display: none must not be drawn: {screen}");
}

#[test]
fn scrollbar_appears_only_when_configured() {
    let fetcher = FakeFetcher::new().with_html(HOME, 200, &fixture("article.html"));
    let mut app = fake_app(fetcher, HOME, 40, 10);
    assert!(!app.config.behavior.show_scrollbar);
    let plain = draw(&app, 40, 10);
    app.config.behavior.show_scrollbar = true;
    let with_bar = draw(&app, 40, 10);
    assert_ne!(plain, with_bar, "the scrollbar changes the output");
    assert!(with_bar.lines().last().unwrap_or_default().contains('│') || with_bar.contains('█'));
}

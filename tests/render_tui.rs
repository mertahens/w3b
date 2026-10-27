//! Rendering tests against ratatui's TestBackend: what actually lands in the
//! terminal buffer, including the status line and the styles.
mod common;

use common::{fake_app, fixture, key, loading_app, press, type_str};
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

/// Draw the whole UI and hand back the buffer, for style assertions.
/// Pin the load time so a status line in a snapshot is reproducible.
fn with_time(mut app: w3b::App) -> w3b::App {
    app.page.load_ms = Some(42);
    app
}

fn draw_buffer(app: &w3b::App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal.draw(|f| render::draw_all(app, f.area(), f.buffer_mut())).expect("draw");
    terminal.backend().buffer().clone()
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
    let app =
        with_time(fake_app(FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")), HOME, 62, 16));
    insta::assert_snapshot!("screen", draw(&app, 62, 16));
}

#[test]
fn the_startup_frame_reads_as_loading_not_as_an_error() {
    let app = loading_app(FakeFetcher::new().with_html(HOME, 200, "<h1>real page</h1>"), HOME, 60, 8);
    let screen = draw(&app, 60, 8);
    assert!(screen.contains("Loading"), "{screen}");
    assert!(!screen.to_uppercase().contains("ERROR"), "no error may flash at startup: {screen}");
    let status = screen.lines().last().unwrap_or_default();
    assert!(status.contains("LOADING"), "{status}");
}

#[test]
fn the_load_time_sits_on_the_right_of_the_status_line() {
    let app = fake_app(FakeFetcher::new().with_html(HOME, 200, "<h1>hi</h1>"), HOME, 60, 8);
    assert!(app.page.load_ms.is_some(), "a fetched page knows how long it took");
    let status = draw(&app, 60, 8).lines().last().unwrap_or_default().to_string();
    assert!(status.contains("ms"), "{status}");
    assert!(status.trim_end().ends_with('%'), "percent stays rightmost: {status}");

    let mut quiet = fake_app(FakeFetcher::new().with_html(HOME, 200, "<h1>hi</h1>"), HOME, 60, 8);
    quiet.config.behavior.show_load_time = false;
    let status = draw(&quiet, 60, 8).lines().last().unwrap_or_default().to_string();
    assert!(!status.contains("ms"), "{status}");
}

#[test]
fn the_status_hint_uses_the_same_colors_as_the_hovered_link() {
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")), HOME, 70, 12);
    press(&mut app, KeyCode::Tab);
    let buf = draw_buffer(&app, 70, 12);
    let y = u16::try_from(app.viewport.height - 1).expect("status row");
    let status: String = (0..70).map(|x| buf.cell((x, y)).expect("cell").symbol().to_string()).collect();
    let hint = status.find("-> ").unwrap_or_else(|| panic!("no link hint in: {status}"));
    let style = buf.cell(((hint + 3) as u16, y)).expect("cell").style();
    assert_eq!(style.fg, app.theme.hovered_link.fg, "the hint matches the link hover");
    assert_eq!(style.bg, app.theme.hovered_link.bg);
}

#[test]
fn the_goto_menu_lists_every_command() {
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")), HOME, 62, 12);
    press(&mut app, KeyCode::Char('g'));
    let screen = draw(&app, 62, 12);
    assert!(screen.contains("go to"), "{screen}");
    for (key, what) in w3b::app::GOTO_MENU {
        assert!(screen.contains(*key), "menu shows {key}");
        assert!(screen.contains(*what), "menu explains {key} as {what}: {screen}");
    }
    // The page is still readable behind a hint menu.
    assert!(screen.contains("Design notes"), "{screen}");
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
    let mut app = with_time(fake_app(FakeFetcher::new().with_html(HOME, 200, "<p>x</p>"), HOME, 60, 8));
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
    let mut app =
        with_time(fake_app(FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")), HOME, 60, 20));
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
fn monospace_slabs_the_text_and_the_rest_of_its_line() {
    let html = "<p>plain <code>ls -l</code></p>";
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 40, 8);
    app.view.cursor.row = 3;
    let buf = draw_buffer(&app, 40, 8);
    let pad = app.padding_x() as u16;

    let slab = app.theme.monospace.bg;
    let ink = app.theme.monospace.fg;
    assert_eq!(slab, Some(Color::Rgb(0x14, 0x14, 0x14)), "the mono background is dark");
    assert_eq!(ink, Some(Color::Rgb(0xd0, 0xd0, 0xd0)), "the mono text is bright but not glaring");
    // The code text sits on the slab, starting where `ls` begins ("plain " is 6).
    let code = buf.cell((pad + 6, 0)).expect("cell").style();
    assert_eq!(code.bg, slab, "the mono run is shaded");
    assert_eq!(code.fg, ink, "and its text is bright, not the <code> colour");
    assert_eq!(buf.cell((pad, 0)).expect("cell").style().bg, Some(Color::Reset), "plain words are not");
    // The slab runs on to the end of the line, so a code block reads as a block.
    assert_eq!(buf.cell((pad + 11, 0)).expect("cell").style().bg, slab, "the rest of the line is filled");
    assert_eq!(buf.cell((39, 0)).expect("cell").style().bg, slab, "up to the last column");
    assert_eq!(
        buf.cell((pad, 1)).expect("cell").style().bg,
        Some(Color::Reset),
        "no shading on the empty row below"
    );
}

#[test]
fn images_are_labelled_and_painted_on_their_own_colour() {
    let html = "<p><img src=/img/logo.png alt=logo><img src=/x/spot.gif></p>";
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 40, 8);
    app.view.cursor.row = 2;
    let buf = draw_buffer(&app, 40, 8);
    let pad = app.padding_x() as u16;
    let text = buffer_text(&buf);
    assert!(text.contains("[img: logo]"), "{text:?}");
    assert!(text.contains("[img: spot.gif]"), "no alt falls back to the file name");
    assert!(!text.contains("[logo]"), "the label always says what it is: {text:?}");
    let style = buf.cell((pad + 2, 0)).expect("cell").style();
    assert_eq!(style.bg, Some(Color::Magenta), "black on purple, as shipped");
    assert_eq!(style.fg, Some(Color::Black));
}

#[test]
fn a_fetch_in_flight_spins_in_the_status_line() {
    let mut app = fake_app(
        FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")).with_html(
            "https://test.local/slow",
            200,
            "<h1>Slow</h1>",
        ),
        HOME,
        50,
        8,
    );
    app.navigate("https://test.local/slow");
    let spin = app.spinner_glyph();
    let line = draw(&app, 50, 8).lines().last().expect("a status line").to_string();
    assert!(line.contains(spin), "the braille spinner shows: {line:?}");
    assert!(line.contains("LOADING"), "{line:?}");
    let done = fake_app(FakeFetcher::new().with_html(HOME, 200, "<h1>Done</h1>"), HOME, 50, 8);
    let quiet = draw(&done, 50, 8);
    let quiet = quiet.lines().last().expect("status").to_string();
    assert!(!quiet.contains(spin), "a settled page has no spinner: {quiet:?}");
}

#[test]
fn the_terminal_caret_parks_in_the_field_being_typed_into() {
    let html = "<h1>T</h1><p>q <input value=ann></p>";
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 40, 8);
    let area = ratatui::layout::Rect::new(0, 0, 40, 8);
    assert_eq!(render::cursor_screen_pos(&app, area), None, "no caret while browsing");

    let (row, end) = app.page.layout.field_end(0).expect("field");
    app.view.cursor.row = row;
    app.view.cursor.col = end - 1;
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode, w3b::app::Mode::Field);
    let (x, y) = render::cursor_screen_pos(&app, area).expect("a caret while typing");
    assert_eq!(y as usize, row);
    assert!((x as usize) < usize::from(area.width), "caret inside the screen: {x}");
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

#[test]
fn the_hover_highlight_covers_the_whole_link() {
    let url = "https://test.local/hover";
    let html = "<p>see <a href=\"/x\">the first long link</a> and stop</p>";
    let mut app = fake_app(FakeFetcher::new().with_html(url, 200, html), url, 60, 12);
    let row = &app.rows()[0];
    let start = row.runs.iter().find(|r| r.text == "the").expect("run").start;
    let end = row.runs.iter().find(|r| r.text == "link").expect("last link word").end();
    app.view.cursor.row = 0;
    app.view.cursor.col = start + 1;
    let buf = draw_buffer(&app, 60, 12);
    let hovered = app.theme.hovered_link.bg.expect("hover background");
    for col in start..end {
        if col == app.view.cursor.col {
            continue; // the cursor cell is painted on top of the hover
        }
        let x = (app.padding_x() as u16) + col as u16;
        assert_eq!(
            // The page starts at screen row 0; the status line is at the bottom.
            buf.cell((x, 0)).expect("cell").style().bg,
            Some(hovered),
            "column {col} of the link should be highlighted"
        );
    }
    // Words outside the link keep the plain background.
    let after = app.rows()[0].runs.iter().find(|r| r.text == "and").expect("plain word");
    assert_ne!(
        buf.cell(((app.padding_x() as u16 + after.start as u16), 0)).expect("cell").style().bg,
        Some(hovered),
        "a word outside the link must not be highlighted"
    );
}

#[test]
fn a_link_that_wraps_is_highlighted_on_every_row_it_covers() {
    let url = "https://test.local/wrap";
    let html = "<p>prefix <a href=\"/x\">alpha beta gamma delta epsilon</a></p>";
    let mut app = fake_app(FakeFetcher::new().with_html(url, 200, html), url, 20, 12);
    let link_rows: Vec<usize> = app
        .page
        .layout
        .links
        .iter()
        .map(|p| p.row)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert!(link_rows.len() > 1, "the fixture link should wrap at width 20, rows: {link_rows:?}");
    let first = app.page.layout.links.first().expect("a link placement");
    app.view.cursor.row = first.row;
    app.view.cursor.col = first.start;
    let hovered = app.theme.hovered_link.bg.expect("hover background");
    let buf = draw_buffer(&app, 20, 12);
    for placement in &app.page.layout.links {
        if placement.row == app.view.cursor.row && placement.start == app.view.cursor.col {
            continue; // the cursor cell is painted on top of the hover
        }
        let x = app.padding_x() as u16 + placement.start as u16;
        let y = (placement.row - app.view.scroll) as u16;
        assert_eq!(
            buf.cell((x, y)).expect("cell").style().bg,
            Some(hovered),
            "the link placement on row {} col {} should be highlighted",
            placement.row,
            placement.start
        );
    }
}

#[test]
fn the_shipped_hover_paints_the_link_colour_behind_black_ink() {
    let app = fake_app(FakeFetcher::new().with_html(HOME, 200, "<p><a href=\"/x\">x</a></p>"), HOME, 60, 16);
    assert_eq!(app.theme.link.fg, Some(Color::Blue), "links are blue");
    assert_eq!(app.theme.hovered_link.bg, app.theme.link.fg, "hover moves that blue to the background");
    assert_eq!(app.theme.hovered_link.fg, Some(Color::Black), "and the ink goes black");
}

#[test]
fn an_image_inside_a_link_stays_black_and_underlined() {
    let html = "<p><a href=\"/x\"><img src=\"a.png\" alt=\"logo\"></a> <a href=\"/y\">word</a></p>";
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 40, 8);
    app.view.cursor.row = 3;
    let buf = draw_buffer(&app, 40, 8);
    let pad = app.padding_x() as u16;
    assert_eq!(buf.cell((pad, 0)).expect("cell").symbol(), "[", "the label starts the row");
    for x in pad..pad + 11 {
        let style = buf.cell((x, 0)).expect("cell").style();
        assert_eq!(style.fg, Some(Color::Black), "the image label keeps black ink");
        assert_eq!(style.bg, Some(Color::Magenta), "on the image background");
        assert!(style.add_modifier.contains(Modifier::UNDERLINED), "and takes the link underline");
    }
    assert_eq!(buf.cell((pad + 12, 0)).expect("cell").style().fg, Some(Color::Blue), "a word link is blue");
}

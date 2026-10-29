//! Rendering tests against ratatui's TestBackend: what actually lands in the
//! terminal buffer, including the status line and the styles.
mod common;

use common::{cell_of, drag, fake_app, fixture, key, loading_app, page_cell, press, type_str};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::Rect;
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

/// The frame of the box floating over the page, found from its top-left corner.
fn box_rect(buf: &Buffer) -> Rect {
    let area = buf.area;
    let Some((x, y)) = (area.top()..area.bottom())
        .flat_map(|y| (area.left()..area.right()).map(move |x| (x, y)))
        .find(|(x, y)| buf.cell((*x, *y)).is_some_and(|c| c.symbol() == "┌"))
    else {
        panic!("nothing floats over the page");
    };
    let height = (y + 1..area.bottom())
        .find(|row| buf.cell((x, *row)).is_some_and(|c| c.symbol() == "└"))
        .unwrap_or(area.bottom() - 1)
        - y
        + 1;
    let width = (x + 1..area.right())
        .find(|col| buf.cell((*col, y)).is_some_and(|c| c.symbol() == "┐"))
        .unwrap_or(area.right() - 1)
        - x
        + 1;
    Rect::new(x, y, width, height)
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
fn the_size_of_the_page_sits_next_to_the_load_time() {
    let app = fake_app(FakeFetcher::new().with_html(HOME, 200, "<h1>hi</h1>"), HOME, 60, 8);
    assert_eq!(app.size_text().as_deref(), Some("11 B"), "the body as it was sent, not as it is drawn");
    let status = draw(&app, 60, 8).lines().last().unwrap_or_default().to_string();
    assert!(status.contains("ms"), "{status}");
    assert!(status.contains("11 B"), "the size sits by the time: {status}");
    assert!(status.trim_end().ends_with('%'), "percent stays rightmost: {status}");

    // Past a kilobyte the count gets the same one decimal treatment.
    let body = format!("<p>{}</p>", "x".repeat(2000));
    let big = fake_app(FakeFetcher::new().with_html(HOME, 200, &body), HOME, 60, 8);
    assert_eq!(big.size_text().as_deref(), Some("2.0 kB"));

    // A page that never came over the wire has no size to say.
    let broken =
        fake_app(FakeFetcher::new().with_error(HOME, w3b::error::NetError::Tls, "bad cert"), HOME, 60, 8);
    assert_eq!(broken.size_text(), None, "nothing was fetched");
    let status = draw(&broken, 60, 8).lines().last().unwrap_or_default().to_string();
    assert!(
        status.ends_with('%') || status.trim_end().ends_with('%'),
        "the percent still sits there: {status}"
    );

    // Switched off, it is gone and the time keeps its place.
    let mut quiet = fake_app(FakeFetcher::new().with_html(HOME, 200, "<h1>hi</h1>"), HOME, 60, 8);
    quiet.config.behavior.show_size = false;
    let status = draw(&quiet, 60, 8).lines().last().unwrap_or_default().to_string();
    assert!(!status.contains("11 B"), "{status}");
    assert!(status.contains("ms"), "{status}");
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
fn what_floats_over_the_page_is_bright_words_with_no_background() {
    // The help pane, the page info pane and the `g` menu all float over the page,
    // so they are drawn the same way and are checked the same way.
    let plain = |c: Option<Color>| c.filter(|c| *c != Color::Reset);
    for (what, key) in [
        ("help pane", KeyCode::Char('?')),
        ("info pane", KeyCode::Char('i')),
        ("go-to menu", KeyCode::Char('g')),
    ] {
        let mut app =
            fake_app(FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")), HOME, 62, 24);
        press(&mut app, key);
        let buf = draw_buffer(&app, 62, 24);
        let frame = box_rect(&buf);
        let inside = |x, y| buf.cell((x, y)).expect("cell").style();

        // Nothing inside the frame carries a background: the box is a window onto
        // the terminal's own background, not a slab pasted over the page.
        for y in frame.top()..frame.bottom() {
            for x in frame.left()..frame.right() {
                assert_eq!(plain(inside(x, y).bg), None, "{what} paints a background at ({x}, {y})");
            }
        }
        // The rules are a step brighter than the `dark_gray` lines in a table.
        assert_eq!(inside(frame.left(), frame.top()).fg, Some(Color::Gray), "the {what} frame");
        // So is everything written inside them. The `g` menu keeps the link colour
        // for its key, which is the only other ink allowed in a box.
        let inks: Vec<Option<Color>> = (frame.top()..frame.bottom())
            .flat_map(|y| (frame.left()..frame.right()).map(move |x| (x, y)))
            .filter(|(x, y)| {
                buf.cell((*x, *y)).expect("cell").symbol().chars().next().is_some_and(char::is_alphanumeric)
            })
            .map(|(x, y)| inside(x, y).fg)
            .collect();
        assert!(
            inks.iter().all(|fg| matches!(fg, Some(Color::Gray) | Some(Color::Blue))),
            "{what} is not drawn in bright ink: {inks:?}"
        );
        assert!(inks.contains(&Some(Color::Gray)), "{what} has no bright words: {inks:?}");
    }
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
fn a_mark_is_painted_over_the_cells_it_covers() {
    let mut app =
        with_time(fake_app(FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")), HOME, 62, 16));
    let (row, col) = page_cell(&app, "bold");
    app.move_cursor_to((row, col));
    press(&mut app, KeyCode::Char('v'));
    for _ in 0..3 {
        press(&mut app, KeyCode::Char('l'));
    }
    let buf = draw_buffer(&app, 62, 16);
    let pad = app.padding_x();
    let y = (row - app.view.scroll) as u16;
    let bg = |column: usize| buf.cell((column as u16, y)).expect("cell").style().bg;
    let plain = |c: Option<Color>| c.filter(|c| *c != Color::Reset);
    for column in col..col + 3 {
        assert_eq!(bg(pad + column), app.theme.selection.bg, "the cell {column} is marked");
    }
    assert_eq!(bg(pad + col + 3), app.theme.cursor.bg, "the cursor's own cell keeps the cursor's colours");
    assert_eq!(plain(bg(pad + col + 4)), plain(app.theme.cursor_line.bg), "nothing past the mark is");
}

#[test]
fn a_mark_over_code_is_painted_over_the_slab() {
    let mut app =
        with_time(fake_app(FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")), HOME, 62, 16));
    let (row, col) = page_cell(&app, "println");
    let pad = app.padding_x();
    let slab = app.theme.monospace.bg;
    assert_eq!(
        draw_buffer(&app, 62, 16).cell(((pad + col) as u16, row as u16)).expect("cell").style().bg,
        slab,
        "the code sits on its slab before anything is marked"
    );
    app.move_cursor_to((row, col));
    press(&mut app, KeyCode::Char('v'));
    for _ in 0..4 {
        press(&mut app, KeyCode::Char('l'));
    }
    let buf = draw_buffer(&app, 62, 16);
    let y = (row - app.view.scroll) as u16;
    assert_eq!(
        buf.cell(((pad + col) as u16, y)).expect("cell").style().bg,
        app.theme.selection.bg,
        "the mark is legible on code too"
    );
}

#[test]
fn dragging_the_mouse_marks_the_cells_it_passed_over() {
    let mut app =
        with_time(fake_app(FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")), HOME, 62, 16));
    let here = cell_of(&app, "Design notes");
    drag(&mut app, here, (here.0, here.1 + 3));
    let buf = draw_buffer(&app, 62, 16);
    for column in here.1..here.1 + 3 {
        assert_eq!(
            buf.cell((column as u16, here.0 as u16)).expect("cell").style().bg,
            app.theme.selection.bg
        );
    }
}

#[test]
fn the_status_line_says_what_is_marked_and_what_was_taken() {
    let mut app =
        with_time(fake_app(FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")), HOME, 62, 16));
    let (row, col) = page_cell(&app, "Design");
    app.move_cursor_to((row, col));
    press(&mut app, KeyCode::Char('v'));
    for _ in 0..3 {
        press(&mut app, KeyCode::Char('l'));
    }
    let status = draw(&app, 62, 16).lines().last().unwrap_or_default().to_string();
    assert!(status.contains("4 selected"), "the mark is counted before it is taken: {status}");
    press(&mut app, KeyCode::Char('y'));
    let status = draw(&app, 62, 16).lines().last().unwrap_or_default().to_string();
    assert!(status.contains("yanked 4 characters"), "{status}");
}

#[test]
fn a_mark_that_stands_is_still_painted_and_counted() {
    let mut app =
        with_time(fake_app(FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")), HOME, 62, 16));
    let (row, col) = page_cell(&app, "bold");
    app.move_cursor_to((row, col));
    press(&mut app, KeyCode::Char('v'));
    for _ in 0..3 {
        press(&mut app, KeyCode::Char('l'));
    }
    // The second `v` frees the cursor; the band is what it was.
    press(&mut app, KeyCode::Char('v'));
    for _ in 0..2 {
        press(&mut app, KeyCode::Char('j'));
    }
    let buf = draw_buffer(&app, 62, 16);
    let pad = app.padding_x();
    let y = (row - app.view.scroll) as u16;
    let bg = |column: usize| buf.cell((column as u16, y)).expect("cell").style().bg;
    for column in col..col + 4 {
        assert_eq!(bg(pad + column), app.theme.selection.bg, "the cell {column} is still marked");
    }
    let status = draw(&app, 62, 16).lines().last().unwrap_or_default().to_string();
    assert!(status.contains("4 selected"), "the count stays with the mark, not the cursor: {status}");
}

#[test]
fn the_status_line_writes_its_own_words_in_bright_grey() {
    // The bar writes the address or title, the mark count, the message and the
    // figures on the right in one bright grey on its dark background. A badge is
    // the other kind of thing on the bar: it brings ink and a background that
    // only work together, so it keeps both.
    let app = loading_app(FakeFetcher::new().with_html(HOME, 200, "<h1>hi</h1>"), HOME, 60, 10);
    let buf = draw_buffer(&app, 60, 10);
    let row = 9u16;
    let bar = app.theme.status.base;
    let badge = app.theme.status.loading;

    let mut words = 0;
    let mut marks = 0;
    for x in 0..60u16 {
        let cell = buf.cell((x, row)).expect("cell");
        if cell.symbol() == " " {
            continue;
        }
        if cell.style().bg == badge.bg {
            marks += 1;
            assert_eq!(cell.style().fg, badge.fg, "the badge at column {x} keeps the ink it brought");
        } else {
            words += 1;
            assert_eq!(cell.style().fg, bar.fg, "the bar's own word at column {x} is one colour");
            assert_eq!(cell.style().bg, bar.bg, "on the bar's own background");
        }
    }
    assert!(marks > 0, "the LOADING badge is painted on the left of the bar");
    assert!(words > 20, "and there are plenty of words beside it: {words}");
    assert_eq!(bar.fg, Some(Color::Gray), "and those words are bright grey");
}

#[test]
fn help_overlay_covers_the_page() {
    let mut app =
        with_time(fake_app(FakeFetcher::new().with_html(HOME, 200, &fixture("article.html")), HOME, 60, 20));
    app.handle_key(key(KeyCode::Char('?')));
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
fn a_link_off_the_site_is_coloured_by_where_it_goes() {
    let html = r#"<p><a href="/here">here</a> <a href="https://elsewhere.example/there">there</a></p>"#;
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 40, 8);
    app.view.cursor.row = 3;
    let buf = draw_buffer(&app, 40, 8);
    let pad = app.padding_x() as u16;
    assert_eq!(
        buf.cell((pad, 0)).expect("cell").style().fg,
        app.theme.link.fg,
        "a link on this site is an ordinary link"
    );

    let off = buf.cell((pad + 5, 0)).expect("the off-site word");
    assert_eq!(off.symbol(), "t", "'there' starts at layout column 5");
    let style = off.style();
    assert_eq!(style.fg, app.theme.external.fg, "a link to another host is coloured by where it goes");
    assert_ne!(app.theme.external.fg, app.theme.link.fg, "and that colour is its own");
    assert!(
        style.add_modifier.contains(Modifier::UNDERLINED),
        "it is still a link, so it is still underlined"
    );
}

#[test]
fn a_visited_external_link_does_not_start_looking_local() {
    let html = r#"<p><a href="https://elsewhere.example/there">there</a></p>"#;
    let fetcher = FakeFetcher::new().with_html(HOME, 200, html).with_html(
        "https://elsewhere.example/there",
        200,
        "<p>read</p>",
    );
    let mut app = fake_app(fetcher, HOME, 40, 8);
    app.open("https://elsewhere.example/there");
    app.open(HOME);
    app.view.cursor.row = 3;
    let buf = draw_buffer(&app, 40, 8);
    let pad = app.padding_x() as u16;
    assert_eq!(
        buf.cell((pad, 0)).expect("cell").style().fg,
        app.theme.external.fg,
        "having read it does not make it look like it belongs here"
    );
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
    // The cursor sits on the first hit, so that one cell wears the cursor style and
    // the rest of the hit shows the band.
    assert_eq!(buf.cell((x + 1, 0)).unwrap().style().bg, app.theme.search_match.bg, "hit is highlighted");
    assert_eq!(
        (app.theme.search_match.bg, app.theme.search_match.fg),
        (Some(Color::Gray), Some(Color::Black)),
        "a hit is a grey band with dark glyphs, not a shout of yellow"
    );
}

#[test]
fn monospace_slabs_its_text_with_one_space_of_padding() {
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
    // The slab carries one blank cell of padding on each side and stops there: no
    // dark bar across the empty part of the line, however wide the terminal is.
    assert_eq!(
        buf.cell((pad + 5, 0)).expect("cell").style().bg,
        slab,
        "the space before the code is on the slab"
    );
    assert_eq!(
        buf.cell((pad + 4, 0)).expect("cell").style().bg,
        Some(Color::Reset),
        "one cell of padding, not two"
    );
    assert_eq!(buf.cell((pad + 10, 0)).expect("cell").style().bg, slab, "the last glyph is still shaded");
    assert_eq!(buf.cell((pad + 11, 0)).expect("cell").style().bg, slab, "plus one cell after it");
    assert_eq!(buf.cell((pad + 12, 0)).expect("cell").style().bg, Some(Color::Reset), "then nothing at all");
    assert_eq!(
        buf.cell((39, 0)).expect("cell").style().bg,
        Some(Color::Reset),
        "clear to the edge of the line"
    );
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
    assert!(
        line.contains(&format!("{spin} LOADING")),
        "a blank cell separates the spinner from the word: {line:?}"
    );
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
fn the_shipped_config_makes_an_off_site_link_cyan() {
    let app = fake_app(FakeFetcher::new().with_html(HOME, 200, "<p><a href=\"/x\">x</a></p>"), HOME, 60, 16);
    assert_eq!(app.theme.external.fg, Some(Color::Cyan), "a link off this site is cyan");
    assert_ne!(app.theme.external.fg, app.theme.link.fg, "and not the blue of a link that stays");
    assert!(
        app.theme.external.add_modifier.contains(Modifier::UNDERLINED),
        "it is still a link, so it is still underlined"
    );
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

#[test]
fn a_summary_line_is_underlined_text_and_not_a_link() {
    let html = "<p>plain words</p><p><a href=\"/x\">a link</a></p><details><summary>Anything</summary>body</details>";
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 40, 10);
    app.view.cursor.row = 3;
    let buf = draw_buffer(&app, 40, 10);
    let pad = app.padding_x() as u16;
    let plain = buf.cell((pad, 0)).expect("cell").style().fg;
    assert_eq!(buf.cell((pad, 2)).expect("cell").style().fg, app.theme.link.fg, "a link is link-coloured");

    let marker = buf.cell((pad + 2, 4)).expect("cell");
    assert_eq!(marker.symbol(), "+", "the marker says the line can be opened");
    assert_eq!(marker.style().fg, plain, "in the colour of ordinary text");
    assert_ne!(marker.style().fg, app.theme.link.fg, "and not in the link colour");
    assert!(marker.style().add_modifier.contains(Modifier::UNDERLINED), "underlined as something to open");

    let word = buf.cell((pad + 4, 4)).expect("cell").style();
    assert_eq!(word.fg, plain, "its words are ordinary text as well");
    assert!(word.add_modifier.contains(Modifier::UNDERLINED), "and underlined");
    assert!(
        !buf.cell((pad, 4)).expect("cell").style().add_modifier.contains(Modifier::UNDERLINED),
        "the blank columns before the marker are not marked"
    );
}

#[test]
fn the_cursor_on_a_summary_marks_the_whole_line() {
    let html = "<details><summary>Anything at all</summary>body</details>";
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 40, 10);
    app.view.cursor.row = 0;
    app.view.cursor.col = 6;
    let buf = draw_buffer(&app, 40, 10);
    let pad = app.padding_x() as u16;
    // One target, one highlight: the marker and every word of the line, however
    // many rows the label wraps over.
    assert_eq!(
        buf.cell((pad + 2, 0)).expect("cell").style().bg,
        app.theme.hovered_link.bg,
        "the marker joins in"
    );
    assert_eq!(
        buf.cell((pad + 9, 0)).expect("cell").style().bg,
        app.theme.hovered_link.bg,
        "and so do the words"
    );
    assert_eq!(
        buf.cell((pad + 30, 0)).expect("cell").style().bg,
        app.theme.cursor_line.bg,
        "nothing past the line joins in"
    );
}

#[test]
fn a_linked_heading_keeps_its_colour_and_gains_the_underline() {
    let html = "<h2><a href=\"/x\">chapter one</a></h2><p><a href=\"/y\">plain</a></p>";
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 40, 8);
    app.view.cursor.row = 4;
    let buf = draw_buffer(&app, 40, 8);
    let pad = app.padding_x() as u16;
    let head = buf.cell((pad + 1, 0)).expect("cell").style();
    assert_eq!(head.fg, app.theme.heading.fg, "the heading keeps the heading colour");
    assert!(head.add_modifier.contains(Modifier::UNDERLINED), "the link shows as an underline");
    assert_eq!(
        buf.cell((pad, 2)).expect("cell").style().fg,
        app.theme.link.fg,
        "a word link is link-coloured"
    );
}

/// The info pane: the numbers a page has a right to be curious about, taken from a
/// fixture that asks for far more than w3b goes and gets.
#[test]
fn the_info_pane_says_what_the_page_asked_for() {
    let html = "<title>many</title><link rel=stylesheet href=/a.css><link rel=stylesheet href=/b.css>\
        <link rel=icon href=/f.ico><script src=/a.js></script><script>inline()</script>\
        <iframe src=/fr></iframe><svg></svg><canvas></canvas>\
        <p><img src=/1.png alt=one><img src=/2.png alt=''><video src=/v.mp4 title=v></p>";
    let app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 70, 24);

    // The page asked for two sheets and w3b read both of them; everything else it
    // counted and left where it was.
    let joined = render::info_lines(&app).join("\n");
    for needle in [
        "200 OK",
        "297 B",
        "2 of 2 stylesheets",
        "2 images",
        "1 video/audio",
        "2 scripts",
        "1 frame",
        "2 svg/canvas/markup",
        "1 other link",
        "2 media labels",
    ] {
        assert!(joined.contains(needle), "{needle} missing from:\n{joined}");
    }

    // The clock: the pane's total is the number the status line shows, and the
    // parts are inside it rather than alongside it.
    let timings = app.page.timings.expect("a fetched page is timed");
    assert_eq!(timings.total, app.page.load_ms.expect("the same span"), "one clock, two places");
    assert!(
        timings.fetch + timings.css + timings.parse + timings.layout <= timings.total,
        "the parts fit inside the whole: {timings:?}"
    );

    // The pane itself, on screen, opened by `i`.
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 70, 24);
    press(&mut app, KeyCode::Char('i'));
    assert!(matches!(app.mode, w3b::Mode::Info), "`i` opens the info pane");
    let screen = draw(&with_time(app), 70, 24);
    insta::assert_snapshot!("info", screen);
    assert!(screen.contains("page info"), "the pane is titled");
    assert!(screen.contains("left"), "...and lists what was left out");
}

#[test]
fn the_parts_of_a_json_body_take_their_own_colours() {
    let json = r#"{"name":"w3b","stars":100,"on":true,"todo":[]}"#;
    let mut app = fake_app(FakeFetcher::new().with_type(HOME, 200, "application/json", json), HOME, 40, 8);
    app.view.cursor.row = 0;
    let buf = draw_buffer(&app, 40, 8);
    let text = buffer_text(&buf);
    assert!(text.contains("\"name\": \"w3b\""), "{text:?}");
    assert!(text.contains("100") && text.contains("true"), "{text:?}");
    // Count the cells carrying each part colour: keys, strings, numbers and the
    // three literals all read differently, and the structure sits back in muted.
    let painted = |fg: Option<Color>| {
        (0..8u16)
            .flat_map(|r| (0..40u16).map(move |c| (c, r)))
            .filter(|&(c, r)| fg.is_some() && buf.cell((c, r)).expect("cell").style().fg == fg)
            .count()
    };
    for style in [
        app.theme.json_key.fg,
        app.theme.json_string.fg,
        app.theme.json_number.fg,
        app.theme.json_literal.fg,
        app.theme.muted.fg,
    ] {
        assert!(painted(style) > 0, "some cell carries {style:?}");
    }
    assert_ne!(app.theme.json_key.fg, app.theme.json_string.fg, "the shipped theme tells a key from a value");
    // The monospace slab is the shape of the body: the text of every part of it is
    // painted on that dark background.
    let slab = app.theme.monospace.bg;
    let on_slab = (0..8u16)
        .flat_map(|r| (0..40u16).map(move |c| (c, r)))
        .filter(|&(c, r)| buf.cell((c, r)).expect("cell").style().bg == slab)
        .count();
    assert!(on_slab > 10, "the body sits on the monospace slab, not on plain text");
}

#[test]
fn a_direct_view_of_a_file_is_monospace_across_the_whole_line() {
    let json = "{\"name\":\"w3b\",\"stars\":100}";
    let mut app = fake_app(FakeFetcher::new().with_type(HOME, 200, "application/json", json), HOME, 60, 12);
    app.view.cursor.row = 3;
    let buf = draw_buffer(&app, 60, 12);
    let slab = app.theme.monospace.bg;
    let pad = app.padding_x() as u16;
    let open = (0..60u16)
        .find(|c| buf.cell((*c, 0)).expect("cell").symbol() == "{")
        .expect("the body starts on the first row");
    assert!(open >= pad, "and not in the page padding");
    // The line number sits in front of the body: the cell before `{` is the gutter's
    // space, and the digit is before that.
    assert_eq!(buf.cell((open - 1, 0)).expect("cell").symbol(), " ", "the gutter ends in a space");
    assert_eq!(buf.cell((open - 2, 0)).expect("cell").symbol(), "1", "and says which line this is");
    assert_eq!(
        buf.cell((open - 2, 0)).expect("cell").style().fg,
        app.theme.muted.fg,
        "the number is furniture: the slab says the text is monospace, not what it is"
    );
    // A file is only its own text, so the slab is not the outline of the glyphs but
    // the whole text area: it reaches the last column of the row, long past the `}`.
    assert_eq!(
        buf.cell((60 - pad - 3, 0)).expect("cell").style().bg,
        slab,
        "the line is on the slab to the end of the text area"
    );
    // The page padding on either side is not part of it.
    assert_eq!(buf.cell((0, 0)).expect("cell").style().bg, Some(Color::Reset), "not the left margin");
    assert_eq!(buf.cell((59, 0)).expect("cell").style().bg, Some(Color::Reset), "not the right margin");
    // A search hit inside that wall of monospace text still gets its grey band: the
    // slab goes under the mark instead of over it. The cursor sits on the first cell
    // of the hit, so it is the second cell that carries the band.
    app.handle_key(key(KeyCode::Char('/')));
    type_str(&mut app, "w3b");
    app.handle_key(key(KeyCode::Enter));
    let buf = draw_buffer(&app, 60, 12);
    let (row, hits) = app.matches.iter().next().expect("a match");
    let start = hits[0].0;
    let cell = buf.cell(((app.padding_x() + start + 1) as u16, *row as u16)).expect("hit cell");
    assert_eq!(cell.style().bg, app.theme.search_match.bg, "a hit keeps its background");
    assert_eq!(cell.style().fg, app.theme.search_match.fg, "and its glyphs stay dark");
}

#[test]
fn any_body_read_as_a_file_gets_the_slab_across_the_row() {
    // Not only JSON: a text/plain or text/css body is the file itself as well.
    let body = "short line\nthe second line is longer than the first one by a fair bit";
    let app = fake_app(FakeFetcher::new().with_type(HOME, 200, "text/plain", body), HOME, 60, 12);
    let buf = draw_buffer(&app, 60, 12);
    let slab = app.theme.monospace.bg;
    assert!(app.page.file_view, "the page knows it is a file and not a page");
    assert_eq!(
        buf.cell((60 - app.padding_x() as u16 - 3, 1)).expect("cell").style().bg,
        slab,
        "a short line is still monospace to the end of the row"
    );
}

#[test]
fn code_inside_a_page_is_only_slabbed_where_the_code_is() {
    // The other half of the same rule: in a page made of prose the slab stays around
    // the code, so the words either side of it keep the page background.
    let html = "<p>plain <code>ls -l</code> again</p>";
    let mut app = fake_app(FakeFetcher::new().with_html(HOME, 200, html), HOME, 60, 12);
    app.view.cursor.row = 3;
    let buf = draw_buffer(&app, 60, 12);
    let pad = app.padding_x() as u16;
    assert_eq!(buf.cell((pad, 0)).expect("cell").style().bg, Some(Color::Reset), "plain words are not");
    assert_eq!(
        buf.cell((60 - pad - 3, 0)).expect("cell").style().bg,
        Some(Color::Reset),
        "nor is the end of the row"
    );
}

//! Painting the laid-out rows, the status line and prompts.
use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::{Block, Clear, Widget};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Mode};
use crate::json::Role;
use crate::layout::CellStyle;
use crate::status::{self, Status};

pub const HELP: &str = "\
w3b keys  (? toggles this)
  j / k          down, up        gg / G      top, bottom
  h / l          left, right     0 ^ $       line edges
  Ctrl-d / Ctrl-u  half page     PgDn/PgUp   full page
  Enter          follow/edit/open  r         reload
  Tab            next target     Shift-Tab   previous
  o or :         open url        /           search
  n / N          next/prev hit   Esc         cancel
  [ or Backspace back            ]           forward
  v V drag       mark text       v again     mark stays
  y              what is marked, else link, else page
  g then ...     go-to menu      w           toggle wrap
  in a field     typing edits it, Esc leaves it
  Enter there    sends the form, if it asks for a GET
  on a + or -    Enter shows or hides the lines under it
  i              page info: cost, what was left out
  q or Ctrl-c    quit            ?           this help
";

/// Compose the style for one run: semantic flags, never page colors.
fn style_for(app: &App, style: CellStyle, link: Option<usize>) -> Style {
    let mut out = app.theme.text;
    if style.code {
        out = out.patch(app.theme.code);
    }
    if style.placeholder {
        out = out.patch(app.theme.placeholder);
    }
    if style.image {
        out = out.patch(app.theme.image);
    }
    if style.border {
        out = out.patch(app.theme.border);
    }
    if style.heading {
        out = out.patch(app.theme.heading);
    } else if style.bold {
        out = out.patch(app.theme.strong);
    }
    if style.italic {
        out = out.patch(app.theme.emphasis);
    }
    if style.mono {
        // Monospace asks for a font, and w3b marks it with a dark slab and
        // light text; render_page carries that background to the end of the line.
        // Applied last of the text styles so it wins over the colour for <code>.
        out = out.patch(app.theme.monospace);
    }
    if style.muted {
        // A muted thing is dimmed on purpose, and the slab only says that the text is
        // monospace: on a monospace line the dimming still has to show, which is what
        // keeps a JSON body's line numbers and its braces out of the reader's way.
        out = out.patch(app.theme.muted);
    }
    if style.disclosure {
        // The summary of a <details> is something to open rather than somewhere to
        // go, so it takes its own style instead of the link color.
        out = out.patch(app.theme.disclosure);
    }
    // JSON parts are coloured over the monospace slab: the slab says what shape the
    // body has, the colours say what its pieces are.
    match style.json {
        Role::Key => out = out.patch(app.theme.json_key),
        Role::String => out = out.patch(app.theme.json_string),
        Role::Number => out = out.patch(app.theme.json_number),
        Role::Literal => out = out.patch(app.theme.json_literal),
        // Braces, commas and colons are structure rather than content, so they keep
        // the reader's eye out of the way with the muted colour.
        Role::Punct => out = out.patch(app.theme.muted),
        Role::Plain => {}
    }
    if let Some(index) = link {
        let resolved = &app.page.links[index];
        let visited = app.visited.contains(resolved.href.as_str())
            || matches!(&resolved.follow, crate::page::Follow::Url(url) if app.visited.contains(url.as_str()));
        // A link off the site is coloured by where it goes rather than by whether
        // it has been read: the colour is the caution, and a visit does not undo it.
        out = out.patch(if resolved.external {
            app.theme.external
        } else if visited {
            app.theme.visited
        } else {
            app.theme.link
        });
        if style.heading {
            // A heading that happens to be a link keeps the heading's colours; all
            // the link contributes is its underline.
            out = out.patch(app.theme.heading);
        }
    }
    if style.image && link.is_some() {
        // A linked image keeps the placeholder's own colours: black ink on the
        // image background, with the underline from the link and none of its blue.
        out = out.patch(app.theme.linked_image);
    }
    out
}

/// Truncate to `width` display columns, adding an ellipsis when cut.
pub fn fit<'a>(text: &'a str, width: usize) -> Cow<'a, str> {
    if text.width() <= width {
        return Cow::Borrowed(text);
    }
    if width == 0 {
        return Cow::Borrowed("");
    }
    let mut out = String::new();
    let mut w = 0usize;
    for g in text.graphemes(true) {
        let gw = g.width();
        if w + gw + 1 > width {
            break;
        }
        out.push_str(g);
        w += gw;
    }
    out.push('\u{2026}');
    Cow::Owned(out)
}

/// Draw the laid out rows of the current page.
pub fn render_page(app: &App, area: Rect, buf: &mut Buffer) {
    let top = app.view.scroll;
    let hscroll = app.view.hscroll;
    let pad = app.padding_x();
    let visible_height = usize::from(area.height);
    // The link the cursor stands on, so it can get a background of its own.
    // The highlight covers the whole link element, not the word the cursor
    // happens to sit on: one link is one target even when it wraps rows. The
    // summary of a <details> is a target too - one that opens instead of going -
    // and gets the same cue.
    let hovered: Vec<(usize, usize, usize)> = app
        .page
        .layout
        .link_at(app.view.cursor.row, app.view.cursor.col)
        .map(|link| {
            app.page.layout.links.iter().filter(|p| p.link == link).map(|p| (p.row, p.start, p.end)).collect()
        })
        .or_else(|| {
            app.page.layout.disclosure_at(app.view.cursor.row, app.view.cursor.col).map(|d| {
                app.page
                    .layout
                    .rows
                    .iter()
                    .enumerate()
                    .flat_map(|(row, r)| {
                        r.runs
                            .iter()
                            .filter(move |run| run.disclosure == Some(d))
                            .map(move |run| (row, run.start, run.end()))
                    })
                    .collect()
            })
        })
        .unwrap_or_default();

    for (i, row) in app.page.layout.rows.iter().enumerate().skip(top).take(visible_height) {
        let y = area.y + (i - top) as u16;
        let is_cursor_row = i == app.view.cursor.row;

        let line_style = app.theme.text;

        // Clear the line first: stale pixels from a longer row must not remain.
        for x in 0..usize::from(area.width) {
            let cell = &mut buf[(area.x + x as u16, y)];
            cell.reset();
            cell.set_symbol(" ");
            cell.set_style(line_style);
            if is_cursor_row {
                cell.set_style(cell.style().patch(app.theme.cursor_line));
            }
        }

        let hits = app.matches.get(&i);
        for run in &row.runs {
            let style = style_for(app, run.style, run.link);
            let mut col = run.start;
            for g in run.text.graphemes(true) {
                let gw = g.width().max(1);
                let x = pad as isize + col as isize - hscroll as isize;
                if x >= 0 && (x as usize) < usize::from(area.width) {
                    let cell = &mut buf[(area.x + x as u16, y)];
                    cell.set_symbol(g);
                    let mut s = style;
                    if hovered.iter().any(|(row, start, end)| *row == i && col >= *start && col < *end) {
                        s = s.patch(app.theme.hovered_link);
                    }
                    if let Some(hits) = hits {
                        if hits.iter().any(|(start, end)| col >= *start && col < *end) {
                            s = s.patch(app.theme.search_match);
                        }
                    }
                    if is_cursor_row && app.view.cursor.col >= col && app.view.cursor.col < col + gw {
                        s = s.patch(app.theme.cursor);
                    }
                    cell.set_style(s);
                }
                col += gw;
            }
        }

        // The slab carries one blank cell on each side of the monospace text, so
        // code never sits flush against its own background. The bounds are taken
        // from the glyphs themselves: a run of spaces marked monospace (the
        // separator before a <code>, say) does not widen the slab.
        let mut first = usize::MAX;
        let mut last = 0usize;
        for run in row.runs.iter().filter(|r| r.style.mono) {
            let mut col = run.start;
            for g in run.text.graphemes(true) {
                let gw = g.width().max(1);
                if !g.trim().is_empty() {
                    first = first.min(col);
                    last = last.max(col + gw - 1);
                }
                col += gw;
            }
        }
        // A direct view of a file - JSON, plain text, a stylesheet - is nothing but
        // its own text, so there is no prose to set the code apart from: the slab is
        // the whole text area rather than the outline of the glyphs on one line.
        let whole = app.page.file_view;
        let from = if whole { 0 } else { first.saturating_sub(1) };
        let to = if whole { usize::from(area.width).saturating_sub(2 * pad + 1) } else { last + 1 };
        if whole || first != usize::MAX {
            for col in from..=to {
                let x = pad as isize + col as isize - hscroll as isize;
                if x >= 0 && (x as usize) < usize::from(area.width) {
                    let cell = &mut buf[(area.x + x as u16, y)];
                    // In a page, a glyph that is not part of a monospace run is prose
                    // next to the code and must not be painted over. In a file view there
                    // is no prose: every cell of the row is the file's own text.
                    if !whole {
                        let in_mono = row.runs.iter().any(|r| {
                            r.style.mono && {
                                let end =
                                    r.start + r.text.graphemes(true).map(|g| g.width().max(1)).sum::<usize>();
                                col >= r.start && col < end
                            }
                        });
                        if !cell.symbol().trim().is_empty() && !in_mono {
                            continue;
                        }
                    }
                    // A search hit is a mark on the page rather than a property of the
                    // text, so the slab goes under it, not over it.
                    if let Some(hits) = hits {
                        if hits.iter().any(|(start, end)| col >= *start && col < *end) {
                            continue;
                        }
                    }
                    // The cell under the cursor is the app's own mark: on a wall of
                    // monospace text it is the one thing that has to stand out.
                    if is_cursor_row && app.view.cursor.col == col {
                        continue;
                    }
                    // The slab is a background and nothing else: whatever colour a glyph
                    // was painted with - a link, a part of a JSON body - survives it.
                    let mut slab = app.theme.monospace;
                    if let Some(fg) = cell.style().fg {
                        slab = slab.patch(Style::default().fg(fg));
                    }
                    cell.set_style(cell.style().patch(slab));
                }
            }
        }

        // The mark of a visual selection goes over everything above, including the
        // monospace slab: it says which cells y would take, so it has to be legible
        // wherever it lands. The cell under the cursor is left out - it already has
        // the cursor's own colours, and the two marks side by side would read as one.
        if let Some((start, end)) = app.visual_span(i) {
            for col in start..end {
                let x = pad as isize + col as isize - hscroll as isize;
                if x < 0 || (x as usize) >= usize::from(area.width) {
                    continue;
                }
                if is_cursor_row && app.view.cursor.col == col {
                    continue;
                }
                let cell = &mut buf[(area.x + x as u16, y)];
                let patched = cell.style().patch(app.theme.selection);
                cell.set_style(patched);
            }
        }
    }

    // The cursor is drawn last and unconditionally: it must be visible on a
    // blank line, in the padding, and past the end of a short row.
    if app.view.cursor.row >= top && app.view.cursor.row < top + visible_height {
        let y = area.y + (app.view.cursor.row - top) as u16;
        let x = pad as isize + app.view.cursor.col as isize - hscroll as isize;
        if x >= 0 && (x as usize) < usize::from(area.width) {
            let cell = &mut buf[(area.x + x as u16, y)];
            if cell.symbol().trim().is_empty() {
                cell.set_symbol(" ");
            }
            let patched = cell.style().patch(app.theme.cursor);
            cell.set_style(patched);
        }
    }

    if app.config.behavior.show_scrollbar {
        draw_scrollbar(app, area, buf);
    }
}

fn draw_scrollbar(app: &App, area: Rect, buf: &mut Buffer) {
    let x = area.x + area.width.saturating_sub(1);
    let total = app.page.row_count().max(1);
    let height = usize::from(area.height).max(1);
    let top = app.view.scroll;
    let thumb_start = top.saturating_mul(height) / total;
    let thumb_len = (height.saturating_mul(height) / total).max(1);
    for (i, cell_y) in (area.y..area.y + area.height).enumerate() {
        let cell = &mut buf[(x, cell_y)];
        cell.reset();
        cell.set_symbol("\u{2502}");
        cell.set_style(app.theme.scrollbar);
        if i >= thumb_start && i < thumb_start + thumb_len {
            cell.set_symbol("\u{2588}");
        }
    }
}

/// `[200 OK] url - title            42%`
pub fn render_status(app: &App, area: Rect, buf: &mut Buffer) {
    let status = app.status();
    let width = usize::from(area.width);
    let bar = app.theme.status.base;

    for x in 0..width {
        let cell = &mut buf[(area.x + x as u16, area.y)];
        cell.reset();
        cell.set_symbol(" ");
        cell.set_style(bar);
    }

    let mut left = status_text(&status);
    // A fetch in flight gets a braille spinner in front of the status word, with a
    // blank cell between the glyph and the word so they never merge into one blob.
    if app.is_loading() {
        left = format!(" {} {}", app.spinner_glyph(), left.trim_start());
    }
    let left_style = app.theme.status.for_status(status.kind(), status.code());
    let left_width = draw(buf, area, 0, &left, left_style);

    // Right hand side: how long the page took, how big it was, then the cursor's
    // position in the document. Either of the first two can be switched off, and the
    // size is missing entirely while a fetch is in flight, so the parts are collected
    // rather than fitted into one shape.
    let mut parts: Vec<String> = Vec::new();
    if app.config.behavior.show_load_time {
        if let Some(took) = app.load_time_text() {
            parts.push(took);
        }
    }
    if app.config.behavior.show_size {
        if let Some(size) = app.size_text() {
            parts.push(size);
        }
    }
    parts.push(format!("{:>3}%", app.percent()));
    let right = parts.join("  ");
    let right_style = app.theme.position.patch(bar);
    let right_start = width.saturating_sub(right.width());
    draw(buf, area, right_start, &right, right_style);

    let middle_width = right_start.saturating_sub(left_width + 1);
    // Each arm carries its finished style: the hover hint must keep its own
    // background, so it patches the bar rather than being patched by it.
    let bar_style = app.theme.status.base;
    let middle = match &app.message {
        Some(msg) => {
            let style = if msg.error { app.theme.status.error } else { app.theme.message };
            (Cow::Owned(msg.text.clone()), style.patch(bar_style))
        }
        // Marking text - or sitting on a mark that has been left standing: say how
        // much, because that is what the user wants to know before pressing y. It
        // outranks the hover hint, and so says at a glance that the mark is still there.
        None if app.marked() => {
            (Cow::Owned(app.visual_status().unwrap_or_default()), app.theme.message.patch(bar_style))
        }
        // Standing on a link: show where it goes.
        None if app.hover_target().is_some() => {
            let target = app.hover_target().unwrap_or_default();
            // The hint uses the same colours as the link highlighted under the
            // cursor, so the pair reads as one thing.
            (
                Cow::Owned(format!("-> {}", fit(&target, middle_width.saturating_sub(3)))),
                bar_style.patch(app.theme.hovered_link),
            )
        }
        None => {
            // Leave a blank column between the middle and the right segment.
            let room = middle_width.saturating_sub(1);
            // The url is the part worth keeping whole, so the title gives way
            // first - and disappears rather than becoming a stub.
            let url = fit(app.url(), room);
            let mut text = String::from(url.as_ref());
            let left_over = room.saturating_sub(url.width());
            if app.config.behavior.show_title_in_status && left_over >= 3 + 6 {
                text.push_str(" \u{2014} ");
                text.push_str(&fit(app.title(), left_over - 3));
            }
            (Cow::Owned(text), app.theme.url.patch(bar_style))
        }
    };
    draw(buf, area, left_width + 1, &middle.0, middle.1);
}

fn status_text(status: &Status) -> String {
    format!(" {} ", status.text())
}

/// Write `text` starting at column `x`. Returns the column after the text.
fn draw(buf: &mut Buffer, area: Rect, x: usize, text: &str, style: Style) -> usize {
    let width = usize::from(area.width);
    if x >= width {
        return x;
    }
    let mut col = x;
    for g in text.graphemes(true) {
        let gw = g.width().max(1);
        if col + gw > width {
            break;
        }
        let cell = &mut buf[(area.x + col as u16, area.y)];
        cell.set_symbol(g);
        cell.set_style(style);
        col += gw;
    }
    col
}

/// The `:open ...` / `/query ...` line, drawn on its own row above the status.
pub fn render_prompt(app: &App, area: Rect, buf: &mut Buffer) {
    let prefix = match app.mode {
        Mode::Open => ":open ",
        Mode::Search => "/",
        _ => " ",
    };
    for x in 0..usize::from(area.width) {
        let cell = &mut buf[(area.x + x as u16, area.y)];
        cell.reset();
        cell.set_symbol(" ");
        cell.set_style(app.theme.status.base);
    }
    draw(buf, area, 0, prefix, app.theme.prompt.patch(app.theme.status.base));
    let text = fit(&app.input, usize::from(area.width).saturating_sub(prefix.len()));
    draw(buf, area, prefix.len(), &text, app.theme.url.patch(app.theme.status.base));
}

/// Full screen frame: page, optional prompt row, status row, help overlay.
/// Where the terminal caret belongs: in the status line while a prompt is open,
/// or over the form field being typed into. `None` means no caret.
pub fn cursor_screen_pos(app: &App, area: Rect) -> Option<(u16, u16)> {
    let width = usize::from(area.width);
    let page_height = area.height.saturating_sub(1);
    match app.mode {
        Mode::Open | Mode::Search => {
            let prefix = if app.mode == Mode::Open { ":open ".chars().count() } else { 1 };
            let row = area.y + page_height.saturating_sub(1);
            Some(((prefix + app.input_pos) as u16, row))
        }
        Mode::Field => {
            let x = app.padding_x() + app.view.cursor.col.saturating_sub(app.view.hscroll);
            let y = usize::from(area.y) + app.view.cursor.row.saturating_sub(app.view.scroll);
            if x < width && y < usize::from(page_height) {
                Some((x as u16, y as u16))
            } else {
                None
            }
        }
        Mode::Normal | Mode::Help | Mode::Info | Mode::Visual => None,
    }
}

pub fn draw_all(app: &App, area: Rect, buf: &mut Buffer) {
    let status_row = 1u16;
    let prompt_rows = if matches!(app.mode, Mode::Open | Mode::Search) { 1 } else { 0 };
    let page_height = area.height.saturating_sub(status_row + prompt_rows);

    let page_area = Rect { x: area.x, y: area.y, width: area.width, height: page_height };
    render_page(app, page_area, buf);

    if prompt_rows == 1 {
        let prompt_area = Rect { x: area.x, y: area.y + page_height, width: area.width, height: 1 };
        render_prompt(app, prompt_area, buf);
    }

    let status_area =
        Rect { x: area.x, y: area.y + page_height + prompt_rows, width: area.width, height: status_row };
    render_status(app, status_area, buf);

    // The `g` menu is a hint, not a blocker: the page stays readable behind it.
    if app.goto_menu_open() {
        render_goto_menu(app, page_area, buf);
    }

    if app.mode == Mode::Help {
        render_help(area, buf, app);
    }
    if app.mode == Mode::Info {
        render_info(area, buf, app);
    }
}

/// The which-key menu shown while `g` waits for its command.
fn render_goto_menu(app: &App, area: Rect, buf: &mut Buffer) {
    let entries = crate::app::GOTO_MENU;
    let rows = entries.len().div_ceil(2);
    let column = ((usize::from(area.width).saturating_sub(6)) / 2).clamp(10, 24);
    let width = (column * 2 + 4).min(usize::from(area.width));
    let height = (rows as u16 + 2).min(area.height);
    let box_area =
        Rect { x: area.x, y: area.y + area.height.saturating_sub(height), width: width as u16, height };
    Clear.render(box_area, buf);
    Block::bordered()
        .border_style(app.theme.pane_border)
        .title(Span::styled(" go to ", app.theme.pane))
        .render(box_area, buf);
    for (i, (key, what)) in entries.iter().enumerate() {
        let (row, col) = (i / 2, i % 2);
        let y = box_area.y + 1 + row as u16;
        let x = usize::from(box_area.x) + 2 + col * column;
        let line = Rect { x: x as u16, y, width: (column - 1) as u16, height: 1 };
        draw(buf, line, 0, key, app.theme.link.bold());
        draw(buf, line, 3, what, app.theme.pane);
    }
}

fn render_help(area: Rect, buf: &mut Buffer, app: &App) {
    let lines: Vec<String> = HELP.lines().map(str::to_string).collect();
    render_box(app, area, buf, "w3b", &lines);
}

/// A bordered pane, centred in the terminal, holding plain text lines: the help
/// screen and the page info pane are the same box with different words in them.
/// It floats over the page in the config's pane colours - bright ink, no
/// background - so the words are the terminal's own background inside a frame.
fn render_box(app: &App, area: Rect, buf: &mut Buffer, title: &str, lines: &[String]) {
    let height = (lines.len() as u16 + 2).min(area.height);
    let longest = lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16 + 4;
    let width = longest.min(area.width.saturating_sub(2)).max(20);
    let width = width.min(area.width);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let box_area = Rect { x, y, width, height };
    Clear.render(box_area, buf);
    Block::bordered()
        .border_style(app.theme.pane_border)
        .title(Span::styled(format!(" {title} "), app.theme.pane))
        .render(box_area, buf);
    let inner = Rect { x: x + 1, y: y + 1, width: width.saturating_sub(2), height: height.saturating_sub(2) };
    for (i, text) in lines.iter().enumerate().take(usize::from(inner.height)) {
        draw_row(buf, inner, i, text, app.theme.pane);
    }
}

/// The info pane: what the last load cost, what came back, and what the page asked
/// for that w3b did not go and get. Every number was counted while the page was
/// read, so it is what that page really holds rather than an estimate.
fn render_info(area: Rect, buf: &mut Buffer, app: &App) {
    render_box(app, area, buf, "page info", &info_lines(app));
}

/// The lines of the info pane, as a public function so a test can read the numbers
/// without rebuilding a screen out of cells.
pub fn info_lines(app: &App) -> Vec<String> {
    let page = &app.page;
    let mut out = vec![
        format!(" url   {}", page.url),
        format!(" {}  {}", page.status.text(), status::human_size(page.bytes)),
    ];
    if let Some(notice) = &page.notice {
        out.push(format!("       {notice}"));
    }
    out.push(String::new());

    match page.timings {
        Some(t) => {
            out.push(format!(" time  {} ms for the whole page", t.total));
            out.push(format!(
                "        net {} + css {} + parse {} + layout {} ms",
                t.fetch, t.css, t.parse, t.layout
            ));
        }
        None => out.push(" time  nothing was fetched: a page w3b wrote itself".to_string()),
    }
    out.push(format!(" got   1 page, {} of {} stylesheets", page.css.read, page.css.found));

    // What a browser would have fetched and w3b left alone, in the order a reader
    // is likely to care about.
    let ignored = page.doc.ignored;
    let missed = page.css.found.saturating_sub(page.css.read);
    let mut parts: Vec<String> = Vec::new();
    if ignored.images > 0 {
        parts.push(count(ignored.images, "image"));
    }
    if ignored.video_audio > 0 {
        parts.push(format!("{} video/audio", ignored.video_audio));
    }
    if missed > 0 {
        parts.push(count(missed, "stylesheet"));
    }
    if ignored.scripts > 0 {
        parts.push(count(ignored.scripts, "script"));
    }
    if ignored.frames > 0 {
        parts.push(count(ignored.frames, "frame"));
    }
    if ignored.markup > 0 {
        parts.push(format!("{} svg/canvas/markup", ignored.markup));
    }
    if ignored.links > 0 {
        parts.push(count(ignored.links, "other link"));
    }
    if parts.is_empty() {
        out.push(" left  nothing: w3b asked for everything this page did".to_string());
    } else {
        for (n, chunk) in wrap_list(&parts.join(", "), 46).into_iter().enumerate() {
            out.push(format!(" {}  {}", if n == 0 { "left" } else { "    " }, chunk));
        }
    }

    let off_site = page.links.iter().filter(|link| link.external).count();
    out.push(String::new());
    out.push(format!(
        " here  {}, {} ({off_site} off-site)",
        count(page.row_count(), "line"),
        count(page.links.len(), "link")
    ));
    out.push(format!(
        "        {}, {}, {}, {}",
        count(page.doc.tables.len(), "table"),
        count(page.doc.fields.len(), "field"),
        count(page.doc.disclosures.len(), "disclosure"),
        count(page.doc.media.len(), "media label")
    ));
    out.push(String::new());
    out.push(" any key closes".to_string());
    out
}

/// A count with its word, so the pane reads `1 line` and not `1 lines`.
fn count(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// Split a comma separated list into pieces that fit the box, never mid-word.
fn wrap_list(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for part in text.split(", ") {
        if !line.is_empty() && line.chars().count() + part.chars().count() + 2 > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push_str(", ");
        }
        line.push_str(part);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn draw_row(buf: &mut Buffer, area: Rect, row: usize, text: &str, style: Style) {
    let width = usize::from(area.width);
    let truncated = fit(text, width);
    for (col, g) in truncated.graphemes(true).enumerate() {
        let cell = &mut buf[(area.x + col as u16, area.y + row as u16)];
        cell.set_symbol(g);
        cell.set_style(style);
    }
}

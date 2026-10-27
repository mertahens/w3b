//! Painting the laid-out rows, the status line and prompts.
use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Widget};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Mode};
use crate::layout::CellStyle;
use crate::status::Status;

pub const HELP: &str = "\
w3b keys  (i or ? toggles this)
  j / k          down, up        gg / G      top, bottom
  h / l          left, right     0 ^ $       line edges
  Ctrl-d / Ctrl-u  half page     PgDn/PgUp   full page
  Enter          follow or edit  r           reload
  Tab            next link       Shift-Tab   prev. link
  o or :         open url        /           search
  n / N          next/prev hit   Esc         cancel
  [ or Backspace back            ]           forward
  y              link target     w           toggle wrap
  g then ...     go-to menu
  in a field     typing edits, Esc or Enter is done
  q or Ctrl-c    quit            i or ?      this help
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
    if style.muted {
        out = out.patch(app.theme.muted);
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
    if let Some(index) = link {
        let visited = app.visited.contains(app.page.links[index].href.as_str())
            || matches!(&app.page.links[index].follow, crate::page::Follow::Url(url) if app.visited.contains(url.as_str()));
        out = out.patch(if visited { app.theme.visited } else { app.theme.link });
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
    // happens to sit on: one link is one target even when it wraps rows.
    let hovered: Vec<(usize, usize, usize)> = app
        .page
        .layout
        .link_at(app.view.cursor.row, app.view.cursor.col)
        .map(|link| {
            app.page.layout.links.iter().filter(|p| p.link == link).map(|p| (p.row, p.start, p.end)).collect()
        })
        .unwrap_or_default();

    for (i, row) in app.page.layout.rows.iter().enumerate().skip(top).take(visible_height) {
        let y = area.y + (i - top) as u16;
        let is_cursor_row = i == app.view.cursor.row;

        let line_style = app.theme.text;
        // A monospace run carries its background to the end of the line, which is
        // what makes a <pre> block read as a block of code.  Only the rows that
        // hold monospace text get it, so no empty row below a block is shaded.
        let mono_from = row.runs.iter().find(|r| r.style.mono).map(|r| r.start);

        // Clear the line first: stale pixels from a longer row must not remain.
        for x in 0..usize::from(area.width) {
            let cell = &mut buf[(area.x + x as u16, y)];
            cell.reset();
            cell.set_symbol(" ");
            cell.set_style(line_style);
            if is_cursor_row {
                cell.set_style(cell.style().patch(app.theme.cursor_line));
            }
            let col = x as isize - pad as isize + hscroll as isize;
            if let Some(from) = mono_from {
                if col >= from as isize {
                    cell.set_style(cell.style().patch(app.theme.monospace));
                }
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
    // A fetch in flight gets a braille spinner in front of the status word.
    if app.is_loading() {
        left = format!(" {}{}", app.spinner_glyph(), left.trim_start());
    }
    let left_style = app.theme.status.for_status(status.kind(), status.code());
    let left_width = draw(buf, area, 0, &left, left_style);

    // Right hand side: how long the page took to load, then the cursor's
    // position in the document.
    let percent = format!("{:>3}%", app.percent());
    let right = match (app.config.behavior.show_load_time, app.load_time_text()) {
        (true, Some(took)) => format!("{took}  {percent}"),
        _ => percent,
    };
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
        Mode::Normal | Mode::Help => None,
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
    Block::bordered().border_style(app.theme.border).title(" go to ").render(box_area, buf);
    let base = app.theme.status.base;
    for (i, (key, what)) in entries.iter().enumerate() {
        let (row, col) = (i / 2, i % 2);
        let y = box_area.y + 1 + row as u16;
        let x = usize::from(box_area.x) + 2 + col * column;
        let line = Rect { x: x as u16, y, width: (column - 1) as u16, height: 1 };
        draw(buf, line, 0, key, app.theme.link.patch(base).bold());
        draw(buf, line, 3, what, app.theme.text.patch(base));
    }
}

fn render_help(area: Rect, buf: &mut Buffer, app: &App) {
    let lines: Vec<Line> = HELP.lines().map(Line::from).collect();
    let height = (lines.len() as u16 + 2).min(area.height);
    let longest = HELP.lines().map(str::width).max().unwrap_or(0) as u16 + 4;
    let width = longest.min(area.width.saturating_sub(2)).max(20);
    let width = width.min(area.width);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let box_area = Rect { x, y, width, height };
    Clear.render(box_area, buf);
    Block::bordered().border_style(app.theme.border).title(" w3b ").render(box_area, buf);
    let inner = Rect { x: x + 1, y: y + 1, width: width.saturating_sub(2), height: height.saturating_sub(2) };
    for (i, line) in lines.iter().enumerate().take(usize::from(inner.height)) {
        let text = line.spans.iter().map(|s| s.content.as_ref()).collect::<String>();
        draw_row(buf, inner, i, &text, app.theme.text.patch(app.theme.status.base));
    }
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

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
  Enter          follow link     r           reload
  Tab            next link       Shift-Tab   prev. link
  o or :         open url        /           search
  n / N          next/prev hit   Esc         cancel
  [ or Backspace back            ]           forward
  y              link target     w           toggle wrap
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
    let hovered = app.placement_at(app.view.cursor.row, app.view.cursor.col).map(|p| (p.row, p.start, p.end));

    for (i, row) in app.page.layout.rows.iter().enumerate().skip(top).take(visible_height) {
        let y = area.y + (i - top) as u16;
        let is_cursor_row = i == app.view.cursor.row;

        // A monospace block greys the whole line, the way a code block reads in
        // a terminal. Table rows keep their grey inside the cells instead.
        let line_style =
            if row.runs.iter().any(|run| run.style.mono) && !row.runs.iter().any(|run| run.style.border) {
                app.theme.text.patch(app.theme.monospace)
            } else {
                app.theme.text
            };

        // Clear the line first: stale pixels from a longer row must not remain.
        for x in 0..usize::from(area.width) {
            let cell = &mut buf[(area.x + x as u16, y)];
            cell.reset();
            cell.set_symbol(" ");
            cell.set_style(line_style);
            if is_cursor_row {
                let patched = cell.style().patch(app.theme.cursor_line);
                cell.set_style(patched);
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
                    if let Some((row, start, end)) = hovered {
                        if row == i && col >= start && col < end {
                            s = s.patch(app.theme.hovered_link);
                        }
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

    let left = status_text(&status);
    let left_style = app.theme.status.for_status(status.kind(), status.code());
    let left_width = draw(buf, area, 0, &left, left_style);

    let right = format!("{:>3}%", app.percent());
    let right_style = app.theme.position.patch(bar);
    let right_start = width.saturating_sub(right.width());
    draw(buf, area, right_start, &right, right_style);

    let middle_width = right_start.saturating_sub(left_width + 1);
    let middle = match &app.message {
        Some(msg) => {
            (Cow::Owned(msg.text.clone()), if msg.error { app.theme.status.error } else { app.theme.message })
        }
        // Standing on a link: show where it goes.
        None if app.hover_target().is_some() => {
            let target = app.hover_target().unwrap_or_default();
            (Cow::Owned(format!("-> {}", fit(&target, middle_width.saturating_sub(3)))), app.theme.link)
        }
        None => {
            let url = fit(app.url(), middle_width / 2);
            let mut text = String::from(url.as_ref());
            if app.config.behavior.show_title_in_status && !app.page.title.trim().is_empty() {
                let title_width = middle_width.saturating_sub(url.width() + 3);
                text.push_str(" \u{2014} ");
                text.push_str(&fit(app.title(), title_width));
            }
            (Cow::Owned(text), app.theme.url)
        }
    };
    draw(buf, area, left_width + 1, &middle.0, middle.1.patch(bar));
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

    if app.mode == Mode::Help {
        render_help(area, buf, app);
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

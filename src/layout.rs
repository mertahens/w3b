//! Flow blocks -> fixed-width terminal rows, with link geometry recorded.
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::dom::{Block, BlockKind, Doc, Piece};

/// Style of one run of a row. Semantic only - the colors come from the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub muted: bool,
    pub image: bool,
    /// Box-drawing character: table rules and borders.
    pub border: bool,
    pub heading: bool,
    pub placeholder: bool,
    /// Monospace font: the renderer greys the whole line.
    pub mono: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    /// Display column where the run starts in its row.
    pub start: usize,
    pub style: CellStyle,
    pub link: Option<usize>,
    /// Set for the text of an editable form field (index into `Doc::fields`).
    pub field: Option<usize>,
}

impl Run {
    pub fn width(&self) -> usize {
        self.text.width()
    }

    pub fn end(&self) -> usize {
        self.start + self.width()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    pub runs: Vec<Run>,
    pub width: usize,
}

impl Row {
    pub fn is_blank(&self) -> bool {
        self.runs.is_empty() || self.runs.iter().all(|r| r.text.trim().is_empty())
    }

    /// The run covering a display column, if any.
    pub fn run_at(&self, col: usize) -> Option<&Run> {
        self.runs.iter().find(|r| r.start <= col && col < r.end())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkPlacement {
    pub link: usize,
    pub row: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub rows: Vec<Row>,
    pub links: Vec<LinkPlacement>,
    pub width: usize,
    /// Row where each block of the document starts, indexed by block index.
    pub block_rows: Vec<usize>,
}

impl Layout {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Link occupying this display cell, if any.
    pub fn link_at(&self, row: usize, col: usize) -> Option<usize> {
        self.links.iter().find(|p| p.row == row && col >= p.start && col < p.end).map(|p| p.link)
    }

    /// The form field whose text covers this cell, if any.
    pub fn field_at(&self, row: usize, col: usize) -> Option<usize> {
        self.rows.get(row)?.runs.iter().find_map(|run| {
            if run.start <= col && col < run.end() {
                run.field
            } else {
                None
            }
        })
    }

    /// The row a field's text ends on and the column just past it, which is
    /// where the caret belongs while the field is being typed into.
    pub fn field_end(&self, index: usize) -> Option<(usize, usize)> {
        self.rows.iter().enumerate().rev().find_map(|(row, r)| {
            r.runs.iter().rev().find(|run| run.field == Some(index)).map(|run| (row, run.end()))
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct Options {
    width: usize,
    tab_width: usize,
    wrap: bool,
}

pub fn layout(doc: &Doc, width: usize, tab_width: usize, wrap: bool) -> Layout {
    let width = width.max(1);
    let opts = Options { width, tab_width, wrap };
    let mut out = Layout { rows: Vec::new(), links: Vec::new(), width, block_rows: Vec::new() };

    let mut previous: Option<BlockKind> = None;
    for block in &doc.blocks {
        // Blank line between blocks, except inside a <pre> run or between two
        // list items, where the marker already separates the lines.
        let same_group = matches!(
            (previous, block.kind),
            (Some(BlockKind::Pre), BlockKind::Pre) | (Some(BlockKind::Item { .. }), BlockKind::Item { .. })
        );
        let separate = !same_group;
        if separate && !out.rows.is_empty() && out.rows.last().map(Row::is_blank) == Some(false) {
            out.rows.push(Row::default());
        }
        previous = Some(block.kind);
        out.block_rows.push(out.rows.len());
        let indent = block.indent as usize * 2;
        match block.kind {
            BlockKind::Divider => out.rows.push(divider_row(width)),
            BlockKind::Table { index } => layout_table(&mut out, doc, index, indent, opts),
            BlockKind::Pre => layout_pre(&mut out, block, indent, opts),
            BlockKind::Item { depth, ordered, index } => {
                let marker = if ordered { format!("{}.", index + 1) } else { "\u{2022}".to_string() };
                let pad = " ".repeat(indent + (depth as usize - 1) * 2);
                let prefix = pad.clone() + &marker + " ";
                let cont = pad + &" ".repeat(marker.width() + 1);
                layout_flow_inner(&mut out, block, prefix, cont, opts);
            }
            BlockKind::Heading(_) => layout_flow(&mut out, block, " ".repeat(indent), opts),
            _ => layout_flow(&mut out, block, " ".repeat(indent), opts),
        }
    }

    // A trailing blank row keeps the last line readable when the cursor is on it.
    if out.rows.is_empty() {
        out.rows.push(Row::default());
    }
    // Widest row: with wrap off this is how far the view can scroll sideways.
    out.width = out.rows.iter().map(|r| r.width).max().unwrap_or(1).max(1);
    out
}

fn divider_row(width: usize) -> Row {
    let text = "\u{2500}".repeat(width);
    Row {
        runs: vec![Run {
            text,
            start: 0,
            style: CellStyle { muted: true, ..CellStyle::default() },
            link: None,
            field: None,
        }],
        width,
    }
}

fn layout_pre(out: &mut Layout, block: &Block, indent: usize, opts: Options) {
    let prefix = " ".repeat(indent + 2);
    let indent_run = Run {
        text: prefix,
        start: 0,
        style: CellStyle { code: true, ..CellStyle::default() },
        link: None,
        field: None,
    };
    let col = indent_run.width();
    let mut row = Row { runs: vec![indent_run], width: col };
    for piece in &block.pieces {
        let style = style_of(piece, false);
        let text = expand_tabs(&piece.text, opts.tab_width);
        let run = Run { text, start: col, style, link: piece.link, field: piece.field };
        if let Some(link) = piece.link {
            out.links.push(LinkPlacement { link, row: out.rows.len(), start: col, end: run.end() });
        }
        row.width = row.width.max(run.end());
        row.runs.push(run);
    }
    trim_and_push(out, row);
}

fn layout_flow(out: &mut Layout, block: &Block, prefix: String, opts: Options) {
    let cont = prefix.clone();
    layout_flow_inner(out, block, prefix, cont, opts)
}

fn layout_flow_inner(out: &mut Layout, block: &Block, prefix: String, cont: String, opts: Options) {
    let cont_run = Run { text: cont.clone(), start: 0, style: CellStyle::default(), link: None, field: None };
    let indent_run =
        Run { text: prefix.clone(), start: 0, style: CellStyle::default(), link: None, field: None };
    let mut row = Row { runs: vec![indent_run.clone()], width: prefix.width() };
    let mut col = prefix.width();

    for piece in &block.pieces {
        let style = style_of(piece, matches!(block.kind, BlockKind::Heading(_)));
        let text = expand_tabs(&piece.text, opts.tab_width);
        for token in tokens_for(text, piece.field.is_some()) {
            let token_width = token.text.width();
            if token.space {
                if !row.runs.is_empty() && col > prefix.width() {
                    col = push_run(
                        out,
                        &mut row,
                        Run { text: " ".into(), start: col, style, link: piece.link, field: piece.field },
                    );
                    continue;
                }
                continue; // never lead a row with a space
            }
            if opts.wrap {
                if col + token_width > opts.width && col > prefix.width() {
                    // word does not fit: start a new row with the marker removed
                    trim_and_push(out, std::mem::take(&mut row));
                    row = Row { runs: vec![cont_run.clone()], width: cont.width() };
                    col = cont.width();
                }
                if col + token_width > opts.width {
                    // single word wider than the screen: hard split on graphemes
                    let chunk = opts.width.saturating_sub(col).max(1);
                    let parts = hard_split(&token.text, chunk);
                    let last = parts.len().saturating_sub(1);
                    for (i, part) in parts.into_iter().enumerate() {
                        col = push_run(
                            out,
                            &mut row,
                            Run { text: part, start: col, style, link: piece.link, field: piece.field },
                        );
                        if i < last {
                            trim_and_push(out, std::mem::take(&mut row));
                            row = Row { runs: vec![cont_run.clone()], width: cont.width() };
                            col = cont.width();
                        }
                    }
                    continue;
                }
            }
            col = push_run(
                out,
                &mut row,
                Run { text: token.text, start: col, style, link: piece.link, field: piece.field },
            );
        }
    }
    trim_and_push(out, row);
}

fn push_run(out: &mut Layout, row: &mut Row, run: Run) -> usize {
    let end = run.end();
    if let Some(link) = run.link {
        out.links.push(LinkPlacement { link, row: out.rows.len(), start: run.start, end });
    }
    row.width = row.width.max(end);
    row.runs.push(run);
    end
}

fn trim_and_push(out: &mut Layout, mut row: Row) {
    while let Some(last) = row.runs.last() {
        if last.link.is_none() && last.text.trim().is_empty() {
            row.runs.pop();
        } else {
            break;
        }
    }
    row.runs.retain(|r| !r.text.is_empty());
    if row.runs.is_empty() {
        return;
    }
    row.width = row.runs.iter().map(Run::end).max().unwrap_or(0);
    out.rows.push(row);
}

fn style_of(piece: &Piece, heading: bool) -> CellStyle {
    CellStyle {
        bold: piece.bold || heading,
        italic: piece.italic,
        code: piece.code,
        muted: piece.muted,
        image: piece.image,
        border: false,
        heading,
        placeholder: false,
        mono: piece.mono,
    }
}

fn expand_tabs(text: &str, tab_width: usize) -> String {
    if !text.contains('\t') {
        return text.to_string();
    }
    text.split('\t').collect::<Vec<_>>().join(&" ".repeat(tab_width.max(1)))
}

struct Token {
    text: String,
    space: bool,
}

/// Split a piece's text into words and whitespace runs, so wrapping can look
/// for break points across piece boundaries.
/// Tokens for a piece. A form field is one unbreakable token: its brackets and
/// its value must not end up on separate rows.
fn tokens_for(text: String, atomic: bool) -> Vec<Token> {
    if atomic {
        vec![Token { text, space: false }]
    } else {
        tokenize(&text)
    }
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut current_space = false;
    for c in text.chars() {
        let space = c.is_whitespace();
        if !current.is_empty() && space != current_space {
            out.push(Token { text: std::mem::take(&mut current), space: current_space });
        }
        current_space = space;
        current.push(c);
    }
    if !current.is_empty() {
        out.push(Token { text: current, space: current_space });
    }
    out
}

fn hard_split(text: &str, chunk: usize) -> Vec<String> {
    let chunk = chunk.max(1);
    let mut out = Vec::new();
    let mut part = String::new();
    let mut w = 0usize;
    for g in text.graphemes(true) {
        let gw = g.width().max(1);
        if w + gw > chunk && !part.is_empty() {
            out.push(std::mem::take(&mut part));
            w = 0;
        }
        part.push_str(g);
        w += gw;
    }
    if !part.is_empty() {
        out.push(part);
    }
    out
}

// --- tables ---------------------------------------------------------------

/// One table row drawn as a grid: `| cell | cell |` with box-drawing rules.
fn layout_table(out: &mut Layout, doc: &Doc, index: usize, indent: usize, opts: Options) {
    let Some(table) = doc.tables.get(index) else { return };
    let cols = table.rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return;
    }
    let pad = " ".repeat(indent);
    // Too narrow for a grid: fall back to `a | b` lines that can wrap.
    if opts.width < indent + cols + 6 {
        table_fallback(out, table, indent, opts);
        return;
    }
    let avail = opts.width - indent - (cols + 1);
    let mut natural = vec![1usize; cols];
    for row in &table.rows {
        for (c, cell) in row.iter().enumerate() {
            natural[c] = natural[c].max(cell.text_width().max(1));
        }
    }
    // One blank column on each side of the cell text, when there is room.
    let gutter = if avail >= natural.iter().sum::<usize>() + cols * 3 || avail >= cols * 3 { 2 } else { 0 };
    let widths = fit_columns(&natural, avail.saturating_sub(cols * gutter));
    let left_gutter = gutter / 2;
    let right_gutter = gutter - left_gutter;

    table_rule(out, &pad, &widths, gutter, '\u{250c}', '\u{252c}', '\u{2510}');
    for (ri, row) in table.rows.iter().enumerate() {
        if ri > 0 {
            table_rule(out, &pad, &widths, gutter, '\u{251c}', '\u{253c}', '\u{2524}');
        }
        let mut cells: Vec<Vec<Vec<Run>>> = Vec::with_capacity(cols);
        let mut aligns: Vec<crate::dom::Align> = Vec::with_capacity(cols);
        let mut bases: Vec<CellStyle> = Vec::with_capacity(cols);
        // Indexed by column on purpose: cells, widths, alignments and bases all
        // have to line up per column.
        #[allow(clippy::needless_range_loop)]
        for c in 0..cols {
            match row.get(c) {
                Some(cell) => {
                    aligns.push(cell.align);
                    bases.push(cell.pieces.first().map(|p| style_of(p, false)).unwrap_or_default());
                    cells.push(wrap_cell(cell, widths[c]));
                }
                None => {
                    aligns.push(crate::dom::Align::Left);
                    bases.push(CellStyle::default());
                    cells.push(vec![Vec::new()]);
                }
            }
        }
        let height = cells.iter().map(|lines| lines.len()).max().unwrap_or(1);
        for li in 0..height {
            let mut runs: Vec<Run> = Vec::new();
            let mut col = 0usize;
            if !pad.is_empty() {
                runs.push(Run {
                    text: pad.clone(),
                    start: 0,
                    style: CellStyle::default(),
                    link: None,
                    field: None,
                });
            }
            push_border(&mut runs, &mut col, '\u{2502}');
            for c in 0..cols {
                if left_gutter > 0 {
                    runs.push(Run {
                        text: " ".repeat(left_gutter),
                        start: col,
                        style: bases[c],
                        link: None,
                        field: None,
                    });
                    col += left_gutter;
                }
                let cell_start = col;
                let line = cells[c].get(li).cloned().unwrap_or_default();
                let used: usize = line.iter().map(|r| r.text.width()).sum();
                let slack = widths[c].saturating_sub(used);
                let left_pad = match aligns[c] {
                    crate::dom::Align::Right => slack,
                    crate::dom::Align::Center => slack / 2,
                    crate::dom::Align::Left => 0,
                };
                if left_pad > 0 {
                    runs.push(Run {
                        text: " ".repeat(left_pad),
                        start: col,
                        style: bases[c],
                        link: None,
                        field: None,
                    });
                    col += left_pad;
                }
                for run in line {
                    let width = run.text.width();
                    if let Some(link) = run.link {
                        out.links.push(LinkPlacement {
                            link,
                            row: out.rows.len(),
                            start: col,
                            end: col + width,
                        });
                    }
                    runs.push(Run {
                        text: run.text,
                        start: col,
                        style: run.style,
                        link: run.link,
                        field: run.field,
                    });
                    col += width;
                }
                if col < cell_start + widths[c] {
                    runs.push(Run {
                        text: " ".repeat(cell_start + widths[c] - col),
                        start: col,
                        style: bases[c],
                        link: None,
                        field: None,
                    });
                    col = cell_start + widths[c];
                }
                if right_gutter > 0 {
                    runs.push(Run {
                        text: " ".repeat(right_gutter),
                        start: col,
                        style: bases[c],
                        link: None,
                        field: None,
                    });
                    col += right_gutter;
                }
                push_border(&mut runs, &mut col, '\u{2502}');
            }
            out.rows.push(Row { runs, width: col });
        }
    }
    table_rule(out, &pad, &widths, gutter, '\u{2514}', '\u{2534}', '\u{2518}');
}

fn push_border(runs: &mut Vec<Run>, col: &mut usize, ch: char) {
    runs.push(Run {
        text: ch.to_string(),
        start: *col,
        style: CellStyle { border: true, ..CellStyle::default() },
        link: None,
        field: None,
    });
    *col += 1;
}

fn table_rule(
    out: &mut Layout,
    pad: &str,
    widths: &[usize],
    gutter: usize,
    left: char,
    mid: char,
    right: char,
) {
    let mut runs: Vec<Run> = Vec::new();
    let mut col = 0usize;
    if !pad.is_empty() {
        runs.push(Run {
            text: pad.to_string(),
            start: 0,
            style: CellStyle::default(),
            link: None,
            field: None,
        });
    }
    let mut ch = left;
    for (i, w) in widths.iter().enumerate() {
        push_border(&mut runs, &mut col, ch);
        runs.push(Run {
            text: "\u{2500}".repeat(*w + gutter),
            start: col,
            style: CellStyle { border: true, ..CellStyle::default() },
            link: None,
            field: None,
        });
        col += w + gutter;
        ch = if i + 1 == widths.len() { right } else { mid };
    }
    push_border(&mut runs, &mut col, right);
    out.rows.push(Row { runs, width: col });
}

/// Column widths that fit into `avail`: columns keep their natural width while
/// they can, and the widest column is shaved one character at a time.
fn fit_columns(natural: &[usize], avail: usize) -> Vec<usize> {
    let mut widths = natural.to_vec();
    let mut guard = 0;
    while widths.iter().sum::<usize>() > avail && guard < 65536 {
        guard += 1;
        match (0..widths.len()).filter(|c| widths[*c] > 1).max_by_key(|c| widths[*c]) {
            Some(c) => widths[c] -= 1,
            None => break,
        }
    }
    widths
}

/// Cell content wrapped to `width` lines, keeping per-piece styles and links.
fn wrap_cell(cell: &crate::dom::TableCell, width: usize) -> Vec<Vec<Run>> {
    let mut tokens: Vec<(String, CellStyle, Option<usize>, Option<usize>)> = Vec::new();
    for piece in &cell.pieces {
        let mut style = style_of(piece, false);
        if cell.header {
            style.bold = true;
        }
        for token in tokens_for(piece.text.clone(), piece.field.is_some()) {
            let text = if token.space { " ".to_string() } else { token.text.clone() };
            if text == " " && tokens.last().map(|t| t.0 == " ").unwrap_or(false) {
                continue;
            }
            tokens.push((text, style, piece.link, piece.field));
        }
    }
    while tokens.first().map(|t| t.0 == " ").unwrap_or(false) {
        tokens.remove(0);
    }

    let mut lines: Vec<Vec<Run>> = Vec::new();
    let mut line: Vec<Run> = Vec::new();
    let mut col = 0usize;
    for (text, style, link, field) in tokens {
        let w = text.width();
        if w == 0 {
            continue;
        }
        if w > width {
            // A word wider than the column: split it across the cell lines.
            let mut chunk = String::new();
            let mut cw = 0usize;
            for g in text.graphemes(true) {
                let gw = g.width().max(1);
                if cw + gw > width && cw > 0 {
                    line.push(Run { text: std::mem::take(&mut chunk), start: col, style, link, field });
                    col += cw;
                    lines.push(std::mem::take(&mut line));
                    cw = 0;
                }
                chunk.push_str(g);
                cw += gw;
            }
            if cw > 0 {
                line.push(Run { text: chunk, start: col, style, link, field });
                col += cw;
            }
            continue;
        }
        if col + w > width && col > 0 {
            lines.push(std::mem::take(&mut line));
            col = 0;
            if text == " " {
                continue;
            }
        }
        line.push(Run { text, start: col, style, link, field });
        col += w;
    }
    if !line.is_empty() {
        lines.push(line);
    }
    for l in lines.iter_mut() {
        while l.last().map(|r| r.text.trim().is_empty()).unwrap_or(false) {
            l.pop();
        }
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

/// Narrow-terminal fallback: one wrapped line per row, cells joined by ` | `.
fn table_fallback(out: &mut Layout, table: &crate::dom::Table, indent: usize, opts: Options) {
    for row in &table.rows {
        let mut pieces: Vec<Piece> = Vec::new();
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                pieces.push(Piece {
                    text: " | ".to_string(),
                    link: None,
                    mono: false,
                    bold: false,
                    italic: false,
                    code: false,
                    muted: true,
                    image: false,
                    field: None,
                });
            }
            pieces.extend(cell.pieces.iter().cloned());
        }
        let block = Block { kind: crate::dom::BlockKind::Paragraph, indent: 0, pieces };
        layout_flow(out, &block, " ".repeat(indent), opts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom;

    fn row_text(layout: &Layout, row: usize) -> String {
        layout.rows[row].runs.iter().map(|r| r.text.as_str()).collect::<String>().trim_end().to_string()
    }

    #[test]
    fn field_runs_are_addressable_by_cell() {
        let doc = dom::parse("<p>name <input name=q value=ann> end</p>");
        let out = layout(&doc, 40, 4, true);
        let (row, end) = out.field_end(0).expect("field placement");
        assert_eq!(row, 0);
        assert_eq!(out.field_at(row, end - 1), Some(0), "the last cell of the field");
        assert_eq!(out.field_at(row, end), None, "the caret parks just past it");
        assert!(out.field_at(row, 0).is_none(), "plain text is not a field");
        assert_eq!(row_text(&out, row), "name [ ann ] end");
    }

    #[test]
    fn a_field_keeps_its_marker_when_the_row_wraps() {
        let doc = dom::parse("<p>lorem ipsum dolor <input value=sit> consectetur</p>");
        let out = layout(&doc, 12, 4, true);
        assert!(out.len() > 1);
        let placed = out.rows.iter().filter(|r| r.runs.iter().any(|run| run.field.is_some())).count();
        assert_eq!(placed, 1, "the field must not be split across rows");
        assert!(out.field_end(0).is_some());
    }
}

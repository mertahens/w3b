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
    /// The text of a `<details>` summary: underlined as something to open.
    pub disclosure: bool,
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
    /// Set on the summary line of a `<details>` (index into `Doc::disclosures`):
    /// Enter opens or closes what follows it.
    pub disclosure: Option<usize>,
    /// Set on the label of an image or piece of media (index into `Doc::media`):
    /// `y` on it copies where the thing comes from.
    pub media: Option<usize>,
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

    /// The `<details>` whose summary text covers this cell, if any.
    pub fn disclosure_at(&self, row: usize, col: usize) -> Option<usize> {
        self.rows.get(row)?.runs.iter().find_map(|run| {
            if run.start <= col && col < run.end() {
                run.disclosure
            } else {
                None
            }
        })
    }

    /// The index into `Doc::media` of the image or media label under the cursor.
    pub fn media_at(&self, row: usize, col: usize) -> Option<usize> {
        self.rows.get(row)?.runs.iter().find_map(|run| {
            if run.start <= col && col < run.end() {
                run.media
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
        // A closed <details> keeps its content off the rows entirely - but the row
        // table stays indexed by block, so an anchor into it still resolves.
        if let Some(disclosure) = block.hidden_by {
            if !doc.is_open(disclosure) {
                out.block_rows.push(out.rows.len());
                continue;
            }
        }
        // Blank line between blocks, except inside a <pre> run or between two
        // list items, where the marker already separates the lines.
        let same_group = matches!(
            (previous, block.kind),
            (Some(BlockKind::Pre), BlockKind::Pre)
                | (
                    Some(BlockKind::Item { .. } | BlockKind::ItemCont { .. }),
                    BlockKind::Item { .. } | BlockKind::ItemCont { .. }
                )
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
            BlockKind::Item { depth, ordered, index, heading } => {
                let marker = if ordered { format!("{}.", index + 1) } else { "\u{2022}".to_string() };
                let pad = " ".repeat(indent + (depth as usize - 1) * 2);
                let prefix = pad.clone() + &marker + " ";
                let cont = pad + &" ".repeat(marker.width() + 1);
                layout_flow_inner(&mut out, block, prefix, cont, opts, heading > 0, None);
            }
            BlockKind::ItemCont { depth, marker } => {
                // Continuation of an open item: no marker, indent up to its text.
                let pad = " ".repeat(indent + (depth as usize - 1) * 2 + marker as usize + 1);
                layout_flow_inner(&mut out, block, pad.clone(), pad, opts, false, None);
            }
            BlockKind::Summary { index } => {
                // The `+`/`-` is drawn here rather than kept in the document: it says
                // what the line does, so opening the disclosure turns it around on
                // its own, and the lines it holds go in below the summary.
                let glyph = if doc.is_open(index) { "-" } else { "+" };
                let pad = " ".repeat(indent);
                let prefix = pad.clone() + glyph + " ";
                let cont = pad + "  ";
                layout_flow_inner(&mut out, block, prefix, cont, opts, false, Some(index));
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
            disclosure: None,
            media: None,
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
        disclosure: None,
        media: None,
    };
    let col = indent_run.width();
    let mut row = Row { runs: vec![indent_run], width: col };
    for piece in &block.pieces {
        let style = style_of(piece, false);
        let text = expand_tabs(&piece.text, opts.tab_width);
        let run = Run {
            text,
            start: col,
            style,
            link: piece.link,
            field: piece.field,
            disclosure: piece.disclosure,
            media: piece.media,
        };
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
    layout_flow_inner(out, block, prefix, cont, opts, matches!(block.kind, BlockKind::Heading(_)), None)
}

fn layout_flow_inner(
    out: &mut Layout,
    block: &Block,
    prefix: String,
    cont: String,
    opts: Options,
    heading: bool,
    marker: Option<usize>,
) {
    // A marker belongs to what it marks: the `+` of a disclosure is part of the
    // line Enter works on, and wears the style of the words next to it. The blank
    // columns in front of it are not part of the marker, so an underline - or any
    // other marking - starts at the marker itself.
    let marker_style = CellStyle { disclosure: marker.is_some(), ..CellStyle::default() };
    let cont_run = Run {
        text: cont.clone(),
        start: 0,
        style: CellStyle::default(),
        link: None,
        field: None,
        disclosure: marker,
        media: None,
    };
    let pad = prefix.chars().take_while(|c| *c == ' ').count();
    let mut indent_runs = Vec::new();
    if pad > 0 {
        indent_runs.push(Run {
            text: prefix[..pad].to_string(),
            start: 0,
            style: CellStyle::default(),
            link: None,
            field: None,
            disclosure: marker,
            media: None,
        });
    }
    if pad < prefix.len() {
        indent_runs.push(Run {
            text: prefix[pad..].to_string(),
            start: pad,
            style: marker_style,
            link: None,
            field: None,
            disclosure: marker,
            media: None,
        });
    }
    let mut row = Row { runs: indent_runs, width: prefix.width() };
    let mut col = prefix.width();

    let mut after_break = false;
    for piece in &block.pieces {
        if piece.hard_break {
            // A <br> ends the line but not the block: the next run starts on the
            // following row with no empty line between them. Two breaks in a row
            // leave an empty line between them, which is what a browser draws.
            if row.runs.iter().any(|r| !r.text.trim().is_empty()) {
                trim_and_push(out, std::mem::take(&mut row));
            } else {
                out.rows.push(Row::default());
            }
            row = Row { runs: vec![cont_run.clone()], width: cont.width() };
            col = cont.width();
            after_break = true;
            continue;
        }
        after_break = false;
        let style = style_of(piece, heading);
        let text = expand_tabs(&piece.text, opts.tab_width);
        for token in tokens_for(text, piece.field.is_some()) {
            let token_width = token.text.width();
            if token.space {
                if !row.runs.is_empty() && col > prefix.width() {
                    col = push_run(
                        out,
                        &mut row,
                        Run {
                            text: " ".into(),
                            start: col,
                            style,
                            link: piece.link,
                            field: piece.field,
                            disclosure: piece.disclosure,
                            media: piece.media,
                        },
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
                            Run {
                                text: part,
                                start: col,
                                style,
                                link: piece.link,
                                field: piece.field,
                                disclosure: piece.disclosure,
                                media: piece.media,
                            },
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
                Run {
                    text: token.text,
                    start: col,
                    style,
                    link: piece.link,
                    field: piece.field,
                    disclosure: piece.disclosure,
                    media: piece.media,
                },
            );
        }
    }
    // A break at the very end of a block leaves an empty row behind; the block
    // separation that follows already provides the space.
    if after_break && row.runs.iter().all(|r| r.text.trim().is_empty()) {
        return;
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
        image: piece.media.is_some(),
        border: false,
        heading,
        placeholder: false,
        disclosure: piece.disclosure.is_some(),
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

/// One table row drawn as a grid: `| cell | cell |` with box-drawing rules, one
/// column line for every column and none where a `colspan` covers it.
#[derive(Debug, Clone, Copy)]
struct Slot<'a> {
    cell: Option<&'a crate::dom::TableCell>,
    at: usize,
    span: usize,
}

/// A row spread over `cols` columns: its cells in order with their spans clamped
/// to what is left, and an empty slot for every column nothing was written in.
fn row_slots(row: &[crate::dom::TableCell], cols: usize) -> Vec<Slot<'_>> {
    let mut slots = Vec::with_capacity(row.len());
    let mut at = 0;
    for cell in row {
        if at >= cols {
            // Cells past the last column have no room left for them.
            break;
        }
        let span = cell.span.min(cols - at);
        slots.push(Slot { cell: Some(cell), at, span });
        at += span;
    }
    while at < cols {
        slots.push(Slot { cell: None, at, span: 1 });
        at += 1;
    }
    slots
}

/// Which boundaries between columns a row reaches over: where a cell covers two
/// columns there is no line between them. Indexed by the column on the left.
fn crossed_boundaries(row: &[crate::dom::TableCell], cols: usize) -> Vec<bool> {
    let mut crossed = vec![false; cols.saturating_sub(1)];
    for slot in row_slots(row, cols) {
        if slot.cell.is_some() {
            for b in slot.at..slot.at + slot.span - 1 {
                if b < crossed.len() {
                    crossed[b] = true;
                }
            }
        }
    }
    crossed
}

/// The room a slot has for its text: the columns it covers plus the gutters and
/// the column lines that fall inside it.
fn slot_width(widths: &[usize], gutter: usize, at: usize, span: usize) -> usize {
    widths[at..at + span].iter().sum::<usize>() + span * gutter + span - 1 - gutter
}
/// The columns of a row that a cell reaches over are laid out as one slot.
fn layout_table(out: &mut Layout, doc: &Doc, index: usize, indent: usize, opts: Options) {
    let Some(table) = doc.tables.get(index) else { return };
    // A `colspan` covers columns, so the grid is counted in columns rather than
    // in cells.
    let cols = table.rows.iter().map(|r| r.iter().map(|c| c.span).sum::<usize>()).max().unwrap_or(0);
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
        for slot in row_slots(row, cols) {
            if slot.span == 1 {
                if let Some(cell) = slot.cell {
                    natural[slot.at] = natural[slot.at].max(cell.text_width().max(1));
                }
            }
        }
    }
    // A cell over several columns wants room as well: whatever it needs past the
    // columns it covers is shared out between them.
    for row in &table.rows {
        for slot in row_slots(row, cols) {
            let Some(cell) = slot.cell else { continue };
            if slot.span < 2 {
                continue;
            }
            let covered = natural[slot.at..slot.at + slot.span].iter().sum::<usize>() + slot.span - 1;
            let extra = cell.text_width().saturating_sub(covered);
            for (k, column) in natural[slot.at..slot.at + slot.span].iter_mut().enumerate() {
                *column += extra / slot.span + usize::from(k < extra % slot.span);
            }
        }
    }
    // One blank column on each side of the cell text, when there is room.
    let gutter = if avail >= natural.iter().sum::<usize>() + cols * 3 || avail >= cols * 3 { 2 } else { 0 };
    let widths = fit_columns(&natural, avail.saturating_sub(cols * gutter));
    let left_gutter = gutter / 2;
    let right_gutter = gutter - left_gutter;
    // Where the cells of each row cover a boundary, the rules and the column lines
    // have to leave it out.
    let crossed: Vec<Vec<bool>> = table.rows.iter().map(|r| crossed_boundaries(r, cols)).collect();
    table_rule(
        out,
        &pad,
        &widths,
        gutter,
        Corners { left: '\u{250c}', mid: '\u{252c}', right: '\u{2510}' },
        &crossed[0],
    );
    for (ri, row) in table.rows.iter().enumerate() {
        if ri > 0 {
            // A rule breaks where a cell does not cover the boundary in both rows.
            let covered: Vec<bool> =
                crossed[ri - 1].iter().zip(&crossed[ri]).map(|(over, under)| *over || *under).collect();
            table_rule(
                out,
                &pad,
                &widths,
                gutter,
                Corners { left: '\u{251c}', mid: '\u{253c}', right: '\u{2524}' },
                &covered,
            );
        }
        let slots = row_slots(row, cols);
        // The room each slot has for its text: its own columns, plus the gutters
        // and the column lines a `colspan` covers.
        let wide: Vec<usize> = slots.iter().map(|s| slot_width(&widths, gutter, s.at, s.span)).collect();
        let mut lines: Vec<Vec<Vec<Run>>> = Vec::with_capacity(slots.len());
        let mut aligns: Vec<crate::dom::Align> = Vec::with_capacity(slots.len());
        let mut bases: Vec<CellStyle> = Vec::with_capacity(slots.len());
        for (s, slot) in slots.iter().enumerate() {
            match slot.cell {
                Some(cell) => {
                    aligns.push(cell.align);
                    bases.push(cell.pieces.first().map(|p| style_of(p, false)).unwrap_or_default());
                    lines.push(wrap_cell(cell, wide[s]));
                }
                None => {
                    aligns.push(crate::dom::Align::Left);
                    bases.push(CellStyle::default());
                    lines.push(vec![Vec::new()]);
                }
            }
        }
        let height = lines.iter().map(|l| l.len()).max().unwrap_or(1);
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
                    disclosure: None,
                    media: None,
                });
            }
            push_border(&mut runs, &mut col, '\u{2502}');
            for s in 0..slots.len() {
                let style = bases[s];
                if left_gutter > 0 {
                    runs.push(Run {
                        text: " ".repeat(left_gutter),
                        start: col,
                        style,
                        link: None,
                        field: None,
                        disclosure: None,
                        media: None,
                    });
                    col += left_gutter;
                }
                let cell_start = col;
                let line = lines[s].get(li).cloned().unwrap_or_default();
                let used: usize = line.iter().map(|r| r.text.width()).sum();
                let slack = wide[s].saturating_sub(used);
                let left_pad = match aligns[s] {
                    crate::dom::Align::Right => slack,
                    crate::dom::Align::Center => slack / 2,
                    crate::dom::Align::Left => 0,
                };
                if left_pad > 0 {
                    runs.push(Run {
                        text: " ".repeat(left_pad),
                        start: col,
                        style,
                        link: None,
                        field: None,
                        disclosure: None,
                        media: None,
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
                        disclosure: run.disclosure,
                        media: run.media,
                    });
                    col += width;
                }
                if col < cell_start + wide[s] {
                    runs.push(Run {
                        text: " ".repeat(cell_start + wide[s] - col),
                        start: col,
                        style,
                        link: None,
                        field: None,
                        disclosure: None,
                        media: None,
                    });
                    col = cell_start + wide[s];
                }
                if right_gutter > 0 {
                    runs.push(Run {
                        text: " ".repeat(right_gutter),
                        start: col,
                        style,
                        link: None,
                        field: None,
                        disclosure: None,
                        media: None,
                    });
                    col += right_gutter;
                }
                push_border(&mut runs, &mut col, '\u{2502}');
            }
            out.rows.push(Row { runs, width: col });
        }
    }
    let last = crossed.last().map(Vec::as_slice).unwrap_or(&[]);
    table_rule(
        out,
        &pad,
        &widths,
        gutter,
        Corners { left: '\u{2514}', mid: '\u{2534}', right: '\u{2518}' },
        last,
    );
}

fn push_border(runs: &mut Vec<Run>, col: &mut usize, ch: char) {
    runs.push(Run {
        text: ch.to_string(),
        start: *col,
        style: CellStyle { border: true, ..CellStyle::default() },
        link: None,
        field: None,
        disclosure: None,
        media: None,
    });
    *col += 1;
}

/// The three glyphs of a horizontal rule: where it starts, where it meets a column
/// line and where it ends.
#[derive(Debug, Clone, Copy)]
struct Corners {
    left: char,
    mid: char,
    right: char,
}

fn table_rule(
    out: &mut Layout,
    pad: &str,
    widths: &[usize],
    gutter: usize,
    corners: Corners,
    covered: &[bool],
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
            disclosure: None,
            media: None,
        });
    }
    let mut ch = corners.left;
    for (i, w) in widths.iter().enumerate() {
        push_border(&mut runs, &mut col, ch);
        runs.push(Run {
            text: "\u{2500}".repeat(*w + gutter),
            start: col,
            style: CellStyle { border: true, ..CellStyle::default() },
            link: None,
            field: None,
            disclosure: None,
            media: None,
        });
        col += w + gutter;
        // A cell that covers this boundary hides the column line, so the rule runs
        // on unbroken instead of meeting it at a junction.
        ch = if i + 1 == widths.len() {
            corners.right
        } else if covered.get(i).copied().unwrap_or(false) {
            '\u{2500}'
        } else {
            corners.mid
        };
    }
    push_border(&mut runs, &mut col, corners.right);
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
    // A <br> inside a cell breaks the line inside the cell, so the cell's pieces
    // are wrapped one group at a time.
    let mut groups: Vec<Vec<crate::dom::Piece>> = vec![Vec::new()];
    for piece in &cell.pieces {
        if piece.hard_break {
            groups.push(Vec::new());
        } else {
            groups.last_mut().unwrap().push(piece.clone());
        }
    }
    let mut lines = Vec::new();
    for (i, group) in groups.iter().enumerate() {
        let group_lines = wrap_cell_lines(group, cell.header, width);
        if group_lines.is_empty() {
            // A break with nothing on it: an empty line, unless it trails the cell.
            if i + 1 < groups.len() {
                lines.push(Vec::new());
            }
            continue;
        }
        lines.extend(group_lines);
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

/// A word inside a table cell, with everything the piece it came from carries:
/// a cell is laid out word by word, and the words of an image label still have
/// to say which image they spell.
#[derive(Debug, Clone)]
struct CellWord {
    text: String,
    style: CellStyle,
    link: Option<usize>,
    field: Option<usize>,
    media: Option<usize>,
}

fn wrap_cell_lines(pieces: &[crate::dom::Piece], header: bool, width: usize) -> Vec<Vec<Run>> {
    let mut words: Vec<CellWord> = Vec::new();
    for piece in pieces {
        let mut style = style_of(piece, false);
        if header {
            style.bold = true;
        }
        for token in tokens_for(piece.text.clone(), piece.field.is_some()) {
            let text = if token.space { " ".to_string() } else { token.text.clone() };
            if text == " " && words.last().map(|w| w.text == " ").unwrap_or(false) {
                continue;
            }
            words.push(CellWord { text, style, link: piece.link, field: piece.field, media: piece.media });
        }
    }
    while words.first().map(|w| w.text == " ").unwrap_or(false) {
        words.remove(0);
    }

    let mut lines: Vec<Vec<Run>> = Vec::new();
    let mut line: Vec<Run> = Vec::new();
    let mut col = 0usize;
    for CellWord { text, style, link, field, media } in words {
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
                    line.push(Run {
                        text: std::mem::take(&mut chunk),
                        start: col,
                        style,
                        link,
                        field,
                        disclosure: None,
                        media,
                    });
                    col += cw;
                    lines.push(std::mem::take(&mut line));
                    cw = 0;
                }
                chunk.push_str(g);
                cw += gw;
            }
            if cw > 0 {
                line.push(Run { text: chunk, start: col, style, link, field, disclosure: None, media });
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
        line.push(Run { text, start: col, style, link, field, disclosure: None, media });
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
                    field: None,
                    disclosure: None,
                    media: None,
                    hard_break: false,
                });
            }
            pieces.extend(cell.pieces.iter().cloned());
        }
        let block = Block { kind: crate::dom::BlockKind::Paragraph, indent: 0, pieces, hidden_by: None };
        layout_flow(out, &block, " ".repeat(indent), opts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom;

    fn rows(layout: &Layout) -> Vec<String> {
        (0..layout.rows.len()).map(|n| row_text(layout, n)).collect()
    }

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

    #[test]
    fn a_break_ends_the_line_without_an_empty_one() {
        let doc = dom::parse("<p>one<br>two</p><p>next</p>");
        let laid = layout(&doc, 40, 4, true);
        let rows: Vec<String> = (0..laid.rows.len()).map(|r| row_text(&laid, r)).collect();
        assert_eq!(rows, vec!["one", "two", "", "next"], "{rows:?}");
    }

    #[test]
    fn two_breaks_leave_one_empty_line() {
        let doc = dom::parse("<p>one<br><br>two</p>");
        let laid = layout(&doc, 40, 4, true);
        let rows: Vec<String> = (0..laid.rows.len()).map(|r| row_text(&laid, r)).collect();
        assert_eq!(rows, vec!["one", "", "two"], "{rows:?}");
    }

    #[test]
    fn a_break_at_the_end_of_a_block_adds_nothing() {
        let doc = dom::parse("<p>a<br></p><div>d</div>");
        let laid = layout(&doc, 40, 4, true);
        let rows: Vec<String> = (0..laid.rows.len()).map(|r| row_text(&laid, r)).collect();
        assert_eq!(rows, vec!["a", "", "d"], "{rows:?}");
    }

    #[test]
    fn every_block_inside_an_item_is_indented() {
        let laid = layout(
            &crate::dom::parse("<ul><li><p>alpha</p><div>beta</div></li><li>gamma</li></ul>"),
            40,
            4,
            true,
        );
        assert_eq!(rows(&laid), ["\u{2022} alpha", "  beta", "\u{2022} gamma"]);
    }

    #[test]
    fn nested_items_line_up_under_their_own_marker() {
        let laid =
            layout(&crate::dom::parse("<ul><li>outer<ul><li>inner</li></ul>tail</li></ul>"), 40, 4, true);
        assert_eq!(rows(&laid), ["\u{2022} outer", "  \u{2022} inner", "  tail"]);
    }

    #[test]
    fn an_ordered_item_indents_past_its_number() {
        let laid = layout(&crate::dom::parse("<ol><li><p>one</p><p>two</p></li></ol>"), 40, 4, true);
        assert_eq!(rows(&laid), ["1. one", "   two"]);
    }

    #[test]
    fn an_item_opened_by_a_heading_keeps_bullet_and_heading_style() {
        let laid = layout(&crate::dom::parse("<ul><li><h2>Head</h2><p>body</p></li></ul>"), 40, 4, true);
        assert_eq!(rows(&laid), ["\u{2022} Head", "  body"]);
        let head = laid.rows[0].runs.iter().find(|r| r.text.contains("Head")).unwrap();
        assert!(head.style.heading, "the heading style survives its bullet");
    }

    #[test]
    fn an_item_with_nothing_in_it_adds_no_row() {
        let laid = layout(&crate::dom::parse("<ul><li>one</li><li>  </li><li>two</li></ul>"), 40, 4, true);
        assert_eq!(rows(&laid), ["\u{2022} one", "\u{2022} two"]);
    }

    #[test]
    fn a_pre_inside_an_item_is_indented_and_takes_the_marker() {
        let laid = layout(&crate::dom::parse("<ul><li><pre>first\nsecond</pre></li></ul>"), 40, 4, true);
        assert_eq!(rows(&laid), ["\u{2022} first", "  second"]);
    }

    #[test]
    fn a_summary_line_marks_its_disclosure_end_to_end() {
        let doc = dom::parse("<details><summary>Anything here</summary><p>Body</p></details>");
        let laid = layout(&doc, 40, 4, true);
        assert_eq!(rows(&laid), ["  + Anything here"], "a closed one shows only its line");
        assert_eq!(laid.disclosure_at(0, 2), Some(0), "Enter on the marker opens it");
        assert_eq!(laid.disclosure_at(0, 6), Some(0), "Enter on its words opens it too");
        assert_eq!(laid.disclosure_at(0, 0), Some(0), "so does the blank in front of it");
        let row = &laid.rows[0];
        assert!(!row.runs[0].style.disclosure, "the indent does not wear the marking");
        assert!(row.runs[1].style.disclosure, "the marker does");
        assert!(row.runs[2].style.disclosure, "and so do the words");
    }

    #[test]
    fn an_open_disclosure_adds_rows_below_and_closes_back_up() {
        let mut doc = dom::parse("<p>up</p><details><summary>a</summary><p>Body</p></details>");
        let closed = layout(&doc, 40, 4, true);
        doc.disclosures[0].open = true;
        let open = layout(&doc, 40, 4, true);
        assert_eq!(rows(&closed), ["up", "", "  + a"]);
        assert_eq!(rows(&open), ["up", "", "  - a", "", "  Body"], "its content goes in below");
        doc.disclosures[0].open = false;
        assert_eq!(rows(&layout(&doc, 40, 4, true)), ["up", "", "  + a"], "and is taken away again");
        // A block that is not shown keeps its entry in the row table: anchors and
        // jumps are indexed by block, and hiding must not shift them.
        assert_eq!(layout(&doc, 40, 4, true).block_rows.len(), doc.blocks.len());
    }

    #[test]
    fn a_disclosure_line_wraps_under_its_own_marker() {
        let laid = layout(&dom::parse("<details><summary>one two three</summary>x</details>"), 12, 4, true);
        assert_eq!(rows(&laid), ["  + one two", "    three"]);
    }

    #[test]
    fn a_spanning_cell_takes_the_room_of_the_columns_it_covers() {
        let doc = dom::parse("<table><tr><td colspan=2>wide</td></tr><tr><td>a</td><td>b</td></tr></table>");
        let laid = layout(&doc, 40, 4, true);
        // The cell runs over the column line it covers, and the rules above and
        // below it break only where a column line really meets them.
        assert_eq!(rows(&laid), ["┌────────┐", "│ wide   │", "├────────┤", "│ a  │ b │", "└────┴───┘"]);
    }

    #[test]
    fn a_spanning_cell_widens_the_columns_it_sits_on() {
        let doc = dom::parse(
            "<table><tr><td colspan=2>one two three four</td></tr><tr><td>aaa</td><td>bbb</td></tr></table>",
        );
        let laid = layout(&doc, 20, 4, true);
        assert_eq!(
            rows(&laid),
            [
                "┌──────────────────┐",
                "│ one two three    │",
                "│ four             │",
                "├──────────────────┤",
                "│ aaa     │ bbb    │",
                "└─────────┴────────┘"
            ]
        );
        let width = laid.rows[0].width;
        assert!(laid.rows.iter().all(|r| r.width == width), "the grid stays a rectangle");
    }

    #[test]
    fn a_spanning_cell_in_the_last_row_leaves_the_rule_unbroken() {
        let doc = dom::parse(
            "<table><tr><td>one</td><td>two</td></tr><tr><td colspan=2>a spanning tail</td></tr></table>",
        );
        let laid = layout(&doc, 30, 4, true);
        assert_eq!(
            rows(&laid),
            [
                "┌─────────┬─────────┐",
                "│ one     │ two     │",
                "├───────────────────┤",
                "│ a spanning tail   │",
                "└───────────────────┘"
            ]
        );
    }

    #[test]
    fn an_image_label_remembers_which_image_it_stands_for() {
        let doc =
            dom::parse("<p>see <img src=\"/p/x.png\" alt=\"x\"> then <img src=\"/p/y.png\" alt=\"y\"></p>");
        let laid = layout(&doc, 40, 4, true);
        let row = laid
            .rows
            .iter()
            .position(|r| r.runs.iter().any(|run| run.media.is_some()))
            .expect("a label on a row");
        let mut indices: Vec<usize> = laid.rows[row].runs.iter().filter_map(|run| run.media).collect();
        indices.dedup();
        assert_eq!(indices, vec![0, 1], "each label points at its own image, in reading order");
        assert_eq!(laid.media_at(row, 4), Some(0), "the first word of a label");
        assert_eq!(laid.media_at(row, 11), Some(0), "the middle of its last word");
        assert_eq!(laid.media_at(row, 12), None, "the space after it belongs to the sentence");
        assert_eq!(laid.media_at(row, 0), None, "and so do the words before it");
        assert_eq!(laid.media_at(row + 1, 0), None, "nor is the next row");
    }

    #[test]
    fn an_image_inside_a_cell_is_still_an_image() {
        let doc =
            dom::parse("<table><tr><td><img src=\"/p/i.png\" alt=\"i\"></td><td>words</td></tr></table>");
        let laid = layout(&doc, 30, 4, true);
        let where_ = laid
            .rows
            .iter()
            .enumerate()
            .find_map(|(row, r)| r.runs.iter().find(|run| run.media.is_some()).map(|run| (row, run.start)))
            .expect("the label survives the grid");
        assert_eq!(laid.media_at(where_.0, where_.1), Some(0));
    }
}

//! Markdown read as a page.
//!
//! A `.md` body is not a different kind of page, it is the same page written
//! differently. So this module does not build a document of its own: it turns
//! markdown text into the HTML the browser already parses
//! (`dom::parse_with_css`), and everything downstream - flow, colour, links,
//! image labels, search, `--dump`, `#anchor` jumps - is the code an HTML page
//! takes, unchanged.
//!
//! The subset is the shape of real READMEs: headings in both spellings, fenced
//! and indented code, blockquotes, ordered and unordered lists nested by
//! indentation, pipe tables, rules, hard breaks, links and images (inline and
//! by reference), emphasis, code spans and autolinks.
//!
//! Two choices are deliberate. Raw HTML is **escaped** rather than passed
//! through: a text file is as untrusted as any page, so it may not invent
//! structure its author did not write, which means a `<details>` in a README
//! shows as written. And footnotes, strikethrough and bare URLs are shown
//! exactly as written too, because the page has nowhere to put them.

use std::collections::{BTreeMap, BTreeSet};

/// One source line: the columns it is indented by, its text with the edges
/// trimmed, and whether it ends in a hard break (two trailing spaces).
#[derive(Clone)]
struct Line {
    indent: usize,
    body: String,
    hard: bool,
}

/// An indent of this many columns is code rather than indentation.
const CODE: usize = 4;

/// Stands for a hard break while a paragraph's words are still being read, so a
/// break written on one line is not mistaken for markup. Control characters are
/// taken out of the source before this is ever put in.
const BREAK: char = '\u{1}';

struct Md {
    /// `[label]: url "title"` definitions, gathered before any inline pass.
    refs: BTreeMap<String, (String, String)>,
    /// Heading slugs already handed out, so `#install` cannot point at two.
    slugs: BTreeSet<String>,
    /// The HTML so far.
    out: String,
}

/// Convert a markdown body into HTML for the page parser.
pub fn to_html(src: &str) -> String {
    // A text file off the net is no more trustworthy than an HTML one, and a converter
    // that copies its text straight through copies the control characters with it. This
    // is the same strip the dom applies to a text node; doing it here means the emitter
    // never sees a byte it would have to think about.
    Md { refs: BTreeMap::new(), slugs: BTreeSet::new(), out: String::new() }
        .run(&crate::sanitize::sanitize(src))
}

impl Md {
    fn run(mut self, src: &str) -> String {
        let text = strip_comments(&strip_front_matter(src));
        let mut lines = split(&text);
        refs_of(&mut self.refs, &mut lines);
        self.blocks(&lines);
        self.out
    }

    /// Blocks into a scratch buffer and back, which is how a quote or a list item
    /// gets its contents read as blocks of their own.
    fn under(&mut self, lines: &[Line]) -> String {
        let out = std::mem::take(&mut self.out);
        self.blocks(lines);
        std::mem::replace(&mut self.out, out)
    }
}

/// Split into lines, tabs expanded to four-column stops. Blank lines keep their
/// place: they are what ends a paragraph.
fn split(src: &str) -> Vec<Line> {
    src.lines()
        .map(|raw| {
            let text = expand_tabs(raw.trim_end_matches('\r'));
            Line {
                hard: text.ends_with("  "),
                indent: text.len() - text.trim_start().len(),
                body: text.trim().to_string(),
            }
        })
        .collect()
}

fn expand_tabs(raw: &str) -> String {
    if !raw.contains('\t') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut column = 0usize;
    for ch in raw.chars() {
        if ch == '\t' {
            let stop = (column / CODE + 1) * CODE;
            out.push_str(&" ".repeat(stop - column));
            column = stop;
        } else {
            // Counting cells rather than bytes, so a wide character before a tab
            // still lands on the same stop it would in an editor.
            out.push(ch);
            column += 1;
        }
    }
    out
}

/// Drop a leading metadata block: `---` on the first line, and again later.
/// A README's front matter is for the site that builds it, not for a reader.
fn strip_front_matter(src: &str) -> String {
    if !src.starts_with("---") {
        return src.to_string();
    }
    let mut lines = src.lines();
    let Some(first) = lines.next() else { return String::new() };
    if first.trim_end() != "---" {
        return src.to_string();
    }
    for (n, line) in lines.enumerate() {
        if matches!(line.trim_end(), "---" | "...") {
            // +2 counts the fence that opened the block as well.
            return src.lines().skip(n + 2).collect::<Vec<_>>().join("\n");
        }
    }
    src.to_string()
}

/// Remove HTML comments, keeping the line count: a comment that spans three
/// lines must not join the line before it to the line after it. An unterminated
/// comment runs to the end of the file, which is what a browser does with one.
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(at) = rest.find("<!--") {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 4..];
        let (comment, after) = match tail.find("-->") {
            Some(end) => (&tail[..end], &tail[end + 3..]),
            None => (tail, ""),
        };
        out.push_str(&"\n".repeat(comment.matches('\n').count()));
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Collect link reference definitions and blank the lines they were on. Fenced
/// code is skipped, because a code sample full of `[a]: b` is a code sample.
fn refs_of(refs: &mut BTreeMap<String, (String, String)>, lines: &mut [Line]) {
    let mut fence = None::<(char, usize)>;
    for line in lines.iter_mut() {
        if let Some((ch, width)) = fence {
            if closes_fence(line, ch, width) {
                fence = None;
            }
            continue;
        }
        if line.indent < CODE {
            if let Some(open) = opening_fence(line) {
                fence = Some(open);
                continue;
            }
        }
        if let Some((label, dest, title)) = definition(line) {
            refs.insert(label, (dest, title));
            *line = Line { indent: 0, body: String::new(), hard: false };
        }
    }
}

impl Md {
    /// The block level: one trip round this loop reads one block, one element out.
    fn blocks(&mut self, lines: &[Line]) {
        let mut i = 0;
        while i < lines.len() {
            let line = &lines[i];
            if line.body.is_empty() {
                i += 1;
                continue;
            }
            if line.indent >= CODE {
                i = self.indented(lines, i);
                continue;
            }
            if let Some((ch, width)) = opening_fence(line) {
                i = self.fenced(lines, i, ch, width);
            } else if let Some(level) = atx(line) {
                let inner = self.inline(heading_body(&line.body));
                self.heading_text(level, inner);
                i += 1;
            } else if thematic_break(line) {
                self.out.push_str("<hr>\n");
                i += 1;
            } else if line.body.starts_with('>') {
                i = self.quote(lines, i);
            } else if marker(line).is_some() {
                i = self.list(lines, i);
            } else if line.body.contains('|') && lines.get(i + 1).is_some_and(delimiter) {
                i = self.table(lines, i);
            } else {
                i = self.paragraph(lines, i);
            }
        }
    }

    /// A ``` or ~~~ block. Up to the fence's own indentation is taken back off the
    /// lines inside it, so a fence written inside a list item does not carry the
    /// item's indent into the slab. The language after the fence is a hint about
    /// colour this page does not have, so it is dropped.
    fn fenced(&mut self, lines: &[Line], start: usize, ch: char, width: usize) -> usize {
        // Up to the fence's own indentation comes back off what is inside it, so
        // a fence written inside a list item does not carry the item's indent.
        let strip = lines[start].indent;
        let mut i = start + 1;
        let mut code = String::new();
        while i < lines.len() && !closes_fence(&lines[i], ch, width) {
            code.push_str(&" ".repeat(lines[i].indent.saturating_sub(strip)));
            code.push_str(&lines[i].body);
            code.push('\n');
            i += 1;
        }
        self.out.push_str("<pre><code>");
        push_escaped(&mut self.out, &code);
        self.out.push_str("</code></pre>\n");
        if i < lines.len() {
            i + 1 // the closing fence, when the author remembered one
        } else {
            i
        }
    }

    /// Four columns of indentation is code, until the text stops being indented.
    fn indented(&mut self, lines: &[Line], start: usize) -> usize {
        let mut i = start;
        let mut code = String::new();
        while i < lines.len() {
            let line = &lines[i];
            if line.body.is_empty() {
                // A blank line inside the block counts only if the next line is
                // still indented; otherwise it is the blank that ends it.
                if !lines.get(i + 1).is_some_and(|next| next.indent >= CODE) {
                    break;
                }
                code.push('\n');
            } else if line.indent >= CODE {
                code.push_str(&" ".repeat(line.indent - CODE));
                code.push_str(&line.body);
                code.push('\n');
            } else {
                break;
            }
            i += 1;
        }
        self.out.push_str("<pre><code>");
        push_escaped(&mut self.out, &code);
        self.out.push_str("</code></pre>\n");
        i
    }
}

impl Md {
    /// A heading, carrying the slug that a table of contents links to. READMEs
    /// are written for hosts that turn `## Compiling from Source` into
    /// `#compiling-from-source`, and the page has to answer the same address for
    /// those links to jump anywhere (`App::anchor_row` reads these ids).
    fn heading_text(&mut self, level: usize, inner: String) {
        let slug = self.slug_for(&strip_tags(&inner));
        let id = if slug.is_empty() { String::new() } else { format!(" id=\"{}\"", esc_attr(&slug)) };
        self.out.push_str(&format!("<h{level}{id}>{inner}</h{level}>\n"));
    }

    /// The slug for a heading's text, made unique the way the hosts do it: the
    /// second `Install` is `install-1`, so neither link lies about where it goes.
    fn slug_for(&mut self, text: &str) -> String {
        let mut slug = String::new();
        for ch in text.trim().chars() {
            if ch.is_whitespace() {
                slug.push('-');
            } else if ch.is_alphanumeric() || ch == '-' || ch == '_' {
                slug.extend(ch.to_lowercase());
            }
        }
        let mut n = 0;
        let mut candidate = slug.clone();
        while !self.slugs.insert(candidate.clone()) {
            n += 1;
            candidate = format!("{slug}-{n}");
        }
        candidate
    }

    /// `>` lines, plus the unmarked lines a quoted paragraph runs into. The
    /// inside is read as blocks, which is how a quote ends up holding a list, a
    /// fence, or another quote.
    fn quote(&mut self, lines: &[Line], start: usize) -> usize {
        let mut inner: Vec<Line> = Vec::new();
        let mut i = start;
        while i < lines.len() {
            let line = &lines[i];
            if let Some(rest) = line.body.strip_prefix('>') {
                inner.push(Line {
                    indent: 0,
                    hard: line.hard,
                    body: rest.strip_prefix(' ').unwrap_or(rest).to_string(),
                });
            } else if !line.body.is_empty() && !starts_block(line) {
                inner.push(line.clone());
            } else {
                break;
            }
            i += 1;
        }
        let body = self.under(&inner);
        self.out.push_str("<blockquote>\n");
        self.out.push_str(&body);
        self.out.push_str("</blockquote>\n");
        i
    }

    /// A list. An item is the text after its marker plus everything lined up to
    /// that text column, and those lines are read as blocks - which is all
    /// nesting turns out to need: a deeper list is a block inside the item.
    fn list(&mut self, lines: &[Line], start: usize) -> usize {
        let Some(first) = marker(&lines[start]) else {
            return start + 1;
        };
        let base = lines[start].indent;
        let mut items: Vec<Vec<Line>> = Vec::new();
        let mut i = start;
        // A blank line anywhere in the list makes the whole of it loose, and a
        // loose item is a paragraph rather than bare text.
        let mut loose = false;
        while i < lines.len() {
            let Some(mark) = marker(&lines[i]).filter(|m| lines[i].indent == base && m.punct == first.punct)
            else {
                break;
            };
            let content = base + mark.width;
            let mut item =
                vec![Line { indent: 0, hard: lines[i].hard, body: lines[i].body[mark.width..].to_string() }];
            i += 1;
            while i < lines.len() {
                let line = &lines[i];
                if line.body.is_empty() {
                    let keeps = lines.get(i + 1).is_some_and(|next| {
                        next.indent >= content
                            || next.indent >= base && marker(next).is_some_and(|m| m.punct == first.punct)
                    });
                    if !keeps {
                        break;
                    }
                    loose = true;
                    item.push(Line { indent: 0, hard: false, body: String::new() });
                    i += 1;
                    continue;
                }
                if line.indent >= content {
                    item.push(Line {
                        indent: line.indent - content,
                        hard: line.hard,
                        body: line.body.clone(),
                    });
                } else if line.indent <= base && marker(line).is_some() || starts_block(line) {
                    break;
                } else {
                    item.push(Line { indent: 0, hard: line.hard, body: line.body.clone() });
                }
                i += 1;
            }
            items.push(item);
        }
        let tag = if first.ordered { "ol" } else { "ul" };
        // The number a list counts from is the one thing about it that cannot be
        // re-derived, so it travels to the page as `start`.
        let open = if first.ordered && first.number != 1 {
            format!("<ol start=\"{}\">", first.number)
        } else {
            format!("<{tag}>")
        };
        self.out.push_str(&open);
        self.out.push('\n');
        for item in &items {
            let body = self.under(item);
            // A tight list - no blank line anywhere in it - spends one row per
            // item: the text after the marker *is* the paragraph, so the wrapper
            // that would put it on a row of its own comes back off.
            let body = if loose { body } else { unwrap_first(&body) };
            self.out.push_str("<li>");
            self.out.push_str(&body);
            self.out.push_str("</li>\n");
        }
        self.out.push_str(&format!("</{tag}>\n"));
        i
    }
}

impl Md {
    /// Running text. Two trailing spaces at the end of a line are a line break
    /// the author asked for; a plain newline is a soft one, and the page's own
    /// word wrapping decides where the line actually ends.
    fn paragraph(&mut self, lines: &[Line], start: usize) -> usize {
        let mut i = start;
        let mut text = String::new();
        while i < lines.len() {
            let line = &lines[i];
            if line.body.is_empty() {
                break;
            }
            if i > start && (starts_block(line) || setext(line).is_some()) {
                break;
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&line.body);
            // A break is stood in for by a character the source cannot contain and
            // the inline pass leaves alone, so it survives until the words are HTML.
            if line.hard {
                text.push(BREAK);
            }
            i += 1;
        }
        // Two trailing spaces on the last line of a paragraph break nothing.
        while text.ends_with(BREAK) {
            text.pop();
        }
        // `Title` over `=====` is the other way to write a heading, and old
        // documents are full of it.
        if let Some(level) = lines.get(i).and_then(setext).filter(|_| i > start) {
            let inner = self.words(&text);
            self.heading_text(level, inner);
            return i + 1;
        }
        let inner = self.words(&text);
        self.out.push_str("<p>");
        self.out.push_str(&inner);
        self.out.push_str("</p>\n");
        i
    }

    /// The inline pass over a whole paragraph, with the hard breaks it carried
    /// put back as markup. One call over the joined words is what lets a link's
    /// own label straddle a line break.
    fn words(&mut self, text: &str) -> String {
        self.inline(text).replace(BREAK, "<br>")
    }

    /// A pipe table: a header row, a row of dashes that says how each column
    /// lines up, then the rows. The table layout already wraps columns and draws
    /// a rule under the header, and it reads `align`, so the dashes land.
    fn table(&mut self, lines: &[Line], start: usize) -> usize {
        let aligns = aligns(&lines[start + 1]);
        let mut i = start + 2;
        self.out.push_str("<table>\n<thead>\n<tr>");
        self.row(&lines[start].body, &aligns, "th");
        self.out.push_str("</tr>\n</thead>\n");
        let first = self.out.len();
        while let Some(line) = lines.get(i) {
            if line.body.is_empty() || !line.body.contains('|') {
                break;
            }
            self.out.push_str("<tr>");
            self.row(&line.body, &aligns, "td");
            self.out.push_str("</tr>\n");
            i += 1;
        }
        // A table of a header and nothing else gets no body at all.
        if self.out.len() > first {
            self.out.insert_str(first, "<tbody>\n");
            self.out.push_str("</tbody>\n");
        }
        self.out.push_str("</table>\n");
        i
    }

    /// One row of cells. Each cell is inline content, so a key can be code and a
    /// description can be a link, exactly as in an HTML table.
    fn row(&mut self, cells: &str, aligns: &[Align], tag: &str) {
        for (n, cell) in split_cells(cells).iter().enumerate() {
            let align = match aligns.get(n).copied().unwrap_or(Align::Left) {
                Align::Left => String::new(),
                Align::Center => " align=\"center\"".into(),
                Align::Right => " align=\"right\"".into(),
            };
            let inner = self.inline(cell);
            self.out.push_str(&format!("<{tag}{align}>{inner}</{tag}>"));
        }
        self.out.push('\n');
    }
}

/// Take the wrapper off the first paragraph of a block's output, which is what
/// makes a tight list item one line instead of a paragraph.
fn unwrap_first(body: &str) -> String {
    let Some(rest) = body.strip_prefix("<p>") else { return body.to_string() };
    match rest.find("</p>") {
        Some(end) => format!("{}{}", &rest[..end], &rest[end + 4..]),
        None => body.to_string(),
    }
}

/// A list marker and the column its text starts at, which is where the rest of
/// the item has to line up. `punct` is what kind of list it marks, because a
/// `-` after a run of `*` is a different list and not another item.
#[derive(Clone, Copy)]
struct Mark {
    width: usize,
    punct: char,
    ordered: bool,
    /// What the marker counts from, for the few lists that do not start at one.
    /// A bullet has no number and is given one anyway.
    number: usize,
}

fn marker(line: &Line) -> Option<Mark> {
    let body = line.body.as_str();
    let first = body.chars().next()?;
    if matches!(first, '-' | '+' | '*') {
        // A run of them is a rule, and `-x` is a hyphen, not a bullet.
        if thematic_break(line) || matches!(body.chars().nth(1), Some(ch) if !ch.is_whitespace()) {
            return None;
        }
        let rest = &body[1..];
        return mark_width(rest).map(|width| Mark {
            width: 1 + width,
            punct: first,
            ordered: false,
            number: 1,
        });
    }
    let digits = body.len() - body.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || digits > 9 {
        return None;
    }
    let after = &body[digits..];
    let punct = after.chars().next()?;
    if punct != '.' && punct != ')' {
        return None;
    }
    // `2.two` is a number in a sentence, not a numbered item.
    let number = body[..digits].parse().unwrap_or(1);
    mark_width(&after[1..]).map(|width| Mark { width: digits + 1 + width, punct, ordered: true, number })
}

/// The gap a marker must have before its text: one to three spaces, or nothing
/// at all for an empty item. Four or more means it was not a marker.
fn mark_width(rest: &str) -> Option<usize> {
    if rest.is_empty() {
        return Some(0);
    }
    let spaces = rest.len() - rest.trim_start().len();
    if (1..=3).contains(&spaces) {
        Some(spaces)
    } else {
        None
    }
}

fn atx(line: &Line) -> Option<usize> {
    let hashes = line.body.len() - line.body.trim_start_matches('#').len();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line.body[hashes..];
    if !(rest.is_empty() || rest.starts_with(' ')) {
        return None;
    }
    Some(hashes)
}

/// The words of an ATX heading. The hashes that open it are its level, and the
/// ones some authors close it with are nothing at all.
fn heading_body(text: &str) -> &str {
    let rest = text.trim_start_matches('#').trim_start();
    if rest.ends_with('#') {
        rest.trim_end_matches('#').trim_end()
    } else {
        rest
    }
}

/// The underline of a setext heading. `-` shares its shape with a rule and with
/// a bullet, and whichever of the three the parser reaches first wins - which is
/// why a paragraph followed by `---` is a heading, as the old documents expect.
fn setext(line: &Line) -> Option<usize> {
    if line.body.chars().all(|ch| ch == '=') && line.body.contains('=') {
        return Some(1);
    }
    if line.body.chars().all(|ch| ch == '-') && line.body.contains('-') {
        return Some(2);
    }
    None
}

fn thematic_break(line: &Line) -> bool {
    let mut mark = None;
    let mut count = 0;
    for ch in line.body.chars() {
        if ch.is_whitespace() {
            continue;
        }
        if !matches!(ch, '-' | '_' | '*') {
            return false;
        }
        if mark.is_some_and(|m| m != ch) {
            return false;
        }
        mark = Some(ch);
        count += 1;
    }
    count >= 3
}

fn opening_fence(line: &Line) -> Option<(char, usize)> {
    let ch = line.body.chars().next().filter(|ch| matches!(ch, '`' | '~'))?;
    let run = line.body.chars().take_while(|c| *c == ch).count();
    if run < 3 {
        return None;
    }
    // A fence of backticks with a language after it is fine; text that is not
    // more backticks means this is a fence opening, not a line of code.
    let info = line.body[run..].trim();
    if ch == '`' && info.contains('`') {
        return None;
    }
    Some((ch, run))
}

fn closes_fence(line: &Line, ch: char, at_least: usize) -> bool {
    let marks = line.body.chars().take_while(|c| *c == ch).count();
    line.indent < CODE && marks >= at_least && line.body.chars().all(|c| c == ch || c.is_whitespace())
}

/// Whether this line opens a block of its own, which is what ends a paragraph
/// running above it.
fn starts_block(line: &Line) -> bool {
    line.indent >= CODE
        || opening_fence(line).is_some()
        || atx(line).is_some()
        || thematic_break(line)
        || line.body.starts_with('>')
        || marker(line).is_some()
}

/// Where a column lines up, read off the dashes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Center,
    Right,
}

/// The dashes between a table's header and its rows: one cell of dashes per
/// column, with a colon on the side the text hugs.
fn delimiter(line: &Line) -> bool {
    line.indent < CODE
        && line.body.contains('-')
        && line.body.chars().all(|ch| matches!(ch, '|' | '-' | ':' | ' '))
        && split_cells(&line.body)
            .iter()
            .all(|cell| !cell.is_empty() && cell.chars().all(|ch| matches!(ch, '-' | ':')))
}

fn aligns(row: &Line) -> Vec<Align> {
    split_cells(&row.body)
        .iter()
        .map(|cell| match (cell.starts_with(':'), cell.ends_with(':')) {
            (true, true) => Align::Center,
            (false, true) => Align::Right,
            _ => Align::Left,
        })
        .collect()
}

/// A table row into cells. A `\|` is a pipe written *inside* a cell, which is
/// the only way to get one, so it does not split the row; it is left as written
/// for the inline pass to put back.
fn split_cells(row: &str) -> Vec<String> {
    let body = row.trim().trim_start_matches('|').trim_end_matches('|');
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = body.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek() == Some(&'|') {
            cell.push('\\');
            cell.push('|');
            chars.next();
        } else if ch == '|' {
            cells.push(cell.trim().to_string());
            cell = String::new();
        } else {
            cell.push(ch);
        }
    }
    cells.push(cell.trim().to_string());
    cells
}

/// A `[label]: url "title"` line. This is where the links in a README that is
/// written to be read as prose get their addresses.
fn definition(line: &Line) -> Option<(String, String, String)> {
    if line.indent >= CODE {
        return None;
    }
    let body = line.body.strip_prefix('[')?;
    let split = body.find("]:")?;
    let label = norm_label(&body[..split]);
    if label.is_empty() {
        return None;
    }
    let (dest, title) = split_dest_title(body[split + 2..].trim())?;
    Some((label, dest, title))
}

/// The address and optional title after a definition's colon.
fn split_dest_title(rest: &str) -> Option<(String, String)> {
    if rest.is_empty() {
        return None;
    }
    let (dest, tail) = match rest.strip_prefix('<').and_then(|inner| inner.find('>')) {
        Some(end) => {
            let inner = &rest[1..];
            (inner[..end].to_string(), inner[end + 1..].trim())
        }
        None => match rest.find(char::is_whitespace) {
            Some(at) => (rest[..at].to_string(), rest[at..].trim()),
            None => (rest.to_string(), ""),
        },
    };
    let title = tail
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .or_else(|| tail.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')))
        .or_else(|| tail.strip_prefix('(').and_then(|t| t.strip_suffix(')')))
        .unwrap_or("")
        .to_string();
    Some((unescape(&dest), unescape(&title)))
}

/// A backslash in front of punctuation in a destination or a title is markup that
/// means nothing on its own, so it goes before the value is escaped.
fn unescape(text: &str) -> String {
    if !text.contains('\\') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some(next) if punct(next) => out.push(next),
            Some(next) => {
                out.push('\\');
                out.push(next);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Reference labels are matched case- and space-insensitively.
fn norm_label(label: &str) -> String {
    label.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

impl Md {
    /// The inline pass. Everything that is not markup - and a page written
    /// yesterday contains a great deal of `<`, `&` and `_` that is not - goes
    /// through as itself, escaped, because it says what it says.
    fn inline(&self, text: &str) -> String {
        let c: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < c.len() {
            match c[i] {
                '\\' if i + 1 < c.len() && punct(c[i + 1]) => {
                    out.push(c[i + 1]);
                    i += 2;
                }
                '`' => {
                    let (html, next) = code_span(&c, i);
                    out.push_str(&html);
                    i = next;
                }
                '<' => {
                    let (html, next) = angle(&c, i);
                    out.push_str(&html);
                    i = next;
                }
                '&' => match entity(&c, i) {
                    // `&amp;` and `&#39;` are already an escape, and passing one
                    // through is what keeps a README about escaping readable.
                    // Text that only looks like one - `&&`, `&copy` - is
                    // escaped, so nothing is invented from an ampersand.
                    Some(end) => {
                        out.extend(&c[i..end]);
                        i = end;
                    }
                    None => {
                        out.push_str("&amp;");
                        i += 1;
                    }
                },
                '>' => {
                    out.push_str("&gt;");
                    i += 1;
                }
                '!' if c.get(i + 1) == Some(&'[') => {
                    let (html, next) = self.reference(&c, i + 1, true);
                    out.push_str(&html);
                    i = next;
                }
                '[' => {
                    let (html, next) = self.reference(&c, i, false);
                    out.push_str(&html);
                    i = next;
                }
                ch @ ('*' | '_') => {
                    let (html, next) = self.emphasis(&c, i, ch);
                    out.push_str(&html);
                    i = next;
                }
                other => {
                    out.push(other);
                    i += 1;
                }
            }
        }
        out
    }

    /// `[text](url "title")`, and the same link with its address written at the
    /// bottom of the file instead: `[text][ref]`, `[ref][]`, `[ref]`. A label
    /// that matches no definition stays exactly as written, which is also how a
    /// task list's `- [ ] do the thing` reaches the page as a checkbox drawn in
    /// characters rather than as a link to nowhere.
    fn reference(&self, c: &[char], open: usize, image: bool) -> (String, usize) {
        let Some((text, after)) = bracket(c, open) else {
            return (String::from("["), open + 1);
        };
        let mut dest = String::new();
        let mut title = String::new();
        let mut end = after;
        let mut found = false;
        if c.get(after) == Some(&'(') {
            if let Some((inner, next)) = parens(c, after) {
                if let Some((d, t)) = split_dest_title(inner.trim()) {
                    dest = d;
                    title = t;
                    end = next;
                    found = true;
                }
            }
        } else {
            let mut label: String = text.iter().collect();
            if let Some((second, next)) = bracket(c, after) {
                // `[text][ref]` names its target, `[ref][]` and `[ref]` are the
                // same link with the label doing the work, empty or absent.
                let written: String = second.iter().collect();
                if !written.trim().is_empty() {
                    label = written;
                }
                end = next;
            }
            if let Some((url, named)) = self.refs.get(&norm_label(&label)) {
                dest = url.clone();
                title = named.clone();
                found = true;
            }
        }
        if !found {
            return (String::from("["), open + 1);
        }
        let mut out = String::new();
        if image {
            // The words inside the brackets are the whole of an image's
            // description, so they go into `alt` rather than onto the page.
            let alt = strip_tags(&self.inline(&text.iter().collect::<String>()));
            out.push_str("<img src=\"");
            push_attr(&mut out, &dest);
            out.push_str("\" alt=\"");
            push_attr(&mut out, &alt);
            out.push('"');
            if !title.is_empty() {
                out.push_str(" title=\"");
                push_attr(&mut out, &title);
                out.push('"');
            }
            out.push('>');
        } else {
            out.push_str("<a href=\"");
            push_attr(&mut out, &dest);
            if !title.is_empty() {
                out.push_str("\" title=\"");
                push_attr(&mut out, &title);
            }
            out.push_str("\">");
            out.push_str(&self.inline(&text.iter().collect::<String>()));
            out.push_str("</a>");
        }
        (out, end)
    }
}

impl Md {
    /// `**strong**`, `*em*`, `__strong__`, `_em_` and the three-mark spelling of
    /// both at once. A closer is a run of at least as many marks with something
    /// between; without one the marks stay as typed, because a shell glob is not
    /// emphasis and neither is a filename in backticks.
    fn emphasis(&self, c: &[char], start: usize, ch: char) -> (String, usize) {
        let width = run(c, start, ch).min(3);
        let before = if start == 0 { None } else { Some(c[start - 1]) };
        let after = c.get(start + width).copied();
        if ch == '_'
            && before.is_some_and(|ch| ch.is_alphanumeric())
            && after.is_some_and(|ch| ch.is_alphanumeric())
        {
            // `snake_case_names` are names, not emphasis.
            return (ch.to_string(), start + 1);
        }
        if after.is_some_and(char::is_whitespace) {
            return (c[start..start + width].iter().collect::<String>(), start + width);
        }
        let mut i = start + width;
        while i + width <= c.len() {
            let closes =
                c[i] == ch && run(c, i, ch) >= width && i > start + width && !c[i - 1].is_whitespace();
            let intraword = ch == '_'
                && c[i - 1].is_alphanumeric()
                && c.get(i + width).is_some_and(|ch| ch.is_alphanumeric());
            if closes && !intraword {
                let body = self.inline(&c[start + width..i].iter().collect::<String>());
                let (open, shut) = match width {
                    3 => ("<b><i>", "</i></b>"),
                    2 => ("<b>", "</b>"),
                    _ => ("<i>", "</i>"),
                };
                return (format!("{open}{body}{shut}"), i + width);
            }
            i += 1;
        }
        (c[start..start + width].iter().collect::<String>(), start + width)
    }
}

/// A run of backticks and the text between the run that closes it. Nothing
/// inside is markup, and one space at each end is dropped so a command that
/// contains a backtick can still be written.
fn code_span(c: &[char], start: usize) -> (String, usize) {
    let width = run(c, start, '`');
    let mut i = start + width;
    while i < c.len() {
        if c[i] == '`' && run(c, i, '`') == width {
            let mut body = c[start + width..i].iter().collect::<String>();
            if body.len() > 2 && body.starts_with(' ') && body.ends_with(' ') {
                body = body[1..body.len() - 1].to_string();
            }
            let mut out = String::from("<code>");
            push_escaped(&mut out, &body);
            out.push_str("</code>");
            return (out, i + width);
        }
        i += 1;
    }
    (c[start..start + width].iter().collect::<String>(), start + width)
}

/// `<https://example.org>` and `<someone@example.org>` are links. Every other
/// `<` is a less-than sign - including the `<div>` of raw HTML, which this
/// module deliberately does not apply, because a text file off the net is as
/// untrusted as any page and must not invent structure of its own.
fn angle(c: &[char], start: usize) -> (String, usize) {
    let mut i = start + 1;
    while i < c.len() && c[i] != '>' && !c[i].is_whitespace() {
        i += 1;
    }
    if i > start + 1 && c.get(i) == Some(&'>') {
        let inner = c[start + 1..i].iter().collect::<String>();
        let scheme = inner.contains(':') && inner.chars().next().is_some_and(|ch| ch.is_ascii_alphabetic());
        let href = if scheme {
            Some(inner.clone())
        } else if inner.matches('@').count() == 1 && inner.contains('.') && !inner.starts_with('@') {
            Some(format!("mailto:{inner}"))
        } else {
            None
        };
        if let Some(href) = href {
            let mut out = String::from("<a href=\"");
            push_attr(&mut out, &href);
            out.push_str("\">");
            push_escaped(&mut out, &inner);
            out.push_str("</a>");
            return (out, i + 1);
        }
    }
    (String::from("&lt;"), start + 1)
}

/// Where an already-written character reference ends, if this is one.
fn entity(c: &[char], start: usize) -> Option<usize> {
    let mut i = start + 1;
    let named = c.get(i).is_some_and(|ch| ch.is_ascii_alphanumeric());
    if c.get(i) == Some(&'#') {
        i += 1;
        let digits = c.get(i).is_some_and(|ch| ch.is_ascii_digit());
        if c.get(i) == Some(&'x') || c.get(i) == Some(&'X') {
            i += 1;
        }
        while c.get(i).is_some_and(|ch| ch.is_ascii_hexdigit()) {
            i += 1;
        }
        if i > start + 2 && digits && c.get(i) == Some(&';') {
            return Some(i + 1);
        }
        return None;
    }
    if !named {
        return None;
    }
    while c.get(i).is_some_and(|ch| ch.is_ascii_alphanumeric()) {
        i += 1;
    }
    if c.get(i) != Some(&';') {
        return None;
    }
    // Only a name that means something is left alone: `&copy;` is a copyright
    // sign, `&copy` of a filename is an ampersand. A README full of `?a=1&b=2`
    // must not lose its `&` to an accident of spelling.
    let name: String = c[start + 1..i].iter().collect();
    (NAMED.contains(&name.to_ascii_lowercase().as_str())).then_some(i + 1)
}

/// The named character references a markdown file is likely to have meant.
const NAMED: &[&str] = &[
    "amp", "lt", "gt", "quot", "apos", "nbsp", "shy", "copy", "reg", "trade", "deg", "plusmn", "micro",
    "para", "sect", "middot", "hellip", "mdash", "ndash", "lsquo", "rsquo", "ldquo", "rdquo", "bull", "star",
    "dagger", "times", "divide", "minus", "le", "ge", "ne", "larr", "rarr", "uarr", "darr", "harr", "laquo",
    "raquo",
];

/// The characters that mean something here, and so the ones a backslash buys.
fn punct(ch: char) -> bool {
    matches!(
        ch,
        '\\' | '`'
            | '*'
            | '_'
            | '{'
            | '}'
            | '['
            | ']'
            | '('
            | ')'
            | '#'
            | '+'
            | '-'
            | '.'
            | '!'
            | '|'
            | '~'
            | '<'
            | '>'
            | '&'
            | ':'
            | ';'
            | '"'
            | '\''
    )
}

/// How many of `ch` start at `start`.
fn run(c: &[char], start: usize, ch: char) -> usize {
    let mut n = 0;
    while c.get(start + n) == Some(&ch) {
        n += 1;
    }
    n
}

/// A bracketed run and where it ended, counting nesting so a picture inside a
/// link is still one link.
fn bracket(c: &[char], open: usize) -> Option<(Vec<char>, usize)> {
    c.get(open).filter(|ch| **ch == '[')?;
    let mut depth = 0usize;
    let mut i = open;
    while i < c.len() {
        match c[i] {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some((c[open + 1..i].to_vec(), i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// A parenthesised run, counted the same way, so an address with brackets in it
/// ends where it says and not at the first `)`.
fn parens(c: &[char], open: usize) -> Option<(String, usize)> {
    c.get(open).filter(|ch| **ch == '(')?;
    let mut depth = 0usize;
    let mut i = open;
    let mut inner = String::new();
    while i < c.len() {
        match c[i] {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((inner, i + 1));
                }
            }
            other => inner.push(other),
        }
        i += 1;
    }
    None
}

/// Text into content: the three characters that mean something to the parser
/// behind this one are the only ones touched, and a `&` already part of a
/// character reference is left alone for the same reason as in the inline pass.
fn push_escaped(out: &mut String, text: &str) {
    let c: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '<' => {
                out.push_str("&lt;");
                i += 1;
            }
            '>' => {
                out.push_str("&gt;");
                i += 1;
            }
            '&' => match entity(&c, i) {
                Some(end) => {
                    out.extend(&c[i..end]);
                    i = end;
                }
                None => {
                    out.push_str("&amp;");
                    i += 1;
                }
            },
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
}

/// Text into an attribute value.
fn push_attr(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
}

/// An attribute value, with the same rule as any other: the parser behind this
/// one owns `<`, `>`, `&` and `"`.
fn esc_attr(text: &str) -> String {
    let mut out = String::new();
    push_attr(&mut out, text);
    out
}

/// The tags this module wrote, for the few places that want the words inside
/// them and nothing else.
fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut markup = false;
    for ch in html.chars() {
        match ch {
            '<' => markup = true,
            '>' => markup = false,
            other if !markup => out.push(other),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page a reader ends up looking at, at a width narrow enough to wrap.
    fn page(md: &str) -> String {
        crate::dump::render_markdown(md, 40)
    }

    /// The same page reached the other way, from HTML written by hand.
    fn from_html(html: &str) -> String {
        crate::dump::render_html(html, 40)
    }

    #[test]
    fn a_heading_becomes_the_heading_the_page_already_knows() {
        assert_eq!(to_html("# One"), "<h1 id=\"one\">One</h1>\n");
        assert_eq!(to_html("### Three"), "<h3 id=\"three\">Three</h3>\n");
    }

    #[test]
    fn a_heading_with_no_words_in_it_is_still_a_heading() {
        assert_eq!(to_html("##"), "<h2></h2>\n");
        assert_eq!(to_html("### Install ###"), "<h3 id=\"install\">Install</h3>\n");
        // Seven markers is not a heading at all, so its words stay a paragraph.
        assert_eq!(to_html("####### seven"), "<p>####### seven</p>\n");
    }

    #[test]
    fn an_underline_is_the_other_way_to_write_a_heading() {
        assert_eq!(to_html("Title\n====="), "<h1 id=\"title\">Title</h1>\n");
        assert_eq!(to_html("Sub\n---"), "<h2 id=\"sub\">Sub</h2>\n");
        // The underline has to be at the left margin to underline anything.
        assert_eq!(to_html("    ---"), "<pre><code>---\n</code></pre>\n");
    }

    #[test]
    fn a_slug_is_the_address_a_host_would_have_used() {
        assert_eq!(to_html("## Hello, World!"), "<h2 id=\"hello-world\">Hello, World!</h2>\n");
        assert_eq!(to_html("## Ünïcode — dash"), "<h2 id=\"ünïcode--dash\">Ünïcode — dash</h2>\n");
        // The slug is the words that are left once the markup has gone.
        assert_eq!(to_html("## **Bold** heading"), "<h2 id=\"bold-heading\"><b>Bold</b> heading</h2>\n");
    }

    #[test]
    fn the_second_heading_of_a_name_gets_a_number() {
        let html = to_html("# Install\n\n## Install\n\n### Install");
        assert!(html.contains("id=\"install\""), "{html}");
        assert!(html.contains("id=\"install-1\""), "{html}");
        assert!(html.contains("id=\"install-2\""), "{html}");
    }

    #[test]
    fn a_rule_is_a_rule() {
        for src in ["---", "***", "___", "- - -", "* * * *"] {
            assert_eq!(to_html(src), "<hr>\n", "{src}");
        }
        // A dash under a line of words is a heading, and a list starts a list.
        assert_eq!(to_html("Words\n---"), "<h2 id=\"words\">Words</h2>\n");
        assert!(to_html("- - one").starts_with("<ul>"));
    }

    #[test]
    fn emphasis_code_and_escapes_survive_a_sentence() {
        let html = to_html("**b** *i* _al_ `c` \\*not em\\* snake_case_name 100_000");
        assert!(html.contains("<b>b</b>"), "{html}");
        assert!(html.contains("<i>i</i>"), "{html}");
        assert!(html.contains("<i>al</i>"), "{html}");
        assert!(html.contains("<code>c</code>"), "{html}");
        assert!(html.contains("*not em*"), "{html}");
        assert!(html.contains("snake_case_name"), "{html}");
        assert!(html.contains("100_000"), "{html}");
    }

    #[test]
    fn a_river_of_stars_does_not_swallow_the_sentence() {
        // The line is a rule only on its own; inside a paragraph it is words.
        assert_eq!(to_html("one\n***\ntwo"), "<p>one</p>\n<hr>\n<p>two</p>\n");
        assert!(to_html("a ***** b").contains("*****"));
    }

    #[test]
    fn text_is_escaped_and_a_real_entity_is_left_alone() {
        assert_eq!(to_html("5 < 6 && R&D"), "<p>5 &lt; 6 &amp;&amp; R&amp;D</p>\n");
        // An entity is left alone because the page will decode it; an unknown one is
        // ordinary text and its ampersand has to be escaped.
        assert_eq!(to_html("&amp; &#39; &copy; &nope;"), "<p>&amp; &#39; &copy; &amp;nope;</p>\n");
    }

    #[test]
    fn a_tag_in_a_text_file_is_text() {
        // Raw HTML in a file of words is not trusted with the page.
        assert_eq!(to_html("<b>bold</b>"), "<p>&lt;b&gt;bold&lt;/b&gt;</p>\n");
        assert!(to_html("<div>\nnot a block\n</div>").starts_with("<p>&lt;div&gt;"));
    }

    #[test]
    fn a_link_takes_every_spelling_a_person_reaches_for() {
        let html = to_html(
            "[a](/x) [b](/y \"Why\") [c](</sp ace>) [d](/z\\&1) ![e](i.png) <https://s.example> <nobody@nowhere.example>",
        );
        assert!(html.contains("<a href=\"/x\">a</a>"), "{html}");
        assert!(html.contains("<a href=\"/y\" title=\"Why\">b</a>"), "{html}");
        assert!(html.contains("<a href=\"/sp ace\">c</a>"), "{html}");
        assert!(html.contains("<a href=\"/z&amp;1\">d</a>"), "{html}");
        assert!(html.contains("<img src=\"i.png\" alt=\"e\">"), "{html}");
        assert!(html.contains("<a href=\"https://s.example\">https://s.example</a>"), "{html}");
        assert!(
            html.contains("<a href=\"mailto:nobody@nowhere.example\">nobody@nowhere.example</a>"),
            "{html}"
        );
    }

    #[test]
    fn an_address_is_escaped_like_one() {
        // A link's own text is escaped, and its address escapes quotes.
        let html = to_html("[a & b](/x?c=1&d=2 \"a \\\"title\\\"\")");
        assert!(
            html.contains("<a href=\"/x?c=1&amp;d=2\" title=\"a &quot;title&quot;\">a &amp; b</a>"),
            "{html}"
        );
    }

    #[test]
    fn an_image_is_a_name_an_address_and_a_caption() {
        assert_eq!(
            to_html("text ![a cat](c.png \"cap\") end"),
            "<p>text <img src=\"c.png\" alt=\"a cat\" title=\"cap\"> end</p>\n"
        );
        let html = to_html("![alt](cat.png \"cap\") and ![ref][logo]\n\n[logo]: l.png");
        assert!(html.contains("<img src=\"cat.png\" alt=\"alt\" title=\"cap\">"), "{html}");
        assert!(html.contains("<img src=\"l.png\" alt=\"ref\">"), "{html}");
        // The dom prints an image as a label rather than pixels.
        assert!(page("![gone](a.png)").contains("[img: gone]"), "{}", page("![gone](a.png)"));
    }

    #[test]
    fn a_reference_that_matches_nothing_stays_as_written() {
        assert_eq!(to_html("[a][b]"), "<p>[a][b]</p>\n");
        assert_eq!(to_html("[nonsense]"), "<p>[nonsense]</p>\n");
        // Which is also how a task list's checkbox reaches the page.
        assert_eq!(to_html("- [ ] a checkbox is text"), "<ul>\n<li>[ ] a checkbox is text\n</li>\n</ul>\n");
    }

    #[test]
    fn a_paragraph_joins_its_lines_and_two_trailing_ones_break() {
        assert_eq!(to_html("one\ntwo"), "<p>one\ntwo</p>\n");
        // The break belongs at the end of the row of words, not on one of its own.
        assert_eq!(to_html("one  \ntwo"), "<p>one<br>\ntwo</p>\n");
    }

    #[test]
    fn a_paragraph_ends_at_the_block_after_it() {
        // A quote swallows an unmarked line after it, which is how a paragraph reads
        // across a line that wrapped; after a blank line the words are their own block.
        assert_eq!(
            to_html("words\n> a quote\nwords two"),
            "<p>words</p>\n<blockquote>\n<p>a quote\nwords two</p>\n</blockquote>\n"
        );
        assert_eq!(
            to_html("words\n> a quote\n\nwords two"),
            "<p>words</p>\n<blockquote>\n<p>a quote</p>\n</blockquote>\n<p>words two</p>\n"
        );
    }

    #[test]
    fn a_fence_is_a_slab_and_its_language_is_dropped() {
        assert_eq!(to_html("```rust\nfn main() {}\n```"), "<pre><code>fn main() {}\n</code></pre>\n");
        assert_eq!(to_html("~~~\n<b>raw</b>\n~~~"), "<pre><code>&lt;b&gt;raw&lt;/b&gt;\n</code></pre>\n");
        // An unclosed fence runs to the end of the file, which is what a browser does.
        assert_eq!(to_html("```\nstill code"), "<pre><code>still code\n</code></pre>\n");
    }

    #[test]
    fn a_fence_is_closed_by_a_run_at_least_as_long_as_its_own() {
        let html = to_html("````\n``` not the end\n````");
        assert_eq!(html, "<pre><code>``` not the end\n</code></pre>\n");
    }

    #[test]
    fn a_fence_at_the_left_margin_belongs_to_no_list() {
        // A fence flush with the margin cannot be inside an item whose own text starts
        // two columns in, so it ends the list and stands on its own - as it does
        // everywhere else markdown is read.
        let html = to_html("* item with a fence:\n```\ndeep kept\n```\n* second item");
        assert_eq!(
            html,
            "<ul>\n<li>item with a fence:\n</li>\n</ul>\n<pre><code>deep kept\n</code></pre>\n<ul>\n<li>second item\n</li>\n</ul>\n"
        );
    }

    #[test]
    fn four_columns_of_indentation_is_code() {
        let html = to_html("    one\n        two\n\n    three");
        assert_eq!(html, "<pre><code>one\n    two\n\nthree\n</code></pre>\n");
    }

    #[test]
    fn a_tab_is_a_stop_not_a_character() {
        assert_eq!(to_html("\tcode"), "<pre><code>code\n</code></pre>\n");
        assert_eq!(to_html("a\tb"), "<p>a   b</p>\n");
    }

    #[test]
    fn a_quote_holds_lines_lists_and_another_quote() {
        let html = to_html("> # H\n> * one\n> * two");
        assert!(html.starts_with("<blockquote>\n<h1 id=\"h\">H</h1>\n<ul>"), "{html}");
        assert!(html.ends_with("</ul>\n</blockquote>\n"), "{html}");
        // A line that forgot its mark still belongs to the paragraph above it.
        assert_eq!(to_html("> one\ntwo"), "<blockquote>\n<p>one\ntwo</p>\n</blockquote>\n");
        // A deeper quote closes when a shallower mark turns up, and a heading after a
        // quote is a heading outside it.
        assert_eq!(
            to_html("> out\n> > in\ntail"),
            "<blockquote>\n<p>out</p>\n<blockquote>\n<p>in\ntail</p>\n</blockquote>\n</blockquote>\n"
        );
        assert_eq!(
            to_html("> one\n# two"),
            "<blockquote>\n<p>one</p>\n</blockquote>\n<h1 id=\"two\">two</h1>\n"
        );
        // A bare `>` is an empty line inside the quote, not the end of it.
        assert_eq!(
            to_html("> top\n>\n> > deep\n>\n> out"),
            "<blockquote>\n<p>top</p>\n<blockquote>\n<p>deep</p>\n</blockquote>\n<p>out</p>\n</blockquote>\n"
        );
        // On the screen the frame is drawn and the words inside it are wrapped.
        let q = page("> A quote holds a paragraph and\n> continues with more words.\n>\n> * one\n> * two");
        // A quote is drawn indented, and its list keeps its bullets inside.
        assert!(q.starts_with("  "), "{q}");
        assert!(q.contains("A quote holds a paragraph and"), "{q}");
        assert!(q.contains('•'), "{q}");
    }

    #[test]
    fn a_list_is_tight_unless_a_blank_line_says_otherwise() {
        // Tight: the words sit under the marker with no row between them.
        assert_eq!(to_html("* one\n* two"), "<ul>\n<li>one\n</li>\n<li>two\n</li>\n</ul>\n");
        // Loose: a blank line anywhere spaces every item out.
        assert_eq!(to_html("* one\n\n* two"), "<ul>\n<li><p>one</p>\n</li>\n<li><p>two</p>\n</li>\n</ul>\n");
        // Indentation nests, and it is measured from the marker's own column.
        let nested = to_html("- one\n  - two");
        assert!(nested.contains("<ul>\n<li>one\n<ul>\n<li>two"), "{nested}");
    }

    #[test]
    fn a_list_item_holds_whole_blocks_too() {
        let html = to_html("* item:\n\n  ```\n  code\n  ```");
        assert!(html.contains("<li>"), "{html}");
        assert!(html.contains("<pre><code>code\n</code></pre>"), "{html}");
        // The marker is not a word and the words keep their own row.
        let p = page("* one\n* two");
        assert!(p.contains("• one"), "{p}");
        assert!(p.contains("• two"), "{p}");
    }

    #[test]
    fn a_different_marker_starts_a_different_list() {
        // `*` and `-` are different bullets, and `.` and `)` are different counts.
        assert!(to_html("* one\n\n- two").contains("</ul>\n<ul>"));
        // A `.` and a `)` do not count alike. A number with no space after it is not a
        // marker at all, so those lines stay one item of the list above them.
        assert_eq!(
            to_html("1) one\n2. two"),
            "<ol>\n<li>one\n</li>\n</ol>\n<ol start=\"2\">\n<li>two\n</li>\n</ol>\n"
        );
        assert_eq!(to_html("1. one\n2.) two"), "<ol>\n<li>one\n2.) two\n</li>\n</ol>\n");
        assert!(to_html("- one\n\n  two").contains("<p>two</p>"));
    }

    #[test]
    fn a_list_can_say_where_to_count_from() {
        // `start` is the only way to keep an author's numbering, and the dom counts on
        // it; a list whose number does not parse counts from one like any other.
        assert!(to_html("10. ten\n11. eleven").starts_with("<ol start=\"10\">"));
        assert!(to_html("3. three\n4. four").starts_with("<ol start=\"3\">"));
        assert_eq!(to_html("x. not a number\ny. nor this"), "<p>x. not a number\ny. nor this</p>\n");
        let p = page("3. three\n4. four");
        assert!(p.contains("3. three"), "{p}");
        assert!(p.contains("4. four"), "{p}");
    }

    #[test]
    fn a_list_of_nothing_is_a_list_of_nothing() {
        assert_eq!(to_html("*"), "<ul>\n<li></li>\n</ul>\n");
        assert_eq!(to_html("1."), "<ol>\n<li></li>\n</ol>\n");
    }

    #[test]
    fn a_pipe_table_lines_up_by_its_dashes() {
        // A header over a row of dashes with no rows under it is a header, and gets
        // no body at all.
        assert_eq!(
            to_html("a | b\n:- | -:"),
            "<table>\n<thead>\n<tr><th>a</th><th align=\"right\">b</th>\n</tr>\n</thead>\n</table>\n"
        );
        // Centre, left, right and default, all read off the same row of dashes.
        let html = to_html("| a | b | c | d |\n| :-: | :- | -: | --- |");
        assert!(html.contains("<th align=\"center\">a</th>"), "{html}");
        assert!(html.contains("<th>b</th>"), "{html}");
        assert!(html.contains("<th align=\"right\">c</th>"), "{html}");
        assert!(html.contains("<th>d</th>"), "{html}");
    }

    #[test]
    fn a_pipe_table_carries_its_cells_and_its_alignment() {
        let html = to_html("| Name | Note |\n| --- | --- |\n| `a` | [x](/y) |\n| b | 2 |");
        assert!(html.starts_with("<table>\n<thead>\n<tr><th>Name</th><th>Note</th>"), "{html}");
        assert!(html.contains("<td><code>a</code></td><td><a href=\"/y\">x</a></td>"), "{html}");
        assert!(html.ends_with("<td>b</td><td>2</td>\n</tr>\n</tbody>\n</table>\n"), "{html}");
        // A row of words over a row of words with no dashes between is not a table.
        assert!(!to_html("a | b\nc | d").contains("<table>"));
    }

    #[test]
    fn a_table_row_may_escape_its_pipes_and_drop_its_edges() {
        // The outer pipes are padding and the inner one is a character.
        assert_eq!(split_cells("a \\| b | c"), vec!["a \\| b", "c"]);
        assert_eq!(split_cells("| a | b |"), vec!["a", "b"]);
        // The inline pass is what turns the escape back into a pipe.
        let html = to_html("a | b\n--- | ---\nx \\| y | 2");
        assert!(html.contains("<td>x | y</td>"), "{html}");
    }

    #[test]
    fn the_definitions_are_written_once_and_used_as_often_as_you_like() {
        let html = to_html("[a][x] and [b][x]\n\n[x]: /u \"T\"");
        assert_eq!(html, "<p><a href=\"/u\" title=\"T\">a</a> and <a href=\"/u\" title=\"T\">b</a></p>\n");
        // A label cares about neither case nor spare space, and `[ref][]` is `[ref]`.
        assert!(to_html("[A B][x]\n\n[X]: /u").contains("<a href=\"/u\">A B</a>"));
        assert!(to_html("[x][]\n\n[X]: /u").contains("<a href=\"/u\">x</a>"));
        // The definition is a definition only on a line of its own, and never inside
        // a code sample.
        assert_eq!(to_html("[r]: /url"), "");
        assert!(to_html("```\n[a]: /b\n```").contains("[a]: /b"));
    }

    #[test]
    fn front_matter_and_comments_are_not_page_text() {
        let html = to_html("---\ntitle: x\n---\n\n# Real\n\none <!-- hidden --> two\n\nthree");
        assert_eq!(html, "<h1 id=\"real\">Real</h1>\n<p>one  two</p>\n<p>three</p>\n");
        // A comment over several lines is a block of its own and keeps the text either
        // side in separate paragraphs, but it says nothing itself.
        assert_eq!(to_html("before\n<!-- two\nlines -->\nafter"), "<p>before</p>\n<p>after</p>\n");
        // Not every document opens with metadata, and `---` under a paragraph is that
        // paragraph's heading underline rather than a rule.
        assert!(to_html("a\n\n---\n\nb").contains("<hr>"));
        assert_eq!(to_html("a\n---\nb"), "<h2 id=\"a\">a</h2>\n<p>b</p>\n");
    }

    #[test]
    fn what_the_subset_leaves_alone_is_shown_as_written() {
        // Documented limits, pinned so a later change cannot silently start eating
        // someone's text.
        assert!(page("~~strike~~").contains("~~strike~~"));
        assert!(page("https://a.example/plain").contains("https://a.example/plain"));
        assert!(page("<div>raw</div>").contains("<div>raw</div>"));
        assert!(to_html("Term\n: body").contains("<p>Term"));
    }

    #[test]
    fn a_page_written_in_markdown_is_the_page_written_in_html() {
        // The point of the whole module: from here down there is only one renderer.
        let md = "# Title\n\nSome *bold* and `code` words, [a link](/x) and an ![img](i.png).\n\n* one\n* two\n\n> quoted\n\n| a | b |\n| - | - |\n| 1 | 2 |\n\n```\ncode\n```\n\n---\n";
        let html = "<h1>Title</h1>\n<p>Some <b>bold</b> and <code>code</code> words, <a href=\"/x\">a link</a> and an <img src=\"i.png\" alt=\"img\">.</p>\n<ul>\n<li>one</li>\n<li>two</li>\n</ul>\n<blockquote><p>quoted</p></blockquote>\n<table>\n<thead><tr><th>a</th><th>b</th></tr></thead>\n<tbody>\n<tr><td>1</td><td>2</td></tr>\n</tbody>\n</table>\n<pre><code>code\n</code></pre>\n<hr>\n";
        assert_eq!(page(md), from_html(html));
    }

    #[test]
    fn nothing_in_a_page_is_left_unescapable() {
        // The same document through both roads lands on the same rows.
        let html = to_html("# a <!-- --> b\n\n[d][e]\n\n[e]: /f \"t\"");
        assert_eq!(html, "<h1 id=\"a--b\">a  b</h1>\n<p><a href=\"/f\" title=\"t\">d</a></p>\n");
        // A file of words cannot invent a tag, however hard it tries, and what it wrote
        // is readable as the text it wrote.
        let raw = to_html("<script>alert(\"<x>&\")</script>");
        assert!(!raw.contains("<script>"), "{raw}");
        let text = crate::dump::plain_text(&crate::layout::layout(&crate::dom::parse(&raw), 70, 4, true));
        assert!(text.contains("<script>alert(\"<x>&\")</script>"), "{text}");
        // Control characters are stripped before conversion, so they cannot reach the
        // terminal as escape sequences.
        assert!(!to_html("a\u{1}b\u{0}c").contains(char::from(1)));
        assert!(!to_html("a\u{1}b\u{0}c").contains(char::from(0)));
    }

    #[test]
    fn nothing_about_a_page_can_make_the_converter_panic() {
        for src in [
            "",
            " ",
            "\n\n\n",
            "```",
            "```\n````",
            "~~~",
            "\t",
            "> ",
            "- ",
            "1. ",
            "|",
            "| |\n|-|",
            "[",
            "](",
            "![",
            "[a](",
            "[a](",
            "***",
            "__",
            "<",
            ">",
            "&",
            "\\",
            "#",
            "\n\\\n",
            &"<".repeat(200),
            &"[".repeat(200),
            &"*".repeat(500),
            &">".repeat(100),
            &"(".repeat(100),
            "[a](b(c)d)",
            "![a](b(c)d)",
            "\u{1}",
            "a\u{0}b",
            "- [ ] task",
            "1) x",
            "- a\n\t- b",
            "a | b\n|-",
            "  \t  x",
            "# \n\n##  \n",
            "```` \n x\n  ```\n````",
        ] {
            let html = to_html(src);
            let doc = crate::dom::parse(&html);
            let _ = crate::dump::plain_text(&crate::layout::layout(&doc, 50, 4, true));
            assert!(
                !html.contains(char::from(1)) && !html.contains(char::from(0)),
                "src {src:?} gave {html:?}"
            );
        }
    }
}
#[test]
fn a_link_whose_label_straddles_a_line_is_still_a_link() {
    // Word-wrapped source is the normal way to write markdown, and a label can
    // break anywhere in it. This is ripgrep's README.
    assert_eq!(
        to_html("downloads for [every\nrelease](https://x/y). ripgrep is similar"),
        "<p>downloads for <a href=\"https://x/y\">every\nrelease</a>. ripgrep is similar</p>\n"
    );
    // Two trailing spaces break the row, except on the last line of the block.
    assert_eq!(to_html("one  \ntwo"), "<p>one<br>\ntwo</p>\n");
    assert_eq!(to_html("one  \n\n two"), "<p>one</p>\n<p>two</p>\n");
}

//! HTML -> a flat flow document: blocks of styled text runs plus links.
//!
//! CSS is consulted for exactly two things, both decided here rather than in
//! the renderer: whether an element belongs on the page at all (`display: none`,
//! `visibility: hidden`, the `hidden` attribute - the whole subtree goes) and
//! which elements ask for a monospace font (`font-family: monospace`, or a tag
//! the user agent already draws monospaced), which is carried on the piece so
//! the renderer can grey the line. Colours, sizes and layout properties from CSS
//! are still ignored; w3b has no box model to apply them to.
use crate::css;
use ego_tree::NodeRef;
use scraper::node::Node;
use scraper::Html;
use unicode_width::UnicodeWidthStr;

use crate::sanitize::sanitize;

#[derive(Debug, Clone)]
pub struct Link {
    pub href: String,
    /// Text of the anchor, used for link hints and the status message.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub text: String,
    pub link: Option<usize>,
    pub mono: bool,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub muted: bool,
    /// An image placeholder such as `[alt text]`.
    pub image: bool,
    /// Index into `Doc::fields`: the user can type into this piece.
    pub field: Option<usize>,
    /// A `<br>`: this piece ends the line it sits on and the text after it
    /// continues in the same block, so no blank line is inserted.
    pub hard_break: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading(u8),
    Item {
        depth: u16,
        ordered: bool,
        index: u32,
    },
    Pre,
    Divider,
    Field,
    /// A table, indexed into `Doc::tables`.
    Table {
        index: usize,
    },
}

/// Horizontal alignment of a table cell, from the `align` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// Which kind of editable form control a `Field` stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Input,
    Textarea,
}

/// A text input or textarea. w3b does not submit forms, but text typed into a
/// field belongs to the page: it is laid out and scrolled like any other text,
/// so the value lives in the document and is re-flowed when it changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub kind: FieldKind,
    /// The `name` attribute (or the input type) used when talking about it.
    pub name: String,
    /// Shown while the value is empty.
    pub placeholder: String,
    pub value: String,
    /// Passwords are shown as dots; the real value is kept.
    pub masked: bool,
}

impl Field {
    /// What the page shows for this field.
    pub fn display(&self) -> String {
        if self.value.is_empty() {
            format!("[ {} ]", self.placeholder)
        } else if self.masked {
            format!("[ {} ]", "\u{2022}".repeat(self.value.chars().count()))
        } else {
            format!("[ {} ]", self.value)
        }
    }
}

#[derive(Debug, Clone)]
pub struct TableCell {
    pub pieces: Vec<Piece>,
    pub header: bool,
    pub align: Align,
}

impl TableCell {
    pub fn text_width(&self) -> usize {
        self.pieces.iter().map(|p| p.text.width()).sum()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Table {
    pub rows: Vec<Vec<TableCell>>,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub kind: BlockKind,
    pub indent: u16,
    pub pieces: Vec<Piece>,
}

#[derive(Debug, Clone, Default)]
pub struct Doc {
    pub title: String,
    pub blocks: Vec<Block>,
    pub links: Vec<Link>,
    /// `id` / `name` attributes mapped to the block index they appear in, so
    /// same-document links such as `#install` can jump instead of doing nothing.
    pub anchors: Vec<(String, usize)>,
    pub tables: Vec<Table>,
    /// Text inputs and textareas, in document order.
    pub fields: Vec<Field>,
}

const TABLE_TAGS: &[&str] = &["table", "tr", "td", "th"];

/// Tags the user agent stylesheet already sets in a monospace font: w3b paints
/// those, and anything CSS puts in a monospace family, as a grey block.
const DEFAULT_MONO: &[&str] = &["code", "kbd", "samp", "tt", "kbd", "pre", "textarea"];

const SKIP: &[&str] = &[
    "script", "style", "noscript", "template", "svg", "math", "canvas", "iframe", "object", "embed", "video",
    "audio", "track", "picture", "source", "map", "link", "meta",
];

/// Elements that end the current block of text.
const BLOCK: &[&str] = &[
    "p",
    "div",
    "section",
    "article",
    "header",
    "footer",
    "main",
    "aside",
    "nav",
    "blockquote",
    "figure",
    "figcaption",
    "details",
    "summary",
    "address",
    "form",
    "fieldset",
    "dl",
    "dt",
    "dd",
    "table",
    "tbody",
    "thead",
    "tfoot",
    "tr",
    "ul",
    "ol",
    "li",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "center",
    "pre",
    "hr",
    "br",
    "caption",
    "colgroup",
    "option",
    "select",
    "button",
    "label",
];

pub fn parse(html: &str) -> Doc {
    parse_with_css(html, &css::Sheet::default())
}

/// Parse with a stylesheet applied: `display: none` elements are dropped and
/// monospace fonts are marked so the renderer can paint the line.
pub fn parse_with_css(html: &str, styles: &css::Sheet) -> Doc {
    let doc = Html::parse_document(html);
    let mut b = Builder { styles: styles.clone(), ..Builder::default() };
    for child in doc.tree.root().children() {
        b.walk(child);
    }
    b.finish()
}

#[derive(Default)]
struct ListCtx {
    depth: u16,
    ordered: bool,
    index: u32,
}

#[derive(Clone, Copy, Default)]
struct Ctx {
    link: Option<usize>,
    mono: bool,
    bold: bool,
    italic: bool,
    code: bool,
    muted: bool,
    image: bool,
    field: Option<usize>,
}

#[derive(Default)]
struct Builder {
    doc: Doc,
    cur: Vec<Piece>,
    ws: bool,
    ctx: Ctx,
    indent: u16,
    pre: bool,
    title: Option<String>,
    lists: Vec<ListCtx>,
    in_head: bool,
    pending_block: Option<BlockKind>,
    tables: Vec<TableCtx>,
    rows: Vec<Vec<TableCell>>,
    cell_align: Option<Align>,
    /// Depth inside `<td>`/`<th>`: block boundaries are suppressed here so a
    /// cell stays one flow container.
    cell_depth: usize,
    /// The parsed stylesheet, empty for `parse`.
    styles: css::Sheet,
    /// Open elements, outermost first, so a selector can walk the chain.
    path: Vec<css::Identity>,
}

#[derive(Debug, Clone, Copy)]
struct TableCtx {
    index: usize,
    indent: u16,
}

impl Builder {
    fn walk(&mut self, node: NodeRef<'_, Node>) {
        match node.value() {
            Node::Text(text) => self.text(&text.text),
            Node::Element(el) => self.element(node, el.name()),
            _ => {
                for child in node.children() {
                    self.walk(child);
                }
            }
        }
    }

    fn element(&mut self, node: NodeRef<'_, Node>, name: &str) {
        if name == "title" {
            self.title = Some(collect_text(node));
            return;
        }
        if name == "head" {
            self.in_head = true;
            for child in node.children() {
                self.walk(child);
            }
            self.in_head = false;
            return;
        }
        if self.in_head || SKIP.contains(&name) {
            return;
        }

        // CSS first: a hidden element is not on the page at all.
        let identity = identity_of(node, name);
        let inline = node.value().as_element().and_then(|e| e.attr("style")).map(str::to_string);
        let hidden = node.value().as_element().map(|e| e.attr("hidden").is_some()).unwrap_or(false)
            || self.styles.hidden(&self.target(&identity), inline.as_deref());
        if hidden {
            return;
        }
        self.path.push(identity.clone());
        let mono_now = self.mono_for(&identity, inline.as_deref());
        let saved_mono = self.ctx.mono;
        if mono_now {
            // No flush: an inline `code` stays inside its paragraph. The
            // renderer greys the whole row when any run in it is monospace.
            self.ctx.mono = true;
        }

        let mut walk_children = true;
        match name {
            "a" => self.enter_link(node),
            "b" | "strong" | "th" => self.ctx.bold = true,
            "i" | "em" | "cite" | "var" => self.ctx.italic = true,
            "code" | "kbd" | "samp" | "tt" | "pre" => self.ctx.code = true,
            "small" | "sub" | "sup" => self.ctx.muted = true,
            _ => {}
        }

        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush();
                self.pending_block = Some(BlockKind::Heading(name[1..2].parse().unwrap_or(1)));
            }
            "hr" => {
                self.flush();
                self.push_block(Block { kind: BlockKind::Divider, indent: self.indent, pieces: vec![] });
            }
            "br" => self.push_break(),
            "pre" => {
                self.flush();
                self.pre = true;
            }
            "blockquote" => self.indent += 1,
            "ul" | "ol" => self.lists.push(ListCtx {
                depth: self.lists.len() as u16 + 1,
                ordered: name == "ol",
                index: 0,
            }),
            "li" => {
                self.flush();
                let kind = match self.lists.last_mut() {
                    Some(list) => {
                        let kind =
                            BlockKind::Item { depth: list.depth, ordered: list.ordered, index: list.index };
                        list.index += 1;
                        kind
                    }
                    // Stray <li> without a list parent: still render as a bullet.
                    None => BlockKind::Item { depth: 1, ordered: false, index: 0 },
                };
                self.pending_block = Some(kind);
            }
            "img" => {
                // `[img: alt]`, falling back to the file name so an image
                // without alt text still says what it is.
                let el = match node.value().as_element() {
                    Some(el) => el,
                    None => return,
                };
                let alt = el.attr("alt").unwrap_or("").trim();
                let label = if alt.is_empty() {
                    el.attr("src")
                        .unwrap_or("")
                        .split('?')
                        .next()
                        .unwrap_or("")
                        .rsplit('/')
                        .next()
                        .unwrap_or("")
                        .to_string()
                } else {
                    alt.to_string()
                };
                let label = collapse(&label);
                if !label.is_empty() {
                    self.push_piece(format!("[img: {label}]"), |c| c.image = true);
                }
            }
            "input" | "textarea" | "select" | "button" => {
                // The label is the element's own text, so its children are not walked.
                // Text inputs and textareas become editable fields.
                if let Some(field) = field_of(node) {
                    let index = self.doc.fields.len();
                    let text = field.display();
                    self.doc.fields.push(field);
                    self.push_piece(text, |c| {
                        c.muted = true;
                        c.field = Some(index);
                    });
                } else {
                    self.push_piece(field_label(node), |c| c.muted = true);
                }
                self.ws = true;
                walk_children = false;
            }
            "table" => {
                self.flush();
                self.doc.tables.push(Table::default());
                self.tables.push(TableCtx { index: self.doc.tables.len() - 1, indent: self.indent });
            }
            "tr" => {
                self.pending_block = None;
                self.cur.clear();
                self.rows.push(Vec::new());
            }
            "td" | "th" => {
                self.pending_block = None;
                self.cur.clear();
                self.cell_depth += 1;
                self.cell_align = Some(match node.value().as_element().and_then(|e| e.attr("align")) {
                    Some("center") => Align::Center,
                    Some("right") => Align::Right,
                    _ => Align::Left,
                });
            }
            // Block elements end the current run of text.
            _ if BLOCK.contains(&name) => self.flush(),
            _ => {}
        }

        // Registered after the block boundary above so the id points at the
        // block this element actually starts in.
        if let Some(id) = node
            .value()
            .as_element()
            .and_then(|e| e.attr("id").or(e.attr("name")))
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            let block = self.doc.blocks.len();
            if !self.doc.anchors.iter().any(|(existing, _)| existing == id) {
                self.doc.anchors.push((id.to_string(), block));
            }
        }

        if walk_children {
            for child in node.children() {
                self.walk(child);
            }
        }

        self.ctx.mono = saved_mono;
        self.path.pop();

        match name {
            "a" => self.ctx.link = None,
            "b" | "strong" => self.ctx.bold = false,
            "i" | "em" | "cite" | "var" => self.ctx.italic = false,
            "code" | "kbd" | "samp" | "tt" | "pre" => {
                self.ctx.code = false;
                if name == "pre" {
                    self.pre = false;
                }
            }
            "small" | "sub" | "sup" => self.ctx.muted = false,
            "blockquote" => self.indent = self.indent.saturating_sub(1),
            "td" | "th" => {
                if name == "th" {
                    self.ctx.bold = false;
                }
                self.cell_depth = self.cell_depth.saturating_sub(1);
                let align = self.cell_align.take().unwrap_or_default();
                let header = name == "th";
                let cell = TableCell { pieces: std::mem::take(&mut self.cur), header, align };
                if let Some(row) = self.rows.last_mut() {
                    row.push(cell);
                }
            }
            "tr" => {
                // Pop only this row so a nested table's rows stay separate.
                let row = self.rows.pop().unwrap_or_default();
                if let Some(ctx) = self.tables.last() {
                    self.doc.tables[ctx.index].rows.push(row);
                }
            }
            "table" => {
                let Some(ctx) = self.tables.pop() else { return };
                self.cur.clear();
                let table = &self.doc.tables[ctx.index];
                if table.rows.iter().any(|row| !row.is_empty()) {
                    let block = Block {
                        kind: BlockKind::Table { index: ctx.index },
                        indent: ctx.indent,
                        pieces: vec![],
                    };
                    self.push_block(block);
                }
            }
            "ul" | "ol" => {
                self.lists.pop();
            }
            _ => {}
        }

        if BLOCK.contains(&name) && name != "br" && !TABLE_TAGS.contains(&name) {
            self.flush();
        }
    }

    /// The element together with its open ancestors, outermost first. Call this
    /// before pushing the element onto the path.
    fn target<'a>(&'a self, el: &'a css::Identity) -> css::Target<'a> {
        css::Target { el, ancestors: &self.path }
    }

    /// True for elements the user agent or the sheet puts in a monospace font.
    fn mono_for(&self, el: &css::Identity, inline: Option<&str>) -> bool {
        DEFAULT_MONO.contains(&el.tag.as_str()) || self.styles.monospace(&self.target(el), inline)
    }

    fn enter_link(&mut self, node: NodeRef<'_, Node>) {
        let href = node.value().as_element().and_then(|e| e.attr("href")).unwrap_or("").trim().to_string();
        if href.is_empty() {
            return;
        }
        let index = self.doc.links.len();
        self.doc.links.push(Link { href, text: String::new() });
        self.ctx.link = Some(index);
    }

    fn text(&mut self, raw: &str) {
        let text = sanitize(raw);
        if self.pre {
            for line in text.split('\n') {
                if !line.trim().is_empty() {
                    self.push_piece(line.to_string(), |_| {});
                }
                self.flush_as(BlockKind::Pre);
            }
            return;
        }
        let trimmed = text.trim();
        if trimmed.is_empty() {
            self.ws = true;
            return;
        }
        if text.starts_with(char::is_whitespace) {
            self.ws = true;
        }
        for (i, word) in trimmed.split_whitespace().enumerate() {
            if i > 0 {
                // Whitespace inside a text node is a separator too.
                self.ws = true;
            }
            self.push_piece(word.to_string(), |_| {});
        }
        if text.ends_with(char::is_whitespace) {
            self.ws = true;
        }
    }

    /// Append a word, inserting the pending separator space first.
    fn push_piece(&mut self, text: String, decorate: impl FnOnce(&mut Ctx)) {
        let mut ctx = self.ctx;
        decorate(&mut ctx);
        if self.ws && !self.cur.is_empty() {
            // A separator belongs to the link only when the link has already started,
            // so the space in "see <a>the docs</a>" is not part of the link.
            let inside = self.ctx.link.is_some() && self.cur.last().map(|p| p.link) == Some(self.ctx.link);
            self.cur.push(Piece {
                text: " ".into(),
                link: if inside { self.ctx.link } else { None },
                ..plain(self.ctx)
            });
            if inside {
                if let Some(link) = self.ctx.link.and_then(|i| self.doc.links.get_mut(i)) {
                    if !link.text.ends_with(' ') {
                        link.text.push(' ');
                    }
                }
            }
        }
        self.ws = false;
        if let Some(index) = ctx.link {
            if let Some(link) = self.doc.links.get_mut(index) {
                link.text.push_str(&text);
            }
        }
        self.cur.push(Piece {
            text,
            link: ctx.link,
            mono: ctx.mono,
            bold: ctx.bold,
            italic: ctx.italic,
            code: ctx.code,
            muted: ctx.muted,
            image: ctx.image,
            field: ctx.field,
            hard_break: false,
        });
    }

    /// `<br>` breaks the line inside the current block: it adds a break piece to
    /// the text being collected rather than closing the block, which is what keeps
    /// a single break from inserting a whole empty line.
    fn push_break(&mut self) {
        self.ws = false;
        self.cur.push(Piece { hard_break: true, ..plain(self.ctx) });
    }

    fn flush(&mut self) {
        if let Some(kind) = self.pending_block.take() {
            self.flush_as(kind);
            return;
        }
        self.flush_as(BlockKind::Paragraph);
    }

    fn flush_as(&mut self, kind: BlockKind) {
        self.ws = false;
        if self.cell_depth > 0 {
            // Keep the cell's text in `cur`; a cell is laid out as one block.
            self.pending_block = None;
            return;
        }
        if self.cur.is_empty() {
            match kind {
                BlockKind::Item { depth, ordered, index } => {
                    self.push_block(Block {
                        kind: BlockKind::Item { depth, ordered, index },
                        indent: self.indent,
                        pieces: vec![],
                    });
                }
                // Keep blank lines inside <pre>.
                BlockKind::Pre => self.push_block(Block { kind, indent: self.indent, pieces: vec![] }),
                _ => {}
            }
            return;
        }
        let indent = self.indent + self.lists.last().map_or(0, |l| l.depth.saturating_sub(1));
        let block = Block { kind, indent, pieces: std::mem::take(&mut self.cur) };
        self.push_block(block);
    }

    fn push_block(&mut self, block: Block) {
        self.doc.blocks.push(block);
    }

    fn finish(mut self) -> Doc {
        self.flush();
        self.doc.title = self.title.take().unwrap_or_default();
        self.doc
    }
}

/// Set while walking into a block element, applied by the next flush.
fn plain(ctx: Ctx) -> Piece {
    Piece {
        text: String::new(),
        link: ctx.link,
        mono: ctx.mono,
        bold: ctx.bold,
        italic: ctx.italic,
        code: ctx.code,
        muted: ctx.muted,
        image: ctx.image,
        field: ctx.field,
        hard_break: false,
    }
}

/// The part of an element a CSS selector looks at.
fn identity_of(node: NodeRef<'_, Node>, name: &str) -> css::Identity {
    let Some(el) = node.value().as_element() else {
        return css::Identity::new(name, None, &[]);
    };
    let id = el.attr("id").map(str::trim).filter(|v| !v.is_empty());
    let classes: Vec<&str> = el.attr("class").map(|c| c.split_whitespace().collect()).unwrap_or_default();
    css::Identity::new(name, id, &classes)
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The controls w3b lets the user type into. Everything else (checkboxes,
/// radios, buttons, selects, file pickers) keeps its static label, because w3b
/// does not submit forms.
fn field_of(node: NodeRef<'_, Node>) -> Option<Field> {
    let el = node.value().as_element()?;
    if !matches!(el.name(), "input" | "textarea") {
        return None;
    }
    let name = el.attr("name").unwrap_or("").trim().to_string();
    if el.name() == "textarea" {
        // A textarea's content is its default value.
        let value = collapse(&collect_text(node));
        return Some(Field {
            kind: FieldKind::Textarea,
            name: if name.is_empty() { "message".to_string() } else { name },
            placeholder: "textarea".to_string(),
            value,
            masked: false,
        });
    }
    let ty = el.attr("type").unwrap_or("text").to_ascii_lowercase();
    let masked = match ty.as_str() {
        "text" | "search" | "email" | "url" | "tel" | "number" => false,
        "password" => true,
        _ => return None,
    };
    let placeholder = el.attr("placeholder").map(collapse).filter(|p| !p.is_empty()).unwrap_or_else(|| {
        if ty == "password" {
            "password".to_string()
        } else {
            "input".to_string()
        }
    });
    Some(Field {
        kind: FieldKind::Input,
        name: if name.is_empty() { ty } else { name },
        placeholder,
        value: el.attr("value").map(collapse).unwrap_or_default(),
        masked,
    })
}

fn field_label(node: NodeRef<'_, Node>) -> String {
    let el = match node.value().as_element() {
        Some(el) => el,
        None => return "[ ]".into(),
    };
    if el.name() == "button" {
        let text = collapse(&collect_text(node));
        return if text.is_empty() { "[ button ]".into() } else { format!("[ {text} ]") };
    }
    match el.attr("type").unwrap_or("text") {
        "checkbox" | "radio" => "[ ]".into(),
        "submit" | "button" | "reset" => {
            let value = el.attr("value").map(collapse).unwrap_or_default();
            if value.is_empty() {
                "[ button ]".into()
            } else {
                format!("[ {value} ]")
            }
        }
        "password" => "[ password ]".into(),
        _ => "[ input ]".into(),
    }
}

fn collect_text(node: NodeRef<'_, Node>) -> String {
    let mut out = String::new();
    if let Node::Text(t) = node.value() {
        out.push_str(&t.text);
    }
    for child in node.children() {
        out.push_str(&collect_text(child));
    }
    collapse(&sanitize(&out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(doc: &Doc) -> Vec<(BlockKind, String)> {
        doc.blocks
            .iter()
            .map(|b| (b.kind, b.pieces.iter().map(|p| p.text.as_str()).collect::<String>()))
            .collect()
    }

    #[test]
    fn display_none_removes_the_whole_subtree() {
        let sheet = css::parse("nav, .ad { display: none }");
        let doc = parse_with_css(
            "<p>keep</p><nav><a href=/x>menu</a></nav><div class=ad>buy <b>now</b></div><p>also</p>",
            &sheet,
        );
        let text = texts(&doc).into_iter().map(|(_, text)| text).collect::<Vec<_>>().join("|");
        assert!(text.contains("keep"), "{text}");
        assert!(text.contains("also"), "{text}");
        assert!(!text.contains("menu"), "{text}");
        assert!(!text.contains("buy"), "{text}");
        assert!(doc.links.is_empty(), "a hidden link must not be walkable");
    }

    #[test]
    fn hidden_attribute_and_inline_visibility() {
        let doc = parse_with_css(
            "<p hidden>gone</p><p style=\"visibility: hidden\">also gone</p><p>here</p>",
            &css::Sheet::default(),
        );
        let text =
            doc.blocks.iter().flat_map(|b| b.pieces.iter().map(|p| p.text.as_str())).collect::<String>();
        assert_eq!(text, "here");
    }

    #[test]
    fn an_ancestor_selector_can_hide_an_element() {
        let sheet = css::parse(".sidebar .widget { display: none }");
        let doc = parse_with_css(
            "<div class=sidebar><section><p class=widget>gone</p></section><p>seen</p></div>",
            &sheet,
        );
        let text =
            doc.blocks.iter().flat_map(|b| b.pieces.iter().map(|p| p.text.as_str())).collect::<String>();
        assert_eq!(text, "seen");
    }

    #[test]
    fn monospace_elements_are_marked() {
        let sheet =
            css::parse(".term { font-family: \'JetBrains Mono\', sans-serif } .prose { font-family: serif }");
        let doc = parse_with_css(
            "<p>plain <code>ls -l</code> <span class=term>make</span> <span class=prose>text</span></p>",
            &sheet,
        );
        let marked: Vec<(&str, bool)> = doc.blocks[0]
            .pieces
            .iter()
            .map(|p| (p.text.as_str(), p.mono))
            .filter(|(text, _)| !text.trim().is_empty())
            .collect();
        assert!(marked.iter().any(|(text, mono)| *text == "ls" && *mono), "{marked:?}");
        assert!(marked.iter().any(|(text, mono)| *text == "make" && *mono), "{marked:?}");
        assert!(marked.iter().any(|(text, mono)| *text == "plain" && !*mono), "{marked:?}");
        assert!(marked.iter().any(|(text, mono)| *text == "text" && !*mono), "{marked:?}");
    }

    #[test]
    fn title_and_body_text() {
        let doc = parse("<html><head><title>My  page</title></head><body><p>hello world</p></body></html>");
        assert_eq!(doc.title, "My page");
        assert_eq!(texts(&doc), vec![(BlockKind::Paragraph, "hello world".into())]);
    }

    #[test]
    fn css_and_script_are_ignored() {
        let doc = parse("<style>body{color:red}</style><script>alert(1)</script><p>visible</p>");
        assert_eq!(texts(&doc), vec![(BlockKind::Paragraph, "visible".into())]);
    }

    #[test]
    fn headings_carry_level() {
        let doc = parse("<h1>one</h1><h3>three</h3>");
        let kinds: Vec<BlockKind> = doc.blocks.iter().map(|b| b.kind).collect();
        assert_eq!(kinds, vec![BlockKind::Heading(1), BlockKind::Heading(3)]);
    }

    #[test]
    fn whitespace_is_collapsed_across_inline_tags() {
        let doc = parse("<p>  some   <b>bold</b>   and\n\n more  </p>");
        assert_eq!(texts(&doc)[0].1, "some bold and more");
    }

    #[test]
    fn links_are_collected_with_their_text() {
        let doc = parse("<a href=\"/a\">First</a> and <a href=\"http://x.test/b\">Second link</a>");
        let hrefs: Vec<&str> = doc.links.iter().map(|l| l.href.as_str()).collect();
        assert_eq!(hrefs, vec!["/a", "http://x.test/b"]);
        assert_eq!(doc.links[0].text, "First");
        assert_eq!(doc.links[1].text, "Second link");
    }

    #[test]
    fn link_pieces_reference_the_link_index() {
        let doc = parse("<p>see <a href=\"/x\">the docs</a> now</p>");
        let linked: Vec<&Piece> = doc.blocks[0].pieces.iter().filter(|p| p.link.is_some()).collect();
        assert!(linked.iter().all(|p| p.link == Some(0)));
        // the separator inside the anchor is part of the link, so it stays clickable
        assert_eq!(linked.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(), vec!["the", " ", "docs"]);
    }

    #[test]
    fn lists_are_numbered_and_nested() {
        let doc = parse("<ol><li>one</li><li>two</li></ol><ul><ul><li>deep</li></ul></ul>");
        let items: Vec<(u16, bool, u32, String)> = doc
            .blocks
            .iter()
            .filter_map(|b| match b.kind {
                BlockKind::Item { depth, ordered, index } => {
                    Some((depth, ordered, index, b.pieces.iter().map(|p| p.text.as_str()).collect()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            items,
            vec![(1, true, 0, "one".into()), (1, true, 1, "two".into()), (2, false, 0, "deep".into())]
        );
    }

    #[test]
    fn pre_keeps_line_breaks_and_indentation() {
        let doc = parse("<pre>line one\n  indented\n\nlast</pre>");
        let lines: Vec<String> =
            doc.blocks.iter().map(|b| b.pieces.iter().map(|p| p.text.as_str()).collect()).collect();
        assert_eq!(lines, vec!["line one", "  indented", "", "last"]);
        assert!(doc.blocks.iter().all(|b| b.kind == BlockKind::Pre));
    }

    #[test]
    fn stray_li_without_a_list_is_still_a_bullet() {
        let doc = parse("<li>odd</li>");
        assert!(matches!(doc.blocks[0].kind, BlockKind::Item { depth: 1, ordered: false, index: 0 }));
    }

    #[test]
    fn blockquote_indents() {
        let doc = parse("<blockquote><p>quoted</p></blockquote>");
        assert_eq!(doc.blocks[0].indent, 1);
    }

    #[test]
    fn text_inputs_and_textareas_become_editable_fields() {
        let doc = parse(
            "<form><input name=\"q\" placeholder=\"search\"><input type=\"password\">\
             <textarea>hello there</textarea><input type=\"checkbox\"><button>Go</button>",
        );
        assert_eq!(doc.fields.len(), 3, "checkboxes and buttons are not text fields");
        assert_eq!(doc.fields[0].name, "q");
        assert_eq!(doc.fields[0].placeholder, "search");
        assert!(doc.fields[1].masked, "a password field masks its value");
        assert_eq!(doc.fields[2].kind, FieldKind::Textarea);
        assert_eq!(doc.fields[2].value, "hello there", "a textarea's content is its default value");
        let marked: Vec<(usize, &str)> = doc
            .blocks
            .iter()
            .flat_map(|b| b.pieces.iter())
            .filter_map(|p| p.field.map(|i| (i, p.text.as_str())))
            .collect();
        assert_eq!(marked, vec![(0, "[ search ]"), (1, "[ password ]"), (2, "[ hello there ]")]);
    }

    #[test]
    fn a_value_prefills_a_field_and_a_password_is_shown_as_dots() {
        let doc = parse("<input value=\"draft one\"><input type=password value=\"ab\">");
        assert_eq!(doc.fields.len(), 2);
        assert_eq!(doc.fields[0].value, "draft one");
        assert_eq!(doc.fields[0].display(), "[ draft one ]");
        assert_eq!(doc.fields[1].display(), "[ \u{2022}\u{2022} ]");
        assert_eq!(doc.fields[1].value, "ab", "the real value is kept, only the display masks");
    }

    #[test]
    fn img_alt_and_form_fields_become_placeholders() {
        let doc = parse("<img src=\"x.png\" alt=\"logo\"><input type=\"checkbox\"><button>Send</button>");
        let all: String = doc.blocks.iter().flat_map(|b| b.pieces.iter().map(|p| p.text.as_str())).collect();
        assert_eq!(all, "[img: logo][ ] [ Send ]");
        let doc = parse("<img src=\"/img/logo.png?v=2\"><img alt=\"\">");
        let all: Vec<String> =
            doc.blocks.iter().flat_map(|b| b.pieces.iter().map(|p| p.text.clone())).collect();
        assert_eq!(all, vec!["[img: logo.png]"], "no alt falls back to the file name");
    }

    #[test]
    fn escape_sequences_in_page_text_never_survive() {
        let doc = parse("<p>safe \x1b]0;pwn\x07 text</p>");
        // only the control characters are dropped; the rest is inert text
        assert_eq!(texts(&doc)[0].1, "safe ]0;pwn text");
    }

    #[test]
    fn unclosed_tags_do_not_break_the_flow() {
        let doc = parse("<p>one<p>two<div>three");
        let strings: Vec<String> =
            doc.blocks.iter().map(|b| b.pieces.iter().map(|p| p.text.as_str()).collect()).collect();
        assert_eq!(strings, vec!["one", "two", "three"]);
    }

    #[test]
    fn tables_keep_cells_headers_and_alignment() {
        let doc = parse(
            "<table><tr><th>name</th><th align=right>qty</th></tr>\
             <tr><td>widget</td><td>12</td></tr></table>",
        );
        assert_eq!(doc.tables.len(), 1);
        assert_eq!(doc.tables[0].rows.len(), 2);
        assert_eq!(doc.tables[0].rows[0][0].pieces[0].text, "name");
        assert!(doc.tables[0].rows[0][0].header);
        assert_eq!(doc.tables[0].rows[0][1].align, Align::Right);
        assert!(!doc.tables[0].rows[1][0].header);
        assert_eq!(doc.tables[0].rows[1][1].text_width(), 2);
        let kinds: Vec<BlockKind> = doc.blocks.iter().map(|b| b.kind).collect();
        assert_eq!(kinds, vec![BlockKind::Table { index: 0 }]);
    }

    #[test]
    fn block_tags_inside_a_cell_do_not_split_the_cell() {
        let doc = parse("<table><tr><td><p>one</p><p>two</p></td><td>x</td></tr></table>");
        let cell = &doc.tables[0].rows[0][0];
        let text: String = cell.pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(text, "onetwo");
        assert_eq!(doc.tables[0].rows[0].len(), 2);
        assert_eq!(doc.blocks.len(), 1, "the table is one block");
    }

    #[test]
    fn fixture_parses_with_links() {
        let html = std::fs::read_to_string("fixtures/article.html").unwrap();
        let doc = parse(&html);
        assert_eq!(doc.title, "Design notes - w3b");
        assert_eq!(doc.links.len(), 3);
        assert!(doc.blocks.iter().any(|b| b.kind == BlockKind::Heading(1)));
        assert!(doc.blocks.iter().any(|b| b.kind == BlockKind::Divider));
    }
    #[test]
    fn anchors_point_at_the_block_they_are_in() {
        let doc = parse(
            "<p>intro</p><h2 id=\"install\">Install</h2><p>words</p>\
             <a name=\"tips\">tips here</a><ul><li id=\"li1\">one</li></ul>",
        );
        let at = |name: &str| doc.anchors.iter().find(|(a, _)| a == name).map(|(_, block)| *block);
        assert_eq!(at("install"), Some(1), "the h2 is the second block");
        assert_eq!(at("tips"), Some(3));
        assert_eq!(at("li1"), Some(4));
        assert_eq!(doc.blocks[at("install").unwrap()].kind, BlockKind::Heading(2));

        let laid = crate::layout::layout(&doc, 40, 4, true);
        let row_text =
            |row: usize| laid.rows[row].runs.iter().map(|r| r.text.as_str()).collect::<Vec<_>>().concat();
        assert!(row_text(laid.block_rows[1]).contains("Install"));
        assert!(row_text(laid.block_rows[4]).contains("one"));
    }

    #[test]
    fn every_display_none_spelling_takes_the_subtree_with_it() {
        let cases = [
            ("inline", "<div style=\"display:none\">gone<b>deeper</b></div><p>keep</p>", ""),
            (
                "inline upper case and important",
                "<div style=\"DISPLAY: NONE !important\">gone<b>deeper</b></div><p>keep</p>",
                "",
            ),
            (
                "rule in upper case",
                "<div class=\"ad\">gone<b>deeper</b></div><p>keep</p>",
                ".ad { DISPLAY: NONE }",
            ),
            (
                "important rule beats an inline override",
                "<div class=\"ad\" style=\"display:block\">gone<b>deeper</b></div><p>keep</p>",
                ".ad { display: none !important }",
            ),
            ("hidden attribute", "<div hidden>gone<b>deeper</b></div><p>keep</p>", ""),
        ];
        for (label, html, sheet) in cases {
            let doc = parse_with_css(html, &css::parse(sheet));
            let text: String =
                doc.blocks.iter().flat_map(|b| b.pieces.iter().map(|p| p.text.as_str())).collect();
            assert!(
                !text.contains("gone") && !text.contains("deeper"),
                "{label}: hidden text survived: {text:?}"
            );
            assert!(text.contains("keep"), "{label}: the visible element vanished: {text:?}");
        }
    }

    #[test]
    fn a_hidden_rule_inside_a_media_block_takes_the_subtree() {
        let doc = parse_with_css(
            "<div class=\"ad\">gone<b>deeper</b></div><p class=\"pr\">printed</p><p>keep</p>",
            &css::parse("@media screen { .ad { DISPLAY: NONE } } @media print { .pr { display: none } }"),
        );
        let text: String = doc.blocks.iter().flat_map(|b| b.pieces.iter().map(|p| p.text.as_str())).collect();
        assert!(!text.contains("gone") && !text.contains("deeper"), "hidden text survived: {text:?}");
        assert!(text.contains("printed") && text.contains("keep"), "screen text was dropped: {text:?}");
    }

    #[test]
    fn br_stays_inside_the_block_it_is_written_in() {
        let doc = parse("<p>one<br>two</p>");
        assert_eq!(doc.blocks.len(), 1, "a <br> must not start a new block: {doc:?}");
        assert!(doc.blocks[0].pieces.iter().any(|p| p.hard_break), "no break piece: {doc:?}");
    }
}

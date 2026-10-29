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
use crate::json::Role;
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
    /// Which part of a JSON body this text is, for colour. `Plain` on every page
    /// that is not JSON.
    pub json: Role,
    /// An image or media placeholder such as `[alt text]`: index into
    /// `Doc::media`, so `y` on it can report where the thing itself comes from.
    pub media: Option<usize>,
    /// Index into `Doc::fields`: the user can type into this piece.
    pub field: Option<usize>,
    /// Index into `Doc::disclosures`: this piece belongs to a `<details>` block,
    /// so pressing Enter on it opens or closes what hangs below it.
    pub disclosure: Option<usize>,
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
        /// A heading that opens an item keeps its bullet: the level rides along
        /// here instead of replacing the kind. 0 means the content is not a heading.
        heading: u8,
    },
    /// A block that continues a list item opened earlier: it carries no marker of
    /// its own but hangs under the item's text, so everything inside an `<li>` is
    /// indented to the same column.
    ItemCont {
        depth: u16,
        marker: u16,
    },
    Pre,
    Divider,
    Field,
    /// The summary line of a `<details>` (index into `Doc::disclosures`). The `+`
    /// and `-` that opens and closes it are drawn as the line is laid out, so
    /// toggling a disclosure never has to rewrite the document.
    Summary {
        index: usize,
    },
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

/// A text input or textarea. Text typed into a field belongs to the page: it is
/// laid out and scrolled like any other text, so the value lives in the document
/// and is re-flowed when it changes. Enter in a field hands that value to the
/// `<form>` the control was written in.
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
    /// The `<form>` this control was written inside, an index into `Doc::forms`.
    /// Enter in a field submits that form.
    pub form: Option<usize>,
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

/// A `<form>`: where its fields go and how they get there. w3b only sends a GET,
/// so what matters is the `action` the form points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    /// The `action` attribute as written. Empty means the form has nowhere to go.
    pub action: String,
    /// The `method` lowercased, `get` when the form does not say - the only
    /// method w3b sends.
    pub method: String,
}

#[derive(Debug, Clone)]
pub struct TableCell {
    pub pieces: Vec<Piece>,
    pub header: bool,
    pub align: Align,
    /// How many columns the cell covers, from its `colspan`. Never less than one.
    pub span: usize,
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

/// A `<details>` disclosure. Its summary line is always on the page; what the
/// element holds below it appears only while it is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disclosure {
    /// The summary text, for the message the toggle reports.
    pub label: String,
    pub open: bool,
    /// The disclosure this one is written inside: a closed parent keeps its
    /// children off the page however they are set.
    pub inside: Option<usize>,
}

/// Something the page offers to show or play. w3b has no pictures and no sound,
/// so it draws a label in place of the thing; `y` on that label copies `src`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    /// The tag it was read from: `img`, `video` or `audio`.
    pub kind: &'static str,
    /// Where the thing comes from, as written in the tag. A `<video>` that names
    /// its formats with `<source>` children gets the first of those.
    pub src: String,
    /// The words drawn on the page: `alt`, `title`, `aria-label`, or else the
    /// file name out of the address.
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub kind: BlockKind,
    pub indent: u16,
    pub pieces: Vec<Piece>,
    /// The disclosure whose content this block is: hidden while that disclosure
    /// (or one around it) is closed. `None` means the block is always shown.
    pub hidden_by: Option<usize>,
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
    /// `<form>` elements, in document order: what the fields inside them submit to.
    pub forms: Vec<Form>,
    /// `<details>` disclosures, in document order.
    pub disclosures: Vec<Disclosure>,
    /// Images and media, in document order: w3b draws a label for each one, and
    /// `y` on that label copies the address it was written with.
    pub media: Vec<Media>,
    /// What the page asks for that w3b leaves alone, counted while reading it.
    pub ignored: Ignored,
}

/// What a browser would have gone and got and w3b does not. Counted while the page
/// is read, so the numbers are what is really written there rather than a guess
/// afterwards. Nothing here is an error: it is the shape of a page w3b reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Ignored {
    /// `<img>` elements. w3b draws a label and never loads the pixels.
    pub images: usize,
    /// `<video>` and `<audio>` elements.
    pub video_audio: usize,
    /// `<link rel=stylesheet>`, however many; w3b reads at most
    /// `behavior.max_stylesheets` of them.
    pub stylesheets: usize,
    /// `<script>`, whether it names a file or holds the code inline.
    pub scripts: usize,
    /// `<iframe>`, `<frame>`, `<object>`, `<embed>`: whole other documents.
    pub frames: usize,
    /// `<svg>`, `<math>`, `<canvas>`, `<picture>`, `<source>`, `<track>`,
    /// `<template>`, `<noscript>`, `<map>`: drawn markup rather than text.
    pub markup: usize,
    /// Other `<link>` elements: favicons, icons, preloads, canonicals.
    pub links: usize,
}

impl Doc {
    /// Whether a `<details>` shows what it holds: open itself, and open all the
    /// way up through any disclosure it is written inside.
    pub fn is_open(&self, index: usize) -> bool {
        match self.disclosures.get(index) {
            Some(d) => d.open && d.inside.is_none_or(|outer| self.is_open(outer)),
            None => false,
        }
    }
}

const TABLE_TAGS: &[&str] = &["table", "tr", "td", "th"];

/// Tags the user agent stylesheet already sets in a monospace font: w3b paints
/// those, and anything CSS puts in a monospace family, as a grey block.
const DEFAULT_MONO: &[&str] = &["code", "kbd", "samp", "tt", "kbd", "pre", "textarea"];

/// Tags w3b has no use for: neither they nor anything inside them reaches the
/// page. `img`, `video` and `audio` are deliberately absent - w3b cannot show or
/// play them, but it says they are there and keeps the address they were written
/// with, which `y` copies. A `<video>`'s `<source>` children stay skipped as
/// elements; the arm that handles the `<video>` reads the first of them itself.
const SKIP: &[&str] = &[
    "script", "style", "noscript", "template", "svg", "math", "canvas", "iframe", "object", "embed", "track",
    "picture", "source", "map", "link", "meta",
];

/// Elements that end the current block of text.
/// The tags whose default `display` is block-level. Anything not listed here is
/// inline, exactly as in HTML: `a`, `span`, `img`, `code`, `input`, `button`,
/// `label` and a `select` with its `option`s flow with the text and break no line.
/// A `display` declaration in the page overrides this list.
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
    /// Width of the marker of the item currently open, 0 when no item is open.
    /// Later blocks in that item are indented by this much.
    marker: u16,
}

#[derive(Clone, Copy, Default)]
struct Ctx {
    link: Option<usize>,
    mono: bool,
    bold: bool,
    italic: bool,
    code: bool,
    muted: bool,
    media: Option<usize>,
    field: Option<usize>,
    disclosure: Option<usize>,
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
    /// Open `<details>` elements, outermost first.
    details: Vec<DetailScope>,
    /// Open `<summary>` elements, outermost first: each says which disclosure its
    /// line opens and which one can hide it. A summary is never hidden by its own
    /// disclosure - that line is what the disclosure is opened from - but a closed
    /// one written around it takes the line off the page like anything else inside.
    summaries: Vec<SummaryScope>,
    /// One entry per open `<li>`: the number of blocks when it opened, so an item
    /// that adds no block of its own can be recognised as empty.
    items: Vec<usize>,
    tables: Vec<TableCtx>,
    rows: Vec<Vec<TableCell>>,
    /// Open `<form>` elements: indices into `Doc::forms`, outermost first, so a
    /// field knows which form Enter would submit.
    open_forms: Vec<usize>,
    cell_align: Option<Align>,
    /// The `colspan` of the cell being read: how many columns it covers.
    cell_span: usize,
    /// Depth inside `<td>`/`<th>`: block boundaries are suppressed here so a
    /// cell stays one flow container.
    cell_depth: usize,
    /// The parsed stylesheet, empty for `parse`.
    styles: css::Sheet,
    /// Open elements, outermost first, so a selector can walk the chain.
    path: Vec<css::Identity>,
    /// Set when an element starts or ends. Two elements that meet here are written
    /// without whitespace between them, and still read as separate words.
    edge: bool,
}

#[derive(Debug, Clone, Copy)]
struct TableCtx {
    index: usize,
    indent: u16,
}

/// An open `<details>` element: which disclosure it is, and whether it has
/// named its summary line yet.
#[derive(Debug, Clone, Copy)]
struct DetailScope {
    index: usize,
    summary_taken: bool,
}

/// An open `<summary>` element.
#[derive(Debug, Clone, Copy)]
struct SummaryScope {
    /// The disclosure this line opens, when the line belongs to one.
    owner: Option<usize>,
    /// The disclosure that keeps this line off the page while it is closed.
    hider: Option<usize>,
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

    /// Tally what the page asks a browser to fetch and w3b leaves alone. Only the
    /// elements the walk does not read are counted here; images and media are
    /// labelled on the page, and labelled is not loaded, so their own arm counts
    /// them. Elements hidden inside a skipped element are not counted: a browser
    /// does not fetch those either.
    fn count_ignored(&mut self, node: NodeRef<'_, Node>, name: &str) {
        let el = match node.value().as_element() {
            Some(el) => el,
            None => return,
        };
        let ignored = &mut self.doc.ignored;
        match name {
            "script" => ignored.scripts += 1,
            "link" => {
                let rel = el.attr("rel").unwrap_or_default().to_ascii_lowercase();
                if rel.split_whitespace().any(|r| r == "stylesheet") {
                    ignored.stylesheets += 1;
                } else {
                    ignored.links += 1;
                }
            }
            "iframe" | "frame" | "object" | "embed" => ignored.frames += 1,
            "svg" | "math" | "canvas" | "picture" | "source" | "track" | "template" | "noscript" | "map" => {
                ignored.markup += 1
            }
            // <style> is read (embedded CSS is parsed) and <meta>/<title> ask for
            // nothing, so neither belongs in the tally.
            _ => {}
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
            self.count_ignored(node, name);
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
        self.edge = true;
        // Where lines break is the `display` property's job; the tag list above is
        // only the default for elements that declare none. Structural elements keep
        // their own layout, because w3b lays tables, lists and <pre> out by tag.
        let display = self.styles.value(&self.target(&identity), "display", inline.as_deref());
        let is_block = match display.as_deref().and_then(display_is_block) {
            _ if TABLE_TAGS.contains(&name) || STRUCTURE.contains(&name) => true,
            Some(on_line) => on_line,
            None => BLOCK.contains(&name),
        };
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
                let level = name[1..2].parse().unwrap_or(1);
                self.flush();
                match &mut self.pending_block {
                    // A heading that opens a list item keeps the marker: the level
                    // rides on the item rather than replacing it.
                    Some(BlockKind::Item { heading, .. }) => *heading = level,
                    _ => self.pending_block = Some(BlockKind::Heading(level)),
                }
            }
            "hr" => {
                self.flush();
                self.push_block(Block {
                    kind: BlockKind::Divider,
                    indent: self.indent,
                    pieces: vec![],
                    hidden_by: None,
                });
            }
            "br" => self.push_break(),
            "form" => {
                // A form is remembered for one thing: Enter in one of its fields
                // goes to its action.
                self.flush();
                let el = node.value().as_element();
                let action = el.and_then(|e| e.attr("action")).map(collapse).unwrap_or_default();
                let method = el
                    .and_then(|e| e.attr("method"))
                    .map(|m| m.trim().to_ascii_lowercase())
                    .filter(|m| !m.is_empty())
                    .unwrap_or_else(|| "get".to_string());
                self.open_forms.push(self.doc.forms.len());
                self.doc.forms.push(Form { action, method });
            }
            "pre" => {
                self.flush();
                self.pre = true;
            }
            "blockquote" => self.indent += 1,
            "details" => {
                self.flush();
                let inside = self.details.last().map(|d| d.index);
                let index = self.doc.disclosures.len();
                let open = node.value().as_element().map(|e| e.attr("open").is_some()).unwrap_or(false);
                self.doc.disclosures.push(Disclosure { label: String::new(), open, inside });
                self.details.push(DetailScope { index, summary_taken: false });
                self.indent += 1;
                let named = names_summary(node);
                if !named && self.cell_depth == 0 {
                    // A <details> that does not name a summary still needs a line
                    // to open it from, and that line says what it is.
                    self.doc.disclosures[index].label = "details".to_string();
                    self.summary_line(index, "details");
                }
            }
            "summary" => {
                self.flush();
                let scope = self.open_summary();
                // Inside a table cell there are no blocks to hide - a cell is one
                // flow container - so a disclosure there would be a knob with
                // nothing to turn: it stays plain text.
                if self.cell_depth == 0 {
                    if let Some(owner) = scope.owner {
                        if let Some(d) = self.doc.disclosures.get_mut(owner) {
                            d.label = collapse(&collect_text(node));
                        }
                        // The words of the summary carry the disclosure, so Enter
                        // anywhere on the line opens or closes what is below it.
                        self.ctx.disclosure = Some(owner);
                    }
                }
            }
            "ul" | "ol" => self.lists.push(ListCtx {
                depth: self.lists.len() as u16 + 1,
                ordered: name == "ol",
                // A list may be told where to count from. One that is not, or is
                // told in a language no number speaks, counts from one anyway.
                index: node
                    .value()
                    .as_element()
                    .and_then(|e| e.attr("start"))
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    // The attribute counts from one, this counts from nothing.
                    .map(|n| n.saturating_sub(1))
                    .unwrap_or(0),
                marker: 0,
            }),
            "li" => {
                self.flush();
                self.items.push(self.doc.blocks.len());
                let kind = match self.lists.last_mut() {
                    Some(list) => {
                        let kind = BlockKind::Item {
                            depth: list.depth,
                            ordered: list.ordered,
                            index: list.index,
                            heading: 0,
                        };
                        list.marker = marker_width(list.ordered, list.index);
                        list.index += 1;
                        kind
                    }
                    // Stray <li> without a list parent: still render as a bullet.
                    None => BlockKind::Item { depth: 1, ordered: false, index: 0, heading: 0 },
                };
                self.pending_block = Some(kind);
            }
            "img" | "video" | "audio" => {
                // A label made of whatever the thing says about itself; one that
                // says nothing is left off the page entirely. Its children - a
                // fallback link, a `<source>` - are not walked: the label is the
                // whole of it, which also keeps one `<video>` to one line.
                let el = match node.value().as_element() {
                    Some(el) => el,
                    None => return,
                };
                let src = media_src(node, el);
                let label = collapse(&media_label(el, &src));
                let kind = match el.name() {
                    "video" => "video",
                    "audio" => "audio",
                    _ => "img",
                };
                // Labelled is not loaded: even an image that says nothing about
                // itself is one more thing the page wanted and w3b did not ask for.
                match kind {
                    "img" => self.doc.ignored.images += 1,
                    _ => self.doc.ignored.video_audio += 1,
                }
                if label.is_empty() {
                    return;
                }
                let index = self.doc.media.len();
                self.doc.media.push(Media { kind, src, label: label.clone() });
                self.push_piece(format!("[{kind}: {label}]"), |c| c.media = Some(index));
                walk_children = false;
            }
            "input" | "textarea" | "select" | "button" => {
                // The label is the element's own text, so its children are not walked.
                // Text inputs and textareas become editable fields.
                if let Some(field) = field_of(node, self.open_forms.last().copied()) {
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
                self.cell_span = cell_span(node);
                self.cell_align = Some(match node.value().as_element().and_then(|e| e.attr("align")) {
                    Some("center") => Align::Center,
                    Some("right") => Align::Right,
                    _ => Align::Left,
                });
            }
            // Block elements end the current run of text.
            _ if is_block => self.flush(),
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
        self.edge = true;

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
                let cell = TableCell {
                    pieces: std::mem::take(&mut self.cur),
                    header,
                    align,
                    span: self.cell_span.max(1),
                };
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
                        hidden_by: None,
                    };
                    self.push_block(block);
                }
            }
            "ul" | "ol" => {
                self.lists.pop();
            }
            "form" => {
                self.open_forms.pop();
            }
            "details" => {
                // Flush while the element is still open: its content belongs to it.
                // A <summary> left unclosed is still the line the disclosure is
                // opened from, so it is flushed as a summary rather than a paragraph.
                match self.summaries.last().and_then(|s| s.owner) {
                    Some(owner) => {
                        self.flush_as(BlockKind::Summary { index: owner });
                        self.summaries.pop();
                        self.ctx.disclosure = None;
                    }
                    None => self.flush(),
                }
                if self.details.pop().is_some() {
                    self.indent = self.indent.saturating_sub(1);
                }
            }
            "summary" => {
                // Flush while the scope is still open: the hider it recorded decides
                // whether this line is on the page, and a summary line stays on it
                // whether its own disclosure is open or closed.
                match self.summaries.last().and_then(|s| s.owner) {
                    Some(owner) => self.flush_as(BlockKind::Summary { index: owner }),
                    None => self.flush(),
                }
                self.summaries.pop();
                self.ctx.disclosure = None;
            }
            "li" => {
                self.flush();
                let opened_at = self.items.pop().unwrap_or(self.doc.blocks.len());
                // An item that says nothing - no words of its own, no block of its
                // own, whitespace and breaks at most - is not on the page at all:
                // no marker, no line, and no number in an ordered list.
                let silent = !self.has_text() && !self.adds_content(opened_at);
                if silent {
                    // Drop the item whole: the blocks it added, the text it collected
                    // and the marker that waited for them.
                    self.doc.blocks.truncate(opened_at);
                    self.cur.clear();
                    self.pending_block = None;
                } else if let Some(kind @ BlockKind::Item { depth, .. }) = self.pending_block.take() {
                    // The content may have gone into blocks of its own - a <pre>, a
                    // nested block - while the marker waited. Put the marker on the
                    // first of those blocks instead of on a line of its own.
                    let mut first = None;
                    for (n, b) in self.doc.blocks.iter().enumerate().skip(opened_at).rev() {
                        if matches!(b.kind, BlockKind::ItemCont { depth: d, .. } if d == depth) {
                            first = Some(n);
                        } else {
                            break;
                        }
                    }
                    match first {
                        Some(n) => self.doc.blocks[n].kind = kind,
                        // An item with no text of its own but content in nested
                        // blocks still shows its marker.
                        None => self.flush_as(kind),
                    }
                }
                if let Some(list) = self.lists.last_mut() {
                    list.marker = 0;
                    if silent {
                        // A silent item does not use up a number either.
                        list.index = list.index.saturating_sub(1);
                    }
                }
            }
            _ => {}
        }

        if is_block && name != "br" && !TABLE_TAGS.contains(&name) {
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
        // A `data:` payload is a base64 blob, not somewhere a reader can go, and a
        // `javascript:` handler is not a destination either: both stay plain text.
        if href.is_empty()
            || href.get(..5).is_some_and(|h| h.eq_ignore_ascii_case("data:"))
            || href.get(..11).is_some_and(|h| h.eq_ignore_ascii_case("javascript:"))
        {
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
        // An element meeting another element with no whitespace between them gets a
        // separator anyway: `<img><img>`, `</a>word`, `<b>x</b>y`.
        let gap = self.edge
            && !self.ws
            && match (self.cur.last(), text.chars().next()) {
                (Some(prev), Some(first)) => element_gap(&prev.text, first),
                _ => false,
            };
        self.edge = false;
        if (self.ws || gap) && !self.cur.is_empty() {
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
            media: ctx.media,
            field: ctx.field,
            disclosure: ctx.disclosure,
            json: Role::Plain,
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

    /// Whether the text collected so far says anything: separator spaces, `<br>`
    /// and whitespace-only runs are not content.
    fn has_text(&self) -> bool {
        self.cur.iter().any(|p| !p.hard_break && !p.text.trim().is_empty())
    }

    /// Whether the blocks added since `from` are worth a line: an empty `<pre>` line
    /// and a whitespace-only run say as little as no blocks at all, while a rule or
    /// a table draws something.
    fn adds_content(&self, from: usize) -> bool {
        self.doc.blocks[from..].iter().any(|b| {
            matches!(b.kind, BlockKind::Divider | BlockKind::Table { .. })
                || b.pieces.iter().any(|p| !p.hard_break && !p.text.trim().is_empty())
        })
    }

    fn flush(&mut self) {
        if let Some(kind) = self.pending_block.take() {
            if matches!(kind, BlockKind::Item { .. }) && !self.has_text() {
                // The marker of an item whose content arrives in nested blocks waits
                // for that content instead of standing on a line of its own.
                self.pending_block = Some(kind);
                return;
            }
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
                BlockKind::Item { depth, ordered, index, heading } => {
                    self.push_block(Block {
                        kind: BlockKind::Item { depth, ordered, index, heading },
                        indent: self.indent,
                        pieces: vec![],
                        hidden_by: None,
                    });
                }
                // Keep blank lines inside <pre>.
                BlockKind::Pre => {
                    self.push_block(Block { kind, indent: self.indent, pieces: vec![], hidden_by: None })
                }
                // A summary with no words still gets its marker: it is the only line
                // the disclosure can be opened from.
                k @ BlockKind::Summary { .. } => {
                    self.push_block(Block { kind: k, indent: self.indent, pieces: vec![], hidden_by: None })
                }
                _ => {}
            }
            return;
        }
        // A block inside an item that is still open continues that item: it gets no
        // marker and is lined up under the item's text.
        let kind = match kind {
            k @ (BlockKind::Paragraph | BlockKind::Heading(_) | BlockKind::Pre) => match self.lists.last() {
                Some(list) if list.marker > 0 => {
                    BlockKind::ItemCont { depth: list.depth, marker: list.marker }
                }
                _ => k,
            },
            other => other,
        };
        let indent = match kind {
            // An item knows its own depth; adding the list term would count it twice.
            BlockKind::Item { .. } | BlockKind::ItemCont { .. } => self.indent,
            _ => self.indent + self.lists.last().map_or(0, |l| l.depth.saturating_sub(1)),
        };
        let block = Block { kind, indent, pieces: std::mem::take(&mut self.cur), hidden_by: None };
        self.push_block(block);
    }

    /// A summary line for a `<details>` that does not name one, so there is always
    /// a line to open the disclosure from.
    fn summary_line(&mut self, index: usize, label: &str) {
        self.open_summary();
        self.ctx.disclosure = Some(index);
        self.push_piece(label.to_string(), |c| c.disclosure = Some(index));
        self.flush_as(BlockKind::Summary { index });
        self.ctx.disclosure = None;
        self.summaries.pop();
    }

    /// Begin a summary line. It belongs to the innermost `<details>` that has not
    /// had one yet, and only a disclosure written *around* that element can hide
    /// the line: it is what the disclosure is opened and closed from.
    fn open_summary(&mut self) -> SummaryScope {
        let owner = match self.details.last_mut() {
            Some(scope) if !scope.summary_taken => {
                scope.summary_taken = true;
                Some(scope.index)
            }
            // A second <summary> in one <details>, or one written outside any: an
            // ordinary line inside whatever is open, with nothing to toggle.
            other => other.map(|s| s.index),
        };
        let hider = match self.details.len().checked_sub(2) {
            Some(i) => Some(self.details[i].index),
            None => None,
        };
        let hider = hider.filter(|outer| Some(*outer) != owner);
        let scope = SummaryScope { owner, hider };
        self.summaries.push(scope);
        scope
    }

    /// The disclosure a new block belongs to: the innermost open `<details>`, or
    /// for a summary line the disclosure around it.
    fn enclosing_disclosure(&self) -> Option<usize> {
        match self.summaries.last() {
            Some(scope) => scope.hider,
            None => self.details.last().map(|d| d.index),
        }
    }

    fn push_block(&mut self, mut block: Block) {
        if block.hidden_by.is_none() {
            block.hidden_by = self.enclosing_disclosure();
        }
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
        media: ctx.media,
        field: ctx.field,
        disclosure: ctx.disclosure,
        json: Role::Plain,
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
/// radios, buttons, selects, file pickers) keeps its static label: w3b submits a
/// form with Enter in a field, but it never presses a button of its own.
/// Whether two elements that meet without whitespace need a separator between
/// them. None goes in front of closing punctuation - `done</a>.` stays `done.` -
/// and none after an opening one.
fn element_gap(prev: &str, first: char) -> bool {
    match (prev.chars().next_back(), Some(first)) {
        (Some(prev), _) if "([{\u{ab}\"'\u{201c}".contains(prev) => false,
        (_, Some(first)) if ".,;:!?)]}%\"'\u{201d}".contains(first) => false,
        _ => true,
    }
}

/// Tags whose layout comes from the tag itself whatever CSS declares: tables are
/// laid out as grids, list items carry markers, `<pre>` keeps its shape.
const STRUCTURE: &[&str] = &[
    "table", "thead", "tbody", "tfoot", "tr", "td", "th", "caption", "colgroup", "col", "ul", "ol", "li",
    "details", "summary", "pre", "hr", "br",
];

/// Whether a `display` value puts an element on a line of its own. `inline-block`
/// counts as inline here: w3b has no box model, and the useful reading of it is
/// "text that does not break the line". An unknown value leaves the tag's default.
fn display_is_block(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "inline" | "inline-block" | "inline-flex" | "inline-grid" | "inline-table" | "contents"
        | "run-in" => Some(false),
        "block" | "flow-root" | "list-item" | "table" | "flex" | "grid" | "table-row" | "table-row-group"
        | "table-header-group" | "table-footer-group" | "table-cell" | "table-caption" => Some(true),
        _ => None,
    }
}

/// Whether a `<details>` names its own summary; when it does not, w3b supplies
/// one so the disclosure can still be opened.
fn names_summary(node: NodeRef<'_, Node>) -> bool {
    node.children().filter_map(|c| c.value().as_element()).next().is_some_and(|el| el.name() == "summary")
}

/// How many columns a `<td>`/`<th>` covers. A `colspan` that is not a number of at
/// least one spans nothing extra, and a wild value is held down so one mistyped
/// attribute cannot ask for a thousand columns.
fn cell_span(node: NodeRef<'_, Node>) -> usize {
    node.value()
        .as_element()
        .and_then(|e| e.attr("colspan"))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(1)
        .min(1000)
}

/// The marker a list item draws: `one.` for ordered, a bullet for unordered.
fn marker_width(ordered: bool, index: u32) -> u16 {
    if ordered {
        format!("{index}.").chars().count() as u16
    } else {
        1
    }
}

/// The words a media placeholder carries: alt text, otherwise the title,
/// otherwise aria-label, otherwise the file name out of `src`. Empty means the
/// media has nothing to say - an explicit `alt=""`, or a base64 `data:` address
/// with no name to show - and such a thing is left out of the page.
fn media_label(el: &scraper::node::Element, src: &str) -> String {
    for name in ["alt", "title", "aria-label"] {
        let value = el.attr(name).unwrap_or("").trim();
        if !value.is_empty() {
            return value.to_string();
        }
        if name == "alt" && el.attr("alt").is_some() {
            return String::new(); // alt="" is the author saying: no information here
        }
    }
    if src.is_empty() || src.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("data:")) {
        return String::new();
    }
    src.split(['?', '#']).next().unwrap_or("").rsplit('/').next().unwrap_or("").to_string()
}

/// Where the media comes from: its own `src`, otherwise the first `<source>`
/// written inside it, which is how a `<video>` offers one clip in several
/// formats. Empty when the tag does not say.
fn media_src(node: NodeRef<'_, Node>, el: &scraper::node::Element) -> String {
    let own = el.attr("src").unwrap_or("").trim();
    if !own.is_empty() {
        return own.to_string();
    }
    node.children()
        .filter_map(|child| child.value().as_element())
        .find(|child| child.name() == "source")
        .and_then(|child| child.attr("src"))
        .unwrap_or("")
        .trim()
        .to_string()
}

fn field_of(node: NodeRef<'_, Node>, form: Option<usize>) -> Option<Field> {
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
            name,
            placeholder: "textarea".to_string(),
            value,
            masked: false,
            form,
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
        name,
        placeholder,
        value: el.attr("value").map(collapse).unwrap_or_default(),
        masked,
        form,
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
                BlockKind::Item { depth, ordered, index, .. } => {
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
        assert!(matches!(doc.blocks[0].kind, BlockKind::Item { depth: 1, ordered: false, index: 0, .. }));
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
    fn a_field_belongs_to_the_form_it_is_written_in() {
        let doc = parse(
            "<form action=/search method=GET><input name=q><textarea name=body>hi</textarea></form>\
             <form action=/x method=post><input name=p></form>\
             <input value=orphan>",
        );
        assert_eq!(doc.forms.len(), 2);
        assert_eq!(doc.forms[0].action, "/search");
        assert_eq!(doc.forms[0].method, "get", "a form that does not say asks for a GET");
        assert_eq!(doc.forms[1].method, "post");
        let owner: Vec<Option<usize>> = doc.fields.iter().map(|f| f.form).collect();
        assert_eq!(owner, vec![Some(0), Some(0), Some(1), None]);
        let names: Vec<&str> = doc.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["q", "body", "p", ""], "a control with no name has nothing to be called");
    }

    #[test]
    fn img_alt_and_form_fields_become_placeholders() {
        let doc = parse("<img src=\"x.png\" alt=\"logo\"><input type=\"checkbox\"><button>Send</button>");
        let all: String = doc.blocks.iter().flat_map(|b| b.pieces.iter().map(|p| p.text.as_str())).collect();
        assert_eq!(all, "[img: logo] [ ] [ Send ]", "adjacent placeholders are separated");
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
    fn a_cell_that_spans_says_how_many_columns_it_covers() {
        let doc = parse(
            "<table><tr><td colspan=2>wide</td><td>x</td></tr>\
             <tr><td>a</td><td>b</td><td>c</td></tr></table>",
        );
        assert_eq!(doc.tables[0].rows[0].iter().map(|c| c.span).collect::<Vec<_>>(), vec![2, 1]);
        assert_eq!(doc.tables[0].rows[0].len(), 2, "a spanning cell is still one cell");
        assert_eq!(doc.tables[0].rows[1].iter().map(|c| c.span).collect::<Vec<_>>(), vec![1, 1, 1]);
        for inner in ["colspan=0", "colspan", "colspan=abc"] {
            let doc = parse(&format!("<table><tr><td {inner}>x</td><td>y</td></tr></table>"));
            assert_eq!(doc.tables[0].rows[0][0].span, 1, "{inner} spans no more than its own column");
        }
        let doc = parse("<table><tr><td colspan=4000000000>x</td></tr></table>");
        assert!(doc.tables[0].rows[0][0].span <= 1000, "a wild colspan cannot ask for the world");
    }

    #[test]
    fn block_tags_inside_a_cell_do_not_split_the_cell() {
        let doc = parse("<table><tr><td><p>one</p><p>two</p></td><td>x</td></tr></table>");
        let cell = &doc.tables[0].rows[0][0];
        let text: String = cell.pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(text, "one two", "the paragraphs in a cell stay separate words");
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

    #[test]
    fn an_image_that_says_nothing_is_left_out() {
        let labels = |html: &str| -> Vec<String> {
            let doc = parse(html);
            doc.blocks
                .iter()
                .flat_map(|b| b.pieces.iter().filter(|p| p.media.is_some()).map(|p| p.text.clone()))
                .collect()
        };
        assert_eq!(labels("<img src=\"a.png\" alt=\"logo\">"), vec!["[img: logo]"]);
        assert_eq!(labels("<img src=\"a.png\" title=\"the title\">"), vec!["[img: the title]"]);
        assert_eq!(labels("<img src=\"a.png\" aria-label=\"aria\">"), vec!["[img: aria]"]);
        assert_eq!(labels("<img src=\"/img/logo.png?v=2#x\">"), vec!["[img: logo.png]"]);
        // alt="" is the author saying the image carries no information.
        assert!(labels("<img src=\"decorative.png\" alt=\"\">").is_empty(), "a decorative image was shown");
        // A base64 payload has no name to show.
        assert!(
            labels("<img src=\"data:image/png;base64,iVBORw0KGgo=\">").is_empty(),
            "a base64 image was shown"
        );
    }

    #[test]
    fn a_media_label_keeps_the_address_it_was_written_with() {
        let media = |html: &str| {
            parse(html)
                .media
                .into_iter()
                .map(|m| (m.kind, m.src, m.label))
                .collect::<Vec<(&'static str, String, String)>>()
        };
        assert_eq!(
            media("<img src=\"/pics/logo.png\" alt=\"logo\">"),
            vec![("img", "/pics/logo.png".into(), "logo".into())]
        );
        // Sound and moving pictures are named by what they are.
        assert_eq!(
            media("<video src=\"talk.mp4\" title=\"the talk\">"),
            vec![("video", "talk.mp4".into(), "the talk".into())]
        );
        // With no words about itself, the file name in the address is the label.
        assert_eq!(
            media("<audio src=\"/radio/jingle.mp3\">"),
            vec![("audio", "/radio/jingle.mp3".into(), "jingle.mp3".into())]
        );
        // A clip offered in several formats is still one thing: the first address wins.
        assert_eq!(
            media("<video><source src=\"clip.webm\"><source src=\"clip.mp4\">your browser cannot play this</video>"),
            vec![("video", "clip.webm".into(), "clip.webm".into())]
        );
        // An image that names no address still shows its alt text; it just has
        // nothing to hand over.
        assert_eq!(
            media("<img alt=\"painted by hand\">"),
            vec![("img", String::new(), "painted by hand".into())]
        );
    }

    #[test]
    fn a_media_label_is_one_line_and_carries_what_is_inside_it() {
        let doc = parse("<p>before <img src=\"a.png\" alt=\"a\"> after</p><video src=\"c.mp4\" aria-label=\"clip\"><a href=\"/c.mp4\">get the clip</a></video>");
        let labels: Vec<(String, Option<usize>)> = doc
            .blocks
            .iter()
            .flat_map(|b| b.pieces.iter().filter(|p| p.media.is_some()).map(|p| (p.text.clone(), p.media)))
            .collect();
        assert_eq!(labels, vec![("[img: a]".to_string(), Some(0)), ("[video: clip]".to_string(), Some(1))]);
        // The fallback link inside the element is not put on the page as well:
        // one label per thing, and it already says where the thing is.
        assert!(doc.links.is_empty(), "the fallback inside a media element was shown too");
    }

    #[test]
    fn base64_and_javascript_targets_stay_plain_text() {
        let doc = parse(
            "<a href=\"data:text/html;base64,AAAA\">blob</a><a href=\"javascript:void(0)\">click</a><a href=\"/real\">real</a>",
        );
        let hrefs: Vec<&str> = doc.links.iter().map(|l| l.href.as_str()).collect();
        assert_eq!(hrefs, vec!["/real"], "only the real target is a link");
        let all: String = doc.blocks.iter().flat_map(|b| b.pieces.iter().map(|p| p.text.as_str())).collect();
        assert!(all.contains("blob") && all.contains("click"), "the text is still read: {all}");
    }

    #[test]
    fn an_item_stays_one_item_however_its_content_is_written() {
        let doc = parse("<ul><li><p>alpha</p><div>beta</div></li></ul>");
        let kinds: Vec<BlockKind> = doc.blocks.iter().map(|b| b.kind).collect();
        assert!(
            matches!(kinds[0], BlockKind::Item { depth: 1, ordered: false, index: 0, heading: 0 }),
            "{kinds:?}"
        );
        assert!(matches!(kinds[1], BlockKind::ItemCont { depth: 1, marker: 1 }), "{kinds:?}");
        let doc = parse("<ul><li><h2>head</h2></li></ul>");
        assert!(
            matches!(doc.blocks[0].kind, BlockKind::Item { depth: 1, heading: 2, .. }),
            "{:?}",
            doc.blocks[0].kind
        );
    }

    #[test]
    fn an_item_that_says_nothing_is_not_on_the_page() {
        // No marker, no line, no blank line: an empty item is as invisible as an
        // empty list, however it was written.
        let empties = [
            "<ul></ul>",
            "<ol>  </ol>",
            "<ul><li></li></ul>",
            "<ul><li>  \n </li></ul>",
            "<ol><li>&nbsp;</li></ol>",
            "<ul><li><br></li></ul>",
            "<ul><li><p>  </p></li></ul>",
            "<ul><li><pre>\n\n</pre></li></ul>",
            "<ul><li><img alt=\"\"></li></ul>",
            "<ul><li><ul></ul></li></ul>",
            "<ul><li><ul><li></li></ul></li></ul>",
        ];
        for html in empties {
            let doc = parse(html);
            assert!(doc.blocks.is_empty(), "{html} left {:?} behind", doc.blocks);
            assert_eq!(crate::dump::render_html(html, 20), "\n", "{html} still takes a line");
        }
    }

    #[test]
    fn an_empty_item_does_not_use_up_a_number_or_a_line() {
        assert_eq!(
            crate::dump::render_html("<ol><li>one</li><li></li><li>two</li></ol>", 20),
            "1. one\n2. two\n",
            "the list is still numbered through the gap"
        );
        assert_eq!(
            crate::dump::render_html("<ul><li>a<p>x</p></li><li></li></ul>", 20),
            "\u{2022} a\n  x\n",
            "an empty item must not steal the marker from the item before it"
        );
    }

    #[test]
    fn a_closed_disclosure_shows_only_its_summary_line() {
        let html = "<p>before</p><details><summary>Anything</summary><p>Body</p></details><p>after</p>";
        let doc = parse(html);
        assert_eq!(doc.disclosures.len(), 1);
        assert_eq!(doc.disclosures[0].label, "Anything", "the summary says what it opens");
        assert!(!doc.is_open(0), "closed unless it says open");
        assert_eq!(crate::dump::render_html(html, 30), "before\n\n  + Anything\n\nafter\n");
    }

    #[test]
    fn an_open_disclosure_shows_what_it_holds() {
        let html = "<p>before</p><details open><summary>Anything</summary><p>Body</p></details><p>after</p>";
        let doc = parse(html);
        assert!(doc.is_open(0));
        assert_eq!(crate::dump::render_html(html, 30), "before\n\n  - Anything\n\n  Body\n\nafter\n");
    }

    #[test]
    fn opening_a_disclosure_puts_its_content_in_the_lines_below() {
        // What a <details> holds is part of the document all along; only the marker
        // and the rows come and go, which is what lets the same Enter close it.
        let mut doc = parse("<details><summary>Anything</summary><p>Body</p></details>");
        let shown = |doc: &Doc| crate::dump::plain_text(&crate::layout::layout(doc, 30, 4, true));
        assert_eq!(shown(&doc), "  + Anything\n");
        doc.disclosures[0].open = true;
        assert_eq!(shown(&doc), "  - Anything\n\n  Body\n", "the content is laid in below");
        doc.disclosures[0].open = false;
        assert_eq!(shown(&doc), "  + Anything\n", "and away again, exactly as it was");
    }

    #[test]
    fn a_details_that_does_not_name_a_summary_gets_one() {
        // A line is needed to open it from, so one is supplied.
        let mut doc = parse("<details><p>bare body</p></details>");
        assert_eq!(doc.disclosures[0].label, "details");
        let shown = |doc: &Doc| crate::dump::plain_text(&crate::layout::layout(doc, 30, 4, true));
        assert_eq!(shown(&doc), "  + details\n");
        doc.disclosures[0].open = true;
        assert_eq!(shown(&doc), "  - details\n\n  bare body\n");
    }

    #[test]
    fn a_nested_disclosure_waits_for_its_parent() {
        let html =
            "<details><summary>a</summary>outer<details open><summary>b</summary>inner</details></details>";
        let doc = parse(html);
        assert!(doc.disclosures[1].open, "the inner one says open");
        assert!(!doc.is_open(1), "a closed parent keeps it off the page however it is set");
        assert_eq!(crate::dump::render_html(html, 30), "  + a\n", "even its own summary line");
    }

    #[test]
    fn a_summary_line_is_the_line_that_opens() {
        let doc = parse("<details><summary>Anything</summary><p>Body</p></details>");
        let kinds: Vec<BlockKind> = doc.blocks.iter().map(|b| b.kind).collect();
        assert!(matches!(kinds[0], BlockKind::Summary { index: 0 }), "{kinds:?}");
        assert!(
            doc.blocks[0].pieces.iter().all(|p| p.disclosure == Some(0)),
            "the words of the line carry the disclosure"
        );
        assert_eq!(doc.blocks[0].hidden_by, None, "the line it is opened from is never hidden");
        assert_eq!(doc.blocks[1].hidden_by, Some(0), "what it holds is");
    }

    #[test]
    fn a_disclosure_inside_a_cell_is_plain_text() {
        // A table cell is one block of text, so there are no lines to put content
        // into below it: the summary stays part of the cell.
        let html = "<table><tr><td><details><summary>a</summary>b</details></td></tr></table>";
        let doc = parse(html);
        assert_eq!(doc.disclosures.len(), 1, "it is still a disclosure");
        assert_eq!(doc.blocks.len(), 1);
        assert!(matches!(doc.blocks[0].kind, BlockKind::Table { .. }));
        assert_eq!(doc.blocks[0].hidden_by, None, "nothing can hide a whole cell");
    }

    fn styled(html: &str, sheet: &str) -> Doc {
        parse_with_css(html, &css::parse(sheet))
    }

    #[test]
    fn display_wins_over_the_tag_for_line_breaks() {
        let kinds = |doc: &Doc| -> Vec<BlockKind> { doc.blocks.iter().map(|b| b.kind).collect() };
        let flow = |doc: &Doc| -> String {
            doc.blocks.iter().flat_map(|b| b.pieces.iter().map(|p| p.text.as_str())).collect()
        };
        let doc = styled("<div>one</div><div>two</div>", "div { display: inline }");
        assert_eq!(doc.blocks.len(), 1, "an inline <div> does not break the line: {:?}", kinds(&doc));
        assert_eq!(flow(&doc), "one two", "elements on one line are still separated");
        let doc = styled("<div>a<span>b</span>c</div>", "span { display: block }");
        assert_eq!(doc.blocks.len(), 3, "a block <span> breaks the line: {:?}", kinds(&doc));
        // An unknown value leaves the tag alone.
        let doc = styled("<div>one</div><div>two</div>", "div { display: talble }");
        assert_eq!(doc.blocks.len(), 2, "{:?}", kinds(&doc));
    }

    #[test]
    fn inline_block_counts_as_inline_and_structure_does_not_move() {
        let doc = styled("<span>a</span><span>b</span>", "span { display: inline-block }");
        assert_eq!(doc.blocks.len(), 1, "{:?}", doc.blocks.iter().map(|b| b.kind).collect::<Vec<_>>());
        let doc = styled("<ul><li>one</li></ul>", "ul, li { display: inline }");
        assert!(
            matches!(doc.blocks[0].kind, BlockKind::Item { .. }),
            "a list item keeps its marker: {:?}",
            doc.blocks[0].kind
        );
    }

    #[test]
    fn buttons_and_labels_are_inline() {
        let doc = parse("<p>name <input name=q><button>Send</button> done</p>");
        assert_eq!(doc.blocks.len(), 1, "a button sits in the sentence: {:?}", doc.blocks.len());
        let text: String = doc.blocks[0].pieces.iter().map(|p| p.text.as_str()).collect();
        assert!(text.contains("[ Send ]"), "{text}");
    }

    #[test]
    fn elements_that_meet_without_whitespace_are_still_separated() {
        let flow = |html: &str| -> String {
            let doc = parse(html);
            doc.blocks.iter().flat_map(|b| b.pieces.iter().map(|p| p.text.as_str())).collect()
        };
        assert_eq!(flow("<p><b>bold</b>word</p>"), "bold word");
        assert_eq!(flow("<p><img src=a.png alt=one><img src=b.png alt=two></p>"), "[img: one] [img: two]");
        // Punctuation stays glued to the word it belongs to.
        assert_eq!(flow("<p>see <a href=/x>the docs</a>.</p>"), "see the docs.");
        assert_eq!(flow("<p>say <b>hello</b>, friend</p>"), "say hello, friend");
    }

    #[test]
    fn inline_level_tags_break_no_line() {
        // A <select> is inline-block in HTML, and w3b reads that as inline: it sits
        // in the sentence and breaks nothing after it.
        let doc = parse("<p>pick <select><option>one</option></select> now</p>");
        assert_eq!(doc.blocks.len(), 1, "{:?}", doc.blocks.iter().map(|b| b.kind).collect::<Vec<_>>());
        let text: String = doc.blocks[0].pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(text.trim(), "pick [ input ] now");
        // A tag w3b has never heard of is inline as well.
        let doc = parse("<p>a<custom-widget>b</custom-widget>c</p>");
        assert_eq!(doc.blocks.len(), 1);
        let text: String = doc.blocks[0].pieces.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(text, "a b c");
    }
    #[test]
    fn the_page_counts_what_w3b_leaves_out() {
        let doc = parse(
            "<head><link rel=stylesheet href=/a.css><link rel=stylesheet href=b.css>\
             <link rel=icon href=/f.ico><meta charset=utf-8><style>p{color:red}</style></head>\
             <body><script src=/a.js></script><script>inline()</script>\
             <iframe src=/fr></iframe><object data=/o></object>\
             <p><img src=/1.png alt=one><img alt=blank><video><source src=/v.webm></video>\
             <svg></svg><canvas></canvas><template>t</template></p>",
        );
        // A `<style>` is read (embedded CSS is parsed) and `<meta>` asks for
        // nothing, so neither is counted. A `<source>` inside a `<video>` is part of
        // that one label, so it is not counted twice either.
        assert_eq!(
            doc.ignored,
            Ignored { images: 2, video_audio: 1, stylesheets: 2, scripts: 2, frames: 2, markup: 3, links: 1 }
        );
    }
}

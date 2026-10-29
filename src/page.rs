//! A rendered resource: parsed document + link targets + laid out rows.
use std::collections::{BTreeSet, VecDeque};
use std::time::Instant;
use url::Url;

use crate::charset;
use crate::css;
use crate::dom::{Block, BlockKind, Doc, Link, Piece};
use crate::error::NetError;
use crate::json::Role;
use crate::layout::{self, Layout};
use crate::net::{Fetched, Fetcher, Kind};
use crate::status::Status;

/// Where the cursor's link points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Follow {
    /// A URL w3b can fetch.
    Url(String),
    /// Same-document anchor.
    Anchor,
    /// Scheme w3b does not handle (mailto, javascript, ...).
    Unsupported(&'static str),
}

/// What pressing Enter in a form field does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Submit {
    /// A GET form with an action: go here, with the form's values in the query.
    Go(String),
    /// The form asks for a method w3b does not send; the name is for the message.
    Method(String),
    /// The action is a scheme w3b does not fetch (mailto, javascript, ...).
    Unsupported(&'static str),
    /// Nothing to submit to: the field is not in a form, or the form has no action.
    Nowhere,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLink {
    pub text: String,
    pub href: String,
    pub follow: Follow,
    /// Whether this link leaves the site it is written on, which is to say it
    /// names a host other than the page's own.
    pub external: bool,
}

#[derive(Debug, Clone)]
pub struct Page {
    pub url: String,
    pub title: String,
    pub status: Status,
    pub doc: Doc,
    pub links: Vec<ResolvedLink>,
    pub layout: Layout,
    /// Explains why a body is not shown (binary content and the like).
    pub notice: Option<String>,
    /// How long the fetch took, in milliseconds. Only a real fetch has one:
    /// pages w3b wrote itself (errors, the loading placeholder) have `None`.
    pub load_ms: Option<u64>,
    /// The same span taken apart: what the network cost and what w3b cost. Only a
    /// real fetch has one, and `total` is exactly `load_ms`.
    pub timings: Option<Timings>,
    /// The stylesheets the page names, and how many of them were read.
    pub css: CssRead,
    /// How many bytes the body was: the page as it was sent, not as it is drawn.
    pub bytes: usize,
    /// True when the body is the file itself - JSON, plain text, a stylesheet -
    /// rather than a page drawn around it. The monospace slab then covers the whole
    /// text area instead of tracing the glyphs of each line.
    pub file_view: bool,
}

/// Where the time of a page load went, in milliseconds. `total` is the whole span
/// from starting the request to the page being laid out - the number the status
/// line shows - and the parts are what w3b can see inside it, so they can add up to
/// less than the total.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Timings {
    /// DNS, connect, TLS, the redirects and the body, as one span.
    pub fetch: u64,
    /// The stylesheet round-trips, at most `behavior.max_stylesheets` of them.
    pub css: u64,
    /// Decoding the bytes, parsing the document, resolving every link.
    pub parse: u64,
    /// Wrapping the text into lines.
    pub layout: u64,
    /// The whole span, clocked by whoever started the request.
    pub total: u64,
}

/// Stylesheets a page asks for, and how many w3b went and read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CssRead {
    /// `<link rel=stylesheet>` written in the page, including ones behind the cap.
    pub found: usize,
    /// How many were fetched, at most `behavior.max_stylesheets`.
    pub read: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    pub width: usize,
    pub tab_width: usize,
    pub wrap: bool,
}

impl Page {
    pub fn from_fetched(fetched: &Fetched, opts: RenderOptions) -> Page {
        Self::from_fetched_css(fetched, opts, &css::Sheet::default(), 0)
    }

    /// Interpret a response with the CSS it points at applied. Sheets are
    /// fetched through the caller's fetcher, at most `max` of them, and every
    /// failure along the way is simply skipped: CSS never breaks a page.
    pub fn from_fetched_styled(
        fetcher: &dyn Fetcher,
        fetched: &Fetched,
        opts: RenderOptions,
        max: usize,
    ) -> Page {
        let started = Instant::now();
        let html = charset::decode(&fetched.bytes, fetched.content_type.as_deref());
        let (sheet, css) = gather_css(fetcher, &fetched.url, &html, max);
        let css_ms = started.elapsed().as_millis() as u64;
        let mut page = Self::from_fetched_css(fetched, opts, &sheet, css_ms);
        page.css = css;
        page
    }

    fn from_fetched_css(fetched: &Fetched, opts: RenderOptions, styles: &css::Sheet, css_ms: u64) -> Page {
        // Everything up to the layout: decode the bytes, parse the document, work
        // out where every link goes.
        let started = Instant::now();
        let (doc, notice) = match fetched.kind() {
            Kind::Html => (
                crate::dom::parse_with_css(
                    &charset::decode(&fetched.bytes, fetched.content_type.as_deref()),
                    styles,
                ),
                None,
            ),
            Kind::Text | Kind::Css => {
                (plain_text_doc(&charset::decode(&fetched.bytes, fetched.content_type.as_deref())), None)
            }
            // A markdown document becomes HTML and is parsed as HTML, which is
            // the whole trick: headings, links, tables and code slabs then reach
            // the screen by the same road as from a `.html`, so a document that
            // has to look like the page it came from needs no second set of
            // rules for doing it.
            Kind::Markdown => {
                let src = charset::decode(&fetched.bytes, fetched.content_type.as_deref());
                let html = crate::markdown::to_html(&src);
                (crate::dom::parse_with_css(&html, styles), None)
            }
            Kind::Json => {
                let text = charset::decode(&fetched.bytes, fetched.content_type.as_deref());
                match crate::json::doc(&text) {
                    Some(doc) => (doc, None),
                    // A body the server called JSON and the scanner disagrees with is
                    // shown exactly as it arrived, with the reason said out loud.
                    None => (
                        plain_text_doc(&text),
                        Some(format!(
                            "json that does not parse ({} bytes) - shown as it was sent",
                            fetched.bytes.len()
                        )),
                    ),
                }
            }
            Kind::Image => {
                (Doc::default(), Some(format!("image ({} bytes) - not displayed", fetched.bytes.len())))
            }
            Kind::Pdf => {
                (Doc::default(), Some(format!("pdf ({} bytes) - not displayed", fetched.bytes.len())))
            }
            Kind::Binary => (
                Doc::default(),
                Some(format!("{} bytes of binary data - not displayed", fetched.bytes.len())),
            ),
        };

        let links = resolve_links(&fetched.url, &doc.links);
        let title =
            if doc.title.trim().is_empty() { prettify_title(&fetched.url) } else { doc.title.clone() };
        let mut page = Page {
            url: fetched.url.clone(),
            title,
            status: Status::http(fetched.status),
            doc,
            links,
            layout: Layout::default(),
            notice,
            load_ms: None,
            timings: None,
            css: CssRead::default(),
            bytes: fetched.bytes.len(),
            file_view: matches!(fetched.kind(), Kind::Json | Kind::Text | Kind::Css),
        };
        let parse_ms = started.elapsed().as_millis() as u64;
        let started = Instant::now();
        page.relayout(opts);
        let layout_ms = started.elapsed().as_millis() as u64;
        // The fetch span and the total belong to whoever started the request; these
        // three are the part that is w3b's own doing.
        page.timings =
            Some(Timings { css: css_ms, parse: parse_ms, layout: layout_ms, ..Timings::default() });
        page
    }

    /// A page for a failed request: status shows the failure, body explains it.
    pub fn failure(url: &str, kind: NetError, detail: &str, opts: RenderOptions) -> Page {
        let mut doc = Doc::default();
        doc.blocks.push(Block {
            kind: BlockKind::Heading(1),
            indent: 0,
            pieces: vec![Piece { text: kind.label().to_string(), ..piece() }],
            hidden_by: None,
        });
        doc.blocks.push(Block {
            kind: BlockKind::Paragraph,
            indent: 0,
            pieces: vec![Piece { text: url.to_string(), ..piece() }],
            hidden_by: None,
        });
        doc.blocks.push(Block {
            kind: BlockKind::Paragraph,
            indent: 0,
            pieces: vec![Piece { text: detail.to_string(), muted: true, ..piece() }],
            hidden_by: None,
        });
        let layout = layout::layout(&doc, opts.width.max(1), opts.tab_width, opts.wrap);
        Page {
            url: url.to_string(),
            title: String::new(),
            status: Status::net(kind),
            doc,
            links: Vec::new(),
            layout,
            notice: None,
            load_ms: None,
            timings: None,
            css: CssRead::default(),
            bytes: 0,
            file_view: false,
        }
    }

    /// The page shown while the first fetch of a session is in flight. It must
    /// not read as a failure: no error heading, and a loading status.
    pub fn loading(url: &str, opts: RenderOptions) -> Page {
        let mut doc = Doc::default();
        doc.blocks.push(Block {
            kind: BlockKind::Heading(1),
            indent: 0,
            pieces: vec![Piece { text: "Loading".into(), ..piece() }],
            hidden_by: None,
        });
        doc.blocks.push(Block {
            kind: BlockKind::Paragraph,
            indent: 0,
            pieces: vec![Piece { text: url.to_string(), ..piece() }],
            hidden_by: None,
        });
        let layout = layout::layout(&doc, opts.width.max(1), opts.tab_width, opts.wrap);
        Page {
            url: url.to_string(),
            title: String::new(),
            status: Status::loading(),
            doc,
            links: Vec::new(),
            layout,
            notice: None,
            load_ms: None,
            timings: None,
            css: CssRead::default(),
            bytes: 0,
            file_view: false,
        }
    }

    /// Re-flow the text for a different terminal width. Never re-fetches.
    pub fn relayout(&mut self, opts: RenderOptions) {
        let mut doc = self.doc.clone();
        if let Some(notice) = &self.notice {
            doc.blocks.push(Block {
                kind: BlockKind::Paragraph,
                indent: 0,
                pieces: vec![Piece { text: notice.clone(), muted: true, ..piece() }],
                hidden_by: None,
            });
        }
        self.layout = layout::layout(&doc, opts.width.max(1), opts.tab_width, opts.wrap);
    }

    /// What a form field holds, or `None` when there is no such field.
    pub fn field_value(&self, index: usize) -> Option<&str> {
        self.doc.fields.get(index).map(|f| f.value.as_str())
    }

    /// Type into a form field. The text of a field belongs to the document, so
    /// the piece is rewritten and the page re-flowed: typed text wraps, scrolls
    /// and is laid out like any other text.
    pub fn set_field(&mut self, index: usize, value: &str, opts: RenderOptions) {
        let Some(field) = self.doc.fields.get_mut(index) else { return };
        field.value = value.to_string();
        let text = field.display();
        for block in &mut self.doc.blocks {
            for piece in &mut block.pieces {
                if piece.field == Some(index) {
                    piece.text = text.clone();
                }
            }
        }
        for table in &mut self.doc.tables {
            for row in &mut table.rows {
                for cell in row {
                    for piece in &mut cell.pieces {
                        if piece.field == Some(index) {
                            piece.text = text.clone();
                        }
                    }
                }
            }
        }
        self.relayout(opts);
    }

    /// Where the form a field belongs to goes, and what pressing Enter there
    /// means. Only a GET with an action is sent, exactly as a browser submits one:
    /// every named field of that form joins the action as a query string.
    pub fn submit_target(&self, field: usize) -> Submit {
        let Some(field) = self.doc.fields.get(field) else { return Submit::Nowhere };
        let Some(index) = field.form else { return Submit::Nowhere };
        let Some(form) = self.doc.forms.get(index) else { return Submit::Nowhere };
        let action = form.action.trim();
        if action.is_empty() {
            return Submit::Nowhere;
        }
        if form.method != "get" {
            return Submit::Method(form.method.clone());
        }
        let base = match Url::parse(&self.url) {
            Ok(url) => url,
            Err(_) => return Submit::Nowhere,
        };
        let Ok(mut url) = base.join(action) else { return Submit::Nowhere };
        // A form that aims at a piece of the page is not a new page to fetch.
        url.set_fragment(None);
        {
            let mut query = url.query_pairs_mut();
            for f in self.doc.fields.iter().filter(|f| f.form == Some(index)) {
                if !f.name.is_empty() {
                    query.append_pair(&f.name, &f.value);
                }
            }
        }
        match follow_url(url) {
            Follow::Url(target) => Submit::Go(target),
            Follow::Unsupported(why) => Submit::Unsupported(why),
            Follow::Anchor => Submit::Nowhere,
        }
    }

    /// What a `<details>` summary says, for the message a toggle reports.
    pub fn disclosure_label(&self, index: usize) -> Option<&str> {
        self.doc.disclosures.get(index).map(|d| d.label.as_str())
    }

    /// Open or close a `<details>` disclosure and re-flow the page. The content of
    /// a closed one is part of the document all along - it is only kept off the
    /// rows - so toggling re-lays out and never re-fetches.
    pub fn toggle_disclosure(&mut self, index: usize, opts: RenderOptions) -> Option<bool> {
        let d = self.doc.disclosures.get_mut(index)?;
        d.open = !d.open;
        let open = d.open;
        self.relayout(opts);
        Some(open)
    }

    pub fn link_at(&self, row: usize, col: usize) -> Option<usize> {
        self.layout.link_at(row, col)
    }

    /// Where an image or media label came from, resolved against this page's URL
    /// so the address stands on its own outside the page. `None` when the tag
    /// does not name a source; a `data:` address comes back as written, however
    /// long it is.
    pub fn media_target(&self, index: usize) -> Option<String> {
        let src = self.doc.media.get(index)?.src.as_str();
        if src.is_empty() {
            return None;
        }
        match Url::parse(&self.url).ok().and_then(|base| base.join(src).ok()) {
            Some(resolved) => Some(resolved.as_str().to_string()),
            None => Some(src.to_string()),
        }
    }

    pub fn row_count(&self) -> usize {
        self.layout.len()
    }
}

/// The document's own `<style>` text plus the sheets it links to, in document
/// order, with `@import` targets resolved against the sheet that named them.
fn gather_css(fetcher: &dyn Fetcher, page_url: &str, html: &str, max: usize) -> (css::Sheet, CssRead) {
    let (embedded, links) = css::extract(html);
    let found = links.len();
    let mut sheet = css::parse(&embedded);
    if max == 0 || links.is_empty() {
        return (sheet, CssRead { found, read: 0 });
    }
    let base = match Url::parse(page_url) {
        Ok(base) => base,
        Err(_) => return (sheet, CssRead { found, read: 0 }),
    };
    let mut queue: VecDeque<(Url, String)> = links.iter().map(|href| (base.clone(), href.clone())).collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut taken = 0usize;
    while let Some((at, href)) = queue.pop_front() {
        let Ok(url) = at.join(&href) else { continue };
        if !matches!(url.scheme(), "http" | "https") {
            continue;
        }
        if !seen.insert(url.as_str().to_string()) {
            continue;
        }
        if taken >= max {
            break;
        }
        taken += 1;
        let Ok(got) = fetcher.fetch(url.as_str()) else { continue };
        if !looks_like_css(&got) {
            continue;
        }
        let text = charset::decode(&got.bytes, got.content_type.as_deref());
        let parsed = css::parse(&text);
        for import in &parsed.imports {
            queue.push_back((url.clone(), import.clone()));
        }
        sheet.rules.extend(parsed.rules);
    }
    (sheet, CssRead { found, read: taken })
}

/// Servers often answer a missing stylesheet with an HTML error page; that must
/// not be parsed as CSS.
fn looks_like_css(fetched: &Fetched) -> bool {
    let ctype = fetched.content_type.as_deref().unwrap_or("").to_ascii_lowercase();
    if ctype.contains("css") {
        return true;
    }
    if ctype.contains("html") {
        return false;
    }
    let head: String = fetched.bytes.iter().take(512).map(|b| *b as char).collect();
    head.contains('{') && !head.contains("<!doctype") && !head.contains("<html")
}

fn piece() -> Piece {
    Piece {
        text: String::new(),
        link: None,
        mono: false,
        bold: false,
        italic: false,
        code: false,
        muted: false,
        field: None,
        disclosure: None,
        media: None,
        json: Role::Plain,
        hard_break: false,
    }
}

fn plain_text_doc(text: &str) -> Doc {
    let mut doc = Doc::default();
    for line in text.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        let mut block = Block { kind: BlockKind::Pre, indent: 0, pieces: Vec::new(), hidden_by: None };
        if !line.is_empty() {
            block.pieces.push(Piece { text: line.to_string(), ..piece() });
        }
        doc.blocks.push(block);
    }
    doc
}

/// Resolve `href` attributes against the final URL of the response.
pub fn resolve_links(base_url: &str, links: &[Link]) -> Vec<ResolvedLink> {
    let base = Url::parse(base_url).ok();
    let here = base.as_ref().and_then(|b| b.host_str()).map(str::to_string);
    links
        .iter()
        .map(|link| {
            let href = link.href.trim();
            let mut external = false;
            let follow = match base.as_ref().and_then(|b| b.join(href).ok()) {
                None => Follow::Unsupported("url"),
                Some(resolved) => {
                    if href.starts_with('#') {
                        Follow::Anchor
                    } else {
                        // A link that names some other machine leaves the site,
                        // and that is worth saying out loud.
                        external = resolved.host_str().is_some_and(|host| Some(host) != here.as_deref());
                        follow_url(resolved)
                    }
                }
            };
            ResolvedLink { text: link.text.trim().to_string(), href: href.to_string(), follow, external }
        })
        .collect()
}

/// What w3b can do with a URL of its own: fetch `http`/`https`, report the rest.
fn follow_url(resolved: Url) -> Follow {
    match resolved.scheme() {
        "http" | "https" => Follow::Url(resolved.into()),
        "mailto" => Follow::Unsupported("mailto"),
        "tel" => Follow::Unsupported("tel"),
        "javascript" => Follow::Unsupported("javascript"),
        "data" => Follow::Unsupported("data"),
        "ftp" => Follow::Unsupported("ftp"),
        "file" => Follow::Unsupported("file"),
        _ => Follow::Unsupported("unknown scheme"),
    }
}

/// Fallback title: last path segment of the URL, or its host.
fn prettify_title(url: &str) -> String {
    match Url::parse(url) {
        Ok(parsed) => {
            let last = parsed.path_segments().and_then(|mut s| s.rfind(|p| !p.is_empty()));
            match last {
                Some(seg) if !seg.is_empty() => seg.to_string(),
                _ => parsed.host_str().unwrap_or(url).to_string(),
            }
        }
        Err(_) => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(width: usize) -> RenderOptions {
        RenderOptions { width, tab_width: 4, wrap: true }
    }

    fn page(html: &str) -> Page {
        let fetched = Fetched {
            status: 200,
            url: "https://test.local/form".to_string(),
            content_type: Some("text/html".to_string()),
            bytes: html.as_bytes().to_vec(),
        };
        Page::from_fetched(&fetched, opts(40))
    }

    fn row(page: &Page, row: usize) -> String {
        page.layout.rows[row].runs.iter().map(|r| r.text.as_str()).collect::<String>().trim_end().to_string()
    }

    fn rows(page: &Page) -> Vec<String> {
        (0..page.row_count()).map(|i| row(page, i)).collect()
    }

    #[test]
    fn typing_rewrites_the_field_text_and_the_layout() {
        let mut page = page("<p>query <input name=q></p>");
        assert_eq!(page.doc.fields.len(), 1);
        assert_eq!(row(&page, 0), "query [ input ]");

        page.set_field(0, "ls -l", opts(40));
        assert_eq!(page.doc.fields[0].value, "ls -l");
        assert_eq!(row(&page, 0), "query [ ls -l ]");
        assert_eq!(page.layout.field_at(0, 8), Some(0), "the field still covers its text");
    }

    #[test]
    fn re_typing_a_field_never_stacks_the_notice_block() {
        let mut page = page("<p>a <input></p>");
        page.notice = Some("hello".to_string());
        page.relayout(opts(10));
        let before = rows(&page).len();
        page.set_field(0, "sit", opts(10));
        let with_text = rows(&page).len();
        for _ in 0..5 {
            page.set_field(0, "sit", opts(10));
        }
        assert_eq!(rows(&page).len(), with_text, "re-typing the same text must not grow the page");
        assert_eq!(rows(&page).iter().filter(|r| r.contains("hello")).count(), 1);
        assert!(rows(&page).iter().any(|r| r.contains("[ sit ]")), "{:?}", rows(&page));
        assert_ne!(before, 0);
    }

    #[test]
    fn an_unknown_field_index_is_ignored() {
        let mut page = page("<p>hello</p>");
        page.set_field(7, "x", opts(40));
        assert_eq!(row(&page, 0), "hello");
    }

    #[test]
    fn enter_in_a_field_sends_its_form_as_a_query() {
        let mut page = page("<form action=/search><input name=q value=ann><input name=sort></form>");
        page.set_field(0, "one two&three", opts(40));
        assert_eq!(
            page.submit_target(0),
            Submit::Go("https://test.local/search?q=one+two%26three&sort=".to_string()),
            "every named field of the form goes into the query"
        );
    }

    #[test]
    fn a_field_that_was_never_named_is_not_sent() {
        let named = page("<form action=/s><input value=hidden><input name=q value=ann></form>");
        assert_eq!(named.submit_target(1), Submit::Go("https://test.local/s?q=ann".to_string()));
        // What a browser does with a GET form that carries nothing: the action, and
        // a query that is empty.
        let empty = page("<form action=/s><input value=hidden></form>");
        assert_eq!(empty.submit_target(0), Submit::Go("https://test.local/s?".to_string()));
    }

    #[test]
    fn an_action_that_already_asks_for_something_keeps_it() {
        let part = page("<form action=\"/search?fixed=1#top\"><input name=q value=ann></form>");
        assert_eq!(part.submit_target(0), Submit::Go("https://test.local/search?fixed=1&q=ann".to_string()));
        let far = page("<form action=\"https://other.example/x\"><input name=q value=ann></form>");
        assert_eq!(far.submit_target(0), Submit::Go("https://other.example/x?q=ann".to_string()));
    }

    #[test]
    fn only_a_get_form_with_an_action_is_sent() {
        let page = page(
            "<form method=POST action=/send><input name=p></form>\
                         <form><input name=n></form>\
                         <form action=\"mailto:someone@example.com\"><input name=m></form>\
                         <p><input name=s></p>",
        );
        assert_eq!(page.submit_target(0), Submit::Method("post".to_string()));
        assert_eq!(page.submit_target(1), Submit::Nowhere, "no action to send it to");
        assert_eq!(page.submit_target(2), Submit::Unsupported("mailto"));
        assert_eq!(page.submit_target(3), Submit::Nowhere, "a field outside any form");
    }

    #[test]
    fn a_link_that_names_another_host_leaves_the_site() {
        let doc = page(
            "<a href=\"/same\">same</a> <a href=next.html>next</a> \
             <a href=\"https://elsewhere.example/x\">elsewhere</a> \
             <a href=\"HTTP://ELSEWHERE.example/y\">upper</a> \
             <a href=\"http://test.local:8080/port\">port</a> \
             <a href=\"#part\">part</a> <a href=\"mailto:a@b.c\">mail</a> \
             <a href=\"http://\">broken</a>",
        );
        let flags: Vec<bool> = doc.links.iter().map(|link| link.external).collect();
        assert_eq!(
            flags,
            vec![false, false, true, true, false, false, false, false],
            "the host decides, however it is spelled; a port, an anchor, a mail address \
             and a broken url all stay on this page's name"
        );
    }

    #[test]
    fn an_image_label_says_where_the_image_is() {
        let here = page(
            "<p><img src=\"logo.png\" alt=\"logo\"> <img src=\"../up.png\" alt=\"up\"> \
             <img src=\"https://cdn.example/x/big.png\" alt=\"cdn\"> \
             <img alt=\"no address\"> <img src=\"data:image/png;base64,AAAA\" alt=\"inline\"></p>",
        );
        let got: Vec<Option<String>> = (0..here.doc.media.len()).map(|i| here.media_target(i)).collect();
        assert_eq!(
            got,
            vec![
                Some("https://test.local/logo.png".to_string()),
                Some("https://test.local/up.png".to_string()),
                Some("https://cdn.example/x/big.png".to_string()),
                None,
                Some("data:image/png;base64,AAAA".to_string()),
            ],
            "a relative address is finished off with the page's own; an image that names \
             no address has nothing to hand over"
        );
        assert_eq!(here.media_target(99), None, "an index off the page is ignored");
        assert_eq!(here.doc.media[0].label, "logo", "the words on the page are kept alongside");
    }
}

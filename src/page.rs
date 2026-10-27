//! A rendered resource: parsed document + link targets + laid out rows.
use std::collections::{BTreeSet, VecDeque};
use url::Url;

use crate::charset;
use crate::css;
use crate::dom::{Block, BlockKind, Doc, Link, Piece};
use crate::error::NetError;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLink {
    pub text: String,
    pub href: String,
    pub follow: Follow,
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
}

#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    pub width: usize,
    pub tab_width: usize,
    pub wrap: bool,
}

impl Page {
    pub fn from_fetched(fetched: &Fetched, opts: RenderOptions) -> Page {
        Self::from_fetched_css(fetched, opts, &css::Sheet::default())
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
        let html = charset::decode(&fetched.bytes, fetched.content_type.as_deref());
        let sheet = gather_css(fetcher, &fetched.url, &html, max);
        Self::from_fetched_css(fetched, opts, &sheet)
    }

    fn from_fetched_css(fetched: &Fetched, opts: RenderOptions, styles: &css::Sheet) -> Page {
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
        };
        page.relayout(opts);
        page
    }

    /// A page for a failed request: status shows the failure, body explains it.
    pub fn failure(url: &str, kind: NetError, detail: &str, opts: RenderOptions) -> Page {
        let mut doc = Doc::default();
        doc.blocks.push(Block {
            kind: BlockKind::Heading(1),
            indent: 0,
            pieces: vec![Piece { text: kind.label().to_string(), ..piece() }],
        });
        doc.blocks.push(Block {
            kind: BlockKind::Paragraph,
            indent: 0,
            pieces: vec![Piece { text: url.to_string(), ..piece() }],
        });
        doc.blocks.push(Block {
            kind: BlockKind::Paragraph,
            indent: 0,
            pieces: vec![Piece { text: detail.to_string(), muted: true, ..piece() }],
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
        }
    }

    pub fn placeholder(url: &str, message: &str, opts: RenderOptions) -> Page {
        let mut page = Page::failure(url, NetError::Other, message, opts);
        page.status = Status::http(200);
        page
    }

    /// Re-flow the text for a different terminal width. Never re-fetches.
    pub fn relayout(&mut self, opts: RenderOptions) {
        let mut doc = self.doc.clone();
        if let Some(notice) = &self.notice {
            doc.blocks.push(Block {
                kind: BlockKind::Paragraph,
                indent: 0,
                pieces: vec![Piece { text: notice.clone(), muted: true, ..piece() }],
            });
        }
        self.layout = layout::layout(&doc, opts.width.max(1), opts.tab_width, opts.wrap);
    }

    pub fn link_at(&self, row: usize, col: usize) -> Option<usize> {
        self.layout.link_at(row, col)
    }

    pub fn row_count(&self) -> usize {
        self.layout.len()
    }
}

/// The document's own `<style>` text plus the sheets it links to, in document
/// order, with `@import` targets resolved against the sheet that named them.
fn gather_css(fetcher: &dyn Fetcher, page_url: &str, html: &str, max: usize) -> css::Sheet {
    let (embedded, links) = css::extract(html);
    let mut sheet = css::parse(&embedded);
    if max == 0 || links.is_empty() {
        return sheet;
    }
    let base = match Url::parse(page_url) {
        Ok(base) => base,
        Err(_) => return sheet,
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
    sheet
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
        image: false,
    }
}

fn plain_text_doc(text: &str) -> Doc {
    let mut doc = Doc::default();
    for line in text.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        let mut block = Block { kind: BlockKind::Pre, indent: 0, pieces: Vec::new() };
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
    links
        .iter()
        .map(|link| {
            let href = link.href.trim();
            let follow = match base.as_ref().and_then(|b| b.join(href).ok()) {
                None => Follow::Unsupported("url"),
                Some(resolved) => {
                    if href.starts_with('#') {
                        Follow::Anchor
                    } else {
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
                }
            };
            ResolvedLink { text: link.text.trim().to_string(), href: href.to_string(), follow }
        })
        .collect()
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

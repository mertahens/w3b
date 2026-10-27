//! Browser state machine: page, cursor, viewport, history, input, fetches.
use std::collections::{BTreeMap, HashSet};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use unicode_width::UnicodeWidthStr;

use crate::config::{Behavior, Config};
use crate::error::{Error, NetError};
use crate::layout::{Layout, LinkPlacement};
use crate::net::{normalize_target, Fetched, Fetcher};
use crate::page::{Follow, Page, RenderOptions};
use crate::status::{self, Status};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    /// Column the cursor tries to return to after vertical movement (vim).
    pub desired: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    /// A link target was yanked; the TUI copies it to the clipboard.
    Yank(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Open,
    Search,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    pub error: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    pub width: usize,
    pub height: usize,
}

/// Cursor + viewport state kept in sync by `clamp`.
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub cursor: Cursor,
    pub scroll: usize,
    pub hscroll: usize,
}

impl View {
    pub fn new() -> View {
        View { cursor: Cursor::default(), scroll: 0, hscroll: 0 }
    }
}

impl Default for View {
    fn default() -> View {
        View::new()
    }
}

type Reply = (u64, Result<Fetched, Error>);

pub struct App {
    pub config: Config,
    pub theme: Theme,
    pub page: Page,
    pub view: View,
    pub mode: Mode,
    pub viewport: Viewport,
    pub input: String,
    pub input_pos: usize,
    pub pending_g: bool,
    pub history: Vec<String>,
    pub history_index: usize,
    pub visited: HashSet<String>,
    pub message: Option<Message>,
    pub query: String,
    pub matches: BTreeMap<usize, Vec<(usize, usize)>>,
    pub match_index: Option<usize>,
    pub quit: bool,
    fetcher: Arc<dyn Fetcher>,
    seq: u64,
    rx: Option<Receiver<Reply>>,
    loading_url: Option<String>,
}

impl App {
    pub fn new(
        config: Config,
        theme: Theme,
        fetcher: Arc<dyn Fetcher>,
        start_url: &str,
        viewport: Viewport,
    ) -> App {
        let mut app = App::empty(config, theme, fetcher, start_url, viewport);
        app.start_fetch(start_url);
        app
    }

    /// Same state, without starting a fetch (used by `--dump` and tests).
    pub fn empty(
        config: Config,
        theme: Theme,
        fetcher: Arc<dyn Fetcher>,
        start_url: &str,
        viewport: Viewport,
    ) -> App {
        let opts = render_options(&config, viewport);
        let page = Page::placeholder(start_url, "loading", opts);
        App {
            config,
            theme,
            page,
            view: View::new(),
            mode: Mode::Normal,
            viewport,
            input: String::new(),
            input_pos: 0,
            pending_g: false,
            history: vec![start_url.to_string()],
            history_index: 0,
            visited: HashSet::new(),
            message: None,
            query: String::new(),
            matches: BTreeMap::new(),
            match_index: None,
            quit: false,
            fetcher,
            seq: 0,
            rx: None,
            loading_url: None,
        }
    }

    fn render_options(&self) -> RenderOptions {
        render_options(&self.config, self.viewport)
    }

    pub fn status(&self) -> Status {
        if self.loading_url.is_some() {
            Status::loading()
        } else {
            self.page.status.clone()
        }
    }

    pub fn is_loading(&self) -> bool {
        self.loading_url.is_some()
    }

    pub fn url(&self) -> &str {
        &self.page.url
    }

    pub fn title(&self) -> &str {
        &self.page.title
    }

    /// Percentage of the document scrolled, from the cursor position.
    pub fn percent(&self) -> u32 {
        status::percent(self.view.cursor.row, self.page.row_count())
    }

    pub fn show_message(&mut self, text: impl Into<String>, error: bool) {
        self.message = Some(Message { text: text.into(), error });
    }

    pub fn enter_input_mode(&mut self, mode: Mode, initial: String) {
        self.mode = mode;
        self.input = initial;
        self.input_pos = self.input.chars().count();
    }

    pub fn clear_message(&mut self) {
        self.message = None;
    }

    /// Fetch without waiting. Used by navigation keys.
    fn start_fetch(&mut self, url: &str) {
        let seq = self.seq + 1;
        self.seq = seq;
        let (tx, rx) = mpsc::channel();
        let fetcher = Arc::clone(&self.fetcher);
        let request_url = url.to_string();
        std::thread::spawn(move || {
            let reply = fetcher.fetch(&request_url);
            let _ = tx.send((seq, reply));
        });
        self.rx = Some(rx);
        self.loading_url = Some(url.to_string());
        self.message = None;
    }

    /// Apply a finished fetch, if one is waiting. Returns true when the page changed.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = self.rx.take() else { return false };
        match rx.try_recv() {
            Ok((_seq, result)) => self.apply(result),
            Err(mpsc::TryRecvError::Empty) => {
                self.rx = Some(rx);
                false
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.fail(NetError::Other, "fetch thread ended");
                true
            }
        }
    }

    /// Wait for a running fetch to finish and apply it. The TUI never needs
    /// this (it polls), but tests and `--dump` do.
    pub fn settle(&mut self) {
        while self.is_loading() {
            std::thread::sleep(std::time::Duration::from_millis(2));
            self.poll();
        }
    }

    /// Fetch and apply synchronously (used by tests and `--dump`).
    pub fn open(&mut self, url: &str) {
        self.loading_url = Some(url.to_string());
        let target = url.to_string();
        let result = self.fetcher.fetch(&target);
        self.apply(result);
    }

    fn apply(&mut self, result: Result<Fetched, Error>) -> bool {
        let requested = self.loading_url.take().unwrap_or_default();
        match result {
            Ok(fetched) => {
                self.page = Page::from_fetched_styled(
                    &*self.fetcher,
                    &fetched,
                    self.render_options(),
                    self.config.behavior.max_stylesheets,
                );
                self.visited.insert(fetched.url.clone());
                self.record_history();
                self.view = View::new();
                self.rebuild_matches();
                true
            }
            Err(err) => {
                let kind = match &err {
                    Error::Http(inner) => NetError::classify(inner),
                    Error::Net(kind, _) => *kind,
                    Error::BadUrl(_) => NetError::BadUrl,
                    Error::TooLarge(_) => NetError::TooLarge,
                    Error::Io(_) => NetError::Io,
                    Error::Config { .. } => NetError::Other,
                    Error::Unrenderable(_) => NetError::Other,
                };
                self.fail_page(&requested, kind, &err.to_string());
                true
            }
        }
    }

    fn fail(&mut self, kind: NetError, detail: &str) {
        let url = self.loading_url.take().unwrap_or_else(|| self.page.url.clone());
        self.fail_page(&url, kind, detail);
    }

    fn fail_page(&mut self, url: &str, kind: NetError, detail: &str) {
        self.page = Page::failure(url, kind, detail, self.render_options());
        self.view = View::new();
        self.rebuild_matches();
    }

    pub fn current_target(&self) -> Option<&str> {
        self.loading_url.as_deref()
    }

    /// Navigate: push history and fetch.
    pub fn navigate(&mut self, url: &str) {
        self.start_fetch(url);
    }

    pub fn reload(&mut self) {
        let url = self.page.url.clone();
        self.start_fetch(&url);
    }

    pub fn open_typed(&mut self, input: &str) {
        match normalize_target(input) {
            Ok(url) => self.navigate(&url),
            Err(err) => {
                let text = err.to_string();
                self.show_message(text, true);
            }
        }
    }

    pub fn go_back(&mut self) {
        if self.history_index == 0 {
            self.show_message("no previous page", false);
            return;
        }
        self.history_index -= 1;
        let url = self.history[self.history_index].clone();
        self.start_fetch(&url);
    }

    pub fn go_forward(&mut self) {
        if self.history_index + 1 >= self.history.len() {
            self.show_message("no next page", false);
            return;
        }
        self.history_index += 1;
        let url = self.history[self.history_index].clone();
        self.start_fetch(&url);
    }

    /// Re-flow for a new terminal size.
    pub fn resize(&mut self, width: usize, height: usize) {
        if width == self.viewport.width && height == self.viewport.height {
            return;
        }
        self.viewport = Viewport { width: width.max(1), height: height.max(1) };
        self.page.relayout(self.render_options());
        self.rebuild_matches();
        self.clamp();
    }

    pub fn toggle_wrap(&mut self) {
        self.config.behavior.wrap = !self.config.behavior.wrap;
        self.page.relayout(self.render_options());
        self.rebuild_matches();
        self.clamp();
        let state = if self.config.behavior.wrap { "on" } else { "off" };
        self.show_message(format!("wrap {state}"), false);
    }

    pub fn rows(&self) -> &[crate::layout::Row] {
        &self.page.layout.rows
    }

    /// The laid-out page as plain text, exactly what `--dump` prints.
    pub fn plain_text(&self) -> String {
        crate::dump::plain_text(self.layout())
    }

    pub fn layout(&self) -> &Layout {
        &self.page.layout
    }

    pub fn clamp(&mut self) {
        let rows = self.page.row_count();
        let width = self.page.layout.rows.get(self.view.cursor.row).map(|r| r.width).unwrap_or(0);
        self.view.cursor.row = self.view.cursor.row.min(rows.saturating_sub(1));
        self.view.cursor.col = self.view.cursor.col.min(width);
        if self.view.cursor.desired == 0 {
            self.view.cursor.desired = self.view.cursor.col;
        }
        self.ensure_visible();
    }
}

/// Blank columns on each side of the page text, clamped for narrow terminals.
pub fn padding_x(behavior: &Behavior, viewport_width: usize) -> usize {
    behavior.page_padding_x.min(viewport_width / 4)
}

fn render_options(config: &Config, viewport: Viewport) -> RenderOptions {
    let pad = padding_x(&config.behavior, viewport.width);
    RenderOptions {
        width: viewport.width.saturating_sub(2 * pad).max(1),
        tab_width: config.behavior.tab_width,
        wrap: config.behavior.wrap,
    }
}

impl App {
    fn record_history(&mut self) {
        if self.history.get(self.history_index).map(String::as_str) == Some(self.page.url.as_str()) {
            return;
        }
        self.history.truncate(self.history_index + 1);
        self.history.push(self.page.url.clone());
        self.history_index = self.history.len() - 1;
        let limit = self.config.behavior.history_size.max(10);
        if self.history.len() > limit {
            let excess = self.history.len() - limit;
            self.history.drain(0..excess);
            self.history_index -= excess;
        }
    }
}

// --- movement -------------------------------------------------------------
impl App {
    /// Rows available for text (the last terminal row is the status line).
    pub fn page_height(&self) -> usize {
        self.viewport.height.saturating_sub(1).max(1)
    }

    pub fn down(&mut self, n: usize) {
        let row = (self.view.cursor.row + n).min(self.page.row_count().saturating_sub(1));
        self.set_row(row);
    }

    pub fn up(&mut self, n: usize) {
        self.set_row(self.view.cursor.row.saturating_sub(n));
    }

    pub fn to_top(&mut self) {
        self.view.scroll = 0;
        self.set_row(0);
    }

    pub fn to_bottom(&mut self) {
        let last = self.page.row_count().saturating_sub(1);
        self.set_row(last);
    }

    pub fn page_down(&mut self, half: bool) {
        let step = if half { self.page_height() / 2 } else { self.page_height().saturating_sub(1) };
        self.down(step.max(1));
    }

    pub fn page_up(&mut self, half: bool) {
        let step = if half { self.page_height() / 2 } else { self.page_height().saturating_sub(1) };
        self.up(step.max(1));
    }

    pub fn left(&mut self) {
        if self.view.cursor.col > 0 {
            self.view.cursor.col -= 1;
            self.view.cursor.desired = self.view.cursor.col;
        } else if !self.config.behavior.wrap && self.view.hscroll > 0 {
            self.view.hscroll -= 1;
        }
        self.ensure_visible();
    }

    pub fn right(&mut self) {
        let width = self.row_width(self.view.cursor.row);
        if self.view.cursor.col < width {
            self.view.cursor.col += 1;
            self.view.cursor.desired = self.view.cursor.col;
        }
        if self.config.behavior.wrap {
            self.view.cursor.col = self.view.cursor.col.min(width);
        }
        self.ensure_visible();
    }

    pub fn line_start(&mut self) {
        self.view.cursor.col = 0;
        self.view.cursor.desired = 0;
        self.ensure_visible();
    }

    /// Vim `^`: first non-blank column.
    pub fn first_non_blank(&mut self) {
        let col = self
            .page
            .layout
            .rows
            .get(self.view.cursor.row)
            .and_then(|row| row.runs.iter().find(|run| !run.text.trim().is_empty()))
            .map(|run| {
                let lead = run.text.chars().take_while(|c| c.is_whitespace()).count();
                run.start + lead
            })
            .unwrap_or(0);
        self.view.cursor.col = col;
        self.view.cursor.desired = col;
    }

    pub fn line_end(&mut self) {
        let width = self.row_width(self.view.cursor.row);
        self.view.cursor.col = width;
        self.view.cursor.desired = width;
        self.ensure_visible();
    }

    fn set_row(&mut self, row: usize) {
        self.view.cursor.row = row;
        let width = self.row_width(row);
        self.view.cursor.col = self.view.cursor.desired.min(width);
        self.ensure_visible();
    }

    /// Row of a same-document anchor such as `#install`, if the page declares it.
    pub fn anchor_row(&self, name: &str) -> Option<usize> {
        let block = self.page.doc.anchors.iter().find(|(anchor, _)| anchor == name).map(|(_, b)| *b)?;
        self.page.layout.block_rows.get(block).copied()
    }

    pub fn row_width(&self, row: usize) -> usize {
        self.page.layout.rows.get(row).map(|r| r.width).unwrap_or(0)
    }

    /// Keep the cursor inside the window, respecting `scroll_margin`.
    pub fn ensure_visible(&mut self) {
        let height = self.page_height();
        let rows = self.page.row_count();
        let margin = self.config.behavior.scroll_margin.min(height / 2);
        let mut top = self.view.scroll;

        if self.view.cursor.row < top + margin {
            top = self.view.cursor.row.saturating_sub(margin);
        } else if self.view.cursor.row + margin >= top + height {
            top = self.view.cursor.row + margin + 1 - height;
        }
        self.view.scroll = top.min(rows.saturating_sub(1));

        if self.config.behavior.wrap {
            self.view.hscroll = 0;
        } else {
            let width = self.row_width(self.view.cursor.row);
            let visible = self.content_width();
            if self.view.cursor.col < self.view.hscroll {
                self.view.hscroll = self.view.cursor.col;
            } else if self.view.cursor.col >= self.view.hscroll + visible {
                self.view.hscroll = self.view.cursor.col + 1 - visible;
            }
            self.view.hscroll = self.view.hscroll.min(width);
        }
    }
}

// --- links ----------------------------------------------------------------
impl App {
    pub fn link_under_cursor(&self) -> Option<usize> {
        self.page.link_at(self.view.cursor.row, self.view.cursor.col)
    }

    /// Blank columns the page is indented by, left and right.
    pub fn padding_x(&self) -> usize {
        padding_x(&self.config.behavior, self.viewport.width)
    }

    /// Columns available for page text once the padding is taken off.
    pub fn content_width(&self) -> usize {
        self.viewport.width.saturating_sub(2 * self.padding_x()).max(1)
    }

    /// The link placement under the cursor, used to highlight a hovered link.
    pub fn placement_at(&self, row: usize, col: usize) -> Option<&LinkPlacement> {
        self.page.layout.links.iter().find(|p| p.row == row && p.start <= col && col < p.end)
    }

    /// Where a link would take you, for the status line and for yanking.
    pub fn link_target(&self, index: usize) -> Option<String> {
        let link = self.page.links.get(index)?;
        Some(match &link.follow {
            Follow::Url(url) => url.clone(),
            _ => link.href.clone(),
        })
    }

    /// Target of the link under the cursor, shown as `-> <target>`.
    pub fn hover_target(&self) -> Option<String> {
        self.link_under_cursor().and_then(|index| self.link_target(index))
    }

    /// Follow the link under the cursor, or report why it cannot be followed.
    pub fn follow_cursor(&mut self) {
        match self.link_under_cursor() {
            None => self.show_message("no link under cursor", false),
            Some(index) => match &self.page.links[index].follow {
                Follow::Url(url) => {
                    let url = url.clone();
                    self.navigate(&url);
                }
                Follow::Anchor => {
                    let href = self.page.links[index].href.clone();
                    let name = href.trim_start_matches('#');
                    match self.anchor_row(name) {
                        Some(row) => {
                            self.set_row(row);
                            self.show_message(format!("anchor #{name}"), false);
                        }
                        None => self.show_message(format!("no #{name} on this page"), true),
                    }
                }
                Follow::Unsupported(scheme) => {
                    let scheme = (*scheme).to_string();
                    self.show_message(format!("unsupported link scheme: {scheme}"), true);
                }
            },
        }
    }

    /// Show the target of the link under the cursor without following it.
    /// Report the link target and hand it to the caller for the clipboard.
    pub fn yank_cursor(&mut self) -> Option<String> {
        let target = self.link_under_cursor().and_then(|index| self.link_target(index));
        match target {
            Some(target) => {
                self.show_message(format!("yanked: {target}"), false);
                Some(target)
            }
            None => {
                self.show_message("nothing to yank", false);
                None
            }
        }
    }

    /// Move the cursor to the next link placement at or after the cursor.
    pub fn next_link(&mut self) {
        let here = (self.view.cursor.row, self.view.cursor.col);
        let next =
            self.page.layout.links.iter().filter(|p| (p.row, p.end) > here).min_by_key(|p| (p.row, p.start));
        self.jump_to_placement(next.map(|p| (p.row, p.start)));
    }

    pub fn prev_link(&mut self) {
        let here = (self.view.cursor.row, self.view.cursor.col);
        let prev = self
            .page
            .layout
            .links
            .iter()
            .filter(|p| (p.row, p.start) < here)
            .max_by_key(|p| (p.row, p.start));
        self.jump_to_placement(prev.map(|p| (p.row, p.start)));
    }

    fn jump_to_placement(&mut self, placement: Option<(usize, usize)>) {
        match placement {
            Some((row, start)) => {
                self.view.cursor.row = row;
                self.view.cursor.col = start;
                self.view.cursor.desired = start;
                self.ensure_visible();
            }
            None => self.show_message("no more links", false),
        }
    }
}

// --- search ---------------------------------------------------------------
impl App {
    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
        self.rebuild_matches();
        self.match_index = None;
    }

    pub fn rebuild_matches(&mut self) {
        self.matches.clear();
        if self.query.is_empty() {
            return;
        }
        let needle = if self.config.behavior.case_insensitive_search {
            self.query.to_lowercase()
        } else {
            self.query.clone()
        };
        for (row, layout_row) in self.page.layout.rows.iter().enumerate() {
            let mut hits = Vec::new();
            for run in &layout_row.runs {
                let haystack = if self.config.behavior.case_insensitive_search {
                    run.text.to_lowercase()
                } else {
                    run.text.clone()
                };
                let mut from = 0usize;
                while let Some(offset) = haystack[from..].find(&needle) {
                    let start = run.start + from + offset;
                    hits.push((start, start + needle.width()));
                    from += offset + needle.width().max(1);
                }
            }
            if !hits.is_empty() {
                self.matches.insert(row, hits);
            }
        }
    }

    /// Move to the next (`forward`) search hit.
    pub fn goto_match(&mut self, forward: bool) {
        if self.query.is_empty() {
            self.show_message("no search in progress (press /)", false);
            return;
        }
        let order: Vec<(usize, usize)> = if forward {
            self.matches.iter().flat_map(|(row, hits)| hits.iter().map(move |h| (*row, h.0))).collect()
        } else {
            self.matches
                .iter()
                .rev()
                .flat_map(|(row, hits)| hits.iter().rev().map(move |h| (*row, h.0)))
                .collect()
        };
        let here = (self.view.cursor.row, self.view.cursor.col);
        let target = if forward {
            order.into_iter().find(|p| *p > here).or_else(|| {
                let first =
                    self.matches.iter().flat_map(|(row, hits)| hits.iter().map(move |h| (*row, h.0))).next();
                first
            })
        } else {
            order.into_iter().find(|p| (p.0, p.1) < here).or_else(|| {
                self.matches
                    .iter()
                    .rev()
                    .flat_map(|(row, hits)| hits.iter().rev().map(move |h| (*row, h.0)))
                    .next()
            })
        };
        match target {
            Some((row, col)) => {
                self.view.cursor.row = row;
                self.view.cursor.col = col;
                self.view.cursor.desired = col;
                self.ensure_visible();
            }
            None => self.show_message(format!("no match for {:?}", self.query), true),
        }
    }
}

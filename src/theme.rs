use ratatui::style::Style;

use crate::config::{Config, StyleSpec};
use crate::error::Error;
use crate::status::StatusKind;

use std::collections::BTreeMap;

/// Config colors resolved once into ratatui styles.
#[derive(Debug, Clone)]
pub struct Theme {
    pub text: Style,
    pub link: Style,
    pub visited: Style,
    /// A link to another site than the one the page is on.
    pub external: Style,
    pub heading: Style,
    pub strong: Style,
    pub emphasis: Style,
    pub code: Style,
    pub muted: Style,
    pub divider: Style,
    pub placeholder: Style,
    pub monospace: Style,
    /// A `<details>` summary: plain text with an underline, and nothing else about it.
    pub disclosure: Style,
    /// The `"key"` of a JSON object: the half a reader scans for.
    pub json_key: Style,
    /// A quoted string value in JSON.
    pub json_string: Style,
    /// A number in JSON.
    pub json_number: Style,
    /// `true`, `false` and `null` in JSON.
    pub json_literal: Style,
    pub cursor: Style,
    pub cursor_line: Style,
    pub search_match: Style,
    /// Text marked with v or V.
    pub selection: Style,
    pub prompt: Style,
    pub message: Style,
    pub url: Style,
    pub title: Style,
    pub position: Style,
    pub scrollbar: Style,
    pub border: Style,
    /// The words inside the panes that float over the page: help, page info, the
    /// `g` menu. Carries no background unless the config gives it one.
    pub pane: Style,
    /// The frame around those panes.
    pub pane_border: Style,
    pub image: Style,
    pub linked_image: Style,
    pub hovered_link: Style,
    pub status: StatusTheme,
}

#[derive(Debug, Clone)]
pub struct StatusTheme {
    pub base: Style,
    pub loading: Style,
    pub ok: Style,
    pub redirect: Style,
    pub client_error: Style,
    pub server_error: Style,
    pub error: Style,
    pub codes: BTreeMap<u16, Style>,
}

impl StatusTheme {
    /// Style for the status segment: exact code override, then status class,
    /// then the plain status bar colors.
    pub fn for_status(&self, kind: StatusKind, code: Option<u16>) -> Style {
        if let Some(code) = code {
            if let Some(style) = self.codes.get(&code) {
                return *style;
            }
        }
        match kind {
            StatusKind::Loading => self.loading,
            StatusKind::Ok => self.ok,
            StatusKind::Redirect => self.redirect,
            StatusKind::ClientError => self.client_error,
            StatusKind::ServerError => self.server_error,
            StatusKind::NetError => self.error,
        }
    }
}

fn spec(s: &StyleSpec, name: &str) -> Result<Style, Error> {
    s.to_style().map_err(|message| Error::Config { path: format!("style.{name}"), message })
}

impl Theme {
    pub fn build(config: &Config) -> Result<Theme, Error> {
        let c = &config.style;
        let st = &c.status;
        Ok(Theme {
            text: spec(&c.text, "text")?,
            link: spec(&c.link, "link")?,
            visited: spec(&c.visited, "visited")?,
            external: spec(&c.external, "external")?,
            heading: spec(&c.heading, "heading")?,
            strong: spec(&c.strong, "strong")?,
            emphasis: spec(&c.emphasis, "emphasis")?,
            code: spec(&c.code, "code")?,
            muted: spec(&c.muted, "muted")?,
            divider: spec(&c.divider, "divider")?,
            placeholder: spec(&c.placeholder, "placeholder")?,
            monospace: spec(&c.monospace, "monospace")?,
            disclosure: spec(&c.disclosure, "disclosure")?,
            json_key: spec(&c.json_key, "json_key")?,
            json_string: spec(&c.json_string, "json_string")?,
            json_number: spec(&c.json_number, "json_number")?,
            json_literal: spec(&c.json_literal, "json_literal")?,
            cursor: spec(&c.cursor, "cursor")?,
            cursor_line: spec(&c.cursor_line, "cursor_line")?,
            search_match: spec(&c.search_match, "search_match")?,
            selection: spec(&c.selection, "selection")?,
            prompt: spec(&c.prompt, "prompt")?,
            message: spec(&c.message, "message")?,
            url: spec(&c.url, "url")?,
            title: spec(&c.title, "title")?,
            position: spec(&c.position, "position")?,
            scrollbar: spec(&c.scrollbar, "scrollbar")?,
            border: spec(&c.border, "border")?,
            pane: spec(&c.pane, "pane")?,
            pane_border: spec(&c.pane_border, "pane_border")?,
            image: spec(&c.image, "image")?,
            linked_image: spec(&c.linked_image, "linked_image")?,
            hovered_link: spec(&c.hovered_link, "hovered_link")?,
            status: StatusTheme {
                base: spec(&st.base_spec(), "status")?,
                loading: spec(&st.loading, "status.loading")?,
                ok: spec(&st.ok, "status.ok")?,
                redirect: spec(&st.redirect, "status.redirect")?,
                client_error: spec(&st.client_error, "status.client_error")?,
                server_error: spec(&st.server_error, "status.server_error")?,
                error: spec(&st.error, "status.error")?,
                codes: st
                    .codes
                    .iter()
                    .map(|(code, s)| {
                        let code: u16 = code.parse().map_err(|_| Error::Config {
                            path: "style.status.codes".into(),
                            message: format!("{code:?} is not a status number"),
                        })?;
                        Ok((code, spec(s, &format!("status.codes.{code}"))?))
                    })
                    .collect::<Result<BTreeMap<u16, Style>, Error>>()?,
            },
        })
    }
}

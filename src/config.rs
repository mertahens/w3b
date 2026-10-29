use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

use crate::error::Error;

pub const DEFAULT_CONFIG_TOML: &str = include_str!("../config/default.toml");

pub fn config_path() -> PathBuf {
    dirs::config_dir().map(|d| d.join("w3b").join("config.toml")).unwrap_or_else(|| PathBuf::from("w3b.toml"))
}

/// A color/style block from the config file.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StyleSpec {
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
    pub dim: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StatusStyle {
    /// Default colors for the status bar itself.
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
    pub dim: bool,
    pub loading: StyleSpec,
    pub ok: StyleSpec,
    pub redirect: StyleSpec,
    pub client_error: StyleSpec,
    pub server_error: StyleSpec,
    pub error: StyleSpec,
    /// Exact status code overrides, e.g. `["404"]`.
    pub codes: BTreeMap<String, StyleSpec>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StyleConfig {
    pub text: StyleSpec,
    pub link: StyleSpec,
    pub visited: StyleSpec,
    /// A link to another site than the page it is on.
    pub external: StyleSpec,
    pub heading: StyleSpec,
    pub strong: StyleSpec,
    pub emphasis: StyleSpec,
    pub code: StyleSpec,
    pub muted: StyleSpec,
    pub divider: StyleSpec,
    pub placeholder: StyleSpec,
    pub cursor: StyleSpec,
    pub cursor_line: StyleSpec,
    pub search_match: StyleSpec,
    pub selection: StyleSpec,
    pub prompt: StyleSpec,
    pub message: StyleSpec,
    pub url: StyleSpec,
    pub title: StyleSpec,
    pub position: StyleSpec,
    pub scrollbar: StyleSpec,
    pub border: StyleSpec,
    /// The ink of what floats over the page: the help and info panes and the `g`
    /// menu. No `bg` means the terminal's own background shows through the frame.
    pub pane: StyleSpec,
    /// The frame around those panes, one step brighter than the page's own rules.
    pub pane_border: StyleSpec,
    /// Image placeholders such as `[alt text]`.
    pub image: StyleSpec,
    /// An image placeholder inside a link.
    pub linked_image: StyleSpec,
    /// Elements that ask for a monospace font, marked over the text itself.
    pub monospace: StyleSpec,
    /// The summary line of a `<details>`: plain text with an underline, because it
    /// is something to open rather than somewhere to go.
    pub disclosure: StyleSpec,
    /// The `"key"` of a JSON object: the half a reader scans for.
    pub json_key: StyleSpec,
    /// A quoted string value in JSON.
    pub json_string: StyleSpec,
    /// A number in JSON.
    pub json_number: StyleSpec,
    /// `true`, `false` and `null` in JSON.
    pub json_literal: StyleSpec,
    /// The link the cursor is standing on.
    pub hovered_link: StyleSpec,
    pub status: StatusStyle,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Behavior {
    /// URL opened when none is given on the command line.
    pub home_url: String,
    pub user_agent: String,
    pub timeout_secs: u64,
    pub max_redirects: u8,
    pub max_response_bytes: u64,
    pub scroll_margin: usize,
    pub tab_width: usize,
    pub wrap: bool,
    pub show_title_in_status: bool,
    pub show_scrollbar: bool,
    pub mouse: bool,
    pub case_insensitive_search: bool,
    pub history_size: usize,
    /// Blank columns kept between the terminal edge and the page text.
    pub page_padding_x: usize,
    /// Copy what was yanked - a link, a media address, the page - to the
    /// terminal clipboard with OSC 52.
    pub yank_to_clipboard: bool,
    /// How many linked stylesheets a page may pull in at most.
    pub max_stylesheets: usize,
    /// Show how long the page took to fetch on the right of the status line.
    pub show_load_time: bool,
    /// Show how big the page was, on the right of the status line next to the
    /// time: bytes below a kilobyte, then kB and MB.
    pub show_size: bool,
}

impl Default for Behavior {
    fn default() -> Self {
        Behavior {
            home_url: "https://example.com".to_string(),
            user_agent: "w3b/0.1 (text mode browser)".to_string(),
            timeout_secs: 15,
            max_redirects: 10,
            max_response_bytes: 8_388_608,
            scroll_margin: 3,
            tab_width: 4,
            wrap: true,
            show_title_in_status: true,
            show_scrollbar: false,
            mouse: true,
            case_insensitive_search: true,
            history_size: 200,
            page_padding_x: 1,
            yank_to_clipboard: true,
            max_stylesheets: 4,
            show_load_time: true,
            show_size: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub style: StyleConfig,
    pub behavior: Behavior,
}

impl Default for Config {
    /// Neutral defaults (terminal colors). The shipped `config/default.toml`
    /// is what `load_optional` falls back to, so the two stay independent and
    /// `Default` can never recurse back into deserialization.
    fn default() -> Self {
        Config { style: StyleConfig::default(), behavior: Behavior::default() }
    }
}

/// The built-in configuration file, parsed once.
pub fn built_in_defaults() -> Config {
    static DEFAULTS: std::sync::OnceLock<Config> = std::sync::OnceLock::new();
    DEFAULTS
        .get_or_init(|| {
            from_toml(DEFAULT_CONFIG_TOML, "built-in default").expect("built-in config must be valid")
        })
        .clone()
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, Error> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| Error::Config { path: path.display().to_string(), message: e.to_string() })?;
        let cfg = from_toml(&text, &path.display().to_string())?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Load `path` if it exists, otherwise use the built-in defaults.
    pub fn load_optional(path: &Path) -> Result<(Config, bool), Error> {
        if path.exists() {
            Ok((Self::load(path)?, true))
        } else {
            Ok((built_in_defaults(), false))
        }
    }

    pub fn write_default(path: &Path) -> Result<(), Error> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(path, DEFAULT_CONFIG_TOML)?;
        Ok(())
    }

    /// Reject colors we cannot render, before the TUI opens.
    pub fn validate(&self) -> Result<(), Error> {
        let groups = [
            ("text", &self.style.text),
            ("link", &self.style.link),
            ("visited", &self.style.visited),
            ("external", &self.style.external),
            ("heading", &self.style.heading),
            ("strong", &self.style.strong),
            ("emphasis", &self.style.emphasis),
            ("code", &self.style.code),
            ("muted", &self.style.muted),
            ("divider", &self.style.divider),
            ("placeholder", &self.style.placeholder),
            ("cursor", &self.style.cursor),
            ("cursor_line", &self.style.cursor_line),
            ("search_match", &self.style.search_match),
            ("prompt", &self.style.prompt),
            ("message", &self.style.message),
            ("url", &self.style.url),
            ("title", &self.style.title),
            ("position", &self.style.position),
            ("scrollbar", &self.style.scrollbar),
            ("border", &self.style.border),
            ("pane", &self.style.pane),
            ("pane_border", &self.style.pane_border),
            ("disclosure", &self.style.disclosure),
        ];
        for (name, spec) in groups {
            check_spec(spec, name)?;
        }
        let s = &self.style.status;
        check_spec(&s.base_spec(), "status")?;
        check_spec(&s.loading, "status.loading")?;
        check_spec(&s.ok, "status.ok")?;
        check_spec(&s.redirect, "status.redirect")?;
        check_spec(&s.client_error, "status.client_error")?;
        check_spec(&s.server_error, "status.server_error")?;
        check_spec(&s.error, "status.error")?;
        for (code, spec) in &s.codes {
            let code_num: u16 = code.parse().map_err(|_| Error::Config {
                path: "config".into(),
                message: format!("[style.status.codes] key {code:?} is not a status number"),
            })?;
            check_spec(spec, &format!("status.codes.{code_num}"))?;
        }
        Ok(())
    }
}

fn check_spec(spec: &StyleSpec, name: &str) -> Result<(), Error> {
    for (field, value) in [("fg", &spec.fg), ("bg", &spec.bg)] {
        if let Some(value) = value {
            parse_color(value)
                .map_err(|e| Error::Config { path: format!("style.{name}.{field}"), message: e })?;
        }
    }
    Ok(())
}

impl StatusStyle {
    /// Colors for segments of the bar that have no class of their own.
    pub fn base_spec(&self) -> StyleSpec {
        StyleSpec {
            fg: self.fg.clone(),
            bg: self.bg.clone(),
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            reverse: self.reverse,
            dim: self.dim,
        }
    }
}

fn from_toml(text: &str, path: &str) -> Result<Config, Error> {
    toml::from_str(text).map_err(|e| Error::Config { path: path.into(), message: e.to_string() })
}

/// "default" | "reset" | "none" => terminal default. Otherwise a name,
/// an index (0..=255) or a hex color.
pub fn parse_color(value: &str) -> Result<Option<Color>, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "default" | "reset" | "none" | "transparent" => return Ok(None),
        _ => {}
    }
    value
        .trim()
        .parse::<Color>()
        .map(Some)
        .map_err(|_| format!("{value:?} is not a color name, index 0-255 or #rrggbb"))
}

impl StyleSpec {
    pub fn to_style(&self) -> Result<Style, String> {
        let mut style = Style::new();
        if let Some(c) = self.fg.as_deref().map(parse_color).transpose()?.flatten() {
            style = style.fg(c);
        }
        if let Some(c) = self.bg.as_deref().map(parse_color).transpose()?.flatten() {
            style = style.bg(c);
        }
        for (on, m) in [
            (self.bold, Modifier::BOLD),
            (self.italic, Modifier::ITALIC),
            (self.underline, Modifier::UNDERLINED),
            (self.reverse, Modifier::REVERSED),
            (self.dim, Modifier::DIM),
        ] {
            if on {
                style = style.add_modifier(m);
            }
        }
        Ok(style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_default_file_is_valid_and_complete() {
        let cfg = built_in_defaults();
        assert_eq!(cfg.behavior.user_agent, "w3b/0.1 (text mode browser)");
        assert_eq!(cfg.behavior.timeout_secs, 15);
        assert_eq!(cfg.behavior.max_response_bytes, 8_388_608);
        assert_eq!(cfg.behavior.home_url, "https://example.com");
        assert_eq!(cfg.style.status.codes.get("404").unwrap().bg.as_deref(), Some("magenta"));
        assert_eq!(cfg.style.status.ok.bg.as_deref(), Some("green"));
        cfg.validate().expect("shipped defaults must validate");
    }

    #[test]
    fn default_never_reenters_deserialization() {
        // Regression: Default used to re-parse DEFAULT_CONFIG_TOML and recurse.
        let cfg = Config::default();
        assert_eq!(cfg.behavior.scroll_margin, 3);
        assert!(!cfg.style.link.underline);
        assert_eq!(cfg.style.link.fg, None);
    }

    #[test]
    fn partial_file_keeps_built_in_behavior_defaults() {
        let cfg: Config = toml::from_str("[behavior]\ntimeout_secs = 2\n").unwrap();
        assert_eq!(cfg.behavior.timeout_secs, 2);
        assert_eq!(cfg.behavior.max_redirects, 10);
        assert_eq!(cfg.behavior.home_url, "https://example.com");
        // Style blocks are opt-in: an absent block means terminal defaults.
        assert_eq!(cfg.style.link.fg, None);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let err = toml::from_str::<Config>("[behavior]\nnot_a_key = 1\n").unwrap_err();
        assert!(err.to_string().contains("not_a_key"), "{err}");
        let err = toml::from_str::<Config>("[style]\ntypo = 1\n").unwrap_err();
        assert!(err.to_string().contains("typo"), "{err}");
    }

    #[test]
    fn status_code_overrides_are_kept_separately() {
        let cfg: Config = toml::from_str(
            r#"
            [style.status]
            bg = "black"
            [style.status.error]
            fg = "red"
            underline = true
            [style.status.codes."500"]
            fg = "white"
            bg = "red"
            italic = true
            "#,
        )
        .unwrap();
        assert_eq!(cfg.style.status.bg.as_deref(), Some("black"));
        assert!(cfg.style.status.error.underline);
        assert!(cfg.style.status.error.fg.is_some());
        assert_eq!(cfg.style.status.ok.fg, None, "untouched classes stay untouched");
        let codes = &cfg.style.status.codes;
        assert_eq!(codes.len(), 1);
        assert!(codes["500"].italic);
        assert_eq!(cfg.style.status.base_spec().bg.as_deref(), Some("black"));
    }

    #[test]
    fn colors_accept_names_indices_and_hex() {
        assert_eq!(parse_color("red").unwrap(), Some(Color::Red));
        assert_eq!(parse_color("bright-blue").unwrap(), Some(Color::LightBlue));
        assert_eq!(parse_color("12").unwrap(), Some(Color::Indexed(12)));
        assert!(parse_color("#ff00aa").unwrap().is_some());
        for blank in ["", "default", "reset", "none", "transparent"] {
            assert_eq!(parse_color(blank).unwrap(), None, "{blank} means terminal default");
        }
    }

    #[test]
    fn bad_colors_are_reported_with_their_path() {
        let err = parse_color("not-a-color").unwrap_err();
        assert!(err.contains("not-a-color"), "{err}");
        let cfg: Config = toml::from_str(
            r#"[style.link]
fg = "not-a-color"
"#,
        )
        .unwrap();
        let err = cfg.validate().unwrap_err();
        match err {
            Error::Config { path, message } => {
                assert_eq!(path, "style.link.fg");
                assert!(message.contains("not-a-color"), "{message}");
            }
            other => panic!("expected a config error, got {other:?}"),
        }
    }

    #[test]
    fn non_numeric_status_code_keys_are_rejected() {
        let cfg: Config = toml::from_str(
            r#"[style.status.codes."oops"]
fg = "red"
"#,
        )
        .unwrap();
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("oops"), "{err}");
    }

    #[test]
    fn style_spec_maps_to_a_ratatui_style() {
        let spec = StyleSpec {
            fg: Some("red".into()),
            bg: Some("2".into()),
            bold: true,
            italic: true,
            underline: true,
            reverse: false,
            dim: false,
        };
        let style = spec.to_style().unwrap();
        assert_eq!(style.fg, Some(Color::Red));
        assert_eq!(style.bg, Some(Color::Indexed(2)));
        for m in [Modifier::BOLD, Modifier::ITALIC, Modifier::UNDERLINED] {
            assert!(style.add_modifier.contains(m), "{m:?} missing");
        }
    }
}

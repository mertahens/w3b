//! w3b - a vim-like text mode web browser.
//!
//! The crate is split so every layer can be tested without a terminal or a
//! network: fetch (`net`) -> decode (`charset`) -> parse (`dom`) -> flow
//! (`layout`) -> paint (`render`), with `app` holding the state machine.
pub mod app;
pub mod charset;
pub mod cli;
pub mod config;
pub mod css;
pub mod dom;
pub mod dump;
pub mod error;
pub mod keys;
pub mod layout;
pub mod net;
pub mod page;
pub mod render;
pub mod sanitize;
pub mod status;
pub mod theme;
pub mod tui;

pub use app::{Action, App, Mode, Viewport};
pub use config::Config;
pub use error::Error;
pub use net::{FakeFetcher, Fetcher, HttpFetcher};
pub use theme::Theme;

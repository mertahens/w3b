//! Command line interface.
use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "w3b", version, about = "A vim-like text mode web browser")]
pub struct Cli {
    /// URL, host, or search terms. Falls back to `behavior.home_url`.
    #[arg(value_name = "TARGET")]
    pub target: Option<String>,

    /// Path to the config file (default: $XDG_CONFIG_HOME/w3b/config.toml).
    #[arg(short = 'c', long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Print the page as plain text and exit instead of starting the TUI.
    #[arg(short = 'd', long)]
    pub dump: bool,

    /// Width used by --dump.
    #[arg(long, default_value_t = 80, value_name = "COLUMNS")]
    pub width: usize,

    /// Write the default config file and exit.
    #[arg(long)]
    pub init_config: bool,

    /// Print the built-in default config and exit.
    #[arg(long)]
    pub print_config: bool,

    /// Where --init-config writes. Defaults to the config path.
    #[arg(long, value_name = "PATH")]
    pub config_out: Option<PathBuf>,
}

impl Cli {
    pub fn parse_and_env() -> Cli {
        Cli::parse()
    }
}

/// Config file location: `--config`, then `$W3B_CONFIG`, then the XDG default.
pub fn config_path(cli: &Cli) -> PathBuf {
    if let Some(path) = &cli.config {
        return path.clone();
    }
    if let Ok(from_env) = std::env::var("W3B_CONFIG") {
        return PathBuf::from(from_env);
    }
    crate::config::config_path()
}

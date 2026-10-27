use std::io::IsTerminal;
use std::process::ExitCode;
use std::sync::Arc;

use w3b::app::Viewport;
use w3b::cli::{config_path, Cli};
use w3b::config::Config;
use w3b::dump::plain_text;
use w3b::error::Error;
use w3b::net::{Fetcher, HttpFetcher};
use w3b::status::StatusKind;
use w3b::theme::Theme;
use w3b::App;

fn main() -> ExitCode {
    install_panic_hook();
    match real_main() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("w3b: {err}");
            ExitCode::from(2)
        }
    }
}

/// A panic inside the TUI must leave the terminal usable.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        ratatui::restore();
        eprintln!("\nw3b panicked: {info}");
        eprintln!("(terminal restored; the location above is where it happened)");
    }));
}

fn real_main() -> Result<ExitCode, Error> {
    let cli = Cli::parse_and_env();

    if cli.print_config {
        print!("{}", w3b::config::DEFAULT_CONFIG_TOML);
        return Ok(ExitCode::SUCCESS);
    }

    let path = config_path(&cli);
    if cli.init_config {
        let target = cli.config_out.clone().unwrap_or_else(|| path.clone());
        Config::write_default(&target)?;
        println!("wrote default config to {}", target.display());
        return Ok(ExitCode::SUCCESS);
    }

    let (config, loaded) = Config::load_optional(&path)?;
    if !loaded {
        eprintln!("w3b: no config at {}, using defaults (w3b --init-config to create one)", path.display());
    }
    let theme = Theme::build(&config)?;

    let target = cli.target.clone().unwrap_or_else(|| config.behavior.home_url.clone());
    let fetcher: Arc<dyn Fetcher> = Arc::new(HttpFetcher::new(&config.behavior));

    if cli.dump {
        return dump(&cli, config, theme, fetcher, &target);
    }

    // The interactive UI needs a terminal; without one, ratatui would fail in a
    // confusing way after the fetch already happened.
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(Error::Unrenderable(
            "stdin/stdout is not a terminal: use --dump <url> to print the page as text".into(),
        ));
    }

    let size = ratatui::crossterm::terminal::size().unwrap_or((80, 24));
    let viewport = Viewport { width: usize::from(size.0), height: usize::from(size.1) };
    let app = App::new(config, theme, fetcher, &target, viewport);
    w3b::tui::run(app)?;
    Ok(ExitCode::SUCCESS)
}

/// `w3b --dump <url>`: same pipeline as the TUI, plain text on stdout.
fn dump(
    cli: &Cli,
    config: Config,
    theme: Theme,
    fetcher: Arc<dyn Fetcher>,
    target: &str,
) -> Result<ExitCode, Error> {
    let viewport = Viewport { width: cli.width.max(20), height: 24 };
    let mut app = App::empty(config, theme, fetcher, target, viewport);
    app.open(target);

    print!("{}", plain_text(app.layout()));
    let status = app.status();
    let took = match (app.config.behavior.show_load_time, app.load_time_text()) {
        (true, Some(t)) => format!(" ({t})"),
        _ => String::new(),
    };
    eprintln!("[{}] {} - {}{took}", status.text(), app.url(), app.title());
    let code = match status.kind() {
        StatusKind::Ok | StatusKind::Redirect => 0,
        StatusKind::ClientError | StatusKind::ServerError => 1,
        _ => 2,
    };
    Ok(ExitCode::from(code))
}

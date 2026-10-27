//! The terminal event loop.
use std::io::{self, Write};
use std::time::Duration;

use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use ratatui::crossterm::execute;
use ratatui::{DefaultTerminal, Frame};

use crate::app::{Action, App};
use crate::error::Error;
use crate::render;

/// How long to wait for input before checking for finished fetches.
const POLL: Duration = Duration::from_millis(60);

pub fn run(mut app: App) -> Result<(), Error> {
    let mouse = app.config.behavior.mouse;
    let mut terminal = ratatui::init();
    if mouse {
        let _ = execute!(io::stdout(), EnableMouseCapture);
    }
    let result = event_loop(&mut terminal, &mut app);
    if mouse {
        let _ = execute!(io::stdout(), DisableMouseCapture);
    }
    ratatui::restore();
    result.map_err(Error::from)
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    let size = terminal.size()?;
    app.resize(usize::from(size.width), usize::from(size.height));

    loop {
        terminal.draw(|frame| draw(frame, app))?;

        if event::poll(POLL)? {
            match event::read()? {
                Event::Key(key) => match app.handle_key(key) {
                    Action::Quit => return Ok(()),
                    Action::Yank(target) => {
                        yank_to_clipboard(&target, app.config.behavior.yank_to_clipboard)?
                    }
                    Action::None => {}
                },
                Event::Mouse(mouse) => {
                    app.handle_mouse(mouse);
                }
                Event::Resize(width, height) => app.resize(usize::from(width), usize::from(height)),
                _ => {}
            }
        }
        app.poll();
    }
}

/// Ask the terminal emulator to put the text on its own clipboard (OSC 52).
fn yank_to_clipboard(text: &str, enabled: bool) -> io::Result<()> {
    if !enabled {
        return Ok(());
    }
    let mut out = io::stdout();
    write!(out, "\x1b]52;c;{}\x1b\\", base64(text))?;
    out.flush()
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        let chars = [
            B64[(n >> 18) as usize & 63] as char,
            B64[(n >> 12) as usize & 63] as char,
            if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' },
            if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' },
        ];
        out.extend(chars);
    }
    out
}

fn draw(frame: &mut Frame, app: &mut App) {
    render::draw_all(app, frame.area(), frame.buffer_mut());
    if matches!(app.mode, crate::app::Mode::Open | crate::app::Mode::Search) {
        let row = frame.area().height.saturating_sub(2);
        let prefix = if app.mode == crate::app::Mode::Open { 6 } else { 1 };
        let column = (prefix + app.input_pos) as u16;
        frame.set_cursor_position((column, row));
    }
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64(""), "");
        assert_eq!(base64("a"), "YQ==");
        assert_eq!(base64("ab"), "YWI=");
        assert_eq!(base64("abc"), "YWJj");
        assert_eq!(base64("hello"), "aGVsbG8=");
        assert_eq!(base64("https://example.com/?q=a b"), "aHR0cHM6Ly9leGFtcGxlLmNvbS8/cT1hIGI=");
    }
}

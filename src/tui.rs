//! The terminal event loop.
use std::io::{self, Write};
use std::time::Duration;

use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, MouseEventKind};
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

        for ev in fold_wheel(burst()?) {
            match ev {
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
        // The loading animation turns even when no input arrives.
        if app.is_loading() {
            app.tick();
        }
    }
}

/// Read one burst: the first waiting event, then everything the terminal sent
/// along with it. Without the drain a burst is one line of movement per loop
/// turn, which is what makes a wheel that reports three events per notch jump.
fn burst() -> io::Result<Vec<Event>> {
    let mut events = Vec::new();
    if event::poll(POLL)? {
        events.push(event::read()?);
        while event::poll(Duration::ZERO)? {
            events.push(event::read()?);
        }
    }
    Ok(events)
}

/// One notch, one line - but a terminal is free to report a single notch as several
/// events, and several do: WezTerm multiplies the wheel by three out of the box, so
/// the notch arrives as a run of three. A run of the same wheel event inside one
/// burst is folded into the step it describes. Anything else in the burst - a key,
/// a resize, the opposite wheel direction - is kept where it was.
fn fold_wheel(events: Vec<Event>) -> Vec<Event> {
    let mut out = Vec::with_capacity(events.len());
    let mut rolled: Option<MouseEventKind> = None;
    for ev in events {
        match &ev {
            Event::Mouse(m) if matches!(m.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown) => {
                if rolled == Some(m.kind) {
                    continue;
                }
                rolled = Some(m.kind);
            }
            _ => rolled = None,
        }
        out.push(ev);
    }
    out
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
    if let Some((column, row)) = render::cursor_screen_pos(app, frame.area()) {
        frame.set_cursor_position((column, row));
    }
}

#[cfg(test)]
mod tests {
    use super::{base64, fold_wheel};
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

    fn notch(up: bool) -> Event {
        let kind = if up { MouseEventKind::ScrollUp } else { MouseEventKind::ScrollDown };
        Event::Mouse(MouseEvent { kind, column: 4, row: 4, modifiers: KeyModifiers::NONE })
    }

    fn typed(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    #[test]
    fn a_burst_of_wheel_events_is_one_step() {
        // WezTerm's default wheel multiplier reports one notch as three events.
        let burst = vec![notch(false), notch(false), notch(false)];
        assert_eq!(fold_wheel(burst), vec![notch(false)], "three events, one line");
    }

    #[test]
    fn a_wheel_burst_keeps_everything_that_is_not_the_wheel() {
        let burst = vec![notch(false), notch(false), typed('j'), notch(true), notch(true), notch(true)];
        assert_eq!(fold_wheel(burst), vec![notch(false), typed('j'), notch(true)]);
    }

    #[test]
    fn the_wheel_turning_both_ways_in_one_burst_is_both_steps() {
        let burst = vec![notch(false), notch(false), notch(true), notch(false)];
        assert_eq!(fold_wheel(burst), vec![notch(false), notch(true), notch(false)]);
    }

    #[test]
    fn a_burst_of_nothing_is_nothing_to_do() {
        assert!(fold_wheel(Vec::new()).is_empty());
    }

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

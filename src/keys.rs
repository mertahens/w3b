//! Key and mouse bindings (vim-ish).
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use crate::app::{Action, App, Mode};

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.kind == ratatui::crossterm::event::KeyEventKind::Release {
            return Action::None;
        }
        self.clear_message();
        match self.mode {
            Mode::Open | Mode::Search => self.handle_input_key(key),
            Mode::Help => {
                self.mode = Mode::Normal;
                Action::None
            }
            Mode::Normal => self.handle_normal_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let code = key.code;
        let mut yanked: Option<String> = None;
        match code {
            KeyCode::Char('c') if ctrl => {
                self.quit = true;
                return Action::Quit;
            }
            KeyCode::Char('q') if !ctrl => {
                self.quit = true;
                return Action::Quit;
            }
            KeyCode::Char('j') | KeyCode::Down => self.down(1),
            KeyCode::Char('k') | KeyCode::Up => self.up(1),
            KeyCode::Char('h') | KeyCode::Left => self.left(),
            KeyCode::Char('l') | KeyCode::Right => self.right(),
            KeyCode::Char('g') => {
                if self.pending_g {
                    self.pending_g = false;
                    self.to_top();
                } else {
                    self.pending_g = true;
                    self.show_message("g… (gg for top)", false);
                }
            }
            KeyCode::Char('G') => self.to_bottom(),
            KeyCode::Char('0') => self.line_start(),
            KeyCode::Char('$') | KeyCode::End => self.line_end(),
            KeyCode::Char('^') => self.first_non_blank(),
            KeyCode::Char('d') if ctrl => self.page_down(true),
            KeyCode::Char('u') if ctrl => self.page_up(true),
            KeyCode::Char('f') if ctrl => self.page_down(false),
            KeyCode::Char('b') if ctrl => self.page_up(false),
            KeyCode::PageDown => self.page_down(false),
            KeyCode::PageUp => self.page_up(false),
            KeyCode::Enter => self.follow_cursor(),
            KeyCode::Tab => self.next_link(),
            KeyCode::BackTab => self.prev_link(),
            KeyCode::Char('o') | KeyCode::Char(':') => self.enter_input_mode(Mode::Open, String::new()),
            KeyCode::Char('/') => self.enter_input_mode(Mode::Search, self.query.clone()),
            KeyCode::Char('n') => self.goto_match(true),
            KeyCode::Char('N') => self.goto_match(false),
            KeyCode::Char('r') => self.reload(),
            KeyCode::Backspace | KeyCode::Char('[') => self.go_back(),
            KeyCode::Char(']') => self.go_forward(),
            KeyCode::Char('y') => yanked = self.yank_cursor(),
            KeyCode::Char('w') => self.toggle_wrap(),
            KeyCode::Char('i') | KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Esc => {
                self.pending_g = false;
                self.input.clear();
                self.input_pos = 0;
            }
            _ => {}
        }
        if !matches!(code, KeyCode::Char('g')) {
            self.pending_g = false;
        }
        match yanked {
            Some(target) => Action::Yank(target),
            None => Action::None,
        }
    }

    fn handle_input_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.input.clear();
                self.input_pos = 0;
            }
            KeyCode::Enter => {
                let input = std::mem::take(&mut self.input);
                self.input_pos = 0;
                let mode = self.mode;
                self.mode = Mode::Normal;
                match mode {
                    Mode::Open => {
                        let target = strip_open_prefix(&input);
                        self.open_typed(target);
                    }
                    Mode::Search => {
                        self.set_query(&input);
                        self.goto_match(true);
                    }
                    _ => {}
                }
            }
            KeyCode::Backspace => self.edit_backspace(),
            KeyCode::Delete => self.edit_delete(),
            KeyCode::Left => self.edit_left(),
            KeyCode::Right => self.edit_right(),
            KeyCode::Home => self.edit_home(),
            KeyCode::End => self.edit_end(),
            KeyCode::Char('u') if ctrl => {
                self.input.clear();
                self.input_pos = 0;
            }
            KeyCode::Char(c) if !ctrl => self.edit_insert(c),
            _ => {}
        }
        Action::None
    }

    pub fn handle_mouse(&mut self, ev: MouseEvent) -> Action {
        match ev.kind {
            MouseEventKind::ScrollDown => self.down(3),
            MouseEventKind::ScrollUp => self.up(3),
            MouseEventKind::Down(ratatui::crossterm::event::MouseButton::Left) => {
                let row = self.view.scroll + ev.row as usize;
                let col = self.view.hscroll + (ev.column as usize).saturating_sub(self.padding_x());
                if row < self.page.row_count() {
                    self.view.cursor.row = row;
                    self.view.cursor.col = col;
                    self.view.cursor.desired = col;
                    self.ensure_visible();
                    if self.link_under_cursor().is_some() {
                        self.follow_cursor();
                    }
                }
            }
            _ => {}
        }
        Action::None
    }
}

/// Line editing for the `:open` and `/` prompts.
impl App {
    fn byte_index(&self, chars: usize) -> usize {
        self.input.char_indices().map(|(i, _)| i).nth(chars).unwrap_or(self.input.len())
    }

    fn input_chars(&self) -> usize {
        self.input.chars().count()
    }

    pub fn edit_insert(&mut self, c: char) {
        let at = self.byte_index(self.input_pos);
        self.input.insert(at, c);
        self.input_pos += 1;
    }

    pub fn edit_backspace(&mut self) {
        if self.input_pos == 0 {
            return;
        }
        let end = self.byte_index(self.input_pos);
        let start = self.byte_index(self.input_pos - 1);
        self.input.replace_range(start..end, "");
        self.input_pos -= 1;
    }

    pub fn edit_delete(&mut self) {
        if self.input_pos >= self.input_chars() {
            return;
        }
        let start = self.byte_index(self.input_pos);
        let end = self.byte_index(self.input_pos + 1);
        self.input.replace_range(start..end, "");
    }

    pub fn edit_left(&mut self) {
        self.input_pos = self.input_pos.saturating_sub(1);
    }

    pub fn edit_right(&mut self) {
        self.input_pos = (self.input_pos + 1).min(self.input_chars());
    }

    pub fn edit_home(&mut self) {
        self.input_pos = 0;
    }

    pub fn edit_end(&mut self) {
        self.input_pos = self.input_chars();
    }
}

/// Accept both `:open example.com` and a bare `example.com`.
pub fn strip_open_prefix(input: &str) -> &str {
    let input = input.trim();
    if let Some(rest) = input.strip_prefix(":open") {
        rest.trim_start()
    } else if let Some(rest) = input.strip_prefix(':') {
        rest.trim_start()
    } else {
        input
    }
}

#[cfg(test)]
mod tests {
    use super::strip_open_prefix;

    #[test]
    fn open_prefix_variants() {
        assert_eq!(strip_open_prefix(":open example.com"), "example.com");
        assert_eq!(strip_open_prefix(":example.com"), "example.com");
        assert_eq!(strip_open_prefix("example.com"), "example.com");
        assert_eq!(strip_open_prefix("  :open  example.com  "), "example.com");
    }
}

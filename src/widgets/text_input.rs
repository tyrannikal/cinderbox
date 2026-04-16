use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::Rect,
    style::Stylize,
    text::Line,
    widgets::{Block, Paragraph},
};

#[derive(Debug, Default)]
pub struct TextInput {
    pub value: String,
    cursor: usize,
    pub label: String,
}

impl TextInput {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..Default::default()
        }
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self.cursor = self.value.len();
        self
    }

    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.cursor = self.cursor.min(self.value.len());
    }

    pub fn handle_input(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char(c) => {
                self.value.insert(self.cursor, c);
                self.cursor += 1;
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.value.remove(self.cursor);
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.value.len() {
                    self.value.remove(self.cursor);
                }
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                if self.cursor < self.value.len() {
                    self.cursor += 1;
                }
            }
            KeyCode::Home => {
                self.cursor = 0;
            }
            KeyCode::End => {
                self.cursor = self.value.len();
            }
            _ => {}
        }
    }

    #[cfg(test)]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let style = if focused {
            ratatui::style::Style::default().white()
        } else {
            ratatui::style::Style::default().dark_gray()
        };

        let block = Block::bordered().title(Line::from(format!(" {} ", self.label)).bold()).style(style);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Render text with cursor
        let display = if focused {
            let before = &self.value[..self.cursor];
            let cursor_char = self.value.get(self.cursor..self.cursor + 1).unwrap_or(" ");
            let after = if self.cursor < self.value.len() {
                &self.value[self.cursor + 1..]
            } else {
                ""
            };
            Line::from(vec![
                before.into(),
                cursor_char.reversed(),
                after.into(),
            ])
        } else {
            Line::from(self.value.as_str())
        };

        frame.render_widget(Paragraph::new(display), inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_empty_input_with_label() {
        let input = TextInput::new("Name");
        assert_eq!(input.label, "Name");
        assert_eq!(input.value, "");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn with_value_sets_cursor_to_end() {
        let input = TextInput::new("Name").with_value("hello");
        assert_eq!(input.value, "hello");
        assert_eq!(input.cursor(), 5);
    }

    #[test]
    fn set_value_clamps_cursor() {
        let mut input = TextInput::new("Name").with_value("hello");
        assert_eq!(input.cursor(), 5);
        input.set_value("hi");
        assert_eq!(input.value, "hi");
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn set_value_preserves_cursor_when_within_bounds() {
        let mut input = TextInput::new("Name").with_value("hello");
        input.handle_input(KeyCode::Home);
        input.handle_input(KeyCode::Right);
        assert_eq!(input.cursor(), 1);
        input.set_value("world");
        assert_eq!(input.cursor(), 1);
    }

    #[test]
    fn char_insertion_at_end() {
        let mut input = TextInput::new("X");
        input.handle_input(KeyCode::Char('a'));
        input.handle_input(KeyCode::Char('b'));
        assert_eq!(input.value, "ab");
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn char_insertion_in_middle() {
        let mut input = TextInput::new("X").with_value("ac");
        input.handle_input(KeyCode::Home);
        input.handle_input(KeyCode::Right);
        input.handle_input(KeyCode::Char('b'));
        assert_eq!(input.value, "abc");
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn backspace_deletes_before_cursor() {
        let mut input = TextInput::new("X").with_value("abc");
        input.handle_input(KeyCode::Backspace);
        assert_eq!(input.value, "ab");
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let mut input = TextInput::new("X").with_value("abc");
        input.handle_input(KeyCode::Home);
        input.handle_input(KeyCode::Backspace);
        assert_eq!(input.value, "abc");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn delete_removes_char_at_cursor() {
        let mut input = TextInput::new("X").with_value("abc");
        input.handle_input(KeyCode::Home);
        input.handle_input(KeyCode::Delete);
        assert_eq!(input.value, "bc");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn delete_at_end_is_noop() {
        let mut input = TextInput::new("X").with_value("abc");
        input.handle_input(KeyCode::Delete);
        assert_eq!(input.value, "abc");
        assert_eq!(input.cursor(), 3);
    }

    #[test]
    fn left_movement() {
        let mut input = TextInput::new("X").with_value("abc");
        assert_eq!(input.cursor(), 3);
        input.handle_input(KeyCode::Left);
        assert_eq!(input.cursor(), 2);
        input.handle_input(KeyCode::Left);
        assert_eq!(input.cursor(), 1);
    }

    #[test]
    fn left_at_zero_stays() {
        let mut input = TextInput::new("X");
        input.handle_input(KeyCode::Left);
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn right_movement() {
        let mut input = TextInput::new("X").with_value("abc");
        input.handle_input(KeyCode::Home);
        input.handle_input(KeyCode::Right);
        assert_eq!(input.cursor(), 1);
    }

    #[test]
    fn right_at_end_stays() {
        let mut input = TextInput::new("X").with_value("ab");
        input.handle_input(KeyCode::Right);
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn home_and_end() {
        let mut input = TextInput::new("X").with_value("hello");
        input.handle_input(KeyCode::Home);
        assert_eq!(input.cursor(), 0);
        input.handle_input(KeyCode::End);
        assert_eq!(input.cursor(), 5);
    }

    #[test]
    fn unknown_key_is_noop() {
        let mut input = TextInput::new("X").with_value("abc");
        input.handle_input(KeyCode::F(1));
        assert_eq!(input.value, "abc");
        assert_eq!(input.cursor(), 3);
    }

    #[test]
    fn default_is_empty() {
        let input = TextInput::default();
        assert_eq!(input.value, "");
        assert_eq!(input.label, "");
        assert_eq!(input.cursor(), 0);
    }
}

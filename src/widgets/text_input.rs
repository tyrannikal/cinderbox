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

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect, widgets::Paragraph};

use crate::ProjectConfig;

pub mod database;
pub mod languages;
pub mod project_type;
pub mod vcs;
pub mod workflows;

pub enum StepResult {
    Continue,
    Done,
    Back,
    Quit,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum Focus {
    #[default]
    Choice,
    SubField(usize),
    Browsing,
}

pub const CURSOR_MARKER: &str = "▸ ";
pub const CURSOR_BLANK: &str = "  ";

pub fn render_choice_line(
    frame: &mut Frame,
    area: Rect,
    label: &impl std::fmt::Display,
    highlighted: bool,
) {
    let marker = if highlighted { CURSOR_MARKER } else { CURSOR_BLANK };
    frame.render_widget(Paragraph::new(format!("{marker}{label}")), area);
}

pub trait StepHandler {
    fn render(&self, frame: &mut Frame, area: Rect);
    fn handle_input(&mut self, key: KeyEvent, config: &mut ProjectConfig) -> StepResult;
    fn planned_actions(&self, config: &ProjectConfig) -> Vec<String>;
    fn execute(&self, config: &ProjectConfig) -> std::io::Result<()>;
    /// True when focus is inside a sub-panel (text input, radio row, browse button, etc.),
    /// rather than on the top-level choice list. Drives the "Back <Esc>" / "Next <Enter>"
    /// variant of the instruction bar.
    fn in_details(&self) -> bool {
        false
    }
    /// True when a choice's sub-panel is currently revealed, regardless of where focus sits.
    /// Drives the "Collapse <←/H>" hint.
    fn is_expanded(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_default_is_choice() {
        assert_eq!(Focus::default(), Focus::Choice);
    }

    #[test]
    fn focus_variants_are_distinct() {
        assert_ne!(Focus::Choice, Focus::SubField(0));
        assert_ne!(Focus::SubField(0), Focus::SubField(1));
        assert_ne!(Focus::Choice, Focus::Browsing);
        assert_ne!(Focus::SubField(0), Focus::Browsing);
    }

    #[test]
    fn focus_copy_and_clone() {
        let f = Focus::SubField(2);
        let f2 = f;
        assert_eq!(f, f2);
    }

    #[test]
    fn cursor_constants_have_same_width() {
        assert_eq!(CURSOR_MARKER.chars().count(), CURSOR_BLANK.chars().count());
    }

    #[test]
    fn cursor_blank_is_spaces() {
        assert!(CURSOR_BLANK.chars().all(|c| c == ' '));
    }
}

use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

use crate::ProjectConfig;

pub mod project_type;
pub mod vcs;

pub enum StepResult {
    Continue,
    Done,
    Back,
    Quit,
}

pub trait StepHandler {
    fn render(&self, frame: &mut Frame, area: Rect);
    fn handle_input(&mut self, key: KeyCode, config: &mut ProjectConfig) -> StepResult;
    fn planned_actions(&self, config: &ProjectConfig) -> Vec<String>;
    fn execute(&self, config: &ProjectConfig) -> std::io::Result<()>;
}

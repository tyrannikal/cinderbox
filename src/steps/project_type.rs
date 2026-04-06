use std::path::PathBuf;

use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::Paragraph,
};

use crate::ProjectConfig;
use crate::widgets::text_input::TextInput;

use super::{StepHandler, StepResult};

#[derive(Debug, Default)]
enum Phase {
    #[default]
    Selecting,
    Details,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum TypeChoice {
    #[default]
    New,
    Existing,
}

const TYPE_CHOICES: [TypeChoice; 2] = [TypeChoice::New, TypeChoice::Existing];

impl std::fmt::Display for TypeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeChoice::New => write!(f, "New"),
            TypeChoice::Existing => write!(f, "Existing"),
        }
    }
}

#[derive(Debug)]
pub struct ProjectTypeHandler {
    phase: Phase,
    cursor: usize,
    choice: TypeChoice,
    name_input: TextInput,
    location_input: TextInput,
    active_field: usize, // 0 = name, 1 = location
    validation_msg: String,
}

impl Default for ProjectTypeHandler {
    fn default() -> Self {
        let cwd = std::env::current_dir()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        Self {
            phase: Phase::Selecting,
            cursor: 0,
            choice: TypeChoice::New,
            name_input: TextInput::new("Project Name"),
            location_input: TextInput::new("Location").with_value(cwd),
            active_field: 0,
            validation_msg: String::new(),
        }
    }
}

impl ProjectTypeHandler {
    pub fn in_details(&self) -> bool {
        matches!(self.phase, Phase::Details)
    }

    /// Restore cursor position from existing config when navigating back
    pub fn restore_from_config(&mut self, config: &ProjectConfig) {
        if let Some(pt) = &config.project_type {
            let choice = match pt {
                crate::ProjectType::New => TypeChoice::New,
                crate::ProjectType::Existing => TypeChoice::Existing,
            };
            self.cursor = TYPE_CHOICES.iter().position(|c| *c == choice).unwrap_or(0);
            self.choice = choice;

            // If we have a name/location already, go back to details phase
            if !config.project_name.is_empty() {
                self.phase = Phase::Details;
                match choice {
                    TypeChoice::New => {
                        self.name_input =
                            TextInput::new("Project Name").with_value(&config.project_name);
                        self.location_input =
                            TextInput::new("Location").with_value(&config.project_location);
                    }
                    TypeChoice::Existing => {
                        self.location_input =
                            TextInput::new("Location").with_value(&config.project_location);
                    }
                }
            }
        }
    }

    fn full_path(&self) -> PathBuf {
        match self.choice {
            TypeChoice::New => {
                PathBuf::from(&self.location_input.value).join(&self.name_input.value)
            }
            TypeChoice::Existing => PathBuf::from(&self.location_input.value),
        }
    }

    fn derived_name(&self) -> String {
        let path = PathBuf::from(&self.location_input.value);
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    fn validate(&mut self) {
        let path = self.full_path();

        match self.choice {
            TypeChoice::New => {
                let name = &self.name_input.value;
                let location = &self.location_input.value;
                if name.is_empty() {
                    self.validation_msg = "Project name is required.".to_string();
                } else if path.exists() {
                    self.validation_msg =
                        format!("Warning: {} already exists.", path.display());
                } else if !PathBuf::from(location).exists() {
                    self.validation_msg =
                        format!("Warning: parent {} does not exist.", location);
                } else {
                    self.validation_msg = format!("Will create: {}", path.display());
                }
            }
            TypeChoice::Existing => {
                let location = &self.location_input.value;
                if location.is_empty() {
                    self.validation_msg = "Location is required.".to_string();
                } else if !path.exists() {
                    self.validation_msg =
                        format!("Warning: {} does not exist.", path.display());
                } else {
                    self.validation_msg = format!("Will use: {}", path.display());
                }
            }
        }
    }

    fn is_valid(&self) -> bool {
        match self.choice {
            TypeChoice::New => {
                !self.name_input.value.is_empty()
                    && !self.full_path().exists()
                    && PathBuf::from(&self.location_input.value).exists()
            }
            TypeChoice::Existing => {
                !self.location_input.value.is_empty() && self.full_path().exists()
            }
        }
    }

    fn render_selecting(&self, frame: &mut Frame, area: Rect) {
        let text: String = TYPE_CHOICES
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let marker = if i == self.cursor { "▸ " } else { "  " };
                format!("{marker}{c}")
            })
            .collect::<Vec<_>>()
            .join("\n");

        frame.render_widget(Paragraph::new(text), area);
    }

    fn render_details(&self, frame: &mut Frame, area: Rect) {
        match self.choice {
            TypeChoice::New => {
                let [name_area, location_area, _spacer, msg_area] = Layout::vertical([
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(1),
                    Constraint::Min(1),
                ])
                .areas(area);

                self.name_input
                    .render(frame, name_area, self.active_field == 0);
                self.location_input
                    .render(frame, location_area, self.active_field == 1);

                if !self.validation_msg.is_empty() {
                    let style = if self.validation_msg.starts_with("Warning") {
                        ratatui::style::Style::default().yellow()
                    } else {
                        ratatui::style::Style::default().green()
                    };
                    frame.render_widget(
                        Paragraph::new(Line::from(self.validation_msg.as_str()).style(style)),
                        msg_area,
                    );
                }
            }
            TypeChoice::Existing => {
                let [location_area, _spacer, msg_area] = Layout::vertical([
                    Constraint::Length(3),
                    Constraint::Length(1),
                    Constraint::Min(1),
                ])
                .areas(area);

                self.location_input.render(frame, location_area, true);

                if !self.validation_msg.is_empty() {
                    let style = if self.validation_msg.starts_with("Warning") {
                        ratatui::style::Style::default().yellow()
                    } else {
                        ratatui::style::Style::default().green()
                    };
                    frame.render_widget(
                        Paragraph::new(Line::from(self.validation_msg.as_str()).style(style)),
                        msg_area,
                    );
                }
            }
        }
    }

    fn handle_selecting(&mut self, key: KeyCode) -> StepResult {
        match key {
            KeyCode::Char('q') => StepResult::Quit,
            KeyCode::Down | KeyCode::Char('j') => {
                if self.cursor + 1 < TYPE_CHOICES.len() {
                    self.cursor += 1;
                }
                StepResult::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                StepResult::Continue
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                self.choice = TYPE_CHOICES[self.cursor];
                self.phase = Phase::Details;
                self.validate();
                StepResult::Continue
            }
            KeyCode::Left | KeyCode::Char('h') => StepResult::Back,
            _ => StepResult::Continue,
        }
    }

    fn handle_details(&mut self, key: KeyCode, config: &mut ProjectConfig) -> StepResult {
        match self.choice {
            TypeChoice::New => self.handle_details_new(key, config),
            TypeChoice::Existing => self.handle_details_existing(key, config),
        }
    }

    fn handle_details_new(&mut self, key: KeyCode, config: &mut ProjectConfig) -> StepResult {
        match key {
            KeyCode::Tab | KeyCode::Down => {
                self.active_field = (self.active_field + 1) % 2;
                StepResult::Continue
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.active_field = if self.active_field == 0 { 1 } else { 0 };
                StepResult::Continue
            }
            KeyCode::Enter => {
                if self.is_valid() {
                    config.project_type = Some(crate::ProjectType::New);
                    config.project_name = self.name_input.value.clone();
                    config.project_location = self.location_input.value.clone();
                    StepResult::Done
                } else {
                    self.validate();
                    StepResult::Continue
                }
            }
            KeyCode::Esc => {
                self.phase = Phase::Selecting;
                StepResult::Continue
            }
            key => {
                let input = if self.active_field == 0 {
                    &mut self.name_input
                } else {
                    &mut self.location_input
                };
                input.handle_input(key);
                self.validate();
                StepResult::Continue
            }
        }
    }

    fn handle_details_existing(&mut self, key: KeyCode, config: &mut ProjectConfig) -> StepResult {
        match key {
            KeyCode::Enter => {
                if self.is_valid() {
                    config.project_type = Some(crate::ProjectType::Existing);
                    config.project_name = self.derived_name();
                    config.project_location = self.location_input.value.clone();
                    StepResult::Done
                } else {
                    self.validate();
                    StepResult::Continue
                }
            }
            KeyCode::Esc => {
                self.phase = Phase::Selecting;
                StepResult::Continue
            }
            key => {
                self.location_input.handle_input(key);
                self.validate();
                StepResult::Continue
            }
        }
    }
}

impl StepHandler for ProjectTypeHandler {
    fn render(&self, frame: &mut Frame, area: Rect) {
        match self.phase {
            Phase::Selecting => self.render_selecting(frame, area),
            Phase::Details => self.render_details(frame, area),
        }
    }

    fn handle_input(&mut self, key: KeyCode, config: &mut ProjectConfig) -> StepResult {
        match self.phase {
            Phase::Selecting => self.handle_selecting(key),
            Phase::Details => self.handle_details(key, config),
        }
    }

    fn planned_actions(&self, config: &ProjectConfig) -> Vec<String> {
        match config.project_type {
            Some(crate::ProjectType::New) => {
                if config.project_name.is_empty() {
                    return vec![];
                }
                let path =
                    PathBuf::from(&config.project_location).join(&config.project_name);
                vec![format!("Create directory: {}", path.display())]
            }
            Some(crate::ProjectType::Existing) => {
                if config.project_location.is_empty() {
                    return vec![];
                }
                vec![format!(
                    "Use existing directory: {}",
                    config.project_location
                )]
            }
            None => vec![],
        }
    }

    fn execute(&self, config: &ProjectConfig) -> std::io::Result<()> {
        if let Some(crate::ProjectType::New) = config.project_type {
            if !config.project_name.is_empty() {
                let path =
                    PathBuf::from(&config.project_location).join(&config.project_name);
                std::fs::create_dir_all(&path)?;
            }
        }
        Ok(())
    }
}

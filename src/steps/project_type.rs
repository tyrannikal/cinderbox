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

#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum Focus {
    #[default]
    Choice,
    SubField(usize),
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
    focus: Focus,
    expanded: Option<TypeChoice>,
    choice_cursor: usize,
    name_input: TextInput,
    location_input: TextInput,
    validation_msg: String,
}

impl Default for ProjectTypeHandler {
    fn default() -> Self {
        let cwd = std::env::current_dir()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        Self {
            focus: Focus::Choice,
            expanded: None,
            choice_cursor: 0,
            name_input: TextInput::new("Project Name"),
            location_input: TextInput::new("Location").with_value(cwd),
            validation_msg: String::new(),
        }
    }
}

impl ProjectTypeHandler {
    pub fn in_details(&self) -> bool {
        matches!(self.focus, Focus::SubField(_))
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded.is_some()
    }

    /// Restore cursor position from existing config when navigating back
    pub fn restore_from_config(&mut self, config: &ProjectConfig) {
        if let Some(pt) = &config.project_type {
            let choice = match pt {
                crate::ProjectType::New => TypeChoice::New,
                crate::ProjectType::Existing => TypeChoice::Existing,
            };
            self.choice_cursor = TYPE_CHOICES.iter().position(|c| *c == choice).unwrap_or(0);

            if !config.project_name.is_empty() {
                self.expanded = Some(choice);
                self.focus = Focus::SubField(0);
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
                self.validate();
            }
        }
    }

    fn full_path(&self) -> PathBuf {
        match self.expanded {
            Some(TypeChoice::New) => {
                PathBuf::from(&self.location_input.value).join(&self.name_input.value)
            }
            _ => PathBuf::from(&self.location_input.value),
        }
    }

    fn derived_name(&self) -> String {
        let path = PathBuf::from(&self.location_input.value);
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    fn validate(&mut self) {
        let Some(choice) = self.expanded else {
            self.validation_msg.clear();
            return;
        };
        let path = self.full_path();

        match choice {
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
        let Some(choice) = self.expanded else {
            return false;
        };
        match choice {
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

    fn max_subfield(&self) -> usize {
        match self.expanded {
            Some(TypeChoice::New) => 1, // 0=name, 1=location
            Some(TypeChoice::Existing) => 0, // 0=location
            None => 0,
        }
    }

    fn render_choice_line(
        &self,
        frame: &mut Frame,
        area: Rect,
        choice: TypeChoice,
        index: usize,
    ) {
        let is_highlighted = matches!(self.focus, Focus::Choice) && self.choice_cursor == index;
        let marker = if is_highlighted { "▸ " } else { "  " };
        let text = format!("{marker}{choice}");
        frame.render_widget(Paragraph::new(text), area);
    }

    fn render_validation(&self, frame: &mut Frame, area: Rect) {
        if !self.validation_msg.is_empty() {
            let style = if self.validation_msg.starts_with("Warning") {
                ratatui::style::Style::default().yellow()
            } else {
                ratatui::style::Style::default().green()
            };
            frame.render_widget(
                Paragraph::new(Line::from(self.validation_msg.as_str()).style(style)),
                area,
            );
        }
    }

    fn handle_choice(&mut self, key: KeyCode) -> StepResult {
        match key {
            KeyCode::Char('q') => StepResult::Quit,
            KeyCode::Down | KeyCode::Char('j') => {
                if self.choice_cursor + 1 < TYPE_CHOICES.len() {
                    self.choice_cursor += 1;
                }
                StepResult::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.choice_cursor = self.choice_cursor.saturating_sub(1);
                StepResult::Continue
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                let choice = TYPE_CHOICES[self.choice_cursor];
                self.expanded = Some(choice);
                self.focus = Focus::SubField(0);
                self.validate();
                StepResult::Continue
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.expanded.is_some() {
                    self.expanded = None;
                    self.validation_msg.clear();
                    StepResult::Continue
                } else {
                    StepResult::Back
                }
            }
            _ => StepResult::Continue,
        }
    }

    fn handle_subfield(
        &mut self,
        key: KeyCode,
        field: usize,
        config: &mut ProjectConfig,
    ) -> StepResult {
        let choice = self.expanded.unwrap(); // safe: SubField only reachable when expanded

        match key {
            KeyCode::Up => {
                if field == 0 {
                    self.focus = Focus::Choice;
                } else {
                    self.focus = Focus::SubField(field - 1);
                }
                StepResult::Continue
            }
            KeyCode::Down => {
                if field < self.max_subfield() {
                    self.focus = Focus::SubField(field + 1);
                } else if choice == TypeChoice::New {
                    // Move past New's fields to highlight Existing
                    self.choice_cursor = 1;
                    self.focus = Focus::Choice;
                }
                StepResult::Continue
            }
            KeyCode::Tab => {
                if field < self.max_subfield() {
                    self.focus = Focus::SubField(field + 1);
                } else {
                    self.focus = Focus::SubField(0);
                }
                StepResult::Continue
            }
            KeyCode::BackTab => {
                if field > 0 {
                    self.focus = Focus::SubField(field - 1);
                } else {
                    self.focus = Focus::SubField(self.max_subfield());
                }
                StepResult::Continue
            }
            KeyCode::Enter => {
                if self.is_valid() {
                    match choice {
                        TypeChoice::New => {
                            config.project_type = Some(crate::ProjectType::New);
                            config.project_name = self.name_input.value.clone();
                            config.project_location = self.location_input.value.clone();
                        }
                        TypeChoice::Existing => {
                            config.project_type = Some(crate::ProjectType::Existing);
                            config.project_name = self.derived_name();
                            config.project_location = self.location_input.value.clone();
                        }
                    }
                    StepResult::Done
                } else {
                    self.validate();
                    StepResult::Continue
                }
            }
            KeyCode::Esc => {
                self.focus = Focus::Choice;
                StepResult::Continue
            }
            key => {
                let input = match choice {
                    TypeChoice::New => {
                        if field == 0 {
                            &mut self.name_input
                        } else {
                            &mut self.location_input
                        }
                    }
                    TypeChoice::Existing => &mut self.location_input,
                };
                input.handle_input(key);
                self.validate();
                StepResult::Continue
            }
        }
    }
}

impl StepHandler for ProjectTypeHandler {
    fn render(&self, frame: &mut Frame, area: Rect) {
        // Build layout constraints dynamically based on expanded state
        let mut constraints: Vec<Constraint> = vec![];

        // "New" label
        constraints.push(Constraint::Length(1));
        // New's sub-fields if expanded
        if self.expanded == Some(TypeChoice::New) {
            constraints.push(Constraint::Length(3)); // Name input
            constraints.push(Constraint::Length(3)); // Location input
        }
        // "Existing" label
        constraints.push(Constraint::Length(1));
        // Existing's sub-field if expanded
        if self.expanded == Some(TypeChoice::Existing) {
            constraints.push(Constraint::Length(3)); // Location input
        }
        // Spacer + validation message
        constraints.push(Constraint::Length(1));
        constraints.push(Constraint::Min(1));

        let areas: Vec<Rect> = Layout::vertical(constraints).split(area).to_vec();
        let mut idx = 0;

        // Render "New" choice line
        self.render_choice_line(frame, areas[idx], TypeChoice::New, 0);
        idx += 1;

        // Render New's sub-fields if expanded
        if self.expanded == Some(TypeChoice::New) {
            let indent = 4u16;
            let name_area = Rect {
                x: areas[idx].x + indent,
                width: areas[idx].width.saturating_sub(indent),
                ..areas[idx]
            };
            self.name_input
                .render(frame, name_area, matches!(self.focus, Focus::SubField(0)));
            idx += 1;

            let loc_area = Rect {
                x: areas[idx].x + indent,
                width: areas[idx].width.saturating_sub(indent),
                ..areas[idx]
            };
            self.location_input
                .render(frame, loc_area, matches!(self.focus, Focus::SubField(1)));
            idx += 1;
        }

        // Render "Existing" choice line
        self.render_choice_line(frame, areas[idx], TypeChoice::Existing, 1);
        idx += 1;

        // Render Existing's sub-field if expanded
        if self.expanded == Some(TypeChoice::Existing) {
            let indent = 4u16;
            let loc_area = Rect {
                x: areas[idx].x + indent,
                width: areas[idx].width.saturating_sub(indent),
                ..areas[idx]
            };
            self.location_input
                .render(frame, loc_area, matches!(self.focus, Focus::SubField(0)));
            idx += 1;
        }

        // Skip spacer
        idx += 1;

        // Render validation message
        self.render_validation(frame, areas[idx]);
    }

    fn handle_input(&mut self, key: KeyCode, config: &mut ProjectConfig) -> StepResult {
        match self.focus {
            Focus::Choice => self.handle_choice(key),
            Focus::SubField(n) => self.handle_subfield(key, n, config),
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
        if let Some(crate::ProjectType::New) = config.project_type
            && !config.project_name.is_empty()
        {
            let path = PathBuf::from(&config.project_location).join(&config.project_name);
            std::fs::create_dir_all(&path)?;
        }
        Ok(())
    }
}

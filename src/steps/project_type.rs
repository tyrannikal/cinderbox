use std::path::PathBuf;

use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::Line,
    widgets::{Block, Clear, FrameExt, Paragraph},
};
use ratatui_explorer::FileExplorer;
use strum::Display;

use crate::ProjectConfig;
use crate::widgets::text_input::TextInput;

use super::{CURSOR_MARKER, Focus, StepHandler, StepResult, render_choice_line};

#[derive(Debug, Default, Clone, Copy, PartialEq, Display)]
enum TypeChoice {
    #[default]
    New,
    Existing,
}

const TYPE_CHOICES: [TypeChoice; 2] = [TypeChoice::New, TypeChoice::Existing];

impl std::fmt::Debug for ProjectTypeHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectTypeHandler")
            .field("focus", &self.focus)
            .field("expanded", &self.expanded)
            .field("choice_cursor", &self.choice_cursor)
            .field("file_explorer", &self.file_explorer.as_ref().map(|_| ".."))
            .finish()
    }
}

pub struct ProjectTypeHandler {
    focus: Focus,
    expanded: Option<TypeChoice>,
    choice_cursor: usize,
    name_input: TextInput,
    location_input: TextInput,
    validation_msg: String,
    file_explorer: Option<FileExplorer>,
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
            file_explorer: None,
        }
    }
}

impl ProjectTypeHandler {
    pub fn in_details(&self) -> bool {
        matches!(self.focus, Focus::SubField(_) | Focus::Browsing)
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
                        self.name_input.set_value(&config.project_name);
                        self.location_input.set_value(&config.project_location);
                    }
                    TypeChoice::Existing => {
                        self.location_input.set_value(&config.project_location);
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
            Some(TypeChoice::New) => 2,      // 0=name, 1=location, 2=browse
            Some(TypeChoice::Existing) => 1, // 0=location, 1=browse
            None => 0,
        }
    }

    /// Returns the sub-field index of the browse button for the current choice.
    fn browse_subfield(&self) -> usize {
        self.max_subfield()
    }

    /// Returns the sub-field index of the location input for the current choice.
    fn location_subfield(&self) -> usize {
        match self.expanded {
            Some(TypeChoice::New) => 1,
            _ => 0,
        }
    }

    fn render_browse_button(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let style = if focused {
            Style::default().fg(Color::Black).bg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        frame.render_widget(Paragraph::new(Line::from("[ Browse ]").style(style)), area);
    }

    fn render_file_explorer(&self, frame: &mut Frame, area: Rect) {
        let Some(ref explorer) = self.file_explorer else {
            return;
        };

        // Center the explorer as an overlay, taking up most of the wizard area
        let vertical_margin = 1;
        let horizontal_margin = 2;
        let overlay = Rect {
            x: area.x + horizontal_margin,
            y: area.y + vertical_margin,
            width: area.width.saturating_sub(horizontal_margin * 2),
            height: area.height.saturating_sub(vertical_margin * 2),
        };

        // Clear the area behind the overlay
        frame.render_widget(Clear, overlay);
        frame.render_widget_ref(explorer.widget(), overlay);
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
                // If on the browse button, open the file explorer
                if field == self.browse_subfield() {
                    return self.open_file_explorer();
                }
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
            KeyCode::Char(' ') if field == self.browse_subfield() => self.open_file_explorer(),
            KeyCode::Esc => {
                self.focus = Focus::Choice;
                StepResult::Continue
            }
            key => {
                // Browse button doesn't accept text input — only Enter/Space activate it
                if field == self.browse_subfield() {
                    return StepResult::Continue;
                }
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

    fn open_file_explorer(&mut self) -> StepResult {
        let mut explorer = match FileExplorer::new() {
            Ok(e) => e,
            Err(_) => return StepResult::Continue,
        };

        // Start in the location input's directory if it's valid
        let start_dir = PathBuf::from(&self.location_input.value);
        if start_dir.is_dir() {
            let _ = explorer.set_cwd(&start_dir);
        }

        // Only show directories
        explorer
            .set_filter_map(|file| if file.is_dir { Some(file) } else { None })
            .ok();

        // Style the explorer with a border
        let theme = ratatui_explorer::Theme::default()
            .with_block(
                Block::bordered()
                    .title(" Browse ")
                    .title_bottom(Line::from(vec![
                        " Select ".into(),
                        "<Space> ".blue().bold(),
                        " Cancel ".into(),
                        "<Esc> ".blue().bold(),
                    ])),
            )
            .with_highlight_symbol(CURSOR_MARKER);
        explorer.set_theme(theme);

        self.file_explorer = Some(explorer);
        self.focus = Focus::Browsing;
        StepResult::Continue
    }

    fn handle_browsing(&mut self, key: KeyEvent) -> StepResult {
        let Some(ref mut explorer) = self.file_explorer else {
            self.focus = Focus::SubField(self.browse_subfield());
            return StepResult::Continue;
        };

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                // Close explorer without selecting
                self.file_explorer = None;
                self.focus = Focus::SubField(self.browse_subfield());
            }
            KeyCode::Char(' ') => {
                // Space confirms: select the explorer's current working directory
                let selected = explorer.cwd().to_string_lossy().to_string();
                self.file_explorer = None;
                self.focus = Focus::SubField(self.location_subfield());
                self.location_input = TextInput::new("Location").with_value(selected);
                self.validate();
            }
            _ => {
                // All other keys (Enter, j/k/h/l, arrows, etc.) are passed to the explorer
                let _ = explorer.handle(&Event::Key(key));
            }
        }

        StepResult::Continue
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
            constraints.push(Constraint::Length(1)); // Browse button
        }
        // "Existing" label
        constraints.push(Constraint::Length(1));
        // Existing's sub-field if expanded
        if self.expanded == Some(TypeChoice::Existing) {
            constraints.push(Constraint::Length(3)); // Location input
            constraints.push(Constraint::Length(1)); // Browse button
        }
        // Spacer + validation message
        constraints.push(Constraint::Length(1));
        constraints.push(Constraint::Min(1));

        let areas: Vec<Rect> = Layout::vertical(constraints).split(area).to_vec();
        let mut idx = 0;

        // Render "New" choice line
        let highlighted = matches!(self.focus, Focus::Choice) && self.choice_cursor == 0;
        render_choice_line(frame, areas[idx], &TypeChoice::New, highlighted);
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

            let browse_area = Rect {
                x: areas[idx].x + indent,
                width: areas[idx].width.saturating_sub(indent),
                ..areas[idx]
            };
            self.render_browse_button(frame, browse_area, matches!(self.focus, Focus::SubField(2)));
            idx += 1;
        }

        // Render "Existing" choice line
        let highlighted = matches!(self.focus, Focus::Choice) && self.choice_cursor == 1;
        render_choice_line(frame, areas[idx], &TypeChoice::Existing, highlighted);
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

            let browse_area = Rect {
                x: areas[idx].x + indent,
                width: areas[idx].width.saturating_sub(indent),
                ..areas[idx]
            };
            self.render_browse_button(frame, browse_area, matches!(self.focus, Focus::SubField(1)));
            idx += 1;
        }

        // Skip spacer
        idx += 1;

        // Render validation message
        self.render_validation(frame, areas[idx]);

        // Render file explorer overlay on top if browsing
        if self.focus == Focus::Browsing {
            self.render_file_explorer(frame, area);
        }
    }

    fn handle_input(&mut self, key: KeyEvent, config: &mut ProjectConfig) -> StepResult {
        match self.focus {
            Focus::Choice => self.handle_choice(key.code),
            Focus::SubField(n) => self.handle_subfield(key.code, n, config),
            Focus::Browsing => self.handle_browsing(key),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    // --- Default state ---

    #[test]
    fn default_state() {
        let h = ProjectTypeHandler::default();
        assert_eq!(h.focus, Focus::Choice);
        assert!(h.expanded.is_none());
        assert_eq!(h.choice_cursor, 0);
        assert!(!h.in_details());
        assert!(!h.is_expanded());
    }

    // --- Choice navigation ---

    #[test]
    fn choice_navigation_down_up() {
        let mut h = ProjectTypeHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Down), &mut c);
        assert_eq!(h.choice_cursor, 1);
        h.handle_input(key(KeyCode::Down), &mut c);
        assert_eq!(h.choice_cursor, 1); // clamped at max
        h.handle_input(key(KeyCode::Up), &mut c);
        assert_eq!(h.choice_cursor, 0);
        h.handle_input(key(KeyCode::Up), &mut c);
        assert_eq!(h.choice_cursor, 0); // clamped at 0
    }

    // --- Expanding choices ---

    #[test]
    fn enter_expands_new() {
        let mut h = ProjectTypeHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.expanded, Some(TypeChoice::New));
        assert_eq!(h.focus, Focus::SubField(0));
        assert!(h.in_details());
        assert!(h.is_expanded());
    }

    #[test]
    fn enter_expands_existing() {
        let mut h = ProjectTypeHandler::default();
        let mut c = ProjectConfig::default();
        h.choice_cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.expanded, Some(TypeChoice::Existing));
        assert_eq!(h.focus, Focus::SubField(0));
    }

    // --- max_subfield ---

    #[test]
    fn max_subfield_new_is_2() {
        let h = ProjectTypeHandler {
            expanded: Some(TypeChoice::New),
            ..Default::default()
        };
        assert_eq!(h.max_subfield(), 2);
    }

    #[test]
    fn max_subfield_existing_is_1() {
        let h = ProjectTypeHandler {
            expanded: Some(TypeChoice::Existing),
            ..Default::default()
        };
        assert_eq!(h.max_subfield(), 1);
    }

    // --- Esc from subfield returns to choice ---

    #[test]
    fn esc_returns_to_choice() {
        let mut h = ProjectTypeHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.focus, Focus::SubField(0));
        h.handle_input(key(KeyCode::Esc), &mut c);
        assert_eq!(h.focus, Focus::Choice);
    }

    // --- Left collapses expanded, then goes back ---

    #[test]
    fn left_collapses_then_backs() {
        let mut h = ProjectTypeHandler::default();
        let mut c = ProjectConfig::default();
        // Expand
        h.handle_input(key(KeyCode::Enter), &mut c);
        h.focus = Focus::Choice; // simulate being back at choice level
        let result = h.handle_input(key(KeyCode::Left), &mut c);
        assert!(h.expanded.is_none());
        assert!(matches!(result, StepResult::Continue));
        let result = h.handle_input(key(KeyCode::Left), &mut c);
        assert!(matches!(result, StepResult::Back));
    }

    // --- Quit ---

    #[test]
    fn q_quits() {
        let mut h = ProjectTypeHandler::default();
        let mut c = ProjectConfig::default();
        let result = h.handle_input(key(KeyCode::Char('q')), &mut c);
        assert!(matches!(result, StepResult::Quit));
    }

    // --- Tab cycling in New (3 subfields: name, location, browse) ---

    #[test]
    fn tab_cycles_new_subfields() {
        let mut h = ProjectTypeHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Enter), &mut c); // expand New
        assert_eq!(h.focus, Focus::SubField(0));
        h.handle_input(key(KeyCode::Tab), &mut c);
        assert_eq!(h.focus, Focus::SubField(1));
        h.handle_input(key(KeyCode::Tab), &mut c);
        assert_eq!(h.focus, Focus::SubField(2));
        h.handle_input(key(KeyCode::Tab), &mut c);
        assert_eq!(h.focus, Focus::SubField(0)); // wraps
    }

    #[test]
    fn backtab_cycles_new_subfields() {
        let mut h = ProjectTypeHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Enter), &mut c);
        assert_eq!(h.focus, Focus::SubField(0));
        h.handle_input(key(KeyCode::BackTab), &mut c);
        assert_eq!(h.focus, Focus::SubField(2)); // wraps to browse
    }

    // --- Text input forwarding ---

    #[test]
    fn typing_in_name_subfield() {
        let mut h = ProjectTypeHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Enter), &mut c); // expand New, focus name
        h.handle_input(key(KeyCode::Char('a')), &mut c);
        h.handle_input(key(KeyCode::Char('b')), &mut c);
        assert_eq!(h.name_input.value, "ab");
    }

    #[test]
    fn typing_in_location_subfield_new() {
        let mut h = ProjectTypeHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Enter), &mut c);
        h.handle_input(key(KeyCode::Tab), &mut c); // move to location
        // Clear existing location value first
        h.location_input = TextInput::new("Location");
        h.handle_input(key(KeyCode::Char('/')), &mut c);
        assert!(h.location_input.value.contains('/'));
    }

    // --- planned_actions ---

    #[test]
    fn planned_actions_new_project() {
        let h = ProjectTypeHandler::default();
        let c = ProjectConfig {
            project_type: Some(crate::ProjectType::New),
            project_name: "myapp".to_string(),
            project_location: "/home/user".to_string(),
            ..Default::default()
        };
        let actions = h.planned_actions(&c);
        assert_eq!(actions.len(), 1);
        assert!(actions[0].contains("/home/user/myapp"));
    }

    #[test]
    fn planned_actions_existing_project() {
        let h = ProjectTypeHandler::default();
        let c = ProjectConfig {
            project_type: Some(crate::ProjectType::Existing),
            project_location: "/home/user/existing".to_string(),
            ..Default::default()
        };
        let actions = h.planned_actions(&c);
        assert_eq!(actions.len(), 1);
        assert!(actions[0].contains("/home/user/existing"));
    }

    #[test]
    fn planned_actions_empty_name() {
        let h = ProjectTypeHandler::default();
        let c = ProjectConfig {
            project_type: Some(crate::ProjectType::New),
            project_name: String::new(),
            ..Default::default()
        };
        assert!(h.planned_actions(&c).is_empty());
    }

    #[test]
    fn planned_actions_none() {
        let h = ProjectTypeHandler::default();
        let c = ProjectConfig::default();
        assert!(h.planned_actions(&c).is_empty());
    }

    // --- Validation (New project) ---

    #[test]
    fn validate_new_requires_name() {
        let mut h = ProjectTypeHandler {
            expanded: Some(TypeChoice::New),
            name_input: TextInput::new("Name"),
            location_input: TextInput::new("Location").with_value("/tmp"),
            ..Default::default()
        };
        h.validate();
        assert!(h.validation_msg.contains("required"));
    }

    #[test]
    fn validate_new_warns_missing_parent() {
        let mut h = ProjectTypeHandler {
            expanded: Some(TypeChoice::New),
            name_input: TextInput::new("Name").with_value("proj"),
            location_input: TextInput::new("Location").with_value("/nonexistent_xyz_path_42"),
            ..Default::default()
        };
        h.validate();
        assert!(h.validation_msg.contains("Warning"));
    }

    #[test]
    fn validate_new_shows_will_create() {
        let mut h = ProjectTypeHandler {
            expanded: Some(TypeChoice::New),
            name_input: TextInput::new("Name").with_value("new_test_project_xyz"),
            location_input: TextInput::new("Location").with_value("/tmp"),
            ..Default::default()
        };
        h.validate();
        assert!(h.validation_msg.contains("Will create"));
    }

    // --- Validation (Existing project) ---

    #[test]
    fn validate_existing_requires_location() {
        let mut h = ProjectTypeHandler {
            expanded: Some(TypeChoice::Existing),
            location_input: TextInput::new("Location"),
            ..Default::default()
        };
        h.validate();
        assert!(h.validation_msg.contains("required"));
    }

    #[test]
    fn validate_existing_warns_nonexistent() {
        let mut h = ProjectTypeHandler {
            expanded: Some(TypeChoice::Existing),
            location_input: TextInput::new("Location").with_value("/nonexistent_xyz_path_42"),
            ..Default::default()
        };
        h.validate();
        assert!(h.validation_msg.contains("Warning"));
    }

    #[test]
    fn validate_existing_shows_will_use() {
        let mut h = ProjectTypeHandler {
            expanded: Some(TypeChoice::Existing),
            location_input: TextInput::new("Location").with_value("/tmp"),
            ..Default::default()
        };
        h.validate();
        assert!(h.validation_msg.contains("Will use"));
    }

    // --- is_valid ---

    #[test]
    fn is_valid_false_when_no_expansion() {
        let h = ProjectTypeHandler::default();
        assert!(!h.is_valid());
    }

    #[test]
    fn is_valid_new_with_valid_path() {
        let h = ProjectTypeHandler {
            expanded: Some(TypeChoice::New),
            name_input: TextInput::new("Name").with_value("unique_test_proj_abc"),
            location_input: TextInput::new("Location").with_value("/tmp"),
            ..Default::default()
        };
        assert!(h.is_valid());
    }

    #[test]
    fn is_valid_existing_with_real_path() {
        let h = ProjectTypeHandler {
            expanded: Some(TypeChoice::Existing),
            location_input: TextInput::new("Location").with_value("/tmp"),
            ..Default::default()
        };
        assert!(h.is_valid());
    }

    // --- derived_name ---

    #[test]
    fn derived_name_from_path() {
        let h = ProjectTypeHandler {
            location_input: TextInput::new("Location").with_value("/home/user/myproject"),
            ..Default::default()
        };
        assert_eq!(h.derived_name(), "myproject");
    }

    // --- Enter on valid New commits to config ---

    #[test]
    fn enter_commits_valid_new_project() {
        let mut h = ProjectTypeHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Enter), &mut c); // expand New
        h.name_input = TextInput::new("Name").with_value("unique_test_proj_def");
        h.location_input = TextInput::new("Location").with_value("/tmp");
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert_eq!(c.project_type, Some(crate::ProjectType::New));
        assert_eq!(c.project_name, "unique_test_proj_def");
        assert_eq!(c.project_location, "/tmp");
    }

    #[test]
    fn enter_commits_valid_existing_project() {
        let mut h = ProjectTypeHandler::default();
        let mut c = ProjectConfig::default();
        h.choice_cursor = 1;
        h.handle_input(key(KeyCode::Enter), &mut c); // expand Existing
        h.location_input = TextInput::new("Location").with_value("/tmp");
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Done));
        assert_eq!(c.project_type, Some(crate::ProjectType::Existing));
        assert_eq!(c.project_name, "tmp");
        assert_eq!(c.project_location, "/tmp");
    }

    // --- Enter on invalid stays ---

    #[test]
    fn enter_on_invalid_continues() {
        let mut h = ProjectTypeHandler::default();
        let mut c = ProjectConfig::default();
        h.handle_input(key(KeyCode::Enter), &mut c); // expand New
        h.name_input = TextInput::new("Name"); // empty name = invalid
        let result = h.handle_input(key(KeyCode::Enter), &mut c);
        assert!(matches!(result, StepResult::Continue));
    }

    // --- execute creates directory for New ---

    #[test]
    fn execute_creates_dir_for_new() {
        let dir = std::env::temp_dir().join("cinderbox_test_execute_new");
        let _ = std::fs::remove_dir_all(&dir);

        let h = ProjectTypeHandler::default();
        let c = ProjectConfig {
            project_type: Some(crate::ProjectType::New),
            project_name: "subdir".to_string(),
            project_location: dir.to_string_lossy().to_string(),
            ..Default::default()
        };
        h.execute(&c).unwrap();
        assert!(dir.join("subdir").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn execute_noop_for_existing() {
        let h = ProjectTypeHandler::default();
        let c = ProjectConfig {
            project_type: Some(crate::ProjectType::Existing),
            project_location: "/tmp".to_string(),
            ..Default::default()
        };
        assert!(h.execute(&c).is_ok());
    }
}

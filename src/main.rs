use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    DefaultTerminal, Frame,
    style::Stylize,
    text::Line,
    widgets::{Block, Paragraph},
};

fn main() -> io::Result<()> {
    ratatui::run(|terminal| App::default().run(terminal))
}

#[derive(Debug, Default)]
enum WizardStep {
    #[default]
    ProjectType,
    Vcs,
    Languages,
    Database,
    Remotes,
    Extras,
    Summary,
}

impl WizardStep {
    fn title(&self) -> &str {
        match self {
            WizardStep::ProjectType => "Project Type",
            WizardStep::Vcs => "Version Control",
            WizardStep::Languages => "Languages",
            WizardStep::Database => "Database",
            WizardStep::Remotes => "Remotes",
            WizardStep::Extras => "Extras",
            WizardStep::Summary => "Summary",
        }
    }

    fn next(&self) -> Option<WizardStep> {
        match self {
            WizardStep::ProjectType => Some(WizardStep::Vcs),
            WizardStep::Vcs => Some(WizardStep::Languages),
            WizardStep::Languages => Some(WizardStep::Database),
            WizardStep::Database => Some(WizardStep::Remotes),
            WizardStep::Remotes => Some(WizardStep::Extras),
            WizardStep::Extras => Some(WizardStep::Summary),
            WizardStep::Summary => None,
        }
    }

    fn prev(&self) -> Option<WizardStep> {
        match self {
            WizardStep::ProjectType => None,
            WizardStep::Vcs => Some(WizardStep::ProjectType),
            WizardStep::Languages => Some(WizardStep::Vcs),
            WizardStep::Database => Some(WizardStep::Languages),
            WizardStep::Remotes => Some(WizardStep::Database),
            WizardStep::Extras => Some(WizardStep::Remotes),
            WizardStep::Summary => Some(WizardStep::Extras),
        }
    }
}

#[derive(Debug, Default)]
struct App {
    step: WizardStep,
    exit: bool,
}

impl App {
    fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        let title = Line::from(format!(" init_blaze — {} ", self.step.title())).bold();
        let instructions = Line::from(vec![
            " Back ".into(),
            "<Left/H> ".blue().bold(),
            " Next ".into(),
            "<Right/L> ".blue().bold(),
            " Quit ".into(),
            "<Q> ".blue().bold(),
        ]);

        let block = Block::bordered()
            .title(title.centered())
            .title_bottom(instructions.centered());

        let content = format!("Step: {}", self.step.title());

        let paragraph = Paragraph::new(content).centered().block(block);

        frame.render_widget(paragraph, frame.area());
    }

    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') => self.exit = true,
                KeyCode::Right | KeyCode::Char('l') => {
                    if let Some(next) = self.step.next() {
                        self.step = next;
                    }
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    if let Some(prev) = self.step.prev() {
                        self.step = prev;
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }
}

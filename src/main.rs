use std::io;
use strum::VariantArray;

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

#[derive(Debug, Default, VariantArray)]
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
            WizardStep::Vcs => "Version Control System",
            WizardStep::Languages => "Languages",
            WizardStep::Database => "Database",
            WizardStep::Remotes => "Remotes",
            WizardStep::Extras => "Extras",
            WizardStep::Summary => "Summary",
        }
    }
}

#[derive(Debug, Default)]
struct App {
    step_index: usize,
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
        let title = Line::from(format!(" init_blaze — {} ", self.current_step().title())).bold();
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

        let content = format!("Step: {}", self.current_step().title());

        let paragraph = Paragraph::new(content).centered().block(block);

        frame.render_widget(paragraph, frame.area());
    }

    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') => self.exit = true,
                KeyCode::Right | KeyCode::Char('l') => self.next(),
                KeyCode::Left | KeyCode::Char('h') => self.prev(),
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }

    fn current_step(&self) -> &WizardStep {
        &WizardStep::VARIANTS[self.step_index]
    }

    fn next(&mut self) {
        if self.step_index + 1 < WizardStep::VARIANTS.len() {
            self.step_index += 1;
        }
    }

    fn prev(&mut self) {
        self.step_index = self.step_index.saturating_sub(1);
    }
}

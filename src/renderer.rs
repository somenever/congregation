use crate::task::Task;
use crossterm::event::{Event, KeyCode, KeyModifiers};
use crossterm::style::{Color, Stylize};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, ClearType};
use crossterm::{cursor, execute, queue, style, terminal, QueueableCommand};
use std::io::{Stdout, Write};
use std::process::ExitStatus;

pub struct Renderer {
    stdout: Stdout,
    viewport_height: usize,
    scroll: usize,
    line_count: usize,
}

#[derive(Clone)]
enum Line<'a> {
    TaskName { name: &'a str, color: Color },
    TaskStatus(Option<ExitStatus>),
    Log(&'a str),
    Empty,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            stdout: std::io::stdout(),
            scroll: 0,
            viewport_height: 0,
            line_count: 0,
        }
    }

    pub fn enter_screen(&mut self) -> std::io::Result<()> {
        execute!(self.stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
        enable_raw_mode()?;
        Ok(())
    }

    pub fn leave_screen(&mut self) -> std::io::Result<()> {
        disable_raw_mode()?;
        execute!(self.stdout, terminal::LeaveAlternateScreen, cursor::Show)?;
        Ok(())
    }

    fn scroll_max(&self) -> usize {
        self.line_count.saturating_sub(self.viewport_height - 1)
    }

    fn scroll_down(&mut self, amount: usize) {
        self.scroll = self.scroll_max().min(self.scroll + amount);
    }

    fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    pub fn handle_input(&mut self, event: Event) -> bool {
        match event {
            Event::Key(event) => match event.code {
                KeyCode::Char('c') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                    return false
                }
                KeyCode::Char('q') => return false,
                KeyCode::Char('d') => self.scroll_down(self.viewport_height),
                KeyCode::Char('u') => self.scroll_up(self.viewport_height),
                KeyCode::Down | KeyCode::Char('j') => self.scroll_down(1),
                KeyCode::Up | KeyCode::Char('k') => self.scroll_up(1),
                _ => {}
            },
            _ => {}
        }
        true
    }

    fn render<'a>(&mut self, tasks: &'a [Task]) -> impl Iterator<Item = Line<'a>> + Clone {
        tasks.iter().flat_map(|task| {
            std::iter::once(Line::TaskName {
                name: &task.name,
                color: task.color,
            })
            .chain(task.logs.iter().map(|log| Line::Log(log)))
            .chain(std::iter::once(Line::TaskStatus(task.exit_status)))
        })
    }

    pub fn print_all_tasks(&mut self, tasks: &[Task]) -> std::io::Result<()> {
        for line in self.render(tasks) {
            self.draw_line(line)?;
        }
        Ok(())
    }

    fn draw_line(&mut self, line: Line) -> std::io::Result<()> {
        match line {
            Line::TaskName { name, color } => {
                let mut name = name.bold();
                name.style_mut().foreground_color = Some(color);
                self.stdout.queue(style::Print(name))?;
            }
            Line::TaskStatus(exit_status) => {
                queue!(
                    self.stdout,
                    style::Print("└ ".dark_grey()),
                    style::Print(match exit_status {
                        Some(status) => {
                            if status.success() {
                                "completed".to_owned().green()
                            } else {
                                match status.code() {
                                    Some(code) => format!("failed (code {})", code),
                                    None => "terminated".into(),
                                }
                                .red()
                            }
                        }
                        None => "running...".to_owned().grey(),
                    })
                )?;
            }
            Line::Log(log) => {
                queue!(
                    self.stdout,
                    style::Print("│ ".dark_grey()),
                    style::Print(log.trim())
                )?;
            }
            Line::Empty => {}
        }
        queue!(self.stdout, style::Print("\n"), cursor::MoveToColumn(0))?;
        Ok(())
    }

    pub fn draw_tasks(&mut self, tasks: &[Task]) -> std::io::Result<()> {
        queue!(
            self.stdout,
            cursor::MoveTo(0, 0),
            terminal::Clear(ClearType::All)
        )?;

        let (_, height) = terminal::size()?;
        self.viewport_height = height as usize;

        let lines = self.render(tasks);

        let snap_to_bottom = self.scroll == self.scroll_max();
        self.line_count = lines.clone().count();
        if snap_to_bottom {
            self.scroll = self.scroll_max();
        }

        for line in lines
            .chain(std::iter::repeat(Line::Empty))
            .skip(self.scroll)
            .take(self.viewport_height - 1)
        {
            self.draw_line(line)?;
        }

        self.stdout.queue(style::Print(
            concat!("congregation ", env!("CARGO_PKG_VERSION")).dark_grey(),
        ))?;
        self.stdout.flush()
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let _ = self.leave_screen();
    }
}

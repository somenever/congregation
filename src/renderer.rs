use crate::task::Task;
use crossterm::event::{Event, KeyCode, KeyModifiers};
use crossterm::style::{Color, Stylize};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, ClearType};
use crossterm::{cursor, execute, queue, style, terminal, QueueableCommand};
use std::io::{Stdout, Write};
use std::process::ExitStatus;

const LOG_PREFIX: &str = "│ ";

pub struct Renderer {
    stdout: Stdout,
    viewport_width: usize,
    viewport_height: usize,
    scroll_x: usize,
    scroll_y: usize,
    line_count: usize,
    longest_line: usize,
    in_screen: bool,
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
            scroll_x: 0,
            scroll_y: 0,
            viewport_width: 0,
            viewport_height: 0,
            line_count: 0,
            longest_line: 0,
            in_screen: false,
        }
    }

    pub fn enter_screen(&mut self) -> std::io::Result<()> {
        execute!(self.stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
        enable_raw_mode()?;
        self.in_screen = true;
        Ok(())
    }

    pub fn leave_screen(&mut self) -> std::io::Result<()> {
        disable_raw_mode()?;
        execute!(self.stdout, terminal::LeaveAlternateScreen, cursor::Show)?;
        self.in_screen = false;
        Ok(())
    }

    fn scroll_max(&self) -> usize {
        self.line_count.saturating_sub(self.viewport_height - 1)
    }

    fn scroll_down(&mut self, amount: usize) {
        self.scroll_y = self.scroll_max().min(self.scroll_y + amount);
    }

    fn scroll_up(&mut self, amount: usize) {
        self.scroll_y = self.scroll_y.saturating_sub(amount);
    }

    fn scroll_left(&mut self, amount: usize) {
        self.scroll_x = self.scroll_x.saturating_sub(amount);
    }

    fn scroll_right(&mut self, amount: usize) {
        self.scroll_x += amount;
    }

    fn scroll_to_end(&mut self) {
        self.scroll_x = self.longest_line - 1;
    }

    pub fn handle_input(&mut self, event: Event) -> bool {
        match event {
            Event::Key(event) => match event.code {
                KeyCode::Char('c') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                    return false
                }
                KeyCode::Char('q') => return false,
                KeyCode::Char('d') | KeyCode::PageDown => self.scroll_down(self.viewport_height),
                KeyCode::Char('u') | KeyCode::PageUp => self.scroll_up(self.viewport_height),
                KeyCode::Down if event.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_down(self.viewport_height)
                }
                KeyCode::Up if event.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_up(self.viewport_height)
                }
                KeyCode::Down | KeyCode::Char('j') => self.scroll_down(1),
                KeyCode::Up | KeyCode::Char('k') => self.scroll_up(1),
                KeyCode::Left if event.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_x = 0
                }
                KeyCode::Home | KeyCode::Char('0') => self.scroll_x = 0,
                KeyCode::Right if event.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.scroll_to_end()
                }
                KeyCode::End | KeyCode::Char('$') => self.scroll_to_end(),
                KeyCode::Left | KeyCode::Char('h') => self.scroll_left(1),
                KeyCode::Right | KeyCode::Char('l') => self.scroll_right(1),
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
                let scrolled_log = if self.in_screen {
                    let len = log.chars().count();
                    self.longest_line = self.longest_line.max(len);

                    let mut content_width = self.viewport_width - LOG_PREFIX.len();
                    let clipped_left = self.scroll_x > 0;
                    if clipped_left {
                        content_width -= 1;
                    }
                    let clipped_right = len > self.scroll_x + content_width;
                    if clipped_right {
                        content_width -= 1;
                    }

                    if self.scroll_x > len {
                        &"‹".dark_grey().to_string()
                    } else if !clipped_left && !clipped_right {
                        log
                    } else {
                        &format!(
                            "{}{}{}",
                            if clipped_left {
                                "‹".dark_grey().to_string()
                            } else {
                                "".into()
                            },
                            log.chars()
                                .skip(self.scroll_x)
                                .take(content_width)
                                .collect::<String>(),
                            if clipped_right {
                                "›".dark_grey().to_string()
                            } else {
                                "".into()
                            },
                        )
                    }
                } else {
                    log
                };
                queue!(
                    self.stdout,
                    style::Print(LOG_PREFIX.dark_grey()),
                    style::Print(scrolled_log),
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

        let (width, height) = terminal::size()?;
        self.viewport_width = width as usize;
        self.viewport_height = height as usize;

        let lines = self.render(tasks);

        let snap_to_bottom = self.scroll_y == self.scroll_max();
        self.line_count = lines.clone().count();
        if snap_to_bottom {
            self.scroll_y = self.scroll_max();
        }

        self.scroll_x = self.scroll_x.min(
            self.longest_line
                .saturating_sub(self.viewport_width - LOG_PREFIX.len() - 1),
        );

        self.longest_line = 0;
        for line in lines
            .chain(std::iter::repeat(Line::Empty))
            .skip(self.scroll_y)
            .take(self.viewport_height - 1)
        {
            self.draw_line(line)?;
        }

        queue!(
            self.stdout,
            style::Print(format!("{} tasks ", tasks.len()).green())
        )?;

        let mut print_key = |key: &str, name: &str| {
            queue!(
                self.stdout,
                style::Print(format!(" {key} ").black().on_dark_grey()),
                style::Print(" "),
                style::Print(name),
                style::Print(" "),
            )
        };
        print_key("q", "quit")?;
        print_key("u", "pgup")?;
        print_key("d", "pgdown")?;
        print_key("←↓↑→/hjkl", "navigate")?;

        let version = concat!("congregation ", env!("CARGO_PKG_VERSION"), " ");
        queue!(
            self.stdout,
            cursor::MoveToColumn((self.viewport_width - version.len()) as u16),
            style::Print(version.dark_grey()),
        )?;

        self.stdout.flush()
    }
}

use crate::task::Task;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::style::{Color, StyledContent, Stylize};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, ClearType};
use crossterm::{cursor, execute, queue, style, terminal, QueueableCommand};
use std::io::{Stdout, Write};

mod help_overlay;

const LOG_PREFIX: &str = "│ ";
const STATUS_PREFIX: &str = "└ ";

#[derive(PartialEq)]
enum Overlay {
    Help,
}

pub struct Renderer {
    stdout: Stdout,
    viewport_width: usize,
    viewport_height: usize,
    scroll_x: usize,
    scroll_y: usize,
    cursor_x: usize,
    cursor_y: usize,
    selected_task_id: usize,
    line_count: usize,
    cursor_line_length: usize,
    in_screen: bool,
    overlays: Vec<Overlay>,
}

#[derive(Clone)]
enum Line<'a> {
    TaskName {
        id: usize,
        name: &'a str,
        color: Option<Color>,
        collapsed: bool,
    },
    TaskStatus(usize, StyledContent<String>),
    Log(usize, &'a str),
    Empty,
}

impl Line<'_> {
    fn task_id(&self) -> Option<usize> {
        match self {
            Line::TaskName { id, .. } => Some(*id),
            Line::TaskStatus(id, _) => Some(*id),
            Line::Log(id, _) => Some(*id),
            Line::Empty => None,
        }
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            stdout: std::io::stdout(),
            scroll_x: 0,
            scroll_y: 0,
            cursor_x: 0,
            cursor_y: 0,
            selected_task_id: 0,
            viewport_width: 0,
            viewport_height: 0,
            line_count: 0,
            cursor_line_length: 0,
            in_screen: false,
            overlays: vec![],
        }
    }

    pub fn enter_screen(&mut self) -> std::io::Result<()> {
        execute!(self.stdout, terminal::EnterAlternateScreen)?;
        enable_raw_mode()?;
        self.in_screen = true;
        Ok(())
    }

    pub fn leave_screen(&mut self) -> std::io::Result<()> {
        disable_raw_mode()?;
        execute!(self.stdout, terminal::LeaveAlternateScreen)?;
        self.in_screen = false;
        Ok(())
    }

    fn set_cursor_x(&mut self, position: usize) {
        self.cursor_x = self.cursor_line_length.min(position);
        if self.cursor_x >= self.viewport_width + self.scroll_x {
            self.scroll_x = self.cursor_x - self.viewport_width + 1;
        }

        const LEFT_GUTTER: usize = 3;
        if self.cursor_x < self.scroll_x + LEFT_GUTTER {
            self.scroll_x = self.cursor_x.saturating_sub(LEFT_GUTTER);
        }
    }

    fn set_cursor_y(&mut self, position: usize) {
        // exclude the status bar
        let actual_viewport_height = self.viewport_height - 1;

        self.cursor_y = self.line_count.min(position);
        if self.cursor_y >= actual_viewport_height + self.scroll_y {
            self.scroll_y = self.cursor_y - actual_viewport_height + 1;
        }
        if self.cursor_y < self.scroll_y {
            self.scroll_y = self.cursor_y;
        }
    }

    fn page_up(&mut self) {
        self.set_cursor_y(self.cursor_y.saturating_sub(self.viewport_height));
    }

    fn page_down(&mut self) {
        self.set_cursor_y(self.cursor_y + self.viewport_height);
    }

    fn jump_to_task_name(&mut self, tasks: &[Task], task_id: usize) {
        for (idx, line) in self.render(tasks).enumerate() {
            if let Line::TaskName { id, .. } = line {
                if id == task_id {
                    self.set_cursor_y(idx);
                    break;
                }
            }
        }
    }

    pub fn quit(&mut self, tasks: &mut [Task]) {
        for task in tasks {
            task.end_gracefully();
        }
    }

    pub fn handle_input(&mut self, event: Event, tasks: &mut [Task]) {
        match event {
            Event::Key(event) if event.kind == KeyEventKind::Press => match event.code {
                KeyCode::Char('c') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.quit(tasks);
                }
                KeyCode::Char('u') | KeyCode::PageUp => self.page_up(),
                KeyCode::Char('d') | KeyCode::PageDown => self.page_down(),
                KeyCode::Up | KeyCode::Char('k')
                    if event.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.jump_to_task_name(tasks, self.selected_task_id.saturating_sub(1))
                }
                KeyCode::Down | KeyCode::Char('j')
                    if event.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.jump_to_task_name(tasks, self.selected_task_id + 1)
                }
                KeyCode::Left if event.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.set_cursor_x(0)
                }
                KeyCode::Right if event.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.set_cursor_x(self.cursor_line_length)
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.set_cursor_y(self.cursor_y.saturating_sub(1))
                }
                KeyCode::Down | KeyCode::Char('j') => self.set_cursor_y(self.cursor_y + 1),
                KeyCode::Left | KeyCode::Char('h') => {
                    self.set_cursor_x(self.cursor_x.saturating_sub(1));
                }
                KeyCode::Right | KeyCode::Char('l') => self.set_cursor_x(self.cursor_x + 1),
                KeyCode::Home | KeyCode::Char('0') => self.set_cursor_x(0),
                KeyCode::End | KeyCode::Char('$') => self.set_cursor_x(self.cursor_line_length),
                KeyCode::Char(' ') | KeyCode::Enter => {
                    if let Some(task) = tasks.get_mut(self.selected_task_id) {
                        task.collapsed = !task.collapsed;

                        if task.collapsed {
                            self.jump_to_task_name(tasks, self.selected_task_id);
                        }
                    }
                }
                KeyCode::Char('?') => self.toggle_overlay(Overlay::Help),
                KeyCode::Char('q') => {
                    if self.overlays.len() == 0 {
                        self.quit(tasks);
                    } else {
                        self.overlays.pop();
                    }
                }
                KeyCode::Char('x') => {
                    if let Some(task) = tasks.get_mut(self.selected_task_id) {
                        task.end_gracefully();
                    }
                }
                KeyCode::Char('r') => {
                    if let Some(task) = tasks.get_mut(self.selected_task_id) {
                        task.force_restart();
                    }
                }
                KeyCode::Esc => {
                    self.overlays.pop();
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn toggle_overlay(&mut self, overlay: Overlay) {
        if let Some((index, _)) = self
            .overlays
            .iter()
            .enumerate()
            .find(|(_, it)| **it == overlay)
        {
            self.overlays.remove(index);
        } else {
            self.overlays.push(overlay);
        }
    }

    fn render<'a>(&mut self, tasks: &'a [Task]) -> impl Iterator<Item = Line<'a>> + Clone {
        let in_screen = self.in_screen;

        tasks.iter().flat_map(move |task| {
            std::iter::once(Line::TaskName {
                id: task.id,
                name: &task.def.name,
                color: task.def.color,
                collapsed: task.collapsed,
            })
            .chain(
                (!task.collapsed || !in_screen)
                    .then(|| task.logs.iter().map(|log| Line::Log(task.id, log)))
                    .into_iter()
                    .flatten(),
            )
            .chain(std::iter::once(Line::TaskStatus(
                task.id,
                task.state.render(),
            )))
        })
    }

    pub fn print_all_tasks(&mut self, tasks: &[Task]) -> std::io::Result<()> {
        self.selected_task_id = usize::MAX;
        for line in self.render(tasks) {
            self.draw_line(line)?;
        }
        Ok(())
    }

    fn draw_line(&mut self, line: Line) -> std::io::Result<usize> {
        let len = match line {
            Line::TaskName {
                id,
                name,
                color,
                collapsed,
            } => {
                let len = name.len();
                let mut name = name.bold();
                name.style_mut().foreground_color = color;
                if collapsed && self.in_screen {
                    self.stdout
                        .queue(style::Print(if id == self.selected_task_id {
                            "+ ".green()
                        } else {
                            "+ ".dark_grey()
                        }))?;
                }
                self.stdout.queue(style::Print(name))?;
                len
            }
            Line::TaskStatus(id, status_text) => {
                let len = STATUS_PREFIX.chars().count() + status_text.content().chars().count();
                queue!(
                    self.stdout,
                    style::Print(if id == self.selected_task_id {
                        STATUS_PREFIX.green()
                    } else {
                        STATUS_PREFIX.dark_grey()
                    }),
                    style::Print(status_text)
                )?;
                len
            }
            Line::Log(id, log) => {
                let len = log.chars().count();
                let scrolled_log = if self.in_screen {
                    let mut content_width = self.viewport_width - LOG_PREFIX.chars().count();
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
                    style::Print(if id == self.selected_task_id {
                        LOG_PREFIX.green()
                    } else {
                        LOG_PREFIX.dark_grey()
                    }),
                    style::Print(scrolled_log),
                )?;
                LOG_PREFIX.chars().count() + len
            }
            Line::Empty => 0,
        };
        queue!(self.stdout, style::Print("\n"), cursor::MoveToColumn(0))?;
        Ok(len)
    }

    fn render_overlays(&mut self) -> std::io::Result<()> {
        for overlay in &self.overlays {
            match overlay {
                Overlay::Help => return help_overlay::render_help_overlay(&mut self.stdout),
            }
        }
        Ok(())
    }

    pub fn draw_tasks(&mut self, tasks: &[Task]) -> std::io::Result<()> {
        queue!(
            self.stdout,
            terminal::BeginSynchronizedUpdate,
            cursor::MoveTo(0, 0),
            terminal::Clear(ClearType::All)
        )?;

        let (width, height) = terminal::size()?;
        self.viewport_width = width as usize;
        self.viewport_height = height as usize;

        let lines = self.render(tasks);

        let snap_to_bottom = self.cursor_y == self.line_count;
        self.line_count = lines.clone().count();
        if snap_to_bottom {
            self.set_cursor_y(self.line_count);
        }

        if self.cursor_x > self.cursor_line_length {
            self.set_cursor_x(0);
            self.set_cursor_x(self.cursor_line_length);
        }

        let visible_lines = lines
            .chain(std::iter::repeat(Line::Empty))
            .skip(self.scroll_y)
            .take(self.viewport_height - 1)
            .enumerate();

        self.selected_task_id = visible_lines
            .clone()
            .find(|(idx, _)| self.cursor_y - self.scroll_y == *idx)
            .and_then(|(_, line)| line.task_id())
            .unwrap_or(tasks.len() - 1);

        for (idx, line) in visible_lines {
            let length = self.draw_line(line)?;
            if self.cursor_y - self.scroll_y == idx {
                self.cursor_line_length = length;
            } else if self.cursor_y == self.viewport_height {
                self.cursor_line_length = 0;
            }
        }

        queue!(
            self.stdout,
            style::Print(format!("{} tasks ", tasks.len()).green())
        )?;

        help_overlay::print_key(&mut self.stdout, "q", "quit")?;
        help_overlay::print_key(&mut self.stdout, "←↓↑→/hjkl", "navigate")?;
        help_overlay::print_key(&mut self.stdout, "?", "help")?;

        let version = concat!("congregation ", env!("CARGO_PKG_VERSION"));
        queue!(
            self.stdout,
            cursor::MoveToColumn((self.viewport_width - version.len()) as u16),
            style::Print(version.dark_grey()),
        )?;

        self.render_overlays()?;

        queue!(
            self.stdout,
            cursor::MoveTo(
                (self.cursor_x - self.scroll_x) as u16,
                (self.cursor_y - self.scroll_y) as u16
            ),
            terminal::EndSynchronizedUpdate,
        )?;

        self.stdout.flush()
    }
}

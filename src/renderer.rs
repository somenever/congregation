use std::io::{Stdout, Write};
use crossterm::{cursor, style, terminal, QueueableCommand};
use crossterm::style::Stylize;
use crate::task::Task;

pub struct Renderer {
    stdout: Stdout,
}

fn get_task_tail_offset(running_tasks: &[Task], id: usize) -> usize {
    let mut offset = 0;
    for task in &running_tasks[id + 1..] {
        offset += 1; // Task name line
        offset += task.logs.len();
        offset += 1; // Task status line
    }
    offset + 1
}

impl Renderer {
    pub fn new() -> Self {
        Self { stdout: std::io::stdout() }
    }

    pub fn draw_initial_tasks(&mut self, tasks: &[Task]) {
        for task in tasks {
            self.draw_task_name(task);
            self.draw_task_status(task);
        }
        self.stdout.flush().unwrap();
    }

    pub fn append_task_line(&mut self, tasks: &[Task], id: usize, line: &str) {
        let offset = get_task_tail_offset(&tasks, id);
        self.stdout.queue(cursor::MoveUp(offset as u16)).unwrap();

        let task = tasks.get(id).unwrap();
        self.stdout
            .queue(terminal::Clear(terminal::ClearType::CurrentLine))
            .unwrap();
        self.stdout
            .queue(style::PrintStyledContent("│ ".dark_grey()))
            .unwrap();
        self.stdout.queue(style::Print(line)).unwrap();
        self.draw_task_status(task);

        for task in &tasks[id + 1..] {
            self.draw_task_name(task);

            for log in &task.logs {
                self.stdout
                    .queue(terminal::Clear(terminal::ClearType::CurrentLine))
                    .unwrap();
                self.stdout
                    .queue(style::PrintStyledContent("│ ".dark_grey()))
                    .unwrap();
                self.stdout.queue(style::Print(log)).unwrap();
            }
            self.draw_task_status(task);
        }

        self.stdout.flush().unwrap();
    }

    pub fn update_task_status(&mut self, tasks: &[Task], id: usize) {
        let offset = get_task_tail_offset(tasks, id);
        self.stdout.queue(cursor::MoveUp(offset as u16)).unwrap();

        let task = tasks.get(id).unwrap();
        self.draw_task_status(task);
        if offset != 1 {
            self.stdout.queue(cursor::MoveDown(offset as u16 - 1)).unwrap();
        }
        self.stdout.flush().unwrap();
    }

    fn draw_task_status(&mut self, task: &Task) {
        self.stdout
            .queue(terminal::Clear(terminal::ClearType::CurrentLine))
            .unwrap();
        self.stdout
            .queue(style::PrintStyledContent("└ ".dark_grey()))
            .unwrap();
        self.stdout
            .queue(style::PrintStyledContent(match task.exit_status {
                Some(status) => {
                    if status.success() {
                        "completed\n".to_owned().green()
                    } else {
                        match status.code() {
                            Some(code) => format!("failed (code {})\n", code),
                            None => "terminated\n".into(),
                        }
                            .red()
                    }
                }
                None => "running...\n".to_owned().grey(),
            }))
            .unwrap();
    }

    fn draw_task_name(&mut self, task: &Task) {
        self.stdout
            .queue(terminal::Clear(terminal::ClearType::CurrentLine))
            .unwrap();

        let mut name = format!("{}\n", task.name).bold();
        name.style_mut().foreground_color = Some(task.color);
        self.stdout.queue(style::Print(name)).unwrap();
    }
}

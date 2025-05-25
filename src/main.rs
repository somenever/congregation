// congregation \
//   run "npm run dev" -d ./app \
//   run "npm run start" -d ./api

use std::io::{stderr, Read};
use std::{
    env::{self, Args},
    fs::{self},
    io::{stdout, BufRead, BufReader, Stdout, Write},
    iter::Peekable,
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
    sync::mpsc::{self, Sender},
};

use crossterm::{
    cursor,
    style::{self, Stylize},
    terminal, QueueableCommand,
};
use crossterm::style::Color;

#[derive(Debug)]
struct TaskDef {
    command: String,
    name: String,
    workdir: PathBuf,
    color: Color,
}

fn error(message: &str, block: impl FnOnce()) -> ! {
    eprintln!("{}", message.red());
    block();
    std::process::exit(1)
}

fn parse_task(args: &mut Peekable<Args>, task_count: i32) -> TaskDef {
    if !args.next().is_some_and(|arg| arg == "run") {
        error("invalid syntax", || {
            eprintln!("expected 'run' keyword as the first argument");
        });
    }

    let command = args.next().unwrap_or_else(|| error("invalid syntax", || {
        eprintln!("expected command after 'run' keyword");
    }));

    let mut name = None;
    let mut workdir = None;
    let mut color = Color::White;

    while args.peek().is_some_and(|arg| arg != "run") {
        match args.next().as_ref().map(|arg| arg.as_str()) {
            Some("-n") => name = Some(args.next().unwrap_or_else(|| error(
                &format!("invalid syntax (in task {})", task_count + 1),
                || eprintln!("expected task name after -n"),
            ))),
            Some("-d") => workdir = Some(args.next().unwrap_or_else(|| error(
                &format!("invalid syntax (in task {})", task_count + 1),
                || eprintln!("expected directory after -d"),
            ))),
            Some("-c") => {
                let color_arg = args.next().unwrap_or_else(|| error(
                    &format!("invalid syntax (in task {})", task_count + 1),
                    || {
                        eprintln!("expected color after -c");
                        eprintln!("{} {}", "note:".green(), "color syntax: RRGGBB (hex)".grey());
                        eprintln!("{}", "      if you have a # symbol, remove it".grey());
                    },
                ));

                let invalid_color = || error(
                    &format!("invalid syntax (in task {})", task_count + 1),
                    || {
                        eprintln!("invalid color '{color_arg}'");
                        eprintln!("{} {}", "note:".green(), "color syntax: 'RRGGBB' (hex)".grey());
                    },
                );

                if color_arg.len() != 6 { invalid_color(); }

                let r = u8::from_str_radix(&color_arg[0..2], 16)
                    .unwrap_or_else(|_| invalid_color());
                let g = u8::from_str_radix(&color_arg[2..4], 16)
                    .unwrap_or_else(|_| invalid_color());
                let b = u8::from_str_radix(&color_arg[4..6], 16)
                    .unwrap_or_else(|_| invalid_color());

                color = Color::Rgb { r, g, b };
            },
            Some(arg) => error(
                &format!("invalid syntax (in task {})", task_count + 1),
                || {
                    eprintln!("expected -n <name>, -d <dir>, -c <color> or run after command, got '{arg}'");
                    eprintln!("{} {}", "note:".green(), "if your command contains spaces, please wrap it in quotes".grey());
                },
            ),
            None => unreachable!(),
        }
    }

    TaskDef {
        name: name.or_else(|| workdir.clone()).unwrap_or(format!(
            "#{}: {}",
            task_count + 1,
            &command,
        )),
        command,
        workdir: workdir
            .map(|path| PathBuf::from(path))
            .unwrap_or(env::current_dir().unwrap()),
        color,
    }
}

enum TaskMessage {
    Stdout { task: usize, line: String },
    Exited { task: usize, status: ExitStatus },
}

struct Task {
    id: usize,
    completed: bool,
    name: String,
    logs: Vec<String>,
    color: Color,
}

impl TaskDef {
    fn run(self, id: usize, message_channel: Sender<TaskMessage>) -> Task {
        let workdir = fs::canonicalize(self.workdir).unwrap_or_else(
            |_| error("unexpected error", || {
                eprintln!("no working directory");
            })
        );
        std::thread::spawn(move || {
            let mut process = if cfg!(windows) {
                Command::new("cmd.exe")
                    .args(["/C", &self.command])
                    .current_dir(workdir)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
            } else {
                Command::new("sh")
                    .args(["-c", &self.command])
                    .current_dir(workdir)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
            }
            .unwrap();

            let stdout = process.stdout.take().unwrap()
                .chain(process.stderr.take().unwrap());
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();

            loop {
                let size = reader
                    .read_line(&mut line)
                    .unwrap_or_else(
                        |_| error(&format!("unexpected error (task {})", id + 1), || {
                            eprintln!("failed to read task output");
                        })
                    );

                if size == 0 {
                    let status = process.wait().unwrap();
                    let _ = message_channel.send(TaskMessage::Exited { task: id, status });
                    break;
                }
                let _ = message_channel.send(TaskMessage::Stdout {
                    task: id,
                    line: line.clone(),
                });
                line.clear();
            }
        });
        Task {
            name: self.name,
            color: self.color,
            id,
            logs: Vec::new(),
            completed: false,
        }
    }
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

fn draw_task_status(stdout: &mut Stdout, completed: bool) {
    stdout
        .queue(terminal::Clear(terminal::ClearType::CurrentLine))
        .unwrap();
    stdout
        .queue(style::PrintStyledContent("└ ".dark_grey()))
        .unwrap();
    stdout
        .queue(style::PrintStyledContent(if completed {
            "completed\n".green()
        } else {
            "running...\n".grey()
        }))
        .unwrap();
}

fn draw_task_name(stdout: &mut Stdout, task: &Task) {
    stdout
        .queue(terminal::Clear(terminal::ClearType::CurrentLine))
        .unwrap();

    let mut name = format!("{}\n", task.name).bold();
    name.style_mut().foreground_color = Some(task.color);
    stdout
        .queue(style::Print(name))
        .unwrap();
}

fn main() {
    let mut args = std::env::args().peekable();
    let name = args.next().unwrap_or("congregation".into());

    let mut tasks = Vec::new();
    while args.peek().is_some() {
        tasks.push(parse_task(&mut args, tasks.len() as i32));
    }

    let (tx, rx) = mpsc::channel::<TaskMessage>();

    let mut running_tasks = Vec::new();
    for (id, task) in tasks.into_iter().enumerate() {
        running_tasks.push(task.run(id, tx.clone()));
    }

    let mut stdout = stdout();
    let mut stderr = stderr();

    if running_tasks.len() == 0 {
        error("no tasks specified!", || {
            eprintln!("please list some commands to execute by using the 'run' keyword:");
            stderr
                .queue(style::PrintStyledContent("│ ".dark_grey()))
                .unwrap();
            eprintln!("{name} run 'echo hello'");
        });
    }

    for task in &running_tasks {
        draw_task_name(&mut stdout, task);
        draw_task_status(&mut stdout, false);
    }
    stdout.flush().unwrap();

    let mut completed_task_count = 0;

    for msg in rx {
        match msg {
            TaskMessage::Stdout { task: id, line } => {
                if true {
                    let task = running_tasks.get_mut(id).unwrap();
                    task.logs.push(line.clone());

                    let offset = get_task_tail_offset(&running_tasks, id);
                    stdout.queue(cursor::MoveUp(offset as u16)).unwrap();
                    stdout
                        .queue(terminal::Clear(terminal::ClearType::CurrentLine))
                        .unwrap();
                    stdout
                        .queue(style::PrintStyledContent("│ ".dark_grey()))
                        .unwrap();
                    stdout.queue(style::Print(line)).unwrap();
                    draw_task_status(&mut stdout, false);

                    for task in &running_tasks[id + 1..] {
                        draw_task_name(&mut stdout, task);

                        for log in &task.logs {
                            stdout
                                .queue(terminal::Clear(terminal::ClearType::CurrentLine))
                                .unwrap();
                            stdout
                                .queue(style::PrintStyledContent("│ ".dark_grey()))
                                .unwrap();
                            stdout.queue(style::Print(log)).unwrap();
                        }
                        draw_task_status(&mut stdout, task.completed);
                    }

                    stdout.flush().unwrap();
                }
            }
            TaskMessage::Exited { task: id, status } => {
                // TODO: handle exit status ^

                let task = running_tasks.get_mut(id).unwrap();
                task.completed = true;
                completed_task_count += 1;

                let offset = get_task_tail_offset(&running_tasks, id);
                stdout.queue(cursor::MoveUp(offset as u16)).unwrap();
                draw_task_status(&mut stdout, true);
                stdout.queue(cursor::MoveDown(offset as u16)).unwrap();
                stdout.flush().unwrap();

                if completed_task_count == running_tasks.len() {
                    break;
                }
            }
        }
    }
}

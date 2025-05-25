use crossterm::style::Color;
use crossterm::{
    cursor,
    style::{self, Stylize},
    terminal, QueueableCommand,
};
use indoc::printdoc;
use nix::sys::signal::Signal;
use nix::sys::termios::{LocalFlags, Termios};
use nix::sys::{signal, termios};
use nix::unistd::Pid;
use std::fmt::Display;
use std::fmt::Formatter;
use std::io::Read;
use std::path::Path;
use std::process::Child;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::{
    env::{self, Args},
    fs::{self},
    io::{stdout, BufRead, BufReader, Stdout, Write},
    iter::Peekable,
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
    sync::mpsc::{self, Sender},
};

#[derive(Debug)]
struct TaskDef {
    command: String,
    name: String,
    workdir: PathBuf,
    color: Color,
}

#[cfg(unix)]
fn disable_echoctl() -> Termios {
    let original = termios::tcgetattr(std::io::stdin()).unwrap();
    let mut raw = original.clone();

    raw.local_flags.remove(LocalFlags::ECHOCTL);
    termios::tcsetattr(std::io::stdin(), termios::SetArg::TCSANOW, &raw).unwrap();

    original
}

#[cfg(unix)]
fn restore_termios(original: &Termios) {
    termios::tcsetattr(std::io::stdin(), termios::SetArg::TCSANOW, original).unwrap();
}

#[derive(Default, Debug)]
struct Error {
    title: String,
    message: String,
    examples: Vec<String>,
    notes: Vec<String>,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        writeln!(f, "{}", self.title.as_str().red())?;

        if self.examples.is_empty() {
            writeln!(f, "{}", self.message)?;
        } else {
            writeln!(f, "")?;
            writeln!(f, "{}:", self.message)?;
            for example in &self.examples {
                writeln!(f, "{} {}", "│".dark_grey(), example)?;
            }
        }

        if !self.notes.is_empty() {
            for (i, note) in self.notes.iter().enumerate() {
                let prefix = "note:";
                let prefix_padding = " ".repeat(prefix.len());

                if i == 0 {
                    writeln!(f, "\n{} {}", prefix.green(), note.as_str().grey())?;
                } else {
                    writeln!(f, "\n{prefix_padding} {}", note.as_str().grey())?;
                }
            }
        }

        Ok(())
    }
}

fn print_help(name: &str) {
    printdoc!("
    Run multiple parallel tasks with grouped output

    Usage: {name} <task> [<task> ...]

    Task syntax:
      run <command> [-d <dir>] [-n <name>] [-c <rrggbb>]

      Options:
        <command>     The shell command to run (wrap in quotes if it contains spaces)
        -d <dir>      Working directory for the task (defaults to the current working directory)
        -n <name>     Name of the task (used in task header, defaults to working directory or command)
        -c <rrggbb>   Hex RGB color for task name (e.g., ff8800, defaults to white)
    ");
}

fn parse_task(args: &mut Peekable<Args>, task_count: i32) -> Result<TaskDef, Error> {
    if !args.next().is_some_and(|arg| arg == "run") {
        return Err(Error {
            title: "invalid syntax".into(),
            message: "expected 'run' or 'help' as the first argument".into(),
            ..Error::default()
        });
    }

    let Some(command) = args.next() else {
        return Err(Error {
            title: "invalid syntax".into(),
            message: "expected command after 'run' keyword".into(),
            ..Error::default()
        });
    };

    let mut name = None;
    let mut workdir = None;
    let mut color = Color::White;

    while args.peek().is_some_and(|arg| arg != "run") {
        match args.next().as_ref().map(|arg| arg.as_str()) {
            Some("-n") => name = Some(match args.next() {
                Some(name) => name,
                None => return Err(Error {
                    title: format!("invalid syntax (in task {})", task_count + 1),
                    message: "expected task name after -n".into(),
                    ..Error::default()
                }),
            }),
            Some("-d") => workdir = Some(match args.next() {
                Some(name) => name,
                None => return Err(Error {
                    title: format!("invalid syntax (in task {})", task_count + 1),
                    message: "expected directory after -d".into(),
                    ..Error::default()
                }),
            }),
            Some("-c") => {
                let Some(color_arg) = args.next() else {
                    return Err(Error {
                        title: format!("invalid syntax (in task {})", task_count + 1),
                        message: "expected color after -c".into(),
                        notes: vec![
                            "color syntax: RRGGBB (hex)".into(),
                            "if you have a # symbol, remove it".into()
                        ],
                        ..Error::default()
                    });
                };

                let invalid_color = Error {
                    title: format!("invalid syntax (in task {})", task_count + 1),
                    message: format!("invalid color '{color_arg}'"),
                    notes: vec!["color syntax: RRGGBB (hex)".into()],
                    ..Error::default()
                };

                if color_arg.len() != 6 { return Err(invalid_color); }

                let Ok(r) = u8::from_str_radix(&color_arg[0..2], 16)
                    else { return Err(invalid_color) };
                let Ok(g) = u8::from_str_radix(&color_arg[2..4], 16)
                    else { return Err(invalid_color) };
                let Ok(b) = u8::from_str_radix(&color_arg[4..6], 16)
                    else { return Err(invalid_color) };

                color = Color::Rgb { r, g, b };
            },
            Some(arg) => return Err(Error {
                title: format!("invalid syntax (in task {})", task_count + 1),
                message: format!("expected -n <name>, -d <dir>, -c <color> or run after command, got '{arg}'"),
                notes: vec![
                    "if your command contains spaces, please wrap it in quotes".into()
                ],
                ..Error::default()
            }),
            None => unreachable!(),
        }
    }

    Ok(TaskDef {
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
    })
}

enum TaskMessage {
    Stdout { task: usize, line: String },
    Exited { task: usize, status: ExitStatus },
}

struct Task {
    id: usize,
    exit_status: Option<ExitStatus>,
    name: String,
    logs: Vec<String>,
    color: Color,
    process: Arc<Mutex<Child>>,
}

impl TaskDef {
    fn run(self, id: usize, message_channel: Sender<TaskMessage>) -> Result<Task, Error> {
        let Ok(workdir) = fs::canonicalize(self.workdir) else {
           return Err(Error {
               title: "unexpected error".into(),
               message: "no working directory".into(),
               ..Error::default()
           });
        };

        let process = Arc::new(Mutex::new(if cfg!(windows) {
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
        }.unwrap()));

        {
            let process = Arc::clone(&process);
            std::thread::spawn(move || {
                let output = {
                    let mut process = process.lock().unwrap();
                    process.stdout.take().unwrap()
                        .chain(process.stderr.take().unwrap())
                };
                let mut reader = BufReader::new(output);
                let mut line = String::new();

                loop {
                    let size = reader.read_line(&mut line).unwrap();

                    if size == 0 {
                        let mut process = process.lock().unwrap();
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
        }

        Ok(Task {
            name: self.name,
            color: self.color,
            id,
            logs: Vec::new(),
            exit_status: None,
            process,
        })
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

impl Task {
    fn draw_status(&self, stdout: &mut Stdout) {
        stdout
            .queue(terminal::Clear(terminal::ClearType::CurrentLine))
            .unwrap();
        stdout
            .queue(style::PrintStyledContent("└ ".dark_grey()))
            .unwrap();
        stdout
            .queue(style::PrintStyledContent(match self.exit_status {
                Some(status) => if status.success() {
                    "completed\n".to_owned().green()
                } else {
                    match status.code() {
                        Some(code) => format!("failed (code {})\n", code),
                        None => "terminated\n".into(),
                    }.red()
                },
                None => "running...\n".to_owned().grey(),
            }))
            .unwrap();
    }

    fn draw_name(&self, stdout: &mut Stdout) {
        stdout
            .queue(terminal::Clear(terminal::ClearType::CurrentLine))
            .unwrap();

        let mut name = format!("{}\n", self.name).bold();
        name.style_mut().foreground_color = Some(self.color);
        stdout
            .queue(style::Print(name))
            .unwrap();
    }
}

fn run() -> Result<(), Error> {
    let mut args = std::env::args().peekable();
    let name = args
        .next()
        .and_then(|p| Path::new(&p)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_owned())
        )
        .unwrap_or("congregation".into());

    let mut tasks = Vec::new();
    while args.peek().is_some() {
        if args.peek().is_some_and(|arg|
            matches!(arg.as_str(), "-h" | "--help") || arg.to_lowercase().starts_with("h")
        ) {
            print_help(&name);
            return Ok(());
        }

        tasks.push(parse_task(&mut args, tasks.len() as i32));
    }

    let (tx, rx) = mpsc::channel::<TaskMessage>();

    let mut running_tasks = Vec::new();
    for (id, task) in tasks.into_iter().enumerate() {
        running_tasks.push(task?.run(id, tx.clone())?);
    }

    #[cfg(unix)]
    {
        let processes = running_tasks.iter()
            .map(|task| Arc::clone(&task.process)).collect::<Vec<_>>();

        let _ = ctrlc::set_handler(move || {
            for process in &processes {
                let process = process.lock().unwrap();
                let _ = signal::kill(Pid::from_raw(process.id() as i32), Signal::SIGINT);
            }
        });
    }

    let mut stdout = stdout();

    if running_tasks.len() == 0 {
        return Err(Error {
            title: "no tasks specified!".into(),
            message: "please list some commands to execute using the 'run' keyword".into(),
            examples: vec![format!("{name} run 'echo hello'")],
            notes: vec![format!("run '{name} help' for more information")],
        });
    }

    for task in &running_tasks {
        task.draw_name(&mut stdout);
        task.draw_status(&mut stdout);
    }
    stdout.flush().unwrap();

    let mut completed_task_count = 0;

    for msg in rx {
        match msg {
            TaskMessage::Stdout { task: id, line } => {
                if true {
                    let offset = get_task_tail_offset(&running_tasks, id);
                    stdout.queue(cursor::MoveUp(offset as u16)).unwrap();

                    let task = running_tasks.get_mut(id).unwrap();
                    task.logs.push(line.clone());

                    stdout
                        .queue(terminal::Clear(terminal::ClearType::CurrentLine))
                        .unwrap();
                    stdout
                        .queue(style::PrintStyledContent("│ ".dark_grey()))
                        .unwrap();
                    stdout.queue(style::Print(line)).unwrap();
                    task.draw_status(&mut stdout);

                    for task in &running_tasks[id + 1..] {
                        task.draw_name(&mut stdout);

                        for log in &task.logs {
                            stdout
                                .queue(terminal::Clear(terminal::ClearType::CurrentLine))
                                .unwrap();
                            stdout
                                .queue(style::PrintStyledContent("│ ".dark_grey()))
                                .unwrap();
                            stdout.queue(style::Print(log)).unwrap();
                        }
                        task.draw_status(&mut stdout);
                    }

                    stdout.flush().unwrap();
                }
            }
            TaskMessage::Exited { task: id, status } => {
                let offset = get_task_tail_offset(&running_tasks, id);
                stdout.queue(cursor::MoveUp(offset as u16)).unwrap();

                let task = running_tasks.get_mut(id).unwrap();
                task.exit_status = Some(status);
                completed_task_count += 1;

                task.draw_status(&mut stdout);
                stdout.queue(cursor::MoveDown(offset as u16)).unwrap();
                stdout.flush().unwrap();

                if completed_task_count == running_tasks.len() {
                    break;
                }
            }
        }
    }


    Ok(())
}

fn main() -> ExitCode {
    #[cfg(unix)]
    let original_termios = disable_echoctl();

    let exit_code = match run() {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprint!("{error}");
            ExitCode::FAILURE
        }
    };

    #[cfg(unix)]
    restore_termios(&original_termios);
    
    exit_code
}

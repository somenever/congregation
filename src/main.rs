// congregation \
//   run "npm run dev" -d ./app \
//   run "npm run start" -d ./api

use std::io::Read;
use std::{
    env::{self, Args},
    fs::{self, File},
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

#[derive(Debug)]
struct TaskDef {
    command: String,
    name: String,
    workdir: PathBuf,
}

fn parse_task(args: &mut Peekable<Args>, task_count: i32) -> TaskDef {
    if !args.next().is_some_and(|arg| arg == "run") {
        panic!("expected run");
    }

    let command = args.next().expect("command after run");

    let mut name = None;
    let mut workdir = None;

    while args.peek().is_some_and(|arg| arg != "run") {
        match args.next().as_ref().map(|arg| arg.as_str()) {
            Some("-n") => name = Some(args.next().expect("name after -n")),
            Some("-d") => workdir = Some(args.next().expect("directory after -d")),
            Some(arg) => panic!("expected -n or -d; got {arg}"),
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
}

impl TaskDef {
    fn run(self, id: usize, message_channel: Sender<TaskMessage>) -> Task {
        let workdir = fs::canonicalize(self.workdir).expect("valid working directory");
        std::thread::spawn(move || {
            let mut file = File::create(format!("./{id}")).unwrap();

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
                    .expect("failed to read task output");
                write!(file, "{}", &line).unwrap();
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

fn main() {
    let mut args = std::env::args().peekable();
    args.next();

    let mut tasks = Vec::new();
    while args.peek().is_some() {
        tasks.push(parse_task(&mut args, tasks.len() as i32));
    }

    let (tx, rx) = mpsc::channel::<TaskMessage>();

    let mut running_tasks = Vec::new();
    for (id, task) in tasks.into_iter().enumerate() {
        running_tasks.push(task.run(id, tx.clone()));
    }

    if running_tasks.len() == 0 {
        panic!("no tasks specified");
    }

    let mut stdout = stdout();

    for task in &running_tasks {
        stdout
            .queue(style::Print(format!("{}\n", task.name)))
            .unwrap();
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
                        stdout
                            .queue(terminal::Clear(terminal::ClearType::CurrentLine))
                            .unwrap();
                        stdout
                            .queue(style::Print(format!("{}\n", task.name)))
                            .unwrap();
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

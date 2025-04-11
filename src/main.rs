// congregation \
//   run "npm run dev" -d ./app \
//   run "npm run start" -d ./api

use std::{
    collections::HashMap,
    env::{self, Args},
    io::{stdout, BufRead, BufReader, Read, Stdout, Write},
    iter::Peekable,
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
    sync::mpsc,
};

use crossterm::{cursor, style, terminal, QueueableCommand};

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
        let sender = tx.clone();
        std::thread::spawn(move || {
            let mut process = if cfg!(windows) {
                Command::new("cmd.exe")
                    .args(["/C", &task.command])
                    .stdout(Stdio::piped())
                    .spawn()
            } else {
                Command::new("sh")
                    .args(["-c", &task.command])
                    .stdout(Stdio::piped())
                    .spawn()
            }
            .unwrap();

            let stdout = process.stdout.take().unwrap();
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();

            loop {
                line.clear();
                let size = reader
                    .read_line(&mut line)
                    .expect("failed to read task output");
                if size == 0 {
                    let status = process.wait().unwrap();
                    let _ = sender.send(TaskMessage::Exited { task: id, status });
                    break;
                }
                let _ = sender.send(TaskMessage::Stdout {
                    task: id,
                    line: line.clone(),
                });
            }
        });
        running_tasks.push(Task {
            name: task.name,
            id,
            logs: Vec::new(),
            completed: false,
        });
    }

    if running_tasks.len() == 0 {
        panic!("no tasks specified");
    }

    let mut stdout = stdout();

    for task in &running_tasks {
        stdout
            .queue(style::Print(format!("{}\n", task.name)))
            .unwrap();
    }
    stdout.flush().unwrap();

    let mut completed_count = 0;

    for msg in rx {
        match msg {
            TaskMessage::Stdout { task: id, line } => {
                let task = running_tasks.get_mut(id).unwrap();
                task.logs.push(line.clone());

                let mut distance = 0;
                for task in &running_tasks[id + 1..] {
                    distance += task.logs.len() + 1;
                }

                if distance > 0 {
                    stdout.queue(cursor::MoveUp(distance as u16)).unwrap();
                }
                stdout
                    .queue(terminal::Clear(terminal::ClearType::CurrentLine))
                    .unwrap();
                stdout.queue(style::Print(line)).unwrap();

                for task in &running_tasks[id + 1..] {
                    stdout
                        .queue(style::Print(format!("{}\n", task.name)))
                        .unwrap();
                    for log in &task.logs {
                        stdout.queue(style::Print(log)).unwrap();
                    }
                }

                stdout.flush().unwrap();
            }
            TaskMessage::Exited { task, status } => {
                let task = running_tasks.get_mut(task).unwrap();
                //println!("{}: exited {status:#?}", task.name);
                task.completed = true;
                completed_count += 1;
                if completed_count == running_tasks.len() {
                    break;
                }
            }
        }
    }
}

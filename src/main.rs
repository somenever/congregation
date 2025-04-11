// congregation \
//   run "npm run dev" -d ./app \
//   run "npm run start" -d ./api

use std::{
    collections::HashMap,
    env::{self, Args},
    io::{BufRead, BufReader, Read},
    iter::Peekable,
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
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
    Exited { task: usize },
}

struct Task {
    id: usize,
    name: String,
}

fn main() {
    let mut args = std::env::args().peekable();
    args.next();

    let mut tasks = Vec::new();
    while args.peek().is_some() {
        tasks.push(parse_task(&mut args, tasks.len() as i32));
    }

    let (tx, rx) = mpsc::channel::<TaskMessage>();

    let mut running_tasks = HashMap::new();
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
                let size = reader
                    .read_line(&mut line)
                    .expect("failed to read task output");
                if size == 0 {
                    let _ = sender.send(TaskMessage::Exited { task: id });
                    break;
                }
                let _ = sender.send(TaskMessage::Stdout {
                    task: id,
                    line: line.clone(),
                });
            }
        });
        running_tasks.insert(
            id,
            Task {
                name: task.name,
                id,
            },
        );
    }

    for msg in rx {
        match msg {
            TaskMessage::Stdout { task, line } => {
                let name = &running_tasks.get(&task).unwrap().name;
                print!("{name}: {line}");
            }
            TaskMessage::Exited { task } => {
                let name = &running_tasks.get(&task).unwrap().name;
                println!("{name}: exited");
                running_tasks.remove(&task);
                if running_tasks.is_empty() {
                    break;
                }
            }
        }
    }
}

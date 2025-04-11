// congregation \
//   run "npm run dev" -d ./app \
//   run "npm run start" -d ./api

use std::{
    env::{self, Args},
    iter::Peekable,
    path::PathBuf,
};

#[derive(Debug)]
struct Task {
    command: String,
    name: String,
    workdir: PathBuf,
}

fn parse_task(args: &mut Peekable<Args>, task_count: i32) -> Task {
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

    Task {
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

fn main() {
    let mut args = std::env::args().peekable();
    args.next();

    let mut tasks = Vec::new();
    while args.peek().is_some() {
        tasks.push(parse_task(&mut args, tasks.len() as i32));
    }

    dbg!(tasks);
}

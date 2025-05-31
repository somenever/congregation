mod arg_parser;
mod diagnostics;
mod task;
mod renderer;

use crate::renderer::Renderer;
use crate::task::TaskMessage;
use arg_parser::parse_args;
use crossterm::style::Color;
use diagnostics::Error;
use std::process::ExitCode;
use std::sync::Arc;
use std::{
    path::PathBuf,
};
use task::Task;
use tokio::sync::broadcast::{self};

#[cfg(unix)]
use nix::{
    sys::{
        signal::{self, Signal},
        termios::{self, LocalFlags, Termios}
    },
    unistd::Pid
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

async fn run() -> Result<(), Error> {
    let tasks = parse_args()?;
    if tasks.is_empty() {
        return Ok(());
    }

    let (tx, mut rx) = broadcast::channel::<TaskMessage>(16);

    let mut running_tasks = Vec::new();
    for (id, task) in tasks.into_iter().enumerate() {
        running_tasks.push(Task::run(task, id, tx.clone()).await?);
    }

    #[cfg(unix)]
    {
        let processes = running_tasks
            .iter()
            .map(|task| Arc::clone(&task.process))
            .collect::<Vec<_>>();

        let _ = ctrlc::set_handler(move || {
            for process in &processes {
                let _ = signal::kill(
                    Pid::from_raw(process.blocking_lock().id().unwrap() as i32),
                    Signal::SIGINT,
                );
            }
        });
    }

    let mut renderer = Renderer::new();
    renderer.draw_initial_tasks(&running_tasks);

    let mut completed_task_count = 0;

    while let Ok(msg) = rx.recv().await {
        match msg {
            TaskMessage::Stdout { task: id, line } => {
                let task = running_tasks.get_mut(id).unwrap();
                task.logs.push(line.clone());

                renderer.append_task_line(&running_tasks, id, &line);
            }
            TaskMessage::Exited { task: id, status } => {
                let task = running_tasks.get_mut(id).unwrap();
                task.exit_status = Some(status);
                completed_task_count += 1;

                renderer.update_task_status(&running_tasks, id);

                if completed_task_count == running_tasks.len() {
                    break;
                }
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    #[cfg(unix)]
    let original_termios = disable_echoctl();

    let exit_code = match run().await {
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

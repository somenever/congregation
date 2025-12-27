mod arg_parser;
mod diagnostics;
mod renderer;
mod task;

use crate::renderer::Renderer;
use crate::task::TaskMessage;
use arg_parser::parse_args;
use crossterm::{event::EventStream, style::Color};
use diagnostics::Error;
use std::{path::PathBuf, process::ExitCode};
use task::Task;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::StreamExt;

#[cfg(unix)]
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};

#[derive(Debug)]
struct TaskDef {
    command: String,
    name: String,
    workdir: PathBuf,
    color: Color,
}

async fn run() -> Result<(), Error> {
    let tasks = parse_args()?;
    if tasks.is_empty() {
        return Ok(());
    }

    let (tx, mut rx) = mpsc::channel::<TaskMessage>(32);

    let mut tasks = tasks
        .into_iter()
        .enumerate()
        .map(|(id, task)| Task::run(task, id, tx.clone()))
        .collect::<Result<Vec<_>, Error>>()?;

    let (interrupt_tx, mut interrupt_rx) = broadcast::channel(1);

    #[cfg(unix)]
    let _ = ctrlc::set_handler(move || {
        let _ = interrupt_tx.send(());
    });

    let mut completed_task_count = 0;
    let mut events = EventStream::new();

    let mut renderer = Renderer::new();
    renderer.enter_screen()?;
    renderer.draw_tasks(&tasks)?;

    loop {
        tokio::select! {
            Some(msg) = rx.recv() => match msg {
                TaskMessage::Stdout { task: id, line } => {
                    let task = tasks.get_mut(id).unwrap();
                    task.logs.push(line.clone());

                    renderer.draw_tasks(&tasks)?;
                }
                TaskMessage::Exited { task: id, status } => {
                    let task = tasks.get_mut(id).unwrap();
                    task.exit_status = Some(status);
                    completed_task_count += 1;

                    renderer.draw_tasks(&tasks)?;

                    if completed_task_count == tasks.len() {
                        break;
                    }
                }
            },
            Some(Ok(event)) = events.next() => {
                if !renderer.handle_input(event) {
                    break;
                }
                renderer.draw_tasks(&tasks)?;
            }
            Ok(()) = interrupt_rx.recv() => break,
            else => break
        }
    }

    #[cfg(unix)]
    for task in tasks.iter_mut() {
        if let Some(process) = task.process.lock().await.id() {
            let _ = signal::kill(Pid::from_raw(process as i32), Signal::SIGINT);
        }
        // Makes task show up as "completed"
        if task.exit_status.is_none() {
            use std::process::ExitStatus;

            task.exit_status = Some(ExitStatus::default());
        }
    }

    renderer.leave_screen()?;
    renderer.print_all_tasks(&tasks)?;

    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprint!("{error}");
            ExitCode::FAILURE
        }
    }
}

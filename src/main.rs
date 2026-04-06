mod arg_parser;
mod diagnostics;
mod renderer;
mod task;

use crate::renderer::Renderer;
use crate::task::TaskMessage;
use arg_parser::parse_args;
use crossterm::event::EventStream;
use diagnostics::Error;
use std::process::ExitCode;
use task::Task;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::StreamExt;

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
                    task.logs.push(strip_ansi_escapes::strip_str(line.trim()));

                    renderer.draw_tasks(&tasks)?;
                }
                TaskMessage::Exited { task: id, state } => {
                    let task = tasks.get_mut(id).unwrap();

                    if task.is_running() {
                        task.state = state;
                    }
                    renderer.draw_tasks(&tasks)?;

                    completed_task_count += 1;
                    if completed_task_count == tasks.len() {
                        break;
                    }
                }
            },
            Some(Ok(event)) = events.next() => {
                renderer.handle_input(event, &mut tasks);
                renderer.draw_tasks(&tasks)?;
            }
            Ok(()) = interrupt_rx.recv() => break,
            else => break
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

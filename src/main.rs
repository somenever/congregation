mod arg_parser;
mod diagnostics;
mod renderer;
mod task;

use crate::task::{TaskMessage, TaskMessageKind};
use crate::{renderer::Renderer, task::TaskState};
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

    let mut tasks: Vec<Task> = tasks
        .into_iter()
        .enumerate()
        .map(|(id, task)| {
            let mut task = Task::new(task, id, tx.clone());
            task.run();
            task
        })
        .collect();

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
            Some(TaskMessage { task: id, kind }) = rx.recv() => match kind {
                TaskMessageKind::Stdout(line) => {
                    let task = tasks.get_mut(id).unwrap();
                    task.logs.push(strip_ansi_escapes::strip_str(line.trim()));

                    renderer.draw_tasks(&tasks)?;
                }
                TaskMessageKind::Exited(reason) => {
                    let task = tasks.get_mut(id).unwrap();

                    match &task.state {
                        TaskState::ForceRestarting => {
                            task.run();
                            renderer.draw_tasks(&tasks)?;
                            continue;
                        }

                        TaskState::Running { .. } => {
                            if let Some(delay) = task.def.restart_delay_secs {
                                task.start_restart_countdown(reason, delay);
                                continue;
                            }

                            task.state = TaskState::Exited(reason);
                        }

                        TaskState::Stopped => {},

                        _ => unreachable!()
                    }

                    renderer.draw_tasks(&tasks)?;

                    completed_task_count += 1;
                    if completed_task_count == tasks.len() {
                        break;
                    }
                }
                TaskMessageKind::Restarting(secs) => {
                    let task = tasks.get_mut(id).unwrap();

                    if let TaskState::Restarting { remaining_secs, .. } = &mut task.state {
                        *remaining_secs = secs;
                        renderer.draw_tasks(&tasks)?;
                    }
                },
                TaskMessageKind::Restart => {
                    let task = tasks.get_mut(id).unwrap();
                    task.run();
                    renderer.draw_tasks(&tasks)?;
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

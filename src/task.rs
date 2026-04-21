use crossterm::style::{Color, StyledContent, Stylize};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;

#[derive(Clone, Debug)]
pub enum TaskMessageKind {
    Stdout(String),
    Exited(TaskExitReason),
    Restarting(u32),
    Restart,
}

#[derive(Clone, Debug)]
pub struct TaskMessage {
    pub task: usize,
    pub kind: TaskMessageKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TaskExitReason {
    Succeeded,
    Failed(i32),
    Killed(&'static str),
}

#[derive(Debug)]
pub enum TaskState {
    Running {
        pid: u32,
        stdin: Option<ChildStdin>,
    },
    Stopped,
    Exited(TaskExitReason),
    Restarting {
        exit_reason: TaskExitReason,
        remaining_secs: u32,
        cancel_tx: oneshot::Sender<()>,
    },
    ForceRestarting,
}

impl TaskExitReason {
    pub fn render(&self) -> StyledContent<String> {
        match self {
            TaskExitReason::Succeeded => "completed".to_owned().green(),
            TaskExitReason::Killed(signal) => format!("killed ({signal})").red(),
            TaskExitReason::Failed(code) => format!("failed (code {code})").red(),
        }
    }
}

impl TaskState {
    pub fn render(&self) -> StyledContent<String> {
        match self {
            TaskState::Running { .. } => "running...".to_owned().green(),
            TaskState::Stopped => "stopped".to_owned().green(),
            TaskState::Exited(reason) => reason.render(),
            TaskState::Restarting {
                exit_reason,
                remaining_secs,
                ..
            } => format!(
                "{}: restarting in {remaining_secs}s...",
                exit_reason.render().content()
            )
            .yellow(),
            TaskState::ForceRestarting => "restarting...".to_owned().yellow(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TaskDef {
    pub command: String,
    pub name: String,
    pub workdir: PathBuf,
    pub color: Option<Color>,
    pub restart_delay_secs: Option<u32>,
}

#[derive(Debug)]
pub struct Task {
    pub def: TaskDef,
    pub id: usize,
    pub state: TaskState,
    pub logs: Vec<String>,
    pub collapsed: bool,
    pub message_channel: Sender<TaskMessage>,
}

impl Task {
    pub fn new(def: TaskDef, id: usize, message_channel: Sender<TaskMessage>) -> Task {
        Task {
            def,
            id,
            logs: Vec::new(),
            state: TaskState::Stopped,
            collapsed: false,
            message_channel,
        }
    }

    pub fn run(&mut self) {
        let id = self.id;
        let def = self.def.clone();

        if let TaskState::Restarting { .. } | TaskState::ForceRestarting = self.state {
            self.logs
                .push("task restarted".dark_grey().italic().to_string());
        }

        let mut process = {
            #[cfg(windows)]
            {
                use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;

                Command::new("cmd.exe")
                    .args(["/C", &def.command])
                    .current_dir(def.workdir)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .creation_flags(CREATE_NEW_PROCESS_GROUP)
                    .spawn()
                    .unwrap()
            }

            #[cfg(not(windows))]
            Command::new("sh")
                .args(["-c", &def.command])
                .current_dir(def.workdir)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .process_group(0)
                .spawn()
                .unwrap()
        };
        self.state = TaskState::Running {
            pid: process.id().unwrap(),
            stdin: process.stdin.take(),
        };

        {
            let message_channel = self.message_channel.clone();
            let stdout = process.stdout.take().unwrap();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();

                while reader.read_line(&mut line).await.unwrap() != 0 {
                    let _ = message_channel
                        .send(TaskMessage {
                            task: id,
                            kind: TaskMessageKind::Stdout(line.clone()),
                        })
                        .await;
                    line.clear();
                }
            });
        }

        {
            let message_channel = self.message_channel.clone();
            let stderr = process.stderr.take().unwrap();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();

                while reader.read_line(&mut line).await.unwrap() != 0 {
                    let _ = message_channel
                        .send(TaskMessage {
                            task: id,
                            kind: TaskMessageKind::Stdout(line.clone()),
                        })
                        .await;
                    line.clear();
                }
            });
        }

        {
            let message_channel = self.message_channel.clone();
            tokio::spawn(async move {
                let status = process.wait().await.unwrap();
                let _ = message_channel
                    .send(TaskMessage {
                        task: id,
                        kind: TaskMessageKind::Exited(match status.code() {
                            Some(0) => TaskExitReason::Succeeded,
                            Some(code) => TaskExitReason::Failed(code),

                            #[cfg(unix)]
                            None => {
                                use nix::sys::signal::Signal;
                                use std::os::unix::process::ExitStatusExt;

                                TaskExitReason::Killed(
                                    Signal::try_from(status.signal().unwrap()).unwrap().as_str(),
                                )
                            }

                            #[cfg(not(unix))]
                            None => unreachable!(),
                        }),
                    })
                    .await;
            });
        }
    }

    pub fn end_gracefully(&mut self) {
        let state = std::mem::replace(&mut self.state, TaskState::Stopped);
        match state {
            TaskState::Running { pid, stdin } => send_stop_signal(pid, stdin),
            TaskState::Restarting { cancel_tx, .. } => {
                cancel_tx.send(()).unwrap();
                let _ = self.message_channel.try_send(TaskMessage {
                    task: self.id,
                    // the receiver will correctly mark this task as completed,
                    // because the state is set to Stopped. this is kind of ugly,
                    // but it works for now and avoids the issue of completed_task_count
                    // not incrementing when a restarting task is stopped.
                    kind: TaskMessageKind::Exited(TaskExitReason::Succeeded),
                });
            }
            TaskState::Exited(reason) => self.state = TaskState::Exited(reason),
            _ => {}
        };
    }

    pub fn force_restart(&mut self) {
        let state = std::mem::replace(&mut self.state, TaskState::ForceRestarting);
        match state {
            TaskState::Running { pid, stdin } => send_stop_signal(pid, stdin),
            TaskState::Restarting { cancel_tx, .. } => {
                cancel_tx.send(()).unwrap();
                self.run();
            }
            TaskState::ForceRestarting => {}
            _ => self.run(),
        }
    }

    pub fn start_restart_countdown(&mut self, exit_reason: TaskExitReason, delay: u32) {
        let id = self.id;
        let message_channel = self.message_channel.clone();

        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        self.state = TaskState::Restarting {
            exit_reason,
            remaining_secs: delay,
            cancel_tx,
        };

        tokio::spawn(async move {
            tokio::select! {
                _ = cancel_rx => {},
                _ = async {
                    for remaining_secs in (1..=delay).rev() {
                        let _ = message_channel
                            .send(TaskMessage {
                                task: id,
                                kind: TaskMessageKind::Restarting(remaining_secs),
                            })
                            .await;
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }

                    let _ = message_channel
                        .send(TaskMessage { task: id, kind: TaskMessageKind::Restart })
                        .await;
                } => {}
            }
        });
    }
}

fn send_stop_signal(pid: u32, #[allow(unused_variables)] stdin: Option<ChildStdin>) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};

        unsafe {
            GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid);
            if let Some(mut stdin) = stdin {
                use tokio::io::AsyncWriteExt;
                tokio::spawn(async move {
                    stdin.write_all(b"Y\n").await.unwrap();
                });
            }
        }
    }

    #[cfg(unix)]
    {
        use nix::{
            sys::signal::{self, Signal},
            unistd::Pid,
        };

        signal::kill(Pid::from_raw(-(pid as i32)), Signal::SIGINT).unwrap();
    }
}

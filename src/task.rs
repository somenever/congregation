use crate::diagnostics::Error;
use crossterm::style::Color;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc::Sender;

#[derive(Clone, Debug)]
pub enum TaskMessage {
    Stdout { task: usize, line: String },
    Exited { task: usize, state: TaskState },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TaskState {
    Running,
    Succeeded,
    Stopped,
    Failed(i32),
    Killed(&'static str),
}

#[derive(Debug)]
pub struct TaskDef {
    pub command: String,
    pub name: String,
    pub workdir: PathBuf,
    pub color: Option<Color>,
}

#[derive(Debug)]
pub struct Task {
    pub id: usize,
    pub state: TaskState,
    pub name: String,
    pub logs: Vec<String>,
    pub color: Option<Color>,
    pub pid: u32,
    pub collapsed: bool,
}

impl Task {
    pub fn run(
        def: TaskDef,
        id: usize,
        message_channel: Sender<TaskMessage>,
    ) -> Result<Task, Error> {
        let Ok(workdir) = dunce::canonicalize(def.workdir) else {
            return Err(Error {
                title: format!("error running task '{}'", def.name),
                message: "invalid working directory".into(),
                ..Error::default()
            });
        };

        let mut process = {
            #[cfg(windows)]
            {
                use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;

                Command::new("cmd.exe")
                    .args(["/C", &def.command])
                    .current_dir(workdir)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .creation_flags(CREATE_NEW_PROCESS_GROUP)
                    .spawn()
                    .unwrap()
            }

            #[cfg(not(windows))]
            Command::new("sh")
                .args(["-c", &def.command])
                .current_dir(workdir)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .process_group(0)
                .spawn()
                .unwrap()
        };
        let pid = process.id().unwrap();

        {
            let message_channel = message_channel.clone();
            let stdout = process.stdout.take().unwrap();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();

                while reader.read_line(&mut line).await.unwrap() != 0 {
                    let _ = message_channel
                        .send(TaskMessage::Stdout {
                            task: id,
                            line: line.clone(),
                        })
                        .await;
                    line.clear();
                }
            });
        }

        {
            let message_channel = message_channel.clone();
            let stderr = process.stderr.take().unwrap();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();

                while reader.read_line(&mut line).await.unwrap() != 0 {
                    let _ = message_channel
                        .send(TaskMessage::Stdout {
                            task: id,
                            line: line.clone(),
                        })
                        .await;
                    line.clear();
                }
            });
        }

        {
            let message_channel = message_channel.clone();
            tokio::spawn(async move {
                let status = process.wait().await.unwrap();
                let _ = message_channel
                    .send(TaskMessage::Exited {
                        task: id,
                        state: match status.code() {
                            Some(0) => TaskState::Succeeded,
                            #[cfg(unix)]
                            Some(130) => TaskState::Stopped, // SIGINT exit code
                            #[cfg(windows)]
                            Some(0xC000013A) => TaskState::Stopped, // STATUS_CONTROL_C_EXIT
                            Some(code) => TaskState::Failed(code),

                            #[cfg(unix)]
                            None => {
                                use nix::sys::signal::Signal;
                                use std::os::unix::process::ExitStatusExt;

                                let signal = Signal::try_from(status.signal().unwrap()).unwrap();

                                if signal == Signal::SIGINT {
                                    TaskState::Stopped
                                } else {
                                    TaskState::Killed(signal.as_str())
                                }
                            }

                            #[cfg(not(unix))]
                            None => unreachable!(),
                        },
                    })
                    .await;
            });
        }

        Ok(Task {
            name: def.name,
            color: def.color,
            id,
            logs: Vec::new(),
            state: TaskState::Running,
            pid,
            collapsed: false,
        })
    }

    pub fn is_running(&self) -> bool {
        self.state == TaskState::Running
    }

    pub fn end_gracefully(&mut self) {
        if !self.is_running() {
            return;
        }

        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_C_EVENT};

            unsafe {
                GenerateConsoleCtrlEvent(CTRL_C_EVENT, self.pid);
            }
        }

        #[cfg(unix)]
        {
            use nix::{
                sys::signal::{self, Signal},
                unistd::Pid,
            };

            signal::kill(Pid::from_raw(-(self.pid as i32)), Signal::SIGINT).unwrap();
        }
    }
}

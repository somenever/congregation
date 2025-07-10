use crate::diagnostics::Error;
use crate::TaskDef;
use crossterm::style::Color;
use std::fs;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;

#[derive(Clone)]
#[derive(Debug)]
pub enum TaskMessage {
    Stdout { task: usize, line: String },
    Exited { task: usize, status: ExitStatus },
}

pub struct Task {
    pub id: usize,
    pub exit_status: Option<ExitStatus>,
    pub name: String,
    pub logs: Vec<String>,
    pub color: Color,
    pub process: Arc<Mutex<Child>>,
}

impl Task {
    pub async fn run(
        def: TaskDef,
        id: usize,
        message_channel: Sender<TaskMessage>,
    ) -> Result<Task, Error> {
        let Ok(workdir) = dunce::canonicalize(def.workdir) else {
            return Err(Error {
                title: "unexpected error".into(),
                message: "no working directory".into(),
                ..Error::default()
            });
        };

        let process = Arc::new(Mutex::new(
            if cfg!(windows) {
                Command::new("cmd.exe")
                    .args(["/C", &def.command])
                    .current_dir(workdir)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
            } else {
                Command::new("sh")
                    .args(["-c", &def.command])
                    .current_dir(workdir)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
            }
            .unwrap(),
        ));

        {
            let message_channel = message_channel.clone();
            let process = Arc::clone(&process);
            tokio::spawn(async move {
                let stdout = process.lock().await.stdout.take().unwrap();
                let mut reader = BufReader::new(stdout);

                let mut line = String::new();

                loop {
                    let size = reader.read_line(&mut line).await.unwrap();

                    if size == 0 {
                        let status = process.lock().await.wait().await.unwrap();
                        let _ = message_channel.send(TaskMessage::Exited { task: id, status }).await;
                        break;
                    }

                    let _ = message_channel.send(TaskMessage::Stdout {
                        task: id,
                        line: line.clone(),
                    }).await;
                    line.clear();
                }
            });
        }

        {
            let process = Arc::clone(&process);
            tokio::spawn(async move {
                let stderr = process.lock().await.stderr.take().unwrap();
                let mut reader = BufReader::new(stderr);

                let mut line = String::new();

                loop {
                    let size = reader.read_line(&mut line).await.unwrap();

                    if size == 0 {
                        break;
                    }

                    let _ = message_channel.send(TaskMessage::Stdout {
                        task: id,
                        line: line.clone(),
                    }).await;
                    line.clear();
                }
            });
        }

        Ok(Task {
            name: def.name,
            color: def.color,
            id,
            logs: Vec::new(),
            exit_status: None,
            process,
        })
    }
}

use crate::{diagnostics::print_help, task::TaskDef, Error};
use crossterm::style::Color;
use std::{
    env::{self, Args},
    iter::Peekable,
    path::{Path, PathBuf},
};

pub fn parse_task(args: &mut Peekable<Args>, task_count: i32) -> Result<TaskDef, Error> {
    let error_title = || format!("invalid syntax (in task {})", task_count + 1);

    if !args.next().is_some_and(|arg| arg == "run") {
        return Err(Error {
            title: error_title(),
            message: "expected 'run'".into(),
            ..Error::default()
        });
    }

    let mut name = None;
    let mut workdir = None;
    let mut color = None;

    let mut parse_flag = |args: &mut Peekable<Args>, flag: &str| {
        match flag {
            "-n" => {
                name = Some(match args.next() {
                    Some(name) => name,
                    None => {
                        return Err(Error {
                            title: error_title(),
                            message: "expected task name after -n".into(),
                            ..Error::default()
                        })
                    }
                });
            }
            "-d" => {
                workdir = Some(match args.next() {
                    Some(name) => name,
                    None => {
                        return Err(Error {
                            title: error_title(),
                            message: "expected directory after -d".into(),
                            ..Error::default()
                        })
                    }
                });
            }
            "-c" => {
                let Some(color_arg) = args.next() else {
                    return Err(Error {
                        title: error_title(),
                        message: "expected color after -c".into(),
                        notes: vec![
                            "color syntax: RRGGBB (hex)".into(),
                            "if you have a # symbol, remove it".into(),
                        ],
                        ..Error::default()
                    });
                };

                let invalid_color = Error {
                    title: error_title(),
                    message: format!("invalid color '{color_arg}'"),
                    notes: vec!["color syntax: RRGGBB (hex)".into()],
                    ..Error::default()
                };

                if color_arg.len() != 6 {
                    return Err(invalid_color);
                }

                let Ok(r) = u8::from_str_radix(&color_arg[0..2], 16) else {
                    return Err(invalid_color);
                };
                let Ok(g) = u8::from_str_radix(&color_arg[2..4], 16) else {
                    return Err(invalid_color);
                };
                let Ok(b) = u8::from_str_radix(&color_arg[4..6], 16) else {
                    return Err(invalid_color);
                };

                color = Some(Color::Rgb { r, g, b });
            }
            _ => {
                return Err(Error {
                    title: error_title(),
                    message: format!("unknown flag {flag}"),
                    ..Error::default()
                });
            }
        }
        Ok(())
    };

    while args.peek().is_some_and(|arg| arg.starts_with('-')) {
        let flag = args.next().unwrap();
        parse_flag(args, &flag)?;
    }

    let Some(command) = args.next() else {
        return Err(Error {
            title: error_title(),
            message: "expected command after 'run' keyword".into(),
            ..Error::default()
        });
    };

    while args.peek().is_some_and(|arg| arg != "run") {
        let arg = args.next().unwrap();
        if arg.starts_with('-') {
            parse_flag(args, &arg)?;
        } else {
            return Err(Error {
                title: error_title(),
                message: format!("expected '-n <name>', '-d <dir>', '-c <color>' or 'run' after command, got '{arg}'"),
                notes: vec!["if your command includes spaces, please wrap it in quotes".into()],
                ..Error::default()
            });
        }
    }

    Ok(TaskDef {
        name: name.or_else(|| workdir.clone()).unwrap_or(format!(
            "#{}: {}",
            task_count + 1,
            &command,
        )),
        command,
        workdir: workdir
            .map(|path| PathBuf::from(path))
            .unwrap_or(env::current_dir().unwrap()),
        color,
    })
}

pub fn parse_args() -> Result<Vec<TaskDef>, Error> {
    let mut args = std::env::args().peekable();
    let name = args
        .next()
        .and_then(|p| {
            Path::new(&p)
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_owned())
        })
        .unwrap_or("congregation".into());

    let mut tasks = Vec::new();
    while args.peek().is_some() {
        if args.peek().is_some_and(|arg| {
            matches!(arg.as_str(), "-h" | "--help") || arg.to_lowercase().starts_with("h")
        }) {
            print_help(&name);
            return Ok(Vec::new());
        }

        tasks.push(parse_task(&mut args, tasks.len() as i32)?);
    }

    if tasks.is_empty() {
        return Err(Error {
            title: "no tasks specified!".into(),
            message: "please list some commands to execute using the 'run' keyword".into(),
            examples: vec![format!("{name} run 'echo hello'")],
            notes: vec![format!("run '{name} help' for more information")],
        });
    }

    Ok(tasks)
}

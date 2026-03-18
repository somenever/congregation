use std::io::Stdout;

use crossterm::{
    cursor, queue,
    style::{self, Stylize},
    terminal,
};

pub fn print_key(stdout: &mut Stdout, key: &str, name: &str) -> std::io::Result<()> {
    queue!(
        stdout,
        style::Print(format!(" {key} ").black().on_dark_grey()),
        style::Print(" "),
        style::Print(name),
        style::Print(" "),
    )
}

pub fn render_help_overlay(stdout: &mut Stdout) -> std::io::Result<()> {
    let (w, h) = terminal::size()?;

    const HELP_WIDTH: i32 = 48;

    enum HelpLine {
        Key((&'static str, &'static str)),
        Text(&'static str),
    }
    let lines = [
        HelpLine::Text(""),
        HelpLine::Text("NAVIGATION"),
        HelpLine::Key(("←↓↑→/hjkl", "move cursor")),
        HelpLine::Key(("u", "pgup")),
        HelpLine::Key(("d", "pgdown")),
        HelpLine::Key(("ctrl+↑/ctrl+k", "jump to next task")),
        HelpLine::Key(("ctrl+↓/ctrl+j", "jump to previous task")),
        HelpLine::Key(("q", "quit")),
        HelpLine::Text(""),
        HelpLine::Text("MANAGING TASKS"),
        HelpLine::Key(("space/enter", "collapse/expand task")),
        HelpLine::Text(""),
    ];
    let help_height = (lines.len() + 2) as i32;

    let help_x = w as i32 / 2 - HELP_WIDTH / 2;
    let help_y = h as i32 / 2 - help_height / 2;

    queue!(stdout, cursor::MoveTo(help_x as u16, help_y as u16),)?;
    queue!(
        stdout,
        style::Print("┌"),
        style::Print("─".repeat((HELP_WIDTH - 2) as usize)),
        style::Print("┐"),
    )?;

    queue!(
        stdout,
        style::Print("\n"),
        cursor::MoveToColumn(help_x as u16)
    )?;

    for line in lines {
        let width = match line {
            HelpLine::Key((name, action)) => name.chars().count() + 4 + action.chars().count(),
            HelpLine::Text(text) => text.len(),
        } as i32;
        queue!(stdout, style::Print("│  "))?;
        match line {
            HelpLine::Key((name, action)) => {
                print_key(stdout, name, action)?;
            }
            HelpLine::Text(text) => queue!(stdout, style::Print(text))?,
        }
        queue!(
            stdout,
            style::Print(" ".repeat((HELP_WIDTH - width - 4) as usize)),
            style::Print("│"),
            style::Print("\n"),
            cursor::MoveToColumn(help_x as u16)
        )?;
    }

    queue!(
        stdout,
        style::Print("└"),
        style::Print("─".repeat((HELP_WIDTH - 2) as usize)),
        style::Print("┘"),
    )?;

    Ok(())
}

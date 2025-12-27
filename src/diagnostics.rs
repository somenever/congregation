use crossterm::style::Stylize;
use indoc::printdoc;
use std::fmt::Display;
use std::fmt::Formatter;

#[derive(Default, Debug)]
pub struct Error {
    pub(crate) title: String,
    pub(crate) message: String,
    pub(crate) examples: Vec<String>,
    pub(crate) notes: Vec<String>,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        writeln!(f, "{}", self.title.as_str().red())?;

        if self.examples.is_empty() {
            writeln!(f, "{}", self.message)?;
        } else {
            writeln!(f, "")?;
            writeln!(f, "{}:", self.message)?;
            for example in &self.examples {
                writeln!(f, "{} {}", "│".dark_grey(), example)?;
            }
        }

        if !self.notes.is_empty() {
            for (i, note) in self.notes.iter().enumerate() {
                let prefix = "note:";
                let prefix_padding = " ".repeat(prefix.len());

                if i == 0 {
                    writeln!(f, "\n{} {}", prefix.green(), note.as_str().grey())?;
                } else {
                    writeln!(f, "\n{prefix_padding} {}", note.as_str().grey())?;
                }
            }
        }

        Ok(())
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self {
            title: "io error".into(),
            message: error.to_string(),
            examples: vec![],
            notes: vec![],
        }
    }
}

pub fn print_help(name: &str) {
    printdoc!("
    Run multiple parallel tasks with grouped output

    Usage: {name} <task> [<task> ...]

    Task syntax:
      run <command> [-d <dir>] [-n <name>] [-c <rrggbb>]

      Options:
        <command>     The shell command to run (wrap in quotes if it contains spaces)
        -d <dir>      Working directory for the task (defaults to the current working directory)
        -n <name>     Name of the task (used in task header, defaults to working directory or command)
        -c <rrggbb>   Hex RGB color for task name (e.g., ff8800, defaults to white)
    ");
}

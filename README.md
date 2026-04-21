# congregation
Run multiple tasks in parallel with beautiful, grouped output.
Inspired by [bun filter](https://bun.sh/docs/cli/filter) and [concurrently](https://www.npmjs.com/package/concurrently).

[![asciicast](services.gif)](https://asciinema.org/a/917043)

## Features
- Beautiful grouped layout with collapsible tasks
- Interactive TUI with vim-like keybindings
- Kill individual tasks or all at once
- Automatic task restart with configurable delay (`-r`)
- Customizable task colors and headers
- Cross-platform (Windows, Linux, macOS)

## Installation
Congregation can be installed using Cargo:
```shell
cargo install congregation
```

## Usage
To get started, run a simple task by using `congregation run`:
```shell
congregation run 'echo hello'
```

You can run multiple tasks simultaneously by adding another `run` keyword:
```shell
congregation run 'echo task 1' run 'echo task 2'
```

You may also add flags such as `-d`, `-n`, `-c` to customize the working directory, task name, and name color respectively:
```shell
congregation \
    run 'bun dev' -d frontend -r \
    run 'go run .' -n server -c ff0000 -r 5
```
The `-r` flag makes the task restart automatically on exit after an optional delay defaulting to 3 seconds. `-r 0` makes the task restart without a delay.

For more information, run `congregation help`.


# congregation
Run multiple tasks in parallel with beautiful, grouped output.
Inspired by [bun filter](https://bun.sh/docs/cli/filter) and [concurrently](https://www.npmjs.com/package/concurrently).

[![asciicast](services.gif)](https://asciinema.org/a/917043)

## Features
- Beautiful grouped layout with collapsible tasks
- Interactive TUI with vim-like keybindings
- Kill individual tasks or all at once
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
    run 'bun dev' -d frontend \
    run 'go run .' -n server -c ff0000
```

For more information, run `congregation help`.


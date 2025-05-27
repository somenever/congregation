# congregation
Run multiple tasks in parallel with beautiful, grouped output.
Inspired by [bun filter](https://bun.sh/docs/cli/filter) and [concurrently](https://www.npmjs.com/package/concurrently).

[![asciicast](examples/services.gif)](https://asciinema.org/a/9Cw5RDUCIejDIVcCkseDOF9Cj)

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
For more information, run `congregation help`.

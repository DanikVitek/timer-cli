# Timer CLI

A simple command line timer application written in Rust.

## Help

```bash
timer --help
```
```
A simple CLI timer application using crossterm and tokio

Usage: timer <[[[d:]h:]m:]s duration>

Arguments:
  <[[[d:]h:]m:]s duration>  Duration in the format "[[[d:]h:]m:]s" (e.g., "1:2:3:4" for 1 day, 2 hours, 3 minutes, and 4 seconds)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## Installation

### Using Cargo

You can install the Timer CLI using Cargo, Rust's package manager. Run the following command:

```bash
cargo install --git https://github.com/DanikVitek/timer-cli.git --locked
```

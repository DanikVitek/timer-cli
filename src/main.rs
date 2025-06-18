use core::{fmt, str::FromStr, time::Duration};
use std::{io, process::ExitCode};

use clap::Parser;
use crossterm::{
    cursor,
    event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    style, terminal,
};
use futures_util::{FutureExt, TryStreamExt};
use human_errors::{Error, system_with_internal, user, user_with_cause, user_with_internal};

fn main() -> ExitCode {
    let Args {
        duration: ColonSeparatedDuration(duration),
    } = Args::parse();

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| {
            system_with_internal(
                "Failed to build the runtime",
                "Try notifying the developer",
                err,
            )
        }) {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let result = rt.block_on(run_timer(duration));

    if let Err(e) = result {
        eprintln!("{e}");
        return ExitCode::FAILURE;
    }

    return ExitCode::SUCCESS;
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(
        name = "[[[d:]h:]m:]s duration",
        help = "Duration in the format \"[[[d:]h:]m:]s\" (e.g., \"1:2:3:4\" for 1 day, 2 hours, 3 minutes, and 4 seconds)",
    )]
    duration: ColonSeparatedDuration,
}

#[derive(Debug, Clone, Copy)]
struct ColonSeparatedDuration(Duration);

impl FromStr for ColonSeparatedDuration {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_duration(s).map(Self)
    }
}

async fn run_timer(mut duration: Duration) -> Result<(), Error> {
    let initial_duration = duration;

    let tick_period = Duration::from_secs(1);
    let mut interval = tokio::time::interval(tick_period);

    let mut writer = std::io::stderr();
    crossterm::execute!(
        &mut writer,
        terminal::EnterAlternateScreen,
        cursor::Hide,
        cursor::MoveTo(0, 0)
    )
    .and_then(|_| crossterm::terminal::enable_raw_mode())
    .map_err(|err| {
        system_with_internal(
            "Failed to enter alternate screen",
            "Try notifying the developer",
            err,
        )
    })?;

    let mut event_stream = EventStream::new();
    let mut paused = false;
    let mut paused_print = true;

    loop {
        let event = event_stream.try_next().fuse();
        let tick = interval.tick().fuse();

        tokio::select! {
            maybe_event = event => match process_event_branch(
                maybe_event,
                &mut writer,
                &mut paused,
                &mut paused_print,
                initial_duration,
                duration
            ) {
                ControlFlow::Return(res) => return res,
                ControlFlow::Break => break,
                ControlFlow::Continue => continue,
            },
            _ = tick => {
                if paused {
                    crossterm::execute!(
                        writer,
                        terminal::BeginSynchronizedUpdate,
                    )
                    .and_then(|_| print_paused(&mut writer, &mut paused_print))
                    .map_err(|err| {
                        system_with_internal(
                            "Failed to write to the terminal",
                            "Try notifying the developer",
                            err,
                        )
                    })?;
                    continue;
                }
                if duration.is_zero() {
                    break;
                }
                crossterm::execute!(
                    writer,
                    terminal::BeginSynchronizedUpdate,
                    terminal::Clear(terminal::ClearType::All),
                    cursor::MoveTo(0, 0),
                    style::Print(format_args!("Remaining time: {}", DurationDisplay(duration))),
                    terminal::EndSynchronizedUpdate,
                )
                .map_err(|err| {
                    system_with_internal(
                        "Failed to write to the terminal",
                        "Try notifying the developer",
                        err,
                    )
                })?;
                duration -= tick_period;
            }
        }
    }

    crossterm::execute!(
        writer,
        cursor::Show,
        terminal::LeaveAlternateScreen,
        style::Print("Timer finished!\n"),
    )
    .and_then(|_| crossterm::terminal::disable_raw_mode())
    .map_err(|err| {
        system_with_internal(
            "Failed to clear the terminal",
            "Try notifying the developer",
            err,
        )
    })
}

enum ControlFlow {
    Return(Result<(), Error>),
    Break,
    Continue,
}

#[inline]
fn process_event_branch(
    maybe_event: io::Result<Option<Event>>,
    writer: &mut io::Stderr,
    paused: &mut bool,
    paused_print: &mut bool,
    initial_duration: Duration,
    duration: Duration,
) -> ControlFlow {
    match maybe_event {
        Ok(None) => ControlFlow::Break,
        Ok(Some(event)) => match event {
            Event::Key(
                KeyEvent {
                    code: KeyCode::Char('q'),
                    kind: KeyEventKind::Press,
                    ..
                }
                | KeyEvent {
                    code: KeyCode::Char('c'),
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::CONTROL,
                    ..
                },
            ) => ControlFlow::Return(
                crossterm::execute!(
                    writer,
                    cursor::Show,
                    terminal::LeaveAlternateScreen,
                    style::Print(format_args!(
                        "Timer stopped by user at {}, after {}.\n",
                        DurationDisplay(duration),
                        DurationDisplay(initial_duration - duration)
                    )),
                )
                .and_then(|_| crossterm::terminal::disable_raw_mode())
                .map_err(|err| {
                    system_with_internal(
                        "Failed to clear the terminal",
                        "Try notifying the developer",
                        err,
                    )
                }),
            ),
            Event::Key(KeyEvent {
                code: KeyCode::Char('p'),
                kind: KeyEventKind::Press,
                ..
            }) => {
                *paused = !*paused;
                let res = if *paused {
                    print_paused(writer, paused_print)
                } else {
                    crossterm::execute!(
                        writer,
                        terminal::BeginSynchronizedUpdate,
                        cursor::MoveTo(0, 1),
                        terminal::Clear(terminal::ClearType::CurrentLine),
                        cursor::MoveTo(0, 2),
                        terminal::Clear(terminal::ClearType::CurrentLine),
                        terminal::EndSynchronizedUpdate,
                    )
                }
                .map_err(|err| {
                    system_with_internal(
                        "Failed to write to the terminal",
                        "Try notifying the developer",
                        err,
                    )
                });
                if res.is_err() {
                    return ControlFlow::Return(res);
                }
                ControlFlow::Continue
            }
            _ => ControlFlow::Continue,
        },
        Err(err) => ControlFlow::Return(Err(system_with_internal(
            "Failed to read events",
            "Try notifying the developer",
            err,
        ))),
    }
}

fn print_paused(writer: &mut std::io::Stderr, print: &mut bool) -> io::Result<()> {
    if *print {
        crossterm::execute!(
            writer,
            cursor::MoveTo(0, 1),
            style::Print("PAUSED"),
            cursor::MoveTo(0, 2),
            style::Print("Timer is paused. Press 'p' to resume or 'q' to quit."),
        )
        .inspect(|_| *print = false)
    } else {
        crossterm::execute!(
            writer,
            cursor::MoveTo(0, 1),
            terminal::Clear(terminal::ClearType::CurrentLine),
            cursor::MoveTo(0, 2),
            style::Print("Timer is paused. Press 'p' to resume or 'q' to quit."),
        )
        .inspect(|_| *print = true)
    }
}

#[derive(Debug, Clone, Copy)]
struct DurationDisplay(Duration);

impl fmt::Display for DurationDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total_seconds = self.0.as_secs();
        let days = total_seconds / 86400;
        let hours = (total_seconds % 86400) / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;

        if days > 0 {
            write!(f, "{}d ", days)?;
        }
        if hours > 0 || days > 0 {
            write!(f, "{hours}h ")?;
        }
        if minutes > 0 || hours > 0 || days > 0 {
            write!(f, "{minutes}m ")?;
        }
        write!(f, "{seconds}s")
    }
}

fn parse_duration(duration_str: &str) -> Result<Duration, Error> {
    let parts = duration_str.rsplit(':').take(5).collect::<Box<[_]>>();
    if parts.is_empty() {
        return Err(user_with_cause(
            "Failed to parse the duration",
            "Provide the duration in the following format: \"[[[d:]h:]m:]s\"",
            user(
                "Missing parts",
                "Make sure to provide at least the seconds part of the duration",
            ),
        ));
    }
    if parts.len() > 4 {
        return Err(user_with_cause(
            "Failed to parse the duration",
            "Provide the duration in the following format: \"[[[d:]h:]m:]s\"",
            user(
                "Too many parts",
                "Make sure to provide at most 4 parts for days, hours, minutes, and seconds",
            ),
        ));
    }
    let s_ms_part = parts[0];

    let (s_part, ms_part) = {
        let parts = s_ms_part.split('.').take(3).collect::<Box<[_]>>();
        match parts.len() {
            0 => unreachable!(),
            1 => (parts[0], None),
            2 => (parts[0], Some(parts[1])),
            _ => {
                return Err(user_with_cause(
                    "Failed to parse the duration",
                    "Provide the duration in the following format: \"[[[d:]h:]m:]s\"",
                    user(
                        "Too many parts in seconds.milliseconds",
                        "Make sure to provide at most one dot in the seconds part",
                    ),
                ));
            }
        }
    };

    let s = Duration::from_secs(s_part.parse().map_err(|err| {
        user_with_internal(
            "Failed to parse the seconds part",
            "Make sure to provide a valid number for the seconds part",
            err,
        )
    })?);
    let ms = if let Some(ms_part) = ms_part {
        Duration::from_millis(ms_part.parse().map_err(|err| {
            user_with_internal(
                "Failed to parse the milliseconds part",
                "Make sure to provide a valid number for the milliseconds part",
                err,
            )
        })?)
    } else {
        Duration::ZERO
    };

    let mut duration = s + ms;
    for (i, part) in parts.iter().copied().enumerate().skip(1) {
        let value = part.parse::<u64>().map_err(|err| {
            user_with_internal(
                "Failed to parse a duration part",
                "Make sure to provide a valid number for the duration part",
                err,
            )
        })?;
        duration = match i {
            1 => duration
                .checked_add(Duration::from_secs(value.checked_mul(60).ok_or_else(
                    || {
                        user_with_cause(
                            "Duration overflow",
                            "The provided duration is too large to be represented",
                            user(
                                "Overflow in minutes",
                                "Make sure the value is within a reasonable range",
                            ),
                        )
                    },
                )?))
                .ok_or_else(|| {
                    user_with_cause(
                        "Duration overflow",
                        "The provided duration is too large to be represented",
                        user(
                            "Overflow in minutes",
                            "Make sure the value is within a reasonable range",
                        ),
                    )
                })?, // minutes
            2 => duration
                .checked_add(Duration::from_secs(value.checked_mul(3600).ok_or_else(
                    || {
                        user_with_cause(
                            "Duration overflow",
                            "The provided duration is too large to be represented",
                            user(
                                "Overflow in hours",
                                "Make sure the value is within a reasonable range",
                            ),
                        )
                    },
                )?))
                .ok_or_else(|| {
                    user_with_cause(
                        "Duration overflow",
                        "The provided duration is too large to be represented",
                        user(
                            "Overflow in hours",
                            "Make sure the value is within a reasonable range",
                        ),
                    )
                })?, // hours
            3 => duration
                .checked_add(Duration::from_secs(value.checked_mul(86400).ok_or_else(
                    || {
                        user_with_cause(
                            "Duration overflow",
                            "The provided duration is too large to be represented",
                            user(
                                "Overflow in days",
                                "Make sure the value is within a reasonable range",
                            ),
                        )
                    },
                )?))
                .ok_or_else(|| {
                    user_with_cause(
                        "Duration overflow",
                        "The provided duration is too large to be represented",
                        user(
                            "Overflow in days",
                            "Make sure the value is within a reasonable range",
                        ),
                    )
                })?, // days
            _ => {
                return Err(user(
                    "Invalid duration part",
                    "Make sure to provide a valid number for the duration part",
                ));
            }
        };
    }

    Ok(duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Args::command().debug_assert();
    }
}

use core::fmt;
use std::{process::ExitCode, time::Duration};

use crossterm::{cursor, style, terminal};
use human_errors::{Error, system_with_internal, user, user_with_cause, user_with_internal};

fn main() -> ExitCode {
    let args: Box<[String]> = std::env::args().collect();

    if args.len() != 2 {
        let usage = format!("Usage: {} <duration in format d:h:m:s>", args[0]);
        let err = user("Invalid usage of the tool", &usage);
        eprintln!("{err}");
        return ExitCode::FAILURE;
    }

    let duration_str = args[1].as_str();

    let mut duration = match parse_duration(duration_str) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

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

    let result: Result<(), Error> = rt.block_on(async move {
        let tick_period = Duration::from_secs(1);
        let mut interval = tokio::time::interval(tick_period);

        let mut writer = std::io::stderr();
        crossterm::execute!(
            &mut writer,
            terminal::EnterAlternateScreen,
            cursor::Hide,
            cursor::MoveTo(0, 0)
        )
        .map_err(|err| {
            system_with_internal(
                "Failed to enter alternate screen",
                "Try notifying the developer",
                err,
            )
        })?;

        while duration > Duration::ZERO {
            tokio::select! {
                _ = interval.tick() => {
                    crossterm::execute!(
                        writer,
                        terminal::BeginSynchronizedUpdate,
                        terminal::Clear(terminal::ClearType::CurrentLine),
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
                _ = tokio::signal::ctrl_c() => return crossterm::execute!(
                    writer,
                    cursor::Show,
                    terminal::LeaveAlternateScreen,
                    style::Print(format_args!("Timer stopped by user at {}.\n", DurationDisplay(duration))),
                ).map_err(|err| {
                    system_with_internal(
                        "Failed to clear the terminal",
                        "Try notifying the developer",
                        err,
                    )
                }),
            }
        }

        crossterm::execute!(
            writer,
            cursor::Show,
            terminal::LeaveAlternateScreen,
            style::Print("Timer finished!\n"),
        ).map_err(|err| {
            system_with_internal(
                "Failed to clear the terminal",
                "Try notifying the developer",
                err,
            )
        })
    });

    if let Err(e) = result {
        eprintln!("{e}");
        return ExitCode::FAILURE;
    }

    return ExitCode::SUCCESS;
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
            write!(f, "{:02}h ", hours)?;
        }
        if minutes > 0 || hours > 0 || days > 0 {
            write!(f, "{:02}m ", minutes)?;
        }
        write!(f, "{:02}s", seconds)
    }
}

fn parse_duration(duration_str: &str) -> Result<Duration, Error> {
    let parts = duration_str.rsplit(':').take(5).collect::<Box<[_]>>();
    if parts.is_empty() {
        return Err(user_with_cause(
            "Failed to parse the duration",
            "Provide the duration in the following format: \"d:h:m:s\"",
            user(
                "Missing parts",
                "Make sure to provide at least the seconds part of the duration",
            ),
        ));
    }
    if parts.len() > 4 {
        return Err(user_with_cause(
            "Failed to parse the duration",
            "Provide the duration in the following format: \"d:h:m:s.ms\"",
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
                    "Provide the duration in the following format: \"d:h:m:s.ms\"",
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

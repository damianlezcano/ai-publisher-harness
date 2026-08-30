//! Generic test double for a supervised child, controlled by `FAKE_PROCESS_MODE`
//! and optional `FAKE_PROCESS_LINE` / `FAKE_PROCESS_LINE2`.

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

fn main() {
    let mode = std::env::var("FAKE_PROCESS_MODE").unwrap_or_default();
    let line = std::env::var("FAKE_PROCESS_LINE").unwrap_or_default();
    match mode.as_str() {
        "print" => {
            println!("{line}");
            sleep_forever();
        }
        "print_stderr" => {
            eprintln!("{line}");
            let _ = io::stderr().flush();
            sleep_forever();
        }
        "print_then_exit" => {
            println!("{line}");
        }
        "print_two" => {
            let line2 = std::env::var("FAKE_PROCESS_LINE2").unwrap_or_default();
            println!("{line}");
            println!("{line2}");
            sleep_forever();
        }
        "exit" => {
            println!("no output");
            std::process::exit(1);
        }
        "flood" => {
            for i in 0..50_000 {
                println!("flood line {i}");
            }
            sleep_forever();
        }
        "malformed" => {
            let mut stdout = io::stdout();
            let _ = stdout.write_all(b"bad \xff bytes\n");
            let _ = stdout.flush();
            println!("{line}");
            sleep_forever();
        }
        "silent" => sleep_forever(),
        "env" => {
            let mut entries: Vec<String> = std::env::vars()
                .map(|(key, value)| format!("{key}={value}"))
                .collect();
            entries.sort();
            let mut stdout = io::stdout();
            for entry in entries {
                let _ = writeln!(stdout, "{entry}");
            }
            let _ = stdout.flush();
            sleep_forever();
        }
        "ignore_term" => {
            println!("{line}");
            ignore_sigterm();
            sleep_forever();
        }
        _ => sleep_forever(),
    }
}

fn ignore_sigterm() {
    #[cfg(unix)]
    {
        // SAFETY: this test binary only ignores SIGTERM so stop() can exercise SIGKILL.
        let _ = unsafe {
            nix::sys::signal::sigaction(
                nix::sys::signal::Signal::SIGTERM,
                &nix::sys::signal::SigAction::new(
                    nix::sys::signal::SigHandler::SigIgn,
                    nix::sys::signal::SaFlags::empty(),
                    nix::sys::signal::SigSet::empty(),
                ),
            )
        };
    }
}

fn sleep_forever() -> ! {
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

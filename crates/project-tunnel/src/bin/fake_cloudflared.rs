//! Test double for `cloudflared`, controlled by `FAKE_CLOUDFLARED_MODE`.

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

const BANNER: &str = "2026-01-01T00:00:00Z INF Your quick Tunnel has been created!";
const URL_LINE: &str = "2026-01-01T00:00:01Z INF https://fake-123.trycloudflare.com";

fn main() {
    let mode = std::env::var("FAKE_CLOUDFLARED_MODE").unwrap_or_default();
    match mode.as_str() {
        "url" => {
            println!("{BANNER}");
            println!("{URL_LINE}");
            sleep_forever();
        }
        "url_stderr" => {
            eprintln!("{URL_LINE}");
            let _ = io::stderr().flush();
            sleep_forever();
        }
        "url_then_exit" => {
            println!("{URL_LINE}");
        }
        "exit_before_url" => {
            println!("starting fake cloudflared");
            println!("no public hostname in this mode");
            std::process::exit(1);
        }
        "garbage" => {
            println!("http://evil.example.com");
            println!("https://not-trycloudflare.example.com/x");
            sleep_forever();
        }
        "garbage_then_url" => {
            println!("http://evil.example.com");
            println!("https://not-trycloudflare.example.com/x");
            println!("{URL_LINE}");
            sleep_forever();
        }
        "url_ignore_term" => {
            println!("{BANNER}");
            println!("{URL_LINE}");
            ignore_sigterm();
            sleep_forever();
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
            println!("{URL_LINE}");
            sleep_forever();
        }
        "silent" => sleep_forever(),
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

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
        "serve_http" => {
            let delay_ms = std::env::var("FAKE_PROCESS_DELAY_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            if delay_ms > 0 {
                thread::sleep(Duration::from_millis(delay_ms));
            }
            serve_http();
        }
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

/// `serve_http` helper: act like `opencode serve` enough for the readiness
/// probe — bind the `--port` argument (or `FAKE_PROCESS_PORT`) on loopback and
/// answer `GET /global/health` with a healthy, in-range version. Reads the
/// optional `FAKE_PROCESS_DELAY_MS` env to widen the boot window so concurrent
/// `ensure_ready` callers race a not-yet-listening server deterministically.
fn serve_http() -> ! {
    let port = parse_port_from_args();
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))
        .unwrap_or_else(|err| panic!("fake-process serve_http: cannot bind {port}: {err}"));
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            continue;
        };
        let _ = handle_health(&mut stream);
    }
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

fn parse_port_from_args() -> u16 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    for (index, arg) in args.iter().enumerate() {
        if arg == "--port"
            && let Some(port) = args.get(index + 1)
        {
            return port.parse().expect("fake-process serve_http: valid --port");
        }
    }
    if let Ok(port) = std::env::var("FAKE_PROCESS_PORT") {
        return port
            .parse()
            .expect("fake-process serve_http: valid FAKE_PROCESS_PORT");
    }
    panic!("fake-process serve_http: requires --port <n> in argv or FAKE_PROCESS_PORT")
}

fn handle_health(stream: &mut std::net::TcpStream) -> std::io::Result<()> {
    use std::io::{BufRead, BufReader, Read, Write};
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    let _ = reader.read_line(&mut request_line)?;
    let mut sink = [0u8; 4096];
    let _ = reader.read(&mut sink);
    let body = if request_line.starts_with("GET /global/health") {
        r#"{"healthy":true,"version":"1.18.25"}"#
    } else {
        r#"{"error":"not found"}"#
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())
}

fn sleep_forever() -> ! {
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

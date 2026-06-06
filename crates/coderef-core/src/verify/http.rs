//! HTTP verification.
//!
//! Strategy: HEAD by default; fall back to GET if the server returns
//! 405 Method Not Allowed (some sites have HEAD disabled). TLS
//! validation is always on (`ureq` with `rustls` default-on per
//! DESIGN.md §18 — no opt-out).
//!
//! Retries follow the configured exponential backoff for transport
//! errors only. A non-accepted status code does *not* retry: it's a
//! definitive answer from the server.

use std::thread;
use std::time::Duration;

use super::{VerifyOptions, VerifyOutcome};

const DEFAULT_ACCEPT_STATUS: std::ops::RangeInclusive<u16> = 200..=299;

pub fn verify_http(url: &str, opts: &VerifyOptions) -> VerifyOutcome {
    let agent = build_agent(opts.timeout);

    let attempts = opts.backoff.max_attempts.max(1);
    let mut last_transport_error: Option<String> = None;

    for attempt in 0..attempts {
        if attempt > 0 {
            thread::sleep(opts.backoff.delay_for(attempt - 1));
        }

        match try_once(&agent, url, opts) {
            Outcome::Status(s) => {
                return classify_status(s, &opts.accept_status);
            }
            Outcome::Transport(reason) => {
                last_transport_error = Some(reason);
            }
        }
    }

    VerifyOutcome::BrokenNetwork {
        reason: last_transport_error
            .unwrap_or_else(|| "no attempts succeeded; reason unknown".into()),
    }
}

enum Outcome {
    /// Server returned a definitive HTTP status.
    Status(u16),
    /// Transport-level failure (DNS, connect, TLS, timeout, etc.).
    Transport(String),
}

fn try_once(agent: &ureq::Agent, url: &str, _opts: &VerifyOptions) -> Outcome {
    // Try HEAD first.
    match agent.head(url).call() {
        Ok(resp) => Outcome::Status(resp.status()),
        Err(ureq::Error::Status(405, _)) => {
            // Method Not Allowed — fall back to GET.
            match agent.get(url).call() {
                Ok(resp) => Outcome::Status(resp.status()),
                Err(ureq::Error::Status(code, _)) => Outcome::Status(code),
                Err(ureq::Error::Transport(t)) => Outcome::Transport(t.to_string()),
            }
        }
        Err(ureq::Error::Status(code, _)) => Outcome::Status(code),
        Err(ureq::Error::Transport(t)) => Outcome::Transport(t.to_string()),
    }
}

fn classify_status(status: u16, accept: &[u16]) -> VerifyOutcome {
    let accepted = if accept.is_empty() {
        DEFAULT_ACCEPT_STATUS.contains(&status)
    } else {
        accept.contains(&status)
    };
    if accepted {
        VerifyOutcome::Ok
    } else {
        VerifyOutcome::BrokenStatus { status }
    }
}

fn build_agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .timeout_write(timeout)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::BackoffOptions;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    /// Mock HTTP/1.1 server that returns a configurable status to each
    /// connection. Returns the chosen port + a counter of received
    /// requests (visible to the test so it can assert retry behaviour).
    fn spawn_mock(status: u16, body: &'static str) -> (u16, Arc<AtomicU32>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 4096];
                // Best-effort read of the request line; if the client
                // already sent the bytes, we get them, otherwise we may
                // get 0 — either is fine, we just want to send a reply.
                let _ = stream.read(&mut buf);
                c.fetch_add(1, Ordering::SeqCst);
                let status_text = match status {
                    200 => "OK",
                    404 => "Not Found",
                    405 => "Method Not Allowed",
                    500 => "Internal Server Error",
                    _ => "Status",
                };
                let resp = format!(
                    "HTTP/1.1 {status} {status_text}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        (port, counter)
    }

    fn opts_for_port() -> VerifyOptions {
        VerifyOptions {
            timeout: Duration::from_millis(500),
            accept_status: vec![],
            backoff: BackoffOptions {
                max_attempts: 1,
                ..BackoffOptions::default()
            },
            workspace_root: ".".into(),
        }
    }

    #[test]
    fn test_http_200_is_ok() {
        let (port, _hits) = spawn_mock(200, "");
        let url = format!("http://127.0.0.1:{port}/");
        let result = verify_http(&url, &opts_for_port());
        assert_eq!(result, VerifyOutcome::Ok);
    }

    #[test]
    fn test_http_404_is_broken_status() {
        let (port, _hits) = spawn_mock(404, "");
        let url = format!("http://127.0.0.1:{port}/");
        let result = verify_http(&url, &opts_for_port());
        assert_eq!(result, VerifyOutcome::BrokenStatus { status: 404 });
    }

    #[test]
    fn test_http_500_is_broken_status() {
        let (port, _hits) = spawn_mock(500, "");
        let url = format!("http://127.0.0.1:{port}/");
        let result = verify_http(&url, &opts_for_port());
        assert_eq!(result, VerifyOutcome::BrokenStatus { status: 500 });
    }

    #[test]
    fn test_http_405_falls_back_to_get_and_succeeds() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let mut hit = 0;
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = std::str::from_utf8(&buf[..n]).unwrap_or("");
                hit += 1;
                // First request (HEAD) → 405; second (GET) → 200.
                let resp = if hit == 1 && req.starts_with("HEAD") {
                    "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_string()
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                };
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        let url = format!("http://127.0.0.1:{port}/");
        let result = verify_http(&url, &opts_for_port());
        assert_eq!(result, VerifyOutcome::Ok);
    }

    #[test]
    fn test_http_custom_accept_status_overrides_default_2xx() {
        let (port, _hits) = spawn_mock(418, ""); // I'm a teapot
        let url = format!("http://127.0.0.1:{port}/");
        let opts = VerifyOptions {
            accept_status: vec![418],
            ..opts_for_port()
        };
        assert_eq!(verify_http(&url, &opts), VerifyOutcome::Ok);
    }

    #[test]
    fn test_http_connection_refused_is_broken_network() {
        // Bind a port then immediately drop it — port is free → refused.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let url = format!("http://127.0.0.1:{port}/");
        let result = verify_http(&url, &opts_for_port());
        assert!(matches!(result, VerifyOutcome::BrokenNetwork { .. }));
    }

    #[test]
    fn test_http_invalid_url_is_broken_network() {
        let result = verify_http("http://not-a-real-host-9999.invalid/", &opts_for_port());
        assert!(matches!(result, VerifyOutcome::BrokenNetwork { .. }));
    }

    #[test]
    fn test_http_retries_transport_errors_up_to_max_attempts() {
        // Connect to a closed port; configure 3 attempts. Each attempt
        // counts even though there's no server to count them — we
        // instead check that retries happen by measuring total time.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let url = format!("http://127.0.0.1:{port}/");
        let opts = VerifyOptions {
            timeout: Duration::from_millis(100),
            accept_status: vec![],
            backoff: BackoffOptions {
                max_attempts: 3,
                base: Duration::from_millis(20),
                multiplier: 1.0, // flat to keep test fast
                max_delay: Duration::from_millis(20),
            },
            workspace_root: ".".into(),
        };
        let start = std::time::Instant::now();
        let result = verify_http(&url, &opts);
        let elapsed = start.elapsed();
        // 2 sleeps of 20ms between 3 attempts = at least 40ms.
        assert!(elapsed >= Duration::from_millis(40), "elapsed: {elapsed:?}");
        assert!(matches!(result, VerifyOutcome::BrokenNetwork { .. }));
    }
}

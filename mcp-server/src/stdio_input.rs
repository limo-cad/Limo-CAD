//! Bound queued requests and individual lines before JSON or CAD allocation.
use std::io::{BufRead, Read};

pub(super) const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
pub(super) const QUEUED_REQUESTS: usize = 1;

pub(super) enum StdinEvent {
    Line(String),
    Error(String),
    Eof,
}

pub(super) fn read_lines(mut reader: impl BufRead, mut send: impl FnMut(StdinEvent) -> bool) {
    loop {
        let mut bytes = Vec::new();
        // Take limits allocation even when a client never terminates a line.
        let read = (&mut reader)
            .take((MAX_REQUEST_BYTES + 1) as u64)
            .read_until(b'\n', &mut bytes);
        match read {
            Err(error) => {
                send(StdinEvent::Error(format!("MCP stdin: {error}")));
                return;
            }
            Ok(0) => {
                send(StdinEvent::Eof);
                return;
            }
            Ok(_) if bytes.len() > MAX_REQUEST_BYTES => {
                send(StdinEvent::Error(format!(
                    "MCP request exceeds the {MAX_REQUEST_BYTES}-byte line limit"
                )));
                return;
            }
            Ok(_) => {}
        }
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        let line = match String::from_utf8(bytes) {
            Ok(line) => line,
            Err(_) => {
                send(StdinEvent::Error("MCP stdin must be valid UTF-8".into()));
                return;
            }
        };
        if !send(StdinEvent::Line(line)) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io, sync::mpsc, thread, time::Duration};

    #[test]
    fn normal_crlf_final_lines_and_eof_are_preserved() {
        let mut lines = Vec::new();
        let mut eof = false;
        read_lines(io::Cursor::new(b"one\r\n\ntwo"), |event| {
            match event {
                StdinEvent::Line(line) => lines.push(line),
                StdinEvent::Eof => eof = true,
                StdinEvent::Error(error) => panic!("{error}"),
            }
            true
        });
        assert_eq!(lines, ["one", "", "two"]);
        assert!(eof);
    }

    #[test]
    fn accepts_the_boundary_and_rejects_an_unterminated_oversized_line() {
        let mut events = Vec::new();
        read_lines(
            io::BufReader::new(io::repeat(b'x').take(MAX_REQUEST_BYTES as u64)),
            |event| {
                events.push(event);
                true
            },
        );
        assert!(matches!(&events[0], StdinEvent::Line(line) if line.len() == MAX_REQUEST_BYTES));
        assert!(matches!(events[1], StdinEvent::Eof));
        let mut bytes_read = 0;
        struct Counted<'a>(&'a mut usize);
        impl io::Read for Counted<'_> {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                out.fill(b'x');
                *self.0 += out.len();
                Ok(out.len())
            }
        }
        let mut oversized = Vec::new();
        read_lines(
            io::BufReader::with_capacity(32, Counted(&mut bytes_read)),
            |event| {
                oversized.push(event);
                true
            },
        );
        assert_eq!(oversized.len(), 1);
        assert!(
            matches!(&oversized[0], StdinEvent::Error(message) if message.contains("line limit"))
        );
        assert!(bytes_read <= MAX_REQUEST_BYTES + 32);
    }

    #[test]
    fn invalid_utf8_and_read_errors_stop_without_disclosing_input() {
        for input in [b"\xffprivate".as_slice(), b"\xff\n"] {
            read_lines(io::Cursor::new(input), |event| {
                assert!(
                    matches!(event, StdinEvent::Error(message) if message == "MCP stdin must be valid UTF-8")
                );
                true
            });
        }
        struct Failed;
        impl io::Read for Failed {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("owned read failure"))
            }
        }
        read_lines(io::BufReader::new(Failed), |event| {
            assert!(
                matches!(event, StdinEvent::Error(message) if message.contains("owned read failure"))
            );
            true
        });
    }

    #[test]
    fn a_slow_dispatcher_applies_backpressure_and_disconnect_releases_the_reader() {
        let (tx, rx) = mpsc::sync_channel(QUEUED_REQUESTS);
        let (progress, observed) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut sent = 0;
            read_lines(io::Cursor::new(b"first\nsecond\nthird\n"), |event| {
                if tx.send(event).is_err() {
                    return false;
                }
                sent += 1;
                progress.send(sent).unwrap();
                true
            });
        });
        assert_eq!(observed.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        assert!(observed.recv_timeout(Duration::from_millis(20)).is_err());
        assert!(matches!(rx.recv().unwrap(), StdinEvent::Line(line) if line == "first"));
        assert_eq!(observed.recv_timeout(Duration::from_secs(2)).unwrap(), 2);
        drop(rx);
        reader.join().unwrap();
    }
}

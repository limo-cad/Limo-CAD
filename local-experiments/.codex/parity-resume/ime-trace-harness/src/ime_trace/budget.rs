//! A shared byte budget for formatter writes, including the truncation marker.
use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
};

const TRUNCATED: &[u8] = b"NBCAD_NATIVE_IME_TRACE truncated: byte budget exhausted\n";

pub(super) struct Bounded<W>(Arc<Mutex<State<W>>>);

struct State<W> {
    output: W,
    remaining: usize,
    truncated: bool,
}

impl<W> Clone for Bounded<W> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<W> Bounded<W> {
    pub(super) fn new(output: W, limit: usize) -> Self {
        assert!(limit >= TRUNCATED.len());
        Self(Arc::new(Mutex::new(State {
            output,
            remaining: limit - TRUNCATED.len(),
            truncated: false,
        })))
    }
}

impl<W: Write> Write for Bounded<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| io::Error::other("IME trace writer poisoned"))?;
        if !state.truncated {
            if bytes.len() <= state.remaining {
                state.output.write_all(bytes)?;
                state.remaining -= bytes.len();
            } else {
                state.truncated = true;
                // Drop the entire over-budget formatter chunk, preserving UTF-8.
                state.output.write_all(TRUNCATED)?;
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("IME trace writer poisoned"))?
            .output
            .flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writers_share_one_budget_and_one_truncation_marker() {
        let mut first = Bounded::new(Vec::new(), TRUNCATED.len() + 5);
        let mut second = first.clone();
        first.write_all(b"abc").unwrap();
        second.write_all(b"de").unwrap();
        first.write_all(b"over").unwrap();
        second.write_all(b"later").unwrap();
        let state = first.0.lock().unwrap();
        assert_eq!(state.output, [b"abcde".as_slice(), TRUNCATED].concat());
        assert_eq!(state.output.len(), TRUNCATED.len() + 5);
    }

    #[test]
    fn over_budget_utf8_is_dropped_as_a_whole_chunk() {
        let mut writer = Bounded::new(Vec::new(), TRUNCATED.len() + 2);
        writer.write_all("春".as_bytes()).unwrap();
        let state = writer.0.lock().unwrap();
        assert_eq!(state.output, TRUNCATED);
        assert!(std::str::from_utf8(&state.output).is_ok());
    }

    #[test]
    fn underlying_write_errors_are_reported() {
        struct Failing;
        impl Write for Failing {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut writer = Bounded::new(Failing, TRUNCATED.len() + 10);
        assert_eq!(
            writer.write_all(b"callback").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }
}

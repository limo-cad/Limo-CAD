//! Diagnostic retention must not change or block the child's protocol stream.
use std::io::{self, Read, Write};

pub(super) const MAX_LOG_BYTES: usize = 2 * 1024 * 1024;
const TRUNCATED: &[u8] = b"\n[diagnostic log truncated]\n";

pub(super) struct BoundedLog<W> {
    writer: W,
    remaining: usize,
    truncated: bool,
}

impl<W: Write> BoundedLog<W> {
    pub(super) fn new(writer: W, limit: usize) -> Self {
        assert!(limit >= TRUNCATED.len());
        Self {
            writer,
            remaining: limit - TRUNCATED.len(),
            truncated: false,
        }
    }
}

impl<W: Write> Write for BoundedLog<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.truncated {
            return Ok(bytes.len());
        }
        let retained = self.remaining.min(bytes.len());
        self.writer.write_all(&bytes[..retained])?;
        self.remaining -= retained;
        if retained < bytes.len() {
            self.writer.write_all(TRUNCATED)?;
            self.truncated = true;
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

pub(super) fn drain(mut source: impl Read, writer: impl Write) -> io::Result<()> {
    let mut log = BoundedLog::new(writer, MAX_LOG_BYTES);
    let result = io::copy(&mut source, &mut log).and_then(|_| log.flush());
    if result.is_err() {
        // A failed diagnostic disk must not fill the pipe and stall the child.
        let _ = io::copy(&mut source, &mut io::sink());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_small_logs_and_marks_only_actual_truncation() {
        let mut output = Vec::new();
        {
            let mut log = BoundedLog::new(&mut output, 64);
            log.write_all(b"short diagnostic\n").unwrap();
            log.flush().unwrap();
        }
        assert_eq!(output, b"short diagnostic\n");
        let mut output = Vec::new();
        let mut log = BoundedLog::new(&mut output, 64);
        for _ in 0..100 {
            log.write_all(b"more output\n").unwrap();
        }
        assert_eq!(output.len(), 64);
        assert!(output.ends_with(TRUNCATED));
        assert_eq!(
            output
                .windows(TRUNCATED.len())
                .filter(|s| *s == TRUNCATED)
                .count(),
            1
        );
    }

    #[test]
    fn drains_oversized_binary_logs_with_constant_retention() {
        let input = vec![0xff; MAX_LOG_BYTES + 128 * 1024];
        let mut reader = io::Cursor::new(&input);
        let mut output = Vec::new();
        drain(&mut reader, &mut output).unwrap();
        assert_eq!(reader.position() as usize, input.len());
        assert_eq!(output.len(), MAX_LOG_BYTES);
        assert!(output.ends_with(TRUNCATED));
    }

    #[test]
    fn a_full_disk_does_not_leave_the_childs_stderr_undrained() {
        struct FailedDisk;
        impl Write for FailedDisk {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("diagnostic disk full"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let input = vec![b'x'; 64 * 1024];
        let mut reader = io::Cursor::new(&input);
        assert!(drain(&mut reader, FailedDisk).is_err());
        assert_eq!(reader.position() as usize, input.len());
    }
}

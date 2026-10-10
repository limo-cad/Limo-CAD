//! Read-only settling for asynchronously completed native filename edits.
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct NativeEditText {
    pub(super) text: Vec<u16>,
    pub(super) start: usize,
    pub(super) end: usize,
}

impl NativeEditText {
    pub(super) fn matches(&self, prefix: &[u16], suffix: &[u16], final_read: bool) -> bool {
        let caret = prefix.len();
        let exact = self.text.len() == prefix.len() + suffix.len()
            && self.text.starts_with(prefix)
            && self.text[caret..] == *suffix
            && self.start == caret
            && self.end == caret;
        let completion = !final_read
            && suffix.is_empty()
            && self.text.starts_with(prefix)
            && self.start == caret
            && self.end == self.text.len()
            && self.end > caret;
        exact || completion
    }
}

#[derive(Default)]
pub(super) struct SettledEdit {
    previous: Option<(NativeEditText, Duration)>,
}

impl SettledEdit {
    // A first matching WM_GETTEXT can precede a queued autocomplete update.
    // Require unchanged acceptable samples over a bounded interval before
    // allowing the next input scalar. This is not a universal timing guarantee.
    pub(super) fn reset(&mut self) {
        self.previous = None;
    }

    pub(super) fn observe(
        &mut self,
        elapsed: Duration,
        current: &NativeEditText,
        acceptable: bool,
    ) -> bool {
        if !acceptable {
            self.previous = None;
            return false;
        }
        if let Some((previous, since)) = &self.previous {
            if previous == current {
                return elapsed.saturating_sub(*since) >= Duration::from_millis(100);
            }
        }
        self.previous = Some((current.clone(), elapsed));
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(text: &str, start: usize, end: usize) -> NativeEditText {
        NativeEditText {
            text: text.encode_utf16().collect(),
            start,
            end,
        }
    }

    #[test]
    fn delayed_replacement_resets_the_quiet_interval() {
        let prefix: Vec<_> = "D:\\l".encode_utf16().collect();
        let accepted = edit("D:\\l", 4, 4);
        let stale = edit("D:\\", 3, 3);
        let mut settled = SettledEdit::default();
        for (ms, snapshot, expected) in [
            (0, &accepted, false),
            (20, &accepted, false),
            (60, &stale, false),
            (80, &accepted, false),
            (160, &accepted, false),
            (180, &accepted, true),
        ] {
            assert_eq!(
                settled.observe(
                    Duration::from_millis(ms),
                    snapshot,
                    snapshot.matches(&prefix, &[], false)
                ),
                expected
            );
        }
    }

    #[test]
    fn changing_completion_and_selection_cannot_qualify_final_text() {
        let prefix: Vec<_> = "Su".encode_utf16().collect();
        let a = edit("Surface", 2, 7);
        let b = edit("Surface UI", 2, 10);
        assert!(a.matches(&prefix, &[], false));
        assert!(!a.matches(&prefix, &[], true));
        assert!(!edit("Su", 0, 2).matches(&prefix, &[], true));
        let mut settled = SettledEdit::default();
        assert!(!settled.observe(Duration::ZERO, &a, true));
        assert!(!settled.observe(Duration::from_millis(80), &b, true));
        assert!(!settled.observe(Duration::from_millis(100), &b, true));
        assert!(settled.observe(Duration::from_millis(180), &b, true));
        let exact = edit("Su", 2, 2);
        assert!(exact.matches(&prefix, &[], true));
        assert!(!settled.observe(Duration::from_millis(200), &exact, true));
        assert!(settled.observe(Duration::from_millis(300), &exact, true));
        settled.reset();
        assert!(!settled.observe(Duration::from_millis(400), &exact, true));
        assert!(settled.observe(Duration::from_millis(500), &exact, true));
        assert!(edit("Sun", 2, 2).matches(&prefix, &[b'n' as u16], true));
        assert!(!edit("Sun", 2, 3).matches(&prefix, &[b'n' as u16], false));
    }
}

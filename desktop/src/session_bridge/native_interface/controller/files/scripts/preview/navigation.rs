//! Small copyable return addresses, independent of documents, images and workers.
//! Only jumps add history; playback, step changes and camera gestures do not.
const LIMIT: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Lesson<L, S> {
    pub id: L,
    pub step: usize,
    pub scenario: S,
    pub instructions: bool,
    pub scroll: f32,
    pub steps_open: bool,
    pub steps_page: Option<usize>,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Location<L, S> {
    Index { page: usize, focus: Option<L> },
    Lesson(Lesson<L, S>),
}

#[derive(Clone, Copy)]
pub(super) struct History<L, S> {
    entries: [Option<Location<L, S>>; LIMIT],
    len: usize,
    index: Option<Location<L, S>>,
}

impl<L: Copy, S: Copy> Default for History<L, S> {
    fn default() -> Self {
        Self {
            entries: [None; LIMIT],
            len: 0,
            index: None,
        }
    }
}

impl<L: Copy + PartialEq, S: Copy + PartialEq> History<L, S> {
    pub fn push(&mut self, location: Location<L, S>) {
        if self.peek() == Some(location) {
            return;
        }
        if matches!(location, Location::Index { .. }) {
            self.index = Some(location);
        }
        if self.len == LIMIT {
            self.entries.copy_within(1.., 0);
            self.len -= 1;
        }
        self.entries[self.len] = Some(location);
        self.len += 1;
    }

    pub fn peek(&self) -> Option<Location<L, S>> {
        self.len
            .checked_sub(1)
            .and_then(|i| self.entries[i])
            .or(self.index)
    }

    pub fn pop(&mut self) -> Option<Location<L, S>> {
        let Some(index) = self.len.checked_sub(1) else {
            return self.index.take();
        };
        self.len = index;
        let location = self.entries[index].take();
        if location == self.index {
            self.index = None;
        }
        location
    }

    /// The index return address survives bounded prerequisite-history eviction.
    #[cfg(test)]
    pub fn index_origin(&self) -> Option<Location<L, S>> {
        self.index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lesson(id: u8, step: usize) -> Location<u8, u8> {
        Location::Lesson(Lesson {
            id,
            step,
            scenario: 1,
            instructions: true,
            scroll: 84.,
            steps_open: true,
            steps_page: Some(2),
            yaw: 0.8,
            pitch: 0.4,
        })
    }

    #[test]
    fn prerequisite_back_restores_the_whole_origin_and_index_destination() {
        let index = Location::Index {
            page: 7,
            focus: Some(42),
        };
        let origin = lesson(42, 3);
        let prerequisite = lesson(9, 1);
        let mut history = History::default();
        history.push(index);
        history.push(origin);
        history.push(prerequisite);
        assert_eq!(history.index_origin(), Some(index));
        assert_eq!(history.pop(), Some(prerequisite));
        assert_eq!(history.pop(), Some(origin));
        assert_eq!(history.pop(), Some(index));
        assert_eq!(history.pop(), None);
        assert_eq!(history.peek(), None);
    }

    #[test]
    fn repeated_jumps_are_bounded_and_duplicate_origins_do_not_grow_history() {
        let mut history = History::default();
        let index = Location::Index {
            page: 4,
            focus: Some(1),
        };
        history.push(index);
        for step in 0..10_000 {
            history.push(lesson(1, step));
            history.push(lesson(1, step));
        }
        for step in (10_000 - LIMIT..10_000).rev() {
            assert_eq!(history.pop(), Some(lesson(1, step)));
        }
        assert_eq!(
            history.index_origin(),
            Some(index),
            "Long chains must not lose their Help origin"
        );
        assert_eq!(history.peek(), Some(index));
        assert_eq!(history.pop(), Some(index));
        assert_eq!(history.pop(), None);
        assert_eq!(history.index_origin(), None);
        assert!(std::mem::size_of_val(&history) < 2048);
    }
}

//! Per-host authored snapshots: reopening a lesson does not repeat its CAD recipe.
//! Limits account serialized payload, not renderer/device resident memory.
use super::*;
use std::{collections::VecDeque, io::Write};
mod bundled;

const MAX_SOURCE_BYTES: usize = 256 * 1024;
const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 4;

struct Entry {
    source: String,
    lesson: lesson::Id,
    frames: Vec<Frame>,
    bytes: usize,
}

#[derive(Default)]
pub(super) struct Cache {
    entries: VecDeque<Entry>,
    bytes: usize,
    #[cfg(test)]
    hits: usize,
    #[cfg(test)]
    misses: usize,
}

/// Count serialization without allocating a second copy of a large snapshot.
struct Budget(usize);
impl Write for Budget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|total| *total <= MAX_PAYLOAD_BYTES)
            .ok_or_else(|| std::io::Error::other("Authored preview cache budget exceeded"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Cache {
    #[cfg(test)]
    pub(super) fn metrics(&self) -> (usize, usize, usize, usize) {
        (self.hits, self.misses, self.entries.len(), self.bytes)
    }

    fn get(&mut self, source: &str, lesson: lesson::Id) -> Option<Vec<Frame>> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.source == source && entry.lesson == lesson)?;
        let entry = self.entries.remove(index)?;
        let frames = entry.frames.clone();
        self.entries.push_back(entry);
        Some(frames)
    }

    fn store(&mut self, source: &str, lesson: lesson::Id, frames: &[Frame]) {
        if source.len() > MAX_SOURCE_BYTES {
            return;
        }
        let mut budget = Budget(source.len());
        if serde_json::to_writer(&mut budget, frames).is_err() {
            return;
        }
        // Source revisions supersede older snapshots of this authored lesson.
        self.entries.retain(|entry| {
            if entry.lesson == lesson {
                self.bytes -= entry.bytes;
                false
            } else {
                true
            }
        });
        while self.entries.len() >= MAX_ENTRIES || self.bytes + budget.0 > MAX_PAYLOAD_BYTES {
            let Some(old) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= old.bytes;
        }
        self.bytes += budget.0;
        self.entries.push_back(Entry {
            source: source.to_owned(),
            lesson,
            frames: frames.to_vec(),
            bytes: budget.0,
        });
    }
}

fn authored(source: &str, id: Option<lesson::Id>) -> Option<lesson::Id> {
    let id = id?;
    (source.len() <= MAX_SOURCE_BYTES && source == bundled::known_source(id)).then_some(id)
}

pub(super) fn prepare(
    cache: &Mutex<Cache>,
    source: &str,
    id: Option<lesson::Id>,
) -> Result<Vec<Frame>, String> {
    prepare_with(cache, source, id, || match authored(source, id) {
        Some(id) => bundled::load(source, id),
        None if id.is_some() => Err("Guided examples require their exact authored source".into()),
        None => prepare_lesson(source, None),
    })
}

fn prepare_with(
    cache: &Mutex<Cache>,
    source: &str,
    id: Option<lesson::Id>,
    build: impl FnOnce() -> Result<Vec<Frame>, String>,
) -> Result<Vec<Frame>, String> {
    let eligible = authored(source, id);
    if let Some(id) = eligible {
        if let Ok(mut cache) = cache.lock() {
            if let Some(frames) = cache.get(source, id) {
                #[cfg(test)]
                {
                    cache.hits += 1;
                }
                return Ok(frames);
            }
            #[cfg(test)]
            {
                cache.misses += 1;
            }
        }
    }
    // Never hold a shared lock while running CAD. Failed or uncached preparation
    // leaves existing cache entries intact; every returned snapshot still passes
    // PreviewService::retain validation before the UI can render it.
    let frames = build()?;
    if let Some(id) = eligible {
        if let Ok(mut cache) = cache.lock() {
            cache.store(source, id, &frames);
        }
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(caption: &str) -> Vec<Frame> {
        vec![Frame {
            caption: caption.into(),
            ..Default::default()
        }]
    }

    #[test]
    fn native_script_preview_authored_source_guard_rejects_edits_and_other_recipes() {
        let cache = Mutex::new(Cache::default());
        for id in lesson::ALL {
            let source = bundled::known_source(id);
            assert_eq!(authored(source, Some(id)), Some(id));
            let edited = format!("{source}\n");
            assert_eq!(authored(&edited, Some(id)), None);
            assert!(prepare(&cache, &edited, Some(id)).is_err());
            let other = lesson::ALL
                .into_iter()
                .find(|other| other.recipe() != id.recipe())
                .unwrap();
            assert_eq!(authored(bundled::known_source(other), Some(id)), None);
            assert!(prepare(&cache, bundled::known_source(other), Some(id)).is_err());
        }
        assert!(cache.lock().unwrap().entries.is_empty());
    }

    #[test]
    fn native_script_preview_prepared_cache_reuses_only_exact_authored_lessons() {
        let cache = Mutex::new(Cache::default());
        let source = &catalog::examples()
            .iter()
            .find(|example| example.id == lesson::RECIPE)
            .unwrap()
            .source;
        let id = Some(lesson::Id::Sketch);
        prepare_with(&cache, source, id, || Ok(frames("first"))).unwrap();
        let mut hit = prepare_with(&cache, source, id, || panic!("Recipe rebuilt")).unwrap();
        hit[0].caption = "caller modification".into();
        assert_eq!(
            prepare_with(&cache, source, id, || panic!("Recipe rebuilt")).unwrap()[0].caption,
            "first"
        );
        for (source, id) in [(format!("{source}\n"), id), (source.clone(), None)] {
            let mut builds = 0;
            for _ in 0..2 {
                prepare_with(&cache, &source, id, || {
                    builds += 1;
                    Ok(frames("uncached"))
                })
                .unwrap();
            }
            assert_eq!(builds, 2);
        }
        assert!(prepare_with(&cache, source, Some(lesson::Id::Cut), || Err(
            "failed".into()
        ))
        .is_err());
        assert_eq!(cache.lock().unwrap().entries.len(), 1);
    }

    #[test]
    fn native_script_preview_prepared_cache_invalidates_source_and_bounds_payload() {
        let mut cache = Cache::default();
        cache.store("old", lesson::Id::Sketch, &frames("old"));
        assert!(cache.get("new", lesson::Id::Sketch).is_none());
        assert!(cache.get("old", lesson::Id::Extrude).is_none());
        cache.store("new", lesson::Id::Sketch, &frames("new"));
        assert!(cache.get("old", lesson::Id::Sketch).is_none());
        assert_eq!(cache.entries.len(), 1);
        let large = frames(&"a".repeat(MAX_PAYLOAD_BYTES / 3));
        for id in lesson::ALL {
            cache.store("source", id, &large);
            assert!(cache.entries.len() <= MAX_ENTRIES);
            assert!(cache.bytes <= MAX_PAYLOAD_BYTES);
        }
        assert!(cache.entries.len() < lesson::ALL.len());
        let before = cache.bytes;
        cache.store(&"s".repeat(MAX_SOURCE_BYTES + 1), lesson::Id::Sketch, &[]);
        cache.store(
            "source",
            lesson::Id::Sketch,
            &frames(&"x".repeat(MAX_PAYLOAD_BYTES)),
        );
        assert_eq!(cache.bytes, before);
        assert_eq!(
            cache.bytes,
            cache.entries.iter().map(|entry| entry.bytes).sum::<usize>()
        );
    }
}

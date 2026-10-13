//! Geometry and exact hit provenance for the focus-only native caption path.
//! These points are screen coordinates and never ordinary CAD client points.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Geometry {
    pub window: [i32; 4],
    pub client: [i32; 4],
    pub title: [i32; 4],
    pub dpi: u32,
}

#[derive(Clone, Copy, Default)]
pub(super) struct Hit {
    pub window: usize,
    pub root: usize,
    pub pid: u32,
    pub code: usize,
}

fn inside(rect: [i32; 4], point: [i32; 2]) -> bool {
    rect[0] < rect[2]
        && rect[1] < rect[3]
        && point[0] >= rect[0]
        && point[0] < rect[2]
        && point[1] >= rect[1]
        && point[1] < rect[3]
}

impl Geometry {
    pub fn patch(&self, point: [i32; 2]) -> Option<[[i32; 2]; 5]> {
        let [x, y] = point;
        let patch = [
            point,
            [x.checked_sub(3)?, y],
            [x.checked_add(3)?, y],
            [x, y.checked_sub(3)?],
            [x, y.checked_add(3)?],
        ];
        (self.dpi != 0
            && self.client[0] < self.client[2]
            && self.client[1] < self.client[3]
            && self.client[0] >= self.window[0]
            && self.client[1] >= self.window[1]
            && self.client[2] <= self.window[2]
            && self.client[3] <= self.window[3]
            && patch.iter().all(|point| {
                i16::try_from(point[0]).is_ok()
                    && i16::try_from(point[1]).is_ok()
                    && inside(self.window, *point)
                    && inside(self.title, *point)
                    && !inside(self.client, *point)
            }))
        .then_some(patch)
    }

    pub fn candidates(&self) -> Vec<[i32; 2]> {
        let width = i64::from(self.title[2]) - i64::from(self.title[0]);
        let height = i64::from(self.title[3]) - i64::from(self.title[1]);
        if width <= 0 || height <= 0 {
            return Vec::new();
        }
        (1..=8)
            .filter_map(|part| {
                let point = [
                    i32::try_from(i64::from(self.title[0]) + width * part / 9).ok()?,
                    i32::try_from(i64::from(self.title[1]) + height / 2).ok()?,
                ];
                self.patch(point).map(|_| point)
            })
            .collect()
    }

    pub fn qualifies(&self, point: [i32; 2], hits: &[Hit; 5], hwnd: usize, pid: u32) -> bool {
        // HTCAPTION is exactly 2. Same-process popups/children are insufficient.
        hwnd != 0
            && pid != 0
            && self.patch(point).is_some()
            && hits.iter().all(|hit| {
                hit.window == hwnd && hit.root == hwnd && hit.pid == pid && hit.code == 2
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry() -> Geometry {
        Geometry {
            window: [-1400, -900, -200, -100],
            client: [-1392, -862, -208, -108],
            title: [-1392, -893, -208, -862],
            dpi: 144,
        }
    }

    #[test]
    fn caption_proof_rejects_client_buttons_children_and_foreign_ownership() {
        let geometry = geometry();
        let point = geometry.candidates()[0];
        let hit = Hit {
            window: 42,
            root: 42,
            pid: 7,
            code: 2,
        };
        assert!(geometry.qualifies(point, &[hit; 5], 42, 7));
        for invalid in [
            Hit { code: 1, ..hit },    // Client.
            Hit { code: 20, ..hit },   // Close button.
            Hit { code: 3, ..hit },    // System menu.
            Hit { window: 43, ..hit }, // Owned child/popup is still refused.
            Hit { root: 43, ..hit },
            Hit { pid: 8, ..hit },
        ] {
            let mut hits = [hit; 5];
            hits[3] = invalid;
            assert!(!geometry.qualifies(point, &hits, 42, 7));
        }
        assert!(!geometry.qualifies([-800, -800], &[hit; 5], 42, 7));
        assert!(!geometry.qualifies([-800, -863], &[hit; 5], 42, 7));
        assert!(!geometry.qualifies(point, &[hit; 5], 0, 7));
        assert!(!geometry.qualifies(point, &[hit; 5], 42, 0));
    }

    #[test]
    fn caption_candidates_preserve_negative_screen_coordinates_and_refuse_truncation() {
        let geometry = geometry();
        let candidates = geometry.candidates();
        assert_eq!(candidates.len(), 8);
        for point in candidates {
            for probe in geometry.patch(point).unwrap() {
                assert!(inside(geometry.title, probe));
                assert!(!inside(geometry.client, probe));
                let packed = (u32::from(probe[0] as i16 as u16)
                    | (u32::from(probe[1] as i16 as u16) << 16))
                    as i32;
                assert_eq!(i32::from(packed as i16), probe[0]);
                assert_eq!(i32::from((packed >> 16) as i16), probe[1]);
            }
        }
        let far_monitor = Geometry {
            window: [40_000, 0, 41_000, 800],
            client: [40_008, 40, 40_992, 792],
            title: [40_008, 8, 40_992, 40],
            dpi: 96,
        };
        assert!(far_monitor.candidates().is_empty());
        assert!(far_monitor.patch([40_500, 24]).is_none());
        assert!(geometry.patch([i32::MIN, i32::MAX]).is_none());
        assert!(Geometry { dpi: 0, ..geometry }.candidates().is_empty());
        assert!(Geometry {
            client: [0; 4],
            ..geometry
        }
        .candidates()
        .is_empty());
    }
}

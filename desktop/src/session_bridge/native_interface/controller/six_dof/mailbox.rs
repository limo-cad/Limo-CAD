//! Constant-size input state. A new route never inherits a displaced cap.
use crate::six_dof_mouse::{MotionPacket, SixDofEvent};
use std::time::{Duration, Instant};

pub(super) const HOLD: Duration = Duration::from_millis(45);

#[derive(Default)]
pub(super) struct Mailbox {
    translation: Option<([i16; 3], Instant)>,
    rotation: Option<([i16; 3], Instant)>,
    fit: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Motion {
    pub translation: [f32; 3],
    pub rotation: [f32; 3],
}
impl Motion {
    pub fn active(self) -> bool {
        self.translation
            .into_iter()
            .chain(self.rotation)
            .any(|v| v != 0.)
    }
}
/// Device axes in the release host's object-motion basis.
///
/// `sixDofMouse.ts` divides by 350, drops a 0.025 dead zone, then flips Y and
/// Z once in `canonicalizeSixDofTranslation` / `canonicalizeSixDofRotation`.
pub(crate) fn device_axes(raw: [i16; 3]) -> [f32; 3] {
    let values = raw.map(|v| {
        let v = (f32::from(v) / 350.).clamp(-1., 1.);
        if v.abs() < 0.025 {
            0.
        } else {
            v
        }
    });
    [values[0], -values[1], -values[2]]
}
impl Mailbox {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn push(&mut self, event: SixDofEvent, now: Instant) {
        match event {
            SixDofEvent::Motion(MotionPacket {
                translation,
                rotation,
            }) => {
                if let Some(value) = translation {
                    self.translation = Some((value, now));
                }
                if let Some(value) = rotation {
                    self.rotation = Some((value, now));
                }
            }
            SixDofEvent::Button(button) if button.button == 1 => self.fit = true,
            _ => (),
        }
    }
    pub fn sample(&mut self, now: Instant) -> (Motion, bool) {
        fn axis(value: &mut Option<([i16; 3], Instant)>, now: Instant) -> [f32; 3] {
            if value.is_some_and(|(_, at)| now.saturating_duration_since(at) > HOLD) {
                *value = None;
            }
            value.map_or([0.; 3], |(value, _)| device_axes(value))
        }
        (
            Motion {
                translation: axis(&mut self.translation, now),
                rotation: axis(&mut self.rotation, now),
            },
            std::mem::take(&mut self.fit),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_axes_coalesce_independently_and_zero_packets_stop_without_waiting() {
        let now = Instant::now();
        let mut slot = Mailbox::default();
        for n in 1..=10_000 {
            slot.push(
                SixDofEvent::Motion(MotionPacket {
                    translation: Some([n, 350, -350]),
                    rotation: None,
                }),
                now,
            );
        }
        slot.push(
            SixDofEvent::Motion(MotionPacket {
                translation: None,
                rotation: Some([175, 8, -9]),
            }),
            now + Duration::from_millis(30),
        );
        let (motion, _) = slot.sample(now + Duration::from_millis(40));
        assert_eq!(motion.translation, [1., -1., 1.]);
        assert_eq!(motion.rotation, [0.5, 0., 9. / 350.]);
        let (motion, _) = slot.sample(now + Duration::from_millis(46));
        assert_eq!(motion.translation, [0.; 3]);
        assert_ne!(motion.rotation, [0.; 3]);
        slot.push(
            SixDofEvent::Motion(MotionPacket {
                translation: None,
                rotation: Some([0; 3]),
            }),
            now + Duration::from_millis(47),
        );
        assert!(!slot.sample(now + Duration::from_millis(47)).0.active());
    }
    #[test]
    fn primary_button_is_bounded_consumable_and_route_reset_discards_all_input() {
        let now = Instant::now();
        let mut slot = Mailbox::default();
        for _ in 0..10_000 {
            slot.push(
                SixDofEvent::Button(crate::six_dof_mouse::ButtonPacket { button: 1 }),
                now,
            );
        }
        assert!(slot.sample(now).1);
        assert!(!slot.sample(now).1);
        slot.push(
            SixDofEvent::Motion(MotionPacket {
                translation: Some([350; 3]),
                rotation: None,
            }),
            now,
        );
        slot.push(
            SixDofEvent::Button(crate::six_dof_mouse::ButtonPacket { button: 1 }),
            now,
        );
        slot.clear();
        assert!(!slot.sample(now).0.active());
        assert!(!slot.sample(now).1);
    }
}

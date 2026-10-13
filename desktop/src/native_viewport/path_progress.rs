//! Splits retained path strokes at the physical playback cursor. This is
//! presentation only: no toolpath generation, stock work, or per-tick storage.

use bevy::prelude::Vec3;

use super::{ViewportCamPathProgress, ViewportLinePlayback};

pub(super) fn active_cursor(
    playback: Option<&ViewportLinePlayback>,
    cursor: Option<ViewportCamPathProgress>,
) -> Option<ViewportCamPathProgress> {
    cursor.filter(|cursor| {
        cursor.time_seconds.is_finite()
            && playback.is_some_and(|path| path.path_id == cursor.path_id)
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ColoredSegment {
    pub start: Vec3,
    pub end: Vec3,
    pub color: [f32; 4],
    pub completed: bool,
}

pub(super) fn split_segment(
    start: Vec3,
    end: Vec3,
    upcoming: [f32; 4],
    completed: [f32; 4],
    timing: Option<[f64; 2]>,
    cursor: Option<ViewportCamPathProgress>,
) -> [Option<ColoredSegment>; 2] {
    let whole = |color, completed| {
        [
            Some(ColoredSegment {
                start,
                end,
                color,
                completed,
            }),
            None,
        ]
    };
    let Some([begin, finish]) = timing else {
        return whole(upcoming, false);
    };

    let Some(cursor) = cursor else {
        return [None, None];
    };
    if !cursor.time_seconds.is_finite()
        || !begin.is_finite()
        || !finish.is_finite()
        || finish < begin
    {
        return [None, None];
    }
    if cursor.time_seconds + 1e-9 >= finish {
        return whole(completed, true);
    }
    if cursor.time_seconds <= begin + 1e-9 {
        return [None, None];
    }

    let tip = Vec3::from_array(cursor.position);
    if !tip.is_finite() {
        return [None, None];
    }
    [
        Some(ColoredSegment {
            start,
            end: tip,
            color: completed,
            completed: true,
        }),
        None,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEXT: [f32; 4] = [0.34, 0.84, 0.64, 0.95];
    const DONE: [f32; 4] = [0.18, 0.48, 1.0, 1.0];

    fn parts(time: f64, tip: Vec3) -> [Option<ColoredSegment>; 2] {
        split_segment(
            Vec3::ZERO,
            Vec3::X * 10.0,
            NEXT,
            DONE,
            Some([2.0, 12.0]),
            Some(ViewportCamPathProgress {
                path_id: 1,
                time_seconds: time,
                position: tip.to_array(),
            }),
        )
    }

    #[test]
    fn completed_plunge_persists_after_the_tool_enters_horizontal_motion() {
        let start = Vec3::new(0.0, 0.0, 2.0);
        let end = Vec3::new(0.0, 0.0, -3.0);
        for (time, tip) in [
            (0.5, Vec3::new(0.0, 0.0, -0.5)),
            (1.5, Vec3::new(5.0, 0.0, -3.0)),
            (2.5, Vec3::new(10.0, 0.0, -3.0)),
        ] {
            let parts = split_segment(
                start,
                end,
                NEXT,
                DONE,
                Some([0.0, 1.0]),
                Some(ViewportCamPathProgress {
                    path_id: 1,
                    time_seconds: time,
                    position: tip.to_array(),
                }),
            );
            assert!(parts[0].unwrap().completed);
            assert_eq!(parts[0].unwrap().color, DONE);
            if time > 1.0 {
                assert_eq!(parts[0].unwrap().end, end);
                assert!(parts[1].is_none());
            }
        }
    }

    #[test]
    fn partial_line_meets_tool_center_and_rewind_hides_future_travel() {
        let middle = parts(7.0, Vec3::X * 5.0);
        assert_eq!(middle[0].unwrap().end, Vec3::X * 5.0);
        assert!(middle[1].is_none(), "no future half of the current move");
        assert_eq!(middle[0].unwrap().color, DONE);
        assert_eq!(parts(12.0, Vec3::X * 10.0)[0].unwrap().color, DONE);
        let rewound = parts(2.0, Vec3::ZERO);
        assert_eq!(rewound, [None, None]);
        assert_eq!(parts(0.0, Vec3::ZERO), [None, None]);
    }

    #[test]
    fn partial_arc_joins_at_true_curve_not_inside_its_chord() {
        let tip = Vec3::new(0.5_f32.sqrt(), 0.5_f32.sqrt(), 2.0);
        let lines = split_segment(
            Vec3::X,
            Vec3::new(0.0, 1.0, 4.0),
            NEXT,
            DONE,
            Some([0.0, 1.0]),
            Some(ViewportCamPathProgress {
                path_id: 1,
                time_seconds: 0.5,
                position: tip.to_array(),
            }),
        );
        assert_eq!(lines[0].unwrap().end, tip);
        assert!(lines[1].is_none());
        assert!((tip.truncate().length() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn zero_duration_and_missing_or_invalid_cursor_do_not_create_partial_lines() {
        let cursor = ViewportCamPathProgress {
            path_id: 1,
            time_seconds: 2.0,
            position: [0.0; 3],
        };
        let zero = split_segment(
            Vec3::ZERO,
            Vec3::X,
            NEXT,
            DONE,
            Some([2.0, 2.0]),
            Some(cursor),
        );
        assert_eq!(zero[0].unwrap().color, DONE);
        assert!(zero[1].is_none());
        let static_line = split_segment(Vec3::ZERO, Vec3::X, NEXT, DONE, None, None);
        assert_eq!(
            static_line[0].unwrap().color,
            NEXT,
            "program preview remains complete"
        );
        let missing_clock = split_segment(Vec3::ZERO, Vec3::X, NEXT, DONE, Some([0.0, 4.0]), None);
        assert_eq!(missing_clock, [None, None]);
        assert_eq!(parts(f64::NAN, Vec3::ZERO), [None, None]);
        assert_eq!(parts(7.0, Vec3::splat(f32::NAN)), [None, None]);
    }

    #[test]
    fn stale_timeline_or_closed_playback_cannot_color_retained_lines() {
        let mut path = ViewportLinePlayback {
            path_id: 2,
            completed_color: DONE,
            segment_times: vec![0.0, 10.0].into(),
            single_tool: false,
            removes_stock: false,
        };
        let cursor = ViewportCamPathProgress {
            path_id: 1,
            time_seconds: 5.0,
            position: [5.0, 0.0, 0.0],
        };
        assert!(active_cursor(Some(&path), Some(cursor)).is_none());
        path.path_id = 1;
        assert_eq!(active_cursor(Some(&path), Some(cursor)), Some(cursor));
        assert!(active_cursor(Some(&path), None).is_none());
        assert!(active_cursor(None, Some(cursor)).is_none());
        assert!(path.is_valid_for(6));
        assert!(
            !path.is_valid_for(12),
            "each segment must have exactly two times"
        );
        path.segment_times = vec![10.0, 0.0].into();
        assert!(!path.is_valid_for(6));
    }

    #[test]
    fn rapid_dots_keep_their_phase_and_only_the_intersected_dot_is_split() {
        let cursor = Some(ViewportCamPathProgress {
            path_id: 1,
            time_seconds: 4.5,
            position: [4.5, 0.0, 0.0],
        });
        let dot = |begin: f32, end: f32| {
            split_segment(
                Vec3::X * begin,
                Vec3::X * end,
                NEXT,
                DONE,
                Some([f64::from(begin), f64::from(end)]),
                cursor,
            )
        };
        assert!(dot(0.0, 1.0)[0].unwrap().completed);
        assert_eq!(dot(8.0, 9.0), [None, None]);
        let active = dot(4.0, 5.0);
        assert_eq!(active[0].unwrap().start.x, 4.0);
        assert_eq!(active[0].unwrap().end.x, 4.5);
        assert!(active[1].is_none());
    }
}

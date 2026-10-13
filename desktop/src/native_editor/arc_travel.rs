//! Signed pointer travel for center-point arcs, matching the reference UI.
use std::f64::consts::{PI, TAU};

const DIRECTION_THRESHOLD: f64 = 2. * PI / 180.;

#[derive(Clone, Copy, Debug)]
pub(super) struct ArcTravel {
    last_angle: f64,
    travel: f64,
    direction: f64,
}

impl ArcTravel {
    pub(super) fn new(start_angle: f64) -> Self {
        Self {
            last_angle: start_angle,
            travel: 0.,
            direction: 0.,
        }
    }

    /// Ignore initial hand jitter, then keep the chosen direction as the
    /// pointer crosses the start ray or completes a full revolution.
    pub(super) fn advance(&mut self, angle: f64) -> f64 {
        let step = (angle - self.last_angle + PI).rem_euclid(TAU) - PI;
        self.last_angle = angle;
        // Keep overshoot so backtracking resolves against the actual pointer
        // travel, rather than subtracting from an already capped full turn.
        self.travel += step;
        if self.direction == 0. {
            if self.travel.abs() < DIRECTION_THRESHOLD {
                return 0.;
            }
            self.direction = self.travel.signum();
        }
        let directed = self.direction * self.travel;
        let magnitude = if directed.abs() >= TAU - 1e-9 {
            TAU
        } else {
            directed.rem_euclid(TAU)
        };
        self.direction * magnitude
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(actual: f64, degrees: f64) {
        assert!((actual - degrees.to_radians()).abs() < 1e-9);
    }

    #[test]
    fn jitter_wraparound_and_start_ray_crossings_preserve_direction() {
        for sign in [-1., 1.] {
            let mut state = ArcTravel::new(175_f64.to_radians());
            near(state.advance((175. - sign * 0.5_f64).to_radians()), 0.);
            near(
                state.advance((175. + sign * 32_f64).to_radians()),
                sign * 32.,
            );
            near(
                state.advance((175. - sign * 5_f64).to_radians()),
                sign * 355.,
            );
        }
    }

    #[test]
    fn clockwise_and_counterclockwise_full_turns_remain_full_turns() {
        for sign in [-1., 1.] {
            let mut state = ArcTravel::new(0.);
            for degrees in (10..=360).step_by(10) {
                let degrees = sign * f64::from(degrees);
                near(state.advance(degrees.to_radians()), degrees);
            }
            near(state.advance(0.), sign * 360.);
        }
    }
}

//! Bounds generated UI work/storage, independently of saved drawing validity.
use super::*;
use std::mem::size_of;

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub segments: usize,
    pub labels: usize,
    pub fills: usize,
    pub retained: usize,
    pub text: usize,
    pub scratch: usize,
    pub work: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            segments: 32_768,
            labels: 8_192,
            fills: 4_096,
            retained: 16 * 1024 * 1024,
            text: 2 * 1024 * 1024,
            scratch: 8 * 1024 * 1024,
            work: 10_000_000,
        }
    }
}
#[derive(Default)]
pub(super) struct Budget {
    pub limits: Limits,
    pub error: Option<String>,
    retained: usize,
    text: usize,
    work: u64,
}
impl Budget {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            ..Default::default()
        }
    }
    pub fn reject(&mut self, message: &str) -> bool {
        if self.error.is_none() {
            self.error = Some(message.into());
        }
        false
    }
    pub fn ready(&self) -> bool {
        self.error.is_none()
    }
    pub fn check(&self) -> Result<(), String> {
        self.error.as_ref().map_or(Ok(()), |e| Err(e.clone()))
    }
    pub fn work(&mut self, count: u64) -> bool {
        if !self.ready() {
            return false;
        }
        let Some(next) = self
            .work
            .checked_add(count)
            .filter(|n| *n <= self.limits.work)
        else {
            return self.reject("Drawing annotations exceed the generation work limit; simplify the annotations or dash pattern");
        };
        self.work = next;
        true
    }
    pub fn steps(&mut self, count: f64) -> bool {
        if !count.is_finite() || count < 0. || count >= u64::MAX as f64 {
            return self
                .reject("Drawing annotation generation work is outside the supported range");
        }
        self.work(count.ceil() as u64)
    }
    pub fn scratch(&mut self, bytes: usize) -> bool {
        if !self.ready() {
            return false;
        }
        if bytes > self.limits.scratch {
            return self.reject("Drawing annotations exceed the temporary memory limit");
        }
        true
    }
    pub fn retained(&mut self, bytes: usize) -> bool {
        if !self.ready() {
            return false;
        }
        let Some(next) = self
            .retained
            .checked_add(bytes)
            .filter(|n| *n <= self.limits.retained)
        else {
            return self.reject("Drawing annotations exceed the retained memory limit");
        };
        self.retained = next;
        true
    }
    pub fn text(&mut self, bytes: usize) -> bool {
        if !self.ready() {
            return false;
        }
        let Some(next) = self
            .text
            .checked_add(bytes)
            .filter(|n| *n <= self.limits.text)
        else {
            return self.reject("Drawing annotations exceed the text limit");
        };
        if !self.retained(bytes) {
            return false;
        }
        self.text = next;
        true
    }
    pub fn reserve<T>(&mut self, values: &mut Vec<T>, count: usize, limit: usize) -> bool {
        if !self.ready() {
            return false;
        }
        let Some(required) = values.len().checked_add(count).filter(|n| *n <= limit) else {
            return self
                .reject("Drawing annotations exceed the primitive limit; simplify the sheet");
        };
        if required > values.capacity() {
            let target = required.saturating_add(63).min(limit);
            let Some(bytes) = (target - values.capacity()).checked_mul(size_of::<T>()) else {
                return self.reject("Drawing annotation size overflow");
            };
            if !self.retained(bytes) {
                return false;
            }
            if values.try_reserve_exact(target - values.len()).is_err() {
                return self.reject("Cannot allocate drawing annotation graphics");
            }
        }
        true
    }
    pub fn path(&mut self, points: &[P], style: &DrawingLineStyleDto) -> bool {
        if !self.work(points.len() as u64 + style.dash_mm.len() as u64) {
            return false;
        }
        if points.iter().flatten().any(|n| !n.is_finite())
            || !style.width_mm.is_finite()
            || style.width_mm <= 0.
            || style.dash_mm.iter().any(|n| !n.is_finite() || *n <= 0.)
        {
            return self.reject("Drawing annotations contain invalid line geometry");
        }
        if let Some(minimum) = style.dash_mm.iter().copied().reduce(f64::min) {
            let length = points
                .windows(2)
                .map(|p| super::geometry::length(sub(p[1], p[0])))
                .sum::<f64>();
            return self.steps(length / minimum + points.len() as f64 * 2.);
        }
        true
    }
    pub fn input(
        &mut self,
        annotation: &DrawingAnnotationDto,
        projection: Option<&DrawingProjectionDto>,
    ) -> bool {
        use DrawingAnnotationDto::*;
        let mut bytes = 0usize;
        let mut add = |s: &str| {
            bytes = bytes.saturating_add(s.len());
        };
        let mut refs = 6usize;
        match annotation {
            LinearDimension {
                prefix,
                suffix,
                presentation,
                ..
            }
            | LineDimension {
                prefix,
                suffix,
                presentation,
                ..
            }
            | PointLineDimension {
                prefix,
                suffix,
                presentation,
                ..
            }
            | RadialDimension {
                prefix,
                suffix,
                presentation,
                ..
            }
            | AngularDimension {
                prefix,
                suffix,
                presentation,
                ..
            } => {
                add(prefix);
                add(suffix);
                add(&presentation.fit_class);
            }
            ChainDimension {
                anchors,
                prefix,
                suffix,
                presentation,
                ..
            } => {
                refs = anchors.len();
                add(prefix);
                add(suffix);
                add(&presentation.fit_class);
            }
            OrdinateDimension { presentation, .. }
            | ArcLengthDimension { presentation, .. }
            | JoggedRadiusDimension { presentation, .. } => add(&presentation.fit_class),
            Note { text, .. } => add(text),
            HoleNote {
                thread,
                note,
                feature_name,
                pattern_note,
                ..
            } => {
                add(thread);
                add(note);
                add(feature_name);
                add(pattern_note);
            }
            ChamferNote { prefix, .. } => add(prefix),
            DatumFeature { label, .. } => add(label),
            GdtFrame { datums, .. } => {
                for datum in datums {
                    add(&datum.label);
                }
            }
            SurfaceTexture { process, .. } => add(process),
            EdgeRequirement { note, .. } => add(note),
            WeldSymbol { tail, .. } => add(tail),
            RevisionCloud { revision, .. } => add(revision),
            BoltCircleCenterLine { features, .. } => refs = features.len(),
            CenterMark { .. }
            | CenterLine { .. }
            | CenterLineBetweenEdges { .. }
            | AutomaticSymmetryAxis { .. }
            | ItemBalloon { .. } => {}
        }
        if bytes > self.limits.text {
            return self.reject("Drawing annotations exceed the text limit");
        }
        if !self.work(bytes as u64)
            || !self.scratch(
                bytes
                    .saturating_mul(4)
                    .saturating_add(16_384)
                    .saturating_add(refs.saturating_mul(64)),
            )
        {
            return false;
        }
        if let Some(p) = projection {
            let work = refs
                .saturating_mul(2)
                .saturating_mul(p.anchors.len().saturating_add(p.circles.len()));
            if !self.work(work as u64) {
                return false;
            }
        }
        true
    }
}

pub(super) fn finite_segment(s: &Segment) -> bool {
    [s.x1, s.y1, s.x2, s.y2, s.width_mm]
        .iter()
        .all(|n| n.is_finite())
}
pub(super) fn finite_label(s: &Label) -> bool {
    [s.x, s.y, s.angle, s.width_mm, s.height_mm, s.text_height_mm]
        .iter()
        .all(|n| n.is_finite())
        && s.width_mm > 0.
        && s.height_mm > 0.
        && s.text_height_mm > 0.
}
pub(super) fn finite_fill(s: &Fill) -> bool {
    [s.x, s.y, s.width, s.height].iter().all(|n| n.is_finite()) && s.width >= 0. && s.height >= 0.
}

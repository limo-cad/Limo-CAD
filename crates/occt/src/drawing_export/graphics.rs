//! Bounded, Rust-only paper graphics. These are presentation, never saved intent.
use super::{PaperPrimitive, P};
use limo_cad_sketch::DrawingLineStyleDto;
use std::mem::size_of;

/// Angles follow paper coordinates: zero is horizontal, positive is clockwise.
/// Export supplies the view's angle/spacing. The live workspace supplies its
/// vertical SVG pattern angle + 90 degrees and the sheet's hatch spacing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HatchPattern {
    pub angle_deg: f64,
    pub spacing_mm: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct PaperGraphicsLimits {
    pub primitives: usize,
    pub points: usize,
    pub retained_bytes: usize,
    pub scratch_bytes: usize,
    pub work: u64,
}
impl Default for PaperGraphicsLimits {
    fn default() -> Self {
        Self {
            primitives: 250_000,
            points: 1_000_000,
            retained_bytes: 64 * 1024 * 1024,
            scratch_bytes: 16 * 1024 * 1024,
            work: 50_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaperGraphicsUsage {
    pub primitives: usize,
    pub points: usize,
    pub retained_bytes: usize,
    pub peak_scratch_bytes: usize,
    pub work: u64,
}

/// Share one budget across every section and source marker on a sheet. Failed
/// generation consumes its attempted budget; discard both the result and budget
/// instead of publishing a partial sheet or retrying individual primitives.
#[derive(Debug, Default)]
pub struct PaperGraphicsBudget {
    limits: PaperGraphicsLimits,
    usage: PaperGraphicsUsage,
}
impl PaperGraphicsBudget {
    pub fn new(limits: PaperGraphicsLimits) -> Self {
        Self {
            limits,
            usage: PaperGraphicsUsage::default(),
        }
    }
    pub fn usage(&self) -> PaperGraphicsUsage {
        self.usage
    }
    /// Combine batches produced with this budget without uncharged Vec growth.
    /// The payloads move unchanged; their storage and primitive/point counts
    /// were charged during generation. Keep the old batch capacity charged as
    /// a conservative bound on the peak while both allocations coexist.
    /// Start the destination empty and use this method for every append.
    pub fn append(
        &mut self,
        destination: &mut Vec<PaperPrimitive>,
        mut batch: Vec<PaperPrimitive>,
    ) -> Result<(), String> {
        let required = destination
            .len()
            .checked_add(batch.len())
            .ok_or("Drawing graphics size overflow")?;
        self.work(batch.len() as u64)?;
        if required > destination.capacity() {
            self.retained(bytes(
                required - destination.capacity(),
                size_of::<PaperPrimitive>(),
            )?)?;
            destination
                .try_reserve_exact(required - destination.len())
                .map_err(|_| "Cannot allocate drawing graphics")?;
        }
        destination.append(&mut batch);
        Ok(())
    }
    pub(super) fn work(&mut self, count: u64) -> Result<(), String> {
        let next = self
            .usage
            .work
            .checked_add(count)
            .ok_or("Drawing graphics work overflow")?;
        if next > self.limits.work {
            return Err("Drawing graphics generation exceeds the work limit".into());
        }
        self.usage.work = next;
        Ok(())
    }
    pub(super) fn scratch(&mut self, bytes: usize) -> Result<(), String> {
        if bytes > self.limits.scratch_bytes {
            return Err("Drawing graphics generation exceeds the scratch memory limit".into());
        }
        self.usage.peak_scratch_bytes = self.usage.peak_scratch_bytes.max(bytes);
        Ok(())
    }
    fn retained(&mut self, bytes: usize) -> Result<(), String> {
        let next = self
            .usage
            .retained_bytes
            .checked_add(bytes)
            .ok_or("Drawing graphics size overflow")?;
        if next > self.limits.retained_bytes {
            return Err("Drawing graphics exceed the retained memory limit".into());
        }
        self.usage.retained_bytes = next;
        Ok(())
    }
    fn points(&mut self, count: usize) -> Result<(), String> {
        let next = self
            .usage
            .points
            .checked_add(count)
            .ok_or("Drawing graphics point count overflow")?;
        if next > self.limits.points {
            return Err("Drawing graphics exceed the point limit".into());
        }
        self.usage.points = next;
        Ok(())
    }
}

pub(super) fn bytes(count: usize, size: usize) -> Result<usize, String> {
    count
        .checked_mul(size)
        .ok_or_else(|| "Drawing graphics size overflow".into())
}
pub(super) fn finite_points(points: &[P]) -> Result<(), String> {
    if points.iter().flatten().any(|n| !n.is_finite()) {
        return Err("Drawing graphics contain non-finite coordinates".into());
    }
    Ok(())
}

/// Checked sink: charges storage before allocating points, dashes, text or the
/// primitive vector. Small exact-capacity chunks avoid unaccounted Vec growth.
pub(super) struct Graphics<'a> {
    pub(super) budget: &'a mut PaperGraphicsBudget,
    items: Vec<PaperPrimitive>,
}
impl<'a> Graphics<'a> {
    pub(super) fn new(budget: &'a mut PaperGraphicsBudget) -> Self {
        Self {
            budget,
            items: Vec::new(),
        }
    }
    fn item(&mut self) -> Result<(), String> {
        if self.budget.usage.primitives >= self.budget.limits.primitives {
            return Err("Drawing graphics exceed the primitive limit".into());
        }
        if self.items.len() == self.items.capacity() {
            let additional = 64.min(self.budget.limits.primitives - self.budget.usage.primitives);
            self.budget
                .retained(bytes(additional, size_of::<PaperPrimitive>())?)?;
            self.items
                .try_reserve_exact(additional)
                .map_err(|_| "Cannot allocate drawing graphics")?;
        }
        self.budget.usage.primitives += 1;
        Ok(())
    }
    pub(super) fn line(
        &mut self,
        points: &[P],
        layer: &'static str,
        style: &DrawingLineStyleDto,
    ) -> Result<(), String> {
        self.budget.work(
            (points.len() as u64)
                .checked_add(style.dash_mm.len() as u64)
                .ok_or("Drawing graphics work overflow")?,
        )?;
        finite_points(points)?;
        if !style.width_mm.is_finite()
            || style.width_mm <= 0.
            || style.dash_mm.iter().any(|n| !n.is_finite() || *n <= 0.)
        {
            return Err("Drawing graphics have invalid line style".into());
        }
        self.budget.points(points.len())?;
        let storage = bytes(points.len(), size_of::<P>())?
            .checked_add(bytes(style.dash_mm.len(), size_of::<f64>())?)
            .ok_or("Drawing graphics size overflow")?;
        self.budget.retained(storage)?;
        self.item()?;
        self.items.push(PaperPrimitive::Line {
            points: points.to_vec(),
            layer,
            width: style.width_mm,
            dash: style.dash_mm.clone(),
        });
        Ok(())
    }
    pub(super) fn triangle(&mut self, points: [P; 3], layer: &'static str) -> Result<(), String> {
        self.budget.work(3)?;
        finite_points(&points)?;
        self.budget.points(3)?;
        self.item()?;
        self.items.push(PaperPrimitive::Triangle { points, layer });
        Ok(())
    }
    pub(super) fn label(
        &mut self,
        point: P,
        value: &str,
        height: f64,
        centered: bool,
    ) -> Result<(), String> {
        finite_points(&[point])?;
        if !height.is_finite() || height <= 0. {
            return Err("Drawing graphics have invalid text height".into());
        }
        self.budget.work(value.len() as u64)?;
        self.budget.retained(value.len())?;
        self.item()?;
        self.items.push(PaperPrimitive::Text {
            layer: "ANNOTATION",
            point,
            value: value.into(),
            height,
            centered,
            rotation_deg: 0.,
            fitted_width: None,
        });
        Ok(())
    }
    pub(super) fn text(&mut self, point: P, value: &str, height: f64) -> Result<(), String> {
        self.budget.work(value.len() as u64)?;
        for (i, line) in value.lines().enumerate() {
            self.label(
                [point[0], point[1] + i as f64 * height * 1.4],
                line,
                height,
                false,
            )?;
        }
        Ok(())
    }
    pub(super) fn finish(self) -> Vec<PaperPrimitive> {
        self.items
    }

    /// Admit one already-built, bounded primitive without cloning its payload.
    /// Callers stage only one fixed-size graphical element at a time and charge
    /// its temporary storage separately; the shared sheet budget owns retention.
    pub(super) fn primitive(&mut self, primitive: PaperPrimitive) -> Result<(), String> {
        match &primitive {
            PaperPrimitive::Line {
                points,
                width,
                dash,
                ..
            } => {
                finite_points(points)?;
                if !width.is_finite()
                    || *width <= 0.
                    || dash.iter().any(|n| !n.is_finite() || *n <= 0.)
                {
                    return Err("Drawing graphics have invalid line style".into());
                }
                self.budget.work((points.len() + dash.len()) as u64)?;
                self.budget.points(points.len())?;
                self.budget.retained(
                    bytes(points.capacity(), size_of::<P>())?
                        .checked_add(bytes(dash.capacity(), size_of::<f64>())?)
                        .ok_or("Drawing graphics size overflow")?,
                )?;
            }
            PaperPrimitive::Triangle { points, .. } => {
                finite_points(points)?;
                self.budget.work(3)?;
                self.budget.points(3)?;
            }
            PaperPrimitive::Text {
                point,
                value,
                height,
                rotation_deg,
                fitted_width,
                ..
            } => {
                finite_points(&[*point])?;
                if !height.is_finite()
                    || *height <= 0.
                    || !rotation_deg.is_finite()
                    || fitted_width.is_some_and(|width| !width.is_finite() || width <= 0.)
                {
                    return Err("Drawing graphics have invalid text metrics".into());
                }
                self.budget.work(value.len() as u64)?;
                self.budget.retained(value.capacity())?;
            }
        }
        self.item()?;
        self.items.push(primitive);
        Ok(())
    }
}

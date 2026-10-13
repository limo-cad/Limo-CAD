//! SVG pattern tiles restart their dash sequence before section clipping.
use super::*;

pub(super) struct Pattern<'a> {
    lengths: &'a [f64],
    count: usize,
    period: f64,
    minimum: f64,
    spacing: f64,
    solid: DrawingLineStyleDto,
}

impl<'a> Pattern<'a> {
    pub(super) fn new(
        style: &'a DrawingLineStyleDto,
        spacing: f64,
        budget: &mut PaperGraphicsBudget,
    ) -> Result<Self, String> {
        budget.work(style.dash_mm.len() as u64)?;
        if !style.width_mm.is_finite()
            || style.width_mm <= 0.
            || style.dash_mm.iter().any(|n| !n.is_finite() || *n <= 0.)
        {
            return Err("Drawing graphics have invalid line style".into());
        }
        let repeat = if style.dash_mm.len().is_multiple_of(2) {
            1
        } else {
            2
        };
        let count = style
            .dash_mm
            .len()
            .checked_mul(repeat)
            .ok_or("Section dash count overflow")?;
        let period = style.dash_mm.iter().sum::<f64>() * repeat as f64;
        if !period.is_finite() || period <= 0. {
            return Err("Section dash period exceeds the supported range".into());
        }
        Ok(Self {
            lengths: &style.dash_mm,
            count,
            period,
            minimum: style.dash_mm.iter().copied().reduce(f64::min).unwrap(),
            spacing,
            solid: DrawingLineStyleDto {
                width_mm: style.width_mm,
                dash_mm: vec![],
            },
        })
    }

    pub(super) fn emit(
        &self,
        graphics: &mut Graphics<'_>,
        [first, last]: [f64; 2],
        across: f64,
        u: P,
        n: P,
    ) -> Result<(), String> {
        if first >= last {
            return Ok(());
        }
        let start = (first / self.spacing).floor();
        let end = (last / self.spacing).floor();
        if !start.is_finite()
            || !end.is_finite()
            || start < i64::MIN as f64
            || end >= i64::MAX as f64
        {
            return Err("Section dash tiles exceed the supported range".into());
        }
        let start = start as i64;
        let end = end as i64;
        let tiles = end
            .checked_sub(start)
            .and_then(|n| n.checked_add(1))
            .ok_or("Section dash tile count overflow")? as u64;
        let transitions = ((last - first) / self.minimum).ceil();
        if !transitions.is_finite() || transitions >= u64::MAX as f64 {
            return Err("Section dash work exceeds the supported range".into());
        }
        let work = (self.count as u64)
            .checked_add(3)
            .and_then(|n| n.checked_mul(tiles))
            .and_then(|n| n.checked_add(transitions as u64))
            .ok_or("Section dash work overflow")?;
        graphics.budget.work(work)?;
        let point = |along: f64| [along * u[0] + across * n[0], along * u[1] + across * n[1]];
        for tile in start..=end {
            let origin = tile as f64 * self.spacing;
            let boundary = (tile + 1) as f64 * self.spacing;
            if !origin.is_finite() || !boundary.is_finite() || boundary <= origin {
                return Err("Section dash tiles are below coordinate precision".into());
            }
            let mut from = first.max(origin);
            let to = last.min(boundary);
            if from >= to {
                continue;
            }
            let mut phase = (from - origin).rem_euclid(self.period);
            let mut index = 0;
            while index < self.count && phase >= self.lengths[index % self.lengths.len()] {
                phase -= self.lengths[index % self.lengths.len()];
                index += 1;
            }
            if index == self.count {
                return Err("Section dash phase exceeds coordinate precision".into());
            }
            let mut remaining = self.lengths[index % self.lengths.len()] - phase;
            while from < to {
                graphics.budget.work(1)?;
                let next = (from + remaining).min(to);
                if next <= from {
                    return Err("Section dashes are below coordinate precision".into());
                }
                if index % 2 == 0 {
                    graphics.line(&[point(from), point(next)], "HATCH", &self.solid)?;
                }
                from = next;
                index = (index + 1) % self.count;
                remaining = self.lengths[index % self.lengths.len()];
            }
        }
        Ok(())
    }
}

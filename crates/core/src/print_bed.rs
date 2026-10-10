use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintProfileSource {
    pub repository: String,
    pub revision: String,
    pub profile: String,
    pub files: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintNozzleMode {
    #[default]
    Main,
    Dual,
}

/// Resolved layout envelope; frozen in the document, not a process preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintBedDto {
    pub name: String,
    pub size_mm: [f64; 3],
    #[serde(default)]
    pub margin_mm: f64,
    #[serde(default)]
    pub nozzle_mode: PrintNozzleMode,
    #[serde(default)]
    pub origin_mm: [f64; 2],
    /// Convex constraints: model bounds must fit every region (both nozzles).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub printable_regions: Vec<Vec<[f64; 2]>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_regions: Vec<Vec<[f64; 2]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PrintProfileSource>,
}
impl Default for PrintBedDto {
    fn default() -> Self {
        embedded_printer_catalog().profiles[0].main.clone()
    }
}
impl PrintBedDto {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty()
            || self.name.chars().any(char::is_control)
            || self
                .size_mm
                .iter()
                .any(|v| !v.is_finite() || *v <= 0. || *v > 1e6)
            || !self.margin_mm.is_finite()
            || self.margin_mm < 0.
            || self.margin_mm * 2. >= self.size_mm[0].min(self.size_mm[1])
            || self
                .origin_mm
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 1e6)
        {
            return Err("Print bed needs a name, positive finite dimensions, and a margin smaller than half the bed".into());
        }
        for polygon in self.printable_regions.iter().chain(&self.excluded_regions) {
            if polygon.len() < 3
                || polygon.len() > 1024
                || polygon
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite() || v.abs() > 1e6)
                || !is_convex(polygon)
            {
                return Err("Print regions require convex polygons with at least three finite XY vertices and nonzero area".into());
            }
        }
        Ok(())
    }
    pub fn contains_xy_bounds(&self, min: [f64; 2], max: [f64; 2]) -> bool {
        if (0..2).any(|i| {
            min[i] < self.origin_mm[i] + self.margin_mm - 1e-5
                || max[i] > self.origin_mm[i] + self.size_mm[i] - self.margin_mm + 1e-5
        }) {
            return false;
        }
        let corners = [
            [min[0], min[1]],
            [max[0], min[1]],
            [max[0], max[1]],
            [min[0], max[1]],
        ];
        for p in &self.printable_regions {
            let orientation = signed_area(p).signum();
            for (a, b) in p.iter().zip(p.iter().cycle().skip(1)).take(p.len()) {
                let length = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
                if corners
                    .iter()
                    .any(|c| orientation * cross(*a, *b, *c) < (self.margin_mm - 1e-5) * length)
                {
                    return false;
                }
            }
        }
        for p in &self.excluded_regions {
            if (0..2).all(|i| {
                max[i]
                    > p.iter().map(|v| v[i]).fold(f64::INFINITY, f64::min) - self.margin_mm + 1e-5
                    && min[i]
                        < p.iter().map(|v| v[i]).fold(f64::NEG_INFINITY, f64::max) + self.margin_mm
                            - 1e-5
            }) {
                return false;
            }
        }
        true
    }
}
fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn signed_area(p: &[[f64; 2]]) -> f64 {
    p.iter()
        .zip(p.iter().cycle().skip(1))
        .take(p.len())
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum()
}
fn is_convex(p: &[[f64; 2]]) -> bool {
    let area = signed_area(p);
    area.abs() > 1e-8
        && (0..p.len()).all(|i| {
            let a = p[i];
            let b = p[(i + 1) % p.len()];
            (a[0] - b[0]).hypot(a[1] - b[1]) > 1e-8
                && p.iter().all(|c| cross(a, b, *c) * area.signum() >= -1e-8)
        })
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinterProfileDto {
    pub id: String,
    pub main: PrintBedDto,
    pub dual: PrintBedDto,
    pub extruders: Vec<PrinterExtruderDto>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinterExtruderDto {
    pub printable_region: Vec<[f64; 2]>,
    pub height_mm: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinterCatalogDto {
    pub schema_version: u32,
    pub profiles: Vec<PrinterProfileDto>,
}
pub fn embedded_printer_catalog() -> &'static PrinterCatalogDto {
    static CATALOG: std::sync::OnceLock<PrinterCatalogDto> = std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../data/printers.json"))
            .expect("validated embedded printer catalog")
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn printable_region_edges_use_the_same_millimeter_tolerance_as_the_envelope() {
        let mut bed = PrintBedDto::default();
        for reverse in [false, true] {
            for polygon in &mut bed.printable_regions {
                if reverse {
                    polygon.reverse();
                }
            }
            assert!(bed.contains_xy_bounds([-0.000005, 0.], [256.000005, 10.]));
            assert!(!bed.contains_xy_bounds([-0.0001, 0.], [256., 10.]));
            assert!(!bed.contains_xy_bounds([0., 0.], [256.0001, 10.]));
        }
        bed.printable_regions = vec![vec![[2., 2.], [8., 2.], [8., 8.], [2., 8.]]];
        bed.margin_mm = 1.;
        assert!(bed.contains_xy_bounds([2.999995, 2.999995], [7.000005, 7.000005]));
        assert!(!bed.contains_xy_bounds([2.9999, 3.], [7., 7.]));
    }
    #[test]
    fn embedded_regions_and_dual_origin() {
        for p in &embedded_printer_catalog().profiles {
            p.main.validate().unwrap();
            p.dual.validate().unwrap();
            assert_eq!(p.dual.origin_mm, [20.5, 0.]);
            assert!(!p.dual.contains_xy_bounds([0., 0.], [10., 10.]));
            assert!(p.dual.contains_xy_bounds([240., 0.], [256., 10.]));
            assert_eq!(p.main.size_mm[2], 261.);
        }
    }
    #[test]
    fn polygon_and_exclusion_are_conservative() {
        let mut bed = PrintBedDto {
            printable_regions: vec![vec![[0., 0.], [256., 0.], [0., 256.]]],
            ..Default::default()
        };
        assert!(!bed.contains_xy_bounds([200., 200.], [210., 210.]));
        bed.excluded_regions = vec![vec![[0., 0.], [10., 0.], [10., 10.], [0., 10.]]];
        assert!(!bed.contains_xy_bounds([0., 0.], [5., 5.]));
        assert!(bed.contains_xy_bounds([20., 20.], [25., 25.]));
    }
}

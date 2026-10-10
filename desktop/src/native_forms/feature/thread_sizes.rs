//! The desktop and web forms read the same conventional thread size data.
use limo_cad_solid::{
    HoleThreadDto, HoleThreadHand, HoleThreadRepresentation, HoleThreadSeries, HoleThreadStandard,
};
use std::sync::OnceLock;

pub(super) struct Preset {
    pub id: String,
    pub thread: HoleThreadDto,
    pub drill: f64,
}

pub(super) fn presets() -> &'static [Preset] {
    static PRESETS: OnceLock<Vec<Preset>> = OnceLock::new();
    PRESETS.get_or_init(|| {
        #[derive(serde::Deserialize)]
        struct Sizes {
            metric_coarse: Vec<(f64, f64, f64)>,
            metric_fine: Vec<(f64, f64, f64)>,
            unc: Vec<(String, f64, f64, f64, String)>,
            unf: Vec<(String, f64, f64, f64, String)>,
        }
        let sizes: Sizes =
            serde_json::from_str(include_str!("../../../../interface/thread-sizes.json"))
                .expect("Embedded thread sizes are tested at build time");
        let mut result = Vec::new();
        for (key, series, rows) in [
            (
                "metric_coarse",
                HoleThreadSeries::MetricCoarse,
                sizes.metric_coarse,
            ),
            (
                "metric_fine",
                HoleThreadSeries::MetricFine,
                sizes.metric_fine,
            ),
        ] {
            for (d, p, drill) in rows {
                result.push(Preset {
                    id: format!("{key}-{d}-{p}"),
                    drill,
                    thread: HoleThreadDto {
                        standard: HoleThreadStandard::IsoMetric,
                        series,
                        designation: format!("M{d} x {p} - 6H"),
                        class: "6H".into(),
                        nominal_diameter: d,
                        pitch: p,
                        threads_per_inch: None,
                        tap_drill_designation: Some(format!("{drill} mm")),
                        hand: HoleThreadHand::Right,
                        depth: None,
                        representation: HoleThreadRepresentation::Simplified,
                        rounded_profile: None,
                    },
                });
            }
        }
        for (key, series, rows) in [
            ("unc", HoleThreadSeries::Unc, sizes.unc),
            ("unf", HoleThreadSeries::Unf, sizes.unf),
        ] {
            for (size, d, tpi, drill_diameter, drill) in rows {
                result.push(Preset {
                    id: format!("{key}-{size}-{tpi}"),
                    drill: drill_diameter * 25.4,
                    thread: HoleThreadDto {
                        standard: HoleThreadStandard::UnifiedInch,
                        series,
                        designation: format!("{size}-{tpi} {}-2B", key.to_uppercase()),
                        class: "2B".into(),
                        nominal_diameter: d * 25.4,
                        pitch: 25.4 / tpi,
                        threads_per_inch: Some(tpi),
                        tap_drill_designation: Some(drill),
                        hand: HoleThreadHand::Right,
                        depth: None,
                        representation: HoleThreadRepresentation::Simplified,
                        rounded_profile: None,
                    },
                });
            }
        }
        result
    })
}

impl Preset {
    pub fn external(&self) -> HoleThreadDto {
        let mut thread = self.thread.clone();
        let class = if thread.standard == HoleThreadStandard::IsoMetric {
            "6g"
        } else {
            "2A"
        };
        thread.designation = thread
            .designation
            .trim_end_matches(&thread.class)
            .to_owned()
            + class;
        thread.class = class.into();
        thread.tap_drill_designation = None;
        thread
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_embedded_size_has_a_unique_id_and_valid_internal_and_external_fit() {
        let mut ids = std::collections::HashSet::new();
        assert_eq!(presets().len(), 95);
        for p in presets() {
            assert!(ids.insert(&p.id));
            let t = p.external();
            limo_cad_solid::validate_external_thread(&t, t.nominal_diameter).unwrap();
            let hole: limo_cad_solid::HoleRequest=serde_json::from_value(serde_json::json!({
                "body_id":1,"face_id":1,"position":{"x":0.,"y":0.},"diameter":p.drill,"thread":p.thread
            })).unwrap();
            limo_cad_solid::validate_hole(&hole).unwrap();
        }
        let m6 = presets()
            .iter()
            .find(|p| p.id == "metric_coarse-6-1")
            .unwrap()
            .external();
        assert_eq!(m6.designation, "M6 x 1 - 6g");
        let quarter = presets()
            .iter()
            .find(|p| p.id == "unc-1/4-20")
            .unwrap()
            .external();
        assert_eq!(quarter.designation, "1/4-20 UNC-2A");
        assert_eq!(quarter.nominal_diameter, 6.35);
        assert_eq!(quarter.pitch, 1.27);
    }
}

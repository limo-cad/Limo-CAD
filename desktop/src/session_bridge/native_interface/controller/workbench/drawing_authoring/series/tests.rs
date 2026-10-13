use super::super::{
    draft::{Draft, Selection},
    fields::{self, Id},
    tests as fixture,
};
use super::*;
use limo_cad_interface::{ControlInput, DocumentContext};

fn stamp() -> Stamp {
    Stamp {
        owner: DocumentContext {
            window_id: "main".into(),
            document_id: "series".into(),
            epoch: 3,
        },
        revision: 7,
        sheet_id: 1,
    }
}
pub(in super::super) fn refs() -> Vec<DrawingTopologyAnchorRefDto> {
    let p = fixture::projection();
    let a = anchors::endpoint_ref(&p.anchors[4], &p);
    let b = anchors::endpoint_ref(&p.anchors[5], &p);
    let mut c = b.clone();
    c.edge_id = limo_cad_core::EdgeId(9);
    c.edge_key = "third".into();
    c.fallback_point = [70., 0., 6.];
    vec![a, b, c]
}
pub(in super::super) fn layouts() -> [Option<DrawingChainDimensionLayout>; 4] {
    [
        Some(DrawingChainDimensionLayout::Chain),
        Some(DrawingChainDimensionLayout::Baseline),
        Some(DrawingChainDimensionLayout::Continued),
        None,
    ]
}
pub(in super::super) fn document(
    layout: Option<DrawingChainDimensionLayout>,
) -> DrawingDocumentDto {
    let mut refs = refs();
    if layout.is_none() {
        refs.truncate(2);
    }
    create(&fixture::document(), 1, 1, refs, layout).unwrap()
}
fn draft(document: &DrawingDocumentDto) -> Draft {
    Draft::new(
        document,
        Selection {
            sheet_id: 1,
            annotation_id: 4,
        },
    )
    .unwrap()
}
fn set(fields: &mut [fields::Field], id: Id, value: &str) {
    fields::edit(fields, id, &ControlInput::SetValue(value.into())).unwrap();
}

#[test]
fn release_endpoint_tools_create_exact_defaults_and_preserve_every_saved_record() {
    for layout in layouts() {
        let before = fixture::document();
        let refs = refs();
        let mut placement = Placement::default();
        let stamp = stamp();
        assert!(placement
            .click(&stamp, 1, refs[0].clone(), layout, &before)
            .unwrap()
            .is_none());
        assert!(placement
            .click(&stamp, 1, refs[0].clone(), layout, &before)
            .unwrap()
            .is_none());
        assert_eq!(placement.picks.len(), 1);
        let second = placement
            .click(&stamp, 1, refs[1].clone(), layout, &before)
            .unwrap();
        let next = if layout.is_some() {
            assert!(second.is_none());
            placement
                .click(&stamp, 1, refs[2].clone(), layout, &before)
                .unwrap()
                .unwrap()
        } else {
            second.unwrap()
        };
        assert!(placement.picks.is_empty());
        let mut expected = before.clone();
        expected.next_annotation_id += 1;
        expected.sheets[0]
            .annotations
            .push(next.sheets[0].annotations.last().unwrap().clone());
        expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
        assert_eq!(next, expected);
        match next.sheets[0].annotations.last().unwrap() {
            DrawingAnnotationDto::ChainDimension {
                anchors,
                layout: actual,
                mode,
                offset,
                spacing,
                precision,
                presentation,
                ..
            } => {
                assert_eq!(anchors, &refs);
                assert_eq!(Some(*actual), layout);
                assert_eq!(*mode, DrawingLinearDimensionMode::Aligned);
                assert_eq!((*offset, *spacing, *precision), (12., 7., 2));
                assert_eq!(presentation, &Default::default());
            }
            DrawingAnnotationDto::OrdinateDimension {
                origin,
                target,
                axis,
                offset,
                precision,
                presentation,
                ..
            } => {
                assert_eq!((origin, target), (&refs[0], &refs[1]));
                assert_eq!(*axis, DrawingOrdinateAxis::Both);
                assert_eq!((*offset, *precision), (10., 2));
                assert_eq!(presentation, &Default::default());
            }
            _ => panic!("Wrong shared annotation"),
        }
    }
}

#[test]
fn series_picks_restart_for_view_receipt_or_owner_and_reject_circle_creation() {
    let document = fixture::document();
    let refs = refs();
    let stamp = stamp();
    let mut p = Placement::default();
    let layout = Some(DrawingChainDimensionLayout::Continued);
    p.click(&stamp, 1, refs[0].clone(), layout, &document)
        .unwrap();
    p.click(&stamp, 2, refs[1].clone(), layout, &document)
        .unwrap();
    assert_eq!(p.picks, vec![refs[1].clone()]);
    let mut later = stamp.clone();
    later.revision += 1;
    p.observe(&later);
    assert!(p.picks.is_empty());
    p.click(&stamp, 1, refs[0].clone(), layout, &document)
        .unwrap();
    later = stamp.clone();
    later.owner.epoch += 1;
    p.observe(&later);
    assert!(p.picks.is_empty());
    let mut circle = refs[0].clone();
    circle.circle_center = true;
    assert!(p.click(&stamp, 1, circle, layout, &document).is_err());
    assert!(p.picks.is_empty());
    let mut exhausted = document;
    exhausted.next_annotation_id = u64::MAX;
    assert!(create(&exhausted, 1, 1, refs, layout).is_err());
}

#[test]
fn loaded_series_keep_all_anchors_and_full_presentation_through_conditional_fields() {
    for layout in layouts() {
        for count in if layout.is_some() {
            vec![2, 256]
        } else {
            vec![2]
        } {
            let mut before = document(layout);
            if let DrawingAnnotationDto::ChainDimension { anchors, .. } =
                before.sheets[0].annotations.last_mut().unwrap()
            {
                *anchors = (0..count)
                    .map(|i| {
                        let mut a = refs()[0].clone();
                        a.edge_id = limo_cad_core::EdgeId(i + 1);
                        a.edge_key = format!("loaded-{i}");
                        a.fallback_point = [i as f64, 0., 6.];
                        a
                    })
                    .collect();
            }
            before.sheets[0].release.status = DrawingReleaseStatus::Released;
            before.validate().unwrap();
            let mut d = draft(&before);
            let mut f = fields::from_annotation(d.annotation());
            fields::apply(&mut d, &f).unwrap();
            assert_eq!(d.apply(&before).unwrap(), before);
            assert_eq!(f.iter().any(|f| f.id == Id::Prefix), layout.is_some());
            for (id, value) in [
                (Id::Tolerance, "deviation"),
                (Id::Upper, "0.25"),
                (Id::Lower, "-0.1"),
                (Id::Basic, "true"),
                (Id::Fit, "H7"),
                (Id::Dual, "true"),
                (Id::DualUnit, "inch"),
                (Id::DualPrecision, "6"),
                (Id::DualPlacement, "stacked"),
                (Id::Precision, "6"),
                (Id::Offset, "-9"),
            ] {
                set(&mut f, id, value);
            }
            if layout.is_some() {
                set(&mut f, Id::Spacing, "0");
                set(&mut f, Id::Prefix, "Caf\u{e9} ");
                set(&mut f, Id::Suffix, " exact");
            } else {
                set(&mut f, Id::Axis, "y");
            }
            fields::apply(&mut d, &f).unwrap();
            let after = d.apply(&before).unwrap();
            let mut expected = before.clone();
            expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
            let a = expected.sheets[0].annotations.last_mut().unwrap();
            let p = match a {
                DrawingAnnotationDto::ChainDimension {
                    offset,
                    spacing,
                    precision,
                    prefix,
                    suffix,
                    presentation,
                    ..
                } => {
                    *offset = -9.;
                    *spacing = 0.;
                    *precision = 6;
                    *prefix = "Caf\u{e9} ".into();
                    *suffix = " exact".into();
                    presentation
                }
                DrawingAnnotationDto::OrdinateDimension {
                    offset,
                    axis,
                    precision,
                    presentation,
                    ..
                } => {
                    *offset = -9.;
                    *axis = DrawingOrdinateAxis::Y;
                    *precision = 6;
                    presentation
                }
                _ => unreachable!(),
            };
            p.tolerance = DrawingDimensionToleranceDto {
                mode: DrawingDimensionToleranceMode::Deviation,
                upper: 0.25,
                lower: -0.1,
            };
            p.basic = true;
            p.reference = false;
            p.fit_class = "H7".into();
            p.dual_units = Some(DrawingDualUnitDto {
                unit: DrawingSecondaryUnit::Inch,
                precision: 6,
                placement: DrawingDualUnitPlacement::Stacked,
            });
            assert_eq!(after, expected);
            for (id, value) in [
                (Id::Precision, "7"),
                (Id::Offset, "NaN"),
                (Id::DualPrecision, "7"),
            ] {
                let mut d = draft(&before);
                let mut f = fields::from_annotation(after.sheets[0].annotations.last().unwrap());
                set(&mut f, id, value);
                assert!(fields::apply(&mut d, &f)
                    .and_then(|_| d.apply(&before))
                    .is_err());
            }
            if layout.is_some() {
                let mut f = fields::from_annotation(d.annotation());
                set(&mut f, Id::Spacing, "-1");
                assert!(fields::apply(&mut d, &f)
                    .and_then(|_| d.apply(&before))
                    .is_err());
            }
        }
    }
}

#[test]
fn every_series_layout_drags_from_original_first_pair_without_spacing_or_reference_changes() {
    for layout in layouts().into_iter().flatten() {
        for (mode, delta, increment) in [
            (DrawingLinearDimensionMode::Horizontal, [8., -3.], -3.),
            (DrawingLinearDimensionMode::Vertical, [8., -3.], 8.),
            (DrawingLinearDimensionMode::Aligned, [5., 5.], 1.),
        ] {
            let mut before = document(Some(layout));
            if let DrawingAnnotationDto::ChainDimension { mode: m, .. } =
                before.sheets[0].annotations.last_mut().unwrap()
            {
                *m = mode;
            }
            let mut d = draft(&before);
            d.move_linear([0., 0.], [4., 3.], [20., 30.]).unwrap();
            d.move_linear([0., 0.], [4., 3.], delta).unwrap();
            let mut expected = before.clone();
            if let DrawingAnnotationDto::ChainDimension { offset, .. } =
                expected.sheets[0].annotations.last_mut().unwrap()
            {
                *offset += increment;
            }
            assert_eq!(d.apply(&before).unwrap(), expected);
            assert!(d
                .move_linear([0., 0.], [4., 3.], [f64::INFINITY, 0.])
                .is_err());
        }
    }
    for (delta, offset) in [
        ([5., -2.], 15.),
        ([2., -5.], 5.),
        ([5., -5.], 5.),
        ([-20., 1.], -10.),
        ([0., 0.], 10.),
    ] {
        let before = document(None);
        let mut d = draft(&before);
        d.move_ordinate([80., 30.]).unwrap();
        d.move_ordinate(delta).unwrap();
        let mut expected = before.clone();
        if let DrawingAnnotationDto::OrdinateDimension { offset: o, .. } =
            expected.sheets[0].annotations.last_mut().unwrap()
        {
            *o = offset;
        }
        assert_eq!(d.apply(&before).unwrap(), expected);
    }
}

#[test]
fn shared_series_anchor_limits_and_spacing_validation_remain_authoritative() {
    for count in [1, 2, 256, 257] {
        let mut document = document(Some(DrawingChainDimensionLayout::Chain));
        if let DrawingAnnotationDto::ChainDimension { anchors, .. } =
            document.sheets[0].annotations.last_mut().unwrap()
        {
            anchors.resize(count, refs()[0].clone());
        }
        assert_eq!(document.validate().is_ok(), (2..=256).contains(&count));
    }
    for value in [-1., 0., 7., f64::INFINITY, f64::NAN] {
        let mut document = document(Some(DrawingChainDimensionLayout::Continued));
        if let DrawingAnnotationDto::ChainDimension { spacing, .. } =
            document.sheets[0].annotations.last_mut().unwrap()
        {
            *spacing = value;
        }
        assert_eq!(
            document.validate().is_ok(),
            value.is_finite() && value >= 0.
        );
    }
}

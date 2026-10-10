//! Presentation of typed feature fields using the product's existing dictionaries.
//! Values, user-authored names, operation IDs and selection identities stay intact.
use crate::app_preferences::{locale::translate, Locale};
use crate::native_forms::{SolidField as F, SolidFormKind as K};
use bevy::prelude::World;
use limo_cad_interface::Field;

pub(super) fn apply(world: &World, panel: &mut super::super::FeaturePanel) {
    let locale = crate::native_viewport::localization::locale(world);
    if locale == Locale::En {
        return;
    }
    let title = title_key(panel.kind);
    let name = translate(locale, title);
    panel.title = if panel.title.starts_with("Edit ") {
        let key = match panel.kind {
            K::Extrude => "extrude.editTitle",
            K::Revolve => "revolve.editTitle",
            K::Sweep => "sweep.editTitle",
            K::Loft => "loft.editTitle",
            K::Rib => "rib.editTitle",
            K::Hole => "hole.editTitle",
            K::Fillet => "solidEdge.editFillet",
            K::Chamfer => "solidEdge.editChamfer",
            K::OffsetPlane | K::Midplane | K::AnglePlane => "constructionPlane.editTitle",
            _ => "bodyFeature.editTitle",
        };
        translate(locale, key).replace("{title}", name)
    } else {
        name.to_owned()
    };
    for row in &mut panel.fields {
        if !matches!(row.value, Field::None) {
            if let Some(key) = field_key(panel.kind, row.field, &row.label) {
                let text = translate(locale, key);
                row.label = if !row.label.contains("(mm)") {
                    text.replace("(mm)", "").trim().to_owned()
                } else {
                    text.to_owned()
                };
            }
        }
        if let Field::Choice { options, .. } = &mut row.value {
            for option in options {
                if let Some(key) = option_key(row.field, &option.value) {
                    option.label = translate(locale, key).to_owned();
                }
            }
        }
    }
}

fn title_key(kind: K) -> &'static str {
    match kind {
        K::Extrude => "extrude.title",
        K::Revolve => "revolve.title",
        K::Sweep => "sweep.title",
        K::Loft => "loft.title",
        K::Rib => "rib.title",
        K::Hole => "hole.title",
        K::Fillet => "solidEdge.fillet",
        K::Chamfer => "solidEdge.chamfer",
        K::ExternalThread => "bodyFeature.titleExternalThread",
        K::Shell => "bodyFeature.titleShell",
        K::MoveCopy => "bodyFeature.titleMoveCopy",
        K::Combine => "bodyFeature.titleCombine",
        K::Mirror => "bodyFeature.titleMirror",
        K::SplitBody => "bodyFeature.titleSplitBody",
        K::RectangularPattern => "bodyFeature.titleRectangularPattern",
        K::CircularPattern => "bodyFeature.titleCircularPattern",
        K::OffsetPlane => "constructionPlane.offsetPlane",
        K::Midplane => "constructionPlane.midplane",
        K::AnglePlane => "constructionPlane.planeAtAngle",
    }
}

fn field_key(kind: K, field: F, label: &str) -> Option<&'static str> {
    Some(match field {
        F::Source => match kind {
            K::Extrude => "extrude.sources",
            K::Loft => "loft.sections",
            _ => "extrude.profiles",
        },
        F::Operation => "extrude.operation",
        F::Extent => "extrude.extent",
        F::Distance => match kind {
            K::Extrude if label == "Side 1 distance" => "extrude.firstDistance",
            K::Extrude if label == "Total distance" => "extrude.totalDistance",
            K::ExternalThread => "bodyFeature.threadLengthMm",
            K::Hole => "hole.threadDepth",
            K::RectangularPattern => "bodyFeature.spacingMm",
            K::OffsetPlane => "constructionPlane.offsetDistance",
            _ => "extrude.distance",
        },
        F::SecondDistance => {
            if kind == K::RectangularPattern {
                "bodyFeature.spacingMm"
            } else {
                "extrude.secondDistance"
            }
        }
        F::Taper => "extrude.taper",
        F::Flip => match kind {
            K::Hole => "hole.flip",
            K::Rib => "rib.flip",
            K::ExternalThread => "bodyFeature.flipThread",
            _ => "extrude.flip",
        },
        F::Targets => "extrude.targetBodies",
        F::StopFace => "extrude.targetFace",
        F::Axis => "revolve.axis",
        F::AxisLine => "revolve.axisLine",
        F::OriginX => {
            if kind == K::Hole {
                "hole.positionX"
            } else {
                "revolve.originX"
            }
        }
        F::OriginY => {
            if kind == K::Hole {
                "hole.positionY"
            } else {
                "revolve.originY"
            }
        }
        F::HolePositionU(_) => "hole.positionX",
        F::HolePositionV(_) => "hole.positionY",
        F::DirectionX => "revolve.directionX",
        F::DirectionY => "revolve.directionY",
        F::Angle => {
            if kind == K::CircularPattern {
                "bodyFeature.totalAngleDegrees"
            } else {
                "bodyFeature.angleDegrees"
            }
        }
        F::Path => {
            if kind == K::Rib {
                "rib.centerlines"
            } else if kind == K::Loft {
                "loft.centerline"
            } else {
                "sweep.path"
            }
        }
        F::Guide => "sweep.guideRail",
        F::GuideEnabled => "sweep.useGuideRail",
        F::CenterlineEnabled => "loft.useCenterline",
        F::Orientation => "sweep.orientation",
        F::Transition => "sweep.cornerTransition",
        F::ForceC1 => "sweep.forceC1",
        F::Ruled => "loft.ruled",
        F::Continuity => "loft.sectionContinuity",
        F::Thickness => {
            if kind == K::Shell {
                "bodyFeature.wallThicknessMm"
            } else {
                "rib.thickness"
            }
        }
        F::Symmetric => "rib.symmetric",
        F::Edges => "solidEdge.edges",
        F::Radius => "solidEdge.radius",
        F::TangentChain => "solidEdge.tangentChain",
        F::Faces => "bodyFeature.facesToRemove",
        F::Inward => "bodyFeature.offsetInward",
        F::TargetBody => "bodyFeature.targetBody",
        F::ToolBodies => "bodyFeature.toolBodies",
        F::KeepTools => "bodyFeature.keepTools",
        F::FirstPlane => {
            if kind == K::Midplane {
                "constructionPlane.firstReference"
            } else {
                "constructionPlane.referencePlane"
            }
        }
        F::SecondPlane => "constructionPlane.secondReference",
        F::AxisEdge => "constructionPlane.straightAxisEdge",
        F::Bodies => "bodyFeature.bodies",
        F::DirectionEdge => "bodyFeature.directionReference",
        F::SecondDirectionEdge => "bodyFeature.secondDirectionReference",
        F::SecondEnabled => "bodyFeature.addSecondDirection",
        F::Count | F::SecondCount => "bodyFeature.count",
        F::MoveMode => "bodyFeature.moveType",
        F::MoveObjectType => "bodyFeature.objectType",
        F::Copy => "bodyFeature.createCopy",
        F::FromPoint => "bodyFeature.fromPoint",
        F::ToPoint => "bodyFeature.toPoint",
        F::PivotPoint => "bodyFeature.rotationPivot",
        F::HoleSupport => "hole.supportFace",
        F::HolePositions => "hole.positions",
        F::HoleStyle => "hole.style",
        F::Threaded => "hole.threaded",
        F::HoleDiameter => "hole.diameter",
        F::HoleDepth => "hole.depth",
        F::CounterboreDiameter => "hole.counterboreDiameter",
        F::CounterboreDepth => "hole.counterboreDepth",
        F::CountersinkDiameter => "hole.countersinkDiameter",
        F::CountersinkAngle => "hole.angle",
        F::BottomStyle => "hole.bottomStyle",
        F::DrillPointAngle => "hole.drillPointAngle",
        F::Cylinder => "bodyFeature.cylindricalSurface",
        F::ThreadStandard => "hole.threadStandard",
        F::ThreadSeries => "hole.threadSeries",
        F::ThreadPreset => "hole.threadSize",
        F::Diameter => "hole.majorDiameter",
        F::Pitch => "hole.pitch",
        F::ThreadClass => {
            if kind == K::Hole {
                "hole.threadClass"
            } else {
                "bodyFeature.toleranceClass"
            }
        }
        F::Designation => "bodyFeature.designation",
        F::ThreadHand => "hole.threadHand",
        F::Representation => "bodyFeature.representation",
        F::FullThread => {
            if kind == K::Hole {
                "hole.fullThreadDepth"
            } else {
                "bodyFeature.fullThread"
            }
        }
        _ => return None,
    })
}

fn option_key(field: F, value: &str) -> Option<&'static str> {
    Some(match (field, value) {
        (F::Operation, "new_body") => "extrude.newBody",
        (F::Operation, "join") => "extrude.join",
        (F::Operation, "cut") => "extrude.cut",
        (F::Operation, "intersect") => "extrude.intersect",
        (F::Extent, "distance") => "extrude.distance",
        (F::Extent, "symmetric") => "extrude.symmetric",
        (F::Extent, "two_sides") => "extrude.twoSides",
        (F::Extent, "through_all") => "extrude.throughAll",
        (F::Extent, "to_face") => "extrude.toFace",
        (F::Extent, "to_next") => "rib.toNext",
        (F::Axis, "x") => "revolve.xAxis",
        (F::Axis, "y") => "revolve.yAxis",
        (F::Axis, "line") => "revolve.sketchLine",
        (F::Axis, "custom") => "revolve.customAxis",
        (F::Orientation, "corrected_frenet") => "sweep.correctedFrenet",
        (F::Orientation, "frenet") => "sweep.frenet",
        (F::Orientation, "fixed") => "sweep.fixedProfile",
        (F::Transition, "transformed") => "sweep.transformed",
        (F::Transition, "right_corner") => "sweep.rightCorner",
        (F::Transition, "round_corner") => "sweep.roundCorner",
        (F::Continuity, "g0") => "loft.g0Position",
        (F::Continuity, "g1") => "loft.g1Tangent",
        (F::Continuity, "g2") => "loft.g2Curvature",
        (F::MoveObjectType, "bodies") => "bodyFeature.bodies",
        (F::MoveObjectType, "component") => "bodyFeature.component",
        (F::MoveMode, "free") => "bodyFeature.moveFree",
        (F::MoveMode, "translate") => "bodyFeature.moveTranslate",
        (F::MoveMode, "rotate") => "bodyFeature.moveRotate",
        (F::MoveMode, "point_to_point") => "bodyFeature.movePointToPoint",
        (F::HoleStyle, "simple") => "hole.simple",
        (F::HoleStyle, "counterbore") => "hole.counterbore",
        (F::HoleStyle, "countersink") => "hole.countersink",
        (F::BottomStyle, "flat") => "hole.flatBottom",
        (F::BottomStyle, "drill_point") => "hole.drillPoint",
        (F::ThreadStandard, "iso_metric") => "hole.isoMetric",
        (F::ThreadStandard, "unified_inch") => "bodyFeature.unifiedInch",
        (F::ThreadStandard, "custom_trapezoidal") => "hole.customRoundedTrapezoidal",
        (F::ThreadSeries, "metric_coarse") => "hole.metricCoarse",
        (F::ThreadSeries, "metric_fine") => "hole.metricFine",
        (F::ThreadSeries, "rounded") => "hole.roundedProfile",
        (F::ThreadSeries, "unc") => "hole.unc",
        (F::ThreadSeries, "unf") => "hole.unf",
        (F::ThreadHand, "right") => "hole.rightHand",
        (F::ThreadHand, "left") => "hole.leftHand",
        (F::Representation, "simplified") => "hole.simplifiedThread",
        (F::Representation, "modeled") => "hole.modeledThread",
        _ => return None,
    })
}

pub(super) fn group<'a>(world: &World, label: &'a str) -> &'a str {
    let key = match label {
        "Translation" => "bodyFeature.translation",
        "Rotation" => "bodyFeature.rotation",
        "Rotation pivot" => "bodyFeature.rotationPivot",
        "Direction" => "bodyFeature.direction",
        "Axis" => "bodyFeature.axis",
        "From point" => "bodyFeature.fromPoint",
        "To point" => "bodyFeature.toPoint",
        "Axis origin" => "bodyFeature.axisOrigin",
        "Axis direction" => "bodyFeature.axisDirection",
        "First direction" => "bodyFeature.firstDirection",
        "Second direction" => "bodyFeature.secondDirection",
        _ => return label,
    };
    translate(crate::native_viewport::localization::locale(world), key)
}

pub(super) fn caption(world: &World, key: &str) -> Option<&'static str> {
    let locale = crate::native_viewport::localization::locale(world);
    if locale == Locale::En {
        return None;
    }
    let translation = match key {
        "apply" => "bodyFeature.ok",
        "cancel" => "file.cancel",
        _ if key.ends_with("-clear") => "solidEdge.clear",
        _ => return None,
    };
    Some(translate(locale, translation))
}

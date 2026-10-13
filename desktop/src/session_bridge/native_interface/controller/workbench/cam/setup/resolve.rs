//! Host-side resolution of the shared stock intent, matching cam/geometry.ts.
use super::*;
use limo_cad_cam::{CamStockFace, CamStockShape};

pub(super) fn bounds(points: impl Iterator<Item = Point3Dto>) -> Option<StockBoxDto> {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for p in points {
        for (i, v) in [p.x, p.y, p.z].into_iter().enumerate() {
            if !v.is_finite() {
                return None;
            }
            min[i] = min[i].min(v);
            max[i] = max[i].max(v);
        }
    }
    min.into_iter()
        .chain(max)
        .all(f64::is_finite)
        .then(|| StockBoxDto {
            min: Point3Dto::new(min[0], min[1], min[2]),
            max: Point3Dto::new(max[0], max[1], max[2]),
        })
}
fn corners(b: StockBoxDto) -> impl Iterator<Item = Point3Dto> {
    [b.min.x, b.max.x].into_iter().flat_map(move |x| {
        [b.min.y, b.max.y].into_iter().flat_map(move |y| {
            [b.min.z, b.max.z]
                .into_iter()
                .map(move |z| Point3Dto::new(x, y, z))
        })
    })
}
fn to_setup(p: Point3Dto, w: WorkCoordinateSystemDto) -> Point3Dto {
    let d = [p.x - w.origin.x, p.y - w.origin.y, p.z - w.origin.z];
    let dot = |axis: [f64; 3]| d.into_iter().zip(axis).map(|(a, b)| a * b).sum();
    Point3Dto::new(dot(w.x_axis), dot(w.y_axis), dot(w.z_axis))
}
pub(super) fn box_to_setup(
    b: StockBoxDto,
    w: WorkCoordinateSystemDto,
) -> Result<StockBoxDto, String> {
    bounds(corners(b).map(|p| to_setup(p, w)))
        .ok_or("Stock coordinates exceed the finite range".into())
}
pub(super) fn box_to_model(
    b: StockBoxDto,
    w: WorkCoordinateSystemDto,
) -> Result<StockBoxDto, String> {
    bounds(corners(b).map(|p| {
        Point3Dto::new(
            w.origin.x + p.x * w.x_axis[0] + p.y * w.y_axis[0] + p.z * w.z_axis[0],
            w.origin.y + p.x * w.x_axis[1] + p.y * w.y_axis[1] + p.z * w.z_axis[1],
            w.origin.z + p.x * w.x_axis[2] + p.y * w.y_axis[2] + p.z * w.z_axis[2],
        )
    }))
    .ok_or("Stock coordinates exceed the finite range".into())
}
pub(super) fn orientation(
    value: &str,
    mut existing: WorkCoordinateSystemDto,
) -> Result<WorkCoordinateSystemDto, String> {
    if value == "keep" {
        return Ok(existing);
    }
    let (down, angle) = if let Some(angle) = value.strip_prefix("down") {
        (true, angle)
    } else if let Some(angle) = value.strip_prefix("up") {
        (false, angle)
    } else {
        return Err("Choose a WCS orientation".into());
    };
    let (c, s) = match angle {
        "0" => (1., 0.),
        "90" => (0., 1.),
        "180" => (-1., 0.),
        "270" => (0., -1.),
        _ => return Err("Choose a quarter-turn WCS orientation".into()),
    };
    existing.x_axis = [c, s, 0.];
    existing.y_axis = if down { [s, -c, 0.] } else { [-s, c, 0.] };
    existing.z_axis = [0., 0., if down { -1. } else { 1. }];
    Ok(existing)
}
pub(super) enum Shape {
    Box,
    Cylinder(Point3Dto, f64),
    Hex(Point3Dto, f64),
    Rest(u64),
    Model(u64),
}
pub(super) fn resolved(shape: Shape, wcs: WorkCoordinateSystemDto) -> Value {
    match shape {
        Shape::Box => json!({"shape":"box"}),
        Shape::Cylinder(center, radius) => {
            let c = to_setup(center, wcs);
            json!({"shape":"cylinder","center":{"x":c.x,"y":c.y},"radius":radius})
        }
        Shape::Hex(center, across_flats) => {
            let c = to_setup(center, wcs);
            json!({"shape":"hex","center":{"x":c.x,"y":c.y},"across_flats":across_flats})
        }
        Shape::Rest(id) => json!({"shape":"rest","source_setup_id":id}),
        Shape::Model(id) => json!({"shape":"model_body","body_id":id}),
    }
}
fn centered_box(x: f64, y: f64, z: f64, sx: f64, sy: f64, sz: f64) -> StockBoxDto {
    StockBoxDto {
        min: Point3Dto::new(x - sx / 2., y - sy / 2., z),
        max: Point3Dto::new(x + sx / 2., y + sy / 2., z + sz),
    }
}
pub(super) fn stock(
    spec: &CamStockSpecDto,
    model: StockBoxDto,
    legacy: StockBoxDto,
    source: Option<&CamSetupDto>,
    wcs: WorkCoordinateSystemDto,
) -> Result<(StockBoxDto, Shape), String> {
    let sq3 = 3_f64.sqrt();
    let cx = (model.min.x + model.max.x) * 0.5;
    let cy = (model.min.y + model.max.y) * 0.5;
    let round_orientation = |shape: CamStockShape| -> Result<bool, String> {
        if !matches!(shape, CamStockShape::Cylinder | CamStockShape::Hex) {
            return Ok(false);
        }
        if wcs.z_axis[0].abs() > 1e-9
            || wcs.z_axis[1].abs() > 1e-9
            || (wcs.z_axis[2].abs() - 1.).abs() > 1e-9
        {
            return Err("Round stock needs model Z up or down; choose a WCS orientation".into());
        }
        if shape == CamStockShape::Hex
            && (wcs.x_axis[0].abs().max(wcs.x_axis[1].abs()) - 1.).abs() > 1e-9
        {
            return Err("Hex stock needs a quarter-turn WCS orientation".into());
        }
        Ok(wcs.x_axis[1].abs() > 0.5)
    };
    match spec {
        CamStockSpecDto::LegacyBox => Ok((legacy, Shape::Box)),
        CamStockSpecDto::ModelBody { body_id } => Ok((model, Shape::Model(*body_id))),
        CamStockSpecDto::RestFromSetup { setup_id } => {
            let source = source.ok_or("Choose a source setup")?;
            Ok((
                match source.stock_model_box {
                    Some(bounds) => bounds,
                    None => box_to_model(source.stock, source.wcs)?,
                },
                Shape::Rest(*setup_id),
            ))
        }
        CamStockSpecDto::Fixed {
            shape,
            size,
            placement,
        } => {
            let swap = round_orientation(*shape)?;
            let (sx, sy) = match shape {
                CamStockShape::Box => (size.x, size.y),
                CamStockShape::Cylinder => (size.x, size.x),
                CamStockShape::Hex => {
                    if swap {
                        (size.x * 2. / sq3, size.x)
                    } else {
                        (size.x, size.x * 2. / sq3)
                    }
                }
                CamStockShape::ModelBody => {
                    return Err("Choose the Modeled stock body definition".into())
                }
            };
            if sx <= 0. || sy <= 0. || size.z <= 0. {
                return Err("Fixed stock dimensions must be positive".into());
            }
            let face = if placement.center {
                None
            } else {
                placement.face
            };
            let axis = |min: f64, max: f64, size: f64, axis: usize| {
                let (low, high) = match axis {
                    0 => (CamStockFace::XMin, CamStockFace::XMax),
                    1 => (CamStockFace::YMin, CamStockFace::YMax),
                    _ => (CamStockFace::ZMin, CamStockFace::ZMax),
                };
                if face == Some(low) {
                    (min - placement.offset, min - placement.offset + size)
                } else if face == Some(high) {
                    (max + placement.offset - size, max + placement.offset)
                } else if axis == 2 {
                    (min, min + size)
                } else {
                    ((min + max - size) * 0.5, (min + max + size) * 0.5)
                }
            };
            let (x0, x1) = axis(model.min.x, model.max.x, sx, 0);
            let (y0, y1) = axis(model.min.y, model.max.y, sy, 1);
            let (z0, z1) = axis(model.min.z, model.max.z, size.z, 2);
            let b = StockBoxDto {
                min: Point3Dto::new(x0, y0, z0),
                max: Point3Dto::new(x1, y1, z1),
            };
            let center = Point3Dto::new((x0 + x1) * 0.5, (y0 + y1) * 0.5, z0);
            let resolved = match shape {
                CamStockShape::Box => Shape::Box,
                CamStockShape::Cylinder => Shape::Cylinder(center, size.x * 0.5),
                CamStockShape::Hex => Shape::Hex(center, size.x),
                _ => unreachable!(),
            };
            Ok((b, resolved))
        }
        CamStockSpecDto::FromModel { shape, offsets: o } => {
            if [o.x_min, o.x_max, o.y_min, o.y_max, o.z_min, o.z_max]
                .into_iter()
                .any(|v| !v.is_finite() || v < 0.)
            {
                return Err("Stock allowances must be finite and nonnegative".into());
            }
            let swap = round_orientation(*shape)?;
            let z = model.min.z - o.z_min;
            let height = model.max.z + o.z_max - z;
            if *shape == CamStockShape::Box {
                return Ok((
                    StockBoxDto {
                        min: Point3Dto::new(model.min.x - o.x_min, model.min.y - o.y_min, z),
                        max: Point3Dto::new(
                            model.max.x + o.x_max,
                            model.max.y + o.y_max,
                            z + height,
                        ),
                    },
                    Shape::Box,
                ));
            }
            let radial = o.x_min.max(o.x_max).max(o.y_min).max(o.y_max);
            let hx = (model.max.x - model.min.x) * 0.5;
            let hy = (model.max.y - model.min.y) * 0.5;
            let center = Point3Dto::new(cx, cy, z);
            if *shape == CamStockShape::Cylinder {
                let radius = hx.hypot(hy) + radial;
                return Ok((
                    centered_box(cx, cy, z, radius * 2., radius * 2., height),
                    Shape::Cylinder(center, radius),
                ));
            }
            if *shape != CamStockShape::Hex {
                return Err("Choose the Modeled stock body definition".into());
            }
            let (hx, hy) = if swap { (hy, hx) } else { (hx, hy) };
            let af = hx.max(hx * 0.5 + hy * sq3 * 0.5) * 2. + radial * 2.;
            let (sx, sy) = if swap {
                (af * 2. / sq3, af)
            } else {
                (af, af * 2. / sq3)
            };
            Ok((
                centered_box(cx, cy, z, sx, sy, height),
                Shape::Hex(center, af),
            ))
        }
    }
}

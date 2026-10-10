//! One axisymmetric cutter envelope for material removal and display.
//! Z=0 is the tool's lowest tip, not the start of its cylindrical diameter.
use crate::{CamToolDto, CamToolKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CamCornerChamferDto {
    /// Radial width removed from the end-mill corner, in mm.
    pub width: f64,
    /// Flank angle measured from the tool axis (45 degrees is equal-leg).
    pub angle_degrees: f64,
}

/// Small geometry-only payload; neither cutting data nor poses affect meshes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CamCutterGeometryDto {
    pub kind: CamToolKind,
    pub diameter: f64,
    pub flute_length: f64,
    pub overall_length: f64,
    #[serde(default)]
    pub point_angle_degrees: Option<f64>,
    #[serde(default)]
    pub corner_radius: Option<f64>,
    #[serde(default)]
    pub corner_chamfer: Option<CamCornerChamferDto>,
}

impl From<&CamToolDto> for CamCutterGeometryDto {
    fn from(tool: &CamToolDto) -> Self {
        Self {
            kind: tool.kind,
            diameter: tool.diameter,
            flute_length: tool.flute_length,
            overall_length: tool.overall_length,
            point_angle_degrees: tool.point_angle_degrees,
            corner_radius: tool.corner_radius,
            corner_chamfer: tool.corner_chamfer,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tip {
    Flat,
    Round {
        corner: f64,
    },
    Cone {
        tangent: f64,
        height: f64,
    },
    Bevel {
        width: f64,
        tangent: f64,
        height: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutterProfile {
    radius: f64,
    flute: f64,
    overall: f64,
    tip: Tip,
}

/// Boundary identity lets the stock mesher retain a crease without blending
/// a flat land's normal into a bevel. It is not a new cutting representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum CutterSurface {
    Bottom,
    Corner,
    Wall,
    Top,
}

impl CutterProfile {
    pub fn new(g: CamCutterGeometryDto) -> Result<Self, String> {
        if [g.diameter, g.flute_length, g.overall_length]
            .iter()
            .any(|n| !n.is_finite() || *n <= 0.)
            || g.flute_length > g.overall_length
        {
            return Err(
                "Cutter dimensions must be positive, with flute length within overall length"
                    .into(),
            );
        }
        let radius = g.diameter * 0.5;
        let corner_capable = matches!(
            g.kind,
            CamToolKind::FlatEndMill | CamToolKind::BullNoseEndMill | CamToolKind::FaceMill
        );
        if let Some(r) = g.corner_radius {
            if !corner_capable || !r.is_finite() || r <= 0. || r > radius {
                return Err("Corner radius must be positive, no larger than the cutter radius, and belong to an end/face mill".into());
            }
        }
        if let Some(angle) = g.point_angle_degrees {
            if !angle.is_finite() || !(10.0..=170.0).contains(&angle) {
                return Err("Point angle must be between 10 and 170 degrees".into());
            }
        }
        let tip = if let Some(c) = g.corner_chamfer {
            if !matches!(g.kind, CamToolKind::FlatEndMill | CamToolKind::FaceMill)
                || g.corner_radius.is_some()
            {
                return Err(
                    "Corner chamfer requires an end/face mill without a corner radius".into(),
                );
            }
            if !c.width.is_finite()
                || c.width <= 0.
                || c.width >= radius
                || !c.angle_degrees.is_finite()
                || !(5.0..=85.0).contains(&c.angle_degrees)
            {
                return Err("Corner chamfer width must be below the tool radius and its angle between 5 and 85 degrees".into());
            }
            let tangent = c.angle_degrees.to_radians().tan();
            Tip::Bevel {
                width: c.width,
                tangent,
                height: c.width / tangent,
            }
        } else {
            match g.kind {
                CamToolKind::Drill | CamToolKind::ChamferMill => {
                    let angle = match (g.kind, g.point_angle_degrees) {
                        (_, Some(angle)) => angle,
                        (CamToolKind::Drill, None) => 118.,
                        _ => return Err("Chamfer mill must declare a point angle".into()),
                    };
                    let tangent = (angle.to_radians() * 0.5).tan();
                    Tip::Cone {
                        tangent,
                        height: radius / tangent,
                    }
                }
                CamToolKind::BallEndMill => Tip::Round { corner: radius },
                CamToolKind::BullNoseEndMill => Tip::Round {
                    corner: g
                        .corner_radius
                        .ok_or("Bull-nose end mill must declare a corner radius")?,
                },
                CamToolKind::FlatEndMill | CamToolKind::FaceMill => g
                    .corner_radius
                    .map_or(Tip::Flat, |corner| Tip::Round { corner }),
                _ => Tip::Flat,
            }
        };
        let tip_height = match tip {
            Tip::Flat => 0.,
            Tip::Round { corner } => corner,
            Tip::Cone { height, .. } | Tip::Bevel { height, .. } => height,
        };
        if tip_height > g.flute_length + 1e-9 && g.kind != CamToolKind::FaceMill {
            return Err("Tool flute length must contain its tip or corner profile; check Flute length and Corner radius/angle".into());
        }
        if tip_height > g.overall_length + 1e-9 {
            return Err("Tool overall length must contain its tip or corner profile".into());
        }
        Ok(Self {
            radius,
            flute: if g.kind == CamToolKind::FaceMill {
                g.flute_length.max(tip_height)
            } else {
                g.flute_length
            },
            overall: g.overall_length,
            tip,
        })
    }

    /// Height above the tool tip at which the full nominal radius is reached.
    pub fn full_radius_height(&self) -> f64 {
        match self.tip {
            Tip::Flat => 0.0,
            Tip::Round { corner } => corner,
            Tip::Cone { height, .. } | Tip::Bevel { height, .. } => height,
        }
    }

    /// Analytic radius, not the polygonized display envelope. None is outside
    /// the cutting length; the shank never removes material.
    pub fn radius_at_height(&self, z: f64) -> Option<f64> {
        if !z.is_finite() || z < -1e-9 || z > self.flute + 1e-9 {
            return None;
        }
        let z = z.max(0.);
        Some(match self.tip {
            Tip::Flat => self.radius,
            Tip::Round { corner } if z < corner => {
                self.radius - corner + (corner * corner - (corner - z).powi(2)).max(0.).sqrt()
            }
            Tip::Cone { tangent, .. } => (z * tangent).min(self.radius),
            Tip::Bevel { width, tangent, .. } => {
                (self.radius - width + z * tangent).min(self.radius)
            }
            _ => self.radius,
        })
    }

    pub fn contains(&self, radial_squared: f64, z: f64) -> bool {
        self.radius_at_height(z)
            .is_some_and(|r| radial_squared <= r * r + 1e-9)
    }

    /// Signed meridian field and outward (radial, Z) normal. Negative is
    /// inside the cutter. A vertical sweep extends only the flute's upper
    /// cap: the lowest tip still determines the floor/corner that remains.
    pub(crate) fn surface_at(
        &self,
        radial: f64,
        z: f64,
        axial_extension: f64,
    ) -> (f64, [f64; 2], CutterSurface) {
        use CutterSurface::*;
        let mut best = (f64::NEG_INFINITY, [0.; 2], Bottom);
        for part in [Bottom, Corner, Wall, Top] {
            if part == Corner && matches!(self.tip, Tip::Flat) {
                continue;
            }
            let (d, normal) = self.surface_component(part, radial, z, axial_extension);
            if d > best.0 {
                best = (d, normal, part);
            }
        }
        best
    }

    /// Inverse radial bound of `surface_at(r,z,extension) <= level`.
    /// Used to prune same-height swept paths before their nearest-point search;
    /// it includes the finite flute and is valid for negative (inside) levels.
    pub(crate) fn radial_extent_at_field(&self, z: f64, extension: f64, level: f64) -> Option<f64> {
        if -z > level || z - self.flute - extension > level {
            return None;
        }
        let mut radius = self.radius + level;
        match self.tip {
            Tip::Cone { tangent, .. } | Tip::Bevel { tangent, .. } => {
                let land = match self.tip {
                    Tip::Bevel { width, .. } => self.radius - width,
                    _ => 0.,
                };
                radius = radius.min(land + z * tangent + level * (1. + tangent * tangent).sqrt());
            }
            Tip::Round { corner } if z < corner => {
                let rounded = corner + level;
                let vertical = corner - z;
                if rounded < vertical {
                    return None;
                }
                radius = radius.min(
                    self.radius - corner + (rounded * rounded - vertical * vertical).max(0.).sqrt(),
                );
            }
            _ => {}
        }
        (radius >= 0.).then_some(radius)
    }

    pub(crate) fn surface_component(
        &self,
        part: CutterSurface,
        radial: f64,
        z: f64,
        axial_extension: f64,
    ) -> (f64, [f64; 2]) {
        match part {
            CutterSurface::Bottom => (-z, [0., -1.]),
            CutterSurface::Wall => (radial - self.radius, [1., 0.]),
            CutterSurface::Top => (z - self.flute - axial_extension, [0., 1.]),
            CutterSurface::Corner => match self.tip {
                Tip::Cone { tangent, .. } | Tip::Bevel { tangent, .. } => {
                    let land = match self.tip {
                        Tip::Bevel { width, .. } => self.radius - width,
                        _ => 0.,
                    };
                    let nr = 1. / (1. + tangent * tangent).sqrt();
                    ((radial - land - z * tangent) * nr, [nr, -tangent * nr])
                }
                Tip::Round { corner } => {
                    let q = [radial - (self.radius - corner), corner - z];
                    let positive = q.map(|v| v.max(0.));
                    let length = positive[0].hypot(positive[1]);
                    let d = length + q[0].max(q[1]).min(0.) - corner;
                    let n = if length > 1e-12 {
                        [positive[0] / length, -positive[1] / length]
                    } else if q[0] > q[1] {
                        [1., 0.]
                    } else {
                        [0., -1.]
                    };
                    (d, n)
                }
                Tip::Flat => (f64::NEG_INFINITY, [0., 0.]),
            },
        }
    }

    fn profile_band(
        &self,
        cutter: &mut CamCutterMeshPartDto,
        shank: &mut CamCutterMeshPartDto,
        low: Ring,
        high: Ring,
    ) {
        if high.z <= self.flute {
            band(cutter, low, high);
        } else if low.z >= self.flute {
            band(shank, low, high);
        } else {
            let r = self.radius_at_height(self.flute).unwrap();
            let (_, n) = self.surface_component(CutterSurface::Corner, r, self.flute, 0.);
            let split = Ring {
                r,
                z: self.flute,
                nr: n[0],
                nz: n[1],
            };
            band(cutter, low, split);
            band(shank, split, high);
        }
    }

    pub fn mesh(&self) -> CamCutterMeshDto {
        let mut cutter = CamCutterMeshPartDto::default();
        let mut shank = CamCutterMeshPartDto::default();
        let mut top = 0.;
        let bottom = self.radius_at_height(0.).unwrap();
        cap(&mut cutter, 0., bottom, -1.);
        match self.tip {
            Tip::Flat => {}
            Tip::Round { corner } => {
                let ring = |i: usize| {
                    let a = i as f64 / 24. * std::f64::consts::FRAC_PI_2;
                    Ring {
                        r: self.radius - corner + corner * a.sin(),
                        z: corner * (1. - a.cos()),
                        nr: a.sin(),
                        nz: -a.cos(),
                    }
                };
                for i in 0..24 {
                    self.profile_band(&mut cutter, &mut shank, ring(i), ring(i + 1));
                }
                top = corner;
            }
            Tip::Cone { tangent, height }
            | Tip::Bevel {
                tangent, height, ..
            } => {
                let nr = 1. / (1. + tangent * tangent).sqrt();
                self.profile_band(
                    &mut cutter,
                    &mut shank,
                    Ring {
                        r: bottom,
                        z: 0.,
                        nr,
                        nz: -tangent * nr,
                    },
                    Ring {
                        r: self.radius,
                        z: height,
                        nr,
                        nz: -tangent * nr,
                    },
                );
                top = height;
            }
        }
        if self.flute > top + 1e-9 {
            band(
                &mut cutter,
                Ring::wall(self.radius, top),
                Ring::wall(self.radius, self.flute),
            );
        }
        let body_start = self.flute.max(top);
        if self.overall > body_start + 1e-9 {
            band(
                &mut shank,
                Ring::wall(self.radius, body_start),
                Ring::wall(self.radius, self.overall),
            );
        }
        if self.overall > self.flute + 1e-9 {
            cap(&mut shank, self.overall, self.radius, 1.);
        } else {
            cap(&mut cutter, self.flute, self.radius, 1.);
        }
        CamCutterMeshDto { cutter, shank }
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct CamCutterMeshPartDto {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
}
#[derive(Debug, Clone, Serialize)]
pub struct CamCutterMeshDto {
    pub cutter: CamCutterMeshPartDto,
    pub shank: CamCutterMeshPartDto,
}

pub fn cutter_mesh(geometry: CamCutterGeometryDto) -> Result<CamCutterMeshDto, String> {
    Ok(CutterProfile::new(geometry)?.mesh())
}

const SIDES: usize = 64;
#[derive(Clone, Copy)]
struct Ring {
    r: f64,
    z: f64,
    nr: f64,
    nz: f64,
}
impl Ring {
    fn wall(r: f64, z: f64) -> Self {
        Self {
            r,
            z,
            nr: 1.,
            nz: 0.,
        }
    }
    fn vertex(self, angle: f64) -> ([f32; 3], [f32; 3]) {
        let (s, c) = angle.sin_cos();
        (
            [(self.r * c) as f32, (self.r * s) as f32, self.z as f32],
            [(self.nr * c) as f32, (self.nr * s) as f32, self.nz as f32],
        )
    }
}
fn triangle(mesh: &mut CamCutterMeshPartDto, vertices: [([f32; 3], [f32; 3]); 3]) {
    for (p, n) in vertices {
        mesh.positions.extend(p);
        mesh.normals.extend(n);
    }
}
fn band(mesh: &mut CamCutterMeshPartDto, low: Ring, high: Ring) {
    for i in 0..SIDES {
        let a = i as f64 / SIDES as f64 * std::f64::consts::TAU;
        let b = (i + 1) as f64 / SIDES as f64 * std::f64::consts::TAU;
        if low.r > 1e-12 {
            triangle(mesh, [low.vertex(a), low.vertex(b), high.vertex(a)]);
        }
        triangle(mesh, [low.vertex(b), high.vertex(b), high.vertex(a)]);
    }
}
fn cap(mesh: &mut CamCutterMeshPartDto, z: f64, r: f64, direction: f32) {
    if r <= 1e-12 {
        return;
    }
    let ring = Ring {
        r,
        z,
        nr: 0.,
        nz: direction as f64,
    };
    for i in 0..SIDES {
        let a = i as f64 / SIDES as f64 * std::f64::consts::TAU;
        let b = (i + 1) as f64 / SIDES as f64 * std::f64::consts::TAU;
        let mut v = [
            ([0., 0., z as f32], [0., 0., direction]),
            ring.vertex(a),
            ring.vertex(b),
        ];
        if direction < 0. {
            v.swap(1, 2);
        }
        triangle(mesh, v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shallow_face_insert_cuts_with_its_whole_programming_corner() {
        for radius in [1.2, 1.5] {
            let mut g = geometry(CamToolKind::FaceMill);
            g.diameter = 16.;
            g.flute_length = 1.;
            g.corner_radius = Some(radius);
            let profile = CutterProfile::new(g).unwrap();
            assert_eq!(profile.radius_at_height(0.), Some(8. - radius));
            assert!(profile.radius_at_height(1.).unwrap() > 7.8);
            assert_eq!(profile.radius_at_height(radius), Some(8.));
            assert!(profile.contains(7.99 * 7.99, radius));
            assert_eq!(profile.radius_at_height(radius + 0.001), None);
            assert!(!profile.contains(0., radius + 0.001));
            let mesh = profile.mesh();
            assert!(mesh
                .cutter
                .positions
                .as_chunks::<3>()
                .0
                .iter()
                .all(|p| f64::from(p[2]) <= radius + 1e-6));
            assert!(mesh
                .shank
                .positions
                .as_chunks::<3>()
                .0
                .iter()
                .all(|p| f64::from(p[2]) >= radius - 1e-6));
            assert_eq!(g.flute_length, 1., "tool data is not rewritten");
            g.kind = CamToolKind::BullNoseEndMill;
            assert!(
                CutterProfile::new(g).is_err(),
                "end mill still needs its entire corner within the flute"
            );
        }
    }

    #[test]
    fn radial_field_bound_is_the_inverse_for_all_cutter_tips() {
        let mut cases = vec![
            geometry(CamToolKind::FlatEndMill),
            geometry(CamToolKind::Drill),
            geometry(CamToolKind::BallEndMill),
        ];
        let mut g = geometry(CamToolKind::ChamferMill);
        g.point_angle_degrees = Some(90.);
        cases.push(g);
        g = geometry(CamToolKind::BullNoseEndMill);
        g.corner_radius = Some(1.);
        cases.push(g);
        g = geometry(CamToolKind::FlatEndMill);
        g.corner_chamfer = Some(CamCornerChamferDto {
            width: 1.,
            angle_degrees: 30.,
        });
        cases.push(g);
        for g in cases {
            let profile = CutterProfile::new(g).unwrap();
            for extension in [0., 3.] {
                for z in [
                    -2., -0.1, 0., 0.2, 0.9, 1., 1.3, 2., 4., 9., 19.9, 20., 21., 23.2,
                ] {
                    for level in [-3., -1.1, -0.7, -0.05, 0., 0.1, 0.6, 2.] {
                        let limit = profile.radial_extent_at_field(z, extension, level);
                        for i in 0..160 {
                            let r = i as f64 * 0.05;
                            let field = profile.surface_at(r, z, extension).0;
                            if (field - level).abs() < 1e-9 {
                                continue;
                            }
                            assert_eq!(field < level, limit.is_some_and(|limit| r <= limit), "{:?} r={r} z={z} level={level} extension={extension} bound={limit:?}", g.kind);
                        }
                    }
                }
            }
        }
    }

    fn geometry(kind: CamToolKind) -> CamCutterGeometryDto {
        CamCutterGeometryDto {
            kind,
            diameter: 10.,
            flute_length: 20.,
            overall_length: 50.,
            point_angle_degrees: None,
            corner_radius: None,
            corner_chamfer: None,
        }
    }
    fn near(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-8, "{a} != {b}");
    }

    #[test]
    fn display_fields_match_the_cutting_envelope_for_all_tip_shapes() {
        let mut shapes = Vec::new();
        for kind in [
            CamToolKind::FlatEndMill,
            CamToolKind::BallEndMill,
            CamToolKind::Drill,
            CamToolKind::ChamferMill,
        ] {
            let mut g = geometry(kind);
            g.point_angle_degrees =
                matches!(kind, CamToolKind::Drill | CamToolKind::ChamferMill).then_some(118.);
            shapes.push(g);
        }
        let mut round = geometry(CamToolKind::BullNoseEndMill);
        round.corner_radius = Some(1.3);
        shapes.push(round);
        let mut bevel = geometry(CamToolKind::FlatEndMill);
        bevel.corner_chamfer = Some(CamCornerChamferDto {
            width: 1.1,
            angle_degrees: 35.,
        });
        shapes.push(bevel);
        for g in shapes {
            let profile = CutterProfile::new(g).unwrap();
            for zi in -5..110 {
                for ri in 0..30 {
                    let z = zi as f64 * 0.2 + 0.031;
                    let r = ri as f64 * 0.2 + 0.027;
                    let (d, n, _) = profile.surface_at(r, z, 0.);
                    assert_eq!(d < 0., profile.contains(r * r, z), "{g:?}, r={r}, z={z}");
                    assert!((n[0].hypot(n[1]) - 1.).abs() < 1e-9);
                }
            }
        }
    }

    #[test]
    fn drill_angles_and_chamfer_cones_use_the_lowest_tip_as_zero() {
        for angle in [90.0_f64, 118., 135., 150.] {
            let mut g = geometry(CamToolKind::Drill);
            g.point_angle_degrees = Some(angle);
            let p = CutterProfile::new(g).unwrap();
            let h = 5. / (angle.to_radians() / 2.).tan();
            near(p.radius_at_height(0.).unwrap(), 0.);
            near(p.radius_at_height(h / 2.).unwrap(), 2.5);
            near(p.radius_at_height(h).unwrap(), 5.);
            assert!(!p.contains(1., 0.));
            assert!(p.contains(25., h));
            g.kind = CamToolKind::ChamferMill;
            near(
                CutterProfile::new(g)
                    .unwrap()
                    .radius_at_height(h / 2.)
                    .unwrap(),
                2.5,
            );
        }
        let legacy = CutterProfile::new(geometry(CamToolKind::Drill)).unwrap();
        near(
            legacy.radius_at_height(1.).unwrap(),
            59.0_f64.to_radians().tan(),
        );
    }

    #[test]
    fn ball_and_corner_radii_apply_to_flat_and_face_mills_too() {
        let ball = CutterProfile::new(geometry(CamToolKind::BallEndMill)).unwrap();
        near(ball.radius_at_height(0.).unwrap(), 0.);
        near(
            ball.radius_at_height(2.5).unwrap(),
            (25.0_f64 - 6.25).sqrt(),
        );
        for kind in [
            CamToolKind::BullNoseEndMill,
            CamToolKind::FlatEndMill,
            CamToolKind::FaceMill,
        ] {
            let mut g = geometry(kind);
            g.corner_radius = Some(1.);
            let p = CutterProfile::new(g).unwrap();
            near(p.radius_at_height(0.).unwrap(), 4.);
            near(p.radius_at_height(0.5).unwrap(), 4. + 0.75_f64.sqrt());
            assert!(!p.contains(4.9 * 4.9, 0.5));
            near(p.radius_at_height(1.).unwrap(), 5.);
        }
    }

    #[test]
    fn chamfered_end_mill_has_a_flat_land_and_an_explicit_flank() {
        let mut g = geometry(CamToolKind::FlatEndMill);
        for angle in [30., 45., 60.] {
            g.corner_chamfer = Some(CamCornerChamferDto {
                width: 1.,
                angle_degrees: angle,
            });
            let p = CutterProfile::new(g).unwrap();
            let h = 1. / angle.to_radians().tan();
            near(p.radius_at_height(0.).unwrap(), 4.);
            near(p.radius_at_height(h / 2.).unwrap(), 4.5);
            near(p.radius_at_height(h).unwrap(), 5.);
            assert!(!p.contains(25., h / 2.));
            assert!(!p.contains(1., 20.01), "shank never removes stock");
        }
    }

    #[test]
    fn invalid_or_conflicting_edge_geometry_is_not_rendered_as_a_cylinder() {
        let mut g = geometry(CamToolKind::FlatEndMill);
        g.corner_chamfer = Some(CamCornerChamferDto {
            width: 1.,
            angle_degrees: 45.,
        });
        g.corner_radius = Some(1.);
        assert!(cutter_mesh(g).is_err());
        g.corner_radius = None;
        g.corner_chamfer.as_mut().unwrap().width = 5.;
        assert!(cutter_mesh(g).is_err());
        g = geometry(CamToolKind::Drill);
        g.point_angle_degrees = Some(f64::NAN);
        assert!(cutter_mesh(g).is_err());
        g.point_angle_degrees = Some(10.);
        g.flute_length = 1.;
        assert!(cutter_mesh(g).is_err());
        g = geometry(CamToolKind::BullNoseEndMill);
        assert!(cutter_mesh(g).is_err());
    }

    #[test]
    fn display_vertices_and_normals_match_the_cutting_envelope_with_bounded_detail() {
        let mut cases = vec![
            geometry(CamToolKind::FlatEndMill),
            geometry(CamToolKind::Drill),
            geometry(CamToolKind::BallEndMill),
        ];
        let mut g = geometry(CamToolKind::ChamferMill);
        g.point_angle_degrees = Some(90.);
        cases.push(g);
        g = geometry(CamToolKind::BullNoseEndMill);
        g.corner_radius = Some(1.);
        cases.push(g);
        g = geometry(CamToolKind::FlatEndMill);
        g.corner_chamfer = Some(CamCornerChamferDto {
            width: 1.,
            angle_degrees: 45.,
        });
        cases.push(g);
        for g in cases {
            let profile = CutterProfile::new(g).unwrap();
            let mesh = profile.mesh();
            assert!((mesh.cutter.positions.len() + mesh.shank.positions.len()) / 9 < 4000);
            for part in [&mesh.cutter, &mesh.shank] {
                assert_eq!(part.positions.len(), part.normals.len());
                for n in part.normals.as_chunks::<3>().0 {
                    assert!((n.iter().map(|n| n * n).sum::<f32>() - 1.).abs() < 1e-5);
                }

                for (p, n) in part
                    .positions
                    .as_chunks::<9>()
                    .0
                    .iter()
                    .zip(part.normals.as_chunks::<9>().0.iter())
                {
                    let u = [p[3] - p[0], p[4] - p[1], p[5] - p[2]];
                    let v = [p[6] - p[0], p[7] - p[1], p[8] - p[2]];
                    let cross = [
                        u[1] * v[2] - u[2] * v[1],
                        u[2] * v[0] - u[0] * v[2],
                        u[0] * v[1] - u[1] * v[0],
                    ];
                    assert!(
                        (0..3)
                            .map(|a| cross[a] * (n[a] + n[a + 3] + n[a + 6]))
                            .sum::<f32>()
                            > 0.
                    );
                }
            }
            for p in mesh.cutter.positions.as_chunks::<3>().0 {
                let r = (p[0] as f64).hypot(p[1] as f64);
                let z = p[2] as f64;

                assert!(r <= profile.radius_at_height(z.min(g.flute_length)).unwrap() + 2e-5);
            }
            near(
                mesh.cutter
                    .positions
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|p| p[2] as f64)
                    .fold(f64::INFINITY, f64::min),
                0.,
            );
        }
    }
}

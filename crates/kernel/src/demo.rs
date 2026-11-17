//! Sample parts, built with nothing but this crate.
//!
//! They are the kernel's showcase and its broadest tests: the test suite
//! checks them against closed-form volumes, the viewport proof draws them
//! and the browser demo remodels them on every slider move.

use crate::geom::CurveKind;
use crate::make::{self, Region, Seg};
use crate::math::*;
use crate::topo::Body;
use crate::{blend, boolean, Op};

fn edges(body: &Body, pick: impl Fn(u32) -> bool) -> Vec<u32> {
    (0..body.edge_count() as u32).filter(|&e| pick(e)).collect()
}

/// Full circles of radius `r` in the plane `z`.
fn rings(body: &Body, z: f64, r: f64) -> Vec<u32> {
    edges(body, |e| {
        let c = body.e_curve_ref(e);
        c.kind == CurveKind::Circle
            && body.e_closed(e)
            && (c.f.o.z - z).abs() < TOL
            && (c.r - r).abs() < TOL
    })
}

/// A plate with rounded corners and rim, a filleted and chamfered boss, a
/// centre bore and four counterbored holes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MountingBlock {
    pub length: f64,
    pub width: f64,
    pub thickness: f64,
    /// Radius of the four upright corners.
    pub corner: f64,
    /// Radius of the round that runs around the top rim.
    pub rim: f64,
    pub boss_radius: f64,
    pub boss_height: f64,
    /// Radius of the round at the foot of the boss.
    pub boss_fillet: f64,
    /// Bevel on the top of the boss.
    pub boss_chamfer: f64,
    pub bore_radius: f64,
    pub hole_radius: f64,
    pub counterbore_radius: f64,
    pub counterbore_depth: f64,
    /// Distance of the hole centres from the sides.
    pub hole_inset: f64,
}

impl Default for MountingBlock {
    fn default() -> Self {
        MountingBlock {
            length: 80.0,
            width: 50.0,
            thickness: 14.0,
            corner: 6.0,
            rim: 2.5,
            boss_radius: 14.0,
            boss_height: 12.0,
            boss_fillet: 3.0,
            boss_chamfer: 1.5,
            bore_radius: 7.0,
            hole_radius: 2.75,
            counterbore_radius: 5.0,
            counterbore_depth: 4.0,
            hole_inset: 12.0,
        }
    }
}

impl MountingBlock {
    pub fn build(&self) -> Result<Body, String> {
        let p = *self;
        let top = p.thickness;
        let centre = v3(0.5 * p.length, 0.5 * p.width, 0.0);
        let mut body = make::block(V3::ZERO, v3(p.length, p.width, top))?;
        if p.corner > TOL {
            let uprights = edges(&body, |e| {
                let c = body.e_curve_ref(e);
                c.kind == CurveKind::Line && c.f.x.z.abs() > 0.9
            });
            body = blend::fillet(&body, &uprights, p.corner)?;
        }
        if p.rim > TOL {
            // Straight runs become cylinders, the rounded corners torus
            // segments: one tangent chain around the whole rim.
            let rim = edges(&body, |e| {
                (body.e_mid(e).z - top).abs() < TOL && body.e_sharp(e)
            });
            body = blend::fillet(&body, &rim, p.rim)?;
        }
        if p.boss_height > TOL && p.boss_radius > TOL {
            let boss = make::cylinder(centre + V3::Z * top, V3::Z, p.boss_radius, p.boss_height)?;
            body = boolean(&body, &boss, Op::Union)?;
            if p.boss_fillet > TOL {
                let foot = rings(&body, top, p.boss_radius);
                body = blend::fillet(&body, &foot, p.boss_fillet)?;
            }
            if p.boss_chamfer > TOL {
                let crown = rings(&body, top + p.boss_height, p.boss_radius);
                body = blend::chamfer(&body, &crown, p.boss_chamfer, None)?;
            }
        }
        if p.bore_radius > TOL {
            let bore = make::cylinder(
                centre - V3::Z,
                V3::Z,
                p.bore_radius,
                top + p.boss_height.max(0.0) + 2.0,
            )?;
            body = boolean(&body, &bore, Op::Cut)?;
        }
        if p.hole_radius > TOL {
            let through = top + 6.0;
            let profile: Vec<(f64, f64)> =
                if p.counterbore_radius > p.hole_radius + TOL && p.counterbore_depth > TOL {
                    vec![
                        (0.0, -0.001),
                        (p.counterbore_radius, -0.001),
                        (p.counterbore_radius, p.counterbore_depth),
                        (p.hole_radius, p.counterbore_depth),
                        (p.hole_radius, through),
                        (0.0, through),
                    ]
                } else {
                    vec![
                        (0.0, -0.001),
                        (p.hole_radius, -0.001),
                        (p.hole_radius, through),
                        (0.0, through),
                    ]
                };
            for (x, y) in [
                (p.hole_inset, p.hole_inset),
                (p.length - p.hole_inset, p.hole_inset),
                (p.hole_inset, p.width - p.hole_inset),
                (p.length - p.hole_inset, p.width - p.hole_inset),
            ] {
                let cutter = make::lathe(v3(x, y, top), -V3::Z, &profile)?;
                body = boolean(&body, &cutter, Op::Cut)?;
            }
        }
        Ok(body)
    }
}

/// A box with every edge rounded: cylinders along the edges, spheres in the
/// corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedBox {
    pub size: V3,
    pub radius: f64,
}

impl Default for RoundedBox {
    fn default() -> Self {
        RoundedBox {
            size: v3(50.0, 34.0, 22.0),
            radius: 6.0,
        }
    }
}

impl RoundedBox {
    pub fn build(&self) -> Result<Body, String> {
        let brick = make::block(V3::ZERO, self.size)?;
        if self.radius <= TOL {
            return Ok(brick);
        }
        let all: Vec<u32> = (0..brick.edge_count() as u32).collect();
        blend::fillet(&brick, &all, self.radius)
    }
}

/// A shaft with an axial bore and a cross hole. The cross hole meets both
/// cylinder walls in exact intersection curves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrilledShaft {
    pub radius: f64,
    pub length: f64,
    pub cross_radius: f64,
    /// Height of the cross hole above the base.
    pub cross_height: f64,
    pub bore_radius: f64,
    /// Bevel on the outer top rim.
    pub chamfer: f64,
}

impl Default for DrilledShaft {
    fn default() -> Self {
        DrilledShaft {
            radius: 12.0,
            length: 60.0,
            cross_radius: 5.0,
            cross_height: 38.0,
            bore_radius: 4.0,
            chamfer: 1.5,
        }
    }
}

impl DrilledShaft {
    pub fn build(&self) -> Result<Body, String> {
        let p = *self;
        let mut body = make::cylinder(V3::ZERO, V3::Z, p.radius, p.length)?;
        if p.cross_radius > TOL {
            let reach = p.radius + 8.0;
            let cross = make::cylinder(
                v3(-reach, 0.0, p.cross_height),
                V3::X,
                p.cross_radius,
                2.0 * reach,
            )?;
            body = boolean(&body, &cross, Op::Cut)?;
        }
        if p.bore_radius > TOL {
            let bore = make::cylinder(v3(0.0, 0.0, -1.0), V3::Z, p.bore_radius, p.length + 2.0)?;
            body = boolean(&body, &bore, Op::Cut)?;
        }
        if p.chamfer > TOL {
            let crown = rings(&body, p.length, p.radius);
            body = blend::chamfer(&body, &crown, p.chamfer, None)?;
        }
        Ok(body)
    }
}

/// A folded bracket: an L section extruded, the bend and the plate corners
/// rounded, every outline edge of both sides broken, three holes. The edge
/// break is one fillet over 22 edges; its chains run from straight edges
/// through torus segments around the rounded corners and back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bracket {
    /// Length of the base leg (along x).
    pub base: f64,
    /// Height of the upright leg (along z).
    pub height: f64,
    pub wall: f64,
    /// Width of the bracket (along y).
    pub width: f64,
    pub bend: f64,
    pub corner: f64,
    pub edge_break: f64,
    pub base_hole_radius: f64,
    pub upright_hole_radius: f64,
}

impl Default for Bracket {
    fn default() -> Self {
        Bracket {
            base: 32.0,
            height: 44.0,
            wall: 8.0,
            width: 40.0,
            bend: 4.0,
            corner: 3.0,
            edge_break: 0.6,
            base_hole_radius: 3.5,
            upright_hole_radius: 13.0,
        }
    }
}

impl Bracket {
    pub fn build(&self) -> Result<Body, String> {
        let p = *self;
        let section = [
            (0.0, 0.0),
            (p.base, 0.0),
            (p.base, p.wall),
            (p.wall, p.wall),
            (p.wall, p.height),
            (0.0, p.height),
        ];
        let outline: Vec<Seg> = (0..section.len())
            .map(|i| {
                let (a, b) = (section[i], section[(i + 1) % section.len()]);
                Seg::Line {
                    a: v3(a.0, 0.0, a.1),
                    b: v3(b.0, 0.0, b.1),
                }
            })
            .collect();
        let mut body = make::extrude(
            &Region {
                loops: vec![outline],
            },
            V3::Y,
            0.0,
            p.width,
        )?;
        let along = |body: &Body, e: u32, axis: V3| {
            let c = body.e_curve_ref(e);
            c.kind == CurveKind::Line && c.f.x.dot(axis).abs() > 0.9
        };
        if p.bend > TOL {
            let inside = edges(&body, |e| {
                let m = body.e_mid(e);
                along(&body, e, V3::Y) && (m.x - p.wall).abs() < TOL && (m.z - p.wall).abs() < TOL
            });
            body = blend::fillet(&body, &inside, p.bend)?;
        }
        if p.corner > TOL {
            // The plate corners, seen face-on: the ends of both legs.
            let on_side = |y: f64| y.abs() < TOL || (y - p.width).abs() < TOL;
            let corners = edges(&body, |e| {
                let m = body.e_mid(e);
                on_side(m.y)
                    && ((along(&body, e, V3::X) && (m.z - p.height).abs() < TOL)
                        || (along(&body, e, V3::Z) && (m.x - p.base).abs() < TOL))
            });
            body = blend::fillet(&body, &corners, p.corner)?;
        }
        if p.edge_break > TOL {
            let sides = edges(&body, |e| {
                let [v0, v1] = body.e_v[e as usize];
                let flat = |y: f64| {
                    [
                        body.verts[v0 as usize].y,
                        body.verts[v1 as usize].y,
                        body.e_mid(e).y,
                    ]
                    .iter()
                    .all(|q| (q - y).abs() < TOL)
                };
                body.e_sharp(e) && (flat(0.0) || flat(p.width))
            });
            body = blend::fillet(&body, &sides, p.edge_break)?;
        }
        if p.base_hole_radius > TOL {
            for y in [p.wall, p.width - p.wall] {
                let hole = make::cylinder(
                    v3(p.base - p.wall, y, -1.0),
                    V3::Z,
                    p.base_hole_radius,
                    p.wall + 2.0,
                )?;
                body = boolean(&body, &hole, Op::Cut)?;
            }
        }
        if p.upright_hole_radius > TOL {
            let hole = make::cylinder(
                v3(-1.0, 0.5 * p.width, 0.5 * (p.height + p.wall) + 1.0),
                V3::X,
                p.upright_hole_radius,
                p.wall + 2.0,
            )?;
            body = boolean(&body, &hole, Op::Cut)?;
        }
        Ok(body)
    }
}

/// One adjustable dimension of a sample part, in millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dimension {
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    /// The value the part is shown with first.
    pub value: f64,
}

const fn dim(label: &'static str, min: f64, max: f64, value: f64) -> Dimension {
    Dimension {
        label,
        min,
        max,
        value,
    }
}

/// A sample part: what it shows, which dimensions drive it, how it is built.
pub struct Part {
    pub name: &'static str,
    pub title: &'static str,
    pub about: &'static str,
    pub dimensions: &'static [Dimension],
    /// Model the part from one value per dimension.
    pub build: fn(&[f64]) -> Result<Body, String>,
}

impl Part {
    /// The part with the dimensions it is shown with first.
    pub fn default_body(&self) -> Result<Body, String> {
        let values: Vec<f64> = self.dimensions.iter().map(|d| d.value).collect();
        (self.build)(&values)
    }
}

/// The sample parts, with the ranges over which each stays valid. A round
/// that runs around a rounded corner has to be smaller than that corner, so
/// those ranges do not overlap.
pub static PARTS: [Part; 4] = [
    Part {
        name: "mounting-block",
        title: "Mounting block",
        about: "A plate with rounded corners and one round running around the whole rim, a boss with a fillet at its foot and a chamfer on top, a bore and four counterbored holes.",
        dimensions: &[
            dim("Length", 80.0, 120.0, 80.0),
            dim("Width", 50.0, 80.0, 50.0),
            dim("Thickness", 10.0, 24.0, 14.0),
            dim("Corner radius", 4.5, 10.0, 6.0),
            dim("Rim radius", 0.0, 4.0, 2.5),
            dim("Boss diameter", 20.0, 32.0, 28.0),
            dim("Boss height", 4.0, 20.0, 12.0),
            dim("Bore diameter", 6.0, 16.0, 14.0),
            dim("Counterbore diameter", 7.0, 12.0, 10.0),
        ],
        build: |d| {
            MountingBlock {
                length: d[0],
                width: d[1],
                thickness: d[2],
                corner: d[3],
                rim: d[4],
                boss_radius: 0.5 * d[5],
                boss_height: d[6],
                bore_radius: 0.5 * d[7],
                counterbore_radius: 0.5 * d[8],
                ..Default::default()
            }
            .build()
        },
    },
    Part {
        name: "bracket",
        title: "Folded bracket",
        about: "An L section extruded, its bend and plate corners rounded, three holes. The edge break is a single fillet: its chains run along straight edges, around the rounded corners as torus segments, and back.",
        dimensions: &[
            dim("Width", 30.0, 60.0, 40.0),
            dim("Bend radius", 1.5, 6.0, 4.0),
            dim("Corner radius", 1.5, 6.0, 3.0),
            dim("Edge break", 0.0, 1.0, 0.6),
            dim("Upright hole diameter", 8.0, 24.0, 24.0),
            dim("Base hole diameter", 4.0, 9.0, 7.0),
        ],
        build: |d| {
            Bracket {
                width: d[0],
                bend: d[1],
                corner: d[2],
                edge_break: d[3],
                upright_hole_radius: 0.5 * d[4],
                base_hole_radius: 0.5 * d[5],
                ..Default::default()
            }
            .build()
        },
    },
    Part {
        name: "rounded-box",
        title: "Rounded box",
        about: "Every edge of a box rounded with one radius: twelve cylinders that meet in eight sphere patches.",
        dimensions: &[
            dim("Length", 30.0, 70.0, 50.0),
            dim("Width", 20.0, 50.0, 34.0),
            dim("Height", 14.0, 30.0, 22.0),
            dim("Radius", 0.0, 6.5, 6.0),
        ],
        build: |d| {
            RoundedBox {
                size: v3(d[0], d[1], d[2]),
                radius: d[3],
            }
            .build()
        },
    },
    Part {
        name: "cross-drilled-shaft",
        title: "Cross-drilled shaft",
        about: "A shaft with an axial bore and a cross hole. The holes meet the wall and each other in exact intersection curves; with equal diameters those curves cross.",
        dimensions: &[
            dim("Cross hole diameter", 2.0, 16.0, 10.0),
            dim("Cross hole height", 14.0, 46.0, 38.0),
            dim("Bore diameter", 2.0, 16.0, 8.0),
            dim("Chamfer", 0.0, 2.5, 1.5),
        ],
        build: |d| {
            DrilledShaft {
                cross_radius: 0.5 * d[0],
                cross_height: d[1],
                bore_radius: 0.5 * d[2],
                chamfer: d[3],
                ..Default::default()
            }
            .build()
        },
    },
];

/// Every sample part with the dimensions it is shown with first.
pub fn showcase() -> Result<Vec<(&'static str, Body)>, String> {
    PARTS
        .iter()
        .map(|part| Ok((part.name, part.default_body()?)))
        .collect()
}

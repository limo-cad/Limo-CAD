//! Compact display of rotational stock. Fit the *remaining occupancy*, never
//! the target CAD model. Every cell must agree outside a one-cell boundary
//! band; an off-axis pocket, flat, slot, or partial cut rejects the entire fit.
//! This changes neither removal nor the verification grid.
use super::*;

#[derive(Clone, Copy, Debug)]
struct Ring {
    inner: f64,
    outer: f64,
}

#[derive(Clone, Copy)]
struct Slab {
    lo: Ring,
    hi: Ring,
    z0: f64,
    z1: f64,
}

pub(super) fn surface(stock: &VoxelStock, budget: usize) -> Option<CamSimulationMeshDto> {
    let (center, stock_radius) = stock.display_cuts.initial.as_ref()?.cylinder()?;
    let [nx, ny, nz] = stock.dimensions;
    let [dx, dy, dz] = stock.cell_size;

    let tolerance = dx.max(dy);
    if stock_radius < tolerance * 8. {
        return None;
    }
    let radii: Vec<_> = (0..ny)
        .flat_map(|y| {
            (0..nx).map(move |x| {
                (stock.min.x + (x as f64 + 0.5) * dx - center.x)
                    .hypot(stock.min.y + (y as f64 + 0.5) * dy - center.y)
            })
        })
        .collect();
    let mut rings = Vec::with_capacity(nz);
    for z in 0..nz {
        let mut low = f64::INFINITY;
        let mut high = 0_f64;
        let mut count = 0;
        for (i, &r) in radii.iter().enumerate() {
            if stock.is_occupied_index(z * nx * ny + i) {
                low = low.min(r);
                high = high.max(r);
                count += 1;
            }
        }
        if count == 0 {
            rings.push(None);
            continue;
        }
        let midpoint = (low + high) * 0.5;
        let hole = if low > 2. * tolerance {
            radii
                .iter()
                .enumerate()
                .filter(|&(i, &r)| r < midpoint && !stock.is_occupied_index(z * nx * ny + i))
                .count()
        } else {
            0
        };
        let area = dx * dy / std::f64::consts::PI;
        let ring = Ring {
            inner: (hole as f64 * area).sqrt(),
            outer: ((count + hole) as f64 * area).sqrt().min(stock_radius),
        };
        if ring.inner > 0. && ring.outer - ring.inner < tolerance * 4. {
            return None;
        }

        for (i, &r) in radii.iter().enumerate() {
            let occupied = stock.is_occupied_index(z * nx * ny + i);
            if occupied {
                if r < ring.inner - tolerance || r > ring.outer + tolerance {
                    return None;
                }
            } else if r < ring.outer - tolerance && (ring.inner == 0. || r > ring.inner + tolerance)
            {
                return None;
            }
        }
        rings.push(Some(ring));
    }

    let blend = |a: Ring, b: Option<Ring>| -> Ring {
        let value = |x: f64, y: f64| {
            if (x - y).abs() <= tolerance && (x == 0.) == (y == 0.) {
                (x + y) * 0.5
            } else {
                x
            }
        };
        b.map_or(a, |b| Ring {
            inner: value(a.inner, b.inner),
            outer: value(a.outer, b.outer),
        })
    };
    let mut slabs: Vec<Slab> = Vec::new();
    for (z, ring) in rings.iter().enumerate() {
        let Some(r) = *ring else {
            continue;
        };
        let slab = Slab {
            lo: blend(r, z.checked_sub(1).and_then(|i| rings[i])),
            hi: blend(r, rings.get(z + 1).copied().flatten()),
            z0: stock.min.z + z as f64 * dz,
            z1: stock.min.z + (z + 1) as f64 * dz,
        };
        if let Some(last) = slabs.last_mut() {
            if last.z1 == slab.z0
                && last.lo.inner == last.hi.inner
                && last.lo.outer == last.hi.outer
                && slab.lo.inner == slab.hi.inner
                && slab.lo.outer == slab.hi.outer
                && last.hi.inner == slab.lo.inner
                && last.hi.outer == slab.lo.outer
            {
                last.z1 = slab.z1;
                continue;
            }
        }
        slabs.push(slab);
    }
    if slabs.is_empty() {
        return None;
    }
    let chord = tolerance * 0.1;
    let segments = ((std::f64::consts::TAU / (8. * chord / stock_radius).sqrt()).ceil() as usize)
        .clamp(96, 512);
    let mut mesh = CamSimulationMeshDto {
        positions: vec![],
        normals: vec![],
        triangle_count: 0,
    };
    for (i, slab) in slabs.iter().enumerate() {
        let previous = i
            .checked_sub(1)
            .map(|j| slabs[j])
            .filter(|s| s.z1 == slab.z0);
        let next = slabs.get(i + 1).copied().filter(|s| s.z0 == slab.z1);
        for inner in [false, true] {
            let r = |s: Ring| if inner { s.inner } else { s.outer };
            if inner && r(slab.lo) == 0. && r(slab.hi) == 0. {
                continue;
            }
            let slope = |s: Slab| (r(s.hi) - r(s.lo)) / (s.z1 - s.z0);
            let current = slope(*slab);
            let blend_slope = |other: Option<Slab>, at: f64, top: bool| {
                other
                    .filter(|s| r(if top { s.lo } else { s.hi }) == at)
                    .map(slope)
                    .filter(|s| (s - current).abs() < 0.5)
                    .map_or(current, |s| (s + current) * 0.5)
            };
            let slopes = [
                blend_slope(previous, r(slab.lo), false),
                blend_slope(next, r(slab.hi), true),
            ];
            for k in 0..segments {
                let a = k as f64 * std::f64::consts::TAU / segments as f64;
                let b = ((k + 1) % segments) as f64 * std::f64::consts::TAU / segments as f64;
                let p = |angle: f64, radius: f64, z: f64| {
                    [
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                        z,
                    ]
                };
                let n = |angle: f64, slope: f64| {
                    let scale = if inner { -1. } else { 1. } / (1. + slope * slope).sqrt();
                    [
                        (scale * angle.cos()) as f32,
                        (scale * angle.sin()) as f32,
                        (-scale * slope) as f32,
                    ]
                };
                let points = [
                    p(a, r(slab.lo), slab.z0),
                    p(b, r(slab.lo), slab.z0),
                    p(b, r(slab.hi), slab.z1),
                    p(a, r(slab.hi), slab.z1),
                ];
                let normals = [
                    n(a, slopes[0]),
                    n(b, slopes[0]),
                    n(b, slopes[1]),
                    n(a, slopes[1]),
                ];
                quad(&mut mesh, points, normals, inner);
            }
        }
        for (ring, adjacent, z, up) in [
            (slab.lo, previous.map(|s| s.hi), slab.z0, false),
            (slab.hi, next.map(|s| s.lo), slab.z1, true),
        ] {
            for (low, high) in difference(ring, adjacent) {
                for k in 0..segments {
                    let a = k as f64 * std::f64::consts::TAU / segments as f64;
                    let b = ((k + 1) % segments) as f64 * std::f64::consts::TAU / segments as f64;
                    let p = |r: f64, t: f64| [center.x + r * t.cos(), center.y + r * t.sin(), z];
                    quad(
                        &mut mesh,
                        [p(low, a), p(high, a), p(high, b), p(low, b)],
                        [[0., 0., if up { 1. } else { -1. }]; 4],
                        !up,
                    );
                }
            }
        }
        if mesh.positions.len() / 9 > budget {
            return None;
        }
    }
    mesh.triangle_count = mesh.positions.len() / 9;
    Some(mesh)
}

fn difference(r: Ring, other: Option<Ring>) -> Vec<(f64, f64)> {
    let Some(o) = other else {
        return vec![(r.inner, r.outer)];
    };
    [
        (r.inner, r.outer.min(o.inner)),
        (r.inner.max(o.outer), r.outer),
    ]
    .into_iter()
    .filter(|(a, b)| b - a > 1e-10)
    .collect()
}

fn quad(mesh: &mut CamSimulationMeshDto, p: [[f64; 3]; 4], n: [[f32; 3]; 4], reverse: bool) {
    for ids in [[0, 1, 2], [0, 2, 3]] {
        let ids = if reverse {
            [ids[0], ids[2], ids[1]]
        } else {
            ids
        };
        let points = ids.map(|i| p[i]);
        if points[0] == points[1] || points[1] == points[2] || points[0] == points[2] {
            continue;
        }
        push_triangle_with_normals(
            &mut mesh.positions,
            &mut mesh.normals,
            points,
            ids.map(|i| n[i]),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn fixture(cut: impl Fn(Point3Dto) -> bool) -> VoxelStock {
        let spec = GridSpec::for_stock(
            &StockBoxDto {
                min: Point3Dto::new(-12., -12., 0.),
                max: Point3Dto::new(12., 12., 10.),
            },
            Some(0.25),
            HARD_MAX_VOXELS,
        )
        .unwrap();
        let mut stock = VoxelStock::filled(&spec, |p| {
            let r = p.x.hypot(p.y);
            r <= if p.z > 5. { 8. } else { 10. } && (p.z <= 2. || r >= 3.) && !cut(p)
        });
        stock.display_cuts.initial = Some(surface::StockBoundary::new(
            &spec,
            CamResolvedStockDto::Cylinder {
                center: crate::Point2Dto::new(0., 0.),
                radius: 12.,
            },
        ));
        stock.display_cuts.limited = true;
        stock
    }

    #[test]
    fn round_stock_is_closed_smooth_and_keeps_bore_and_sharp_shoulders() {
        let stock = fixture(|_| false);
        let words = stock.occupied.clone();
        let count = stock.occupied_count;
        let mesh = surface(&stock, MAX_SURFACE_TRIANGLES).unwrap();
        assert_eq!(stock.occupied, words);
        assert_eq!(stock.occupied_count, count);
        assert!(mesh.triangle_count < 3000);
        let mut edges = HashMap::new();
        let mut walls = 0;
        let mut flats = 0;
        for (p, n) in mesh
            .positions
            .as_chunks::<9>()
            .0
            .iter()
            .zip(mesh.normals.as_chunks::<9>().0.iter())
        {
            let vertex: [[u32; 3]; 3] = std::array::from_fn(|i| {
                std::array::from_fn(|k| {
                    let v = p[i * 3 + k];
                    if v == 0. {
                        0
                    } else {
                        v.to_bits()
                    }
                })
            });
            for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                let key = if vertex[a] < vertex[b] {
                    [vertex[a], vertex[b]]
                } else {
                    [vertex[b], vertex[a]]
                };
                *edges.entry(key).or_insert(0) += 1;
            }
            if p[2] == p[5] && p[5] == p[8] {
                assert!([0., 2., 5., 10.].contains(&p[2]));
                assert!([n[2], n[5], n[8]].iter().all(|z| z.abs() > 0.999));
                flats += 1;
            } else {
                for i in 0..3 {
                    let r = p[i * 3].hypot(p[i * 3 + 1]);
                    assert!([3., 8., 10.].iter().any(|truth| (r - truth).abs() < 0.05));
                    let dot = (p[i * 3] * n[i * 3] + p[i * 3 + 1] * n[i * 3 + 1]) / r;
                    assert!(if r < 4. { dot < -0.999 } else { dot > 0.999 });
                }
                walls += 1;
            }
        }
        assert!(walls > 100 && flats > 100);
        assert!(
            edges.values().all(|count| *count == 2),
            "closed seam, caps, shoulder and bore"
        );
        let mut warnings = vec![];
        assert_eq!(
            stock
                .presentation_mesh(MAX_SURFACE_TRIANGLES, &mut warnings)
                .unwrap(),
            mesh
        );
        assert!(warnings.iter().any(|s| s.contains("Round-stock display")));
        let mut exact = fixture(|_| false);
        exact.display_cuts.limited = false;
        let mut warnings = vec![];
        exact
            .presentation_mesh(MAX_SURFACE_TRIANGLES, &mut warnings)
            .unwrap();
        assert!(!warnings.iter().any(|s| s.contains("Round-stock display")));
    }

    #[test]
    fn flats_slots_off_axis_holes_and_insufficient_budgets_keep_general_surface() {
        for cut in [
            (|p: Point3Dto| p.x > 6. && p.z > 6.) as fn(Point3Dto) -> bool,
            |p| (p.x - 5.).hypot(p.y) < 1.,
            |p| p.x > 2. && p.y.abs() < 0.5 && p.z > 6.,
        ] {
            assert!(surface(&fixture(cut), MAX_SURFACE_TRIANGLES).is_none());
        }
        assert!(surface(&fixture(|_| false), 12).is_none());
        let mut stock = fixture(|_| false);
        stock.display_cuts.initial = None;
        assert!(surface(&stock, MAX_SURFACE_TRIANGLES).is_none());
    }
}

//! Constrained Delaunay triangulation by edge flips.
//!
//! A [`Net`] starts from any triangulation of a region of the plane. The
//! sides that only one triangle uses are the region's outline and never
//! change. Every other side is swapped for the other diagonal of its two
//! triangles wherever that brings the triangulation closer to Delaunay, and
//! points added inside afterwards keep it there.
//!
//! Coordinates are whatever the caller measures closeness in. The mesher
//! passes surface parameters scaled to the longest chord the tolerance
//! allows in each direction, so "round" here means "equally accurate every
//! way" there.

use crate::fast::Map;
use crate::math::*;

const NOBODY: u32 = u32::MAX;

/// Twice the area below which a point counts as lying on a side.
const ON_SIDE: f64 = 1.0e-9;

pub struct Net {
    pub points: Vec<V2>,
    /// Counter-clockwise triangles of point indices.
    pub tris: Vec<[u32; 3]>,
    /// Per triangle and side (from corner `k` to corner `k + 1`): the
    /// triangle on the other side, or `NOBODY` on the outline.
    across: Vec<[u32; 3]>,
    /// Sides waiting to be looked at.
    stack: Vec<(u32, u8)>,
}

#[inline]
fn orient(a: V2, b: V2, c: V2) -> f64 {
    (b - a).cross(c - a)
}

/// Where a point lies in a triangle that contains it.
enum Spot {
    Inside,
    Side(usize),
    Corner,
}

impl Net {
    /// Take over a triangulation and make it Delaunay.
    pub fn new(points: Vec<V2>, tris: Vec<[u32; 3]>) -> Net {
        let mut across = vec![[NOBODY; 3]; tris.len()];
        let mut open: Map<(u32, u32), (u32, u8)> = Map::default();
        for (t, tri) in tris.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                match open.remove(&(b, a)) {
                    Some((other, side)) => {
                        across[t][k] = other;
                        across[other as usize][side as usize] = t as u32;
                    }
                    None => {
                        open.insert((a, b), (t as u32, k as u8));
                    }
                }
            }
        }
        let mut net = Net {
            points,
            tris,
            across,
            stack: Vec::new(),
        };
        for t in 0..net.tris.len() as u32 {
            for k in 0..3 {
                net.stack.push((t, k));
            }
        }
        net.settle();
        net
    }

    /// Look at every side again. Taking over a triangulation stops after a
    /// number of swaps in proportion to its size, which one far from
    /// Delaunay can use up; points added since have put right what is near
    /// them, and this does the rest.
    pub fn tidy(&mut self) {
        for t in 0..self.tris.len() as u32 {
            for k in 0..3 {
                self.stack.push((t, k));
            }
        }
        self.settle();
    }

    /// Whether side `k` of triangle `t` is part of the outline.
    pub fn on_outline(&self, t: usize, k: usize) -> bool {
        self.across[t][k] == NOBODY
    }

    /// Add a point inside the region. `near` is any triangle, preferably one
    /// close by. Returns a triangle near the new point, or `None` when the
    /// point is outside, on the outline or on top of an existing point.
    pub fn insert(&mut self, q: V2, near: u32) -> Option<u32> {
        let (t, spot) = self.locate(q, near)?;
        let id = self.points.len() as u32;
        let ti = t as usize;
        match spot {
            Spot::Corner => return None,
            Spot::Inside => {
                self.points.push(q);
                let [a, b, c] = self.tris[ti];
                let [n_ab, n_bc, n_ca] = self.across[ti];
                let (t1, t2) = (self.tris.len() as u32, self.tris.len() as u32 + 1);
                self.tris[ti] = [a, b, id];
                self.across[ti] = [n_ab, t1, t2];
                self.tris.push([b, c, id]);
                self.across.push([n_bc, t2, t]);
                self.tris.push([c, a, id]);
                self.across.push([n_ca, t, t1]);
                self.relink(n_bc, t, t1, c, b);
                self.relink(n_ca, t, t2, a, c);
                self.stack.extend([(t, 0), (t1, 0), (t2, 0)]);
            }
            Spot::Side(k) => {
                let n = self.across[ti][k];
                if n == NOBODY {
                    return None;
                }
                let ni = n as usize;
                let tri = self.tris[ti];
                let (a, b, c) = (tri[k], tri[(k + 1) % 3], tri[(k + 2) % 3]);
                let other = self.tris[ni];
                let j = (0..3).find(|&j| other[j] == b && other[(j + 1) % 3] == a)?;
                let d = other[(j + 2) % 3];
                let (t_bc, t_ca) = (self.across[ti][(k + 1) % 3], self.across[ti][(k + 2) % 3]);
                let (n_ad, n_db) = (self.across[ni][(j + 1) % 3], self.across[ni][(j + 2) % 3]);
                self.points.push(q);
                let (t3, n3) = (self.tris.len() as u32, self.tris.len() as u32 + 1);
                self.tris[ti] = [a, id, c];
                self.across[ti] = [n, t3, t_ca];
                self.tris[ni] = [id, a, d];
                self.across[ni] = [t, n_ad, n3];
                self.tris.push([id, b, c]);
                self.across.push([n3, t_bc, t]);
                self.tris.push([b, id, d]);
                self.across.push([t3, n, n_db]);
                self.relink(t_bc, t, t3, c, b);
                self.relink(n_db, n, n3, b, d);
                self.stack.extend([(t, 2), (t3, 1), (n, 1), (n3, 2)]);
            }
        }
        self.settle();
        Some(t)
    }

    /// The triangle holding `q`: a walk towards it from `near`, and a search
    /// of everything when the outline or a loop gets in the way.
    fn locate(&self, q: V2, near: u32) -> Option<(u32, Spot)> {
        let count = self.tris.len();
        if count == 0 {
            return None;
        }
        let mut t = (near as usize).min(count - 1);
        let limit = 16 + 4 * (count as f64).sqrt() as usize;
        'walk: for step in 0..limit {
            let tri = self.tris[t];
            let p = tri.map(|i| self.points[i as usize]);
            for i in 0..3 {
                // Trying the sides in a different order each time breaks
                // the cycles a walk can otherwise fall into.
                let k = (i + step) % 3;
                if orient(p[k], p[(k + 1) % 3], q) < -ON_SIDE {
                    let n = self.across[t][k];
                    if n == NOBODY {
                        break 'walk;
                    }
                    t = n as usize;
                    continue 'walk;
                }
            }
            return Some((t as u32, self.spot(t, q)));
        }
        (0..count)
            .find(|&t| {
                let p = self.tris[t].map(|i| self.points[i as usize]);
                (0..3).all(|k| orient(p[k], p[(k + 1) % 3], q) >= -ON_SIDE)
            })
            .map(|t| (t as u32, self.spot(t, q)))
    }

    fn spot(&self, t: usize, q: V2) -> Spot {
        let p = self.tris[t].map(|i| self.points[i as usize]);
        let on: Vec<usize> = (0..3)
            .filter(|&k| orient(p[k], p[(k + 1) % 3], q) <= ON_SIDE)
            .collect();
        match on.len() {
            0 => Spot::Inside,
            1 => Spot::Side(on[0]),
            _ => Spot::Corner,
        }
    }

    /// In triangle `m`, the side from `a` to `b` that looked at `from` now
    /// looks at `to`.
    fn relink(&mut self, m: u32, from: u32, to: u32, a: u32, b: u32) {
        if m == NOBODY {
            return;
        }
        let tri = self.tris[m as usize];
        for s in 0..3 {
            if tri[s] == a && tri[(s + 1) % 3] == b && self.across[m as usize][s] == from {
                self.across[m as usize][s] = to;
                return;
            }
        }
    }

    /// Work through the waiting sides.
    fn settle(&mut self) {
        let mut budget = 1024 + 64 * self.tris.len();
        while let Some((t, k)) = self.stack.pop() {
            if budget == 0 {
                self.stack.clear();
                break;
            }
            budget -= 1;
            self.flip(t, k as usize);
        }
    }

    /// The two triangles on side `k` of triangle `t`: the neighbour, which
    /// of its sides this is, and the four corners. The side runs from the
    /// first to the second; the third is `t`'s own, the fourth the
    /// neighbour's.
    fn pair(&self, t: u32, k: usize) -> Option<(u32, usize, [u32; 4])> {
        let n = self.across[t as usize][k];
        if n == NOBODY {
            return None;
        }
        let tri = self.tris[t as usize];
        let (a, b, c) = (tri[k], tri[(k + 1) % 3], tri[(k + 2) % 3]);
        let other = self.tris[n as usize];
        let j = (0..3).find(|&j| other[j] == b && other[(j + 1) % 3] == a)?;
        (self.across[n as usize][j] == t).then_some((n, j, [a, b, c, other[(j + 2) % 3]]))
    }

    /// Whether the other diagonal of those two triangles would leave both
    /// with their sense.
    fn can_swap(&self, [a, b, c, d]: [u32; 4]) -> bool {
        let at = |i: u32| self.points[i as usize];
        let (pa, pb, pc, pd) = (at(a), at(b), at(c), at(d));
        let size = (pa - pd).len().max((pb - pd).len()).max((pc - pd).len());
        let least = 1.0e-12 * size * size;
        orient(pa, pd, pc) > least && orient(pd, pb, pc) > least
    }

    /// Swap side `k` of triangle `t`, found by [`Net::pair`], for the other
    /// diagonal.
    fn swap(&mut self, t: u32, k: usize, n: u32, j: usize, [a, b, c, d]: [u32; 4]) {
        let (ti, ni) = (t as usize, n as usize);
        let (t_bc, t_ca) = (self.across[ti][(k + 1) % 3], self.across[ti][(k + 2) % 3]);
        let (n_ad, n_db) = (self.across[ni][(j + 1) % 3], self.across[ni][(j + 2) % 3]);
        self.tris[ti] = [a, d, c];
        self.across[ti] = [n_ad, n, t_ca];
        self.tris[ni] = [d, b, c];
        self.across[ni] = [n_db, t_bc, t];
        self.relink(n_ad, n, t, d, a);
        self.relink(t_bc, t, n, c, b);
    }

    /// Swap side `k` of triangle `t` for the other diagonal if the corner
    /// opposite it in the neighbour lies inside this triangle's circle.
    fn flip(&mut self, t: u32, k: usize) {
        let Some((n, j, corners)) = self.pair(t, k) else {
            return;
        };
        let [a, b, c, d] = corners;
        let (pa, pb, pc, pd) = (
            self.points[a as usize],
            self.points[b as usize],
            self.points[c as usize],
            self.points[d as usize],
        );
        let (ax, ay) = (pa.x - pd.x, pa.y - pd.y);
        let (bx, by) = (pb.x - pd.x, pb.y - pd.y);
        let (cx, cy) = (pc.x - pd.x, pc.y - pd.y);
        let (aw, bw, cw) = (ax * ax + ay * ay, bx * bx + by * by, cx * cx + cy * cy);
        let inside = aw * (bx * cy - cx * by) - bw * (ax * cy - cx * ay) + cw * (ax * by - bx * ay);
        let size = aw.max(bw).max(cw);
        if inside <= 1.0e-10 * size * size {
            return;
        }
        // Both new triangles must keep their sense.
        let least = 1.0e-12 * size;
        if orient(pa, pd, pc) <= least || orient(pd, pb, pc) <= least {
            return;
        }
        self.swap(t, k, n, j, corners);
        for s in 0..3 {
            self.stack.push((t, s));
            self.stack.push((n, s));
        }
    }

    /// Swap sides to be rid of triangles that `sound` objects to, wherever
    /// the other diagonal leaves two that it accepts.
    ///
    /// The plane this net lies in is only a picture of the surface being
    /// meshed. Where the picture is skewed, a long thin triangle that faces
    /// the right way here can face the other way there, and the caller
    /// knows which. This is a last step: what it leaves need not be
    /// Delaunay.
    pub fn mend(&mut self, sound: impl Fn([u32; 3]) -> bool) {
        for _ in 0..4 {
            let mut mended = false;
            for t in 0..self.tris.len() as u32 {
                if sound(self.tris[t as usize]) {
                    continue;
                }
                for k in 0..3 {
                    let Some((n, j, corners)) = self.pair(t, k) else {
                        continue;
                    };
                    let [a, b, c, d] = corners;
                    if self.can_swap(corners) && sound([a, d, c]) && sound([d, b, c]) {
                        self.swap(t, k, n, j, corners);
                        mended = true;
                        break;
                    }
                }
            }
            if !mended {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(net: &Net) -> f64 {
        net.tris
            .iter()
            .map(|t| {
                let p = t.map(|i| net.points[i as usize]);
                0.5 * orient(p[0], p[1], p[2])
            })
            .sum()
    }

    /// No triangle's circle may hold the far corner of a neighbour.
    fn assert_delaunay(net: &Net) {
        for (t, tri) in net.tris.iter().enumerate() {
            let p = tri.map(|i| net.points[i as usize]);
            assert!(
                orient(p[0], p[1], p[2]) > 0.0,
                "triangle {t} is not counter-clockwise"
            );
            for k in 0..3 {
                let n = net.across[t][k];
                if n == NOBODY {
                    continue;
                }
                let other = net.tris[n as usize];
                let j = (0..3)
                    .find(|&j| other[j] == tri[(k + 1) % 3] && other[(j + 1) % 3] == tri[k])
                    .expect("neighbours share a side");
                assert_eq!(net.across[n as usize][j], t as u32);
                let d = net.points[other[(j + 2) % 3] as usize];
                let (a, b, c) = (p[0] - d, p[1] - d, p[2] - d);
                let inside = a.dot(a) * b.cross(c) - b.dot(b) * a.cross(c) + c.dot(c) * a.cross(b);
                assert!(
                    inside <= 1.0e-7,
                    "triangle {t} side {k} is not Delaunay: {inside}"
                );
            }
        }
    }

    #[test]
    fn a_fan_becomes_delaunay_and_takes_points() {
        // A flat ellipse, fanned from one end: about as far from Delaunay
        // as a triangulation gets.
        let count = 40;
        let points: Vec<V2> = (0..count)
            .map(|i| {
                let angle = TAU * i as f64 / count as f64;
                v2(8.0 * angle.cos(), 2.0 * angle.sin())
            })
            .collect();
        let whole: f64 = (0..count)
            .map(|i| 0.5 * points[i].cross(points[(i + 1) % count]))
            .sum();
        let tris: Vec<[u32; 3]> = (1..count as u32 - 1).map(|i| [0, i, i + 1]).collect();
        let mut net = Net::new(points, tris);
        assert!((area(&net) - whole).abs() < 1.0e-9);
        assert_delaunay(&net);

        // A square grid: points in line, four to a circle, and many of them
        // exactly on sides that are already there.
        let (mut near, mut wanted, mut added) = (0, 0, 0);
        for j in -8..=8 {
            for i in -32..=32 {
                let q = v2(0.25 * i as f64, 0.25 * j as f64);
                if (q.x / 8.0).powi(2) + (q.y / 2.0).powi(2) < 0.9 {
                    wanted += 1;
                    if let Some(t) = net.insert(q, near) {
                        near = t;
                        added += 1;
                    }
                }
            }
        }
        assert_eq!(added, wanted);
        assert!((area(&net) - whole).abs() < 1.0e-9);
        assert_delaunay(&net);
        // Outside, on an existing point and on the outline: all refused.
        assert!(net.insert(v2(9.0, 0.0), near).is_none());
        assert!(net.insert(v2(0.25, 0.25), near).is_none());
        let on_outline = (net.points[0] + net.points[1]) * 0.5;
        assert!(net.insert(on_outline, near).is_none());
    }

    #[test]
    fn the_outline_of_a_notched_region_stays() {
        // An L: the walk from one arm to the other runs into the outline.
        let points = vec![
            v2(0.0, 0.0),
            v2(8.0, 0.0),
            v2(8.0, 2.0),
            v2(2.0, 2.0),
            v2(2.0, 8.0),
            v2(0.0, 8.0),
        ];
        let tris = vec![[0, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 5]];
        let mut net = Net::new(points, tris);
        let mut near = 0;
        for &q in &[
            v2(7.0, 1.0),
            v2(1.0, 7.0),
            v2(6.5, 0.7),
            v2(0.6, 6.4),
            v2(1.0, 1.0),
        ] {
            near = net.insert(q, near).expect("inside the L");
        }
        assert!((area(&net) - 28.0).abs() < 1.0e-9);
        assert_delaunay(&net);
        // Nothing crosses the notch.
        for tri in &net.tris {
            let centre = tri
                .iter()
                .fold(v2(0.0, 0.0), |sum, &i| sum + net.points[i as usize])
                * (1.0 / 3.0);
            assert!(
                centre.x < 2.0 || centre.y < 2.0,
                "a triangle fills the notch"
            );
        }
    }
}

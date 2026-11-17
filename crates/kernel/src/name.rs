//! Names for faces and edges that outlast their indices.
//!
//! An index says where a face happens to sit in a body's arrays, and every
//! Boolean deals the faces out afresh. A name says where the face came from.
//! Each face carries a tag ([`Body::f_tag`]) set by whatever made it: this
//! side of that extrusion, the round on that edge. Operations hand tags on
//! unchanged, to every piece when a face is cut up, so a tag follows its
//! face through any amount of later modelling.
//!
//! A face's name is its tag. Where several faces carry one tag (the pieces
//! of a face cut in two) the name goes on to say which faces it borders,
//! and if that still leaves a choice, which one it is in a fixed order of
//! position.
//!
//! An edge is named after the two tags it lies between: "where the front
//! meets the right side". That holds however either face is cut up later.
//! Only when several edges lie between the same two tags does an edge's
//! name fall back on the names of its two faces, and a number.
//!
//! A reference kept from an earlier state of the model is looked up by
//! name first. If the exact name is gone, what the name says is still
//! tried: the only face left with that tag, every edge now between those
//! two tags (an edge cut in two is still that edge). Only when that fails
//! does the reference need reselecting.

use crate::fast::Map;
use crate::math::*;
use crate::topo::Body;

/// Combine two tags into one that depends on both, in that order.
pub fn mix(a: u64, b: u64) -> u64 {
    let mut h = (a ^ 0x9e37_79b9_7f4a_7c15).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h ^= h >> 29;
    h = (h ^ b).wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^ (h >> 32)
}

/// A tag from text, such as a name made by [`Names`].
pub fn tag_of(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

/// What the constructors in [`crate::make`] say about the faces they
/// build, as the tag of each. A caller that knows more (which feature this
/// is, which sketch line that side follows) reads the role back and writes
/// a tag of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The flat end where an extrusion or a part turn starts.
    Start,
    /// The flat end where it stops.
    End,
    /// Swept by one segment of the profile: which loop, and which segment
    /// of that loop as the caller gave them.
    Side { lp: u32, seg: u32 },
    /// Anything else.
    Other,
}

impl Role {
    pub fn tag(self) -> u64 {
        match self {
            Role::Start => 1 << 56,
            Role::End => 2 << 56,
            Role::Side { lp, seg } => 3 << 56 | u64::from(lp) << 28 | u64::from(seg),
            Role::Other => 0,
        }
    }

    pub fn of(tag: u64) -> Role {
        match tag >> 56 {
            1 if tag == 1 << 56 => Role::Start,
            2 if tag == 2 << 56 => Role::End,
            3 => Role::Side {
                lp: (tag >> 28 & 0x0fff_ffff) as u32,
                seg: (tag & 0x0fff_ffff) as u32,
            },
            _ => Role::Other,
        }
    }
}

/// The names of every face and edge of one body.
pub struct Names {
    pub faces: Vec<String>,
    pub edges: Vec<String>,
    face_at: Map<u64, u32>,
    edge_at: Map<u64, u32>,
    /// Faces by tag, for a name that no longer matches exactly.
    tagged: Map<u64, Vec<u32>>,
    /// Edges by the pair of faces they separate (lower index first).
    between: Map<(u32, u32), Vec<u32>>,
    /// Edges by the tags of the two faces they separate (lower first).
    joining: Map<(u64, u64), Vec<u32>>,
}

/// The tag a face name starts with.
fn tag_in(name: &str) -> Option<u64> {
    let body = name.strip_prefix('f')?;
    let tag = body.split_once('~').map_or(body, |(tag, _)| tag);
    u64::from_str_radix(tag, 16).ok()
}

/// Where something is, coarsely enough for rounding not to reorder things.
fn place(p: V3) -> [i64; 3] {
    [p.x, p.y, p.z].map(|c| (c * 1.0e6).round() as i64)
}

impl Names {
    pub fn of(body: &Body) -> Names {
        let (nf, ne) = (body.face_count(), body.edge_count());
        // The tags around each face, and a point to order faces by.
        let mut around: Vec<Vec<u64>> = vec![Vec::new(); nf];
        let mut middle: Vec<(V3, f64)> = vec![(V3::ZERO, 0.0); nf];
        for e in 0..ne as u32 {
            let (a, b) = (body.h_face(2 * e), body.h_face(2 * e + 1));
            around[a as usize].push(body.f_tag[b as usize]);
            around[b as usize].push(body.f_tag[a as usize]);
            let mid = body.e_mid(e);
            for f in [a, b] {
                middle[f as usize].0 += mid;
                middle[f as usize].1 += 1.0;
            }
        }
        let context: Vec<u64> = around
            .iter_mut()
            .map(|tags| {
                tags.sort_unstable();
                tags.dedup();
                tags.iter().fold(0x51_7cc1_b727_220a, |h, t| mix(h, *t))
            })
            .collect();
        let mut tagged: Map<u64, Vec<u32>> = Map::default();
        for f in 0..nf as u32 {
            tagged.entry(body.f_tag[f as usize]).or_default().push(f);
        }
        let at = |f: u32| place(middle[f as usize].0 / middle[f as usize].1.max(1.0));
        let mut faces: Vec<String> = vec![String::new(); nf];
        for (tag, members) in &tagged {
            if members.len() == 1 {
                faces[members[0] as usize] = format!("f{tag:x}");
                continue;
            }
            // Told apart by their neighbours, then by where they are.
            let mut order = members.clone();
            order.sort_by_key(|&f| (context[f as usize], at(f), f));
            let mut k = 0;
            while k < order.len() {
                let ctx = context[order[k] as usize];
                let run = order[k..]
                    .iter()
                    .take_while(|&&f| context[f as usize] == ctx)
                    .count();
                for (i, &f) in order[k..k + run].iter().enumerate() {
                    faces[f as usize] = if run == 1 {
                        format!("f{tag:x}~{:x}", ctx & 0xffff_ffff)
                    } else {
                        format!("f{tag:x}~{:x}.{i}", ctx & 0xffff_ffff)
                    };
                }
                k += run;
            }
        }
        let mut between: Map<(u32, u32), Vec<u32>> = Map::default();
        let mut joining: Map<(u64, u64), Vec<u32>> = Map::default();
        for e in 0..ne as u32 {
            let (a, b) = (body.h_face(2 * e), body.h_face(2 * e + 1));
            between.entry((a.min(b), a.max(b))).or_default().push(e);
            let (ta, tb) = (body.f_tag[a as usize], body.f_tag[b as usize]);
            joining.entry((ta.min(tb), ta.max(tb))).or_default().push(e);
        }
        let mut edges: Vec<String> = vec![String::new(); ne];
        for (&(a, b), members) in &mut between {
            members.sort_by_key(|&e| (place(body.e_mid(e)), e));
            let (ta, tb) = (body.f_tag[a as usize], body.f_tag[b as usize]);
            let tags = (ta.min(tb), ta.max(tb));
            if joining[&tags].len() == 1 {
                // The only edge between these two tags.
                edges[members[0] as usize] = format!("e{:x}-{:x}", tags.0, tags.1);
                continue;
            }
            let (first, second) = if faces[a as usize] <= faces[b as usize] {
                (&faces[a as usize], &faces[b as usize])
            } else {
                (&faces[b as usize], &faces[a as usize])
            };
            for (i, &e) in members.iter().enumerate() {
                edges[e as usize] = if members.len() == 1 {
                    format!("e{first}+{second}")
                } else {
                    format!("e{first}+{second}#{i}")
                };
            }
        }
        let index = |names: &[String]| -> Map<u64, u32> {
            names
                .iter()
                .enumerate()
                .map(|(i, name)| (tag_of(name), i as u32))
                .collect()
        };
        Names {
            face_at: index(&faces),
            edge_at: index(&edges),
            faces,
            edges,
            tagged,
            between,
            joining,
        }
    }

    /// The face a name refers to, if the body still has one it can only
    /// mean. Names from before faces had tags (`face:7`) are indices.
    pub fn face(&self, name: &str) -> Option<u32> {
        if let Some(index) = name.strip_prefix("face:") {
            return index
                .parse::<u32>()
                .ok()
                .filter(|f| (*f as usize) < self.faces.len());
        }
        if let Some(&f) = self.face_at.get(&tag_of(name)) {
            if self.faces[f as usize] == name {
                return Some(f);
            }
        }
        // The exact name is gone. Its tag may still pick out one face: the
        // others that shared it were merged back, or it was cut in two and
        // one half removed.
        let members = self.tagged.get(&tag_in(name)?)?;
        if members.len() == 1 {
            return Some(members[0]);
        }
        // Several carry the tag: the one with the same neighbours, if the
        // name says which and there is exactly one.
        let ctx = name.split_once('~')?.1.split('.').next()?;
        let mut same = members.iter().filter(|&&f| {
            self.faces[f as usize]
                .split_once('~')
                .is_some_and(|(_, r)| r.split('.').next() == Some(ctx))
        });
        match (same.next(), same.next()) {
            (Some(&f), None) => Some(f),
            _ => None,
        }
    }

    /// The edge a name refers to, if the body still has one it can only
    /// mean. Names from before faces had tags (`edge:7`) are indices.
    pub fn edge(&self, name: &str) -> Option<u32> {
        match self.edges_named(name).as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    /// Every edge a name can stand for. That is one edge, unless the edge
    /// it was given to has been cut in pieces since: then it is all of
    /// them, which is what rounding "that edge" should still mean.
    pub fn edges_named(&self, name: &str) -> Vec<u32> {
        if let Some(index) = name.strip_prefix("edge:") {
            return index
                .parse::<u32>()
                .ok()
                .filter(|e| (*e as usize) < self.edges.len())
                .into_iter()
                .collect();
        }
        if let Some(&e) = self.edge_at.get(&tag_of(name)) {
            if self.edges[e as usize] == name {
                return vec![e];
            }
        }
        let Some(body) = name.strip_prefix('e') else {
            return Vec::new();
        };
        let between_tags = |ta: u64, tb: u64| -> Vec<u32> {
            self.joining
                .get(&(ta.min(tb), ta.max(tb)))
                .cloned()
                .unwrap_or_default()
        };
        let Some((first, second)) = body.split_once('+') else {
            // Named after two tags: whatever lies between them now.
            return match body.split_once('-') {
                Some((a, b)) => match (u64::from_str_radix(a, 16), u64::from_str_radix(b, 16)) {
                    (Ok(ta), Ok(tb)) => between_tags(ta, tb),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            };
        };
        // Named after two faces, with its place among the edges between
        // them if there were several.
        let (second, nth) = match second.rsplit_once('#') {
            Some((second, nth)) => (second, nth.parse::<usize>().ok()),
            None => (second, None),
        };
        if let Some((a, b)) = self.face(first).zip(self.face(second)) {
            if let Some(members) = self.between.get(&(a.min(b), a.max(b))) {
                match nth {
                    _ if members.len() == 1 => return vec![members[0]],
                    Some(nth) if nth < members.len() => return vec![members[nth]],
                    _ => {}
                }
            }
        }
        // Its faces can no longer be told from their other pieces, but it
        // may be the only edge left between their tags.
        match (tag_in(first), tag_in(second)) {
            (Some(ta), Some(tb)) => match between_tags(ta, tb).as_slice() {
                [only] => vec![*only],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }
}

use super::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

// Semantic admission bounds, not a bound on deserialization or rendering heap.
const OBJECTS: usize = 100_000;
const POINTS: usize = 1_000_000;
const TEXT_BYTES: usize = 4 * 1024 * 1024;
const COORDINATE: f64 = 1.0e12;

#[derive(Default)]
struct Budget {
    objects: usize,
    points: usize,
    text: usize,
}
fn add(value: &mut usize, count: usize, limit: usize) -> Result<(), String> {
    *value = value
        .checked_add(count)
        .filter(|n| *n <= limit)
        .ok_or("Drafting document exceeds its semantic admission budget")?;
    Ok(())
}
impl Budget {
    fn text(&mut self, value: &str, nonempty: bool) -> Result<(), String> {
        if value.len() > 65_536 || (nonempty && value.trim().is_empty()) || value.contains('\0') {
            return Err("Invalid drafting text".into());
        }
        add(&mut self.text, value.len(), TEXT_BYTES)
    }
    fn points(&mut self, points: &[[f64; 2]]) -> Result<(), String> {
        add(&mut self.points, points.len(), POINTS)?;
        if points.iter().flatten().any(|x| !coordinate(*x)) {
            return Err("Invalid drafting coordinate".into());
        }
        Ok(())
    }
}
fn coordinate(x: f64) -> bool {
    x.is_finite() && x.abs() <= COORDINATE
}
fn positive(x: f64) -> bool {
    coordinate(x) && x > 0.
}
fn transform(t: &Affine) -> Result<(), String> {
    let [a, b, c, d, _, _] = t.0;
    let determinant = a * d - b * c;
    if t.0.iter().any(|x| !coordinate(*x)) || !determinant.is_finite() || determinant == 0. {
        return Err("Drafting transform must be finite and nonsingular".into());
    }
    Ok(())
}
fn sweep(start: f64, sweep: f64) -> bool {
    start.is_finite() && sweep.is_finite() && sweep != 0. && sweep.abs() <= std::f64::consts::TAU
}
fn curve(curve: &Curve, budget: &mut Budget) -> Result<(), String> {
    let valid = match curve {
        Curve::Line { start, end } => {
            budget.points(&[*start, *end])?;
            start != end
        }
        Curve::Arc {
            center,
            radius,
            start_rad,
            sweep_rad,
        } => {
            budget.points(&[*center])?;
            positive(*radius) && sweep(*start_rad, *sweep_rad)
        }
        Curve::Ellipse {
            center,
            axis_u,
            axis_v,
            start_rad,
            sweep_rad,
        } => {
            budget.points(&[*center, *axis_u, *axis_v])?;
            let determinant = axis_u[0] * axis_v[1] - axis_u[1] * axis_v[0];
            determinant.is_finite() && determinant != 0. && sweep(*start_rad, *sweep_rad)
        }
        Curve::Polyline { points, closed } => {
            budget.points(points)?;
            let minimum = if *closed { 3 } else { 2 };
            points.len() >= minimum && points.windows(2).all(|pair| pair[0] != pair[1])
        }
        Curve::Nurbs {
            degree,
            control_points,
            weights,
            knots,
        } => {
            budget.points(control_points)?;
            add(&mut budget.points, weights.len(), POINTS)?;
            add(&mut budget.points, knots.len(), POINTS)?;
            let degree = *degree as usize;
            let n = control_points.len();
            let cardinality = degree > 0
                && degree <= 32
                && n > degree
                && weights.len() == n
                && knots.len() == n + degree + 1;
            let mut multiplicity = 1;
            let knots_valid = knots.iter().all(|x| x.is_finite())
                && knots.windows(2).all(|pair| {
                    multiplicity = if pair[0] == pair[1] {
                        multiplicity + 1
                    } else {
                        1
                    };
                    pair[0] <= pair[1] && multiplicity <= degree + 1
                });
            cardinality
                && knots_valid
                && weights.iter().all(|x| positive(*x))
                && knots[degree] < knots[n]
        }
    };
    if valid {
        Ok(())
    } else {
        Err("Invalid exact drafting curve".into())
    }
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Node {
    Block(BlockId),
    Reference(ReferenceId),
}

impl Document {
    /// Checks a complete candidate without changing it or resolving external files.
    /// Callers must separately enforce mutation locks, ownership, resource-bounded
    /// decoding and revision fences. This is not an edit or reload operation.
    pub fn validate(&self, sheet_ids: &[u64]) -> Result<(), String> {
        if self.version != 1 {
            return Err("Unsupported drafting contract version".into());
        }
        let mut budget = Budget::default();
        for count in [
            self.layers.len(),
            self.line_types.len(),
            self.blocks.len(),
            self.references.len(),
            self.entities.len(),
            self.viewports.len(),
            sheet_ids.len(),
        ] {
            add(&mut budget.objects, count, OBJECTS)?;
        }
        let sheets: BTreeSet<_> = sheet_ids.iter().copied().collect();
        if sheets.len() != sheet_ids.len() || sheets.contains(&0) {
            return Err("Invalid owning drawing sheet IDs".into());
        }
        let mut ids = BTreeSet::new();
        let mut claim = |id: u64| -> Result<(), String> {
            if id == 0 || id >= self.next_id || !ids.insert(id) {
                Err("Drafting IDs must be unique, nonzero and below next_id".into())
            } else {
                Ok(())
            }
        };
        if self.next_id == 0 {
            return Err("Invalid drafting ID allocator".into());
        }
        let line_types: BTreeSet<_> = self.line_types.iter().map(|x| x.id).collect();
        let layers: BTreeSet<_> = self.layers.iter().map(|x| x.id).collect();
        let blocks: BTreeMap<_, _> = self.blocks.iter().map(|x| (x.id, x)).collect();
        let references: BTreeSet<_> = self.references.iter().map(|x| x.id).collect();
        let style = |s: &Style| -> Result<(), String> {
            if !positive(s.width_mm) || s.line_type.is_some_and(|id| !line_types.contains(&id)) {
                Err("Drafting style references an invalid width or line type".into())
            } else {
                Ok(())
            }
        };
        let mut names = BTreeSet::new();
        for line in &self.line_types {
            claim(line.id.0)?;
            budget.text(&line.name, true)?;
            add(&mut budget.points, line.pattern.len(), POINTS)?;
            if !names.insert(&line.name)
                || line.pattern.is_empty()
                || line.pattern.iter().any(|x| !coordinate(*x))
                || !line.pattern.iter().any(|x| *x != 0.)
            {
                return Err("Invalid or duplicate drafting line type".into());
            }
        }
        names.clear();
        for layer in &self.layers {
            claim(layer.id.0)?;
            budget.text(&layer.name, true)?;
            style(&layer.style)?;
            if !names.insert(&layer.name) {
                return Err("Duplicate drafting layer name".into());
            }
        }
        names.clear();
        let mut nodes = BTreeSet::new();
        let mut edges = BTreeSet::new();
        let mut attributes = BTreeMap::new();
        for block in &self.blocks {
            claim(block.id.0)?;
            budget.text(&block.name, true)?;
            budget.points(&[block.base_point])?;
            if !names.insert(&block.name) {
                return Err("Duplicate drafting block name".into());
            }
            nodes.insert(Node::Block(block.id));
            add(&mut budget.objects, block.attributes.len(), OBJECTS)?;
            let mut tags = BTreeSet::new();
            for attribute in &block.attributes {
                claim(attribute.id.0)?;
                budget.text(&attribute.tag, true)?;
                budget.text(&attribute.default_value, false)?;
                if !tags.insert(&attribute.tag) {
                    return Err("Duplicate block attribute tag".into());
                }
                attributes.insert(attribute.id, block.id);
            }
        }
        for reference in &self.references {
            claim(reference.id.0)?;
            budget.text(&reference.locator, true)?;
            if !positive(reference.source_mm_per_unit) {
                return Err("Invalid reference units".into());
            }
            nodes.insert(Node::Reference(reference.id));
            let resolved = match &reference.resolution {
                Resolution::Unresolved { diagnostic } => {
                    budget.text(diagnostic, true)?;
                    None
                }
                Resolution::Resolved { block, sha256 } => Some((block, sha256)),
                Resolution::Stale {
                    block,
                    sha256,
                    diagnostic,
                } => {
                    budget.text(diagnostic, true)?;
                    Some((block, sha256))
                }
            };
            if let Some((block, sha)) = resolved {
                if !blocks.contains_key(block)
                    || sha.len() != 64
                    || !sha.bytes().all(|x| x.is_ascii_hexdigit())
                {
                    return Err("Invalid resolved drafting reference".into());
                }
                edges.insert((Node::Reference(reference.id), Node::Block(*block)));
            }
        }
        for entity in &self.entities {
            claim(entity.id.0)?;
            if !layers.contains(&entity.layer) {
                return Err("Drafting entity layer is missing".into());
            }
            if let Some(s) = &entity.style {
                style(s)?;
            }
            let parent = match entity.space {
                Space::Model => None,
                Space::Paper(id) => {
                    if !sheets.contains(&id) {
                        return Err("Drafting entity sheet is missing".into());
                    }
                    None
                }
                Space::Block(id) => {
                    if !blocks.contains_key(&id) {
                        return Err("Drafting entity block is missing".into());
                    }
                    Some(Node::Block(id))
                }
            };
            let target = match &entity.content {
                Content::Curve { curve: c } => {
                    curve(c, &mut budget)?;
                    None
                }
                Content::Text {
                    position,
                    height_mm,
                    rotation_rad,
                    text,
                } => {
                    budget.points(&[*position])?;
                    budget.text(text, false)?;
                    if !positive(*height_mm) || !rotation_rad.is_finite() {
                        return Err("Invalid drafting text placement".into());
                    }
                    None
                }
                Content::Block {
                    block,
                    transform: t,
                    attributes: values,
                } => {
                    transform(t)?;
                    if !blocks.contains_key(block) {
                        return Err("Missing drafting block definition".into());
                    }
                    add(&mut budget.objects, values.len(), OBJECTS)?;
                    let mut seen = BTreeSet::new();
                    for value in values {
                        budget.text(&value.value, false)?;
                        if attributes.get(&value.attribute) != Some(block)
                            || !seen.insert(value.attribute)
                        {
                            return Err("Invalid block instance attribute override".into());
                        }
                    }
                    Some(Node::Block(*block))
                }
                Content::Reference {
                    reference,
                    transform: t,
                } => {
                    transform(t)?;
                    if !references.contains(reference) {
                        return Err("Missing drafting reference".into());
                    }
                    Some(Node::Reference(*reference))
                }
            };
            if let (Some(parent), Some(target)) = (parent, target) {
                edges.insert((parent, target));
            }
        }
        // Iterative topological traversal: deeply nested input cannot overflow a
        // recursive validator. Repeated instances share a dependency edge.
        let mut indegree: BTreeMap<_, usize> = nodes.iter().map(|n| (*n, 0)).collect();
        let mut outgoing: BTreeMap<Node, Vec<Node>> = BTreeMap::new();
        for (from, to) in edges {
            *indegree.get_mut(&to).ok_or("Missing drafting dependency")? += 1;
            outgoing.entry(from).or_default().push(to);
        }
        let mut ready: VecDeque<_> = indegree
            .iter()
            .filter_map(|(n, d)| (*d == 0).then_some(*n))
            .collect();
        let mut visited = 0;
        while let Some(node) = ready.pop_front() {
            visited += 1;
            if let Some(children) = outgoing.get(&node) {
                for child in children {
                    let degree = indegree
                        .get_mut(child)
                        .ok_or("Missing drafting dependency")?;
                    *degree -= 1;
                    if *degree == 0 {
                        ready.push_back(*child);
                    }
                }
            }
        }
        if visited != nodes.len() {
            return Err("Cyclic drafting block/reference dependency".into());
        }
        for viewport in &self.viewports {
            claim(viewport.id.0)?;
            budget.points(&[viewport.model_center_mm, viewport.paper_center_mm])?;
            add(&mut budget.objects, viewport.hidden_layers.len(), OBJECTS)?;
            let hidden: BTreeSet<_> = viewport.hidden_layers.iter().copied().collect();
            if !sheets.contains(&viewport.sheet_id)
                || !viewport.rotation_rad.is_finite()
                || !positive(viewport.paper_mm_per_model_mm)
                || viewport.clip_mm.iter().any(|x| !coordinate(*x))
                || !positive(viewport.clip_mm[2])
                || !positive(viewport.clip_mm[3])
                || hidden.len() != viewport.hidden_layers.len()
                || !hidden.is_subset(&layers)
            {
                return Err("Invalid drafting paper viewport".into());
            }
        }
        Ok(())
    }
}

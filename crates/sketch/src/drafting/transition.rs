//! Edit policy over already validated authored graphs. Project loading/history
//! replacement remain separate operations; they do not impersonate user edits.
use super::{validate::Node, *};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Copy, PartialEq, Eq)]
enum IdentityKind {
    Entity,
    Layer,
    LineType,
    Block,
    Reference,
    Viewport,
    Attribute(BlockId),
}
fn identities(document: &Document) -> BTreeMap<u64, IdentityKind> {
    let mut ids = BTreeMap::new();
    ids.extend(
        document
            .entities
            .iter()
            .map(|x| (x.id.0, IdentityKind::Entity)),
    );
    ids.extend(
        document
            .layers
            .iter()
            .map(|x| (x.id.0, IdentityKind::Layer)),
    );
    ids.extend(
        document
            .line_types
            .iter()
            .map(|x| (x.id.0, IdentityKind::LineType)),
    );
    ids.extend(
        document
            .references
            .iter()
            .map(|x| (x.id.0, IdentityKind::Reference)),
    );
    ids.extend(
        document
            .viewports
            .iter()
            .map(|x| (x.id.0, IdentityKind::Viewport)),
    );
    for block in &document.blocks {
        ids.insert(block.id.0, IdentityKind::Block);
        ids.extend(
            block
                .attributes
                .iter()
                .map(|a| (a.id.0, IdentityKind::Attribute(block.id))),
        );
    }
    ids
}

/// Ordinary edits preserve the allocator high-water mark and object identity.
/// Both graphs must already pass semantic validation. Deliberate project/history
/// replacement restores an entire document incarnation through its separate path.
pub(crate) fn validate_edit(before: &Document, after: &Document) -> Result<(), String> {
    if after.next_id < before.next_id {
        return Err("Drafting edits cannot move the ID allocator backward".into());
    }
    let previous = identities(before);
    for (id, kind) in identities(after) {
        match previous.get(&id) {
            Some(old) if *old != kind => {
                return Err("Drafting IDs cannot change object kind or attribute owner".into())
            }
            None if id < before.next_id => {
                return Err(
                    "New drafting objects require fresh IDs; retired IDs cannot be reused".into(),
                )
            }
            _ => {}
        }
    }
    validate_layer_edit(before, after)
}

struct Index<'a> {
    entities: BTreeMap<EntityId, &'a Entity>,
    blocks: BTreeMap<BlockId, &'a Block>,
    references: BTreeMap<ReferenceId, &'a Reference>,
    layers: BTreeMap<LayerId, &'a Layer>,
    lines: BTreeMap<LineTypeId, &'a LineType>,
}
impl<'a> Index<'a> {
    fn new(document: &'a Document) -> Self {
        Self {
            entities: document.entities.iter().map(|x| (x.id, x)).collect(),
            blocks: document.blocks.iter().map(|x| (x.id, x)).collect(),
            references: document.references.iter().map(|x| (x.id, x)).collect(),
            layers: document.layers.iter().map(|x| (x.id, x)).collect(),
            lines: document.line_types.iter().map(|x| (x.id, x)).collect(),
        }
    }
    fn style(&self, entity: &'a Entity) -> Option<(&'a Style, Option<&'a LineType>)> {
        let style = entity
            .style
            .as_ref()
            .or_else(|| self.layers.get(&entity.layer).copied().map(|l| &l.style))?;
        Some((
            style,
            style.line_type.and_then(|id| self.lines.get(&id).copied()),
        ))
    }
}
fn target(content: &Content) -> Option<Node> {
    match content {
        Content::Block { block, .. } => Some(Node::Block(*block)),
        Content::Reference { reference, .. } => Some(Node::Reference(*reference)),
        _ => None,
    }
}
fn reverse_dependencies(document: &Document, reverse: &mut BTreeMap<Node, BTreeSet<Node>>) {
    for entity in &document.entities {
        if let (Space::Block(parent), Some(child)) = (entity.space, target(&entity.content)) {
            reverse
                .entry(child)
                .or_default()
                .insert(Node::Block(parent));
        }
    }
    for reference in &document.references {
        let block = match &reference.resolution {
            Resolution::Resolved { block, .. } | Resolution::Stale { block, .. } => block,
            Resolution::Unresolved { .. } => continue,
        };
        reverse
            .entry(Node::Block(*block))
            .or_default()
            .insert(Node::Reference(reference.id));
    }
}

/// Before and after must pass Document::validate before calling this policy.
/// Unlocking is permitted, but it must be a prior committed edit: combining an
/// unlock and a protected content change in one replacement remains rejected.
pub(crate) fn validate_layer_edit(before: &Document, after: &Document) -> Result<(), String> {
    let locked: BTreeSet<_> = before
        .layers
        .iter()
        .filter(|l| l.locked)
        .map(|l| l.id)
        .collect();
    if locked.is_empty() {
        return Ok(());
    }
    let old = Index::new(before);
    let new = Index::new(after);
    // Keep locks explicit. Deleting a locked layer is not an implicit unlock.
    if locked.iter().any(|id| !new.layers.contains_key(id)) {
        return Err("Unlock the drafting layer before deleting it".into());
    }
    let mut changed = BTreeSet::new();
    for id in old.blocks.keys().chain(new.blocks.keys()) {
        if old.blocks.get(id) != new.blocks.get(id) {
            changed.insert(Node::Block(*id));
        }
    }
    for id in old.references.keys().chain(new.references.keys()) {
        if old.references.get(id) != new.references.get(id) {
            changed.insert(Node::Reference(*id));
        }
    }
    for id in old.entities.keys().chain(new.entities.keys()) {
        let prior = old.entities.get(id).copied();
        let next = new.entities.get(id).copied();
        let different =
            prior != next || prior.and_then(|e| old.style(e)) != next.and_then(|e| new.style(e));
        if !different {
            continue;
        }
        for entity in [prior, next].into_iter().flatten() {
            if locked.contains(&entity.layer) {
                return Err("Unlock the drafting layer before changing its entities".into());
            }
            if let Space::Block(id) = entity.space {
                changed.insert(Node::Block(id));
            }
        }
    }
    // Propagate definition changes through both graphs, including changed or
    // removed edges. No geometry expansion and no recursive call stack.
    let mut reverse = BTreeMap::new();
    reverse_dependencies(before, &mut reverse);
    reverse_dependencies(after, &mut reverse);
    let mut pending: VecDeque<_> = changed.iter().copied().collect();
    while let Some(node) = pending.pop_front() {
        if let Some(parents) = reverse.get(&node) {
            for parent in parents {
                if changed.insert(*parent) {
                    pending.push_back(*parent);
                }
            }
        }
    }
    for entity in &before.entities {
        if locked.contains(&entity.layer)
            && target(&entity.content).is_some_and(|node| changed.contains(&node))
        {
            return Err(
                "Unlock the drafting instance layer before changing its referenced definition"
                    .into(),
            );
        }
    }
    Ok(())
}

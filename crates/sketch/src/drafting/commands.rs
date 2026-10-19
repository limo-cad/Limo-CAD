//! Typed edits shared by hosts. IDs are allocated only on a staged candidate;
//! semantic, identity and layer-lock checks run before the manager commits it.
use super::*;
use crate::{session::SessionError, DrawingDocumentDto, SketchManager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewAttribute {
    pub tag: String,
    pub default_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "arguments",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Command {
    CreateLayer {
        name: String,
        visible: bool,
        locked: bool,
        plottable: bool,
        style: Style,
    },
    UpdateLayer(Layer),
    DeleteLayer {
        id: LayerId,
    },
    CreateLineType {
        name: String,
        pattern: Vec<f64>,
        space: PatternSpace,
    },
    UpdateLineType(LineType),
    DeleteLineType {
        id: LineTypeId,
    },
    CreateBlock {
        name: String,
        base_point: [f64; 2],
        attributes: Vec<NewAttribute>,
    },
    UpdateBlock {
        id: BlockId,
        name: String,
        base_point: [f64; 2],
    },
    DeleteBlock {
        id: BlockId,
    },
    AddAttribute {
        block: BlockId,
        attribute: NewAttribute,
    },
    UpdateAttribute {
        block: BlockId,
        attribute: Attribute,
    },
    DeleteAttribute {
        block: BlockId,
        id: AttributeId,
    },
    CreateEntity {
        space: Space,
        layer: LayerId,
        style: Option<Style>,
        content: Content,
    },
    UpdateEntity(Entity),
    DeleteEntity {
        id: EntityId,
    },
    CreateViewport {
        sheet_id: u64,
        model_center_mm: [f64; 2],
        paper_center_mm: [f64; 2],
        rotation_rad: f64,
        paper_mm_per_model_mm: f64,
        clip_mm: [f64; 4],
        hidden_layers: Vec<LayerId>,
    },
    UpdateViewport(Viewport),
    DeleteViewport {
        id: ViewportId,
    },
    /// Registers unresolved intent only. It cannot assert a successful reload or
    /// access the filesystem; a future resolver must stage retained source data.
    AttachReference {
        locator: String,
        mode: ReferenceMode,
        source_mm_per_unit: f64,
    },
    SetReferenceMode {
        id: ReferenceId,
        mode: ReferenceMode,
    },
    DetachReference {
        id: ReferenceId,
    },
}

impl SketchManager {
    pub fn drafting_command(
        &mut self,
        command: Command,
    ) -> Result<DrawingDocumentDto, SessionError> {
        if self.has_active_sketch() {
            return Err(SessionError::Solid(
                "Finish the active sketch before editing drafting content".into(),
            ));
        }
        let mut drawing = self.drawing_document();
        let graph = drawing.drafting.get_or_insert_with(Document::default);
        apply(graph, command).map_err(SessionError::Solid)?;
        self.set_drawing_document(drawing)
    }
}
fn allocate(document: &mut Document) -> Result<u64, String> {
    let id = document.next_id;
    document.next_id = id.checked_add(1).ok_or("Drafting object IDs exhausted")?;
    Ok(id)
}
fn target<T>(items: &mut [T], id: u64, key: impl Fn(&T) -> u64) -> Result<&mut T, String> {
    items
        .iter_mut()
        .find(|x| key(x) == id)
        .ok_or_else(|| format!("Drafting target {id} does not exist"))
}
fn replace<T>(items: &mut [T], value: T, key: impl Fn(&T) -> u64) -> Result<(), String> {
    let id = key(&value);
    *target(items, id, key)? = value;
    Ok(())
}
fn remove<T>(items: &mut Vec<T>, id: u64, key: impl Fn(&T) -> u64) -> Result<(), String> {
    let position = items
        .iter()
        .position(|x| key(x) == id)
        .ok_or_else(|| format!("Drafting target {id} does not exist"))?;
    items.remove(position);
    Ok(())
}
fn apply(d: &mut Document, command: Command) -> Result<(), String> {
    match command {
        Command::CreateLayer {
            name,
            visible,
            locked,
            plottable,
            style,
        } => {
            let id = LayerId(allocate(d)?);
            d.layers.push(Layer {
                id,
                name,
                visible,
                locked,
                plottable,
                style,
            });
        }
        Command::UpdateLayer(value) => replace(&mut d.layers, value, |x| x.id.0)?,
        Command::DeleteLayer { id } => remove(&mut d.layers, id.0, |x| x.id.0)?,
        Command::CreateLineType {
            name,
            pattern,
            space,
        } => {
            let id = LineTypeId(allocate(d)?);
            d.line_types.push(LineType {
                id,
                name,
                pattern,
                space,
            });
        }
        Command::UpdateLineType(value) => replace(&mut d.line_types, value, |x| x.id.0)?,
        Command::DeleteLineType { id } => remove(&mut d.line_types, id.0, |x| x.id.0)?,
        Command::CreateBlock {
            name,
            base_point,
            attributes,
        } => {
            if attributes.len() > 100_000 {
                return Err("Too many drafting attributes".into());
            }
            let id = BlockId(allocate(d)?);
            let mut assigned = Vec::with_capacity(attributes.len());
            for a in attributes {
                assigned.push(Attribute {
                    id: AttributeId(allocate(d)?),
                    tag: a.tag,
                    default_value: a.default_value,
                });
            }
            d.blocks.push(Block {
                id,
                name,
                base_point,
                attributes: assigned,
            });
        }
        Command::UpdateBlock {
            id,
            name,
            base_point,
        } => {
            let block = target(&mut d.blocks, id.0, |x| x.id.0)?;
            block.name = name;
            block.base_point = base_point;
        }
        Command::DeleteBlock { id } => remove(&mut d.blocks, id.0, |x| x.id.0)?,
        Command::AddAttribute { block, attribute } => {
            let id = AttributeId(allocate(d)?);
            target(&mut d.blocks, block.0, |x| x.id.0)?
                .attributes
                .push(Attribute {
                    id,
                    tag: attribute.tag,
                    default_value: attribute.default_value,
                });
        }
        Command::UpdateAttribute { block, attribute } => {
            replace(
                &mut target(&mut d.blocks, block.0, |x| x.id.0)?.attributes,
                attribute,
                |x| x.id.0,
            )?;
        }
        Command::DeleteAttribute { block, id } => {
            remove(
                &mut target(&mut d.blocks, block.0, |x| x.id.0)?.attributes,
                id.0,
                |x| x.id.0,
            )?;
        }
        Command::CreateEntity {
            space,
            layer,
            style,
            content,
        } => {
            let id = EntityId(allocate(d)?);
            d.entities.push(Entity {
                id,
                space,
                layer,
                style,
                content,
            });
        }
        Command::UpdateEntity(value) => replace(&mut d.entities, value, |x| x.id.0)?,
        Command::DeleteEntity { id } => remove(&mut d.entities, id.0, |x| x.id.0)?,
        Command::CreateViewport {
            sheet_id,
            model_center_mm,
            paper_center_mm,
            rotation_rad,
            paper_mm_per_model_mm,
            clip_mm,
            hidden_layers,
        } => {
            let id = ViewportId(allocate(d)?);
            d.viewports.push(Viewport {
                id,
                sheet_id,
                model_center_mm,
                paper_center_mm,
                rotation_rad,
                paper_mm_per_model_mm,
                clip_mm,
                hidden_layers,
            });
        }
        Command::UpdateViewport(value) => replace(&mut d.viewports, value, |x| x.id.0)?,
        Command::DeleteViewport { id } => remove(&mut d.viewports, id.0, |x| x.id.0)?,
        Command::AttachReference {
            locator,
            mode,
            source_mm_per_unit,
        } => {
            let id = ReferenceId(allocate(d)?);
            d.references.push(Reference {
                id,
                locator,
                mode,
                source_mm_per_unit,
                resolution: Resolution::Unresolved {
                    diagnostic: "Reference has not been loaded".into(),
                },
            });
        }
        Command::SetReferenceMode { id, mode } => {
            target(&mut d.references, id.0, |x| x.id.0)?.mode = mode
        }
        Command::DetachReference { id } => remove(&mut d.references, id.0, |x| x.id.0)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests;

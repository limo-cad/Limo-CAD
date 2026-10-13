//! Immutable views over the existing central-library JSON snapshot. Canonical
//! tools feed the existing form; the original object retains extension metadata.
use limo_cad_cam::{CamDocumentDto, CamToolDto, CamUnits};
use serde_json::{json, Value};

const MAX_COUNTER: u64 = 9_007_199_254_740_991;

pub(super) struct Snapshot {
    pub(super) path: String,
    pub(super) revision: String,
    value: Value,
    pub(super) tools: Vec<CamToolDto>,
}

pub(super) struct Edit {
    pub(super) json: String,
    pub(super) tool: Option<CamToolDto>,
}

impl Snapshot {
    pub(super) fn new(source: crate::cam_library::Snapshot) -> Result<Self, String> {
        let json = source
            .json
            .unwrap_or_else(|| "{\"next_tool_id\":1,\"tools\":[]}".into());
        crate::cam_library::validate_library(&json)?;
        let value: Value = serde_json::from_str(&json).map_err(|error| error.to_string())?;
        let tools =
            serde_json::from_value(value["tools"].clone()).map_err(|error| error.to_string())?;
        Ok(Self {
            path: source.path,
            revision: source.revision,
            value,
            tools,
        })
    }

    /// A form adapter only: central entries may share machine-facing numbers.
    /// Storage validation checks each cutter independently, unlike a project.
    pub(super) fn form_document(&self, units: CamUnits) -> CamDocumentDto {
        CamDocumentDto {
            units,
            next_tool_id: self.value["next_tool_id"].as_u64().unwrap(),
            tools: self.tools.clone(),
            ..Default::default()
        }
    }

    fn finish(&self, value: Value, tool: Option<CamToolDto>) -> Result<Edit, String> {
        let json = serde_json::to_string(&value).map_err(|error| error.to_string())?;
        crate::cam_library::validate_library(&json)?;
        Ok(Edit { json, tool })
    }

    fn index(&self, id: u64) -> Result<usize, String> {
        self.tools
            .iter()
            .position(|tool| tool.id == id)
            .ok_or("The central tool was removed".into())
    }

    /// Edit a known row without dropping metadata owned by a newer library.
    pub(super) fn update(&self, tool: CamToolDto) -> Result<Edit, String> {
        let index = self.index(tool.id)?;
        let mut value = self.value.clone();
        merge_tool(&mut value["tools"][index], &tool)?;
        self.finish(value, Some(tool))
    }

    /// An explicit duplicate retains unknown source-row metadata. New entries
    /// have only their canonical tool data. IDs belong to this collection.
    pub(super) fn add(
        &self,
        mut tool: CamToolDto,
        copied_from: Option<u64>,
    ) -> Result<Edit, String> {
        let id = self.value["next_tool_id"]
            .as_u64()
            .unwrap()
            .max(self.tools.iter().map(|tool| tool.id + 1).max().unwrap_or(1));
        if id >= MAX_COUNTER {
            return Err("The central tool library has exhausted its identities".into());
        }
        tool.id = id;
        let mut row = copied_from
            .map(|id| {
                self.index(id)
                    .map(|index| self.value["tools"][index].clone())
            })
            .transpose()?
            .unwrap_or(json!({}));
        merge_tool(&mut row, &tool)?;
        let mut value = self.value.clone();
        value["tools"].as_array_mut().unwrap().push(row);
        sort_tools(&mut value);
        value["next_tool_id"] = json!(id + 1);
        self.finish(value, Some(tool))
    }

    pub(super) fn remove(&self, id: u64) -> Result<Edit, String> {
        let index = self.index(id)?;
        let mut value = self.value.clone();
        value["tools"].as_array_mut().unwrap().remove(index);
        self.finish(value, None)
    }

    /// Publishing replaces the snapshot while preserving its ID.
    pub(super) fn publish(&self, tool: CamToolDto) -> Result<Edit, String> {
        if tool.id == 0 || tool.id >= MAX_COUNTER {
            return Err("The project tool has an invalid library identity".into());
        }
        let mut value = self.value.clone();
        let row = serde_json::to_value(&tool).map_err(|error| error.to_string())?;
        if let Ok(index) = self.index(tool.id) {
            value["tools"][index] = row;
        } else {
            value["tools"].as_array_mut().unwrap().push(row);
        }
        sort_tools(&mut value);
        value["next_tool_id"] = json!(self.value["next_tool_id"]
            .as_u64()
            .unwrap()
            .max(tool.id + 1));
        self.finish(value, Some(tool))
    }

    pub(super) fn import(
        &self,
        project: &CamDocumentDto,
        id: u64,
    ) -> Result<CamDocumentDto, String> {
        let tool = self.tools[self.index(id)?].clone();
        import(project, tool)
    }
}

fn merge_tool(row: &mut Value, tool: &CamToolDto) -> Result<(), String> {
    let fields = serde_json::to_value(tool).map_err(|error| error.to_string())?;
    let row = row
        .as_object_mut()
        .ok_or("Central tool must be an object")?;
    row.remove("corner_chamfer");
    row.extend(
        fields
            .as_object()
            .ok_or("Could not serialize the central tool")?
            .clone(),
    );
    Ok(())
}

fn sort_tools(value: &mut Value) {
    value["tools"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|tool| tool["id"].as_u64().unwrap());
}

pub(super) fn import(project: &CamDocumentDto, tool: CamToolDto) -> Result<CamDocumentDto, String> {
    let mut next = project.clone();
    let id = tool.id;
    if let Some(existing) = next.tools.iter_mut().find(|entry| entry.id == id) {
        *existing = tool;
    } else {
        next.tools.push(tool);
    }
    next.tools.sort_by_key(|entry| entry.id);
    next.next_tool_id = next
        .next_tool_id
        .max(id.checked_add(1).ok_or("CAM identities exhausted")?);
    next.validate_for_editing()?;
    Ok(next)
}

/// Match desktop creation: the library allocates first, then a project that
/// already holds that ID advances locally without rewriting either snapshot.
pub(super) fn import_created(
    project: &CamDocumentDto,
    mut tool: CamToolDto,
) -> Result<CamDocumentDto, String> {
    while project.tools.iter().any(|entry| entry.id == tool.id) {
        tool.id = tool.id.checked_add(1).ok_or("CAM identities exhausted")?;
    }
    import(project, tool)
}

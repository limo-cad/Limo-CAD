//! Row edits preserve shared IDs, issue receipts and balloon references.
use super::model::{return_released_sheets_to_draft, Kind, Selection};
use limo_cad_sketch::*;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Table {
    Revisions,
    Bom,
}
const STATUS: &[(&str, &str)] = &[
    ("draft", "Draft"),
    ("in_review", "In review"),
    ("released", "Released"),
    ("superseded", "Superseded"),
    ("obsolete", "Obsolete"),
];
pub(super) fn sheet(document: &DrawingDocumentDto, id: u64) -> Result<&DrawingSheetDto, String> {
    document
        .sheets
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| "Sheet was removed".into())
}
pub(super) fn context(selection: Selection) -> Option<(u64, Table)> {
    match selection {
        Selection::Table(id, table)
        | Selection::NewRow(id, table)
        | Selection::Row(id, table, _) => Some((id, table)),
        _ => None,
    }
}
pub(super) fn record(document: &DrawingDocumentDto, selection: Selection) -> Result<Value, String> {
    let (id, table) = context(selection).ok_or("Select a drawing table")?;
    let sheet = sheet(document, id)?;
    Ok(match selection {
        Selection::Table(..) => {
            let position = match table {
                Table::Revisions => sheet.revision_table_position,
                Table::Bom => sheet.bom_table_position,
            };
            let [x, y] = position.unwrap_or(default_position(table));
            json!({"visibility":if position.is_some() {"shown"} else {"hidden"}, "x":x,"y":y})
        }
        Selection::NewRow(..) => {
            json!({"sheet":sheet,"counter":match table { Table::Revisions => document.next_revision_id, Table::Bom => document.next_bom_item_id }})
        }
        Selection::Row(_, Table::Revisions, row) => serde_json::to_value(
            sheet
                .revisions
                .iter()
                .find(|r| r.id == row)
                .ok_or("Revision was removed")?,
        )
        .map_err(|e| e.to_string())?,
        Selection::Row(_, Table::Bom, row) => serde_json::to_value(
            sheet
                .bom
                .iter()
                .find(|r| r.id == row)
                .ok_or("BOM item was removed")?,
        )
        .map_err(|e| e.to_string())?,
        _ => unreachable!(),
    })
}
pub(super) fn seed(
    document: &DrawingDocumentDto,
    selection: Selection,
    source: &Value,
) -> Result<Value, String> {
    let Selection::NewRow(id, table) = selection else {
        return Ok(source.clone());
    };
    let sheet = sheet(document, id)?;
    Ok(match table {
        Table::Revisions => json!({
            "id":document.next_revision_id,"revision":sheet.title_block.revision,
            "description":"","date":time::OffsetDateTime::now_utc().date().to_string(),
            "author":sheet.title_block.author,"checked_by":sheet.title_block.checked_by,
            "approved_by":sheet.title_block.approved_by,"change_order":"","status":"draft"
        }),
        Table::Bom => {
            json!({"id":document.next_bom_item_id,"item_number":(sheet.bom.len()+1).to_string(),
            "body_id":null,"part_number":"","description":format!("Item {}",sheet.bom.len()+1),
            "quantity":1.,"material":"","finish":""})
        }
    })
}
pub(super) fn descriptors(selection: Selection) -> Vec<(&'static str, &'static str, Kind)> {
    match selection {
        Selection::Table(..) => vec![
            (
                "/visibility",
                "Show table",
                Kind::Choice(&[("shown", "Shown"), ("hidden", "Hidden")]),
            ),
            ("/x", "Table X (mm)", Kind::Number),
            ("/y", "Table Y (mm)", Kind::Number),
        ],
        Selection::Row(_, Table::Revisions, _) | Selection::NewRow(_, Table::Revisions) => vec![
            ("/revision", "Revision code", Kind::Text),
            ("/description", "Description", Kind::Text),
            ("/date", "Date", Kind::Text),
            ("/status", "Status", Kind::Choice(STATUS)),
            ("/author", "Drawn by", Kind::Text),
            ("/checked_by", "Checked by", Kind::Text),
            ("/approved_by", "Approved by", Kind::Text),
            ("/change_order", "Change order", Kind::Text),
        ],
        Selection::Row(_, Table::Bom, _) | Selection::NewRow(_, Table::Bom) => vec![
            ("/item_number", "Item number", Kind::Text),
            ("/part_number", "Part number", Kind::Text),
            ("/description", "Description", Kind::Text),
            ("/body_id", "Body", Kind::Body),
            ("/quantity", "Quantity", Kind::Number),
            ("/material", "Material", Kind::Text),
            ("/finish", "Finish", Kind::Text),
        ],
        _ => unreachable!(),
    }
}
pub(super) fn immutable(selection: Selection, row: &Value) -> bool {
    matches!(selection, Selection::Row(_, Table::Revisions, _)) && row["status"] == "released"
}
pub(super) fn default_position(table: Table) -> [f64; 2] {
    match table {
        Table::Revisions => [10., 10.],
        Table::Bom => [10., 45.],
    }
}
pub(super) fn apply(
    document: &DrawingDocumentDto,
    selection: Selection,
    edited: Value,
) -> Result<DrawingDocumentDto, String> {
    let (id, table) = context(selection).ok_or("Select a drawing table")?;
    let mut next = document.clone();
    let new_row = matches!(selection, Selection::NewRow(..));
    if new_row {
        let counter = match table {
            Table::Revisions => &mut next.next_revision_id,
            Table::Bom => &mut next.next_bom_item_id,
        };
        if edited["id"].as_u64() != Some(*counter) {
            return Err("Drawing counter changed; reset the form".into());
        }
        *counter = counter
            .checked_add(1)
            .ok_or("Drawing row IDs are exhausted")?;
    }
    let sheet = next
        .sheets
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or("Sheet was removed")?;
    let mut issued = false;
    match selection {
        Selection::Table(..) => {
            let position = if edited["visibility"] == "shown" {
                Some([
                    edited["x"].as_f64().ok_or("Enter table X")?,
                    edited["y"].as_f64().ok_or("Enter table Y")?,
                ])
            } else {
                None
            };
            match table {
                Table::Revisions => sheet.revision_table_position = position,
                Table::Bom => sheet.bom_table_position = position,
            }
        }
        Selection::Row(_, Table::Revisions, _) | Selection::NewRow(_, Table::Revisions) => {
            let entry: DrawingRevisionDto =
                serde_json::from_value(edited).map_err(|e| e.to_string())?;
            if new_row {
                sheet.revisions.push(entry.clone());
            } else {
                let row = sheet
                    .revisions
                    .iter_mut()
                    .find(|r| r.id == entry.id)
                    .ok_or("Revision was removed")?;
                if row.status == DrawingReleaseStatus::Released {
                    return Err("Released revisions cannot be edited".into());
                }
                *row = entry.clone();
            }
            sheet.title_block.revision = entry.revision.clone();
            issued = entry.status == DrawingReleaseStatus::Released;
            if issued {
                sheet.release = DrawingReleaseDto {
                    status: DrawingReleaseStatus::Released,
                    released_revision: entry.revision,
                    released_at: entry.date,
                };
            }
        }
        Selection::Row(_, Table::Bom, _) | Selection::NewRow(_, Table::Bom) => {
            let entry: DrawingBomItemDto =
                serde_json::from_value(edited).map_err(|e| e.to_string())?;
            if new_row {
                sheet.bom.push(entry);
            } else {
                let row = sheet
                    .bom
                    .iter_mut()
                    .find(|r| r.id == entry.id)
                    .ok_or("BOM item was removed")?;
                *row = entry;
            }
        }
        _ => unreachable!(),
    }
    if !issued {
        return_released_sheets_to_draft(document, &mut next);
    }
    next.validate()?;
    Ok(next)
}
pub(super) fn delete(
    document: &DrawingDocumentDto,
    selection: Selection,
) -> Result<DrawingDocumentDto, String> {
    let Selection::Row(id, table, row) = selection else {
        return Err("Select a saved table row".into());
    };
    let mut next = document.clone();
    let sheet = next
        .sheets
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or("Sheet was removed")?;
    match table {
        Table::Revisions => {
            let existing = sheet
                .revisions
                .iter()
                .find(|r| r.id == row)
                .ok_or("Revision was removed")?;
            if existing.status == DrawingReleaseStatus::Released {
                return Err("Released revisions cannot be deleted".into());
            }
            sheet.revisions.retain(|r| r.id != row);
        }
        Table::Bom => {
            if !sheet.bom.iter().any(|r| r.id == row) {
                return Err("BOM item was removed".into());
            }
            sheet.bom.retain(|r| r.id != row);
            sheet.annotations.retain(|a| !matches!(a,DrawingAnnotationDto::ItemBalloon{bom_item_id,..} if *bom_item_id == row));
        }
    }
    return_released_sheets_to_draft(document, &mut next);
    next.validate()?;
    Ok(next)
}
fn row_caption(code: &str, description: &str) -> String {
    let mut characters = code.chars().chain(" · ".chars()).chain(description.chars());
    let mut caption: String = characters.by_ref().take(80).collect();
    if characters.next().is_some() {
        caption.push('\u{2026}');
    }
    caption
}
pub(super) fn rows(
    document: &DrawingDocumentDto,
    id: u64,
    table: Table,
) -> Result<Vec<(u64, String)>, String> {
    let sheet = sheet(document, id)?;
    let mut rows = vec![(0, "Table layout".into())];
    rows.extend(match table {
        Table::Revisions => sheet
            .revisions
            .iter()
            .map(|r| (r.id, row_caption(&r.revision, &r.description)))
            .collect::<Vec<_>>(),
        Table::Bom => sheet
            .bom
            .iter()
            .map(|r| (r.id, row_caption(&r.item_number, &r.description)))
            .collect(),
    });
    Ok(rows)
}

//! Locate authored note strings without rewriting JSONC or resolving includes.
//! Serde validates the document. The scanner retains only byte spans in the
//! length-preserving JSONC view, including last-property duplicate semantics.
use super::{strip_jsonc, MAX_SCRIPT_BYTES};
use serde_json::{json, Value};
use std::ops::Range;

pub fn authored_chapters(source: &str, expanded_steps: usize) -> Result<Value, String> {
    if source.len() > MAX_SCRIPT_BYTES {
        return Err("Script exceeds 16 MiB".into());
    }
    let clean = strip_jsonc(source)?;
    let document: Value = serde_json::from_str(&clean).map_err(|e| e.to_string())?;
    let root_steps = document["steps"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let offset = expanded_steps
        .checked_sub(root_steps.len())
        .ok_or("Expanded step count is inconsistent")?;
    let Some(steps) = member(&clean, 0..clean.len(), "steps") else {
        return Ok(json!([]));
    };
    let mut cursor = steps.start + 1;
    let mut notes = Vec::new();
    for (index, step) in root_steps.iter().enumerate() {
        whitespace(&clean, &mut cursor);
        let range = value(&clean, &mut cursor);
        if let Some(text) = step["note"].as_str() {
            if let Some(note) = member(&clean, range, "note") {
                if serde_json::from_str::<String>(&clean[note.clone()])
                    .ok()
                    .as_deref()
                    == Some(text)
                {
                    notes.push(
                        json!({"step_index":offset+index+1,"start":note.start,"end":note.end}),
                    );
                }
            }
        }
        whitespace(&clean, &mut cursor);
        if clean.as_bytes().get(cursor) == Some(&b',') {
            cursor += 1;
        }
    }
    Ok(Value::Array(notes))
}

fn whitespace(source: &str, cursor: &mut usize) {
    while source
        .as_bytes()
        .get(*cursor)
        .is_some_and(u8::is_ascii_whitespace)
    {
        *cursor += 1;
    }
}
fn string(source: &str, cursor: &mut usize) {
    *cursor += 1;
    while let Some(byte) = source.as_bytes().get(*cursor) {
        *cursor += 1;
        match byte {
            b'\\' => *cursor += 1,
            b'"' => break,
            _ => {}
        }
    }
}
fn value(source: &str, cursor: &mut usize) -> Range<usize> {
    whitespace(source, cursor);
    let start = *cursor;
    match source.as_bytes().get(*cursor) {
        Some(b'"') => string(source, cursor),
        Some(b'{' | b'[') => {
            let mut depth = 0;
            while let Some(byte) = source.as_bytes().get(*cursor) {
                match byte {
                    b'"' => {
                        string(source, cursor);
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            *cursor += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                *cursor += 1;
            }
        }
        _ => {
            while source
                .as_bytes()
                .get(*cursor)
                .is_some_and(|b| !b.is_ascii_whitespace() && !b",]}".contains(b))
            {
                *cursor += 1;
            }
        }
    }
    start..*cursor
}
fn member(source: &str, bounds: Range<usize>, wanted: &str) -> Option<Range<usize>> {
    let mut cursor = bounds.start;
    whitespace(source, &mut cursor);
    if source.as_bytes().get(cursor) != Some(&b'{') {
        return None;
    }
    cursor += 1;
    let mut found = None;
    while cursor < bounds.end {
        whitespace(source, &mut cursor);
        if source.as_bytes().get(cursor) != Some(&b'"') {
            break;
        }
        let key = value(source, &mut cursor);
        whitespace(source, &mut cursor);
        cursor += 1;
        let span = value(source, &mut cursor);
        if serde_json::from_str::<String>(&source[key]).ok().as_deref() == Some(wanted) {
            found = Some(span);
        }
        whitespace(source, &mut cursor);
        if source.as_bytes().get(cursor) != Some(&b',') {
            break;
        }
        cursor += 1;
    }
    found
}

//! Native OS recipe delivery. Opening a URL only queues installed source;
//! draft/document guards remain owned by the existing Scripts controller.
#[cfg(all(target_os = "linux", not(debug_assertions)))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(target_os = "windows", not(debug_assertions)))]
mod windows;

use bevy::prelude::App;
#[cfg(any(target_os = "macos", test))]
use bevy::prelude::{Resource, World};
#[cfg(any(target_os = "macos", test))]
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[cfg(any(target_os = "macos", test))]
const GET_URL: [u8; 4] = *b"GURL";
#[cfg(any(target_os = "macos", test))]
const KEY_DIRECT_OBJECT: [u8; 4] = *b"----";

/// Direct object of one OS GetURL (`GURL`/`GURL`) Apple event.
#[cfg(any(target_os = "macos", test))]
struct GetUrlPayload {
    event_class: [u8; 4],
    event_id: [u8; 4],
    keyword: [u8; 4],
    descriptor_type: [u8; 4],
    data: Vec<u8>,
}

#[derive(Resource, Clone, Default)]
#[cfg(any(target_os = "macos", test))]
struct Pending {
    queue: Arc<Mutex<VecDeque<String>>>,
    requested: Arc<Mutex<Vec<String>>>,
}

#[cfg(any(target_os = "macos", test))]
enum Queued {
    Fresh,
    Duplicate,
}

pub(crate) fn install(app: &mut App) {
    #[cfg(target_os = "macos")]
    macos::install(app);
    #[cfg(all(target_os = "linux", not(debug_assertions)))]
    if let Err(error) = std::thread::Builder::new()
        .name("cad-recipe-registration".into())
        .spawn(|| {
            if let Err(error) = linux::register() {
                eprintln!("Could not register recipe links: {error}");
            }
        })
    {
        eprintln!("Could not start recipe link registration: {error}");
    }
    #[cfg(all(target_os = "windows", not(debug_assertions)))]
    if let Err(error) = windows::register() {
        eprintln!("Could not register recipe links: {error}");
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

#[cfg(any(target_os = "macos", test))]
fn url_from_get_url(payload: &GetUrlPayload) -> Result<String, String> {
    if payload.event_class != GET_URL || payload.event_id != GET_URL {
        return Err("Not a GetURL Apple event".into());
    }
    if payload.keyword != KEY_DIRECT_OBJECT {
        return Err("GetURL is missing its direct object".into());
    }
    let url = decode_descriptor(&payload.descriptor_type, &payload.data)?;
    let url = url.trim_end_matches('\0');
    if url.is_empty() {
        return Err("GetURL direct object is empty".into());
    }
    limo_cad_mcp::recipe_id_from_uri(url)?;
    Ok(url.to_string())
}

#[cfg(any(target_os = "macos", test))]
fn decode_descriptor(descriptor_type: &[u8; 4], data: &[u8]) -> Result<String, String> {
    match descriptor_type {
        b"utf8" | b"TEXT" => {
            let bytes = data.strip_suffix(&[0]).unwrap_or(data);
            String::from_utf8(bytes.to_vec()).map_err(|_| "GetURL text is not UTF-8".into())
        }
        b"utxt" => decode_utf16(data, Endian::Native),
        b"ut16" => decode_utf16_external(data),
        _ => Err(format!(
            "GetURL text type {} is not supported",
            String::from_utf8_lossy(descriptor_type)
        )),
    }
}

#[cfg(any(target_os = "macos", test))]
enum Endian {
    Native,
    Big,
    Little,
}

#[cfg(any(target_os = "macos", test))]
fn decode_utf16(data: &[u8], endian: Endian) -> Result<String, String> {
    if !data.len().is_multiple_of(2) {
        return Err("GetURL UTF-16 text is truncated".into());
    }
    let mut units = Vec::with_capacity(data.len() / 2);
    for chunk in data.as_chunks::<2>().0 {
        let unit = match endian {
            Endian::Native => u16::from_ne_bytes([chunk[0], chunk[1]]),
            Endian::Big => u16::from_be_bytes([chunk[0], chunk[1]]),
            Endian::Little => u16::from_le_bytes([chunk[0], chunk[1]]),
        };
        units.push(unit);
    }
    if units.first() == Some(&0xFEFF) {
        units.remove(0);
    }
    if units.last() == Some(&0) {
        units.pop();
    }
    String::from_utf16(&units).map_err(|_| "GetURL text is not UTF-16".into())
}

#[cfg(any(target_os = "macos", test))]
fn decode_utf16_external(data: &[u8]) -> Result<String, String> {
    if data.starts_with(&[0xFE, 0xFF]) {
        decode_utf16(&data[2..], Endian::Big)
    } else if data.starts_with(&[0xFF, 0xFE]) {
        decode_utf16(&data[2..], Endian::Little)
    } else {
        decode_utf16(data, Endian::Big)
    }
}

#[cfg(any(target_os = "macos", test))]
fn enqueue(pending: &Pending, payload: &GetUrlPayload) -> Result<Queued, String> {
    let url = url_from_get_url(payload)?;
    let mut queue = pending
        .queue
        .lock()
        .map_err(|_| "Recipe link queue is unavailable".to_string())?;
    if queue.iter().any(|queued| queued == &url) {
        return Ok(Queued::Duplicate);
    }
    if queue.len() >= 16 {
        return Err("Recipe link queue is full; finish opening pending recipes".into());
    }
    queue.push_back(url);
    Ok(Queued::Fresh)
}

#[cfg(any(target_os = "macos", test))]
fn deliver(world: &mut World) {
    let Some(pending) = world.get_resource::<Pending>().cloned() else {
        return;
    };
    let requests = pending
        .queue
        .lock()
        .map(|mut queue| queue.drain(..).collect::<Vec<_>>())
        .unwrap_or_default();
    for url in requests {
        request_document_open(world, &pending, &url);
    }
}

#[cfg(any(target_os = "macos", test))]
fn request_document_open(world: &mut World, pending: &Pending, url: &str) {
    let id = match limo_cad_mcp::recipe_id_from_uri(url) {
        Ok(id) => id,
        Err(error) => {
            eprintln!("Recipe link rejected: {error}");
            return;
        }
    };
    if let Ok(mut requested) = pending.requested.lock() {
        if requested.len() == 32 {
            requested.remove(0);
        }
        requested.push(id.to_string());
    }
    crate::session_bridge::native_interface::controller::open_startup_recipe(world, id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(descriptor_type: [u8; 4], data: Vec<u8>) -> GetUrlPayload {
        GetUrlPayload {
            event_class: GET_URL,
            event_id: GET_URL,
            keyword: KEY_DIRECT_OBJECT,
            descriptor_type,
            data,
        }
    }

    fn utf16_native(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .flat_map(|unit| unit.to_ne_bytes())
            .collect()
    }

    #[test]
    fn get_url_payload_requests_document_open_with_decoded_id() {
        let url = "limo-cad://recipe/fillet-basics";
        let mut world = World::new();
        crate::session_bridge::native_interface::controller::insert_startup_controller(&mut world);
        let pending = Pending::default();
        world.insert_resource(pending.clone());

        let rejected = payload(*b"utf8", b"https://example.invalid/model".to_vec());
        assert!(enqueue(&pending, &rejected).is_err());
        let other_event = GetUrlPayload {
            event_class: *b"oapp",
            event_id: *b"oapp",
            keyword: KEY_DIRECT_OBJECT,
            descriptor_type: *b"utf8",
            data: url.as_bytes().to_vec(),
        };
        assert!(enqueue(&pending, &other_event).is_err());

        enqueue(&pending, &payload(*b"utxt", utf16_native(url))).unwrap();
        enqueue(&pending, &payload(*b"utxt", utf16_native(url))).unwrap();
        deliver(&mut world);

        assert_eq!(
            pending.requested.lock().unwrap().as_slice(),
            ["fillet-basics"]
        );
        assert_eq!(
            crate::session_bridge::native_interface::controller::queued_startup_recipe(&world)
                .as_deref(),
            Some("fillet-basics")
        );
    }
}

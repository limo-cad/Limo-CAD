//! Read/write the private post catalog outside the render thread.
use super::*;
use std::sync::{mpsc, Mutex};

#[derive(Resource, Default)]
struct Library {
    snapshot: Arc<Snapshot>,
    receiver: Option<Mutex<mpsc::Receiver<Result<Snapshot, String>>>>,
}

fn catalog(catalog: crate::cam_posts::Catalog, notice: String) -> Snapshot {
    Snapshot {
        profiles: catalog
            .entries
            .into_iter()
            .filter_map(|entry| entry.machine.map(|machine| (entry.file_name, machine)))
            .collect(),
        notice,
    }
}

fn start(
    world: &mut World,
    notice: &str,
    operation: impl FnOnce() -> Result<Snapshot, String> + Send + 'static,
) -> Result<(), String> {
    if busy(world) {
        return Err("Wait for the private profile library".into());
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    let wake = world.get_resource::<NativeInterfaceHandle>().cloned();
    std::thread::Builder::new()
        .name("native-cam-profiles".into())
        .spawn(move || {
            let _ = sender.send(operation());
            if let Some(handle) = wake {
                handle.request_redraw();
            }
        })
        .map_err(|error| format!("Could not open the private profile library: {error}"))?;
    let profiles = snapshot(world).profiles.clone();
    world.insert_resource(Library {
        snapshot: Arc::new(Snapshot {
            profiles,
            notice: notice.into(),
        }),
        receiver: Some(Mutex::new(receiver)),
    });
    Ok(())
}

pub(in super::super) fn reload(world: &mut World) -> Result<(), String> {
    if busy(world) {
        return Ok(());
    }
    start(world, "Loading private machine profiles…", || {
        let directory = crate::app_config::native_directory()?;
        crate::cam_posts::list(&directory).map(|result| catalog(result, String::new()))
    })
}

fn profile_file_name(name: &str) -> String {
    let mut stem = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            stem.push(ch);
        } else if !stem.ends_with('-') {
            stem.push('-');
        }
        if stem.len() == 120 {
            break;
        }
    }
    let stem = stem.trim_matches('-');
    format!("{}.nbpost", if stem.is_empty() { "machine" } else { stem })
}

pub(in super::super) fn save_profile(
    world: &mut World,
    machine: CamMachineAssignmentDto,
) -> Result<(), String> {
    let filename = profile_file_name(&machine.profile.name);
    start(world, "Saving private machine profile…", move || {
        let directory = crate::app_config::native_directory()?;
        crate::cam_posts::save_profile(&directory, &filename, machine)
            .map(|result| catalog(result, format!("Saved private profile {filename}")))
    })
}

pub(in super::super) fn poll(world: &mut World) -> bool {
    if !world.contains_resource::<Library>() {
        if let Err(error) = reload(world) {
            world.insert_resource(Library {
                snapshot: Arc::new(Snapshot {
                    profiles: vec![],
                    notice: error,
                }),
                receiver: None,
            });
            return true;
        }
    }
    let mut library = world.resource_mut::<Library>();
    let result = library
        .receiver
        .as_ref()
        .and_then(|receiver| match receiver.lock() {
            Ok(receiver) => match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Private profile worker stopped".into()))
                }
            },
            Err(_) => Some(Err("Private profile worker failed".into())),
        });
    if let Some(result) = result {
        library.receiver = None;
        library.snapshot = Arc::new(result.unwrap_or_else(|error| Snapshot {
            profiles: library.snapshot.profiles.clone(),
            notice: format!("Private profiles: {error}"),
        }));
        true
    } else {
        false
    }
}
pub(in super::super) fn snapshot(world: &World) -> Arc<Snapshot> {
    world
        .get_resource::<Library>()
        .map(|library| library.snapshot.clone())
        .unwrap_or_default()
}
pub(in super::super) fn busy(world: &World) -> bool {
    world
        .get_resource::<Library>()
        .is_some_and(|library| library.receiver.is_some())
}

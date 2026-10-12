//! Installed recipes open as editable source. URL delivery never runs a script.
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Mutex;
use tauri::Manager;
use tauri_plugin_deep_link::DeepLinkExt;

#[derive(Clone, Serialize)]
pub struct PendingRecipe {
    id: u64,
    recipe: String,
}

#[derive(Default)]
struct Queue {
    next_id: u64,
    requests: VecDeque<PendingRecipe>,
}

impl Queue {
    fn push(&mut self, uri: &str) -> Result<(), String> {
        let recipe = nbcad_mcp::recipe_id_from_uri(uri)?;
        if self.requests.iter().any(|request| request.recipe == recipe) {
            return Ok(());
        }
        if self.requests.len() >= 16 {
            return Err("Finish opening the pending recipes before opening another link".into());
        }
        self.next_id += 1;
        self.requests.push_back(PendingRecipe {
            id: self.next_id,
            recipe: recipe.into(),
        });
        Ok(())
    }

    fn acknowledge(&mut self, id: u64) {
        self.requests.retain(|request| request.id != id);
    }
}

#[derive(Default)]
pub struct RecipeLinkState(Mutex<Queue>);

#[tauri::command]
pub fn native_recipe_open_pending(state: tauri::State<'_, RecipeLinkState>) -> Vec<PendingRecipe> {
    state.0.lock().unwrap().requests.iter().cloned().collect()
}

#[tauri::command]
pub fn native_recipe_open_ack(state: tauri::State<'_, RecipeLinkState>, id: u64) {
    state.0.lock().unwrap().acknowledge(id);
}

fn enqueue(app: &tauri::AppHandle, uri: &str) {
    let result =
        crate::launch_requests::LaunchTarget::from_argument(uri, std::path::Path::new("/"))
            .and_then(|target| crate::launch_requests::enqueue(app, &[target]));
    if let Err(error) = result {
        eprintln!("Recipe link rejected: {error}");
    }
}

pub fn install(app: &tauri::AppHandle) {
    // OS links share the retained startup-ready launch queue. Explicit new
    // windows and isolated diagnostic profiles do not share a relaunch broker.
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    if !cfg!(debug_assertions)
        && !app
            .state::<crate::launch_requests::LaunchRequestsState>()
            .isolated
    {
        if let Err(error) = app.deep_link().register_all() {
            eprintln!("Could not register recipe links: {error}");
        }
    }
    let handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        for uri in event.urls() {
            enqueue(&handle, uri.as_str());
        }
    });
    // Cold-start events can precede the webview. Retain them until the frontend
    // explicitly acknowledges queueing the ID, including across webview reloads.
    if let Ok(Some(uris)) = app.deep_link().get_current() {
        for uri in uris {
            enqueue(app, uri.as_str());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_and_warm_delivery_survive_until_acknowledged() {
        let mut queue = Queue::default();
        queue.push("nbcad://recipe/garden-bench").unwrap();
        queue.push("nbcad://recipe/garden-bench").unwrap();
        assert_eq!(
            queue.requests.len(),
            1,
            "startup event/current URL deduplicate"
        );
        let cold = queue.requests.front().unwrap().id;
        queue.push("nbcad://recipe/d-screw-vise").unwrap();
        queue.acknowledge(999);
        assert_eq!(
            queue.requests.len(),
            2,
            "unknown acknowledgements cannot lose work"
        );
        queue.acknowledge(cold);
        assert_eq!(queue.requests.front().unwrap().recipe, "d-screw-vise");
        queue.push("nbcad://recipe/garden-bench").unwrap();
        assert_eq!(
            queue.requests.len(),
            2,
            "a later deliberate open is accepted"
        );
        assert!(queue.push("nbcad://recipe/d-screw-vise?run=true").is_err());
        assert_eq!(queue.requests.len(), 2);
    }
}

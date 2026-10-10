//! Receipt for the exact extracted frame whose GPU commands were submitted.
//! The OS/compositor may present later; hidden windows retain usable semantic
//! layout without claiming that a visible frame was rendered.

use super::super::interface_shell::NativeInterfaceHandle;
use bevy::{
    prelude::*,
    render::{
        render_resource::PipelineCache,
        renderer::{RenderGraph, RenderGraphSystems},
        view::window::ExtractedWindow,
        Extract, ExtractSchedule, RenderApp,
    },
    window::PrimaryWindow,
};

#[derive(Resource)]
struct ExtractedReceipt {
    handle: NativeInterfaceHandle,
    revision: u64,
}

pub(super) fn install(app: &mut App) {
    let render = app.sub_app_mut(RenderApp);
    render
        .add_systems(ExtractSchedule, extract)
        .add_systems(RenderGraph, submitted.in_set(RenderGraphSystems::Finish));
}

fn extract(mut commands: Commands, handle: Extract<Option<Res<NativeInterfaceHandle>>>) {
    if let Some(handle) = handle.as_ref() {
        if let Some(receipt) = handle.render_receipt() {
            commands.insert_resource(ExtractedReceipt {
                handle: (*handle).clone(),
                revision: receipt.laid_out_revision,
            });
        }
    }
}

fn submitted(
    receipt: Option<Res<ExtractedReceipt>>,
    windows: Query<&ExtractedWindow, With<PrimaryWindow>>,
    pipelines: Res<PipelineCache>,
    mut compiling: Local<bool>,
) {
    let Some(receipt) = receipt else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    if window.swap_chain_texture.is_some() && window.swap_chain_texture_view.is_some() {
        // The host sleeps until an event. A pipeline waiting for shader assets
        // or background compilation needs another render pass, so keep drawing
        // while any are pending and once more when the queue becomes ready.
        let pending = pipelines.waiting_pipelines().next().is_some();
        if pending || *compiling {
            receipt.handle.request_redraw();
        }
        *compiling = pending;
        if let Err(error) = receipt.handle.submitted_revision(receipt.revision) {
            eprintln!("Native render receipt rejected: {error}");
        }
    }
}

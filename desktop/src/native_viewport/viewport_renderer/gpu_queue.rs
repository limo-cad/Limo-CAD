//! Serialize surface configuration with submissions on the shared GPU queue.
//!
//! wgpu requires an idle queue when configuring a surface. A concurrent CAD
//! submission can invalidate that wait, leaving a new window unconfigured.
//! Asset preparation can submit buffer/texture copies too. Shader compilation
//! stays outside the lease, so it cannot hold up interface rendering or input.

use bevy::{
    app::SubApp,
    prelude::*,
    render::{
        renderer::{render_system, RenderGraph, RenderGraphSystems},
        Render, RenderSystems,
    },
};
use std::sync::{Arc, Condvar, Mutex};

#[derive(Resource, Clone, Default)]
pub(super) struct SharedQueue(Arc<(Mutex<bool>, Condvar)>);

impl SharedQueue {
    fn acquire(&self) -> Lease {
        let (busy, ready) = &*self.0;
        let mut busy = ready
            .wait_while(busy.lock().unwrap(), |busy| *busy)
            .unwrap();
        *busy = true;
        Lease(self.clone())
    }
}

struct Lease(SharedQueue);

impl Drop for Lease {
    fn drop(&mut self) {
        let SharedQueue(state) = &self.0;
        let (busy, ready) = &**state;
        *busy.lock().unwrap() = false;
        ready.notify_one();
    }
}

// An owned lease can live across Bevy systems and releases on render-world
// teardown even if a system panics before the normal release point.
#[derive(Resource, Default)]
struct QueueLease(Option<Lease>);

fn acquire(world: &mut World) {
    assert!(world.resource::<QueueLease>().0.is_none());
    let lease = world.resource::<SharedQueue>().acquire();
    world.resource_mut::<QueueLease>().0 = Some(lease);
}

fn release(world: &mut World) {
    drop(world.resource_mut::<QueueLease>().0.take());
}

pub(super) fn install_interface(render: &mut SubApp, queue: SharedQueue) {
    render
        .insert_resource(queue)
        .init_resource::<QueueLease>()
        .add_systems(
            Render,
            (
                acquire.before(RenderSystems::ExtractCommands),
                // prepare_windows can reconfigure an outdated surface even
                // when its size/present mode did not change.
                release
                    .after(RenderSystems::PrepareViews)
                    .before(RenderSystems::Queue),
            ),
        );
}

pub(super) fn install_worker(render: &mut SubApp, queue: SharedQueue) {
    render
        .insert_resource(queue)
        .init_resource::<QueueLease>()
        .add_systems(
            Render,
            (
                acquire.before(RenderSystems::ExtractCommands),
                release
                    .after(RenderSystems::Prepare)
                    .before(RenderSystems::Render),
                release.after(render_system).before(RenderSystems::Cleanup),
            ),
        )
        // Bevy compiles queued pipelines before entering the render graph.
        // Acquire here, and retain the lease through render_system's final
        // screenshot/readback submission after the graph returns.
        .add_systems(RenderGraph, acquire.before(RenderGraphSystems::Begin));
}

#[cfg(test)]
#[path = "gpu_queue/tests.rs"]
mod tests;

//! Coordinate the two renderers' shared queue with window surface configuration.
//!
//! wgpu requires the queue to stay idle during surface configuration. Submitting
//! concurrently can fail configuration, leaving the next window texture acquire
//! unconfigured. Asset preparation can also submit, so protect the whole GPU
//! update and run CAD pipeline compilation separately, outside that protection.

use super::*;
use bevy::{
    app::SubApp,
    ecs::schedule::{InternedScheduleLabel, ScheduleCleanupPolicy, ScheduleLabel},
    render::{
        renderer::render_system,
        view::window::{create_surfaces, need_surface_configuration, prepare_windows},
    },
};

#[derive(Resource, Clone, Default)]
pub(super) struct Submissions(Arc<Mutex<()>>);

impl Submissions {
    fn update(
        &self,
        world: &mut World,
        schedule: InternedScheduleLabel,
        stopped: Option<&AtomicBool>,
    ) -> bool {
        // Shutdown may join this worker while the interface's final frame is
        // waiting for main-thread work. Do not wait for its queue guard then.
        let _submitting = if let Some(stopped) = stopped {
            loop {
                if stopped.load(Ordering::Acquire) {
                    return false;
                }
                match self.0.try_lock() {
                    Ok(submitting) => break submitting,
                    Err(std::sync::TryLockError::WouldBlock) => {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    Err(std::sync::TryLockError::Poisoned(error)) => {
                        panic!("shared GPU queue: {error}");
                    }
                }
            }
        } else {
            self.0.lock().expect("shared GPU queue")
        };
        world.run_schedule(schedule);
        true
    }
}

#[derive(Resource, Clone)]
struct RenderUpdate {
    original: InternedScheduleLabel,
    submissions: Submissions,
    stopped: Option<Arc<AtomicBool>>,
}

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct CoordinatedUpdate;

pub(super) fn coordinate_interface(render: &mut SubApp) -> Submissions {
    let submissions = Submissions::default();
    coordinate_render(render, submissions.clone(), None);
    // Bevy's weak phase ordering cannot see GPU dependencies behind read-only
    // RenderDevice/RenderQueue resources. Complete uploads before acquiring a
    // window frame (which can reconfigure an outdated surface). Keep each
    // phase's parallelism and main-thread surface creation on macOS.
    render.configure_sets(
        Render,
        (
            RenderSystems::ExtractCommands,
            RenderSystems::PrepareAssets,
            RenderSystems::PrepareMeshes,
            RenderSystems::CreateViews,
            RenderSystems::Specialize,
            RenderSystems::PrepareViews,
            RenderSystems::Queue,
            RenderSystems::PhaseSort,
            RenderSystems::Prepare,
            RenderSystems::Render,
            RenderSystems::Cleanup,
            RenderSystems::PostCleanup,
        )
            .chain(),
    );
    submissions
}

pub(super) fn order_surfaces(render: &mut SubApp) {
    render
        .world_mut()
        .schedule_scope(Render, |world, schedule| {
            let removed = schedule
                .remove_systems_in_set(
                    create_surfaces,
                    world,
                    ScheduleCleanupPolicy::RemoveSystemsOnly,
                )
                .expect("window surface system");
            assert_eq!(removed, 1, "Bevy window surface system changed");
            schedule.add_systems(
                create_surfaces
                    .run_if(need_surface_configuration)
                    .in_set(RenderSystems::PrepareViews)
                    .after(RenderSystems::ExtractCommands)
                    .before(prepare_windows),
            );
        });
}

fn coordinate_render(
    render: &mut SubApp,
    submissions: Submissions,
    stopped: Option<Arc<AtomicBool>>,
) {
    render.insert_resource(RenderUpdate {
        original: render.update_schedule.expect("GPU render schedule"),
        submissions,
        stopped,
    });
    render.update_schedule = Some(CoordinatedUpdate.intern());
    render.add_systems(CoordinatedUpdate, update_render);
}

fn update_render(world: &mut World) {
    let update = world.resource::<RenderUpdate>().clone();
    update
        .submissions
        .update(world, update.original, update.stopped.as_deref());
}

pub(super) fn prepare_worker(app: &mut App, submissions: Submissions, stopped: Arc<AtomicBool>) {
    let render = app.sub_app_mut(RenderApp);
    coordinate_render(render, submissions, Some(stopped));
    render
        .world_mut()
        .schedule_scope(Render, |world, schedule| {
            // Bevy installs pipeline compilation and render_system in this set.
            // Move compilation outside the queue guard without omitting rendering.
            let removed = schedule
                .remove_systems_in_set(
                    RenderSystems::Render,
                    world,
                    ScheduleCleanupPolicy::RemoveSystemsOnly,
                )
                .expect("CAD render systems");
            assert_eq!(removed, 2, "Bevy CAD render systems changed");
            schedule.add_systems(render_system.in_set(RenderSystems::Render));
        });
}

pub(super) fn update_worker(app: &mut App, stopped: &AtomicBool) -> bool {
    // Mesh rebuilding and extraction run before the guarded RenderApp schedule.
    app.update();
    if stopped.load(Ordering::Acquire) {
        return false;
    }
    #[cfg(test)]
    if let Some(gate) = app
        .world_mut()
        .remove_resource::<super::tests::CompilationGate>()
    {
        let _ = gate.entered.send(());
        let _ = gate.receiver.lock().unwrap().recv();
    }
    if stopped.load(Ordering::Acquire) {
        return false;
    }
    // Specialization queues pipelines in the preceding update. Compile them here
    // and render them on the next update, leaving interface frames free to run.
    app.sub_app_mut(RenderApp)
        .world_mut()
        .resource_mut::<PipelineCache>()
        .process_queue();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[derive(Resource)]
    struct Upload {
        pending: Arc<AtomicBool>,
        started: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
        acquired: mpsc::Sender<bool>,
    }

    fn upload(upload: Res<Upload>) {
        upload.started.send(()).unwrap();
        upload
            .release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        upload.pending.store(false, Ordering::Release);
    }

    fn acquire(_: bevy::ecs::system::NonSendMarker, upload: Res<Upload>) {
        upload
            .acquired
            .send(!upload.pending.load(Ordering::Acquire))
            .unwrap();
    }

    fn intermediate_phase(_: Res<Upload>) {}

    fn render_fixture() -> SubApp {
        bevy::tasks::ComputeTaskPool::get_or_init(|| {
            bevy::tasks::TaskPoolBuilder::new().num_threads(2).build()
        });
        let mut render = SubApp::new();
        render.add_schedule(Render::base_schedule());
        render.update_schedule = Some(Render.intern());
        render
    }

    #[test]
    fn window_frame_acquisition_waits_for_gpu_asset_uploads() {
        let mut render = render_fixture();
        coordinate_interface(&mut render);
        let (started, uploading) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let (acquired, acquisition) = mpsc::channel();
        render.insert_resource(Upload {
            pending: Arc::new(AtomicBool::new(true)),
            started,
            release: Mutex::new(released),
            acquired,
        });
        // Both systems read their ECS resource, just like GPU queue operations.
        // Phase ordering must protect GPU operations even without an ECS conflict.
        // Populate the intervening phases to reproduce the native render schedule.
        render.add_systems(
            Render,
            (
                acquire.in_set(RenderSystems::PrepareViews),
                upload.in_set(RenderSystems::PrepareAssets),
                intermediate_phase.in_set(RenderSystems::PrepareMeshes),
                intermediate_phase.in_set(RenderSystems::CreateViews),
                intermediate_phase.in_set(RenderSystems::Specialize),
            ),
        );
        let thread = std::thread::spawn(move || render.world_mut().run_schedule(Render));
        uploading.recv_timeout(Duration::from_secs(5)).unwrap();
        let premature = acquisition.recv_timeout(Duration::from_millis(100));
        release.send(()).unwrap();
        thread.join().unwrap();
        assert!(
            premature.is_err(),
            "window acquisition overlapped an upload"
        );
        assert!(acquisition.recv_timeout(Duration::from_secs(5)).unwrap());
    }

    #[test]
    fn interface_surface_update_excludes_cad_queue_submissions() {
        let mut render = render_fixture();
        let submissions = coordinate_interface(&mut render);
        let (started, configuring) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let (acquired, submitted) = mpsc::channel();
        let pending = Arc::new(AtomicBool::new(true));
        render.insert_resource(Upload {
            pending: pending.clone(),
            started,
            release: Mutex::new(released),
            acquired: acquired.clone(),
        });
        render.add_systems(Render, upload.in_set(RenderSystems::PrepareViews));
        let host = std::thread::spawn(move || render.update());
        configuring.recv_timeout(Duration::from_secs(5)).unwrap();

        let (attempted, attempting) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut app = App::new();
            let (started, _) = mpsc::channel();
            let (_, released) = mpsc::channel();
            app.insert_resource(Upload {
                pending,
                started,
                release: Mutex::new(released),
                acquired,
            });
            app.add_systems(Update, acquire);
            attempted.send(()).unwrap();
            assert!(submissions.update(
                app.world_mut(),
                Update.intern(),
                Some(&AtomicBool::new(false)),
            ));
        });
        attempting.recv_timeout(Duration::from_secs(5)).unwrap();
        let premature = submitted.recv_timeout(Duration::from_millis(100));
        release.send(()).unwrap();
        host.join().unwrap();
        worker.join().unwrap();
        assert!(premature.is_err(), "CAD submitted during surface setup");
        assert!(submitted.recv_timeout(Duration::from_secs(5)).unwrap());
    }

    #[test]
    fn interface_frames_continue_while_cad_geometry_prepares() {
        let mut interface = render_fixture();
        let submissions = coordinate_interface(&mut interface);
        let (acquired, acquisition) = mpsc::channel();
        let (started, _) = mpsc::channel();
        let (_, released) = mpsc::channel();
        interface.insert_resource(Upload {
            pending: Arc::new(AtomicBool::new(false)),
            started,
            release: Mutex::new(released),
            acquired: acquired.clone(),
        });
        interface.add_systems(Render, acquire.in_set(RenderSystems::PrepareViews));

        let (started, preparing) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut app = App::new();
            app.insert_resource(Upload {
                pending: Arc::new(AtomicBool::new(false)),
                started,
                release: Mutex::new(released),
                acquired,
            });
            app.add_systems(Update, upload);
            let mut render = render_fixture();
            coordinate_render(
                &mut render,
                submissions,
                Some(Arc::new(AtomicBool::new(false))),
            );
            app.insert_sub_app(RenderApp, render);
            app.update();
        });
        preparing.recv_timeout(Duration::from_secs(5)).unwrap();
        let host = std::thread::spawn(move || interface.update());
        let frame = acquisition.recv_timeout(Duration::from_secs(1));
        release.send(()).unwrap();
        host.join().unwrap();
        worker.join().unwrap();
        assert!(frame.expect("CAD geometry preparation blocked the interface"));
    }

    #[test]
    fn stopped_worker_does_not_wait_for_an_interface_frame() {
        let mut render = render_fixture();
        let submissions = coordinate_interface(&mut render);
        let (started, configuring) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let (acquired, _) = mpsc::channel();
        render.insert_resource(Upload {
            pending: Arc::new(AtomicBool::new(true)),
            started,
            release: Mutex::new(released),
            acquired,
        });
        render.add_systems(Render, upload.in_set(RenderSystems::PrepareViews));
        let host = std::thread::spawn(move || render.update());
        configuring.recv_timeout(Duration::from_secs(5)).unwrap();
        let (finished, completion) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            finished
                .send(submissions.update(
                    &mut World::new(),
                    Update.intern(),
                    Some(&AtomicBool::new(true)),
                ))
                .unwrap();
        });
        let result = completion.recv_timeout(Duration::from_secs(1));
        release.send(()).unwrap();
        host.join().unwrap();
        worker.join().unwrap();
        assert!(!result.unwrap());
    }
}

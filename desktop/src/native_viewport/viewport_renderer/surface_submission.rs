//! Coordinate native surface operations with the independent CAD renderer.
//!
//! wgpu must finish all submissions before configuring a surface. Its wait can
//! fail if another thread submits on the shared queue before the queue is empty.
//! Acquisition can also reconfigure an outdated surface. Keep both surface paths
//! protected, while main-world modeling and pipeline compilation stay independent.
use super::{NativeViewportCamera, CAD_TARGET_FORMAT};
use bevy::ecs::system::NonSendMarker;
use bevy::{
    app::{AppExit, SubApp},
    camera::{CameraOutputMode, CompositingSpace, RenderTarget},
    core_pipeline::blit::{BlitPipeline, BlitPipelineKey},
    ecs::{
        schedule::{ScheduleCleanupPolicy, ScheduleLabel, Schedules, SingleThreadedExecutor},
        system::{CombinatorSystem, Combine, RunSystemError, System, SystemAccess, SystemIn},
    },
    prelude::*,
    render::{
        camera::SortedCameras,
        render_resource::{BlendState, PipelineCache, SpecializedRenderPipelines},
        renderer::{RenderAdapter, RenderDevice, RenderInstance, RenderQueue},
        sync_world::MainEntity,
        view::window::{
            create_surfaces, need_surface_configuration, prepare_windows, ExtractedWindow,
            SurfaceData,
        },
        Render, RenderSystems,
    },
    window::RawHandleWrapper,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[derive(Clone, Default, Resource)]
pub(super) struct SharedSubmission(Arc<Mutex<()>>);

impl SharedSubmission {
    pub(super) fn run<T>(&self, work: impl FnOnce() -> T) -> T {
        let _guard = self.0.lock().expect("shared GPU submission gate");
        work()
    }

    pub(super) fn run_until_stopped<T>(
        &self,
        stopped: &AtomicBool,
        work: impl FnOnce() -> T,
    ) -> Option<T> {
        // Shutdown joins the worker while the interface may still need the main
        // thread to finish its last surface operation. Cancel a pending wait.
        loop {
            if stopped.load(Ordering::Acquire) {
                return None;
            }
            match self.0.try_lock() {
                Ok(_guard) => return Some(work()),
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(std::sync::TryLockError::Poisoned(error)) => {
                    panic!("shared GPU submission gate: {error}");
                }
            }
        }
    }
}

struct RunGuardedWindow;

impl<A, B> Combine<A, B> for RunGuardedWindow
where
    A: System<In = (), Out = ()>,
    B: System<In = (), Out = ()>,
{
    type In = ();
    type Out = ();

    fn combine<T>(
        _: (),
        data: &mut T,
        guarded: impl FnOnce(SystemIn<'_, A>, &mut T) -> Result<(), RunSystemError>,
        _: impl FnOnce(SystemIn<'_, B>, &mut T) -> Result<(), RunSystemError>,
    ) -> Result<(), RunSystemError> {
        // The guarded system calls the original function with its parameters.
        // Running the other closure as well would duplicate surface operations.
        guarded((), data)
    }
}

fn guarded_window_system<A, B, AM, BM>(original: A, guarded: B) -> impl System<In = (), Out = ()>
where
    A: IntoSystem<(), (), AM>,
    B: IntoSystem<(), (), BM>,
{
    let original = IntoSystem::into_system(original);
    let guarded = IntoSystem::into_system(guarded);
    let name = guarded.name();
    // Bevy's combinator retains both original function-type ordering sets and
    // normal access/thread/deferred metadata through its public System API.
    // Function-type sets cannot be assigned with in_set. The original system
    // contributes identity and metadata, and has no commands to apply because
    // only the guarded closure runs.
    // CombinatorSystem reports the first system's last-run tick. Put the system
    // that actually executes first so change/error metadata stays accurate.
    CombinatorSystem::<RunGuardedWindow, _, _>::new(guarded, original, name)
}

pub(super) fn install_interface(render: &mut SubApp) -> SharedSubmission {
    let submission = SharedSubmission::default();
    render.insert_resource(submission.clone());
    render
        .world_mut()
        .schedule_scope(Render, |world, schedule| {
            // This replaces exactly the pinned Bevy window configuration system,
            // rather than changing or skipping the rendering schedule around it.
            let removed = schedule
                .remove_systems_in_set(
                    create_surfaces,
                    world,
                    ScheduleCleanupPolicy::RemoveSystemsOnly,
                )
                .expect("find Bevy window surface configuration");
            assert_eq!(removed, 1, "expected one Bevy surface configuration system");
            let removed = schedule
                .remove_systems_in_set(
                    prepare_windows,
                    world,
                    ScheduleCleanupPolicy::RemoveSystemsOnly,
                )
                .expect("find Bevy window surface acquisition");
            assert_eq!(removed, 1, "expected one Bevy surface acquisition system");
            schedule.add_systems(
                guarded_window_system(create_surfaces, configure_surfaces)
                    .run_if(need_surface_configuration)
                    .after(RenderSystems::ExtractCommands)
                    .before(acquire_surfaces),
            );
            schedule.add_systems(
                guarded_window_system(prepare_windows, acquire_surfaces)
                    // With no extracted windows the original loop does no work.
                    .run_if(any_with_component::<ExtractedWindow>)
                    .in_set(RenderSystems::PrepareViews),
            );
        });
    submission
}

// Mirrors Bevy's surface-system inputs, with the shared submission guard.
fn configure_surfaces(
    commands: Commands,
    _marker: NonSendMarker,
    windows: Query<(
        Entity,
        &mut ExtractedWindow,
        &RawHandleWrapper,
        Option<&mut SurfaceData>,
    )>,
    render_instance: Res<RenderInstance>,
    render_adapter: Res<RenderAdapter>,
    render_device: Res<RenderDevice>,
    (submission, _queue_exclusion): (Res<SharedSubmission>, ResMut<RenderQueue>),
) {
    // A CAD buffer scope can tick unrelated global tasks while holding this
    // gate. Keep surface work on the interface's existing main-thread executor
    // on every platform so the worker cannot steal it and reacquire its own
    // gate. Mutable queue access also excludes same-world GPU-copy systems.
    submission.run(|| {
        create_surfaces(
            commands,
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            _marker,
            windows,
            render_instance,
            render_adapter,
            render_device,
        );
    });
}

fn acquire_surfaces(
    _marker: NonSendMarker,
    windows: Query<(MainEntity, &mut ExtractedWindow, Option<&SurfaceData>)>,
    render_device: Res<RenderDevice>,
    sorted_cameras: Res<SortedCameras>,
    #[cfg(target_os = "linux")] render_instance: Res<RenderInstance>,
    submission: Res<SharedSubmission>,
    _queue_exclusion: ResMut<RenderQueue>,
) {
    // The upstream acquisition system owns the Outdated reconfiguration branch
    // and its camera/retained-texture conditions. Do not duplicate that logic.
    submission.run(|| {
        prepare_windows(
            windows,
            render_device,
            sorted_cameras,
            #[cfg(target_os = "linux")]
            render_instance,
        );
    });
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, ScheduleLabel)]
struct CadGpuUpdate;

#[derive(Default, Resource)]
struct CompiledAfterDraw(bool);

#[derive(Default, Resource)]
struct WorkerExiting(bool);

pub(super) fn compiled_after_draw(world: &World) -> bool {
    world.resource::<CompiledAfterDraw>().0
}

pub(super) fn install_worker(
    render: &mut SubApp,
    submission: SharedSubmission,
    stopped: Arc<AtomicBool>,
) {
    remove_cache_processor::<PipelineCache>(render);
    install_worker_schedule(
        render,
        submission,
        stopped,
        prewarm_blit_pipelines,
        compile_pipelines,
    );
}

fn remove_cache_processor<R: Resource>(render: &mut SubApp) {
    render
        .world_mut()
        .schedule_scope(Render, |world, schedule| {
            schedule
                .initialize(world)
                .expect("initialize CAD renderer access metadata");
            let cache_id = world.component_id::<R>().expect("renderer cache resource");
            // The pinned Render stage contains the private queue processor and the
            // exclusive render system. Upscaling's cache writer is in Prepare. Use
            // actual stage/access/default sets, which survive release debug stripping.
            let render_ids = schedule
                .graph()
                .systems_in_set(RenderSystems::Render.intern())
                .expect("CAD render-stage membership")
                .clone();
            let mut candidates: Vec<_> = schedule
                .systems_with_access()
                .expect("CAD renderer system access")
                .filter_map(|(key, system)| {
                    if !render_ids.contains(&key) {
                        return None;
                    }
                    let SystemAccess::Shared(access) = system.access() else {
                        return None;
                    };
                    access
                        .combined_access()
                        .has_write(cache_id)
                        .then(|| system.system().default_system_sets())
                })
                .collect();
            assert_eq!(candidates.len(), 1, "expected one isolated cache processor");
            let mut sets: Vec<_> = candidates
                .pop()
                .unwrap()
                .into_iter()
                .filter(|set| set.system_type().is_some())
                .collect();
            assert_eq!(sets.len(), 1, "expected one typed cache processor set");
            let removed = schedule
                .remove_systems_in_set(
                    sets.pop().unwrap(),
                    world,
                    ScheduleCleanupPolicy::RemoveSystemsOnly,
                )
                .expect("remove original CAD pipeline processing");
            assert_eq!(removed, 1, "expected one CAD pipeline processing system");
        });
}

fn prewarm_blit_pipelines(main: &mut World, world: &mut World) {
    // setup_scene owns exactly two linear CAD cameras on the same image: the
    // first replaces its output and the transient overlay uses alpha blending.
    // Materialize their actual native target keys before guarded Prepare. The
    // upstream upscaling system otherwise calls process_queue for a new blit ID,
    // which can compile every newly queued PBR shader while holding the gate.
    // Repeat after extraction because RenderStartup also resets these resources
    // during device recovery; keep the upstream upscaling system unchanged.
    // Render camera insertion is deferred until ExtractCommands. Derive keys
    // from the actual worker cameras/image here, not stale extracted components.
    let mut camera_query = main.query_filtered::<
        (&Camera, &RenderTarget, Option<&CompositingSpace>),
        With<NativeViewportCamera>,
    >();
    let mut cameras: Vec<_> = camera_query
        .iter(main)
        .filter(|(camera, ..)| camera.is_active)
        .map(|(camera, target, space)| {
            assert!(
                matches!(space, None | Some(CompositingSpace::Linear)),
                "CAD camera compositing must remain linear"
            );
            let RenderTarget::Image(target) = target else {
                panic!("CAD worker camera must target its native image");
            };
            (camera.order, target.handle.clone(), camera.output_mode)
        })
        .collect();
    assert_eq!(cameras.len(), 2, "expected two native CAD cameras");
    cameras.sort_unstable_by_key(|(order, ..)| *order);
    assert!(
        cameras[0].0 < cameras[1].0,
        "CAD camera order must be distinct"
    );
    assert_eq!(
        cameras[0].1, cameras[1].1,
        "CAD cameras must share an image"
    );
    let images = main.resource::<Assets<Image>>();
    let keys = cameras
        .into_iter()
        .enumerate()
        .map(|(index, (_, image, output))| {
            let image = images.get(&image).expect("native CAD target image");
            let format = image
                .texture_view_descriptor
                .as_ref()
                .and_then(|view| view.format)
                .unwrap_or(image.texture_descriptor.format);
            assert_eq!(format, CAD_TARGET_FORMAT, "native CAD image view format");
            let blend_state = match output {
                CameraOutputMode::Skip => None,
                CameraOutputMode::Write { blend_state, .. } => {
                    blend_state.or_else(|| (index > 0).then_some(BlendState::ALPHA_BLENDING))
                }
            };
            BlitPipelineKey {
                target_format: format,
                blend_state,
                samples: 1,
                source_space: None,
            }
        });
    world.resource_scope(|world, mut cache: Mut<PipelineCache>| {
        world.resource_scope(
            |world, mut pipelines: Mut<SpecializedRenderPipelines<BlitPipeline>>| {
                let blit = world.resource::<BlitPipeline>();
                let ids: Vec<_> = keys
                    .map(|key| pipelines.specialize(&cache, blit, key))
                    .collect();
                for id in ids {
                    cache.block_on_render_pipeline(id);
                }
            },
        );
    });
}

fn compile_pipelines(world: &mut World) -> bool {
    #[cfg(test)]
    if let Some(gate) = world.remove_resource::<super::tests::CompilationGate>() {
        let _ = gate.entered.send(());
        if gate.receiver.lock().unwrap().recv().is_err() {
            return false;
        }
    }
    let mut cache = world.resource_mut::<PipelineCache>();
    let before = cache.pipelines().count();
    let pending = cache.waiting_pipelines().next().is_some();
    cache.process_queue();
    pending || before != cache.pipelines().count()
}

fn install_worker_schedule(
    render: &mut SubApp,
    submission: SharedSubmission,
    stopped: Arc<AtomicBool>,
    prewarm: fn(&mut World, &mut World),
    compile: fn(&mut World) -> bool,
) {
    // Guarded render systems must execute on their owning worker. A Send ECS
    // system otherwise enters the global compute pool and can be stolen by the
    // interface's main thread while it services window tasks. A nested scoped
    // job can then run surface acquisition on that same thread, waiting for the
    // GPU gate held by this worker while the worker awaits the stolen system.
    // Keep internal scoped buffer work parallel, but never dispatch an entire
    // guarded system to that shared executor. Include startup/device recovery.
    for (_, schedule) in render.world_mut().resource_mut::<Schedules>().iter_mut() {
        schedule.set_executor(SingleThreadedExecutor::new());
    }
    // Extraction runs Bevy's RenderStartup and recovery callbacks too. Protect
    // those GPU initialization paths without gating the CAD main-world update.
    render.init_resource::<CompiledAfterDraw>();
    render.init_resource::<WorkerExiting>();
    let mut original_extract = render.take_extract().expect("CAD renderer extraction");
    let extract_submission = submission.clone();
    let extract_stopped = stopped.clone();
    render.set_extract(move |main, world| {
        let extracted = extract_submission
            .run_until_stopped(&extract_stopped, || original_extract(main, world));
        let exiting = extracted.is_none()
            || main
                .get_resource::<Messages<AppExit>>()
                .is_some_and(|messages| !messages.is_empty());
        world.resource_mut::<WorkerExiting>().0 = exiting;
        if exiting {
            world.resource_mut::<CompiledAfterDraw>().0 = false;
            return;
        }
        prewarm(main, world);
    });
    let original = render
        .update_schedule
        .expect("CAD renderer update schedule");
    render.update_schedule = Some(CadGpuUpdate.intern());
    render.add_systems(CadGpuUpdate, move |world: &mut World| {
        if world.resource::<WorkerExiting>().0 {
            return;
        }
        // Asset preparation can submit image/slab/storage copies too. Protect
        // the entire render update, including Bevy's existing recovery policy,
        // while leaving CAD main-world/model preparation outside the gate.
        if submission
            .run_until_stopped(&stopped, || world.run_schedule(original))
            .is_none()
            || stopped.load(Ordering::Acquire)
        {
            world.resource_mut::<CompiledAfterDraw>().0 = false;
            return;
        }
        // macOS pipeline creation is synchronous even when Bevy's async flag is
        // false. Compile outside the gate after draining this draw's requests;
        // require a later stable draw before publishing its texture.
        let changed = compile(world);
        world.resource_mut::<CompiledAfterDraw>().0 = changed;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::{change_detection::Tick, system::SystemChangeTick};
    use bevy::render::RenderStartup;
    use std::{sync::mpsc, thread, time::Duration};

    #[derive(Clone, Debug, PartialEq, Eq, Hash, ScheduleLabel)]
    struct OriginalWorkerUpdate;

    #[derive(Default, Resource)]
    struct SurfaceOrder(Vec<&'static str>);

    #[derive(Resource)]
    struct PreparedSurface(Tick);

    fn original_cpu_surface(mut order: ResMut<SurfaceOrder>) {
        order.0.push("unguarded");
    }

    fn guarded_cpu_surface(
        mut commands: Commands,
        mut order: ResMut<SurfaceOrder>,
        tick: SystemChangeTick,
    ) {
        order.0.push("guarded");
        commands.insert_resource(PreparedSurface(tick.this_run()));
    }

    #[derive(Clone, Default, Resource)]
    struct CpuPipelines {
        processed_in_render: usize,
        prepares: usize,
        draws: usize,
        extracts: usize,
        startups: usize,
        prewarms: usize,
        compiles: usize,
        pending_same_id: bool,
    }

    #[derive(Resource)]
    struct Holds {
        gpu_entered: mpsc::Sender<()>,
        gpu_release: Mutex<mpsc::Receiver<()>>,
        compile_entered: mpsc::Sender<()>,
        compile_release: Mutex<mpsc::Receiver<()>>,
    }

    #[derive(Resource)]
    struct RequestExit;

    #[derive(Resource)]
    struct RenderThread(std::thread::ThreadId);

    fn guarded_scoped_buffers(owner: Res<RenderThread>, submission: Res<SharedSubmission>) {
        assert_eq!(
            std::thread::current().id(),
            owner.0,
            "a guarded render system must not be stolen by another executor"
        );
        assert!(submission.0.try_lock().is_err());
        bevy::tasks::ComputeTaskPool::get().scope(|scope| {
            scope.spawn(async {});
        });
    }

    #[test]
    fn guarded_worker_schedules_keep_scoped_systems_on_the_render_thread() {
        bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        let submission = SharedSubmission::default();
        let worker = thread::spawn(move || {
            let mut worker = SubApp::new();
            worker.update_schedule = Some(OriginalWorkerUpdate.intern());
            worker.insert_resource(RenderThread(thread::current().id()));
            worker.insert_resource(submission.clone());
            worker.add_systems(RenderStartup, guarded_scoped_buffers);
            worker.add_systems(bevy::render::ExtractSchedule, guarded_scoped_buffers);
            worker.add_systems(Render, guarded_scoped_buffers);
            worker.add_systems(OriginalWorkerUpdate, |world: &mut World| {
                world.run_schedule(Render)
            });
            worker.set_extract(|_, world| {
                world.run_schedule(RenderStartup);
                world.run_schedule(bevy::render::ExtractSchedule);
            });
            install_worker_schedule(
                &mut worker,
                submission,
                Arc::new(AtomicBool::new(false)),
                |_, _| {},
                |_| false,
            );
            worker.extract(&mut World::new());
            worker.update();
        });
        worker.join().unwrap();
    }

    fn original_queue_processor(mut pipelines: ResMut<CpuPipelines>) {
        pipelines.processed_in_render += 1;
    }

    fn prepare_cpu_pipeline(mut pipelines: ResMut<CpuPipelines>) {
        pipelines.prepares += 1;
    }

    fn draw_cpu_frame(world: &mut World) {
        assert!(world.resource::<SharedSubmission>().0.try_lock().is_err());
        let mut pipelines = world.resource_mut::<CpuPipelines>();
        pipelines.draws += 1;
        if pipelines.draws == 1 {
            // A same-ID reload has no pipeline-count change; it still needs a
            // later draw after compilation rather than publishing this frame.
            pipelines.pending_same_id = true;
            let holds = world.resource::<Holds>();
            holds.gpu_entered.send(()).unwrap();
            holds
                .gpu_release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }
    }

    fn prewarm_cpu_pipeline(_: &mut World, world: &mut World) {
        assert!(world.resource::<SharedSubmission>().0.try_lock().is_ok());
        world.resource_mut::<CpuPipelines>().prewarms += 1;
    }

    fn compile_cpu_pipeline(world: &mut World) -> bool {
        let mut pipelines = world.resource_mut::<CpuPipelines>();
        pipelines.compiles += 1;
        let changed = std::mem::take(&mut pipelines.pending_same_id);
        if pipelines.compiles == 1 {
            let holds = world.resource::<Holds>();
            holds.compile_entered.send(()).unwrap();
            holds
                .compile_release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }
        changed
    }

    #[test]
    fn stopped_worker_cancels_extraction_and_render_waits() {
        for cancel_extraction in [true, false] {
            let submission = SharedSubmission::default();
            let stopped = Arc::new(AtomicBool::new(false));
            // The interface still owns its last surface operation when it
            // joins the worker. Shutdown must finish without releasing this.
            let interface_guard = submission.0.lock().unwrap();
            let (started, attempting) = mpsc::channel();
            let (finished, completion) = mpsc::channel();
            let worker_submission = submission.clone();
            let worker_stopped = stopped.clone();
            let thread = thread::spawn(move || {
                let mut worker = SubApp::new();
                worker.update_schedule = Some(OriginalWorkerUpdate.intern());
                worker.init_resource::<CpuPipelines>();
                worker.add_systems(
                    OriginalWorkerUpdate,
                    |mut pipelines: ResMut<CpuPipelines>| pipelines.draws += 1,
                );
                worker.set_extract(|_, world| {
                    world.resource_mut::<CpuPipelines>().extracts += 1;
                });
                install_worker_schedule(
                    &mut worker,
                    worker_submission,
                    worker_stopped,
                    |_, world| world.resource_mut::<CpuPipelines>().prewarms += 1,
                    |world| {
                        world.resource_mut::<CpuPipelines>().compiles += 1;
                        false
                    },
                );
                started.send(()).unwrap();
                if cancel_extraction {
                    worker.extract(&mut World::new());
                }
                worker.update();
                finished
                    .send(worker.world().resource::<CpuPipelines>().clone())
                    .unwrap();
            });
            attempting.recv_timeout(Duration::from_secs(5)).unwrap();
            stopped.store(true, Ordering::Release);
            let result = completion.recv_timeout(Duration::from_secs(1));
            drop(interface_guard);
            thread.join().unwrap();
            let pipelines = result.expect("shutdown waited for the interface's GPU gate");
            assert_eq!(pipelines.extracts, 0);
            assert_eq!(pipelines.prewarms, 0);
            assert_eq!(pipelines.draws, 0);
            assert_eq!(pipelines.compiles, 0);
        }
    }

    #[test]
    fn worker_submission_and_compilation_preserve_surface_order_and_frame_readiness() {
        // This is a CPU schedule/concurrency contract, not native surface or
        // GPU qualification. Use the real installed window hooks with no window.
        let mut identity = World::new();
        identity.init_resource::<SurfaceOrder>();
        let mut identity_schedule = Schedule::default();
        identity_schedule.add_systems((
            guarded_window_system(original_cpu_surface, guarded_cpu_surface),
            (|_: Res<PreparedSurface>, mut order: ResMut<SurfaceOrder>| {
                order.0.push("prepared");
            })
            .after(original_cpu_surface),
        ));
        identity_schedule.run(&mut identity);
        assert_eq!(
            identity.resource::<SurfaceOrder>().0,
            ["guarded", "prepared"],
            "original identity must order deferred work without duplicate execution"
        );
        let original_set = original_cpu_surface.into_system_set().intern();
        let (_, guarded) = identity_schedule
            .systems_with_access()
            .unwrap()
            .find(|(_, system)| {
                system
                    .system()
                    .default_system_sets()
                    .contains(&original_set)
            })
            .unwrap();
        assert_eq!(
            guarded.system().get_last_run(),
            identity.resource::<PreparedSurface>().0,
            "reported last-run tick must belong to the executed guarded system"
        );

        let mut interface = SubApp::new();
        interface.update_schedule = Some(Render.intern());
        interface.add_schedule(Render::base_schedule());
        interface.add_systems(
            Render,
            create_surfaces
                .run_if(need_surface_configuration)
                .after(RenderSystems::ExtractCommands)
                .before(prepare_windows),
        );
        interface.add_systems(Render, prepare_windows.in_set(RenderSystems::PrepareViews));
        interface.init_resource::<SurfaceOrder>();
        interface.add_systems(
            Render,
            (
                (|mut order: ResMut<SurfaceOrder>| order.0.push("before")).before(create_surfaces),
                (|mut order: ResMut<SurfaceOrder>| order.0.push("between"))
                    .after(create_surfaces)
                    .before(prepare_windows),
                (|mut order: ResMut<SurfaceOrder>| order.0.push("after")).after(prepare_windows),
            ),
        );
        let submission = install_interface(&mut interface);
        interface
            .world_mut()
            .schedule_scope(Render, |world, schedule| {
                schedule.initialize(world).unwrap();
                for original in [
                    create_surfaces.into_system_set().intern(),
                    prepare_windows.into_system_set().intern(),
                ] {
                    let ids = schedule.graph().systems_in_set(original).unwrap();
                    assert_eq!(
                        ids.len(),
                        1,
                        "original window ordering set must stay populated"
                    );
                    let (_, system) = schedule
                        .systems_with_access()
                        .unwrap()
                        .find(|(key, _)| ids.contains(key))
                        .unwrap();
                    assert!(matches!(system.access(), SystemAccess::Shared(_)));
                    assert!(
                        !system.system().is_send(),
                        "guarded surface work must not enter the global pool while a CAD buffer scope holds the GPU gate"
                    );
                }
            });

        let (gpu_entered_tx, gpu_entered_rx) = mpsc::channel();
        let (gpu_release_tx, gpu_release_rx) = mpsc::channel();
        let (compile_entered_tx, compile_entered_rx) = mpsc::channel();
        let (compile_release_tx, compile_release_rx) = mpsc::channel();
        let mut worker = SubApp::new();
        worker.update_schedule = Some(OriginalWorkerUpdate.intern());
        worker.insert_resource(submission.clone());
        worker.init_resource::<CpuPipelines>();
        worker.insert_resource(Holds {
            gpu_entered: gpu_entered_tx,
            gpu_release: Mutex::new(gpu_release_rx),
            compile_entered: compile_entered_tx,
            compile_release: Mutex::new(compile_release_rx),
        });
        worker.add_schedule(Render::base_schedule());
        worker.add_systems(
            Render,
            (
                // Like upstream upscaling, this normal cache writer is in
                // Prepare and must remain when the Render processor is removed.
                prepare_cpu_pipeline.in_set(RenderSystems::Prepare),
                (original_queue_processor, draw_cpu_frame)
                    .chain()
                    .in_set(RenderSystems::Render),
            ),
        );
        worker.add_systems(OriginalWorkerUpdate, |world: &mut World| {
            world.run_schedule(Render)
        });
        worker.add_systems(RenderStartup, |world: &mut World| {
            assert!(world.resource::<SharedSubmission>().0.try_lock().is_err());
            world.resource_mut::<CpuPipelines>().startups += 1;
        });
        worker.set_extract(|main, world| {
            assert!(world.resource::<SharedSubmission>().0.try_lock().is_err());
            world.resource_mut::<CpuPipelines>().extracts += 1;
            world.run_schedule(RenderStartup);
            if main.contains_resource::<RequestExit>() {
                main.write_message(AppExit::error()).unwrap();
            }
        });
        remove_cache_processor::<CpuPipelines>(&mut worker);
        install_worker_schedule(
            &mut worker,
            submission.clone(),
            Arc::new(AtomicBool::new(false)),
            prewarm_cpu_pipeline,
            compile_cpu_pipeline,
        );
        let worker_thread = thread::spawn(move || {
            let mut main = World::new();
            main.init_resource::<Messages<AppExit>>();
            worker.extract(&mut main);
            worker.update();
            let first_changed = compiled_after_draw(worker.world());
            worker.update();
            let stable_changed = compiled_after_draw(worker.world());
            // An exit emitted by the delegated extract must skip all subsequent
            // prewarm, draw and compilation work rather than reviving a device.
            main.insert_resource(RequestExit);
            worker.extract(&mut main);
            worker.update();
            (
                worker.world().resource::<CpuPipelines>().clone(),
                first_changed,
                stable_changed,
                compiled_after_draw(worker.world()),
            )
        });
        let entered = gpu_entered_rx.recv_timeout(Duration::from_secs(5));

        let (configured_tx, configured_rx) = mpsc::channel();
        let (configure_during_compile_tx, configure_during_compile_rx) = mpsc::channel();
        let configure_submission = submission.clone();
        let configuration_thread = thread::spawn(move || {
            configure_submission.run(|| {
                assert!(configure_submission.0.try_lock().is_err());
                configured_tx.send(()).unwrap();
            });
            configure_during_compile_rx
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            configure_submission.run(|| configured_tx.send(()).unwrap());
        });

        let (interface_tx, interface_rx) = mpsc::channel();
        let interface_thread = thread::spawn(move || {
            // The actual empty-window conditions leave CPU interface work
            // independent. Real window acquisition intentionally takes the gate.
            interface.update();
            interface_tx
                .send(interface.world().resource::<SurfaceOrder>().0.clone())
                .unwrap();
        });
        let progressed = interface_rx.recv_timeout(Duration::from_secs(5));
        let configured_before_release = configured_rx.try_recv();
        // Release both bounded CPU phases and join owned threads before reporting
        // failures. Configuration must finish while compilation remains paused.
        let _ = gpu_release_tx.send(());
        let compiling = compile_entered_rx.recv_timeout(Duration::from_secs(5));
        let configured_after_gpu = configured_rx.recv_timeout(Duration::from_secs(5));
        let _ = configure_during_compile_tx.send(());
        let configured_during_compile = configured_rx.recv_timeout(Duration::from_secs(5));
        let _ = compile_release_tx.send(());
        let worker_result = worker_thread.join();
        let configuration_result = configuration_thread.join();
        let interface_result = interface_thread.join();
        assert!(entered.is_ok());
        assert!(compiling.is_ok());
        assert!(configured_after_gpu.is_ok());
        assert!(configured_during_compile.is_ok());
        configuration_result.unwrap();
        interface_result.unwrap();
        let (pipelines, first_changed, stable_changed, exited_changed) = worker_result.unwrap();
        assert_eq!(progressed.unwrap(), ["before", "between", "after"]);
        assert!(matches!(
            configured_before_release,
            Err(mpsc::TryRecvError::Empty)
        ));
        assert_eq!(pipelines.processed_in_render, 0);
        assert_eq!(pipelines.prepares, 2);
        assert_eq!(pipelines.draws, 2);
        assert_eq!(pipelines.extracts, 2);
        assert_eq!(pipelines.startups, 2);
        assert_eq!(pipelines.prewarms, 1);
        assert_eq!(pipelines.compiles, 2);
        assert!(first_changed);
        assert!(!stable_changed);
        assert!(!exited_changed);
        assert!(submission.0.try_lock().is_ok());
    }
}

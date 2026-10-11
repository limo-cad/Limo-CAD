use super::*;
use bevy::ecs::schedule::SingleThreadedExecutor;
use std::{sync::mpsc, time::Duration};

fn render_app() -> SubApp {
    let mut app = SubApp::new();
    let mut schedule = Render::base_schedule();
    schedule.set_executor(SingleThreadedExecutor::new());
    app.add_schedule(schedule);
    let mut graph = RenderGraph::base_schedule();
    graph.set_executor(SingleThreadedExecutor::new());
    app.add_schedule(graph);
    app
}

fn assert_leased(world: &mut World) {
    assert!(world.resource::<QueueLease>().0.is_some());
}

#[test]
fn shared_gpu_queue_guards_uploads_surfaces_and_submission_but_releases_for_compilation() {
    let queue = SharedQueue::default();
    let mut interface = render_app();
    install_interface(&mut interface, queue.clone());
    interface.add_systems(
        Render,
        (
            assert_leased.in_set(RenderSystems::PrepareAssets),
            assert_leased.in_set(RenderSystems::PrepareViews),
            (|world: &mut World| assert!(world.resource::<QueueLease>().0.is_none()))
                .in_set(RenderSystems::Render),
        ),
    );
    let mut worker = render_app();
    install_worker(&mut worker, queue);
    let (compiling, compilation_started) = mpsc::channel();
    let (resume, compilation_resumed) = mpsc::channel();
    let compilation_resumed = Mutex::new(compilation_resumed);
    worker.add_systems(
        Render,
        (
            assert_leased.in_set(RenderSystems::PrepareAssets),
            assert_leased.in_set(RenderSystems::PrepareResources),
            (move |world: &mut World| {
                assert!(world.resource::<QueueLease>().0.is_none());
                compiling.send(()).unwrap();
                compilation_resumed
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                world.run_schedule(RenderGraph);
                assert_leased(world);
            })
            .in_set(RenderSystems::Render)
            .before(render_system),
            (|world: &mut World| assert!(world.resource::<QueueLease>().0.is_none()))
                .in_set(RenderSystems::Cleanup),
        ),
    );
    worker.add_systems(
        RenderGraph,
        assert_leased.in_set(RenderGraphSystems::Submit),
    );
    let worker = std::thread::spawn(move || {
        worker.world_mut().run_schedule(Render);
        assert!(worker.world().resource::<QueueLease>().0.is_none());
    });
    compilation_started
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    // The interface must complete surface preparation while CAD compilation
    // is held, instead of taking the worker's compilation lock.
    interface.world_mut().run_schedule(Render);
    assert!(interface.world().resource::<QueueLease>().0.is_none());
    resume.send(()).unwrap();
    worker.join().unwrap();
}

#[test]
fn shared_gpu_queue_prevents_concurrent_submissions_and_releases_on_world_teardown() {
    let queue = SharedQueue::default();
    let mut world = World::new();
    world.insert_resource(queue.clone());
    world.init_resource::<QueueLease>();
    acquire(&mut world);
    let (started, waiting) = mpsc::channel();
    let (acquired, completion) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        started.send(()).unwrap();
        let _lease = queue.acquire();
        acquired.send(()).unwrap();
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(completion.recv_timeout(Duration::from_millis(20)).is_err());
    // Teardown also releases the queue after an interrupted render schedule.
    drop(world);
    completion.recv_timeout(Duration::from_secs(5)).unwrap();
    worker.join().unwrap();
}

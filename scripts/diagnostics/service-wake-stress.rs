// Instrumentation compiled only into a temporary copy of bevy_winit by
// service-wake-stress.py. Uses the real OS event loop with no native windows.
#[path = "service_wake.rs"]
mod service_wake;
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
};
use std::time::{Duration, Instant as Clock};

#[derive(Debug)]
enum ProbeEvent {
    Wake,
    Marker(Clock),
    Stop,
}
#[derive(Default)]
struct Counts {
    updates: AtomicUsize,
    progress: AtomicUsize,
    sent: AtomicUsize,
    delivered: AtomicUsize,
    dispatched: AtomicUsize,
    callbacks: AtomicUsize,
    markers: AtomicUsize,
    max_marker_us: AtomicUsize,
    generating: AtomicBool,
    idle_delta: AtomicUsize,
}
#[derive(Resource)]
struct Shared(Arc<Counts>);
#[derive(Resource)]
struct Scenario(String);
#[derive(Resource)]
struct Proxy(Arc<dyn Fn() + Send + Sync>);
struct Probe {
    runner: WinitAppRunnerState,
    counts: Arc<Counts>,
}
impl ApplicationHandler<ProbeEvent> for Probe {
    fn resumed(&mut self, _: &ActiveEventLoop) {
        // Intentionally no OS window: keep a visible ECS Window to exercise the
        // blocked-redraw gate, with no compositor redraws and no user UI changes.
    }
    fn new_events(&mut self, el: &ActiveEventLoop, cause: StartCause) {
        self.runner.new_events(el, cause);
    }
    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {
        panic!("probe must not create a native window");
    }
    fn user_event(&mut self, el: &ActiveEventLoop, event: ProbeEvent) {
        match event {
            ProbeEvent::Wake => {
                self.counts.delivered.fetch_add(1, SeqCst);
                let before = self.counts.updates.load(SeqCst);
                self.runner.user_event(el, WinitUserEvent::WakeUp);
                assert_eq!(
                    self.counts.updates.load(SeqCst),
                    before,
                    "a wake callback must not recursively execute an update"
                );
            }
            ProbeEvent::Marker(start) => {
                self.counts.markers.fetch_add(1, SeqCst);
                self.counts
                    .max_marker_us
                    .fetch_max(start.elapsed().as_micros() as usize, SeqCst);
            }
            ProbeEvent::Stop => el.exit(),
        }
    }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.counts.callbacks.fetch_add(1, SeqCst);
        let before = self.counts.updates.load(SeqCst);
        self.runner.about_to_wait(el);
        assert!(
            self.counts.updates.load(SeqCst) - before <= 1,
            "a single event-loop batch must not execute multiple updates"
        );
    }
}
pub fn run() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "burst".into());
    let mut builder = EventLoop::<ProbeEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        builder.with_activate_ignoring_other_apps(false);
    }
    let el = builder.build().unwrap();
    el.set_control_flow(ControlFlow::Wait);
    let proxy = el.create_proxy();
    let counts = Arc::new(Counts::default());
    let send_counts = counts.clone();
    let wake_proxy = proxy.clone();
    let send = move || {
        let alive = wake_proxy.send_event(ProbeEvent::Wake).is_ok();
        if alive {
            send_counts.dispatched.fetch_add(1, SeqCst);
        }
        alive
    };
    let limited = std::env::var_os("LIMIT_SERVICE_WAKES").is_some();
    let wake: Arc<dyn Fn() + Send + Sync> = if limited {
        let limiter = service_wake::ServiceWake::new(send).unwrap();
        Arc::new(move || limiter.request())
    } else {
        Arc::new(move || {
            assert!(send());
        })
    };
    let mut app = App::new();
    let update_mode = if mode == "continuous" {
        UpdateMode::Continuous
    } else {
        UpdateMode::Reactive {
            wait: Duration::MAX,
            react_to_device_events: false,
            react_to_user_events: mode != "disabled",
            react_to_window_events: true,
        }
    };
    app.insert_resource(WinitSettings {
        focused_mode: update_mode,
        unfocused_mode: update_mode,
    })
    .init_resource::<crate::WinitMonitors>()
    .insert_resource(Shared(counts.clone()))
    .insert_resource(Scenario(mode.clone()))
    .insert_resource(Proxy(wake.clone()));
    app.world_mut().spawn(Window {
        visible: true,
        focused: true,
        ..Default::default()
    });
    app.add_systems(bevy_app::First, |shared: Res<Shared>| {
        let n = shared.0.updates.fetch_add(1, SeqCst) + 1;
        if n >= 100_000 {
            shared.0.generating.store(false, SeqCst);
        }
    });
    // Match the review concern: ten distinct systems each send a wake per update.
    for _ in 0..10 {
        app.add_systems(
            bevy_app::Update,
            |shared: Res<Shared>, proxy: Res<Proxy>, scenario: Res<Scenario>| {
                if shared.0.generating.load(SeqCst)
                    || (scenario.0 == "ten-systems-once" && shared.0.updates.load(SeqCst) == 1)
                {
                    (proxy.0)();
                    shared.0.sent.fetch_add(1, SeqCst);
                }
            },
        );
    }
    app.add_systems(
        bevy_app::Update,
        |shared: Res<Shared>, proxy: Res<Proxy>, scenario: Res<Scenario>| {
            if scenario.0 == "single-system-once" && shared.0.updates.load(SeqCst) == 1 {
                for _ in 0..10 {
                    (proxy.0)();
                    shared.0.sent.fetch_add(1, SeqCst);
                }
            }
        },
    );
    app.add_systems(bevy_app::Last, |shared: Res<Shared>| {
        shared.0.progress.fetch_add(1, SeqCst);
    });
    app.finish();
    app.cleanup();
    let mut runner = WinitAppRunnerState::new(app);
    runner.lifecycle = AppLifecycle::Running;
    runner.previous_lifecycle = AppLifecycle::Running;
    runner.startup_forced_updates = 0;
    runner.ran_update_since_last_redraw = true;
    runner.update_mode = update_mode;
    let worker_counts = counts.clone();
    let worker_mode = mode.clone();
    let worker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        worker_counts
            .generating
            .store(worker_mode == "feedback", SeqCst);
        let n = if worker_mode == "feedback" || worker_mode.ends_with("-once") {
            1
        } else if worker_mode == "burst10" {
            10
        } else {
            10_000
        };
        for _ in 0..n {
            wake();
            worker_counts.sent.fetch_add(1, SeqCst);
        }
        for _ in 0..50 {
            proxy.send_event(ProbeEvent::Marker(Clock::now())).unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }
        worker_counts.generating.store(false, SeqCst);
        std::thread::sleep(Duration::from_millis(100));
        let idle_start = worker_counts.updates.load(SeqCst);
        std::thread::sleep(Duration::from_millis(250));
        worker_counts
            .idle_delta
            .store(worker_counts.updates.load(SeqCst) - idle_start, SeqCst);
        proxy.send_event(ProbeEvent::Stop).unwrap();
    });
    let start = Clock::now();
    let mut probe = Probe {
        runner,
        counts: counts.clone(),
    };
    el.run_app(&mut probe).unwrap();
    worker.join().unwrap();
    println!(
        "limited={limited} mode={mode} sent={} dispatched={} delivered={} updates={} last_schedule={} event_loop_batches={} markers={} max_marker_us={} idle_updates={} elapsed_ms={}",
        counts.sent.load(SeqCst),
        counts.dispatched.load(SeqCst),
        counts.delivered.load(SeqCst),
        counts.updates.load(SeqCst),
        counts.progress.load(SeqCst),
        counts.callbacks.load(SeqCst),
        counts.markers.load(SeqCst),
        counts.max_marker_us.load(SeqCst),
        counts.idle_delta.load(SeqCst),
        start.elapsed().as_millis()
    );
    assert_eq!(
        counts.dispatched.load(SeqCst),
        counts.delivered.load(SeqCst)
    );
    assert_eq!(counts.markers.load(SeqCst), 50);
    assert_eq!(counts.updates.load(SeqCst), counts.progress.load(SeqCst));
    assert_eq!(counts.idle_delta.load(SeqCst), 0);
    if limited && mode == "feedback" {
        assert!(
            counts.updates.load(SeqCst) <= start.elapsed().as_millis() as usize / 16 + 2,
            "service feedback exceeded its rate bound"
        );
    }
    if mode == "disabled" || std::env::var_os("EXPECT_NO_UPDATES").is_some() {
        assert_eq!(counts.updates.load(SeqCst), 0);
    } else {
        assert!(counts.updates.load(SeqCst) > 0);
    }
}

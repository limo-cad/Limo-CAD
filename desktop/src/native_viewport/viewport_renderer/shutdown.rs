use bevy::{
    app::{App, AppExit, Last},
    prelude::{Messages, Res, Resource},
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Resource)]
struct RendererStop(Arc<AtomicBool>);

pub(super) fn install(app: &mut App, stopped: Arc<AtomicBool>) {
    // Close receipts are finalized in PostUpdate. Cancel GPU gate waits before
    // final render extraction, which must finish before Worker can be dropped.
    app.insert_resource(RendererStop(stopped))
        .add_systems(Last, stop_on_exit);
}

fn stop_on_exit(exits: Res<Messages<AppExit>>, stopped: Res<RendererStop>) {
    if !exits.is_empty() {
        stopped.0.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        app::{AppLabel, PostUpdate, SubApp},
        prelude::*,
    };
    use std::sync::atomic::AtomicUsize;

    #[derive(AppLabel, Clone, Debug, PartialEq, Eq, Hash)]
    struct FinalRenderExtract;

    #[derive(Resource, Default)]
    struct CloseReceipt(bool);

    #[test]
    fn close_receipt_cancels_worker_before_final_extraction_without_dropping_app() {
        let stopped = Arc::new(AtomicBool::new(false));
        let extracted = Arc::new(AtomicBool::new(false));
        let mut app = App::new();
        app.add_message::<AppExit>();
        app.init_resource::<CloseReceipt>();
        install(&mut app, stopped.clone());
        app.add_systems(PostUpdate, |world: &mut World| {
            world.resource_mut::<CloseReceipt>().0 = true;
            world.write_message(AppExit::Success);
        });
        let mut render = SubApp::new();
        let worker_stopped = stopped.clone();
        let final_extract = extracted.clone();
        render.set_extract(move |main, _| {
            assert!(main.resource::<CloseReceipt>().0);
            assert!(worker_stopped.load(Ordering::Acquire));
            final_extract.store(true, Ordering::Release);
        });
        app.insert_sub_app(FinalRenderExtract, render);

        app.update();

        assert_eq!(app.should_exit(), Some(AppExit::Success));
        assert!(extracted.load(Ordering::Acquire));
    }

    #[test]
    fn ordinary_frames_keep_worker_running_and_extract_each_frame() {
        let stopped = Arc::new(AtomicBool::new(false));
        let extracted = Arc::new(AtomicUsize::new(0));
        let mut app = App::new();
        app.add_message::<AppExit>();
        install(&mut app, stopped.clone());
        let mut render = SubApp::new();
        let worker_stopped = stopped.clone();
        let frames = extracted.clone();
        render.set_extract(move |_, _| {
            assert!(!worker_stopped.load(Ordering::Acquire));
            frames.fetch_add(1, Ordering::AcqRel);
        });
        app.insert_sub_app(FinalRenderExtract, render);

        app.update();
        app.update();

        assert_eq!(extracted.load(Ordering::Acquire), 2);
        assert!(app.should_exit().is_none());
        assert!(!stopped.load(Ordering::Acquire));
    }
}

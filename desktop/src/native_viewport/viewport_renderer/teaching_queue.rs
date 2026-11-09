//! Two bounded demand slots. Main CAD work always wins at a scheduling boundary.
use super::*;
use crate::native_viewport::script_preview::{PreviewService, RenderJob};
use std::sync::Condvar;

#[derive(Default)]
struct Pending {
    main: Option<Snapshot>,
    teaching: Option<Box<RenderJob>>,
    closed: bool,
    retire_teaching: bool,
    #[cfg(test)]
    retirement_in_flight: bool,
}
#[derive(Default)]
pub(super) struct JobQueue {
    pending: Mutex<Pending>,
    changed: Condvar,
    #[cfg(test)]
    pub(super) inject_shader_failure: std::sync::atomic::AtomicU8,
    #[cfg(test)]
    pub(super) retired_scene: Mutex<Option<std::sync::Weak<SolidSceneDto>>>,
}
pub(super) enum Job {
    Main(Box<Snapshot>),
    Teaching(Box<RenderJob>),
    RetireTeaching,
}
impl JobQueue {
    pub(super) fn try_send(
        &self,
        snapshot: Snapshot,
    ) -> Result<(), mpsc::TrySendError<Box<Snapshot>>> {
        let mut pending = self.pending.lock().unwrap();
        if pending.closed {
            return Err(mpsc::TrySendError::Disconnected(Box::new(snapshot)));
        }
        if pending.main.is_some() {
            return Err(mpsc::TrySendError::Full(Box::new(snapshot)));
        }
        pending.main = Some(snapshot);
        self.changed.notify_one();
        Ok(())
    }
    pub(super) fn teaching(&self, job: Box<RenderJob>) -> Result<(), String> {
        let mut pending = self.pending.lock().map_err(|_| "CAD queue unavailable")?;
        if pending.closed {
            return Err("CAD renderer stopped".into());
        }
        // Close can race a main-priority deferral of an in-flight job. Never
        // put its document back behind a stream of main requests.
        if job.view_closed.load(Ordering::Acquire) {
            return Err("Feature preview closed".into());
        }
        // A changed view drops its receipt immediately. Reclaim that canceled
        // slot even when continuous main work prevents the worker draining it.
        if pending.teaching.as_ref().is_some_and(|queued| {
            !queued.cancelled.load(Ordering::Acquire) && std::time::Instant::now() < queued.deadline
        }) {
            return Err("Feature preview renderer is busy; try again".into());
        }
        pending.teaching = Some(job);
        self.changed.notify_one();
        Ok(())
    }
    pub(super) fn retire_closed(&self) {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if pending.closed {
            return;
        }
        if pending
            .teaching
            .as_ref()
            .is_some_and(|job| job.view_closed.load(Ordering::Acquire))
        {
            pending.teaching = None;
        }
        // One bit bounds any number of closes while the worker is busy.
        pending.retire_teaching = true;
        self.changed.notify_one();
    }
    pub(super) fn has_main(&self) -> bool {
        self.pending.lock().unwrap().main.is_some()
    }
    pub(super) fn next(&self) -> Option<Job> {
        let mut pending = self.pending.lock().unwrap();
        loop {
            if pending.closed {
                return None;
            }
            // Constant-time retirement cannot starve behind continuous main
            // work. It submits no teaching render and preserves main priority.
            if std::mem::take(&mut pending.retire_teaching) {
                #[cfg(test)]
                {
                    pending.retirement_in_flight = true;
                }
                return Some(Job::RetireTeaching);
            }
            if let Some(main) = pending.main.take() {
                return Some(Job::Main(Box::new(main)));
            }
            if let Some(teaching) = pending.teaching.take() {
                return Some(Job::Teaching(teaching));
            }
            pending = self.changed.wait(pending).unwrap();
        }
    }
    pub(super) fn close(&self) {
        // Teardown must still disconnect clients after a poisoned worker lock.
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        pending.closed = true;
        pending.main = None;
        pending.teaching = None;
        pending.retire_teaching = false;
        self.changed.notify_all();
    }

    #[cfg(test)]
    pub(super) fn finish_retirement(&self) {
        self.pending.lock().unwrap().retirement_in_flight = false;
    }

    #[cfg(test)]
    fn retirement_finished(&self) -> Result<bool, String> {
        let pending = self.pending.lock().map_err(|_| "CAD queue unavailable")?;
        if pending.closed {
            return Err("CAD renderer stopped".into());
        }
        Ok(!pending.retire_teaching && !pending.retirement_in_flight)
    }
}

/// Evidence must observe worker retirement, not just host texture removal.
#[cfg(test)]
pub(crate) fn teaching_retirement_finished(world: &World) -> Result<bool, String> {
    world
        .get_resource::<Worker>()
        .and_then(|worker| worker.requests.as_ref())
        .ok_or("CAD renderer is not ready")?
        .retirement_finished()
}

pub(crate) fn connect_preview(world: &World, service: &PreviewService) -> Result<(), String> {
    let queue = world
        .get_resource::<Worker>()
        .and_then(|worker| worker.requests.as_ref())
        .ok_or("CAD renderer is not ready")?;
    let render_queue = Arc::downgrade(queue);
    let retire_queue = Arc::downgrade(queue);
    service.connect(
        Arc::new(move |job| {
            render_queue
                .upgrade()
                .ok_or_else(|| "CAD renderer stopped".to_owned())?
                .teaching(job)
        }),
        Arc::new(move || {
            if let Some(queue) = retire_queue.upgrade() {
                queue.retire_closed();
            }
        }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    fn snapshot() -> Snapshot {
        Snapshot {
            stamp: Stamp {
                changed: [0; 8],
                size: UVec2::ONE,
                scale: 1.0,
            },
            owner: "live".into(),
            model: None,
            edge_cache: None,
            camera: None,
            preview: None,
            stock: None,
            palette: None,
            presentation: None,
            section: None,
            viewport: ViewportSizeResource {
                logical_width: 1.0,
                logical_height: 1.0,
            },
            retained_sessions: None,
        }
    }
    fn teaching() -> Box<RenderJob> {
        let frame = serde_json::from_value(serde_json::json!({"caption":"sample", "scene":{
            "bodies":[{"id":1,"name":"Stock","feature_id":2,"faces":[],"edges":[],"mesh":{
            "positions":[0,0,0, 10,0,0, 0,10,0],"normals":[0,0,1,0,0,1,0,0,1],"indices":[0,1,2]}}],"errors":[]}
        })).unwrap();
        let (reply, _) = mpsc::sync_channel(1);
        Box::new(RenderJob {
            view_closed: Arc::new(AtomicBool::new(false)),
            progress: 1.0,
            document: Arc::new(
                crate::native_viewport::script_preview::PreviewDocument::new(vec![frame]).unwrap(),
            ),
            request: crate::native_viewport::script_preview::RenderRequest {
                preview_id: "example".into(),
                view_id: "view".into(),
                revision: 1,
                frame_index: 0,
                width: 300,
                height: 176,
                yaw: 0.5,
                pitch: 0.5,
                palette: None,
            },
            cancelled: Arc::new(AtomicBool::new(false)),
            reply,
            deadline: Instant::now() + Duration::from_secs(20),
        })
    }
    #[test]
    fn native_teaching_queue_worker_exit_disconnects_pending_reply_and_rejects_work() {
        let queue = JobQueue::default();
        let (reply, response) = mpsc::sync_channel(1);
        let mut job = teaching();
        job.reply = reply;
        queue.teaching(job).unwrap();
        // This is the teardown performed after any worker outcome, including panic.
        queue.close();
        assert!(queue.retirement_finished().unwrap_err().contains("stopped"));
        assert!(matches!(
            response.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        assert!(queue.teaching(teaching()).unwrap_err().contains("stopped"));
        assert!(matches!(
            queue.try_send(snapshot()),
            Err(mpsc::TrySendError::Disconnected(_))
        ));
    }

    #[test]
    fn native_teaching_retirement_receipt_waits_for_flush_and_repeated_closes() {
        let queue = JobQueue::default();
        assert!(queue.retirement_finished().unwrap());
        queue.retire_closed();
        assert!(!queue.retirement_finished().unwrap());
        assert!(matches!(queue.next(), Some(Job::RetireTeaching)));
        assert!(
            !queue.retirement_finished().unwrap(),
            "dequeueing Close is not completion of the render-world flush"
        );
        // Another Close can arrive while the first extraction is in flight.
        queue.retire_closed();
        queue.retire_closed();
        queue.finish_retirement();
        assert!(!queue.retirement_finished().unwrap());
        assert!(matches!(queue.next(), Some(Job::RetireTeaching)));
        assert!(!queue.retirement_finished().unwrap());
        queue.finish_retirement();
        assert!(queue.retirement_finished().unwrap());
        // This is the caught-panic teardown, including a panic during Close.
        queue.retire_closed();
        assert!(matches!(queue.next(), Some(Job::RetireTeaching)));
        queue.close();
        assert!(queue.retirement_finished().unwrap_err().contains("stopped"));
    }

    #[test]
    fn native_teaching_queue_bounds_work_and_prioritizes_main() {
        let queue = JobQueue::default();
        queue.teaching(teaching()).unwrap();
        assert!(queue.teaching(teaching()).is_err());
        assert!(queue.try_send(snapshot()).is_ok());
        assert!(matches!(
            queue.try_send(snapshot()),
            Err(mpsc::TrySendError::Full(_))
        ));
        assert!(matches!(queue.next(), Some(Job::Main(_))));
        let Some(Job::Teaching(job)) = queue.next() else {
            panic!("missing teaching")
        };
        let deadline = job.deadline;
        // Deferral retains the request's original deadline and bounded slot.
        queue.teaching(job).unwrap();
        assert!(queue.try_send(snapshot()).is_ok());
        assert!(matches!(queue.next(), Some(Job::Main(_))));
        let Some(Job::Teaching(job)) = queue.next() else {
            panic!("missing teaching")
        };
        assert_eq!(job.deadline, deadline);
        job.cancelled.store(true, Ordering::Release);
        assert!(crate::native_viewport::script_preview::check_request(
            &job.cancelled,
            job.deadline
        )
        .is_err());
        queue.close();
        assert!(queue.next().is_none());
        assert!(queue.teaching(teaching()).is_err());
        assert!(matches!(
            queue.try_send(snapshot()),
            Err(mpsc::TrySendError::Disconnected(_))
        ));
    }
    #[test]
    fn native_teaching_queue_replaces_cancelled_work_while_main_is_pending() {
        let queue = JobQueue::default();
        let (reply, response) = mpsc::sync_channel(1);
        let mut obsolete = teaching();
        obsolete.reply = reply;
        let cancelled = obsolete.cancelled.clone();
        queue.teaching(obsolete).unwrap();
        assert!(queue.try_send(snapshot()).is_ok());
        cancelled.store(true, Ordering::Release);
        let mut current = teaching();
        current.request.revision = 2;
        queue.teaching(current).unwrap();
        assert!(matches!(
            response.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        assert!(matches!(queue.next(), Some(Job::Main(_))));
        let Some(Job::Teaching(current)) = queue.next() else {
            panic!("missing current job")
        };
        assert_eq!(current.request.revision, 2);
        assert!(!current.cancelled.load(Ordering::Acquire));
    }

    #[test]
    fn native_teaching_queue_close_wakes_parked_worker() {
        let queue = Arc::new(JobQueue::default());
        let worker = queue.clone();
        let (finished, completion) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            assert!(worker.next().is_none());
            finished.send(()).unwrap();
        });
        queue.close();
        completion.recv_timeout(Duration::from_secs(2)).unwrap();
        thread.join().unwrap();
    }

    #[test]
    fn native_teaching_close_releases_queued_document_before_continuous_main_work() {
        let queue = JobQueue::default();
        let (reply, response) = mpsc::sync_channel(1);
        let mut job = teaching();
        job.reply = reply;
        let document = Arc::downgrade(&job.document);
        let closed = Arc::clone(&job.view_closed);
        queue.teaching(job).unwrap();
        queue.try_send(snapshot()).unwrap();
        closed.store(true, Ordering::Release);
        for _ in 0..1000 {
            queue.retire_closed();
        }
        assert!(
            document.upgrade().is_none(),
            "Close must drop the queued CPU payload immediately"
        );
        assert!(matches!(
            response.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        assert!(matches!(queue.next(), Some(Job::RetireTeaching)));
        assert!(matches!(queue.next(), Some(Job::Main(_))));
        assert!(
            !queue.pending.lock().unwrap().retire_teaching,
            "Closes coalesce into one demand wake"
        );
        queue.close();
        assert!(queue.next().is_none());
    }

    #[test]
    fn native_teaching_close_fences_deferred_work_without_cancelling_a_new_view() {
        let queue = JobQueue::default();
        let old = teaching();
        let old_document = Arc::downgrade(&old.document);
        old.view_closed.store(true, Ordering::Release);
        queue.retire_closed();
        assert!(queue.teaching(old).unwrap_err().contains("closed"));
        assert!(old_document.upgrade().is_none());
        let current = teaching();
        let closed = Arc::clone(&current.view_closed);
        queue.teaching(current).unwrap();
        queue.retire_closed();
        assert!(matches!(queue.next(), Some(Job::RetireTeaching)));
        let Some(Job::Teaching(current)) = queue.next() else {
            panic!("new view lost")
        };
        assert!(Arc::ptr_eq(&current.view_closed, &closed));
        assert!(!closed.load(Ordering::Acquire));
    }

    #[test]
    fn native_teaching_retirement_wakes_a_parked_worker_without_a_render_request() {
        let queue = Arc::new(JobQueue::default());
        let worker = Arc::clone(&queue);
        let (finished, completion) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            assert!(matches!(worker.next(), Some(Job::RetireTeaching)));
            finished.send(()).unwrap();
            assert!(worker.next().is_none());
        });
        queue.retire_closed();
        completion.recv_timeout(Duration::from_secs(2)).unwrap();
        queue.close();
        thread.join().unwrap();
    }
}

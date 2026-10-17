//! Two bounded demand slots. Main CAD work always wins at a scheduling boundary.
use super::*;
use crate::native_viewport::script_preview::{PreviewService, RenderJob};
use std::sync::Condvar;

#[derive(Default)]
struct Pending {
    main: Option<Snapshot>,
    teaching: Option<Box<RenderJob>>,
    closed: bool,
}
#[derive(Default)]
pub(super) struct JobQueue {
    pending: Mutex<Pending>,
    changed: Condvar,
}
pub(super) enum Job {
    Main(Box<Snapshot>),
    Teaching(Box<RenderJob>),
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
        if pending.teaching.is_some() {
            return Err("Feature preview renderer is busy; try again".into());
        }
        pending.teaching = Some(job);
        self.changed.notify_one();
        Ok(())
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
        self.changed.notify_all();
    }
}

pub(crate) fn connect_preview(world: &World, service: &PreviewService) -> Result<(), String> {
    let queue = world
        .get_resource::<Worker>()
        .and_then(|worker| worker.requests.as_ref())
        .ok_or("CAD renderer is not ready")?;
    let queue = Arc::downgrade(queue);
    service.connect(Arc::new(move |job| {
        queue
            .upgrade()
            .ok_or_else(|| "CAD renderer stopped".to_owned())?
            .teaching(job)
    }));
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
}

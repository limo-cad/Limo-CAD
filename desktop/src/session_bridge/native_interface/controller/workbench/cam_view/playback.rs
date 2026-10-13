//! One bounded stock worker per owned CAM view. Only a completed stock frame
//! advances the displayed cutter/path clock; slow simulation never lets the
//! cutter run ahead of its stock. All cutting remains in the shared kernel.
use super::*;
use std::time::{Duration, Instant};

pub(super) struct Frame {
    pub ticket: u64,
    pub time: f64,
    pub stock: Option<ViewportCamStock>,
    pub stock_revision: u64,
}
struct Request {
    ticket: u64,
    time: f64,
}
pub(super) struct Player {
    cancellation: CamSimulationCancellation,
    send: mpsc::SyncSender<Request>,
    receive: Mutex<mpsc::Receiver<Result<Frame, String>>>,
    pub busy: bool,
    pub ticket: u64,
    pub playing: bool,
    pub requested: Option<f64>,
    pub frame: Option<Frame>,
    pub speed: f64,
    clock: Instant,
}
impl Drop for Player {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}
impl Player {
    pub fn new(
        document: CamDocumentDto,
        request: CamSimulationRequestDto,
        wake: Option<NativeInterfaceHandle>,
    ) -> Result<Self, String> {
        Self::spawn(
            move |cancel| {
                limo_cad_cam::CamPlayback::new(document, request, 0., Some(cancel))
                    .map_err(|error| error.to_string())
            },
            wake,
        )
    }

    /// The NC preparation worker has already verified the complete program.
    /// Transfer its owned stock kernel, without parsing or simulating it again.
    pub fn from_prepared(
        kernel: limo_cad_cam::CamPlayback,
        wake: Option<NativeInterfaceHandle>,
    ) -> Result<Self, String> {
        Self::spawn(move |_| Ok(kernel), wake)
    }

    fn spawn(
        prepare: impl FnOnce(&CamSimulationCancellation) -> Result<limo_cad_cam::CamPlayback, String>
            + Send
            + 'static,
        wake: Option<NativeInterfaceHandle>,
    ) -> Result<Self, String> {
        let cancellation = CamSimulationCancellation::default();
        let cancel = cancellation.clone();
        let (send, requests) = mpsc::sync_channel::<Request>(1);
        let (results, receive) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("cad-native-cam-playback".into())
            .spawn(move || {
                let run = || -> Result<(), String> {
                    let mut kernel = prepare(&cancel)?;
                    let mut stock = None;
                    let mut stock_revision = 0;
                    while let Ok(request) = requests.recv() {
                        if cancel.is_cancelled() {
                            break;
                        }
                        let started = Instant::now();
                        let simulation = kernel
                            .sample(request.time, Some(&cancel))
                            .map_err(|e| e.to_string())?;
                        if simulation.stock_mesh.is_some() {
                            stock = crate::retained_cam_stock(&simulation).map(|stock| {
                                ViewportCamStock {
                                    time_seconds: Some(request.time),
                                    ..stock
                                }
                            });
                            stock_revision += 1;
                        }
                        let cadence = Duration::from_millis(33);
                        if started.elapsed() < cadence {
                            std::thread::sleep(cadence.saturating_sub(started.elapsed()));
                        }
                        if cancel.is_cancelled() {
                            break;
                        }
                        results
                            .send(Ok(Frame {
                                ticket: request.ticket,
                                time: simulation.estimated_seconds,
                                stock: stock.clone(),
                                stock_revision,
                            }))
                            .map_err(|_| "CAM playback closed".to_string())?;
                        if let Some(wake) = &wake {
                            wake.request_redraw();
                        }
                    }
                    Ok(())
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
                    .unwrap_or_else(|_| Err("CAM playback worker stopped unexpectedly".into()));
                if let Err(error) = result {
                    let _ = results.send(Err(error));
                    if let Some(wake) = &wake {
                        wake.request_redraw();
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            cancellation,
            send,
            receive: Mutex::new(receive),
            busy: false,
            ticket: 0,
            playing: false,
            requested: Some(0.),
            frame: None,
            speed: 1.,
            clock: Instant::now(),
        })
    }

    pub fn time(&self) -> f64 {
        self.frame.as_ref().map_or(0., |f| f.time)
    }

    pub fn seek(&mut self, time: f64, duration: f64) {
        self.ticket = self.ticket.wrapping_add(1);
        self.playing = false;
        self.requested = Some(time.clamp(0., duration));
        self.clock = Instant::now();
    }

    pub fn toggle(&mut self, start: f64, duration: f64) {
        self.ticket = self.ticket.wrapping_add(1);
        self.playing = !self.playing;
        self.requested = self.playing.then(|| {
            if self.time() >= duration {
                start
            } else {
                self.requested.unwrap_or_else(|| self.time()).max(start)
            }
        });
        self.clock = Instant::now();
    }

    /// Returns true only when a matching frame is ready to be presented.
    pub fn poll(&mut self, duration: f64) -> Result<bool, String> {
        let received = self.receive.lock().unwrap().try_recv();
        let mut changed = false;
        match received {
            Ok(result) => {
                self.busy = false;
                let frame = result?;
                if frame.ticket == self.ticket {
                    if frame.time >= duration {
                        self.playing = false;
                    }
                    self.frame = Some(frame);
                    changed = true;
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("CAM playback worker stopped".into())
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if !self.busy {
            let next = self.requested.take().or_else(|| {
                self.playing.then(|| {
                    self.time()
                        + self.clock.elapsed().as_secs_f64().clamp(1. / 30., 0.25) * self.speed
                })
            });
            if let Some(time) = next {
                self.clock = Instant::now();
                self.send
                    .try_send(Request {
                        ticket: self.ticket,
                        time: time.min(duration),
                    })
                    .map_err(|_| "CAM playback worker is unavailable".to_string())?;
                self.busy = true;
            }
        }
        Ok(changed)
    }
}

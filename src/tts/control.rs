//! Cooperative execution control shared by HTTP admission and native workers.
use serde::Serialize;
use std::{
    cell::RefCell,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub stage: String,
    pub frames: usize,
    pub elapsed_secs: f64,
}

#[derive(Debug)]
pub struct InferenceControl {
    cancelled: AtomicBool,
    started: Instant,
    deadline: Instant,
    progress: Mutex<Progress>,
}

impl InferenceControl {
    pub fn new(timeout: Duration) -> Arc<Self> {
        let started = Instant::now();
        Arc::new(Self {
            cancelled: AtomicBool::new(false),
            started,
            deadline: started + timeout,
            progress: Mutex::new(Progress {
                stage: "queued".into(),
                frames: 0,
                elapsed_secs: 0.0,
            }),
        })
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn stopped(&self) -> bool {
        self.cancelled.load(Ordering::Acquire) || Instant::now() >= self.deadline
    }
    pub fn update(&self, stage: &str, frames: usize) -> bool {
        if stage != "acoustic" {
            let mut p = self.progress.lock().unwrap_or_else(|e| e.into_inner());
            p.stage = stage.into();
            p.frames = frames;
            p.elapsed_secs = self.started.elapsed().as_secs_f64();
        }
        !self.stopped()
    }
    pub fn snapshot(&self) -> Progress {
        let mut p = self
            .progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        p.elapsed_secs = self.started.elapsed().as_secs_f64();
        p
    }
}
thread_local! { static CURRENT: RefCell<Option<Arc<InferenceControl>>> = const { RefCell::new(None) }; }
pub fn current() -> Option<Arc<InferenceControl>> {
    CURRENT.with(|c| c.borrow().clone())
}
pub fn scope<T>(control: Arc<InferenceControl>, work: impl FnOnce() -> T) -> T {
    struct Restore(Option<Arc<InferenceControl>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|c| {
                c.replace(self.0.take());
            });
        }
    }
    let _restore = Restore(CURRENT.with(|c| c.replace(Some(control))));
    work()
}

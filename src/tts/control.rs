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
pub struct ExecutionContext {
    pub provider: String,
    pub model: String,
    pub voice: String,
    pub effective_device: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub context: Option<ExecutionContext>,
    pub stage: String,
    pub frames: usize,
    pub elapsed_secs: f64,
    pub audio_secs: Option<f64>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub stage_secs: std::collections::BTreeMap<String, f64>,
}

#[derive(Debug)]
pub struct InferenceControl {
    pub id: String,
    cancelled: AtomicBool,
    started: Instant,
    deadline: Instant,
    progress: Mutex<Progress>,
}

impl InferenceControl {
    pub fn new(timeout: Duration) -> Arc<Self> {
        let started = Instant::now();
        Arc::new(Self {
            id: uuid::Uuid::new_v4().to_string(),
            cancelled: AtomicBool::new(false),
            started,
            deadline: started + timeout.min(Duration::from_secs(86400)),
            progress: Mutex::new(Progress {
                context: None,
                stage: "queued".into(),
                frames: 0,
                elapsed_secs: 0.0,
                audio_secs: None,
                updated_at: chrono::Utc::now(),
                stage_secs: std::collections::BTreeMap::new(),
            }),
        })
    }
    pub fn set_audio_duration(&self, seconds: f64) {
        if seconds.is_finite() && seconds > 0.0 {
            self.progress
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .audio_secs = Some(seconds);
        }
    }
    pub fn set_context(&self, provider: &str, model: &str, voice: &str, device: &str) {
        self.progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .context = Some(ExecutionContext {
            provider: provider.into(),
            model: model.into(),
            voice: voice.into(),
            effective_device: device.into(),
        });
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn cancellation_requested(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    pub fn deadline_exceeded(&self) -> bool {
        Instant::now() >= self.deadline
    }
    pub fn stopped(&self) -> bool {
        self.cancelled.load(Ordering::Acquire) || Instant::now() >= self.deadline
    }
    pub fn update(&self, stage: &str, frames: usize) -> bool {
        if stage != "acoustic" {
            let mut p = self.progress.lock().unwrap_or_else(|e| e.into_inner());
            let elapsed = self.started.elapsed().as_secs_f64();
            let previous_stage = p.stage.clone();
            let delta = elapsed - p.elapsed_secs;
            *p.stage_secs.entry(previous_stage).or_default() += delta;
            p.stage = stage.into();
            p.frames = frames;
            p.elapsed_secs = elapsed;
            p.updated_at = chrono::Utc::now();
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

/// Model serialization without an uninterruptible mutex wait. Lock ordering:
/// lifecycle -> model/worker -> short-lived state/config locks.
pub fn lock_cancellable<T>(mutex: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, String> {
    loop {
        if current().is_some_and(|c| c.stopped()) {
            return Err("inference cancelled or deadline exceeded".into());
        }
        match mutex.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(std::sync::TryLockError::Poisoned(_)) => return Err("model lock poisoned".into()),
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(Duration::from_millis(10))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_and_deadline_are_observed_at_checkpoints() {
        let control = InferenceControl::new(Duration::from_secs(1));
        assert!(control.update("generating", 1));
        control.cancel();
        assert!(!control.update("generating", 2));
        let expired = InferenceControl::new(Duration::ZERO);
        assert!(expired.stopped());
    }
    #[test]
    fn scoped_control_is_restored_after_panics() {
        let outer = InferenceControl::new(Duration::from_secs(1));
        scope(outer.clone(), || {
            let _ = std::panic::catch_unwind(|| {
                scope(InferenceControl::new(Duration::ZERO), || {
                    panic!("test panic")
                })
            });
            assert!(Arc::ptr_eq(&current().unwrap(), &outer));
        });
        assert!(current().is_none());
    }
}

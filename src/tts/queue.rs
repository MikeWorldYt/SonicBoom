use super::devices::{ActiveOutputDevice, OutputDeviceList};
use anyhow::Result;
use rodio::{Decoder, OutputStream, Sink};
use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use tokio::sync::oneshot;

/// Default cap on items waiting in the playback queue.
pub const DEFAULT_MAX_QUEUE_ITEMS: usize = 100;

/// Failure to perform a playback operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AudioManagerError {
    /// The playback queue is full; the caller should respond `429` and the
    /// caller-owned temp file (if any) must be deleted by the caller.
    #[error("playback queue is full")]
    QueueFull,
    /// The audio thread is dead (command channel closed); the caller should
    /// respond `503` rather than reporting success.
    #[error("audio playback system unavailable")]
    Unavailable,
    /// The selected output device is missing or cannot be opened, or a sink
    /// cannot be created on the live stream. The thread is alive and the
    /// selection/queue are retained; the caller should respond `503`.
    #[error("audio output unavailable")]
    OutputUnavailable,
    /// The audio file cannot be opened or decoded. Caller-owned files (queue
    /// API) map to `422`; server-generated files (TTS play) map to `500`.
    #[error("audio file is invalid or undecodable")]
    InvalidAudio,
    /// The requested output device does not exist; the caller should respond
    /// `400` and must not fall back to another device.
    #[error("unknown audio output device")]
    UnknownDevice,
    /// OS device enumeration failed; the caller should respond `500`.
    #[error("audio device enumeration failed")]
    DeviceError,
}

/// Represents an audio item in the queue
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AudioItem {
    /// Unique identifier for the audio item
    pub id: String,
    /// Path to the audio file
    pub path: PathBuf,
}

/// State for the audio queue system
pub struct AudioQueue {
    /// Queue of audio items waiting to be played (bounded by `max_items`).
    queue: VecDeque<AudioItem>,
    /// Current playing item
    current: Option<AudioItem>,
    /// Whether playback is paused
    paused: bool,
    /// Volume level (0.0 to 1.0)
    volume: f32,
    /// Hard cap on waiting items; enqueue beyond this is rejected.
    max_items: usize,
}

impl AudioQueue {
    /// Create a new audio queue with the default bound.
    pub fn new() -> Self {
        Self::with_max_items(DEFAULT_MAX_QUEUE_ITEMS)
    }

    /// Create a new audio queue holding at most `max_items` waiting items.
    pub fn with_max_items(max_items: usize) -> Self {
        Self {
            queue: VecDeque::new(),
            current: None,
            paused: false,
            volume: 1.0,
            max_items: max_items.max(1),
        }
    }

    /// Add an audio item to the queue, rejecting when the bound is reached.
    /// Existing entries are never dropped to make room.
    pub fn enqueue(&mut self, item: AudioItem) -> Result<(), AudioManagerError> {
        if self.queue.len() >= self.max_items {
            return Err(AudioManagerError::QueueFull);
        }
        self.queue.push_back(item);
        Ok(())
    }

    /// Remove and return the next item from the queue
    pub fn dequeue(&mut self) -> Option<AudioItem> {
        self.queue.pop_front()
    }

    /// Requeue an item at the front. Used when playback stalls on output
    /// failure — the item itself is not at fault, so it keeps its place
    /// instead of being dropped or temp-cleaned. A requeued item was already
    /// admitted, so it never violates the bound.
    pub fn push_front(&mut self, item: AudioItem) {
        self.queue.push_front(item);
    }

    /// Get the current item being played
    pub fn current(&self) -> Option<&AudioItem> {
        self.current.as_ref()
    }

    /// Set the current item
    pub fn set_current(&mut self, item: Option<AudioItem>) {
        self.current = item;
    }

    /// Take the current item, leaving none.
    pub fn take_current(&mut self) -> Option<AudioItem> {
        self.current.take()
    }

    /// Drain all waiting items (used to clean temp files on stop/replace).
    pub fn drain_waiting(&mut self) -> Vec<AudioItem> {
        self.queue.drain(..).collect()
    }

    /// Get queue length
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn max_items(&self) -> usize {
        self.max_items
    }

    /// Set paused state
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Update the queue bound. Existing items are
    /// never dropped to enforce a smaller cap; the
    /// new bound applies to future admissions.
    pub fn set_max_items(&mut self, max_items: usize) {
        self.max_items = max_items.max(1);
    }

    /// Check if playback is paused
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Set volume
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }

    /// Get volume
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Clear the queue (drops references without deleting files; use
    /// [`clear_queue_with_cleanup`] when temp files may be involved).
    pub fn clear(&mut self) {
        self.queue.clear();
        self.current = None;
    }
}

impl Default for AudioQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Commands for the audio thread
#[derive(Debug)]
pub enum AudioCommand {
    /// Add to queue (replies with admission decision: bound enforcement).
    ///
    /// `temporary` marks a server-generated file owned by SonicBoom: the
    /// audio thread registers ownership atomically on admission, before the
    /// item can complete, be evicted, stopped, or interrupted. Caller-owned
    /// library files must always send `temporary: false` so cleanup never
    /// deletes them.
    Enqueue {
        id: String,
        path: PathBuf,
        temporary: bool,
        reply: oneshot::Sender<Result<(), AudioManagerError>>,
    },
    /// Play immediately (clears queue). Replies once the playback thread
    /// has attempted to start playback — never a blind "sent" success.
    ///
    /// `temporary` has the same ownership meaning as in [`AudioCommand::Enqueue`].
    PlayNow {
        id: String,
        path: PathBuf,
        temporary: bool,
        reply: oneshot::Sender<Result<(), AudioManagerError>>,
    },
    /// Play next in queue
    PlayNext,
    /// Pause
    Pause,
    /// Resume
    Resume,
    /// Stop and clear queue
    Stop,
    /// Set volume
    SetVolume(f32),
    /// Update the queue bound (hot config reload).
    SetMaxQueueItems {
        max_items: usize,
        reply: oneshot::Sender<()>,
    },
    /// Register a temp file for cleanup after playback.
    ///
    /// Retained for backward compatibility with external callers that admit
    /// caller-owned files and register them separately. The server-generated
    /// playback path no longer uses this split protocol: it sends
    /// `temporary: true` on admission instead (see [`AudioManager::add_to_queue_temp`]
    /// and [`AudioManager::play_now_temp`]).
    RegisterTemp { path: PathBuf },
    /// Get status (returns sender for response)
    GetStatus(oneshot::Sender<QueueStatus>),
    /// List output devices. The audio thread enumerates; backend failure
    /// replies [`AudioManagerError::DeviceError`] (the API reports `500`,
    /// never an empty success).
    GetOutputDevices {
        reply: oneshot::Sender<Result<OutputDeviceList, AudioManagerError>>,
    },
    /// Report the configured selection versus live hardware state.
    GetOutputDevice {
        reply: oneshot::Sender<ActiveOutputDevice>,
    },
    /// Switch the output device. The switch commits only if the new stream
    /// opens; unknown devices reply [`AudioManagerError::UnknownDevice`]
    /// without touching the active output.
    SetOutputDevice {
        device: String,
        reply: oneshot::Sender<Result<ActiveOutputDevice, AudioManagerError>>,
    },
}

/// Thread-safe audio manager that runs rodio in a separate thread
pub struct AudioManager {
    /// Command sender for the audio thread
    command_tx: tokio::sync::mpsc::Sender<AudioCommand>,
}

impl AudioManager {
    /// Create a new audio manager and start the audio thread.
    /// `max_queue_items` bounds the waiting queue; `0` means the default.
    /// The initial output selection follows the OS default device.
    pub fn new(max_queue_items: usize) -> Result<Self> {
        Self::with_output_device(
            max_queue_items,
            super::devices::DEFAULT_DEVICE_ID.to_string(),
        )
    }

    /// Create a new audio manager with an explicit initial output selection
    /// (`default` or a device name; see `AUDIO_OUTPUT_DEVICE`).
    ///
    /// A missing device never fails startup: the selection is retained, a
    /// warning is logged, and the thread retries whenever playback is
    /// attempted (only configuration *syntax* errors fail startup).
    pub fn with_output_device(max_queue_items: usize, output_device: String) -> Result<Self> {
        let max_queue_items = if max_queue_items == 0 {
            DEFAULT_MAX_QUEUE_ITEMS
        } else {
            max_queue_items
        };
        let initial_device = super::devices::normalize_selection(&output_device);
        let (command_tx, command_rx) = tokio::sync::mpsc::channel(100);

        // Spawn the audio thread
        std::thread::spawn(move || {
            audio_thread(command_rx, max_queue_items, initial_device);
        });

        Ok(Self { command_tx })
    }

    /// Build a manager over an explicit command channel. Used by tests to
    /// simulate a dead audio thread (dropped receiver) or scripted replies
    /// without audio hardware.
    pub fn for_test(command_tx: tokio::sync::mpsc::Sender<AudioCommand>) -> Self {
        Self { command_tx }
    }

    /// Add a caller-owned audio file to the queue (`temporary: false`).
    ///
    /// Returns [`AudioManagerError::QueueFull`] when the bound is reached
    /// (the caller must delete any caller-owned temp file) or
    /// [`AudioManagerError::Unavailable`] when the audio thread is dead.
    /// The queued file is never deleted by playback cleanup.
    pub async fn add_to_queue(&self, id: String, path: PathBuf) -> Result<(), AudioManagerError> {
        self.enqueue_with_ownership(id, path, false).await
    }

    /// Add a server-generated temporary file to the queue. Ownership is
    /// registered atomically inside the audio thread on admission, so no
    /// separate [`AudioManager::register_temp`] call is needed. On rejection
    /// the file is *not* registered and the caller must delete it.
    pub async fn add_to_queue_temp(
        &self,
        id: String,
        path: PathBuf,
    ) -> Result<(), AudioManagerError> {
        self.enqueue_with_ownership(id, path, true).await
    }

    async fn enqueue_with_ownership(
        &self,
        id: String,
        path: PathBuf,
        temporary: bool,
    ) -> Result<(), AudioManagerError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(AudioCommand::Enqueue {
                id,
                path,
                temporary,
                reply: tx,
            })
            .await
            .map_err(|_| AudioManagerError::Unavailable)?;
        rx.await.map_err(|_| AudioManagerError::Unavailable)?
    }

    /// Play a caller-owned audio file immediately (clears queue,
    /// `temporary: false`).
    ///
    /// Completes only after the playback thread has attempted to start
    /// playback: `Ok` means playback started, [`AudioManagerError::OutputUnavailable`]
    /// means no output stream/sink, [`AudioManagerError::InvalidAudio`] means
    /// the file cannot be opened/decoded.
    pub async fn play_now(&self, id: String, path: PathBuf) -> Result<(), AudioManagerError> {
        self.play_now_with_ownership(id, path, false).await
    }

    /// Play a server-generated temporary file immediately. Ownership is
    /// registered atomically inside the audio thread before playback starts,
    /// so no separate [`AudioManager::register_temp`] call is needed. On
    /// failure the file is *not* left registered and the caller must delete
    /// it; on success cleanup happens at the normal lifecycle point.
    pub async fn play_now_temp(&self, id: String, path: PathBuf) -> Result<(), AudioManagerError> {
        self.play_now_with_ownership(id, path, true).await
    }

    async fn play_now_with_ownership(
        &self,
        id: String,
        path: PathBuf,
        temporary: bool,
    ) -> Result<(), AudioManagerError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(AudioCommand::PlayNow {
                id,
                path,
                temporary,
                reply: tx,
            })
            .await
            .map_err(|_| AudioManagerError::Unavailable)?;
        rx.await.map_err(|_| AudioManagerError::Unavailable)?
    }

    /// Play the next item in the queue
    pub async fn play_next(&self) -> Result<(), AudioManagerError> {
        self.command_tx
            .send(AudioCommand::PlayNext)
            .await
            .map_err(|_| AudioManagerError::Unavailable)
    }

    /// Pause playback
    pub async fn pause(&self) -> Result<(), AudioManagerError> {
        self.command_tx
            .send(AudioCommand::Pause)
            .await
            .map_err(|_| AudioManagerError::Unavailable)
    }

    /// Resume playback
    pub async fn resume(&self) -> Result<(), AudioManagerError> {
        self.command_tx
            .send(AudioCommand::Resume)
            .await
            .map_err(|_| AudioManagerError::Unavailable)
    }

    /// Stop playback
    pub async fn stop(&self) -> Result<(), AudioManagerError> {
        self.command_tx
            .send(AudioCommand::Stop)
            .await
            .map_err(|_| AudioManagerError::Unavailable)
    }

    /// Set volume
    pub async fn set_volume(&self, volume: f32) -> Result<(), AudioManagerError> {
        self.command_tx
            .send(AudioCommand::SetVolume(volume))
            .await
            .map_err(|_| AudioManagerError::Unavailable)?;
        Ok(())
    }

    /// Update the queue bound at runtime (hot
    /// configuration reload). Existing items are
    /// never dropped; the new bound applies to
    /// future admissions.
    pub async fn set_max_queue_items(&self, max_items: usize) -> Result<(), AudioManagerError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(AudioCommand::SetMaxQueueItems {
                max_items,
                reply: tx,
            })
            .await
            .map_err(|_| AudioManagerError::Unavailable)?;
        rx.await.map_err(|_| AudioManagerError::Unavailable)
    }

    /// Register a temporary file for automatic cleanup after playback completes.
    /// Temp files (e.g., WAV files synthesized for local playback) are tracked
    /// and deleted once they have finished playing to prevent disk space leaks.
    ///
    /// Retained for backward compatibility with external callers. The
    /// server-generated playback path uses [`AudioManager::add_to_queue_temp`]
    /// / [`AudioManager::play_now_temp`] instead, which register ownership
    /// atomically on admission with no window for orphaning.
    pub async fn register_temp(&self, path: PathBuf) -> Result<(), AudioManagerError> {
        self.command_tx
            .send(AudioCommand::RegisterTemp { path })
            .await
            .map_err(|_| AudioManagerError::Unavailable)
    }

    /// Get queue status. A dead audio thread is an error — never a faked
    /// healthy empty queue.
    pub async fn status(&self) -> Result<QueueStatus, AudioManagerError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(AudioCommand::GetStatus(tx))
            .await
            .map_err(|_| AudioManagerError::Unavailable)?;
        rx.await.map_err(|_| AudioManagerError::Unavailable)
    }

    /// List available output devices plus the current selection.
    /// [`AudioManagerError::DeviceError`] means OS enumeration failed.
    pub async fn output_devices(&self) -> Result<OutputDeviceList, AudioManagerError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(AudioCommand::GetOutputDevices { reply: tx })
            .await
            .map_err(|_| AudioManagerError::Unavailable)?;
        rx.await.map_err(|_| AudioManagerError::Unavailable)?
    }

    /// Report the configured output selection versus live hardware state.
    pub async fn output_device(&self) -> Result<ActiveOutputDevice, AudioManagerError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(AudioCommand::GetOutputDevice { reply: tx })
            .await
            .map_err(|_| AudioManagerError::Unavailable)?;
        rx.await.map_err(|_| AudioManagerError::Unavailable)
    }

    /// Switch the output device at runtime (`default` or an explicit id).
    /// The switch commits only if the new stream opens; otherwise the
    /// previous working output stays active. [`AudioManagerError::UnknownDevice`]
    /// means the name matched nothing (the API reports `400`).
    pub async fn set_output_device(
        &self,
        device: String,
    ) -> Result<ActiveOutputDevice, AudioManagerError> {
        let (tx, rx) = oneshot::channel();
        self.command_tx
            .send(AudioCommand::SetOutputDevice { device, reply: tx })
            .await
            .map_err(|_| AudioManagerError::Unavailable)?;
        rx.await.map_err(|_| AudioManagerError::Unavailable)?
    }
}

/// The audio thread that handles playback. All Rodio enumeration, stream
/// opening, and device swapping happens here — never concurrently from HTTP
/// handlers. `initial_device` is the normalized startup selection.
fn audio_thread(
    mut command_rx: tokio::sync::mpsc::Receiver<AudioCommand>,
    max_queue_items: usize,
    initial_device: String,
) {
    // Initialize rodio. A missing output device must not kill playback
    // forever: servers often boot before audio is available (USB replug,
    // remote session, service start). The thread stays alive, the requested
    // selection is retained, and `ensure_audio_output` retries lazily
    // whenever playback is attempted, so recovery needs no process restart.
    let mut selected_device = initial_device;
    let mut resolved_name: Option<String> = None;
    let mut audio_output = match open_selected_device(&selected_device) {
        Ok((stream, handle, name)) => {
            tracing::info!(device = %name, "Audio output initialized");
            resolved_name = Some(name);
            Some((stream, handle))
        }
        Err(e) => {
            tracing::warn!(
                "Audio output '{selected_device}' unavailable at startup ({e}); selection retained, playback will retry when requested"
            );
            None
        }
    };

    let mut queue = AudioQueue::with_max_items(max_queue_items);
    let mut sink: Option<Sink> = None;
    let mut temp_files: HashSet<PathBuf> = HashSet::new();
    let mut last_stream_retry = std::time::Instant::now();

    // Runtime for async operations
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    loop {
        // Recovery tick: when the selected output device is missing, retain
        // the selection and retry on a throttle (never every 100 ms tick),
        // keeping pending items queued and starting them once the device
        // reappears. Never auto-switches to another device. A sink implies
        // a stream, so the finished check below only runs while output exists.
        if audio_output.is_none() {
            let now = std::time::Instant::now();
            if stream_retry_due(last_stream_retry, now) {
                last_stream_retry = now;
                ensure_audio_output(
                    &mut audio_output,
                    &selected_device,
                    &mut resolved_name,
                    true,
                );
            }
            if audio_output.is_some() && sink.is_none() && !queue.is_paused() {
                pump_queue_when_idle(
                    &mut audio_output,
                    &selected_device,
                    &mut resolved_name,
                    &mut queue,
                    &mut sink,
                    &mut temp_files,
                    true,
                );
            }
        }
        // Check if current playback finished
        if let Some(s) = &sink
            && s.empty()
            && !s.is_paused()
        {
            // Playback finished — clean up temp file if registered
            if let Some(current_item) = queue.take_current() {
                cleanup_item_temp(&current_item, &mut temp_files);
            }
            sink = None;

            // Try to play next; broken items clean their temp files and
            // are skipped so one bad file cannot stall the queue, while
            // output failure requeues the item and drops the dead stream.
            pump_queue_when_idle(
                &mut audio_output,
                &selected_device,
                &mut resolved_name,
                &mut queue,
                &mut sink,
                &mut temp_files,
                false,
            );
        }

        // Event-driven wait: block indefinitely while healthy and idle so
        // the thread sleeps with zero wakeups; poll only while a sink is
        // active (completion + responsive controls) or while a throttled
        // device-recovery retry is pending. Commands themselves wake the
        // indefinite wait immediately, preserving PlayNow acknowledgment
        // and stop/skip latency.
        let wait = recv_wait(
            sink.is_some(),
            audio_output.is_none(),
            STREAM_RETRY_INTERVAL.saturating_sub(last_stream_retry.elapsed()),
        );
        /// Receive outcome: timeout and channel-close stay distinct so a
        /// retry-timeout tick can never be mistaken for manager teardown.
        enum RecvOutcome {
            Command(AudioCommand),
            Timeout,
            Closed,
        }
        let received = rt.block_on(async {
            match wait {
                Some(timeout) => match tokio::time::timeout(timeout, command_rx.recv()).await {
                    Ok(Some(cmd)) => RecvOutcome::Command(cmd),
                    Ok(None) => RecvOutcome::Closed,
                    Err(_) => RecvOutcome::Timeout,
                },
                None => match command_rx.recv().await {
                    Some(cmd) => RecvOutcome::Command(cmd),
                    None => RecvOutcome::Closed,
                },
            }
        });
        match received {
            RecvOutcome::Command(cmd) => {
                match cmd {
                    AudioCommand::Enqueue {
                        id,
                        path,
                        temporary,
                        reply,
                    } => {
                        // Ownership is registered atomically with admission:
                        // once admitted, the item can be completed, stopped,
                        // or replaced only through paths that consult the
                        // registered set, so no interleaving can orphan it.
                        // A rejected item is never registered; the caller
                        // deletes its own file.
                        let admitted = admit_enqueue(
                            &mut queue,
                            &mut temp_files,
                            AudioItem { id, path },
                            temporary,
                        )
                        .is_ok();
                        let _ =
                            reply.send(admitted.then_some(()).ok_or(AudioManagerError::QueueFull));
                        if !admitted {
                            continue;
                        }

                        // Auto-play if nothing is currently playing and queue is not paused.
                        // Without an output device the item stays queued and
                        // the recovery tick starts it once a device appears.
                        if sink.is_none() && !queue.is_paused() {
                            pump_queue_when_idle(
                                &mut audio_output,
                                &selected_device,
                                &mut resolved_name,
                                &mut queue,
                                &mut sink,
                                &mut temp_files,
                                false,
                            );
                        }
                    }
                    AudioCommand::PlayNow {
                        id,
                        path,
                        temporary,
                        reply,
                    } => {
                        // The replaced current item and every queued item
                        // become unreachable: clean their temp files first.
                        clear_queue_with_cleanup(&mut queue, &mut temp_files);
                        if let Some(s) = sink.take() {
                            s.stop();
                        }
                        sink = None;

                        // Register the new temp file before it can be
                        // completed, stopped, or replaced, so failure and
                        // success paths below both see consistent ownership.
                        let item =
                            admit_play_now(&mut temp_files, AudioItem { id, path }, temporary);
                        let attempt = match ensure_audio_output(
                            &mut audio_output,
                            &selected_device,
                            &mut resolved_name,
                            false,
                        ) {
                            None => Err(AudioManagerError::OutputUnavailable),
                            Some(handle) => start_playing(handle, queue.volume(), &mut sink, &item),
                        };
                        match attempt {
                            Ok(()) => {
                                queue.set_current(Some(item));
                                let _ = reply.send(Ok(()));
                            }
                            Err(error @ AudioManagerError::OutputUnavailable) => {
                                // The output itself failed: drop the stream so
                                // the next attempt re-opens instead of pinning
                                // a dead handle in the slot forever.
                                audio_output = None;
                                resolved_name = None;
                                tracing::warn!(path = ?item.path, "PlayNow failed: audio output unavailable");
                                cleanup_item_temp(&item, &mut temp_files);
                                let _ = reply.send(Err(error));
                            }
                            Err(error) => {
                                tracing::warn!(path = ?item.path, "PlayNow failed: invalid audio");
                                cleanup_item_temp(&item, &mut temp_files);
                                let _ = reply.send(Err(error));
                            }
                        }
                    }
                    AudioCommand::PlayNext => {
                        if let Some(s) = sink.take() {
                            s.stop();
                        }
                        sink = None;
                        // The interrupted item never completes: clean it now.
                        if let Some(interrupted) = queue.take_current() {
                            cleanup_item_temp(&interrupted, &mut temp_files);
                        }

                        pump_queue_when_idle(
                            &mut audio_output,
                            &selected_device,
                            &mut resolved_name,
                            &mut queue,
                            &mut sink,
                            &mut temp_files,
                            false,
                        );
                    }
                    AudioCommand::Pause => {
                        if let Some(s) = &sink {
                            s.pause();
                        }
                        queue.set_paused(true);
                    }
                    AudioCommand::Resume => {
                        if let Some(s) = &sink {
                            s.play();
                        }
                        queue.set_paused(false);
                        // A device switch while paused drops the sink with
                        // items still waiting; starting here keeps the queue
                        // from stalling until the next enqueue.
                        if sink.is_none() {
                            pump_queue_when_idle(
                                &mut audio_output,
                                &selected_device,
                                &mut resolved_name,
                                &mut queue,
                                &mut sink,
                                &mut temp_files,
                                false,
                            );
                        }
                    }
                    AudioCommand::Stop => {
                        if let Some(s) = sink.take() {
                            s.stop();
                        }
                        sink = None;
                        clear_queue_with_cleanup(&mut queue, &mut temp_files);
                    }
                    AudioCommand::SetVolume(volume) => {
                        let vol = volume.clamp(0.0, 1.0);
                        if let Some(s) = &sink {
                            s.set_volume(vol);
                        }
                        queue.set_volume(vol);
                    }
                    AudioCommand::SetMaxQueueItems { max_items, reply } => {
                        queue.set_max_items(max_items);
                        let _ = reply.send(());
                    }
                    AudioCommand::RegisterTemp { path } => {
                        temp_files.insert(path);
                    }
                    AudioCommand::GetStatus(tx) => {
                        let is_playing = sink
                            .as_ref()
                            .map(|s| !s.is_paused() && !s.empty())
                            .unwrap_or(false);
                        let is_paused = sink.as_ref().map(|s| s.is_paused()).unwrap_or(false);

                        let _ = tx.send(QueueStatus {
                            current: queue.current().cloned(),
                            queue_length: queue.len(),
                            is_playing,
                            is_paused,
                            volume: queue.volume(),
                        });
                    }
                    AudioCommand::GetOutputDevices { reply } => {
                        let _ = reply.send(handle_get_output_devices(&selected_device));
                    }
                    AudioCommand::GetOutputDevice { reply } => {
                        let _ = reply.send(current_active_device(
                            &selected_device,
                            &resolved_name,
                            &audio_output,
                        ));
                    }
                    AudioCommand::SetOutputDevice { device, reply } => {
                        let _ = reply.send(handle_set_output_device(
                            &mut audio_output,
                            &mut selected_device,
                            &mut resolved_name,
                            &mut queue,
                            &mut sink,
                            &mut temp_files,
                            &device,
                        ));
                    }
                }
            }
            RecvOutcome::Closed => {
                // Channel closed: managers are gone. Best-effort cleanup of
                // every remaining temp file before exiting.
                let mut queue = queue;
                let mut temp_files = temp_files;
                clear_queue_with_cleanup(&mut queue, &mut temp_files);
                for leftover in temp_files {
                    let _ = std::fs::remove_file(&leftover);
                }
                break;
            }
            RecvOutcome::Timeout => {
                // Poll tick (active sink or pending device retry): loop back
                // through the completion check and the recovery tick.
            }
        }
    }
}

/// Outcome of attempting to start queued playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayNextOutcome {
    /// Something is playing or the queue is empty (nothing more to do).
    Settled,
    /// The output itself failed (sink creation): the dequeued item was
    /// requeued at the front and the caller must drop the stream so the next
    /// attempt re-opens it.
    OutputDead,
}

/// Start the next playable queued item, skipping (and temp-cleaning) broken
/// items so one bad file cannot stall the queue behind it. Output failure is
/// not the item's fault: the item is requeued and [`PlayNextOutcome::OutputDead`]
/// is returned. Returns when something plays or the queue is empty.
fn play_next_available(
    stream_handle: &rodio::OutputStreamHandle,
    queue: &mut AudioQueue,
    sink_slot: &mut Option<Sink>,
    temp_files: &mut HashSet<PathBuf>,
) -> PlayNextOutcome {
    while sink_slot.is_none() {
        let Some(item) = queue.dequeue() else {
            break;
        };
        match start_playing(stream_handle, queue.volume(), sink_slot, &item) {
            Ok(()) => {
                queue.set_current(Some(item));
                break;
            }
            Err(AudioManagerError::InvalidAudio) => {
                cleanup_item_temp(&item, temp_files);
            }
            Err(_) => {
                tracing::warn!(path = ?item.path, "Playback stalled: output unavailable, item requeued");
                queue.push_front(item);
                return PlayNextOutcome::OutputDead;
            }
        }
    }
    PlayNextOutcome::Settled
}

/// Ensure the output stream exists and start the next queued item when idle.
/// A dead stream is dropped (with its resolved name) so the next attempt
/// re-opens the retained selection.
#[allow(clippy::too_many_arguments)]
fn pump_queue_when_idle(
    slot: &mut Option<(OutputStream, rodio::OutputStreamHandle)>,
    selected: &str,
    resolved: &mut Option<String>,
    queue: &mut AudioQueue,
    sink_slot: &mut Option<Sink>,
    temp_files: &mut HashSet<PathBuf>,
    quiet: bool,
) {
    let outcome = ensure_audio_output(slot, selected, resolved, quiet)
        .map(|handle| play_next_available(handle, queue, sink_slot, temp_files));
    if matches!(outcome, Some(PlayNextOutcome::OutputDead)) {
        *slot = None;
        *resolved = None;
    }
}

/// How often the idle audio thread retries a missing output device.
/// Command-driven attempts (enqueue / play-now) always retry immediately;
/// this only throttles the background tick so a deviceless machine does not
/// spam one attempt per wakeup.
const STREAM_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);
/// Completion/controls poll while a sink is active. Rodio offers no
/// end-of-playback notification, so the active-only check keeps this cadence;
/// it never runs while idle (see [`recv_wait`]).
const ACTIVE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// True when a background output-device retry is due.
fn stream_retry_due(last_retry: std::time::Instant, now: std::time::Instant) -> bool {
    now.duration_since(last_retry) >= STREAM_RETRY_INTERVAL
}

/// How long the audio thread waits for the next command.
///
/// - A live sink (playing or paused) needs responsive pause/stop/skip and
///   prompt completion detection: poll on [`ACTIVE_POLL_INTERVAL`].
/// - A missing output with nothing playing waits only until the next
///   throttled recovery retry is due — never a fixed hot tick.
/// - A healthy idle thread (no sink, output present) blocks indefinitely:
///   the next command itself is the wakeup, so idle CPU/wakeups drop to
///   zero without harming command latency or PlayNow acknowledgment.
fn recv_wait(
    sink_active: bool,
    output_missing: bool,
    until_retry: std::time::Duration,
) -> Option<std::time::Duration> {
    if sink_active {
        Some(ACTIVE_POLL_INTERVAL)
    } else if output_missing {
        Some(until_retry)
    } else {
        None
    }
}

/// Open the OS default output stream.
fn open_default_stream()
-> Result<(OutputStream, rodio::OutputStreamHandle, String), AudioManagerError> {
    let (stream, handle) = OutputStream::try_default().map_err(|e| {
        tracing::debug!("Default audio output unavailable: {e}");
        AudioManagerError::OutputUnavailable
    })?;
    // Best-effort resolved name (rodio may fall back past the OS default
    // internally, so this labels the selection, not the wire).
    let name = {
        use rodio::cpal::traits::{DeviceTrait, HostTrait};
        rodio::cpal::default_host()
            .default_output_device()
            .and_then(|d| d.name().ok())
            .unwrap_or_else(|| "System Default".to_string())
    };
    Ok((stream, handle, name))
}

/// Open the `occurrence`-th OS output device named `name`.
fn open_named_stream(
    name: &str,
    occurrence: usize,
) -> Result<(OutputStream, rodio::OutputStreamHandle, String), AudioManagerError> {
    let device = super::devices::find_host_device(name, occurrence).ok_or_else(|| {
        tracing::debug!("Audio output device '{name}' disappeared before open");
        AudioManagerError::OutputUnavailable
    })?;
    let (stream, handle) = OutputStream::try_from_device(&device).map_err(|e| {
        tracing::warn!("Cannot open audio output device '{name}': {e}");
        AudioManagerError::OutputUnavailable
    })?;
    Ok((stream, handle, name.to_string()))
}

/// Open the stream for a normalized selection (`default` or an explicit id).
///
/// A retained explicit device that is currently missing reports
/// [`AudioManagerError::OutputUnavailable`] (retryable — the selection is
/// kept); only fresh runtime requests are validated as
/// [`AudioManagerError::UnknownDevice`] by [`handle_set_output_device`].
fn open_selected_device(
    selected: &str,
) -> Result<(OutputStream, rodio::OutputStreamHandle, String), AudioManagerError> {
    if super::devices::is_default_selection(selected) {
        return open_default_stream();
    }
    let (names, _) = super::devices::enumerate_host_devices().map_err(|e| {
        tracing::debug!("Audio device lookup failed: {e}");
        AudioManagerError::DeviceError
    })?;
    // Missing here means "not currently present" (disconnect/reconnect
    // race or a startup selection for an unplugged device): retain and
    // retry rather than rejecting the selection itself.
    let matched = super::devices::match_hardware(selected, &names)
        .ok_or(AudioManagerError::OutputUnavailable)?;
    match matched {
        super::devices::DeviceMatch::Default => open_default_stream(),
        super::devices::DeviceMatch::Named { name, occurrence } => {
            open_named_stream(&name, occurrence)
        }
    }
}

/// Returns the output handle, initializing the stream for the retained
/// `selected` device when it is missing (first use, or recovery after the
/// device was unavailable). `quiet` keeps periodic background retries at
/// debug level; attempts that back a user command always warn loudly so
/// silence is never mysterious.
fn ensure_audio_output<'a>(
    slot: &'a mut Option<(OutputStream, rodio::OutputStreamHandle)>,
    selected: &str,
    resolved: &mut Option<String>,
    quiet: bool,
) -> Option<&'a rodio::OutputStreamHandle> {
    if slot.is_none() {
        match open_selected_device(selected) {
            Ok((stream, handle, name)) => {
                tracing::info!(device = %name, "Audio output initialized");
                *resolved = Some(name);
                *slot = Some((stream, handle));
            }
            Err(error) => {
                *resolved = None;
                if quiet {
                    tracing::debug!("Audio output '{selected}' still unavailable: {error}");
                } else {
                    tracing::warn!("Audio output '{selected}' unavailable: {error}");
                }
                return None;
            }
        }
    }
    slot.as_ref().map(|(_, handle)| handle)
}

/// Attempt to start playback of `item` into `sink_slot`.
///
/// `Ok` means playback started. [`AudioManagerError::InvalidAudio`] covers
/// missing/undecodable files; [`AudioManagerError::OutputUnavailable`]
/// covers sink creation on a dead stream. The caller must clean the item's
/// temp file via [`cleanup_item_temp`] on [`AudioManagerError::InvalidAudio`].
fn start_playing(
    stream_handle: &rodio::OutputStreamHandle,
    volume: f32,
    sink_slot: &mut Option<Sink>,
    item: &AudioItem,
) -> Result<(), AudioManagerError> {
    let file = match File::open(&item.path) {
        Ok(file) => file,
        Err(error) => {
            tracing::warn!(path = ?item.path, "Playback failed: cannot open audio file: {error}");
            return Err(AudioManagerError::InvalidAudio);
        }
    };
    let source = match Decoder::new(BufReader::new(file)) {
        Ok(source) => source,
        Err(error) => {
            tracing::warn!(path = ?item.path, "Playback failed: cannot decode audio file: {error:?}");
            return Err(AudioManagerError::InvalidAudio);
        }
    };
    let new_sink = match Sink::try_new(stream_handle) {
        Ok(sink) => sink,
        Err(error) => {
            tracing::warn!("Playback failed: cannot create audio sink: {error:?}");
            return Err(AudioManagerError::OutputUnavailable);
        }
    };
    new_sink.set_volume(volume);
    new_sink.append(source);
    *sink_slot = Some(new_sink);
    Ok(())
}

/// Snapshot the configured selection versus live hardware state.
fn current_active_device(
    selected: &str,
    resolved: &Option<String>,
    slot: &Option<(OutputStream, rodio::OutputStreamHandle)>,
) -> ActiveOutputDevice {
    ActiveOutputDevice {
        device: selected.to_string(),
        resolved_name: resolved.clone(),
        available: slot.is_some(),
    }
}

/// Enumerate output devices for `GET /api/audio/devices` (audio thread).
fn handle_get_output_devices(selected: &str) -> Result<OutputDeviceList, AudioManagerError> {
    let (names, default_name) = super::devices::enumerate_host_devices().map_err(|e| {
        tracing::warn!("Audio device enumeration failed: {e}");
        AudioManagerError::DeviceError
    })?;
    Ok(super::devices::build_device_list(
        names,
        default_name.as_deref(),
        selected,
    ))
}

/// Switch the output device (audio thread only).
///
/// 1. Normalize and validate the request against live enumeration — unknown
///    names are rejected without touching the active output.
/// 2. Open the new stream; on failure the previous working output stays
///    active and the selection is unchanged.
/// 3. On success: stop the current sink, drop the interrupted item (its temp
///    is cleaned, like `PlayNext` — the waiting queue is never cleared),
///    commit the stream + selection, and continue with queued items on the
///    new device (unless paused; `Resume` starts the queue then).
#[allow(clippy::too_many_arguments)]
fn handle_set_output_device(
    slot: &mut Option<(OutputStream, rodio::OutputStreamHandle)>,
    selected: &mut String,
    resolved: &mut Option<String>,
    queue: &mut AudioQueue,
    sink_slot: &mut Option<Sink>,
    temp_files: &mut HashSet<PathBuf>,
    requested: &str,
) -> Result<ActiveOutputDevice, AudioManagerError> {
    let normalized = super::devices::normalize_selection(requested);
    if normalized == *selected && slot.is_some() {
        return Ok(current_active_device(selected, resolved, slot));
    }
    let (names, _) = super::devices::enumerate_host_devices().map_err(|e| {
        tracing::warn!(
            "Output device switch to '{normalized}' failed: cannot enumerate devices: {e}"
        );
        AudioManagerError::DeviceError
    })?;
    let matched = super::devices::match_hardware(&normalized, &names).ok_or_else(|| {
        tracing::info!("Unknown audio output device requested: '{normalized}'");
        AudioManagerError::UnknownDevice
    })?;
    let (new_stream, new_handle, new_resolved) = match matched {
        super::devices::DeviceMatch::Default => open_default_stream()?,
        super::devices::DeviceMatch::Named { name, occurrence } => {
            open_named_stream(&name, occurrence)?
        }
    };
    if let Some(s) = sink_slot.take() {
        s.stop();
    }
    if let Some(interrupted) = queue.take_current() {
        cleanup_item_temp(&interrupted, temp_files);
    }
    *slot = Some((new_stream, new_handle));
    *selected = normalized;
    *resolved = Some(new_resolved);
    tracing::info!(device = %selected, resolved = ?resolved, "Audio output device switched");
    if !queue.is_paused() {
        pump_queue_when_idle(
            slot, selected, resolved, queue, sink_slot, temp_files, false,
        );
    }
    Ok(current_active_device(selected, resolved, slot))
}

/// Status information about the queue
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QueueStatus {
    pub current: Option<AudioItem>,
    pub queue_length: usize,
    pub is_playing: bool,
    pub is_paused: bool,
    pub volume: f32,
}

/// Admit an `Enqueue` item, registering `temporary` ownership atomically
/// with the bound check. A rejected item is never registered (the caller
/// deletes its own file); an admitted temporary item is registered before
/// any completion/stop/replacement path can observe it, so it cannot leak.
fn admit_enqueue(
    queue: &mut AudioQueue,
    temp_files: &mut HashSet<PathBuf>,
    item: AudioItem,
    temporary: bool,
) -> Result<(), AudioManagerError> {
    queue.enqueue(item.clone())?;
    if temporary {
        temp_files.insert(item.path);
    }
    Ok(())
}

/// Register a `PlayNow` item's `temporary` ownership before playback starts.
/// The caller must have already cleared the previous queue/current state.
/// Failure paths clean the item via [`cleanup_item_temp`], which now finds
/// the registration; success paths keep it until the normal lifecycle point.
fn admit_play_now(
    temp_files: &mut HashSet<PathBuf>,
    item: AudioItem,
    temporary: bool,
) -> AudioItem {
    if temporary {
        temp_files.insert(item.path.clone());
    }
    item
}

/// Attempts to delete a temporary audio file after use.
/// Tracks the set of temp files so each one is only deleted once.
///
/// The registration is removed only when the file is actually gone (or was
/// already absent). On platforms where an open file cannot be unlinked
/// (notably Windows, where the playback backend may still hold the file
/// when the finished check fires), a failed deletion keeps the registration
/// so a later lifecycle pass or the startup sweep retries instead of
/// orphaning the file.
fn cleanup_temp_file(path: &std::path::Path, temp_files: &mut HashSet<PathBuf>) {
    if !temp_files.contains(path) {
        return;
    }
    match std::fs::remove_file(path) {
        Ok(()) => {
            temp_files.remove(path);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            temp_files.remove(path);
        }
        Err(e) => {
            tracing::debug!("Failed to remove temp file '{path:?}': {e}");
        }
    }
}

/// Delete `item`'s file when it is a registered temp file.
fn cleanup_item_temp(item: &AudioItem, temp_files: &mut HashSet<PathBuf>) {
    cleanup_temp_file(&item.path, temp_files);
}

/// Drop the current item and every waiting item, deleting each one's temp
/// file when registered. After this call no queued item references a temp
/// file, so nothing can be orphaned by stop/replace/clear transitions.
fn clear_queue_with_cleanup(queue: &mut AudioQueue, temp_files: &mut HashSet<PathBuf>) {
    if let Some(current) = queue.take_current() {
        cleanup_item_temp(&current, temp_files);
    }
    for item in queue.drain_waiting() {
        cleanup_item_temp(&item, temp_files);
    }
}

// ---------------------------------------------------------------------------
// Temporary playback-file lifecycle (shared by the HTTP handlers and the
// audio thread so every temp file ends as playing, queued, or deleted).
// ---------------------------------------------------------------------------

/// Ensure `dir` exists as a real directory with owner-only permissions.
///
/// - Missing directories are created (`0700` on Unix).
/// - Existing directories are tightened to `0700` on Unix (generated WAVs
///   may contain private synthesized speech).
/// - Symlinks and non-directories are rejected: the temp dir must be a
///   location the service genuinely owns.
pub fn ensure_temp_dir(dir: &Path) -> std::io::Result<PathBuf> {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) if meta.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "temp audio directory must not be a symlink",
        )),
        Ok(meta) if !meta.is_dir() => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "temp audio path exists and is not a directory",
        )),
        Ok(_) => {
            restrict_dir_permissions(dir)?;
            Ok(dir.to_path_buf())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(dir)?;
            restrict_dir_permissions(dir)?;
            Ok(dir.to_path_buf())
        }
        Err(e) => Err(e),
    }
}

#[cfg(unix)]
fn restrict_dir_permissions(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_dir_permissions(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Write a synthesized WAV with owner-only permissions (`0600` on Unix).
pub async fn write_temp_wav(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::AsyncWriteExt;
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true).mode(0o600);
        let mut file = options.open(path).await?;
        file.write_all(bytes).await?;
        file.flush().await?;
        file.sync_all().await?;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        tokio::fs::write(path, bytes).await
    }
}

/// A temp file is owned by SonicBoom only when it is a regular file named
/// `<uuid>.wav` — exactly what the TTS-play handler generates. The startup
/// sweep deletes nothing else.
fn is_owned_temp_file(path: &Path) -> bool {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
    {
        return false;
    }
    let stem_ok = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.parse::<uuid::Uuid>().is_ok());
    if !stem_ok {
        return false;
    }
    // Refuse symlinks/directories even when the name matches.
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file() && !meta.is_symlink())
}

/// Sweep `TEMP_AUDIO_DIR` on startup, deleting only SonicBoom-owned
/// `<uuid>.wav` files. Returns the number of files removed. Missing or
/// unreadable directories sweep zero files without failing startup.
pub fn sweep_temp_dir(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if is_owned_temp_file(&path) && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Prepare the temp directory for use: ensure it exists safely, then sweep
/// leftovers from an unclean shutdown. Returns files reaped by the sweep.
pub fn prepare_temp_dir(dir: &Path) -> std::io::Result<usize> {
    ensure_temp_dir(dir)?;
    Ok(sweep_temp_dir(dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn idle_wait_blocks_while_active_and_deviceless_poll() {
        use std::time::Duration;
        // Healthy idle thread: block indefinitely — zero wakeups, with the
        // next command itself as the wakeup.
        assert_eq!(recv_wait(false, false, Duration::from_secs(10)), None);
        assert_eq!(recv_wait(false, false, Duration::ZERO), None);
        // Active sink (playing or paused): poll for completion and controls.
        assert_eq!(
            recv_wait(true, false, Duration::from_secs(10)),
            Some(ACTIVE_POLL_INTERVAL)
        );
        assert_eq!(
            recv_wait(true, true, Duration::from_secs(10)),
            Some(ACTIVE_POLL_INTERVAL)
        );
        // Missing output, nothing playing: wait only until the throttled
        // retry is due — never a fixed hot tick, never indefinite (which
        // would stall recovery until the next command).
        assert_eq!(
            recv_wait(false, true, Duration::from_secs(7)),
            Some(Duration::from_secs(7))
        );
        assert_eq!(recv_wait(false, true, Duration::ZERO), Some(Duration::ZERO));
        assert_eq!(ACTIVE_POLL_INTERVAL, Duration::from_millis(100));
    }

    #[test]
    fn stream_retries_are_throttled_to_the_recovery_interval() {
        let start = std::time::Instant::now();
        assert!(!stream_retry_due(start, start));
        assert!(!stream_retry_due(
            start,
            start + STREAM_RETRY_INTERVAL - std::time::Duration::from_millis(1)
        ));
        assert!(stream_retry_due(start, start + STREAM_RETRY_INTERVAL));
        assert!(stream_retry_due(
            start,
            start + STREAM_RETRY_INTERVAL + std::time::Duration::from_secs(60)
        ));
    }

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir()
                .join(format!("sonicboom-queue-test-{}-{id}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn file(&self, name: &str) -> PathBuf {
            let path = self.path.join(name);
            std::fs::write(&path, b"fake audio").unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn item(id: &str, path: PathBuf) -> AudioItem {
        AudioItem {
            id: id.to_string(),
            path,
        }
    }

    #[test]
    fn enqueue_rejects_beyond_bound_without_dropping() {
        let mut queue = AudioQueue::with_max_items(2);
        assert!(queue.enqueue(item("a", PathBuf::from("a.wav"))).is_ok());
        assert!(queue.enqueue(item("b", PathBuf::from("b.wav"))).is_ok());
        assert_eq!(
            queue.enqueue(item("c", PathBuf::from("c.wav"))),
            Err(AudioManagerError::QueueFull)
        );
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.dequeue().unwrap().id, "a");
        // Room freed: enqueue works again.
        assert!(queue.enqueue(item("c", PathBuf::from("c.wav"))).is_ok());
    }

    #[test]
    fn bound_is_at_least_one() {
        assert_eq!(AudioQueue::with_max_items(0).max_items(), 1);
    }

    #[test]
    fn stop_clears_current_and_queued_temps() {
        let dir = TempDir::new();
        let current = dir.file("current.wav");
        let waiting = dir.file("waiting.wav");
        let foreign = dir.file("foreign.wav");
        let mut temps: HashSet<PathBuf> =
            HashSet::from([current.clone(), waiting.clone(), foreign.clone()]);
        let mut queue = AudioQueue::new();
        queue.set_current(Some(item("now", current.clone())));
        queue.enqueue(item("next", waiting.clone())).unwrap();

        clear_queue_with_cleanup(&mut queue, &mut temps);

        assert!(!current.exists(), "current temp must be deleted");
        assert!(!waiting.exists(), "queued temp must be deleted");
        assert!(foreign.exists(), "unreferenced files must be kept");
        assert!(queue.current().is_none());
        assert!(queue.is_empty());
    }

    #[test]
    fn interrupted_current_temp_is_cleaned() {
        let dir = TempDir::new();
        let current = dir.file("current.wav");
        let mut temps: HashSet<PathBuf> = HashSet::from([current.clone()]);
        // PlayNext path: take + clean the interrupted item.
        let mut queue = AudioQueue::new();
        queue.set_current(Some(item("now", current.clone())));
        if let Some(interrupted) = queue.take_current() {
            cleanup_item_temp(&interrupted, &mut temps);
        }
        assert!(!current.exists());
        assert!(temps.is_empty());
    }

    #[test]
    fn failed_item_temp_is_cleaned() {
        let dir = TempDir::new();
        let broken = dir.file("broken.wav");
        let mut temps: HashSet<PathBuf> = HashSet::from([broken.clone()]);
        // Decode/open/sink failure path.
        cleanup_item_temp(&item("bad", broken.clone()), &mut temps);
        assert!(!broken.exists());
        assert!(temps.is_empty());
    }

    #[test]
    fn non_temp_files_are_never_deleted() {
        let dir = TempDir::new();
        let library = dir.file("library.wav");
        let mut temps: HashSet<PathBuf> = HashSet::new();
        let mut queue = AudioQueue::new();
        queue.set_current(Some(item("now", library.clone())));
        clear_queue_with_cleanup(&mut queue, &mut temps);
        assert!(library.exists(), "library files must survive cleanup");
    }

    #[test]
    fn sweep_deletes_only_owned_uuid_wavs() {
        let dir = TempDir::new();
        let owned = dir.path.join(format!("{}.wav", uuid::Uuid::new_v4()));
        std::fs::write(&owned, b"leftover").unwrap();
        let other_txt = dir.file("notes.txt");
        let fake_wav = dir.file("not-a-uuid.wav");
        let upper = dir.file("ALSO-NOT-A-UUID.WAV");
        let sub = dir.path.join("sub");
        std::fs::create_dir_all(&sub).unwrap();

        assert_eq!(sweep_temp_dir(&dir.path), 1);
        assert!(!owned.exists());
        assert!(other_txt.exists());
        assert!(fake_wav.exists());
        assert!(upper.exists());
        assert!(sub.is_dir());
    }

    #[test]
    fn sweep_missing_dir_removes_nothing() {
        let missing = std::env::temp_dir().join(format!(
            "sonicboom-sweep-missing-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        assert_eq!(sweep_temp_dir(&missing), 0);
    }

    #[test]
    fn ensure_temp_dir_creates_and_rejects_non_dirs() {
        let base = TempDir::new();
        let nested = base.path.join("a").join("b");
        let ensured = ensure_temp_dir(&nested).unwrap();
        assert!(ensured.is_dir());

        let file = base.file("file.wav");
        assert!(ensure_temp_dir(&file).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_temp_dir_rejects_symlinks() {
        let base = TempDir::new();
        let target = base.path.join("real");
        std::fs::create_dir_all(&target).unwrap();
        let link = base.path.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(ensure_temp_dir(&link).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn temp_dir_and_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let base = TempDir::new();
        // Pre-existing wide directory is tightened.
        let wide = base.path.join("wide");
        std::fs::create_dir_all(&wide).unwrap();
        std::fs::set_permissions(&wide, std::fs::Permissions::from_mode(0o755)).unwrap();
        ensure_temp_dir(&wide).unwrap();
        let mode = std::fs::metadata(&wide).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let wav = wide.join("x.wav");
        rt.block_on(write_temp_wav(&wav, b"RIFF")).unwrap();
        let mode = std::fs::metadata(&wav).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[tokio::test]
    async fn dead_audio_thread_reports_unavailable_on_every_operation() {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        drop(rx); // Simulate a terminated audio thread.
        let manager = AudioManager::for_test(tx);
        let path = PathBuf::from("x.wav");

        assert_eq!(
            manager.add_to_queue("a".into(), path.clone()).await,
            Err(AudioManagerError::Unavailable)
        );
        assert_eq!(
            manager.play_now("a".into(), path.clone()).await,
            Err(AudioManagerError::Unavailable)
        );
        assert_eq!(
            manager.play_next().await,
            Err(AudioManagerError::Unavailable)
        );
        assert_eq!(manager.pause().await, Err(AudioManagerError::Unavailable));
        assert_eq!(manager.resume().await, Err(AudioManagerError::Unavailable));
        assert_eq!(manager.stop().await, Err(AudioManagerError::Unavailable));
        assert_eq!(
            manager.set_volume(0.5).await,
            Err(AudioManagerError::Unavailable)
        );
        assert_eq!(
            manager.register_temp(path).await,
            Err(AudioManagerError::Unavailable)
        );
        assert_eq!(
            manager.status().await.unwrap_err(),
            AudioManagerError::Unavailable
        );
        assert_eq!(
            manager.output_devices().await.unwrap_err(),
            AudioManagerError::Unavailable
        );
        assert_eq!(
            manager.output_device().await.unwrap_err(),
            AudioManagerError::Unavailable
        );
        assert_eq!(
            manager
                .set_output_device("default".to_string())
                .await
                .unwrap_err(),
            AudioManagerError::Unavailable
        );
    }

    #[test]
    fn requeued_item_keeps_its_place_without_bound_violation() {
        let mut queue = AudioQueue::with_max_items(1);
        queue.enqueue(item("a", PathBuf::from("a.wav"))).unwrap();
        let stalled = queue.dequeue().unwrap();
        queue.push_front(stalled);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.dequeue().unwrap().id, "a");
    }

    #[tokio::test]
    async fn enqueue_reply_propagates_queue_full() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        // Scripted stand-in for the audio thread: reject the enqueue.
        tokio::spawn(async move {
            if let Some(AudioCommand::Enqueue { reply, .. }) = rx.recv().await {
                let _ = reply.send(Err(AudioManagerError::QueueFull));
            }
        });
        let manager = AudioManager::for_test(tx);
        assert_eq!(
            manager
                .add_to_queue("a".into(), PathBuf::from("x.wav"))
                .await,
            Err(AudioManagerError::QueueFull)
        );
    }

    #[tokio::test]
    async fn status_reply_propagates_queue_state() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            if let Some(AudioCommand::GetStatus(reply)) = rx.recv().await {
                let _ = reply.send(QueueStatus {
                    current: None,
                    queue_length: 3,
                    is_playing: true,
                    is_paused: false,
                    volume: 0.5,
                });
            }
        });
        let manager = AudioManager::for_test(tx);
        let status = manager.status().await.unwrap();
        assert_eq!(status.queue_length, 3);
        assert!(status.is_playing);
    }

    #[tokio::test]
    async fn play_now_reply_propagates_playback_outcome() {
        // Success: the thread started playback.
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            if let Some(AudioCommand::PlayNow { reply, .. }) = rx.recv().await {
                let _ = reply.send(Ok(()));
            }
        });
        let manager = AudioManager::for_test(tx);
        assert!(
            manager
                .play_now("a".into(), PathBuf::from("x.wav"))
                .await
                .is_ok()
        );

        // Failure: no output stream/sink — the caller must see the error,
        // never a blind "Playing immediately" success.
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            if let Some(AudioCommand::PlayNow { reply, .. }) = rx.recv().await {
                let _ = reply.send(Err(AudioManagerError::OutputUnavailable));
            }
        });
        let manager = AudioManager::for_test(tx);
        assert_eq!(
            manager.play_now("a".into(), PathBuf::from("x.wav")).await,
            Err(AudioManagerError::OutputUnavailable)
        );

        // Failure: undecodable file.
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            if let Some(AudioCommand::PlayNow { reply, .. }) = rx.recv().await {
                let _ = reply.send(Err(AudioManagerError::InvalidAudio));
            }
        });
        let manager = AudioManager::for_test(tx);
        assert_eq!(
            manager.play_now("a".into(), PathBuf::from("x.wav")).await,
            Err(AudioManagerError::InvalidAudio)
        );
    }

    #[tokio::test]
    async fn device_command_replies_propagate() {
        use super::super::devices::{ActiveOutputDevice, OutputDeviceList};

        // Device list.
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            if let Some(AudioCommand::GetOutputDevices { reply }) = rx.recv().await {
                let _ = reply.send(Ok(OutputDeviceList {
                    devices: vec![],
                    selected: "default".to_string(),
                }));
            }
        });
        let manager = AudioManager::for_test(tx);
        let list = manager.output_devices().await.unwrap();
        assert_eq!(list.selected, "default");

        // Active device.
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            if let Some(AudioCommand::GetOutputDevice { reply }) = rx.recv().await {
                let _ = reply.send(ActiveOutputDevice {
                    device: "default".to_string(),
                    resolved_name: Some("Speakers".to_string()),
                    available: true,
                });
            }
        });
        let manager = AudioManager::for_test(tx);
        let active = manager.output_device().await.unwrap();
        assert!(active.available);

        // Unknown device selection is an error, never a silent fallback.
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            if let Some(AudioCommand::SetOutputDevice { reply, .. }) = rx.recv().await {
                let _ = reply.send(Err(AudioManagerError::UnknownDevice));
            }
        });
        let manager = AudioManager::for_test(tx);
        assert_eq!(
            manager.set_output_device("Nope".to_string()).await,
            Err(AudioManagerError::UnknownDevice)
        );
    }

    // ------------------------------------------------------------------
    // SB-001: atomic temp-file ownership tests.
    // ------------------------------------------------------------------

    #[test]
    fn atomic_enqueue_registers_temp_on_admission() {
        let dir = TempDir::new();
        let temp = dir.file("admitted.wav");
        let mut temps: HashSet<PathBuf> = HashSet::new();
        let mut queue = AudioQueue::new();

        admit_enqueue(&mut queue, &mut temps, item("a", temp.clone()), true).unwrap();

        // Registered before any completion/stop/replacement path can run.
        assert!(temps.contains(&temp));
        assert_eq!(queue.len(), 1);
        assert!(temp.exists(), "live queued audio must not be deleted");
    }

    #[test]
    fn rejected_enqueue_never_registers_temp() {
        let dir = TempDir::new();
        let mut temps: HashSet<PathBuf> = HashSet::new();
        let mut queue = AudioQueue::with_max_items(1);
        queue.enqueue(item("a", dir.file("a.wav"))).unwrap();

        let rejected = dir.file("rejected.wav");
        assert_eq!(
            admit_enqueue(&mut queue, &mut temps, item("b", rejected.clone()), true),
            Err(AudioManagerError::QueueFull)
        );
        assert!(!temps.contains(&rejected));
        // Caller deletes its own rejected file; nothing is orphaned.
        std::fs::remove_file(&rejected).unwrap();
        assert!(!rejected.exists());
    }

    #[test]
    fn caller_owned_files_are_never_registered() {
        let dir = TempDir::new();
        let library = dir.file("library.wav");
        let mut temps: HashSet<PathBuf> = HashSet::new();
        let mut queue = AudioQueue::new();

        admit_enqueue(&mut queue, &mut temps, item("lib", library.clone()), false).unwrap();
        assert!(!temps.contains(&library));

        // Stop/replace lifecycle must not delete caller-owned files.
        clear_queue_with_cleanup(&mut queue, &mut temps);
        assert!(library.exists(), "caller-owned files must survive");
        assert!(temps.is_empty());
    }

    #[test]
    fn play_now_registers_temp_before_start_attempt() {
        let dir = TempDir::new();
        let temp = dir.file("now.wav");
        let mut temps: HashSet<PathBuf> = HashSet::new();

        // Registration happens before the playback attempt, so both the
        // failure path (cleanup here) and the success path (later lifecycle
        // cleanup) observe consistent ownership.
        let admitted = admit_play_now(&mut temps, item("now", temp.clone()), true);
        assert!(temps.contains(&temp));

        // Simulated start failure: the item's temp file is released.
        cleanup_item_temp(&admitted, &mut temps);
        assert!(!temp.exists());
        assert!(temps.is_empty());

        // Caller-owned play-now is never registered.
        let library = dir.file("owned.wav");
        let owned = admit_play_now(&mut temps, item("o", library.clone()), false);
        assert!(!temps.contains(&library));
        cleanup_item_temp(&owned, &mut temps);
        assert!(library.exists(), "caller-owned files must survive");
    }

    #[test]
    fn missing_files_clear_registration_without_orphan() {
        let dir = TempDir::new();
        let ghost = dir.path.join("ghost.wav");
        let mut temps: HashSet<PathBuf> = HashSet::from([ghost.clone()]);
        cleanup_item_temp(&item("g", ghost.clone()), &mut temps);
        assert!(temps.is_empty(), "already-gone files must not linger");
    }

    #[test]
    fn failed_deletion_keeps_registration_for_retry() {
        // A directory cannot be unlinked with `remove_file` on either
        // platform under test, which deterministically exercises the
        // Windows live-file branch (sharing violation): the registration
        // must be retained for a later lifecycle pass instead of orphaning
        // the path.
        let dir = TempDir::new();
        let blocked = dir.path.join("blocked.wav");
        std::fs::create_dir_all(&blocked).unwrap();
        let mut temps: HashSet<PathBuf> = HashSet::from([blocked.clone()]);
        cleanup_item_temp(&item("b", blocked.clone()), &mut temps);
        assert!(
            temps.contains(&blocked),
            "failed deletion must retain registration for retry"
        );
        assert!(blocked.exists());
        // A later pass after the handle closes succeeds and deregisters.
        std::fs::remove_dir(&blocked).unwrap();
        std::fs::write(&blocked, b"leftover").unwrap();
        cleanup_item_temp(&item("b", blocked.clone()), &mut temps);
        assert!(temps.is_empty());
        assert!(!blocked.exists());
    }

    /// Deterministic xorshift64 PRNG so the interleaving stress test is
    /// reproducible without model weights or audio hardware.
    struct StressRng(u64);

    impl StressRng {
        fn next(&mut self, bound: u64) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x % bound
        }
    }

    #[test]
    fn randomized_ownership_interleavings_leave_no_orphans() {
        for seed in 0..1_000u64 {
            let case_dir = TempDir::new();
            let mut rng = StressRng(seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(1));
            let mut queue = AudioQueue::with_max_items(3);
            let mut temps: HashSet<PathBuf> = HashSet::new();
            // Every admitted temp file: path -> still referenced (queued or
            // current). Owned files must never be deleted by lifecycle paths.
            let mut live_temps: HashSet<PathBuf> = HashSet::new();
            let mut owned: Vec<PathBuf> = Vec::new();
            let mut counter = 0u32;

            let make_file = |counter: &mut u32| {
                *counter += 1;
                let path = case_dir.path.join(format!("t-{seed}-{counter}.wav"));
                std::fs::write(&path, b"fake audio").unwrap();
                path
            };

            for _ in 0..12 {
                match rng.next(7) {
                    // Enqueue a server-generated temp file.
                    0 | 1 => {
                        let path = make_file(&mut counter);
                        let temporary = rng.next(2) == 0;
                        let admitted = admit_enqueue(
                            &mut queue,
                            &mut temps,
                            item(&format!("e-{counter}"), path.clone()),
                            temporary,
                        )
                        .is_ok();
                        if admitted {
                            if temporary {
                                assert!(temps.contains(&path));
                                live_temps.insert(path.clone());
                            } else {
                                owned.push(path.clone());
                            }
                            assert!(path.exists(), "live queued audio must not be deleted");
                        } else {
                            // Rejected: never registered; caller deletes.
                            assert!(!temps.contains(&path));
                            std::fs::remove_file(&path).unwrap();
                        }
                    }
                    // Play-now a server-generated temp file (interrupts).
                    2 => {
                        let path = make_file(&mut counter);
                        let temporary = rng.next(2) == 0;
                        // Replacement clears previous state first, exactly
                        // like the audio thread's PlayNow handler.
                        let dropped_current = queue.take_current();
                        let dropped_waiting = queue.drain_waiting();
                        for dropped in dropped_current.into_iter().chain(dropped_waiting) {
                            cleanup_item_temp(&dropped, &mut temps);
                            live_temps.remove(&dropped.path);
                        }
                        let admitted_item =
                            admit_play_now(&mut temps, item("now", path.clone()), temporary);
                        if temporary {
                            assert!(temps.contains(&path));
                        }
                        // Simulated start attempt: temp failures clean up
                        // via the registration; caller-owned failures leave
                        // the library file for the caller.
                        if rng.next(4) == 0 {
                            cleanup_item_temp(&admitted_item, &mut temps);
                            if temporary {
                                live_temps.remove(&path);
                                // Failed admission: caller deletes its own
                                // generated file (already cleaned above).
                                let _ = std::fs::remove_file(&path);
                            } else {
                                owned.push(path.clone());
                            }
                        } else {
                            queue.set_current(Some(admitted_item));
                            if temporary {
                                live_temps.insert(path.clone());
                            } else {
                                owned.push(path.clone());
                            }
                            assert!(path.exists(), "live current audio must not be deleted");
                        }
                    }
                    // Stop: clears current + waiting.
                    3 => {
                        let dropped_current = queue.take_current();
                        let dropped_waiting = queue.drain_waiting();
                        for dropped in dropped_current.into_iter().chain(dropped_waiting) {
                            cleanup_item_temp(&dropped, &mut temps);
                            live_temps.remove(&dropped.path);
                        }
                        assert!(queue.current().is_none() && queue.is_empty());
                    }
                    // Complete the current item (playback finished).
                    4 => {
                        if let Some(current) = queue.take_current() {
                            assert!(
                                current.path.exists(),
                                "current file must survive until completion"
                            );
                            cleanup_item_temp(&current, &mut temps);
                            live_temps.remove(&current.path);
                        }
                    }
                    // Decoding failure on the next waiting item.
                    _ => {
                        if let Some(broken) = queue.dequeue() {
                            cleanup_item_temp(&broken, &mut temps);
                            live_temps.remove(&broken.path);
                        }
                    }
                }

                // Invariant after every step: every live temp is registered
                // and present; every owned file is present.
                for live in &live_temps {
                    assert!(temps.contains(live), "live temp {live:?} lost registration");
                    assert!(live.exists(), "live temp {live:?} deleted early");
                }
                for path in &owned {
                    assert!(path.exists(), "caller-owned file {path:?} was deleted");
                }
            }

            // Drain: everything admitted must resolve to deleted temps
            // and surviving owned files — zero orphans.
            if let Some(current) = queue.take_current() {
                cleanup_item_temp(&current, &mut temps);
                live_temps.remove(&current.path);
            }
            for item in queue.drain_waiting() {
                cleanup_item_temp(&item, &mut temps);
                live_temps.remove(&item.path);
            }
            assert!(
                live_temps.is_empty(),
                "admitted temp files never reached a terminal lifecycle point"
            );
            // `temps` may only retain paths for files already gone from
            // disk (retry-pending live-file deletions); here nothing is
            // pending, so it must be empty.
            assert!(temps.is_empty(), "orphaned temp registrations remain");
            // On-disk check: every surviving file must be caller-owned.
            let owned_set: HashSet<&PathBuf> = owned.iter().collect();
            for entry in std::fs::read_dir(&case_dir.path)
                .expect("case dir must remain readable")
                .flatten()
            {
                let surviving = entry.path();
                if surviving
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
                {
                    assert!(
                        owned_set.contains(&surviving),
                        "orphaned temp file {surviving:?}"
                    );
                }
            }
            for path in &owned {
                assert!(path.exists(), "caller-owned file {path:?} was deleted");
            }
        }
    }

    #[tokio::test]
    async fn temp_admission_sends_ownership_flag() {
        // Server-generated path carries `temporary: true`.
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            match rx.recv().await {
                Some(AudioCommand::Enqueue {
                    temporary, reply, ..
                }) => {
                    assert!(temporary);
                    let _ = reply.send(Ok(()));
                }
                other => panic!("unexpected command: {other:?}"),
            }
        });
        let manager = AudioManager::for_test(tx);
        assert!(
            manager
                .add_to_queue_temp("a".into(), PathBuf::from("gen.wav"))
                .await
                .is_ok()
        );

        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            match rx.recv().await {
                Some(AudioCommand::PlayNow {
                    temporary, reply, ..
                }) => {
                    assert!(temporary);
                    let _ = reply.send(Ok(()));
                }
                other => panic!("unexpected command: {other:?}"),
            }
        });
        let manager = AudioManager::for_test(tx);
        assert!(
            manager
                .play_now_temp("a".into(), PathBuf::from("gen.wav"))
                .await
                .is_ok()
        );

        // Caller-owned path carries `temporary: false` so cleanup never
        // deletes library files.
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            match rx.recv().await {
                Some(AudioCommand::Enqueue {
                    temporary, reply, ..
                }) => {
                    assert!(!temporary);
                    let _ = reply.send(Ok(()));
                }
                other => panic!("unexpected command: {other:?}"),
            }
        });
        let manager = AudioManager::for_test(tx);
        assert!(
            manager
                .add_to_queue("a".into(), PathBuf::from("lib.wav"))
                .await
                .is_ok()
        );

        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            match rx.recv().await {
                Some(AudioCommand::PlayNow {
                    temporary, reply, ..
                }) => {
                    assert!(!temporary);
                    let _ = reply.send(Ok(()));
                }
                other => panic!("unexpected command: {other:?}"),
            }
        });
        let manager = AudioManager::for_test(tx);
        assert!(
            manager
                .play_now("a".into(), PathBuf::from("lib.wav"))
                .await
                .is_ok()
        );
    }
}

pub mod audio;
pub mod control;
#[cfg(feature = "playback")]
pub mod devices;
pub mod download;
pub mod inference;
pub mod model;
pub mod providers;
#[cfg(feature = "playback")]
pub mod queue;
pub mod text;
pub mod voices;

use model::ModelHandle;

#[derive(Clone)]
pub enum ModelStatus {
    Idle,
    Downloading { progress: f32 },
    Loading,
    Ready(std::sync::Arc<ModelHandle>),
    Failed(String),
}

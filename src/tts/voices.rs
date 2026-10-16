//! Managed voice-profile store for voice cloning.
//!
//! Opaque UUID profiles bind a provider/model/revision, language, cloning
//! mode, reference-audio hash and optional transcript to an
//! authorization/license/consent declaration. No arbitrary filenames from
//! clients: reference audio is stored under `<store-dir>/<uuid>.wav` and
//! addressed only by profile ID. Reference transcripts and audio bytes are
//! never logged.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}

/// Authorization/license/consent declaration stored with every profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConsent {
    /// e.g. `owner`, `licensed`, `authorized`.
    pub basis: String,
    /// Free-text note (owner name, license reference). Never logged.
    pub note: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// A managed cloning voice profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceProfile {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub model: String,
    pub model_revision: String,
    pub language: String,
    pub clone_mode: String,
    pub reference_hash: String,
    pub reference_sample_rate: u32,
    pub reference_duration_secs: f64,
    pub transcript: Option<String>,
    pub owner_token_hash: Option<String>,
    pub consent: VoiceConsent,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Input for creating a profile (validated before persistence).
#[derive(Debug, Clone)]
pub struct NewVoiceProfile {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub model_revision: String,
    pub language: String,
    pub clone_mode: String,
    pub reference_pcm: Vec<f32>,
    pub reference_sample_rate: u32,
    pub transcript: Option<String>,
    pub owner_token_hash: Option<String>,
    pub consent_basis: String,
    pub consent_note: String,
}

impl NewVoiceProfile {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.chars().count() > 64 {
            return Err("profile name must be 1-64 characters".to_string());
        }
        if self
            .transcript
            .as_ref()
            .is_some_and(|t| t.chars().count() > 10_000)
            || self.consent_note.len() > 4096
        {
            return Err("reference transcript or consent note exceeds its bound".into());
        }
        if self.reference_pcm.iter().any(|s| !s.is_finite()) {
            return Err("reference samples must be finite".into());
        }
        if self.clone_mode == "icl"
            && self
                .transcript
                .as_deref()
                .is_none_or(|t| t.trim().is_empty())
        {
            return Err("ICL requires a reference transcript".into());
        }
        if self.provider.trim().is_empty() || self.provider.len() > 32 {
            return Err("provider must be a known provider id".to_string());
        }
        if self.model.trim().is_empty() || self.model.len() > 64 {
            return Err("model must be a known model id".to_string());
        }
        if self.language.trim().is_empty() || self.language.len() > 16 {
            return Err("language must be a valid language tag".to_string());
        }
        if self.consent_basis.trim().is_empty() {
            return Err("consent basis is required (e.g. owner, licensed, authorized)".to_string());
        }
        if self.reference_pcm.is_empty() {
            return Err("reference audio is empty".to_string());
        }
        if self.reference_sample_rate == 0 {
            return Err("reference sample rate must be nonzero".to_string());
        }
        let secs = self.reference_pcm.len() as f64 / f64::from(self.reference_sample_rate);
        if secs < 1.0 || secs > 30.0 {
            return Err(format!(
                "reference audio duration {secs:.1}s is outside the 1-30 s window"
            ));
        }
        if self.reference_pcm.len() > 8_000_000 {
            return Err("reference audio exceeds the decoded sample cap".to_string());
        }
        Ok(())
    }
}

/// Validate a profile ID is an opaque UUID (path-traversal protection:
/// IDs never contain `/`, `.` or separators).
pub fn validate_profile_id(id: &str) -> Result<(), String> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| "unknown voice profile".to_string())
}

/// Filesystem-backed profile store (`profiles.json` + `<uuid>.wav`).
#[derive(Debug)]
pub struct VoiceProfileStore {
    dir: PathBuf,
    profiles: HashMap<String, VoiceProfile>,
}

impl VoiceProfileStore {
    pub fn manifest_path(dir: &Path) -> PathBuf {
        dir.join("profiles.json")
    }

    pub fn reference_path(&self, id: &str) -> Result<PathBuf, String> {
        validate_profile_id(id)?;
        Ok(self.dir.join(format!("{id}.wav")))
    }

    pub fn open(dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&dir).map_err(|e| format!("could not create voice store: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let manifest = Self::manifest_path(&dir);
        let profiles = if manifest.is_file() {
            if std::fs::metadata(&manifest)
                .map_err(|e| e.to_string())?
                .len()
                > 64 * 1024 * 1024
            {
                return Err("voice manifest exceeds 64 MiB bound".into());
            }
            let text = std::fs::read_to_string(&manifest)
                .map_err(|e| format!("could not read voice manifest: {e}"))?;
            serde_json::from_str::<HashMap<String, VoiceProfile>>(&text)
                .map_err(|e| format!("voice manifest is corrupt: {e}"))?
        } else {
            HashMap::new()
        };
        Ok(Self { dir, profiles })
    }

    fn persist(&self) -> Result<(), String> {
        let manifest = Self::manifest_path(&self.dir);
        let text = serde_json::to_string_pretty(&self.profiles)
            .map_err(|e| format!("could not serialize voice manifest: {e}"))?;
        if text.len() > 64 * 1024 * 1024 {
            return Err("voice manifest exceeds 64 MiB bound".into());
        }
        let tmp = self
            .dir
            .join(format!(".profiles.{}.tmp", uuid::Uuid::new_v4()));
        let _staging = Staging(tmp.clone());
        write_private(&tmp, text.as_bytes())
            .map_err(|e| format!("could not write voice manifest: {e}"))?;
        std::fs::rename(&tmp, &manifest)
            .map_err(|e| format!("could not finalize voice manifest: {e}"))?;
        Ok(())
    }

    fn hash_reference(pcm: &[f32]) -> String {
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        for sample in pcm {
            hasher.update(sample.to_le_bytes());
        }
        hex::encode(hasher.finalize())
    }

    /// Create a profile, persisting reference WAV atomically.
    pub fn create(&mut self, input: NewVoiceProfile) -> Result<VoiceProfile, String> {
        input.validate()?;
        let id = uuid::Uuid::new_v4().to_string();
        let hash = Self::hash_reference(&input.reference_pcm);
        let duration = input.reference_pcm.len() as f64 / f64::from(input.reference_sample_rate);
        let now = chrono::Utc::now();
        let profile = VoiceProfile {
            id: id.clone(),
            name: input.name.trim().to_string(),
            provider: input.provider,
            model: input.model,
            model_revision: input.model_revision,
            language: input.language,
            clone_mode: input.clone_mode,
            reference_hash: hash,
            reference_sample_rate: input.reference_sample_rate,
            reference_duration_secs: duration,
            transcript: input
                .transcript
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty()),
            owner_token_hash: input.owner_token_hash,
            consent: VoiceConsent {
                basis: input.consent_basis,
                note: input.consent_note,
                created_at: now,
            },
            created_at: now,
            updated_at: now,
        };
        let wav = crate::tts::audio::encode_audio(
            &input.reference_pcm,
            input.reference_sample_rate,
            crate::tts::audio::AudioFormat::Wav,
        )
        .map_err(|e| format!("could not encode reference audio: {e}"))?;
        let tmp = self.dir.join(format!(".{id}.wav.tmp"));
        let _staging = Staging(tmp.clone());
        write_private(&tmp, &wav).map_err(|e| format!("could not store reference audio: {e}"))?;
        std::fs::rename(&tmp, self.dir.join(format!("{id}.wav")))
            .map_err(|e| format!("could not finalize reference audio: {e}"))?;
        self.profiles.insert(id.clone(), profile.clone());
        if let Err(error) = self.persist() {
            let _ = std::fs::remove_file(self.dir.join(format!("{id}.wav")));
            let _ = std::fs::remove_file(&tmp);
            self.profiles.remove(&profile.id);
            return Err(error);
        }
        Ok(profile)
    }

    pub fn get(&self, id: &str) -> Option<&VoiceProfile> {
        validate_profile_id(id).ok()?;
        self.profiles.get(id)
    }

    pub fn list(&self) -> Vec<&VoiceProfile> {
        let mut out: Vec<&VoiceProfile> = self.profiles.values().collect();
        out.sort_by_key(|a| a.created_at);
        out
    }

    /// Delete a profile and its reference audio. Missing IDs are `None`
    /// (callers map to 404 without disclosing existence).
    pub fn delete(&mut self, id: &str) -> Result<bool, String> {
        validate_profile_id(id)?;
        if self.profiles.remove(id).is_none() {
            return Ok(false);
        }
        let _ = std::fs::remove_file(self.dir.join(format!("{id}.wav")));
        self.persist()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> NewVoiceProfile {
        NewVoiceProfile {
            name: "mine".to_string(),
            provider: "qwen3-gguf".to_string(),
            model: "qwen3-tts-1.7b-base".to_string(),
            model_revision: "rev".to_string(),
            language: "ja".to_string(),
            clone_mode: "icl".to_string(),
            reference_pcm: vec![0.1; 24000],
            reference_sample_rate: 24000,
            transcript: Some("hello".to_string()),
            owner_token_hash: None,
            consent_basis: "owner".to_string(),
            consent_note: "self-recorded".to_string(),
        }
    }

    fn store() -> VoiceProfileStore {
        let dir = std::env::temp_dir().join(format!("voices-{}", uuid::Uuid::new_v4()));
        VoiceProfileStore::open(dir).unwrap()
    }

    #[test]
    fn create_get_delete_round_trip() {
        let mut store = store();
        let dir = store.dir.clone();
        let profile = store.create(input()).unwrap();
        assert_eq!(store.get(&profile.id).unwrap().name, "mine");
        assert!(store.reference_path(&profile.id).unwrap().is_file());
        assert_eq!(store.list().len(), 1);
        assert!(store.delete(&profile.id).unwrap());
        assert!(store.get(&profile.id).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn traversal_ids_are_rejected() {
        assert!(validate_profile_id("../evil").is_err());
        assert!(validate_profile_id("a/b").is_err());
        assert!(validate_profile_id("").is_err());
        let mut store = store();
        let dir = store.dir.clone();
        assert!(store.delete("../evil").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn consent_basis_is_required() {
        let mut bad = input();
        bad.consent_basis.clear();
        assert!(bad.validate().is_err());
    }

    #[test]
    fn reference_bounds_are_enforced() {
        let mut bad = input();
        bad.reference_pcm = vec![];
        assert!(bad.validate().is_err());
        let mut bad = input();
        bad.reference_pcm = vec![0.1; 100];
        assert!(bad.validate().is_err());
    }
}

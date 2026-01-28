use cadenza_ports::storage::{SettingsDto, StorageError, StoragePort};
use std::fs;
use std::path::{Path, PathBuf};

pub struct FsStorage {
    base_dir: PathBuf,
}

impl FsStorage {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn default_base_dir() -> Result<PathBuf, StorageError> {
        let base = dirs_next::config_dir()
            .ok_or_else(|| StorageError::Io("config dir not found".to_string()))?;
        Ok(base.join("Cadenza"))
    }

    fn settings_path(&self) -> PathBuf {
        self.base_dir.join("settings.json")
    }

    fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, StorageError> {
        let data = fs::read(path).map_err(|e| StorageError::Io(e.to_string()))?;
        serde_json::from_slice(&data).map_err(|e| StorageError::Serde(e.to_string()))
    }

    fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), StorageError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| StorageError::Io(e.to_string()))?;
        }
        let data =
            serde_json::to_vec_pretty(value).map_err(|e| StorageError::Serde(e.to_string()))?;
        fs::write(path, data).map_err(|e| StorageError::Io(e.to_string()))
    }
}

impl Default for FsStorage {
    fn default() -> Self {
        let base_dir = Self::default_base_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self { base_dir }
    }
}

impl StoragePort for FsStorage {
    fn load_settings(&self) -> Result<SettingsDto, StorageError> {
        let path = self.settings_path();
        if !path.exists() {
            return Ok(SettingsDto::default());
        }
        Self::read_json(&path)
    }

    fn save_settings(&self, s: &SettingsDto) -> Result<(), StorageError> {
        let path = self.settings_path();
        Self::write_json(&path, s)
    }
}

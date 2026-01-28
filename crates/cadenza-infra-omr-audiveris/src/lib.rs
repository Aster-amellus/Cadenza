use cadenza_ports::omr::{OmrError, OmrOptions, OmrPort, OmrResult};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct AudiverisOmr {
    default_engine_path: Option<String>,
}

impl AudiverisOmr {
    pub fn new(default_engine_path: Option<String>) -> Self {
        Self {
            default_engine_path,
        }
    }

    fn engine_path(&self, options: &OmrOptions) -> String {
        let engine = options
            .engine_path
            .clone()
            .or_else(|| self.default_engine_path.clone())
            .unwrap_or_else(|| "audiveris".to_string());
        Self::normalize_engine_path(&engine)
    }

    fn normalize_engine_path(engine: &str) -> String {
        let path = Path::new(engine);
        let ext_is_app = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"));

        if ext_is_app {
            let candidate = path.join("Contents").join("MacOS").join("Audiveris");
            if candidate.exists() {
                return candidate.to_string_lossy().into_owned();
            }
        }

        engine.to_string()
    }

    fn make_workdir() -> Result<PathBuf, OmrError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| OmrError::Backend(e.to_string()))?
            .as_millis();
        let pid = std::process::id();
        let dir = std::env::temp_dir()
            .join("cadenza-omr")
            .join(format!("job-{}-{}", pid, now));
        fs::create_dir_all(&dir).map_err(|e| OmrError::Backend(e.to_string()))?;
        Ok(dir)
    }

    fn find_output_musicxml(output_dir: &Path, stem: &str) -> Option<PathBuf> {
        let mxl = output_dir.join(format!("{}.mxl", stem));
        if mxl.exists() {
            return Some(mxl);
        }
        let xml = output_dir.join(format!("{}.xml", stem));
        if xml.exists() {
            return Some(xml);
        }
        let entries = fs::read_dir(output_dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext.eq_ignore_ascii_case("mxl") || ext.eq_ignore_ascii_case("xml") {
                    return Some(path);
                }
            }
        }
        None
    }
}

impl OmrPort for AudiverisOmr {
    fn recognize_pdf(&self, pdf_path: &str, options: OmrOptions) -> Result<OmrResult, OmrError> {
        let engine = self.engine_path(&options);
        let input_path = Path::new(pdf_path);
        let stem = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| OmrError::UnsupportedFormat("invalid pdf filename".to_string()))?;

        let output_dir = Self::make_workdir()?;
        let output = Command::new(engine)
            .arg("-batch")
            .arg("-export")
            .arg("-output")
            .arg(&output_dir)
            .arg(input_path)
            .output()
            .map_err(|e| OmrError::Backend(e.to_string()))?;

        let diagnostics_path = if options.enable_diagnostics {
            let diag_path = output_dir.join("audiveris.log");
            let mut content = Vec::new();
            content.extend_from_slice(&output.stdout);
            content.extend_from_slice(&output.stderr);
            let _ = fs::write(&diag_path, content);
            Some(diag_path)
        } else {
            None
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(OmrError::RecognitionFailed(stderr));
        }

        let musicxml_path = Self::find_output_musicxml(&output_dir, stem)
            .ok_or_else(|| OmrError::RecognitionFailed("musicxml not found".to_string()))?;

        Ok(OmrResult {
            musicxml_path: Some(musicxml_path),
            diagnostics_path,
        })
    }

    fn diagnostics(&self) -> Result<Option<PathBuf>, OmrError> {
        Ok(None)
    }
}

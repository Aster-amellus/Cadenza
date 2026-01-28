use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct OmrOptions {
    pub enable_diagnostics: bool,
    pub engine_path: Option<String>,
}

#[derive(Clone, Debug)]
pub struct OmrResult {
    pub musicxml_path: Option<PathBuf>,
    pub diagnostics_path: Option<PathBuf>,
}

#[derive(thiserror::Error, Debug)]
pub enum OmrError {
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("recognition failed: {0}")]
    RecognitionFailed(String),
    #[error("backend error: {0}")]
    Backend(String),
}

pub trait OmrPort: Send + Sync {
    fn recognize_pdf(&self, pdf_path: &str, options: OmrOptions) -> Result<OmrResult, OmrError>;
    fn diagnostics(&self) -> Result<Option<PathBuf>, OmrError>;
}

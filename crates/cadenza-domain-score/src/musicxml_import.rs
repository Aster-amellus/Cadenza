use crate::model::Score;
use std::path::Path;

#[derive(thiserror::Error, Debug)]
pub enum MusicXmlImportError {
    #[error("io error: {0}")]
    Io(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("unsupported feature: {0}")]
    Unsupported(String),
}

pub fn import_musicxml_path(_path: &Path) -> Result<Score, MusicXmlImportError> {
    Err(MusicXmlImportError::Unsupported(
        "MusicXML import not implemented yet".to_string(),
    ))
}

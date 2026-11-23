//! Recording status helpers. Files are muxed by the media engine.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordStatus {
    Idle,
    Recording,
    Error,
}

impl RecordStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Not recording",
            Self::Recording => "Recording",
            Self::Error => "Recording error",
        }
    }
}

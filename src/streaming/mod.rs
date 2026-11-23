//! Streaming status and target validation. The GStreamer sink itself is
//! managed by `crate::media::MediaEngine`.

use crate::settings::StreamSettings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamStatus {
    Idle,
    Connecting,
    Live,
    Reconnecting,
    Error,
}

impl StreamStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Offline",
            Self::Connecting => "Connecting…",
            Self::Live => "Live",
            Self::Reconnecting => "Reconnecting…",
            Self::Error => "Error",
        }
    }
}

/// Validates a stream target without leaking the key into diagnostics.
pub fn validate(settings: &StreamSettings) -> Result<(), &'static str> {
    if settings.endpoint().is_none() {
        return Err("Server must start with rtmp:// or rtmps:// and a stream key is required");
    }
    if settings.bitrate_kbps == 0 {
        return Err("Stream bitrate must be greater than zero");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation() {
        let mut settings = StreamSettings::default();
        assert!(validate(&settings).is_err());
        settings.server_url = "rtmps://live.example.com/app".into();
        settings.stream_key = "key".into();
        settings.bitrate_kbps = 2500;
        assert!(validate(&settings).is_ok());
    }
}

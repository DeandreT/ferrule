use serde::{Deserialize, Deserializer, Serialize};

const DEFAULT_TIMEOUT_SECONDS: u16 = 30;

/// Whole-request timeout for a static HTTP GET source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct HttpTimeoutSeconds(u16);

impl HttpTimeoutSeconds {
    pub const MAX: u16 = 300;

    pub const fn new(seconds: u16) -> Option<Self> {
        if seconds >= 1 && seconds <= Self::MAX {
            Some(Self(seconds))
        } else {
            None
        }
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

impl Default for HttpTimeoutSeconds {
    fn default() -> Self {
        Self(DEFAULT_TIMEOUT_SECONDS)
    }
}

impl<'de> Deserialize<'de> for HttpTimeoutSeconds {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let seconds = u16::deserialize(deserializer)?;
        Self::new(seconds).ok_or_else(|| {
            serde::de::Error::custom(format_args!(
                "HTTP timeout must be between 1 and {} seconds",
                Self::MAX
            ))
        })
    }
}

/// Transport policy for a requestless HTTP GET source. The URL remains in
/// the owning source path so local file overrides keep their usual meaning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpGetOptions {
    #[serde(default)]
    timeout_seconds: HttpTimeoutSeconds,
}

impl HttpGetOptions {
    pub const fn new(timeout_seconds: HttpTimeoutSeconds) -> Self {
        Self { timeout_seconds }
    }

    pub const fn timeout_seconds(self) -> HttpTimeoutSeconds {
        self.timeout_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_is_bounded_and_defaults_to_thirty_seconds() {
        assert!(HttpTimeoutSeconds::new(0).is_none());
        assert!(HttpTimeoutSeconds::new(301).is_none());
        assert_eq!(
            HttpTimeoutSeconds::new(40).map(HttpTimeoutSeconds::get),
            Some(40)
        );

        let options: HttpGetOptions = serde_json::from_str("{}").unwrap();
        assert_eq!(options.timeout_seconds().get(), 30);
        assert!(serde_json::from_str::<HttpGetOptions>(r#"{"timeout_seconds":0}"#).is_err());
        assert!(serde_json::from_str::<HttpGetOptions>(r#"{"timeout_seconds":301}"#).is_err());
    }
}

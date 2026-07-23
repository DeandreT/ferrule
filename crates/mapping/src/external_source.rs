use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

use ir::SchemaNode;
use serde::{Deserialize, Deserializer, Serialize};

use crate::HttpTimeoutSeconds;

/// Serialization format of a response captured outside ferrule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalPayloadFormat {
    Json,
    Xml,
}

/// MapForce HTTP POST authoring mode retained for inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalHttpMode {
    Manual,
    Graphql,
}

/// One HTTP request header declaration. Values are deliberately excluded so
/// credentials cannot leak into a ferrule project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExternalHttpHeader {
    name: String,
    required: bool,
    mapped: bool,
}

impl ExternalHttpHeader {
    pub fn new(
        name: impl Into<String>,
        required: bool,
        mapped: bool,
    ) -> Result<Self, ExternalSourceOptionsError> {
        let name = nonempty(name.into()).ok_or(ExternalSourceOptionsError::EmptyHeaderName)?;
        Ok(Self {
            name,
            required,
            mapped,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn required(&self) -> bool {
        self.required
    }

    pub const fn mapped(&self) -> bool {
        self.mapped
    }
}

impl<'de> Deserialize<'de> for ExternalHttpHeader {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            name: String,
            #[serde(default)]
            required: bool,
            #[serde(default)]
            mapped: bool,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.name, wire.required, wire.mapped).map_err(serde::de::Error::custom)
    }
}

/// Origin metadata for one typed captured-response boundary.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExternalSourceOrigin {
    UserFunction {
        name: String,
        reason: String,
    },
    HttpPost {
        mode: ExternalHttpMode,
        timeout_seconds: HttpTimeoutSeconds,
        request_format: Option<ExternalPayloadFormat>,
        request_schema: Option<Box<SchemaNode>>,
        headers: Vec<ExternalHttpHeader>,
    },
}

/// A source whose value must be captured by an external host. Ferrule can
/// inspect and execute mappings against a local captured response, but never
/// invokes the owning UDF or sends the HTTP POST itself.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExternalSourceOptions {
    payload: ExternalPayloadFormat,
    origin: ExternalSourceOrigin,
}

impl ExternalSourceOptions {
    pub fn user_function(
        name: impl Into<String>,
        reason: impl Into<String>,
        payload: ExternalPayloadFormat,
    ) -> Result<Self, ExternalSourceOptionsError> {
        let name = nonempty(name.into()).ok_or(ExternalSourceOptionsError::EmptyFunctionName)?;
        let reason = nonempty(reason.into()).ok_or(ExternalSourceOptionsError::EmptyReason)?;
        Ok(Self {
            payload,
            origin: ExternalSourceOrigin::UserFunction { name, reason },
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn http_post(
        mode: ExternalHttpMode,
        timeout_seconds: HttpTimeoutSeconds,
        request_format: Option<ExternalPayloadFormat>,
        request_schema: Option<SchemaNode>,
        response_format: ExternalPayloadFormat,
        headers: Vec<ExternalHttpHeader>,
    ) -> Result<Self, ExternalSourceOptionsError> {
        if request_format.is_some() != request_schema.is_some() {
            return Err(ExternalSourceOptionsError::RequestShapeMismatch);
        }
        let mut names = BTreeSet::new();
        for header in &headers {
            if !names.insert(header.name.to_ascii_lowercase()) {
                return Err(ExternalSourceOptionsError::DuplicateHeader);
            }
        }
        Ok(Self {
            payload: response_format,
            origin: ExternalSourceOrigin::HttpPost {
                mode,
                timeout_seconds,
                request_format,
                request_schema: request_schema.map(Box::new),
                headers,
            },
        })
    }

    pub const fn payload(&self) -> ExternalPayloadFormat {
        self.payload
    }

    pub const fn origin(&self) -> &ExternalSourceOrigin {
        &self.origin
    }

    pub const fn is_http_post(&self) -> bool {
        matches!(self.origin, ExternalSourceOrigin::HttpPost { .. })
    }
}

impl<'de> Deserialize<'de> for ExternalSourceOptions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            payload: ExternalPayloadFormat,
            origin: OriginWire,
        }

        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum OriginWire {
            UserFunction {
                name: String,
                reason: String,
            },
            HttpPost {
                mode: ExternalHttpMode,
                timeout_seconds: HttpTimeoutSeconds,
                request_format: Option<ExternalPayloadFormat>,
                request_schema: Option<Box<SchemaNode>>,
                #[serde(default)]
                headers: Vec<ExternalHttpHeader>,
            },
        }

        let wire = Wire::deserialize(deserializer)?;
        match wire.origin {
            OriginWire::UserFunction { name, reason } => {
                Self::user_function(name, reason, wire.payload)
            }
            OriginWire::HttpPost {
                mode,
                timeout_seconds,
                request_format,
                request_schema,
                headers,
            } => Self::http_post(
                mode,
                timeout_seconds,
                request_format,
                request_schema.map(|schema| *schema),
                wire.payload,
                headers,
            ),
        }
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalSourceOptionsError {
    EmptyFunctionName,
    EmptyReason,
    EmptyHeaderName,
    DuplicateHeader,
    RequestShapeMismatch,
}

impl fmt::Display for ExternalSourceOptionsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyFunctionName => "external user-function name cannot be empty",
            Self::EmptyReason => "external source reason cannot be empty",
            Self::EmptyHeaderName => "external HTTP header name cannot be empty",
            Self::DuplicateHeader => "external HTTP header names must be unique",
            Self::RequestShapeMismatch => {
                "external HTTP request format and schema must either both be set or both be absent"
            }
        })
    }
}

impl Error for ExternalSourceOptionsError {}

fn nonempty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use ir::{ScalarType, SchemaNode};

    use super::*;

    #[test]
    fn boundaries_roundtrip_without_secret_header_values() -> Result<(), Box<dyn Error>> {
        let options = ExternalSourceOptions::http_post(
            ExternalHttpMode::Manual,
            HttpTimeoutSeconds::new(40).ok_or("invalid timeout")?,
            Some(ExternalPayloadFormat::Json),
            Some(SchemaNode::scalar("request", ScalarType::String)),
            ExternalPayloadFormat::Json,
            vec![ExternalHttpHeader::new("Authorization", true, true)?],
        )?;
        let encoded = serde_json::to_string(&options)?;
        assert!(!encoded.contains("secret"));
        let encoded_value = serde_json::from_str::<serde_json::Value>(&encoded)?;
        assert_eq!(
            encoded_value
                .pointer("/origin/request_schema/name")
                .and_then(serde_json::Value::as_str),
            Some("request")
        );
        assert_eq!(
            serde_json::from_str::<ExternalSourceOptions>(&encoded)?,
            options
        );

        let duplicate = ExternalSourceOptions::http_post(
            ExternalHttpMode::Manual,
            HttpTimeoutSeconds::default(),
            None,
            None,
            ExternalPayloadFormat::Json,
            vec![
                ExternalHttpHeader::new("Token", false, false)?,
                ExternalHttpHeader::new("token", true, true)?,
            ],
        );
        assert_eq!(duplicate, Err(ExternalSourceOptionsError::DuplicateHeader));
        Ok(())
    }
}

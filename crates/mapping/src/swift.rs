use std::collections::HashSet;

use serde::{Deserialize, Serialize};

const MAX_MESSAGES: usize = 256;
const MAX_FIELDS: usize = 4_096;
const MAX_EXPR_NODES: usize = 32_768;
const MAX_EXPR_DEPTH: usize = 128;

/// Portable SWIFT MT text-block grammar compiled from selected message
/// configurations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SwiftMtLayout {
    messages: Vec<SwiftMessageLayout>,
}

impl SwiftMtLayout {
    pub fn new(messages: Vec<SwiftMessageLayout>) -> Result<Self, SwiftMtLayoutError> {
        if messages.is_empty() {
            return Err(SwiftMtLayoutError::EmptyMessages);
        }
        if messages.len() > MAX_MESSAGES {
            return Err(SwiftMtLayoutError::Limit("message count"));
        }
        let mut message_types = HashSet::new();
        let mut fields = 0usize;
        let mut nodes = 0usize;
        for message in &messages {
            validate_token(&message.message_type, "message type")?;
            if !message_types.insert(message.message_type.as_str()) {
                return Err(SwiftMtLayoutError::DuplicateMessage(
                    message.message_type.clone(),
                ));
            }
            fields = fields
                .checked_add(message.fields.len())
                .ok_or(SwiftMtLayoutError::Limit("field count"))?;
            let mut paths = HashSet::new();
            for field in &message.fields {
                validate_token(&field.tag, "field tag")?;
                validate_path(&field.path)?;
                if !paths.insert(field.path.as_slice()) {
                    return Err(SwiftMtLayoutError::DuplicateFieldPath {
                        message: message.message_type.clone(),
                        path: field.path.join("/"),
                    });
                }
                validate_expr(&field.value, 1, &mut nodes)?;
            }
        }
        if fields > MAX_FIELDS {
            return Err(SwiftMtLayoutError::Limit("field count"));
        }
        if nodes > MAX_EXPR_NODES {
            return Err(SwiftMtLayoutError::Limit("expression node count"));
        }
        Ok(Self { messages })
    }

    pub fn messages(&self) -> &[SwiftMessageLayout] {
        &self.messages
    }

    pub fn message(&self, message_type: &str) -> Option<&SwiftMessageLayout> {
        self.messages
            .iter()
            .find(|message| message.message_type == message_type)
    }
}

impl<'de> Deserialize<'de> for SwiftMtLayout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr {
            messages: Vec<SwiftMessageLayout>,
        }
        let value = Repr::deserialize(deserializer)?;
        Self::new(value.messages).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwiftMessageLayout {
    message_type: String,
    fields: Vec<SwiftFieldLayout>,
}

impl SwiftMessageLayout {
    pub fn new(message_type: impl Into<String>, fields: Vec<SwiftFieldLayout>) -> Self {
        Self {
            message_type: message_type.into(),
            fields,
        }
    }

    pub fn message_type(&self) -> &str {
        &self.message_type
    }

    pub fn fields(&self) -> &[SwiftFieldLayout] {
        &self.fields
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwiftFieldLayout {
    tag: String,
    path: Vec<String>,
    repeating: bool,
    value: SwiftValueExpr,
}

impl SwiftFieldLayout {
    pub fn new(
        tag: impl Into<String>,
        path: Vec<String>,
        repeating: bool,
        value: SwiftValueExpr,
    ) -> Self {
        Self {
            tag: tag.into(),
            path,
            repeating,
            value,
        }
    }

    pub fn tag(&self) -> &str {
        &self.tag
    }

    pub fn path(&self) -> &[String] {
        &self.path
    }

    pub const fn repeating(&self) -> bool {
        self.repeating
    }

    pub const fn value(&self) -> &SwiftValueExpr {
        &self.value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwiftCharset {
    Numeric,
    Alphabetic,
    Alphanumeric,
    Decimal,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SwiftValueExpr {
    Empty,
    Literal {
        value: String,
    },
    Capture {
        path: Vec<String>,
        min: u16,
        max: u16,
        charset: SwiftCharset,
    },
    EnumCapture {
        path: Vec<String>,
        values: Vec<String>,
    },
    Sequence {
        parts: Vec<SwiftValueExpr>,
    },
    Alternatives {
        choices: Vec<SwiftValueExpr>,
    },
    Optional {
        value: Box<SwiftValueExpr>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwiftMtLayoutError {
    EmptyMessages,
    EmptyToken(&'static str),
    InvalidToken(&'static str),
    EmptyPath,
    InvalidCaptureRange,
    EmptyEnum,
    DuplicateMessage(String),
    DuplicateFieldPath { message: String, path: String },
    Limit(&'static str),
}

impl std::fmt::Display for SwiftMtLayoutError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyMessages => formatter.write_str("SWIFT MT layout has no messages"),
            Self::EmptyToken(kind) => write!(formatter, "SWIFT {kind} must not be empty"),
            Self::InvalidToken(kind) => write!(formatter, "SWIFT {kind} is not ASCII text"),
            Self::EmptyPath => formatter.write_str("SWIFT field/capture path must not be empty"),
            Self::InvalidCaptureRange => formatter.write_str("invalid SWIFT capture length range"),
            Self::EmptyEnum => formatter.write_str("SWIFT enum capture has no alternatives"),
            Self::DuplicateMessage(value) => write!(formatter, "duplicate SWIFT message `{value}`"),
            Self::DuplicateFieldPath { message, path } => {
                write!(
                    formatter,
                    "duplicate SWIFT field path `{path}` in `{message}`"
                )
            }
            Self::Limit(limit) => write!(formatter, "SWIFT MT layout exceeds the {limit} limit"),
        }
    }
}

impl std::error::Error for SwiftMtLayoutError {}

fn validate_token(value: &str, kind: &'static str) -> Result<(), SwiftMtLayoutError> {
    if value.is_empty() {
        return Err(SwiftMtLayoutError::EmptyToken(kind));
    }
    if !value.is_ascii() || value.chars().any(char::is_control) {
        return Err(SwiftMtLayoutError::InvalidToken(kind));
    }
    Ok(())
}

fn validate_path(path: &[String]) -> Result<(), SwiftMtLayoutError> {
    if path.is_empty() {
        return Err(SwiftMtLayoutError::EmptyPath);
    }
    for part in path {
        validate_token(part, "path segment")?;
    }
    Ok(())
}

fn validate_expr(
    expression: &SwiftValueExpr,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), SwiftMtLayoutError> {
    if depth > MAX_EXPR_DEPTH {
        return Err(SwiftMtLayoutError::Limit("expression depth"));
    }
    *nodes = nodes
        .checked_add(1)
        .ok_or(SwiftMtLayoutError::Limit("expression node count"))?;
    match expression {
        SwiftValueExpr::Empty | SwiftValueExpr::Literal { .. } => Ok(()),
        SwiftValueExpr::Capture { path, min, max, .. } => {
            if !path.is_empty() {
                validate_path(path)?;
            }
            if *max == 0 || min > max {
                return Err(SwiftMtLayoutError::InvalidCaptureRange);
            }
            Ok(())
        }
        SwiftValueExpr::EnumCapture { path, values } => {
            validate_path(path)?;
            if values.is_empty() {
                return Err(SwiftMtLayoutError::EmptyEnum);
            }
            for value in values {
                validate_token(value, "enum value")?;
            }
            Ok(())
        }
        SwiftValueExpr::Sequence { parts } => {
            for part in parts {
                validate_expr(part, depth + 1, nodes)?;
            }
            Ok(())
        }
        SwiftValueExpr::Alternatives { choices } => {
            if choices.is_empty() {
                return Err(SwiftMtLayoutError::EmptyEnum);
            }
            for choice in choices {
                validate_expr(choice, depth + 1, nodes)?;
            }
            Ok(())
        }
        SwiftValueExpr::Optional { value } => validate_expr(value, depth + 1, nodes),
    }
}

use crate::{MAX_SCHEMA_BYTES, ProtobufError};

/// Stable identifier for a resolved message within one [`Layout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageId(pub(super) usize);

impl MessageId {
    pub fn index(self) -> usize {
        self.0
    }
}

/// Stable identifier for a resolved enum within one [`Layout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnumId(pub(super) usize);

impl EnumId {
    pub fn index(self) -> usize {
        self.0
    }
}

/// Stable identifier for a resolved oneof declaration within one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OneofId(pub(super) usize);

impl OneofId {
    pub fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    Required,
    Optional,
    /// A proto3 singular field without an explicit presence label.
    Implicit,
    Repeated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarType {
    Double,
    Float,
    Int32,
    Int64,
    Uint32,
    Uint64,
    Sint32,
    Sint64,
    Fixed32,
    Fixed64,
    Sfixed32,
    Sfixed64,
    Bool,
    String,
    Bytes,
}

impl ScalarType {
    pub(super) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "double" => Self::Double,
            "float" => Self::Float,
            "int32" => Self::Int32,
            "int64" => Self::Int64,
            "uint32" => Self::Uint32,
            "uint64" => Self::Uint64,
            "sint32" => Self::Sint32,
            "sint64" => Self::Sint64,
            "fixed32" => Self::Fixed32,
            "fixed64" => Self::Fixed64,
            "sfixed32" => Self::Sfixed32,
            "sfixed64" => Self::Sfixed64,
            "bool" => Self::Bool,
            "string" => Self::String,
            "bytes" => Self::Bytes,
            _ => return None,
        })
    }

    pub(crate) fn is_packable(self) -> bool {
        !matches!(self, Self::String | Self::Bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Scalar(ScalarType),
    Message(MessageId),
    Enum(EnumId),
}

#[derive(Debug, Clone, PartialEq)]
pub enum DefaultValue {
    Float(f64),
    Signed(i64),
    Unsigned(u64),
    Bool(bool),
    String(String),
    Bytes(Vec<u8>),
    Enum(i32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub(super) name: String,
    pub(super) number: u32,
    pub(super) cardinality: Cardinality,
    pub(super) ty: FieldType,
    pub(super) packed: bool,
    pub(super) default: Option<DefaultValue>,
    pub(super) oneof: Option<OneofId>,
}

impl Field {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn number(&self) -> u32 {
        self.number
    }

    pub fn cardinality(&self) -> Cardinality {
        self.cardinality
    }

    pub fn ty(&self) -> FieldType {
        self.ty
    }

    pub fn packed(&self) -> bool {
        self.packed
    }

    pub fn default(&self) -> Option<&DefaultValue> {
        self.default.as_ref()
    }

    pub fn oneof(&self) -> Option<OneofId> {
        self.oneof
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Oneof {
    pub(super) name: String,
}

impl Oneof {
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub(super) name: String,
    pub(super) full_name: String,
    pub(super) fields: Vec<Field>,
    pub(super) oneofs: Vec<Oneof>,
}

impl Message {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn full_name(&self) -> &str {
        &self.full_name
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|field| field.name == name)
    }

    pub fn oneofs(&self) -> &[Oneof] {
        &self.oneofs
    }

    pub fn oneof(&self, id: OneofId) -> Option<&Oneof> {
        self.oneofs.get(id.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumValue {
    pub(super) name: String,
    pub(super) number: i32,
}

impl EnumValue {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn number(&self) -> i32 {
        self.number
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enum {
    pub(super) name: String,
    pub(super) full_name: String,
    pub(super) values: Vec<EnumValue>,
}

impl Enum {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn full_name(&self) -> &str {
        &self.full_name
    }

    pub fn values(&self) -> &[EnumValue] {
        &self.values
    }

    pub fn value_by_name(&self, name: &str) -> Option<&EnumValue> {
        self.values.iter().find(|value| value.name == name)
    }

    pub fn value_by_number(&self, number: i32) -> Option<&EnumValue> {
        self.values.iter().find(|value| value.number == number)
    }
}

/// Fully resolved and validated proto2/proto3-lite schema.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub(super) package: Option<String>,
    pub(super) messages: Vec<Message>,
    pub(super) enums: Vec<Enum>,
}

impl Layout {
    pub fn parse(source: &str) -> Result<Self, ProtobufError> {
        if source.len() > MAX_SCHEMA_BYTES {
            return Err(ProtobufError::schema(format!(
                "schema exceeds the {MAX_SCHEMA_BYTES}-byte limit"
            )));
        }
        super::parser::parse(source)
    }

    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn enums(&self) -> &[Enum] {
        &self.enums
    }

    pub fn message(&self, id: MessageId) -> Option<&Message> {
        self.messages.get(id.0)
    }

    pub fn enumeration(&self, id: EnumId) -> Option<&Enum> {
        self.enums.get(id.0)
    }

    pub fn resolve_message(&self, name: &str) -> Result<MessageId, ProtobufError> {
        let canonical = name.strip_prefix('.').unwrap_or(name);
        if let Some((index, _)) = self
            .messages
            .iter()
            .enumerate()
            .find(|(_, message)| message.full_name == canonical)
        {
            return Ok(MessageId(index));
        }
        let mut matches = self
            .messages
            .iter()
            .enumerate()
            .filter(|(_, message)| message.name == canonical);
        let Some((index, _)) = matches.next() else {
            return Err(ProtobufError::UnknownRoot(name.to_string()));
        };
        if matches.next().is_some() {
            return Err(ProtobufError::AmbiguousRoot(name.to_string()));
        }
        Ok(MessageId(index))
    }
}

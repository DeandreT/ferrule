use std::io::Write;

use ir::{Instance, Value, XmlTypeOrigin};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

use crate::{
    CodecError, CodecErrorKind, EncodedBody, InstanceDescriptor, MAX_INSTANCE_BYTES, Sha256Digest,
    encode, scan,
};

#[derive(Default)]
struct Budget {
    nodes: usize,
    values: usize,
}
impl Budget {
    fn node(&mut self, depth: usize) -> Result<(), CodecError> {
        if depth > format_pdf::MAX_INSTANCE_DEPTH {
            return Err(CodecError::new(CodecErrorKind::InstanceDepth));
        }
        if self.nodes == format_pdf::MAX_OUTPUT_NODES {
            return Err(CodecError::new(CodecErrorKind::InstanceNodes));
        }
        self.nodes += 1;
        Ok(())
    }
    fn value(&mut self, size: usize) -> Result<(), CodecError> {
        self.values = self
            .values
            .checked_add(size)
            .filter(|n| *n <= format_pdf::MAX_VALUE_BYTES)
            .ok_or_else(|| CodecError::new(CodecErrorKind::InstanceValueBytes))?;
        Ok(())
    }
}
fn validate(node: &Instance, budget: &mut Budget, depth: usize) -> Result<(), CodecError> {
    budget.node(depth)?;
    match node {
        Instance::Scalar(Value::Null) => {}
        Instance::Scalar(Value::String(value)) => budget.value(value.len())?,
        Instance::Group(fields) => {
            if !matches!(fields.xml_type_origin(), XmlTypeOrigin::Unknown) {
                return Err(CodecError::new(CodecErrorKind::UnsupportedXmlOrigin));
            }
            for (_, child) in fields.iter() {
                validate(child, budget, depth + 1)?;
            }
        }
        Instance::Repeated(items) => {
            for child in items {
                validate(child, budget, depth + 1)?;
            }
        }
        _ => return Err(CodecError::new(CodecErrorKind::UnsupportedInstanceKind)),
    }
    Ok(())
}
fn node(bytes: &[u8], budget: &mut Budget, depth: usize) -> Result<Instance, CodecError> {
    budget.node(depth)?;
    let fields = scan::object(bytes, &["kind", "fields", "items", "value"])?;
    let kind = scan::small_text(
        fields.required("kind")?,
        32,
        CodecErrorKind::UnsupportedInstanceKind,
    )?;
    match kind.as_str() {
        "null" => {
            fields.exact(&["kind"])?;
            Ok(Instance::Scalar(Value::Null))
        }
        "string" => {
            fields.exact(&["kind", "value"])?;
            let raw = fields.required("value")?;
            // Count escaped UTF-8 BEFORE serde allocates an owned String.
            budget.value(scan::text_len(raw)?)?;
            Ok(Instance::Scalar(Value::String(scan::text(raw)?)))
        }
        "group" => {
            fields.exact(&["kind", "fields"])?;
            let mut array = scan::Array::new(fields.required("fields")?)?;
            let mut values = Vec::new();
            while let Some(pair) = array.next()? {
                let mut pair = scan::Array::new(pair)
                    .map_err(|_| CodecError::new(CodecErrorKind::InvalidPairArity))?;
                let name = pair
                    .next()?
                    .ok_or_else(|| CodecError::new(CodecErrorKind::InvalidPairArity))?;
                let child = pair
                    .next()?
                    .ok_or_else(|| CodecError::new(CodecErrorKind::InvalidPairArity))?;
                if pair.next()?.is_some() {
                    return Err(CodecError::new(CodecErrorKind::InvalidPairArity));
                }
                // No untrusted size_hint/reservation; each child is budgeted before push.
                let child = node(child, budget, depth + 1)?;
                values.push((scan::text(name)?, child));
            }
            Ok(Instance::Group(values.into()))
        }
        "repeated" => {
            fields.exact(&["kind", "items"])?;
            let mut array = scan::Array::new(fields.required("items")?)?;
            let mut values = Vec::new();
            while let Some(raw) = array.next()? {
                values.push(node(raw, budget, depth + 1)?);
            }
            Ok(Instance::Repeated(values))
        }
        _ => Err(CodecError::new(CodecErrorKind::UnsupportedInstanceKind)),
    }
}
pub fn decode_verified_instance(
    bytes: &[u8],
    expected: &InstanceDescriptor,
) -> Result<Instance, CodecError> {
    if bytes.len() > MAX_INSTANCE_BYTES {
        return Err(CodecError::new(CodecErrorKind::InstanceTooLarge));
    }
    if bytes.len() as u64 != expected.byte_len() {
        return Err(CodecError::new(CodecErrorKind::LengthMismatch));
    }
    if Sha256Digest::of(bytes) != expected.sha256() {
        return Err(CodecError::new(CodecErrorKind::DigestMismatch));
    }
    scan::prepare(bytes, MAX_INSTANCE_BYTES, CodecErrorKind::InstanceTooLarge)?;
    node(bytes, &mut Budget::default(), 0)
}
struct Wire<'a>(&'a Instance);
struct Items<'a>(&'a [Instance]);
struct Fields<'a>(&'a [(String, Instance)]);
struct Pair<'a>(&'a str, &'a Instance);
impl Serialize for Pair<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(2))?;
        seq.serialize_element(self.0)?;
        seq.serialize_element(&Wire(self.1))?;
        seq.end()
    }
}
impl Serialize for Fields<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.0.len()))?;
        for (name, node) in self.0 {
            seq.serialize_element(&Pair(name, node))?;
        }
        seq.end()
    }
}
impl Serialize for Items<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.0.len()))?;
        for node in self.0 {
            seq.serialize_element(&Wire(node))?;
        }
        seq.end()
    }
}
impl Serialize for Wire<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        match self.0 {
            Instance::Scalar(Value::Null) => map.serialize_entry("kind", "null")?,
            Instance::Scalar(Value::String(text)) => {
                map.serialize_entry("kind", "string")?;
                map.serialize_entry("value", text)?;
            }
            Instance::Group(fields) => {
                map.serialize_entry("kind", "group")?;
                map.serialize_entry("fields", &Fields(fields))?;
            }
            Instance::Repeated(items) => {
                map.serialize_entry("kind", "repeated")?;
                map.serialize_entry("items", &Items(items))?;
            }
            _ => return Err(serde::ser::Error::custom("unsupported PDF instance")),
        }
        map.end()
    }
}
pub fn encode_instance<W: Write>(
    instance: &Instance,
    output: &mut W,
) -> Result<EncodedBody, CodecError> {
    validate(instance, &mut Budget::default(), 0)?;
    encode(
        &Wire(instance),
        output,
        MAX_INSTANCE_BYTES,
        CodecErrorKind::InstanceTooLarge,
    )
}

//! Bounded serde-to-JSON conversion which does not erase non-finite floats.
use super::{FileCodecError, MAX_DEPTH, MAX_DOCUMENT_BYTES};
use serde::ser::{self, Serialize};
use serde_json::{Map, Number, Value};

pub(super) fn to_value<T: Serialize>(value: &T) -> Result<Value, FileCodecError> {
    value.serialize(Tree {
        path: String::new(),
        depth: 0,
        bytes: &mut 0,
    })
}

struct Tree<'a> {
    path: String,
    depth: usize,
    bytes: &'a mut usize,
}
impl<'a> Tree<'a> {
    fn charge(&mut self, bytes: usize) -> Result<(), FileCodecError> {
        *self.bytes = self.bytes.saturating_add(bytes);
        if *self.bytes > MAX_DOCUMENT_BYTES {
            return Err(FileCodecError::TooLarge {
                bytes: *self.bytes,
                max: MAX_DOCUMENT_BYTES,
            });
        }
        Ok(())
    }
    fn scalar(mut self, value: Value) -> Result<Value, FileCodecError> {
        self.charge(match &value {
            Value::String(s) => s.len().saturating_add(2),
            Value::Null => 4,
            Value::Bool(true) => 4,
            Value::Bool(false) => 5,
            _ => 1,
        })?;
        Ok(value)
    }
    fn compound(mut self, variant: Option<&str>) -> Result<Compound<'a>, FileCodecError> {
        self.depth += if variant.is_some() { 2 } else { 1 };
        if self.depth > MAX_DEPTH {
            return Err(FileCodecError::DepthLimit {
                depth: self.depth,
                max: MAX_DEPTH,
            });
        }
        self.charge(if variant.is_some() { 4 } else { 2 })?;
        let path = match variant {
            Some(name) => child_path(&self.path, name),
            None => self.path,
        };
        Ok(Compound {
            path,
            depth: self.depth,
            bytes: self.bytes,
            array: Vec::new(),
            object: Map::new(),
            key: None,
            variant: variant.map(str::to_owned),
        })
    }
}
pub(super) fn child_path(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}
impl<'a> ser::Serializer for Tree<'a> {
    type Ok = Value;
    type Error = FileCodecError;
    type SerializeSeq = Compound<'a>;
    type SerializeTuple = Compound<'a>;
    type SerializeTupleStruct = Compound<'a>;
    type SerializeTupleVariant = Compound<'a>;
    type SerializeMap = Compound<'a>;
    type SerializeStruct = Compound<'a>;
    type SerializeStructVariant = Compound<'a>;
    fn serialize_bool(self, v: bool) -> Result<Value, Self::Error> {
        self.scalar(Value::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<Value, Self::Error> {
        self.serialize_i64(v.into())
    }
    fn serialize_i16(self, v: i16) -> Result<Value, Self::Error> {
        self.serialize_i64(v.into())
    }
    fn serialize_i32(self, v: i32) -> Result<Value, Self::Error> {
        self.serialize_i64(v.into())
    }
    fn serialize_i64(self, v: i64) -> Result<Value, Self::Error> {
        self.scalar(Value::Number(v.into()))
    }
    fn serialize_u8(self, v: u8) -> Result<Value, Self::Error> {
        self.serialize_u64(v.into())
    }
    fn serialize_u16(self, v: u16) -> Result<Value, Self::Error> {
        self.serialize_u64(v.into())
    }
    fn serialize_u32(self, v: u32) -> Result<Value, Self::Error> {
        self.serialize_u64(v.into())
    }
    fn serialize_u64(self, v: u64) -> Result<Value, Self::Error> {
        self.scalar(Value::Number(v.into()))
    }
    fn serialize_f32(self, v: f32) -> Result<Value, Self::Error> {
        self.serialize_f64(v.into())
    }
    fn serialize_f64(self, v: f64) -> Result<Value, Self::Error> {
        let number = Number::from_f64(v).ok_or_else(|| FileCodecError::NonFiniteValue {
            path: self.path.clone(),
        })?;
        self.scalar(Value::Number(number))
    }
    fn serialize_char(self, v: char) -> Result<Value, Self::Error> {
        self.serialize_str(&v.to_string())
    }
    fn serialize_str(mut self, v: &str) -> Result<Value, Self::Error> {
        self.charge(v.len().saturating_add(2))?;
        Ok(Value::String(v.to_owned()))
    }
    fn serialize_bytes(mut self, v: &[u8]) -> Result<Value, Self::Error> {
        self.charge(v.len().saturating_mul(2))?;
        let mut seq = self.compound(None)?;
        for byte in v {
            seq.push(byte)?;
        }
        Ok(Value::Array(seq.array))
    }
    fn serialize_none(self) -> Result<Value, Self::Error> {
        self.scalar(Value::Null)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, v: &T) -> Result<Value, Self::Error> {
        v.serialize(self)
    }
    fn serialize_unit(self) -> Result<Value, Self::Error> {
        self.serialize_none()
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<Value, Self::Error> {
        self.serialize_none()
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<Value, Self::Error> {
        self.serialize_str(variant)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        v: &T,
    ) -> Result<Value, Self::Error> {
        v.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        v: &T,
    ) -> Result<Value, Self::Error> {
        let mut object = self.compound(None)?;
        object.field(variant, v)?;
        Ok(Value::Object(object.object))
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        self.compound(None)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Self::Error> {
        self.compound(None)
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        self.compound(None)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        self.compound(Some(variant))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        self.compound(None)
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        self.compound(None)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        self.compound(Some(variant))
    }
}
struct Compound<'a> {
    path: String,
    depth: usize,
    bytes: &'a mut usize,
    array: Vec<Value>,
    object: Map<String, Value>,
    key: Option<String>,
    variant: Option<String>,
}
impl Compound<'_> {
    fn push<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), FileCodecError> {
        let value = v.serialize(Tree {
            path: child_path(&self.path, &self.array.len().to_string()),
            depth: self.depth,
            bytes: self.bytes,
        })?;
        self.array.push(value);
        Ok(())
    }
    fn field<T: Serialize + ?Sized>(&mut self, key: &str, v: &T) -> Result<(), FileCodecError> {
        *self.bytes = self.bytes.saturating_add(key.len().saturating_add(3));
        if *self.bytes > MAX_DOCUMENT_BYTES {
            return Err(FileCodecError::TooLarge {
                bytes: *self.bytes,
                max: MAX_DOCUMENT_BYTES,
            });
        }
        let value = v.serialize(Tree {
            path: child_path(&self.path, key),
            depth: self.depth,
            bytes: self.bytes,
        })?;
        if self.object.insert(key.to_owned(), value).is_some() {
            return Err(FileCodecError::InvalidEnvelope {
                message: format!("duplicate serialized field {key}"),
            });
        }
        Ok(())
    }
    fn finish(self, value: Value) -> Value {
        match self.variant {
            None => value,
            Some(name) => {
                let mut object = Map::new();
                object.insert(name, value);
                Value::Object(object)
            }
        }
    }
}
macro_rules! sequence {
    ($trait:ident, $method:ident) => {
        impl ser::$trait for Compound<'_> {
            type Ok = Value;
            type Error = FileCodecError;
            fn $method<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
                self.push(v)
            }
            fn end(self) -> Result<Value, Self::Error> {
                Ok(Value::Array(self.array))
            }
        }
    };
}
sequence!(SerializeSeq, serialize_element);
sequence!(SerializeTuple, serialize_element);
sequence!(SerializeTupleStruct, serialize_field);
impl ser::SerializeTupleVariant for Compound<'_> {
    type Ok = Value;
    type Error = FileCodecError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        self.push(v)
    }
    fn end(mut self) -> Result<Value, Self::Error> {
        let array = std::mem::take(&mut self.array);
        Ok(self.finish(Value::Array(array)))
    }
}
impl ser::SerializeMap for Compound<'_> {
    type Ok = Value;
    type Error = FileCodecError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.key = Some(
            match serde_json::to_value(key).map_err(FileCodecError::Serialization)? {
                Value::String(s) => s,
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => {
                    return Err(FileCodecError::InvalidEnvelope {
                        message: "model has an unsupported JSON object key".into(),
                    });
                }
            },
        );
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), Self::Error> {
        let key = self
            .key
            .take()
            .ok_or_else(|| FileCodecError::InvalidEnvelope {
                message: "model object value has no key".into(),
            })?;
        self.field(&key, v)
    }
    fn end(self) -> Result<Value, Self::Error> {
        Ok(Value::Object(self.object))
    }
}
impl ser::SerializeStruct for Compound<'_> {
    type Ok = Value;
    type Error = FileCodecError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        v: &T,
    ) -> Result<(), Self::Error> {
        self.field(key, v)
    }
    fn end(self) -> Result<Value, Self::Error> {
        Ok(Value::Object(self.object))
    }
}
impl ser::SerializeStructVariant for Compound<'_> {
    type Ok = Value;
    type Error = FileCodecError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        v: &T,
    ) -> Result<(), Self::Error> {
        self.field(key, v)
    }
    fn end(mut self) -> Result<Value, Self::Error> {
        let object = std::mem::take(&mut self.object);
        Ok(self.finish(Value::Object(object)))
    }
}

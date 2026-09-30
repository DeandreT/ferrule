//! Comparison key for editor history and layout sidecars.
//!
//! Keep the ordinary JSON bytes for every finite project so existing layout
//! fingerprints remain valid. Serde writes non-finite floats as `null`, which
//! can also represent an absent value; append their exact positions and bits
//! only when they occur. History restores cloned projects, never this key.

use std::fmt;

use mapping::{Pipeline, Project};
use serde::ser::{self, Serialize};

pub(crate) fn project_snapshot_key(project: &Project) -> String {
    snapshot_key(project)
}

pub(crate) fn pipeline_snapshot_key(pipeline: &Pipeline) -> String {
    snapshot_key(pipeline)
}

fn snapshot_key<T: Serialize>(model: &T) -> String {
    let mut key = serde_json::to_string(model).expect("model serialization cannot fail");
    if !key.contains("null") {
        return key;
    }
    let mut markers = Vec::new();
    model
        .serialize(FloatVisitor {
            path: String::new(),
            markers: &mut markers,
        })
        .expect("model float traversal cannot fail after serialization");
    for marker in markers {
        key.push_str("\n#nonfinite:");
        key.push_str(&marker.path.len().to_string());
        key.push(':');
        key.push_str(&marker.path);
        key.push(':');
        key.push_str(marker.kind);
        key.push(':');
        key.push_str(&format!("{:016x}", marker.bits));
    }
    key
}

struct Marker {
    path: String,
    kind: &'static str,
    bits: u64,
}

#[derive(Debug)]
struct VisitError;

impl fmt::Display for VisitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("could not traverse project float values")
    }
}

impl std::error::Error for VisitError {}

impl ser::Error for VisitError {
    fn custom<T: fmt::Display>(_: T) -> Self {
        Self
    }
}

fn child_path(parent: &str, kind: char, name: &str) -> String {
    format!("{parent}/{kind}{}:{name}", name.len())
}

struct FloatVisitor<'a> {
    path: String,
    markers: &'a mut Vec<Marker>,
}

impl<'a> FloatVisitor<'a> {
    fn children(self, variant: Option<&str>) -> Children<'a> {
        Children {
            path: variant.map_or(self.path.clone(), |name| child_path(&self.path, 'v', name)),
            markers: self.markers,
            next_index: 0,
            pending_key: None,
        }
    }
}

impl<'a> ser::Serializer for FloatVisitor<'a> {
    type Ok = ();
    type Error = VisitError;
    type SerializeSeq = Children<'a>;
    type SerializeTuple = Children<'a>;
    type SerializeTupleStruct = Children<'a>;
    type SerializeTupleVariant = Children<'a>;
    type SerializeMap = Children<'a>;
    type SerializeStruct = Children<'a>;
    type SerializeStructVariant = Children<'a>;

    fn serialize_bool(self, _: bool) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_i8(self, _: i8) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_i16(self, _: i16) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_i32(self, _: i32) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_i64(self, _: i64) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_u8(self, _: u8) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_u16(self, _: u16) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_u32(self, _: u32) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_u64(self, _: u64) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_f32(self, value: f32) -> Result<(), Self::Error> {
        if !value.is_finite() {
            self.markers.push(Marker {
                path: self.path,
                kind: "f32",
                bits: u64::from(value.to_bits()),
            });
        }
        Ok(())
    }
    fn serialize_f64(self, value: f64) -> Result<(), Self::Error> {
        if !value.is_finite() {
            self.markers.push(Marker {
                path: self.path,
                kind: "f64",
                bits: value.to_bits(),
            });
        }
        Ok(())
    }
    fn serialize_char(self, _: char) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_str(self, _: &str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_none(self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), Self::Error> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        value.serialize(FloatVisitor {
            path: child_path(&self.path, 'v', variant),
            markers: self.markers,
        })
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(self.children(None))
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(self.children(None))
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(self.children(None))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Ok(self.children(Some(variant)))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(self.children(None))
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(self.children(None))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Ok(self.children(Some(variant)))
    }
}

struct Children<'a> {
    path: String,
    markers: &'a mut Vec<Marker>,
    next_index: usize,
    pending_key: Option<String>,
}

impl Children<'_> {
    fn element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), VisitError> {
        let path = child_path(&self.path, 'i', &self.next_index.to_string());
        self.next_index += 1;
        value.serialize(FloatVisitor {
            path,
            markers: self.markers,
        })
    }

    fn field<T: Serialize + ?Sized>(&mut self, name: &str, value: &T) -> Result<(), VisitError> {
        value.serialize(FloatVisitor {
            path: child_path(&self.path, 'f', name),
            markers: self.markers,
        })
    }
}

macro_rules! sequence {
    ($trait:ident, $method:ident) => {
        impl ser::$trait for Children<'_> {
            type Ok = ();
            type Error = VisitError;
            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
                self.element(value)
            }
            fn end(self) -> Result<(), Self::Error> {
                Ok(())
            }
        }
    };
}
sequence!(SerializeSeq, serialize_element);
sequence!(SerializeTuple, serialize_element);
sequence!(SerializeTupleStruct, serialize_field);

impl ser::SerializeTupleVariant for Children<'_> {
    type Ok = ();
    type Error = VisitError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.element(value)
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl ser::SerializeMap for Children<'_> {
    type Ok = ();
    type Error = VisitError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.pending_key = Some(serde_json::to_string(key).map_err(|_| VisitError)?);
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        let key = self.pending_key.take().ok_or(VisitError)?;
        value.serialize(FloatVisitor {
            path: child_path(&self.path, 'm', &key),
            markers: self.markers,
        })
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl ser::SerializeStruct for Children<'_> {
    type Ok = ();
    type Error = VisitError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.field(name, value)
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl ser::SerializeStructVariant for Children<'_> {
    type Ok = ();
    type Error = VisitError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.field(name, value)
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::Value;
    use mapping::{Node, PipelineStage};

    #[test]
    fn finite_snapshot_keeps_legacy_bytes_and_nonfinite_bits_stay_distinct() {
        let mut project = crate::new_mapping::blank_project();
        project.graph.nodes.insert(
            1,
            Node::Const {
                value: Value::Float(f64::from_bits(0x3feffffffffffc19)),
            },
        );
        assert_eq!(
            project_snapshot_key(&project),
            serde_json::to_string(&project).unwrap()
        );

        project
            .graph
            .nodes
            .insert(1, Node::Const { value: Value::Null });
        let null_key = project_snapshot_key(&project);
        project.graph.nodes.insert(
            1,
            Node::Const {
                value: Value::Float(f64::from_bits(0x7ff8000000000001)),
            },
        );
        let first_nan_key = project_snapshot_key(&project);
        assert_ne!(first_nan_key, null_key);
        project.graph.nodes.insert(
            1,
            Node::Const {
                value: Value::Float(f64::from_bits(0x7ff8000000000002)),
            },
        );
        assert_ne!(project_snapshot_key(&project), first_nan_key);

        project.graph.nodes.insert(
            2,
            Node::ValueMap {
                input: 1,
                input_type: None,
                table: vec![(Value::String("a".into()), Value::Float(f64::INFINITY))],
                default: None,
            },
        );
        let with_map_float = project_snapshot_key(&project);
        if let Some(Node::ValueMap { table, .. }) = project.graph.nodes.get_mut(&2) {
            table[0].1 = Value::Null;
        }
        assert_ne!(project_snapshot_key(&project), with_map_float);

        let pipeline = Pipeline {
            main_mapping_path: None,
            stages: vec![PipelineStage {
                id: "first".into(),
                mapping_path: None,
                project,
                source: mapping::PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: Vec::new(),
            }],
        };
        assert_ne!(
            pipeline_snapshot_key(&pipeline),
            serde_json::to_string(&pipeline).unwrap()
        );
    }
}

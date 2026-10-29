//! Shared invariants for the protobuf wire-decoder fuzz target.
//!
//! Every input is decoded against each root in [`roots`] through the public
//! `format_protobuf::from_slice` API. A decode must either succeed or return a
//! typed `ProtobufError::InvalidInstance` scoped to that root; anything else,
//! including a panic, is a finding. Successful decodes must stay within the
//! per-input budget documented in the README and must survive an
//! encode/decode round trip unchanged.

use format_protobuf::{Layout, ProtobufError, from_slice, to_vec};
use ir::Instance;

pub mod seeds;

/// Self-authored proto3 schema covering every scalar wire form, packed and
/// unpacked repetition, oneofs, maps, nested and recursive messages, and the
/// largest legal field number.
pub const PROTO3_SCHEMA: &str = r#"
syntax = "proto3";
package fuzz.wire;

enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  GREEN = 2;
  NEGATIVE = -1;
}

message Scalars {
  int32 i32 = 1;
  int64 i64 = 2;
  uint32 u32 = 3;
  uint64 u64 = 4;
  sint32 s32 = 5;
  sint64 s64 = 6;
  bool flag = 7;
  Color color = 8;
  fixed32 f32 = 9;
  fixed64 f64 = 10;
  sfixed32 sf32 = 11;
  sfixed64 sf64 = 12;
  float real32 = 13;
  double real64 = 14;
  string text = 15;
  bytes blob = 16;
  optional int32 present = 17;
}

message Packed {
  repeated int32 i32 = 1;
  repeated sint64 s64 = 2;
  repeated fixed32 f32 = 3;
  repeated double real64 = 4;
  repeated bool flags = 5;
  repeated Color colors = 6;
  repeated uint64 unpacked = 7 [packed = false];
  repeated string texts = 8;
}

message Leaf {
  string label = 1;
  int32 weight = 2;
}

message Node {
  Node child = 1;
  int32 value = 2;
  repeated Node children = 3;
}

message Root {
  Scalars scalars = 1;
  Packed packed = 2;
  repeated Leaf leaves = 3;
  oneof choice {
    string name = 4;
    int64 code = 5;
    Leaf leaf = 6;
  }
  map<string, int32> counts = 7;
  map<int64, Leaf> by_id = 8;
  map<bool, Color> toggles = 9;
  Node tree = 10;
  uint32 last = 536870911;
}
"#;

/// Self-authored proto2 schema covering required presence, explicit defaults,
/// explicit packing, a proto2 oneof and map, and singular message merging.
pub const PROTO2_SCHEMA: &str = r#"
syntax = "proto2";
package fuzz.legacy;

enum Kind {
  HOME = 1;
  WORK = 2;
}

message Legacy {
  required string id = 1;
  optional Kind kind = 2 [default = WORK];
  optional sint32 offset = 3 [default = -7];
  repeated int32 packed = 4 [packed = true];
  repeated int32 unpacked = 5;
  optional Legacy next = 6;
  oneof value {
    string text = 7;
    double number = 8;
  }
  map<string, Kind> kinds = 9;
  optional bytes raw = 10 [default = "raw"];
}
"#;

/// Message nesting limit enforced by the decoder and encoder (`depth > 128`
/// is rejected, so at most 129 message levels decode).
pub const MAX_MESSAGE_DEPTH: usize = 128;

/// Largest input the fuzz target examines. Larger inputs are ignored so a
/// run's per-input cost stays proportional to this bound. The README's
/// bounded smoke run passes a smaller `-max_len`.
pub const MAX_INPUT_BYTES: usize = 64 * 1024;

/// One decoding root and the layout that owns it.
pub struct Root {
    pub layout: Layout,
    pub name: &'static str,
    /// Largest number of fields declared by any message reachable from the
    /// layout, including synthesized map-entry messages.
    max_fields: usize,
}

/// Parses both schemas and returns every fuzzed root.
///
/// The schemas are constants, so failing to parse them is a harness defect.
pub fn roots() -> Vec<Root> {
    let proto3 = parse(PROTO3_SCHEMA);
    let proto2 = parse(PROTO2_SCHEMA);
    let mut roots = Vec::new();
    for name in [
        "fuzz.wire.Root",
        "fuzz.wire.Scalars",
        "fuzz.wire.Packed",
        "fuzz.wire.Node",
    ] {
        roots.push(Root::new(proto3.clone(), name));
    }
    roots.push(Root::new(proto2, "fuzz.legacy.Legacy"));
    roots
}

fn parse(source: &str) -> Layout {
    match Layout::parse(source) {
        Ok(layout) => layout,
        Err(error) => panic!("fuzz schema must parse: {error}"),
    }
}

impl Root {
    fn new(layout: Layout, name: &'static str) -> Self {
        if let Err(error) = layout.resolve_message(name) {
            panic!("fuzz root `{name}` must resolve: {error}");
        }
        let max_fields = layout
            .messages()
            .iter()
            .map(|message| message.fields().len())
            .max()
            .unwrap_or(0);
        Self {
            layout,
            name,
            max_fields,
        }
    }

    /// Upper bound on decoded instance nodes for an input of `len` bytes.
    ///
    /// Every decoded value (message occurrence, scalar or packed item) is
    /// charged against at least one distinct input byte, and every decoded
    /// group comes from the root or one charged message value. A group holds
    /// at most `max_fields` direct members, so the tree has at most
    /// `(len + 1) * (max_fields + 1)` group and member nodes plus `len`
    /// repeated items.
    pub fn node_budget(&self, len: usize) -> usize {
        (len + 1) * (self.max_fields + 2)
    }
}

/// Result of decoding one input against one root.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Decoded { nodes: usize, depth: usize },
    Rejected { path: String, message: String },
}

/// Decodes `bytes` against every root and checks the harness invariants.
///
/// Panics with a descriptive message when an invariant fails, which is how
/// libFuzzer records a finding.
pub fn check(roots: &[Root], bytes: &[u8]) -> Vec<Outcome> {
    roots.iter().map(|root| check_root(root, bytes)).collect()
}

fn check_root(root: &Root, bytes: &[u8]) -> Outcome {
    let decoded = match from_slice(&root.layout, root.name, bytes) {
        Ok(decoded) => decoded,
        Err(ProtobufError::InvalidInstance { path, message }) => {
            assert!(
                path.starts_with(root.name),
                "{}: error path `{path}` is outside the root ({message})",
                root.name
            );
            return Outcome::Rejected { path, message };
        }
        Err(error) => panic!(
            "{}: malformed wire input produced a non-instance error: {error:?}",
            root.name
        ),
    };

    let (nodes, depth) = measure(&decoded);
    let budget = root.node_budget(bytes.len());
    assert!(
        nodes <= budget,
        "{}: {nodes} decoded nodes exceed the budget of {budget} for {} input bytes",
        root.name,
        bytes.len()
    );
    // Each message level adds a group and, for repeated fields, a sequence.
    let depth_budget = 2 * (MAX_MESSAGE_DEPTH + 1);
    assert!(
        depth <= depth_budget,
        "{}: decoded depth {depth} exceeds {depth_budget}",
        root.name
    );

    let encoded = match to_vec(&root.layout, root.name, &decoded) {
        Ok(encoded) => encoded,
        Err(error) => panic!(
            "{}: decoded instance does not re-encode: {error}\n{decoded:?}",
            root.name
        ),
    };
    match from_slice(&root.layout, root.name, &encoded) {
        Ok(again) => assert_eq!(
            again, decoded,
            "{}: re-encoded instance decodes differently",
            root.name
        ),
        Err(error) => panic!(
            "{}: re-encoded instance does not decode: {error}\n{decoded:?}",
            root.name
        ),
    }
    Outcome::Decoded { nodes, depth }
}

/// Counts instance nodes and returns the deepest node level (root is zero)
/// without recursing, so a pathological tree cannot overflow the stack here.
fn measure(instance: &Instance) -> (usize, usize) {
    let mut nodes = 0;
    let mut deepest = 0;
    let mut pending = vec![(instance, 0usize)];
    while let Some((node, depth)) = pending.pop() {
        nodes += 1;
        deepest = deepest.max(depth);
        match node {
            Instance::Group(fields) => {
                pending.extend(fields.iter().map(|(_, value)| (value, depth + 1)));
            }
            Instance::Repeated(items) | Instance::MappedSequence(items) => {
                pending.extend(items.iter().map(|item| (item, depth + 1)));
            }
            Instance::Scalar(_) | Instance::DocumentSet(_) => {}
        }
    }
    (nodes, deepest)
}

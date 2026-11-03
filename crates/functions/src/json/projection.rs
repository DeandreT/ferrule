//! Parsed documents belong to an execution, never to a process-wide cache.

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use ir::{Instance, SchemaNode, Value};

use crate::FunctionError;

/// Source-expression identity in the main mapping graph. The state belongs to
/// one invocation of that graph, using the graph-function validation profile.
/// Input and schema content are checked as well, never just pointer identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct JsonProjectionOwner {
    pub source: u32,
}

/// Invocation-owned JSON array projection documents. Each graph owner retains
/// only its most recent input/schema pair, not a history of record inputs.
/// Drop this state when the invocation ends. It contains private input data.
#[derive(Default)]
pub struct JsonArrayProjections {
    documents: BTreeMap<JsonProjectionOwner, Document>,
}

struct Document {
    input: String,
    descriptor: String,
    schema: SchemaNode,
    instance: Instance,
}

impl JsonArrayProjections {
    /// Calls the existing projection helpers without changing argument/error
    /// contracts. Root fields and arrays use the same complete-document
    /// validation profile, but keep their own path/type checks and errors.
    pub fn call(
        &mut self,
        owner: JsonProjectionOwner,
        function: &str,
        args: &[Value],
    ) -> Result<Value, FunctionError> {
        let (function, arity) = match function {
            "json_parse_field" => return super::parse_field_in(args, self, owner),
            "json_parse_object_exists" => return super::object_exists_in(args, self, owner),
            "json_parse_array_length" => ("json_parse_array_length", 3),
            "json_parse_array_field" => ("json_parse_array_field", 5),
            _ => return crate::call(function, args),
        };
        if args.len() != arity {
            return Err(FunctionError::ArityMismatch {
                function,
                expected: arity,
                got: args.len(),
            });
        }
        let mut text = Vec::with_capacity(3);
        for value in &args[..3] {
            let Value::String(value) = value else {
                return Err(FunctionError::TypeMismatch {
                    function,
                    got: value.type_name(),
                });
            };
            text.push(value);
        }
        // Preserve descriptor-before-document error ordering even for an
        // invalid array descriptor. No field can bypass whole-document checks.
        let schema = super::parse_schema(text[1], function)?;
        let path: Vec<String> =
            serde_json::from_str(text[2]).map_err(|_| FunctionError::InvalidArgument {
                function,
                message: "array path descriptor is invalid",
            })?;
        let document = self.document(owner, text[0], text[1], schema, function)?;
        let array = super::select_array(&document.instance, &document.schema, &path, function)?;
        if arity == 3 {
            i64::try_from(array.items.len())
                .map(Value::Int)
                .map_err(|_| FunctionError::InvalidArgument {
                    function,
                    message: "array length exceeds the supported range",
                })
        } else {
            super::select_array_field(array, args, function)
        }
    }

    fn document(
        &mut self,
        owner: JsonProjectionOwner,
        input: &str,
        descriptor: &str,
        schema: SchemaNode,
        function: &'static str,
    ) -> Result<&Document, FunctionError> {
        use std::collections::btree_map::Entry;
        let entry = self.documents.entry(owner);
        let entry = match entry {
            Entry::Occupied(previous)
                if previous.get().input == input && previous.get().descriptor == descriptor =>
            {
                return Ok(previous.into_mut());
            }
            entry => entry,
        };
        let instance = parse_document(input, &schema, function)?;
        let document = Document {
            input: input.into(),
            descriptor: descriptor.into(),
            schema,
            instance,
        };
        Ok(match entry {
            Entry::Occupied(mut previous) => {
                previous.insert(document);
                previous.into_mut()
            }
            Entry::Vacant(vacant) => vacant.insert(document),
        })
    }

    pub(super) fn input(
        &mut self,
        owner: JsonProjectionOwner,
        input: &str,
        descriptor: &str,
        schema: SchemaNode,
        function: &'static str,
    ) -> Result<&Instance, FunctionError> {
        self.document(owner, input, descriptor, schema, function)
            .map(|document| &document.instance)
    }
}

fn parse_document(
    input: &str,
    schema: &SchemaNode,
    function: &'static str,
) -> Result<Instance, FunctionError> {
    format_json::from_str(input, schema).map_err(|_| FunctionError::InvalidArgument {
        function,
        message: "input does not match the JSON schema",
    })
}

/// Test instrumentation: actual input-document DOM decode attempts on this
/// thread, excluding schema decoding and raw keyword-validation prepasses.
#[cfg(feature = "json-projection-instrumentation")]
pub fn json_projection_full_parse_count() -> usize {
    format_json::document_dom_decode_attempt_count()
}

/// Resets only the current thread's opt-in test counter; retains no input data.
#[cfg(feature = "json-projection-instrumentation")]
pub fn reset_json_projection_full_parse_count() {
    format_json::reset_document_dom_decode_attempt_count();
}

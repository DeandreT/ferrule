use ir::{DocumentMember, Instance, Value};

use crate::RuntimeError;

/// Pairs one generated target value with its portable output path.
///
/// Resolved filesystem locations are intentionally absent: generated mappings
/// return portable document members and leave confinement and publication to
/// their host boundary.
pub fn dynamic_document(
    node: u32,
    path: Value,
    value: Instance,
) -> Result<DocumentMember, RuntimeError> {
    let Value::String(path) = path else {
        return Err(RuntimeError::DynamicTargetPath {
            node,
            found: path.type_name(),
        });
    };
    if path.trim().is_empty() {
        return Err(RuntimeError::EmptyDynamicTargetPath { node });
    }
    DocumentMember::new(path, value).ok_or(RuntimeError::EmptyDynamicTargetPath { node })
}

#[cfg(test)]
mod tests {
    use ir::{Instance, Value};

    use super::dynamic_document;
    use crate::RuntimeError;

    #[test]
    fn creates_portable_members_without_a_resolved_source_location() {
        let member = dynamic_document(
            7,
            Value::String("nested/result.xml".into()),
            Instance::Group(Vec::new()),
        )
        .expect("a non-empty string path is valid");

        assert_eq!(member.path(), "nested/result.xml");
        assert_eq!(member.source_path(), member.path());
    }

    #[test]
    fn rejects_non_string_and_empty_paths_with_typed_errors() {
        assert_eq!(
            dynamic_document(8, Value::Int(3), Instance::Group(Vec::new())),
            Err(RuntimeError::DynamicTargetPath {
                node: 8,
                found: "int",
            })
        );
        assert_eq!(
            dynamic_document(9, Value::String(" \t".into()), Instance::Group(Vec::new())),
            Err(RuntimeError::EmptyDynamicTargetPath { node: 9 })
        );
    }

    #[test]
    fn rejects_nested_document_sets_like_the_interpreter() {
        assert_eq!(
            dynamic_document(
                10,
                Value::String("outer.xml".into()),
                Instance::DocumentSet(Vec::new()),
            ),
            Err(RuntimeError::EmptyDynamicTargetPath { node: 10 })
        );
    }
}

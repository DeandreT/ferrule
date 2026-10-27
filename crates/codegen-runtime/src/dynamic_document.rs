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

/// Flattens completed document sets without changing their member order.
pub fn collect_documents(
    collections: impl IntoIterator<Item = Instance>,
) -> Result<Instance, RuntimeError> {
    let mut documents = Vec::new();
    for collection in collections {
        match collection {
            Instance::DocumentSet(items) => documents.extend(items),
            Instance::Scalar(_) => {
                return Err(RuntimeError::InvalidDocumentCollectionItem { found: "a scalar" });
            }
            Instance::Group(_) => {
                return Err(RuntimeError::InvalidDocumentCollectionItem { found: "a group" });
            }
            Instance::Repeated(_) => {
                return Err(RuntimeError::InvalidDocumentCollectionItem {
                    found: "a repeated value",
                });
            }
            Instance::MappedSequence(_) => {
                return Err(RuntimeError::InvalidDocumentCollectionItem {
                    found: "a mapped sequence",
                });
            }
        }
    }
    Ok(Instance::DocumentSet(documents))
}

#[cfg(test)]
mod tests {
    use ir::{Instance, Value};

    use super::{collect_documents, dynamic_document};
    use crate::RuntimeError;

    #[test]
    fn creates_portable_members_without_a_resolved_source_location() {
        let member = dynamic_document(
            7,
            Value::String("nested/result.xml".into()),
            Instance::Group((Vec::new()).into()),
        )
        .expect("a non-empty string path is valid");

        assert_eq!(member.path(), "nested/result.xml");
        assert_eq!(member.source_path(), member.path());
    }

    #[test]
    fn rejects_non_string_and_empty_paths_with_typed_errors() {
        assert_eq!(
            dynamic_document(8, Value::Int(3), Instance::Group((Vec::new()).into())),
            Err(RuntimeError::DynamicTargetPath {
                node: 8,
                found: "int",
            })
        );
        assert_eq!(
            dynamic_document(
                9,
                Value::String(" \t".into()),
                Instance::Group((Vec::new()).into())
            ),
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

    #[test]
    fn collects_empty_and_nonempty_sets_in_order() {
        let first = dynamic_document(
            1,
            Value::String("first.xml".into()),
            Instance::Scalar(Value::Int(1)),
        )
        .unwrap();
        let last = dynamic_document(
            1,
            Value::String("last.xml".into()),
            Instance::Scalar(Value::Int(2)),
        )
        .unwrap();
        assert_eq!(
            collect_documents([
                Instance::DocumentSet(vec![first.clone()]),
                Instance::DocumentSet(Vec::new()),
                Instance::DocumentSet(vec![last.clone()]),
            ])
            .unwrap(),
            Instance::DocumentSet(vec![first, last])
        );
        assert_eq!(
            collect_documents([]).unwrap(),
            Instance::DocumentSet(Vec::new())
        );
    }

    #[test]
    fn rejects_ordinary_values_in_document_collections() {
        assert_eq!(
            collect_documents([Instance::Scalar(Value::Int(3))]),
            Err(RuntimeError::InvalidDocumentCollectionItem { found: "a scalar" })
        );
        assert_eq!(
            collect_documents([Instance::Group(Vec::new().into())]),
            Err(RuntimeError::InvalidDocumentCollectionItem { found: "a group" })
        );
    }
}

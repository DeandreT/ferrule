use ir::Instance;
use mapping::JoinId;

#[derive(Clone)]
pub(super) struct PositionFrame {
    pub(super) collection: Vec<String>,
    pub(super) index: usize,
    /// The matching context instance has a synthetic named collection
    /// wrapper immediately before it.
    pub(super) grouped: bool,
    /// Join owning this raw source frame, if any.
    pub(super) join: Option<JoinId>,
    /// Flattened tuple position stored without adding a synthetic context
    /// frame. The raw source `index` remains independently addressable.
    pub(super) join_position: Option<(JoinId, usize)>,
    /// Local path retained by a document-set boundary for this source frame.
    pub(super) document_path: Option<String>,
}

pub(super) struct WalkExtension<'a> {
    pub(super) instances: Vec<&'a Instance>,
    pub(super) positions: Vec<PositionFrame>,
}

/// Walks `path` from `base`, branching (and pushing one context frame) each
/// time it crosses a repeating element -- whether mid-path or, if `path` is
/// exhausted and the final value is itself repeating (e.g. `path` is empty
/// and `base` is a CSV file's rows), at the very end. Returns one extension
/// (the new frames to push, innermost last) per produced item. Repeating
/// frames also retain their collection path and 1-based source position.
pub(super) fn walk<'a>(
    base: &'a Instance,
    path: &[String],
    prefix: &[String],
    acc: &[&'a Instance],
    positions: &[PositionFrame],
) -> Vec<WalkExtension<'a>> {
    match path.split_first() {
        None => match base {
            Instance::DocumentSet(documents) => documents
                .iter()
                .enumerate()
                .map(|(index, document)| {
                    let mut next_instances = acc.to_vec();
                    next_instances.push(document.value());
                    let mut next_positions = positions.to_vec();
                    next_positions.push(PositionFrame {
                        collection: prefix.to_vec(),
                        index: index + 1,
                        grouped: false,
                        join: None,
                        join_position: None,
                        document_path: Some(document.path().to_string()),
                    });
                    WalkExtension {
                        instances: next_instances,
                        positions: next_positions,
                    }
                })
                .collect(),
            Instance::Repeated(items) => items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let mut next_instances = acc.to_vec();
                    next_instances.push(item);
                    let mut next_positions = positions.to_vec();
                    next_positions.push(PositionFrame {
                        collection: prefix.to_vec(),
                        index: index + 1,
                        grouped: false,
                        join: None,
                        join_position: None,
                        document_path: None,
                    });
                    WalkExtension {
                        instances: next_instances,
                        positions: next_positions,
                    }
                })
                .collect(),
            _ => {
                let mut next_instances = acc.to_vec();
                next_instances.push(base);
                vec![WalkExtension {
                    instances: next_instances,
                    positions: positions.to_vec(),
                }]
            }
        },
        Some((segment, rest)) => {
            if let Instance::DocumentSet(documents) = base {
                return documents
                    .iter()
                    .enumerate()
                    .flat_map(|(index, document)| {
                        let mut next_instances = acc.to_vec();
                        next_instances.push(document.value());
                        let mut next_positions = positions.to_vec();
                        next_positions.push(PositionFrame {
                            collection: prefix.to_vec(),
                            index: index + 1,
                            grouped: false,
                            join: None,
                            join_position: None,
                            document_path: Some(document.path().to_string()),
                        });
                        walk(
                            document.value(),
                            path,
                            prefix,
                            &next_instances,
                            &next_positions,
                        )
                    })
                    .collect();
            }
            let mut collection_path = prefix.to_vec();
            collection_path.push(segment.clone());
            match base.field(segment) {
                None => Vec::new(),
                Some(Instance::Repeated(items)) => items
                    .iter()
                    .enumerate()
                    .flat_map(|(index, item)| {
                        let mut next_instances = acc.to_vec();
                        next_instances.push(item);
                        let mut next_positions = positions.to_vec();
                        next_positions.push(PositionFrame {
                            collection: collection_path.clone(),
                            index: index + 1,
                            grouped: false,
                            join: None,
                            join_position: None,
                            document_path: None,
                        });
                        if rest.is_empty() {
                            vec![WalkExtension {
                                instances: next_instances,
                                positions: next_positions,
                            }]
                        } else {
                            walk(
                                item,
                                rest,
                                &collection_path,
                                &next_instances,
                                &next_positions,
                            )
                        }
                    })
                    .collect(),
                Some(other) => walk(other, rest, &collection_path, acc, positions),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ir::{DocumentMember, Instance, Value};

    use super::walk;

    #[test]
    fn walks_descendants_across_a_document_set_and_retains_paths() {
        let document = |path: &str, value: &str| {
            DocumentMember::new(
                path,
                Instance::Group(vec![(
                    "items".into(),
                    Instance::Repeated(vec![Instance::Scalar(Value::String(value.into()))]),
                )]),
            )
        };
        let Some(first) = document("a.xml", "a") else {
            panic!("valid first document member")
        };
        let Some(second) = document("b.xml", "b") else {
            panic!("valid second document member")
        };
        let source = Instance::DocumentSet(vec![first, second]);

        let walked = walk(&source, &["items".into()], &[], &[], &[]);

        assert_eq!(walked.len(), 2);
        assert_eq!(walked[0].instances.len(), 2);
        assert_eq!(walked[1].instances.len(), 2);
        assert_eq!(walked[0].positions[0].collection, Vec::<String>::new());
        assert_eq!(walked[0].positions[0].index, 1);
        assert_eq!(walked[1].positions[0].index, 2);
        assert_eq!(
            walked[0].positions[0].document_path.as_deref(),
            Some("a.xml")
        );
        assert_eq!(
            walked[1].positions[0].document_path.as_deref(),
            Some("b.xml")
        );
        assert_eq!(walked[0].positions[1].collection, ["items"]);
        assert_eq!(walked[1].positions[1].index, 1);
    }
}

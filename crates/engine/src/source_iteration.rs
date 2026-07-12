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

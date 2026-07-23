use crate::{Instance, RuntimeError, Value};

use super::{CollectionIdentity, NamedInput, ScopeContext, ScopeFrame, collection_frame};

/// Owns grouped member collections while their borrowed scope contexts run.
///
/// Grouping clones source members because the synthetic repeated collection
/// and its optional named wrapper must outlive every generated group context.
pub struct GroupedItems<'a> {
    groups: Vec<OwnedGroup<'a>>,
}

struct OwnedGroup<'a> {
    prefix: Vec<ScopeFrame<'a>>,
    named_inputs: &'a [NamedInput<'a>],
    execution: Option<crate::ExecutionContext<'a>>,
    wrapper: Option<Instance>,
    members: Instance,
    collection: Vec<String>,
    document_path: Option<&'a str>,
}

struct GroupBucket<'a, K> {
    key: K,
    first: ScopeContext<'a>,
    members: Vec<Instance>,
}

impl<'a> GroupedItems<'a> {
    /// Partitions candidates by exact tagged scalar equality in first-seen
    /// key order.
    pub fn by(candidates: Vec<(ScopeContext<'a>, Value)>, wrapper_name: Option<&str>) -> Self {
        let mut groups: Vec<GroupBucket<'a, Value>> = Vec::new();
        for (candidate, key) in candidates {
            let Some(member) = candidate.current_instance().cloned() else {
                continue;
            };
            if let Some(group) = groups.iter_mut().find(|group| group.key == key) {
                group.members.push(member);
            } else {
                groups.push(GroupBucket {
                    key,
                    first: candidate,
                    members: vec![member],
                });
            }
        }
        Self::from_buckets(groups, wrapper_name)
    }

    /// Partitions consecutive candidates with the same exact tagged scalar
    /// key. A later occurrence of a previous key starts a new group.
    pub fn adjacent_by(
        candidates: Vec<(ScopeContext<'a>, Value)>,
        wrapper_name: Option<&str>,
    ) -> Self {
        let mut groups: Vec<GroupBucket<'a, Value>> = Vec::new();
        for (candidate, key) in candidates {
            let Some(member) = candidate.current_instance().cloned() else {
                continue;
            };
            if let Some(group) = groups.last_mut().filter(|group| group.key == key) {
                group.members.push(member);
            } else {
                groups.push(GroupBucket {
                    key,
                    first: candidate,
                    members: vec![member],
                });
            }
        }
        Self::from_buckets(groups, wrapper_name)
    }

    /// Starts a new contiguous group whenever the candidate's predicate is
    /// true. A leading false candidate still creates the first group.
    pub fn starting_with(
        candidates: Vec<(ScopeContext<'a>, bool)>,
        wrapper_name: Option<&str>,
    ) -> Self {
        let mut groups: Vec<GroupBucket<'a, ()>> = Vec::new();
        for (candidate, starts_group) in candidates {
            let Some(member) = candidate.current_instance().cloned() else {
                continue;
            };
            if !starts_group && let Some(group) = groups.last_mut() {
                group.members.push(member);
            } else {
                groups.push(GroupBucket {
                    key: (),
                    first: candidate,
                    members: vec![member],
                });
            }
        }
        Self::from_buckets(groups, wrapper_name)
    }

    /// Ends the current contiguous group after every candidate whose
    /// predicate is true. A trailing false candidate remains in the final
    /// group.
    pub fn ending_with(
        candidates: Vec<(ScopeContext<'a>, bool)>,
        wrapper_name: Option<&str>,
    ) -> Self {
        let mut groups: Vec<GroupBucket<'a, ()>> = Vec::new();
        let mut previous_ended_group = true;
        for (candidate, ends_group) in candidates {
            let Some(member) = candidate.current_instance().cloned() else {
                continue;
            };
            if previous_ended_group {
                groups.push(GroupBucket {
                    key: (),
                    first: candidate,
                    members: vec![member],
                });
            } else if let Some(group) = groups.last_mut() {
                group.members.push(member);
            }
            previous_ended_group = ends_group;
        }
        Self::from_buckets(groups, wrapper_name)
    }

    /// Chunks candidates into fixed positive-size groups.
    pub fn into_blocks(
        candidates: Vec<ScopeContext<'a>>,
        wrapper_name: Option<&str>,
        size: usize,
        node: u32,
    ) -> Result<Self, RuntimeError> {
        if size == 0 {
            return Err(RuntimeError::InvalidBlockSize { node });
        }

        let mut groups: Vec<GroupBucket<'a, ()>> = Vec::new();
        for candidate in candidates {
            let Some(member) = candidate.current_instance().cloned() else {
                continue;
            };
            if let Some(group) = groups.last_mut().filter(|group| group.members.len() < size) {
                group.members.push(member);
            } else {
                groups.push(GroupBucket {
                    key: (),
                    first: candidate,
                    members: vec![member],
                });
            }
        }
        Ok(Self::from_buckets(groups, wrapper_name))
    }

    /// Reborrows each owned group as one candidate context. The grouped
    /// collection's position is one-based and independent of member positions.
    pub fn contexts<'b>(&'b self) -> Vec<ScopeContext<'b>>
    where
        'a: 'b,
    {
        self.groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                let mut frames = group
                    .prefix
                    .iter()
                    .map(|frame| ScopeFrame {
                        instance: frame.instance,
                        collection: frame.collection.clone(),
                        document_path: frame.document_path,
                        join: frame.join,
                        join_position: frame.join_position,
                    })
                    .collect::<Vec<_>>();
                if let Some(wrapper) = &group.wrapper {
                    frames.push(ScopeFrame {
                        instance: wrapper,
                        collection: None,
                        document_path: None,
                        join: None,
                        join_position: None,
                    });
                }
                let mut grouped_frame = collection_frame(
                    &group.members,
                    CollectionIdentity::Grouped {
                        path: group.collection.clone(),
                        index: index + 1,
                    },
                );
                grouped_frame.document_path = group.document_path;
                frames.push(grouped_frame);
                ScopeContext {
                    frames,
                    named_inputs: group.named_inputs,
                    execution: group.execution,
                }
            })
            .collect()
    }

    fn from_buckets<K>(groups: Vec<GroupBucket<'a, K>>, wrapper_name: Option<&str>) -> Self {
        Self {
            groups: groups
                .into_iter()
                .filter_map(|group| {
                    let terminal = group.first.frames.last()?;
                    let collection = terminal
                        .collection
                        .as_ref()
                        .map(|identity| identity.path().to_vec())
                        .unwrap_or_default();
                    let members = Instance::Repeated(group.members);
                    let wrapper = wrapper_name
                        .map(|name| Instance::Group(vec![(name.to_string(), members.clone())]));
                    let prefix_len = group.first.frames.len().saturating_sub(1);
                    Some(OwnedGroup {
                        prefix: group.first.frames[..prefix_len].to_vec(),
                        named_inputs: group.first.named_inputs,
                        execution: group.first.execution,
                        wrapper,
                        members,
                        collection,
                        document_path: terminal.document_path,
                    })
                })
                .collect(),
        }
    }
}

impl ScopeContext<'_> {
    fn current_instance(&self) -> Option<&Instance> {
        self.frames.last().map(|frame| frame.instance)
    }
}

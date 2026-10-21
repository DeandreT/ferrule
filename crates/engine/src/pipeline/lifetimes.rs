//! Ownership and last-consumer analysis for values passed between stages.
//!
//! A validated plan fixes one sequential execution order, so every value a
//! stage can read has a statically known final reader. [`ValueLifetimes`] is
//! computed once from that order; a [`ReleaseSchedule`] then combines it with
//! one retention policy to say, for every step, which named-source bindings may
//! take ownership of their value and which values are dead after the stage.

use std::collections::{BTreeMap, BTreeSet};

use mapping::{Pipeline, PipelineInput};

/// One value a stage can read: a host input, or a primary/named stage target.
/// Stages are identified by their declaration index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PipelineValue<'a> {
    Host(&'a str),
    Stage {
        stage: usize,
        target: Option<&'a str>,
    },
}

impl<'a> PipelineValue<'a> {
    fn of(input: &'a PipelineInput, ids: &BTreeMap<&str, usize>) -> Self {
        match input {
            PipelineInput::Host { name } => Self::Host(name),
            PipelineInput::StageTarget { stage, target } => Self::Stage {
                stage: ids[stage.as_str()],
                target: target.as_deref(),
            },
        }
    }
}

/// The values one stage reads, in binding order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StageReads<'a> {
    pub(crate) source: PipelineValue<'a>,
    /// Static named-source bindings as `(source name, value)`.
    pub(crate) extras: Vec<(&'a str, PipelineValue<'a>)>,
}

/// Every stage read and the final execution step that reads each value.
#[derive(Debug, Clone)]
pub(crate) struct ValueLifetimes<'a> {
    /// Reads indexed by execution step, not declaration index.
    reads: Vec<StageReads<'a>>,
    /// Values produced by each execution step: the primary target, then each
    /// named target in declaration order.
    produced: Vec<Vec<PipelineValue<'a>>>,
    last_read: BTreeMap<PipelineValue<'a>, usize>,
}

/// Which values must survive the run regardless of later readers.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Retention<'r, 'a> {
    /// Keep every host and stage value; nothing is moved or released.
    All,
    /// Keep only these stage values. Host inputs are owned by the run and
    /// released after their final reader like any intermediate.
    Selected(&'r BTreeSet<PipelineValue<'a>>),
}

impl Retention<'_, '_> {
    fn retains(&self, value: &PipelineValue<'_>) -> bool {
        match self {
            Self::All => true,
            Self::Selected(selected) => selected.contains(value),
        }
    }
}

/// How one named-source binding obtains its owned instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transfer {
    Clone,
    /// This binding is the value's final read and nothing retains it.
    Move,
}

/// Instructions for one execution step under one retention policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StepRelease<'a> {
    /// One entry per named-source binding, parallel to [`StageReads::extras`].
    pub(crate) transfers: Vec<Transfer>,
    /// Values to drop after the stage succeeds: inputs read for the last time
    /// and not moved, plus produced values that no later stage reads.
    pub(crate) release: Vec<PipelineValue<'a>>,
}

/// Per-step transfers and releases, indexed by execution step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReleaseSchedule<'a> {
    pub(crate) steps: Vec<StepRelease<'a>>,
}

impl<'a> ValueLifetimes<'a> {
    /// Analyzes a pipeline that already passed planning, in its execution order.
    pub(crate) fn analyze(pipeline: &'a Pipeline, order: &[usize]) -> Self {
        let ids: BTreeMap<_, _> = pipeline
            .stages
            .iter()
            .enumerate()
            .map(|(index, stage)| (stage.id.as_str(), index))
            .collect();
        let mut reads = Vec::with_capacity(order.len());
        let mut produced = Vec::with_capacity(order.len());
        let mut last_read = BTreeMap::new();
        for (step, &index) in order.iter().enumerate() {
            let stage = &pipeline.stages[index];
            let stage_reads = StageReads {
                source: PipelineValue::of(&stage.source, &ids),
                extras: stage
                    .extra_sources
                    .iter()
                    .map(|binding| {
                        (
                            binding.name.as_str(),
                            PipelineValue::of(&binding.from, &ids),
                        )
                    })
                    .collect(),
            };
            last_read.insert(stage_reads.source, step);
            for (_, value) in &stage_reads.extras {
                last_read.insert(*value, step);
            }
            reads.push(stage_reads);
            produced.push(
                std::iter::once(None)
                    .chain(
                        stage
                            .project
                            .extra_targets
                            .iter()
                            .map(|target| Some(target.name.as_str())),
                    )
                    .map(|target| PipelineValue::Stage {
                        stage: index,
                        target,
                    })
                    .collect(),
            );
        }
        Self {
            reads,
            produced,
            last_read,
        }
    }

    pub(crate) fn reads(&self, step: usize) -> &StageReads<'a> {
        &self.reads[step]
    }

    pub(crate) fn produced(&self, step: usize) -> &[PipelineValue<'a>] {
        &self.produced[step]
    }

    /// The final execution step that reads `value`, or `None` when no stage does.
    pub(crate) fn last_read(&self, value: &PipelineValue<'_>) -> Option<usize> {
        self.last_read.get(value).copied()
    }

    pub(crate) fn schedule(&self, retention: Retention<'_, 'a>) -> ReleaseSchedule<'a> {
        let steps = self
            .reads
            .iter()
            .enumerate()
            .map(|(step, reads)| {
                let dies_here = |value: &PipelineValue<'_>| {
                    !retention.retains(value) && self.last_read(value) == Some(step)
                };
                // The primary source is borrowed for the whole stage, and an
                // earlier binding of the same value still needs its own copy,
                // so only the final named binding of a dying value may move it.
                let transfers = reads
                    .extras
                    .iter()
                    .enumerate()
                    .map(|(position, (_, value))| {
                        let final_binding = reads.source != *value
                            && reads.extras[position + 1..]
                                .iter()
                                .all(|(_, later)| later != value);
                        if final_binding && dies_here(value) {
                            Transfer::Move
                        } else {
                            Transfer::Clone
                        }
                    })
                    .collect::<Vec<_>>();
                let moved: BTreeSet<_> = reads
                    .extras
                    .iter()
                    .zip(&transfers)
                    .filter(|(_, transfer)| **transfer == Transfer::Move)
                    .map(|((_, value), _)| *value)
                    .collect();
                let mut release = BTreeSet::new();
                for value in std::iter::once(&reads.source)
                    .chain(reads.extras.iter().map(|(_, value)| value))
                {
                    if dies_here(value) && !moved.contains(value) {
                        release.insert(*value);
                    }
                }
                for value in self.produced(step) {
                    if !retention.retains(value) && self.last_read(value).is_none() {
                        release.insert(*value);
                    }
                }
                StepRelease {
                    transfers,
                    release: release.into_iter().collect(),
                }
            })
            .collect();
        ReleaseSchedule { steps }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{ScalarType, SchemaNode};
    use mapping::{NamedTarget, PipelineNamedInput, PipelineStage, Scope, ScopeConstruction};

    fn project(named_targets: &[&str]) -> mapping::Project {
        let schema = SchemaNode::group(
            "Record",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        );
        mapping::Project {
            source: schema.clone(),
            target: schema.clone(),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            extra_targets: named_targets
                .iter()
                .map(|name| NamedTarget {
                    name: (*name).into(),
                    path: None,
                    schema: schema.clone(),
                    options: Default::default(),
                    root: Scope::default(),
                })
                .collect(),
            failure_rules: Vec::new(),
            user_functions: Default::default(),
            graph: Default::default(),
            root: Scope {
                construction: ScopeConstruction::CopyCurrentSource,
                ..Scope::default()
            },
        }
    }

    fn host(name: &str) -> PipelineInput {
        PipelineInput::Host { name: name.into() }
    }

    fn output(stage: &str, target: Option<&str>) -> PipelineInput {
        PipelineInput::StageTarget {
            stage: stage.into(),
            target: target.map(str::to_string),
        }
    }

    fn stage(id: &str, source: PipelineInput, extras: &[(&str, PipelineInput)]) -> PipelineStage {
        PipelineStage {
            id: id.into(),
            mapping_path: None,
            project: project(&[]),
            source,
            extra_sources: extras
                .iter()
                .map(|(name, from)| PipelineNamedInput {
                    name: (*name).into(),
                    from: from.clone(),
                })
                .collect(),
        }
    }

    fn value(stage: usize, target: Option<&str>) -> PipelineValue<'_> {
        PipelineValue::Stage { stage, target }
    }

    fn pipeline(stages: Vec<PipelineStage>) -> Pipeline {
        Pipeline {
            main_mapping_path: None,
            stages,
        }
    }

    #[test]
    fn last_reads_follow_execution_order_rather_than_declaration_order() {
        // Declared consumer-first; executed a, b, c.
        let pipeline = pipeline(vec![
            stage("c", output("a", None), &[]),
            stage("b", output("a", None), &[]),
            stage("a", host("in"), &[]),
        ]);
        let lifetimes = ValueLifetimes::analyze(&pipeline, &[2, 1, 0]);
        assert_eq!(lifetimes.last_read(&PipelineValue::Host("in")), Some(0));
        assert_eq!(lifetimes.last_read(&value(2, None)), Some(2));
        assert_eq!(lifetimes.last_read(&value(1, None)), None);
        assert_eq!(lifetimes.last_read(&value(0, None)), None);
        assert_eq!(lifetimes.produced(0), &[value(2, None)]);
        assert_eq!(lifetimes.reads(2).source, value(2, None));
    }

    #[test]
    fn fan_out_releases_only_after_the_final_reader() {
        let pipeline = pipeline(vec![
            stage("a", host("in"), &[]),
            stage("b", output("a", None), &[]),
            stage("c", output("a", None), &[]),
        ]);
        let lifetimes = ValueLifetimes::analyze(&pipeline, &[0, 1, 2]);
        let retained = BTreeSet::from([value(1, None), value(2, None)]);
        let schedule = lifetimes.schedule(Retention::Selected(&retained));
        assert_eq!(schedule.steps[0].release, vec![PipelineValue::Host("in")]);
        assert!(schedule.steps[1].release.is_empty());
        assert_eq!(schedule.steps[2].release, vec![value(0, None)]);
    }

    #[test]
    fn only_the_final_named_binding_of_a_dying_value_moves() {
        let pipeline = pipeline(vec![
            stage("a", host("in"), &[]),
            stage(
                "b",
                host("in"),
                &[("first", output("a", None)), ("second", output("a", None))],
            ),
            stage("c", output("b", None), &[("same", output("b", None))]),
        ]);
        let lifetimes = ValueLifetimes::analyze(&pipeline, &[0, 1, 2]);
        let retained = BTreeSet::from([value(2, None)]);
        let schedule = lifetimes.schedule(Retention::Selected(&retained));
        // Two bindings of one value: the earlier one copies, the final one moves.
        assert_eq!(
            schedule.steps[1].transfers,
            vec![Transfer::Clone, Transfer::Move]
        );
        assert_eq!(schedule.steps[1].release, vec![PipelineValue::Host("in")]);
        // The primary source borrows the value, so the named binding copies it.
        assert_eq!(schedule.steps[2].transfers, vec![Transfer::Clone]);
        assert_eq!(schedule.steps[2].release, vec![value(1, None)]);
    }

    #[test]
    fn retained_values_are_neither_moved_nor_released() {
        let pipeline = pipeline(vec![
            stage("a", host("in"), &[]),
            stage("b", host("in"), &[("lookup", output("a", None))]),
        ]);
        let lifetimes = ValueLifetimes::analyze(&pipeline, &[0, 1]);
        let retained = BTreeSet::from([value(0, None), value(1, None)]);
        let schedule = lifetimes.schedule(Retention::Selected(&retained));
        assert_eq!(schedule.steps[1].transfers, vec![Transfer::Clone]);
        assert_eq!(schedule.steps[1].release, vec![PipelineValue::Host("in")]);

        let all = lifetimes.schedule(Retention::All);
        assert!(all.steps.iter().all(|step| step.release.is_empty()));
        assert_eq!(all.steps[1].transfers, vec![Transfer::Clone]);
    }

    #[test]
    fn unread_and_unselected_targets_are_released_when_produced() {
        let mut producer = stage("a", host("in"), &[]);
        producer.project = project(&["audit", "unused"]);
        let pipeline = pipeline(vec![producer, stage("b", output("a", Some("audit")), &[])]);
        let lifetimes = ValueLifetimes::analyze(&pipeline, &[0, 1]);
        assert_eq!(
            lifetimes.produced(0),
            &[
                value(0, None),
                value(0, Some("audit")),
                value(0, Some("unused"))
            ]
        );
        let retained = BTreeSet::from([value(1, None)]);
        let schedule = lifetimes.schedule(Retention::Selected(&retained));
        assert_eq!(
            schedule.steps[0].release,
            vec![
                PipelineValue::Host("in"),
                value(0, None),
                value(0, Some("unused"))
            ]
        );
        assert_eq!(schedule.steps[1].release, vec![value(0, Some("audit"))]);
    }
}

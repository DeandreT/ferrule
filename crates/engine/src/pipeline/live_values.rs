//! Host and stage values that later stages can still read.

use std::borrow::Cow;
use std::collections::BTreeMap;

use ir::Instance;

use super::lifetimes::PipelineValue;
use crate::{ExecutionOutputs, NamedOutput};

/// One stage's targets; a slot becomes `None` once its value is released.
pub(super) struct StageSlots {
    primary: Option<Instance>,
    extras: Vec<(String, Option<Instance>)>,
}

impl StageSlots {
    fn slot(&mut self, target: Option<&str>) -> Option<&mut Option<Instance>> {
        match target {
            None => Some(&mut self.primary),
            Some(name) => self
                .extras
                .iter_mut()
                .find(|(output, _)| output == name)
                .map(|(_, slot)| slot),
        }
    }

    pub(super) fn into_outputs(self) -> Option<ExecutionOutputs> {
        Some(ExecutionOutputs {
            primary: self.primary?,
            extras: self
                .extras
                .into_iter()
                .map(|(name, instance)| {
                    Some(NamedOutput {
                        name,
                        instance: instance?,
                    })
                })
                .collect::<Option<_>>()?,
        })
    }
}

/// Values available to later stages, with a live-value high-water mark.
pub(super) struct LiveValues<'h> {
    hosts: BTreeMap<String, Cow<'h, Instance>>,
    pub(super) stages: BTreeMap<usize, StageSlots>,
    count: usize,
    pub(super) peak: usize,
}

impl<'h> LiveValues<'h> {
    pub(super) fn new(hosts: impl IntoIterator<Item = (String, Cow<'h, Instance>)>) -> Self {
        let hosts: BTreeMap<_, _> = hosts.into_iter().collect();
        let count = hosts.len();
        Self {
            hosts,
            stages: BTreeMap::new(),
            count,
            peak: count,
        }
    }

    pub(super) fn insert(&mut self, stage: usize, outputs: ExecutionOutputs) {
        self.count += 1 + outputs.extras.len();
        self.peak = self.peak.max(self.count);
        let slots = StageSlots {
            primary: Some(outputs.primary),
            extras: outputs
                .extras
                .into_iter()
                .map(|output| (output.name, Some(output.instance)))
                .collect(),
        };
        self.stages.insert(stage, slots);
    }

    // The schedule only reads or takes a value before its final reader has
    // run, and validation resolved every reference, so a missing value is an
    // internal ownership defect rather than an input error.
    pub(super) fn get(&self, value: PipelineValue<'_>) -> &Instance {
        let found = match value {
            PipelineValue::Host(name) => self.hosts.get(name).map(Cow::as_ref),
            PipelineValue::Stage { stage, target } => {
                self.stages.get(&stage).and_then(|slots| match target {
                    None => slots.primary.as_ref(),
                    Some(name) => slots
                        .extras
                        .iter()
                        .find(|(output, _)| output == name)
                        .and_then(|(_, instance)| instance.as_ref()),
                })
            }
        };
        found.expect("pipeline value is live until its final reader")
    }

    pub(super) fn take(&mut self, value: PipelineValue<'_>) -> Instance {
        self.remove(value)
            .expect("pipeline value is live until its final reader")
    }

    pub(super) fn release(&mut self, value: PipelineValue<'_>) {
        self.remove(value);
    }

    fn remove(&mut self, value: PipelineValue<'_>) -> Option<Instance> {
        let removed = match value {
            PipelineValue::Host(name) => self.hosts.remove(name).map(Cow::into_owned),
            PipelineValue::Stage { stage, target } => self
                .stages
                .get_mut(&stage)
                .and_then(|slots| slots.slot(target))
                .and_then(Option::take),
        };
        if removed.is_some() {
            self.count -= 1;
        }
        removed
    }
}

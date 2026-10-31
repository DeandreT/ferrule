//! Keep an unsupported value distinct from an ordinary absent (Null) value.

use std::collections::BTreeSet;

use super::graph::GraphBuilder;
use super::schema::ComponentFormat;
use super::value_producer::{InputContext, SpecializedProducer};

// The suffix belongs to the selected value, not the complete producer. Control
// dependencies only inherit whole-producer refusals, never a sibling projection.
type Request = (u32, Vec<String>, bool);

impl GraphBuilder<'_> {
    fn refuse_dependency_limit(&mut self, key: u32) -> bool {
        if self.refused_value_feeds.insert(key) {
            self.warnings.push(format!("output {key} exceeds the bounded value-dependency traversal; affected binding skipped"));
        }
        true
    }
    fn value_feed_inputs(&self, key: u32, suffix: &[String], control: bool) -> Vec<Request> {
        let producer = self.specialized_value_producer(key);
        let producer_inputs = |producer: SpecializedProducer| {
            producer
                .input_pins(self)
                .into_iter()
                .filter_map(|port| self.edge_from.get(&port).copied())
                .map(|feed| (feed, Vec::new(), false))
                .collect()
        };
        if let Some(producer) = producer
            && !matches!(producer, SpecializedProducer::Udf { .. })
        {
            return producer_inputs(producer);
        }
        if self.source_abs_path(key).is_some() {
            return Vec::new();
        }
        if let Some(intermediate) = self.intermediate_feed(key) {
            let mut path = intermediate.suffix;
            path.extend_from_slice(suffix);
            let mut inputs = vec![(intermediate.feed, path, control)];
            // Whole constructed values use every descendant override. A
            // selected child only depends on overrides along its own path.
            for (projection, feed) in intermediate.ordered_projections {
                if projection.starts_with(suffix) || suffix.starts_with(&projection) {
                    let tail = suffix
                        .strip_prefix(projection.as_slice())
                        .unwrap_or_default();
                    inputs.push((feed, tail.to_vec(), control));
                }
            }
            if let Some(feed) = intermediate.control {
                inputs.push((feed, Vec::new(), true));
            }
            return inputs;
        }
        if let Some(producer) = producer {
            return producer_inputs(producer);
        }
        let sequence_control = control && self.resolve_iteration_feed(key).source_key != key;
        self.fn_by_output
            .get(&key)
            .into_iter()
            .flat_map(|index| self.fn_components[*index].inputs.iter().enumerate())
            .filter_map(|(position, port)| {
                let feed = self.edge_from.get(&(*port)?)?;
                // A structural projection follows the value input of a
                // sequence control, not its predicate or scalar arguments.
                let path = if position == 0 {
                    suffix.to_vec()
                } else {
                    Vec::new()
                };
                Some((*feed, path, sequence_control && position == 0))
            })
            .collect()
    }

    pub(super) fn value_feed_is_refused(&mut self, key: u32) -> bool {
        if self.refused_value_feeds.is_empty() && self.refused_filter_projections.is_empty() {
            return false;
        }
        let mut pending = vec![(key, Vec::new(), false)];
        let mut visited = BTreeSet::new();
        while let Some((feed, suffix, control)) = pending.pop() {
            if visited.len() >= 4096 || suffix.len() > 128 {
                return self.refuse_dependency_limit(key);
            }
            if !visited.insert((feed, suffix.clone(), control)) {
                continue;
            }
            if self.refused_value_feeds.contains(&feed)
                || !control
                    && self
                        .refused_filter_projections
                        .contains(&(feed, suffix.clone()))
            {
                return true;
            }
            pending.extend(self.value_feed_inputs(feed, &suffix, control));
        }
        false
    }

    /// These consumers currently instantiate arguments without the receiving
    /// target's item frame. Refuse a filtered variable projection rather than
    /// reusing the first matching record for every output item.
    pub(super) fn refuse_unanchored_filter_consumer(&mut self, key: u32) -> bool {
        let Some(producer) = self.specialized_value_producer(key) else {
            return false;
        };
        let mut pending = producer
            .input_contexts(self)
            .into_iter()
            .filter_map(|(pin, context)| {
                self.edge_from
                    .get(&pin)
                    .map(|feed| (*feed, Vec::new(), false, context))
            })
            .collect::<Vec<_>>();
        let mut visited = BTreeSet::new();
        while let Some((feed, suffix, control, context)) = pending.pop() {
            let (structural, consumer) = match context {
                InputContext::Unanchored(name) => (false, name),
                InputContext::Structural(name) => (true, name),
            };
            if visited.len() >= 4096 || suffix.len() > 128 {
                return self.refuse_dependency_limit(key);
            }
            if !visited.insert((feed, suffix.clone(), control, structural)) {
                continue;
            }
            if structural && self.resolve_iteration_feed(feed).has_filter {
                if self.refused_value_feeds.insert(key) {
                    self.warnings.push(format!("{consumer} output {key} cannot preserve a filtered sequence on a structural input; affected binding skipped"));
                }
                return true;
            }
            if !suffix.is_empty()
                && self.resolve_iteration_feed(feed).has_filter
                && let Some(source) = self.sequence_source_path(feed)
                && let Some(component) = self.sources.get(source.source)
            {
                let mut path = source.path;
                path.extend_from_slice(&suffix);
                let repeating = component.schema.repeating
                    || component.options.local_xml_file_set
                    || matches!(
                        component.format,
                        ComponentFormat::Csv | ComponentFormat::Xlsx
                    )
                    || (1..=path.len()).any(|depth| {
                        super::schema::schema_node_at(&component.schema, &path[..depth])
                            .is_some_and(|node| node.repeating)
                    });
                if repeating {
                    if self.refused_value_feeds.insert(key) {
                        self.warnings.push(format!("{consumer} output {key} consumes a filtered variable descendant without an owned item context; affected binding skipped"));
                    }
                    return true;
                }
            }
            pending.extend(
                self.value_feed_inputs(feed, &suffix, control)
                    .into_iter()
                    .map(|(feed, suffix, control)| (feed, suffix, control, context)),
            );
        }
        false
    }
}

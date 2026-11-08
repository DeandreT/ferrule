use std::cell::RefCell;

use ir::SchemaNode;
use mapping::{Graph, Project, Scope};

use crate::{TraceEvent, TraceSink};

#[path = "owned_scope_observers.rs"]
mod owned_scope_observers;

#[path = "trace/aggregates.rs"]
mod aggregates;
#[path = "trace/basic.rs"]
mod basic;
#[path = "trace/owned_output_trace_producer.rs"]
mod owned_output_trace_producer;
#[path = "trace/owned_scope_lifecycle.rs"]
mod owned_scope_lifecycle;
#[path = "trace/owned_trace_producer.rs"]
mod owned_trace_producer;
#[path = "trace/owned_trace_witness.rs"]
mod owned_trace_witness;
#[path = "trace/target_fields.rs"]
mod target_fields;

#[derive(Default)]
struct Collector(RefCell<Vec<TraceEvent>>);

impl TraceSink for Collector {
    fn record(&self, event: TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}

fn field_trace_project(target: SchemaNode, graph: Graph, root: Scope) -> Project {
    Project {
        execution_contract: None,
        source: SchemaNode::group("Input", Vec::new()),
        target,
        graph,
        root,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
    }
}

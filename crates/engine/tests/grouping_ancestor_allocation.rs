//! Requested-allocation volume of legacy multi-hop grouping (#782).
//!
//! One order holding `n` distinctly keyed items forms `n` groups. Each group
//! reads its intermediate order frame, so retaining a copy of that order per
//! group would request O(n^2) bytes; borrowing it keeps the volume linear.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use engine::run;
use ir::{Instance, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration};

thread_local! {
    static MEASURING: Cell<bool> = const { Cell::new(false) };
    static REQUESTED_BYTES: Cell<usize> = const { Cell::new(0) };
    static ALLOCATION_CALLS: Cell<usize> = const { Cell::new(0) };
}

struct CountingSystem;

fn record(bytes: usize) {
    let _ = MEASURING.try_with(|active| {
        if active.get() {
            REQUESTED_BYTES.with(|total| total.set(total.get().saturating_add(bytes)));
            ALLOCATION_CALLS.with(|total| total.set(total.get().saturating_add(1)));
        }
    });
}

unsafe impl GlobalAlloc for CountingSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        record(layout.size());
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        record(layout.size());
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let pointer = unsafe { System.realloc(pointer, layout, new_size) };
        record(new_size);
        pointer
    }
}

#[global_allocator]
static ALLOCATOR: CountingSystem = CountingSystem;

fn measure<T>(operation: impl FnOnce() -> T) -> (T, usize, usize) {
    REQUESTED_BYTES.with(|value| value.set(0));
    ALLOCATION_CALLS.with(|value| value.set(0));
    MEASURING.with(|value| value.set(true));
    let output = operation();
    MEASURING.with(|value| value.set(false));
    (
        output,
        REQUESTED_BYTES.with(Cell::get),
        ALLOCATION_CALLS.with(Cell::get),
    )
}

fn field(frame: &[&str], name: &str) -> Node {
    Node::SourceField {
        frame: Some(frame.iter().map(|segment| (*segment).into()).collect()),
        path: vec![name.into()],
    }
}

fn project() -> Project {
    Project {
        source: SchemaNode::group("root", vec![]),
        target: SchemaNode::group("root", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (0, field(&["Order", "Item"], "Key")),
                (1, field(&["Order"], "Id")),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Group".into(),
                iteration: ScopeIteration::Source(vec!["Order".into(), "Item".into()]),
                group_by: Some(0),
                bindings: vec![
                    Binding {
                        target_field: "Key".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "OrderId".into(),
                        node: 1,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn text(value: String) -> Instance {
    Instance::Scalar(Value::String(value))
}

fn source(items: usize) -> Instance {
    let order = group(vec![
        ("Id", text("O1".into())),
        (
            "Item",
            Instance::Repeated(
                (0..items)
                    .map(|index| group(vec![("Key", text(format!("K{index:05}")))]))
                    .collect(),
            ),
        ),
    ]);
    group(vec![("Order", Instance::Repeated(vec![order]))])
}

fn expected(items: usize) -> Instance {
    group(vec![(
        "Group",
        Instance::Repeated(
            (0..items)
                .map(|index| {
                    group(vec![
                        ("Key", text(format!("K{index:05}"))),
                        ("OrderId", text("O1".into())),
                    ])
                })
                .collect(),
        ),
    )])
}

#[test]
fn distinct_multihop_groups_request_linear_allocation_volume() {
    const N: usize = 256;
    let project = project();
    let mut observations = Vec::new();
    for items in [N, 2 * N] {
        // The project, input and expected output exist before counting. Each
        // realloc records its requested new size, not live heap or peak RSS.
        let source = source(items);
        let expected = expected(items);
        let (output, bytes, calls) = measure(|| run(&project, &source));
        assert_eq!(output, Ok(expected));
        println!("{{\"items\":{items},\"requested_bytes\":{bytes},\"allocation_calls\":{calls}}}");
        observations.push((bytes, calls));
    }
    let [(bytes, calls), (double_bytes, double_calls)] = observations[..] else {
        unreachable!("two observations");
    };
    // Linear volume roughly doubles; a per-group ancestor copy quadruples it.
    assert!(
        double_bytes * 4 <= bytes * 10,
        "doubling items grew requested bytes from {bytes} to {double_bytes}"
    );
    assert!(
        double_calls * 4 <= calls * 10,
        "doubling items grew allocation calls from {calls} to {double_calls}"
    );
}

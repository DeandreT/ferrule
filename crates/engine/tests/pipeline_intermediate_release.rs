//! Peak live heap bytes of a stage chain when every intermediate is retained
//! versus when each is released after its last reader.
//!
//! Run with `--nocapture` to print the table recorded in
//! `docs/performance/pipeline-intermediate-release.md`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;

use engine::{ExecutionContext, PipelineTargetRef, run_pipeline_retaining};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Graph, Pipeline, PipelineInput, PipelineStage, Project, Scope, ScopeConstruction};

thread_local! {
    static MEASURING: Cell<bool> = const { Cell::new(false) };
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

/// Counts live bytes on the measuring thread only, so concurrently running
/// tests in this binary do not perturb a measurement.
struct CountingSystem;

fn adjust(delta: isize) {
    let _ = MEASURING.try_with(|active| {
        if active.get() {
            LIVE.with(|live| {
                let next = live.get() + delta;
                live.set(next);
                PEAK.with(|peak| peak.set(peak.get().max(next)));
            });
        }
    });
}

fn size(bytes: usize) -> isize {
    isize::try_from(bytes).unwrap_or(isize::MAX)
}

unsafe impl GlobalAlloc for CountingSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            adjust(size(layout.size()));
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            adjust(size(layout.size()));
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        adjust(-size(layout.size()));
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let moved = unsafe { System.realloc(pointer, layout, new_size) };
        if !moved.is_null() {
            adjust(size(new_size) - size(layout.size()));
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: CountingSystem = CountingSystem;

/// The highest live-byte count reached during `operation`, relative to its start.
fn peak_bytes<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    LIVE.with(|live| live.set(0));
    PEAK.with(|peak| peak.set(0));
    MEASURING.with(|active| active.set(true));
    let output = operation();
    MEASURING.with(|active| active.set(false));
    let peak = PEAK.with(Cell::get);
    (output, usize::try_from(peak).unwrap_or_default())
}

const ROWS: usize = 1_024;
const TEXT_BYTES: usize = 1_024;

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Batch",
        vec![
            SchemaNode::group("Row", vec![SchemaNode::scalar("Text", ScalarType::String)])
                .repeating(),
        ],
    )
}

/// 1,024 rows of 1 KiB text: about 1 MiB of string payload per value.
fn batch() -> Instance {
    let rows = (0..ROWS)
        .map(|row| {
            let text = format!("{row:08}:{}", "x".repeat(TEXT_BYTES - 9));
            Instance::Group((vec![("Text".into(), Instance::Scalar(Value::String(text)))]).into())
        })
        .collect();
    Instance::Group((vec![("Row".into(), Instance::Repeated(rows))]).into())
}

fn copy_project() -> Project {
    Project {
        source: schema(),
        target: schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
    }
}

/// `stages` whole-batch copies in a line; only the last one is published.
fn chain(stages: usize) -> Pipeline {
    Pipeline {
        main_mapping_path: None,
        stages: (0..stages)
            .map(|index| PipelineStage {
                id: format!("s{index}"),
                mapping_path: None,
                project: copy_project(),
                source: match index {
                    0 => PipelineInput::Host {
                        name: "batch".into(),
                    },
                    _ => PipelineInput::StageTarget {
                        stage: format!("s{}", index - 1),
                        target: None,
                    },
                },
                extra_sources: Vec::new(),
            })
            .collect(),
    }
}

fn context(_: &str) -> ExecutionContext<'static> {
    ExecutionContext::new(Path::new("pipeline.json"))
}

#[test]
fn releasing_intermediates_keeps_chain_peak_flat() {
    let (_, value_bytes) = peak_bytes(batch);
    let mut rows = Vec::new();
    for stages in [1, 2, 4, 8] {
        let pipeline = chain(stages);
        let last = format!("s{}", stages - 1);
        let (all, all_peak) = peak_bytes(|| {
            let hosts = BTreeMap::from([("batch".to_string(), batch())]);
            engine::run_pipeline_with_stage_contexts(&pipeline, &hosts, context).unwrap()
        });
        let retain = [PipelineTargetRef {
            stage: &last,
            target: None,
        }];
        let (released, released_peak) = peak_bytes(|| {
            let hosts = BTreeMap::from([("batch".to_string(), batch())]);
            run_pipeline_retaining(&pipeline, hosts, &retain, context).unwrap()
        });
        let expected = batch();
        assert_eq!(
            all.stage(&last).map(|outputs| &outputs.primary),
            Some(&expected)
        );
        assert_eq!(released.get(&last, None), Some(&expected));
        rows.push((stages, all_peak, released_peak));
    }

    eprintln!("one value: {value_bytes} live bytes");
    eprintln!("stages | retain-all peak | releasing peak | releasing / retain-all");
    for (stages, all, released) in &rows {
        eprintln!(
            "{stages} | {all} | {released} | {:.3}",
            *released as f64 / *all as f64
        );
    }
    // Retaining every value grows with chain length; releasing holds at most
    // the current input and the current output.
    let (_, one_all, one_released) = rows[0];
    let (_, eight_all, eight_released) = rows[3];
    assert!(eight_all >= one_all + 6 * value_bytes, "{rows:?}");
    assert!(eight_released < one_released + value_bytes / 2, "{rows:?}");
    assert!(eight_released * 3 < eight_all, "{rows:?}");
}

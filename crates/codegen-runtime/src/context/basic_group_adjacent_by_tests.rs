//! Literal adjacent-run oracles. Shared helpers only construct fields, controls
//! and authenticated walks; no expected result calls a partitioning operation.
// Preserve the byte-bound independent component oracle source.
#[rustfmt::skip]
#[path = "basic_group_starting_with_tests.rs"]
mod basic_group_starting_with;
#[path = "basic_group_ending_with_tests.rs"]
mod basic_group_ending_with;
use super::*;
use std::cell::Cell;

fn adjacent_rows(source: &Instance, c: &mut Controls) -> Result<Vec<Instance>, RuntimeError> {
    let roots = BasicSourceRoots::try_new(source, &[], policy())?;
    let parent = roots.root_context()?;
    let original = parent.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])?;
    parent.map_group_adjacent_by_source_cohort(&original, c, &mut |g: &BasicScopeContext<'_>| {
        render(g, SourceOwnerRef::Primary, &["Rows"])
    })
}

fn aba() -> Instance {
    input([
        row(text("A"), "a1", true, 0),
        row(text("B"), "b1", true, 1),
        row(text("A"), "a2", true, 2),
    ])
}

// ID comes from immutable row data, not a possibly sorted/compacted position.
struct Probe {
    base: Controls,
    metadata_reads: Cell<usize>,
    log: Rc<RefCell<Vec<String>>>,
    selected: Option<&'static [usize]>,
    fail: Option<(&'static str, usize)>,
    change_directions: bool,
}
impl Probe {
    fn new() -> Self {
        Self {
            base: Controls::new(),
            metadata_reads: Cell::new(0),
            log: Rc::default(),
            selected: None,
            fail: None,
            change_directions: false,
        }
    }
    fn r04() -> Self {
        let mut p = Self::new();
        p.base.directions = vec![SortDirection::Ascending; 2];
        p.base.windows = vec![SequenceWindow::First(3), SequenceWindow::Last(3)];
        p.base.filter = true;
        p.base.post = true;
        p
    }
    fn id(&self, c: &BasicScopeContext<'_>) -> Result<usize, RuntimeError> {
        let Value::Int(id) = self.base.value(c, "Rank")? else {
            panic!("fixture Rank is a literal integer")
        };
        Ok(id as usize)
    }
    fn record(
        &self,
        phase: &'static str,
        suffix: impl std::fmt::Display,
    ) -> Result<(), RuntimeError> {
        let mut log = self.log.borrow_mut();
        log.push(format!("{phase}{suffix}"));
        let count = log.iter().filter(|s| s.starts_with(phase)).count();
        if self.fail == Some((phase, count)) {
            let (node, message) = match phase {
                "k" => (101, "key"),
                "f" => (102, "filter"),
                "s" => (103, "sort"),
                "w" => (104, "bound"),
                "p" => (105, "post"),
                _ => unreachable!(),
            };
            return Err(RuntimeError::MappingException {
                node,
                message: Some(message.into()),
            });
        }
        Ok(())
    }
}
impl BasicGroupByControls<RuntimeError> for Probe {
    fn metadata(&self) -> BasicGroupByMetadata<'_> {
        self.metadata_reads.set(self.metadata_reads.get() + 1);
        self.base.metadata()
    }
    fn filter(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        let id = self.id(c)?;
        self.record("f", id)?;
        Ok(self.selected.is_none_or(|selected| selected.contains(&id)))
    }
    fn sort_key(&mut self, key: usize, c: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        let id = self.id(c)?;
        self.record("s", format_args!("{id}{key}"))?;
        // The operation must have snapshotted directions, not re-read them.
        if self.change_directions {
            self.base.directions.clear();
        }
        Ok(Value::Int(id as i64))
    }
    fn window(
        &mut self,
        bound: usize,
        parent: &BasicScopeContext<'_>,
    ) -> Result<SequenceWindow, RuntimeError> {
        self.record("w", bound)?;
        assert_eq!(
            parent.owned_field(SourceOwnerRef::Primary, &["Marker"], None)?,
            text("P.root")
        );
        Ok(self.base.windows[bound])
    }
    fn group_key(&mut self, c: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        self.record("k", self.id(c)?)?;
        self.base.group_key(c)
    }
    fn retain_member(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        self.record("p", self.id(c)?)?;
        self.base.retain_member(c)
    }
}

fn probe_rows(source: &Instance, p: &mut Probe) -> Result<Vec<Instance>, RuntimeError> {
    let roots = BasicSourceRoots::try_new(source, &[], policy())?;
    let parent = roots.root_context()?;
    let original = parent.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])?;
    let log = Rc::clone(&p.log);
    parent.map_group_adjacent_by_source_cohort(&original, p, &mut |g: &BasicScopeContext<'_>| {
        // Emit the first raw ID as well as independently rendering members.
        let Value::Int(id) = g.owned_field(SourceOwnerRef::Primary, &["Rows", "Rank"], None)?
        else {
            panic!("literal Rank")
        };
        log.borrow_mut().push(format!("t{id}"));
        render(g, SourceOwnerRef::Primary, &["Rows"])
    })
}

#[test]
fn r01_empty_runs_both_parent_bounds_and_samples_metadata_once() {
    let mut p = Probe::r04();
    p.base.windows = vec![SequenceWindow::First(0), SequenceWindow::Last(1)];
    assert_eq!(
        probe_rows(&input([]), &mut p).unwrap(),
        Vec::<Instance>::new()
    );
    assert_eq!(p.metadata_reads.get(), 1);
    assert_eq!(&*p.log.borrow(), &["w0", "w1"]);
    p.log.borrow_mut().clear();
    p.fail = Some(("w", 2));
    assert_eq!(
        probe_rows(&input([]), &mut p).unwrap_err(),
        RuntimeError::MappingException {
            node: 104,
            message: Some("bound".into()),
        }
    );
    assert_eq!(&*p.log.borrow(), &["w0", "w1"]);
}

#[test]
fn r02_empty_foreign_registry_and_clone_parent_are_not_bypasses() {
    let source = input([]);
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let other_roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let other = other_roots.root_context().unwrap();
    let foreign = other
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let mut p = Probe::r04();
    assert_eq!(
        parent
            .map_group_adjacent_by_source_cohort(&foreign, &mut p, &mut constant_target)
            .unwrap_err(),
        BasicSourceBridgeError::RegistryMismatch.into()
    );
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let clone = parent.try_clone_view().unwrap();
    assert_eq!(
        clone
            .map_group_adjacent_by_source_cohort(&original, &mut p, &mut constant_target)
            .unwrap_err(),
        BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 0 }.into()
    );
    assert_eq!(p.metadata_reads.get(), 0);
    assert!(p.log.borrow().is_empty());
}

#[test]
fn r03_duplicate_incomplete_reordered_and_malformed_terminal_keep_exact_causes() {
    let source = aba();
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    for (variation, error, metadata_reads) in [
        (
            0,
            BasicSourceBridgeError::DuplicatePhaseCandidate {
                input_ordinal: 1,
                candidate_ordinal: 0,
            },
            0,
        ),
        (
            1,
            BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 2 },
            0,
        ),
        (
            2,
            BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 0 },
            0,
        ),
        (
            3,
            BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 1 },
            1,
        ),
    ] {
        let mut original = parent
            .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
            .unwrap();
        match variation {
            0 => original.candidates[1].ordinal = 0,
            1 => {
                original.candidates.pop();
            }
            2 => original.candidates.swap(0, 1),
            3 => original.candidates[1].terminal.origin_cut = usize::MAX,
            _ => unreachable!(),
        }
        let mut p = Probe::r04();
        assert_eq!(
            parent
                .map_group_adjacent_by_source_cohort(&original, &mut p, &mut constant_target)
                .unwrap_err(),
            error.into()
        );
        assert_eq!(p.metadata_reads.get(), metadata_reads);
        assert!(p.log.borrow().is_empty());
    }
}

#[test]
fn r04_three_candidates_three_runs_have_fixed_storage_and_snapshot_directions() {
    let source = aba();
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let p = Probe::r04();
    let prepared = prepare::<RuntimeError, _>(&parent, &original, &p).unwrap();
    assert_eq!(p.metadata_reads.get(), 1);
    assert_eq!(
        (
            prepared.directions.capacity(),
            prepared.cache.capacity(),
            prepared.windows.capacity()
        ),
        (2, 6, 2)
    );
    assert_eq!(
        (
            prepared.order.capacity(),
            prepared.scratch.capacity(),
            prepared.next.capacity()
        ),
        (3, 3, 3)
    );
    assert_eq!(
        (
            prepared.buckets.capacity(),
            prepared.keys.capacity(),
            prepared.output.capacity()
        ),
        (3, 3, 3)
    );
    assert_eq!(
        (
            prepared.plans.capacity(),
            prepared.templates.capacity(),
            prepared.selection.capacity()
        ),
        (3, 3, 3)
    );
    assert!(p.log.borrow().is_empty());
    drop(prepared);
    let mut p = Probe::r04();
    p.change_directions = true;
    assert_eq!(
        probe_rows(&source, &mut p).unwrap(),
        vec![
            expected("a1", 1, &["a1"]),
            expected("b1", 2, &["b1"]),
            expected("a2", 3, &["a2"])
        ]
    );
    assert_eq!(p.metadata_reads.get(), 1);
    assert_eq!(
        &*p.log.borrow(),
        &[
            "s00", "s01", "s10", "s11", "s20", "s21", "w0", "w1", "f0", "k0", "p0", "f1", "k1",
            "p1", "f2", "k2", "p2", "t0", "t1", "t2"
        ]
    );
    // The original public operation must remain global first-seen By.
    assert_eq!(
        run_rows(&source, &mut Controls::new()).unwrap(),
        vec![expected("a1", 1, &["a1", "a2"]), expected("b1", 2, &["b1"])]
    );
}

#[test]
fn r05_filter_then_sort_can_join_runs_and_has_literal_candidate_major_log() {
    let mut p = Probe::r04();
    p.base.prefilter = true;
    p.selected = Some(&[0, 2]);
    assert_eq!(
        probe_rows(&aba(), &mut p).unwrap(),
        vec![expected("a1", 1, &["a1", "a2"])]
    );
    assert_eq!(p.metadata_reads.get(), 1);
    assert_eq!(
        &*p.log.borrow(),
        &[
            "f0", "f1", "f2", "s00", "s01", "s20", "s21", "w0", "w1", "k0", "p0", "k2", "p2", "t0"
        ]
    );
}

#[test]
fn r06_sort_then_filter_has_literal_candidate_major_log() {
    let mut p = Probe::r04();
    p.selected = Some(&[0, 2]);
    assert_eq!(
        probe_rows(&aba(), &mut p).unwrap(),
        vec![expected("a1", 1, &["a1", "a2"])]
    );
    assert_eq!(p.metadata_reads.get(), 1);
    assert_eq!(
        &*p.log.borrow(),
        &[
            "s00", "s01", "s10", "s11", "s20", "s21", "w0", "w1", "f0", "k0", "p0", "f1", "f2",
            "k2", "p2", "t0"
        ]
    );
}

#[test]
fn r07_zero_window_does_not_hide_any_candidate_key_or_post() {
    let mut p = Probe::r04();
    p.base.windows[0] = SequenceWindow::First(0);
    assert!(probe_rows(&aba(), &mut p).unwrap().is_empty());
    assert_eq!(p.metadata_reads.get(), 1);
    assert_eq!(
        &*p.log.borrow(),
        &[
            "s00", "s01", "s10", "s11", "s20", "s21", "w0", "w1", "f0", "k0", "p0", "f1", "k1",
            "p1", "f2", "k2", "p2"
        ]
    );
}

#[test]
fn adjacent_whole_run_or_retention_does_not_coalesce_after_dropping_a_separator() {
    let source = input([
        row(text("A"), "a1", false, 30),
        row(text("B"), "b1", true, 10),
        row(text("A"), "a2", false, 20),
        row(text("A"), "a3", true, 25),
        row(text("C"), "c1", false, 40),
        row(text("A"), "a4", true, 50),
    ]);
    assert_eq!(
        adjacent_rows(&source, &mut Controls::new()).unwrap(),
        vec![
            expected("a1", 1, &["a1"]),
            expected("b1", 2, &["b1"]),
            expected("a2", 3, &["a2", "a3"]),
            expected("c1", 4, &["c1"]),
            expected("a4", 5, &["a4"])
        ]
    );
    let mut c = Controls::new();
    c.post = true;
    assert_eq!(
        adjacent_rows(&source, &mut c).unwrap(),
        vec![
            expected("b1", 1, &["b1"]),
            expected("a2", 2, &["a2", "a3"]),
            expected("a4", 3, &["a4"])
        ]
    );
    assert_eq!(
        &*c.calls.borrow(),
        &[
            "key", "post", "key", "post", "key", "post", "key", "post", "key", "post", "key",
            "post"
        ]
    );
    c.calls.borrow_mut().clear();
    c.windows = vec![SequenceWindow::SkipFirst(1), SequenceWindow::First(1)];
    assert_eq!(
        adjacent_rows(&source, &mut c).unwrap(),
        vec![expected("a2", 1, &["a2", "a3"])]
    );
    let dropped = input([row(text("A"), "drop", false, 0)]);
    assert!(adjacent_rows(&dropped, &mut c).unwrap().is_empty());
}

#[test]
fn adjacent_exact_tagged_equality_nan_signed_zero_null_and_unicode_are_literal() {
    let nan = Value::Float(f64::from_bits(0x7ff8_0000_0000_0042));
    let source = input([
        row(Value::Int(1), "i1", false, 0),
        row(Value::Float(1.0), "f1", false, 0),
        row(Value::Int(1), "i2", false, 0),
        row(Value::Float(0.0), "zp", false, 0),
        row(Value::Float(-0.0), "zn", false, 0),
        row(nan.clone(), "nan1", false, 0),
        row(nan, "nan2", false, 0),
        row(Value::Null, "n1", false, 0),
        row(Value::json_null(), "jn", false, 0),
        row(Value::Null, "n2", false, 0),
        row(text("A"), "sA1", false, 0),
        row(text("a"), "sa", false, 0),
        row(text("A"), "sA2", false, 0),
    ]);
    assert_eq!(
        adjacent_rows(&source, &mut Controls::new()).unwrap(),
        vec![
            expected("i1", 1, &["i1"]),
            expected("f1", 2, &["f1"]),
            expected("i2", 3, &["i2"]),
            expected("zp", 4, &["zp", "zn"]),
            expected("nan1", 5, &["nan1"]),
            expected("nan2", 6, &["nan2"]),
            expected("n1", 7, &["n1"]),
            expected("jn", 8, &["jn"]),
            expected("n2", 9, &["n2"]),
            expected("sA1", 10, &["sA1"]),
            expected("sa", 11, &["sa"]),
            expected("sA2", 12, &["sA2"]),
        ]
    );
    let unicode = input([
        row(text("é"), "u1", false, 0),
        row(text("é"), "u2", false, 0),
        row(text("e\u{301}"), "u3", false, 0),
        row(text("é"), "u4", false, 0),
    ]);
    assert_eq!(
        adjacent_rows(&unicode, &mut Controls::new()).unwrap(),
        vec![
            expected("u1", 1, &["u1", "u2"]),
            expected("u3", 2, &["u3"]),
            expected("u4", 3, &["u4"])
        ]
    );
}

#[test]
fn adjacent_phase_error_frontiers_and_lazy_targets_preserve_original_nodes() {
    for (phase, occurrence, sorted, prefilter, windows, log, node, message) in [
        ("k", 1, false, false, vec![], vec!["f0", "k0"], 101, "key"),
        (
            "p",
            1,
            false,
            false,
            vec![],
            vec!["f0", "k0", "p0"],
            105,
            "post",
        ),
        ("f", 2, true, true, vec![], vec!["f0", "f1"], 102, "filter"),
        ("s", 1, true, false, vec![], vec!["s00"], 103, "sort"),
        (
            "w",
            1,
            true,
            false,
            vec![SequenceWindow::First(0)],
            vec!["s00", "s10", "s20", "w0"],
            104,
            "bound",
        ),
        (
            "k",
            1,
            false,
            false,
            vec![SequenceWindow::First(0)],
            vec!["w0", "f0", "k0"],
            101,
            "key",
        ),
        (
            "p",
            1,
            false,
            false,
            vec![SequenceWindow::First(0)],
            vec!["w0", "f0", "k0", "p0"],
            105,
            "post",
        ),
        (
            "w",
            2,
            false,
            false,
            vec![SequenceWindow::First(0), SequenceWindow::Last(1)],
            vec!["w0", "w1"],
            104,
            "bound",
        ),
    ] {
        let mut p = Probe::new();
        p.base.filter = true;
        p.base.post = true;
        p.base.prefilter = prefilter;
        p.base.windows = windows;
        if sorted {
            p.base.directions.push(SortDirection::Ascending);
        }
        p.fail = Some((phase, occurrence));
        assert_eq!(
            probe_rows(&aba(), &mut p).unwrap_err(),
            RuntimeError::MappingException {
                node,
                message: Some(message.into())
            }
        );
        assert_eq!(&*p.log.borrow(), &log);
    }
    // No sort: FilterThenSort is not an eager filter phase. Implicit First also
    // cannot suppress the first key's failure.
    let mut p = Probe::new();
    p.base.filter = true;
    p.base.prefilter = true;
    p.base.implicit_first = true;
    p.fail = Some(("k", 1));
    assert_eq!(
        probe_rows(&aba(), &mut p).unwrap_err(),
        RuntimeError::MappingException {
            node: 101,
            message: Some("key".into())
        }
    );
    assert_eq!(&*p.log.borrow(), &["f0", "k0"]);
    // OR retention must still evaluate a later post after the first true.
    let mut p = Probe::new();
    p.base.key = Key::Constant(Value::Int(0));
    p.base.post = true;
    p.fail = Some(("p", 2));
    assert_eq!(
        probe_rows(&aba(), &mut p).unwrap_err(),
        RuntimeError::MappingException {
            node: 105,
            message: Some("post".into())
        }
    );
    assert_eq!(&*p.log.borrow(), &["k0", "p0", "k1", "p1"]);
    let mut p = Probe::r04();
    p.selected = Some(&[]);
    assert!(probe_rows(&aba(), &mut p).unwrap().is_empty());
    assert_eq!(
        &*p.log.borrow(),
        &[
            "s00", "s01", "s10", "s11", "s20", "s21", "w0", "w1", "f0", "f1", "f2"
        ]
    );
}

fn adjacent_budget(
    rows: usize,
    bytes: usize,
    key: Value,
) -> (Result<Vec<Instance>, RuntimeError>, Vec<&'static str>) {
    let source = input((0..rows).map(|_| row(Value::Int(0), "x", true, 0)));
    let mut limits = small_policy(bytes);
    limits.max_path_segments = 2; // Post reads Rows/Keep; the selector is still Rows.
    let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let mut c = Controls::new();
    c.key = Key::Constant(key);
    c.post = true;
    let result =
        parent.map_group_adjacent_by_source_cohort(&original, &mut c, &mut constant_target);
    let log = c.calls.borrow().clone();
    (result, log)
}

#[test]
fn r09_known_rows_metadata_is_32_bytes_independent_of_the_key() {
    // Three original Rows copies = 12; four template paths + wrapper = 20.
    let (result, log) = adjacent_budget(1, 31, Value::Int(0));
    assert_eq!(
        result.unwrap_err(),
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataBytes",
            used: 12,
            requested: 20,
            maximum: 31
        }
        .into()
    );
    assert!(log.is_empty());
    let (result, log) = adjacent_budget(1, 32, Value::Int(0));
    assert_eq!(result.unwrap(), vec![scalar(Value::Int(7))]);
    assert_eq!(log, vec!["key", "post"]);
}

#[test]
fn r10_each_utf8_key_including_duplicate_is_charged_before_its_post() {
    let (result, log) = adjacent_budget(1, 33, text("é"));
    assert_eq!(
        result.unwrap_err(),
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataBytes",
            used: 32,
            requested: 2,
            maximum: 33
        }
        .into()
    );
    assert_eq!(log, vec!["key"]);
    let (result, log) = adjacent_budget(1, 34, text("é"));
    assert_eq!(result.unwrap(), vec![scalar(Value::Int(7))]);
    assert_eq!(log, vec!["key", "post"]);
    let (result, log) = adjacent_budget(2, 67, text("é"));
    assert_eq!(
        result.unwrap_err(),
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataBytes",
            used: 66,
            requested: 2,
            maximum: 67
        }
        .into()
    );
    assert_eq!(log, vec!["key", "post", "key"]);
    let (result, log) = adjacent_budget(2, 68, text("é"));
    assert_eq!(result.unwrap(), vec![scalar(Value::Int(7))]);
    assert_eq!(log, vec!["key", "post", "key", "post"]);
}

#[test]
fn r11_unchanged_shared_layout_has_exact_210_slot_frontier() {
    let source = group([]);
    let limits = OwnedSourceLimits {
        max_roots: 1,
        max_active_frames: 1,
        max_path_segments: 0,
        max_metadata_bytes: 0,
        max_walk_contexts: 1,
    };
    let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
    let parent = roots.root_context().unwrap();
    let mut original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &[])
        .unwrap();
    replace_original_backing(&mut original, 4);
    let mut c = ScalarKeysControls::new(vec![Value::Int(0); 2], 48);
    // Independently enumerated seed38 + fixed/template/selection26 + windows144.
    let prepared = prepare::<RuntimeError, _>(&parent, &original, &c).unwrap();
    assert_eq!((prepared.budget.slots, prepared.budget.maximum), (210, 210));
    assert_eq!(
        (
            prepared.directions.capacity(),
            prepared.cache.capacity(),
            prepared.windows.capacity()
        ),
        (2, 2, 48)
    );
    assert_eq!(
        (
            prepared.buckets.capacity(),
            prepared.keys.capacity(),
            prepared.next.capacity()
        ),
        (1, 1, 1)
    );
    drop(prepared);
    assert_eq!(
        parent
            .map_group_adjacent_by_source_cohort(&original, &mut c, &mut constant_target)
            .unwrap(),
        vec![scalar(Value::Int(7))]
    );
    assert_eq!(
        c.calls.borrow().iter().filter(|p| **p == "bound").count(),
        48
    );
    c.calls.borrow_mut().clear();
    replace_original_backing(&mut original, 5);
    assert_eq!(
        parent
            .map_group_adjacent_by_source_cohort(&original, &mut c, &mut constant_target)
            .unwrap_err(),
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataSlots",
            used: 39,
            requested: 172,
            maximum: 210
        }
        .into()
    );
    assert!(c.calls.borrow().is_empty());
}

#[test]
fn r12_exact_425_slots_then_426_preserves_slot_before_known_text_priority() {
    let source = input([row(Value::Int(0), "x", false, 0)]);
    let mut limits = small_policy(31);
    limits.max_walk_contexts = 1;
    let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
    let parent = roots.root_context().unwrap();
    let mut original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let mut c = ScalarKeysControls::new(Vec::new(), 118);
    replace_original_backing(&mut original, 4);
    assert_eq!(
        parent
            .map_group_adjacent_by_source_cohort(&original, &mut c, &mut constant_target)
            .unwrap_err(),
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataBytes",
            used: 12,
            requested: 20,
            maximum: 31
        }
        .into()
    );
    replace_original_backing(&mut original, 5);
    assert_eq!(
        parent
            .map_group_adjacent_by_source_cohort(&original, &mut c, &mut constant_target)
            .unwrap_err(),
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataSlots",
            used: 48,
            requested: 378,
            maximum: 425
        }
        .into()
    );
    assert!(c.calls.borrow().is_empty());
}

#[test]
fn r15_window_inventory_slot_products_and_element_representability_are_checked() {
    let source = aba();
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    for (windows, implicit_first) in [(usize::MAX, true), (usize::MAX / 3 + 1, false)] {
        let mut p = Probe::new();
        p.base.virtual_windows = Some(windows);
        p.base.implicit_first = implicit_first;
        assert_eq!(
            parent
                .map_group_adjacent_by_source_cohort(&original, &mut p, &mut constant_target)
                .unwrap_err(),
            BasicSourceBridgeError::PairedMetadataArithmeticOverflow {
                component: "MetadataSlots"
            }
            .into()
        );
        assert_eq!(p.metadata_reads.get(), 1);
        assert!(p.log.borrow().is_empty());
    }
    let mut limits = policy();
    limits.max_walk_contexts = usize::MAX;
    assert_eq!(
        group_maximum(limits).unwrap_err(),
        BasicSourceBridgeError::PairedMetadataArithmeticOverflow {
            component: "MetadataSlots"
        }
        .into()
    );
    // Exercise the same preallocation guard without constructing an impossible
    // source/control slice or allocating the unrepresentable backing buffer.
    let count = isize::MAX as usize / std::mem::size_of::<Bucket>() + 1;
    assert_eq!(
        paired_capacity::<Bucket>(count).unwrap_err(),
        BasicSourceBridgeError::PairedMetadataArithmeticOverflow {
            component: "MetadataSlots"
        }
        .into()
    );
}

#[test]
fn r16_walk_count_and_synthetic_group_depth_keep_typed_exact_plus_one_limits() {
    let mut limits = small_policy(4096);
    limits.max_walk_contexts = 3;
    limits.max_path_segments = 2; // This success fixture reads Rows/Key.
    let source = aba();
    let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    assert_eq!(original.len(), 3);
    assert_eq!(
        parent
            .map_group_adjacent_by_source_cohort(
                &original,
                &mut Controls::new(),
                &mut constant_target
            )
            .unwrap(),
        vec![scalar(Value::Int(7)); 3]
    );
    limits.max_path_segments = 1; // Keep P1 for count refusal and root-depth probes.
    let source = input([
        row(text("A"), "a1", true, 0),
        row(text("B"), "b1", true, 1),
        row(text("A"), "a2", true, 2),
        row(text("B"), "b2", true, 3),
    ]);
    let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
    let parent = roots.root_context().unwrap();
    let mut p = Probe::new();
    let error = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .and_then(|original| {
            parent.map_group_adjacent_by_source_cohort(&original, &mut p, &mut constant_target)
        })
        .unwrap_err();
    assert_eq!(
        error,
        OwnedSourceError::MetadataLimit {
            kind: OwnedSourceLimitKind::WalkContexts,
            used: 3,
            requested: 1,
            max: 3
        }
        .into()
    );
    assert_eq!(p.metadata_reads.get(), 0);
    assert!(p.log.borrow().is_empty());

    // An identity walk of an actual nonrepeated root needs no Item frame; the
    // synthetic keyed group itself still consumes exactly one active frame.
    let source = group([]);
    for frames in [1, 0] {
        limits.max_active_frames = frames;
        let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
        let parent = roots.root_context().unwrap();
        let original = parent
            .walk_owned_source_cohort(SourceOwnerRef::Primary, &[])
            .unwrap();
        let mut c = Controls::new();
        c.key = Key::Constant(Value::Int(0));
        let result =
            parent.map_group_adjacent_by_source_cohort(&original, &mut c, &mut constant_target);
        if frames == 1 {
            assert_eq!(result.unwrap(), vec![scalar(Value::Int(7))]);
        } else {
            assert_eq!(
                result.unwrap_err(),
                BasicSourceBridgeError::PairedMetadataLimit {
                    component: "frame depth",
                    used: 0,
                    requested: 1,
                    maximum: 0
                }
                .into()
            );
            assert!(c.calls.borrow().is_empty());
        }
    }
}

#[test]
fn r16_complete_owned_and_raw_alias_selectors_admit_exact_and_reject_unvisited_suffixes() {
    for (rows, legacy_null) in [
        (Some(repeated([])), false),
        (Some(scalar(Value::Null)), true),
        (Some(scalar(Value::json_null())), true),
        (None, false),
    ] {
        let source = group(rows.clone().map(|rows| field("Rows", rows)));
        let roots = BasicSourceRoots::try_new(&source, &[], small_policy(4096)).unwrap();
        let parent = roots.root_context().unwrap();
        for legacy in [false, true] {
            let original = if legacy {
                parent.walk_source_cohort(&["Rows"])
            } else {
                parent.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
            }
            .unwrap();
            let mut c = Controls::new();
            c.key = Key::Constant(Value::Int(0));
            c.windows.push(SequenceWindow::First(0));
            assert!(
                parent
                    .map_group_adjacent_by_source_cohort(&original, &mut c, &mut constant_target)
                    .unwrap()
                    .is_empty()
            );
            // Owned walking prunes null slots; ordinary legacy selection keeps
            // its preexisting terminal-null candidate. Neither changes the
            // complete-selector refusal below.
            let exact_log: &[&str] = if legacy && legacy_null {
                &["bound", "key"]
            } else {
                &["bound"]
            };
            assert_eq!(&*c.calls.borrow(), exact_log);
            c.calls.borrow_mut().clear();
            let result = if legacy {
                parent.walk_source_cohort(&["Rows", "Extra"])
            } else {
                parent.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows", "Extra"])
            }
            .and_then(|original| {
                parent.map_group_adjacent_by_source_cohort(&original, &mut c, &mut constant_target)
            });
            assert_eq!(result.unwrap_err(), path_limit(2, 1));
            assert!(c.calls.borrow().is_empty());
        }
        let primary = group([]);
        let declarations = [DeclaredNamedSourceRoot {
            name: "Alias",
            instance: Some(&source),
        }];
        let mut limits = small_policy(4096);
        limits.max_roots = 2;
        let roots = BasicSourceRoots::try_new(&primary, &declarations, limits).unwrap();
        let parent = roots.root_context().unwrap();
        let original = parent.walk_source_cohort(&["Alias", "Rows"]).unwrap();
        let mut c = Controls::new();
        c.key = Key::Constant(Value::Int(0));
        c.windows.push(SequenceWindow::First(0));
        assert!(
            parent
                .map_group_adjacent_by_source_cohort(&original, &mut c, &mut constant_target)
                .unwrap()
                .is_empty()
        );
        let exact_log: &[&str] = if legacy_null {
            &["bound", "key"]
        } else {
            &["bound"]
        };
        assert_eq!(&*c.calls.borrow(), exact_log);
        c.calls.borrow_mut().clear();
        let result = parent
            .walk_source_cohort(&["Alias", "Rows", "Extra"])
            .and_then(|original| {
                parent.map_group_adjacent_by_source_cohort(&original, &mut c, &mut constant_target)
            });
        assert_eq!(result.unwrap_err(), path_limit(2, 1));
        assert!(c.calls.borrow().is_empty());
    }
    // A matching spelling is not an alias when the actual Primary field wins.
    let primary = group([field("Alias", repeated([]))]);
    let named = group([]);
    let declarations = [DeclaredNamedSourceRoot {
        name: "Alias",
        instance: Some(&named),
    }];
    let mut limits = small_policy(4096);
    limits.max_roots = 2;
    let roots = BasicSourceRoots::try_new(&primary, &declarations, limits).unwrap();
    let parent = roots.root_context().unwrap();
    assert_eq!(
        parent.walk_source_cohort(&["Alias", "Rows"]).err(),
        Some(path_limit(2, 1))
    );
}

#[test]
fn adjacent_repeated_root_pins_are_immutable_but_newer_exact_owner_members_shadow() {
    let primary = repeated([
        row(text("A"), "p1", true, 0),
        row(text("B"), "p2", true, 1),
        row(text("A"), "p3", true, 2),
        row(text("A"), "p4", true, 3),
    ]);
    let named = repeated([
        row(text("A"), "n1", true, 0),
        row(text("B"), "n2", true, 1),
        row(text("A"), "n3", true, 2),
        row(text("A"), "n4", true, 3),
    ]);
    let declarations = [DeclaredNamedSourceRoot {
        name: "Alias",
        instance: Some(&named),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declarations, policy()).unwrap();
    let parent = roots.root_context().unwrap();
    for (owner, identity, alias, expected_output) in [
        (
            SourceOwnerRef::Primary,
            OwnedSourceIdentity::Primary,
            false,
            vec![
                expected("p1", 1, &["p1"]),
                expected("p2", 2, &["p2"]),
                expected("p3", 3, &["p3", "p4"]),
            ],
        ),
        (
            SourceOwnerRef::Named("Alias"),
            OwnedSourceIdentity::Named("Alias".into()),
            false,
            vec![
                expected("n1", 1, &["n1"]),
                expected("n2", 2, &["n2"]),
                expected("n3", 3, &["n3", "n4"]),
            ],
        ),
        (
            SourceOwnerRef::Named("Alias"),
            OwnedSourceIdentity::Named("Alias".into()),
            true,
            vec![
                expected("n1", 1, &["n1"]),
                expected("n2", 2, &["n2"]),
                expected("n3", 3, &["n3", "n4"]),
            ],
        ),
    ] {
        let original = if alias {
            parent.walk_source_cohort(&["Alias"])
        } else {
            parent.walk_owned_source_cohort(owner, &[])
        }
        .unwrap();
        let mut c = Controls::new();
        c.owner = owner;
        c.path = &[];
        let output = parent
            .map_group_adjacent_by_source_cohort(&original, &mut c, &mut |g: &BasicScopeContext<
                '_,
            >| {
                let root_error =
                    RuntimeError::OwnedSource(OwnedSourceError::UniteratedRepetition {
                        owner: identity.clone(),
                        path: vec!["Key".into()],
                        segment: 0,
                    });
                assert_eq!(g.owned_field(owner, &["Key"], Some(&[])), Err(root_error));
                for nonrepeating in [false, true] {
                    let error = g
                        .owned_field_with_root_pin(owner, &["Key"], Some(&[]), nonrepeating)
                        .unwrap_err();
                    assert_eq!(
                        error,
                        OwnedSourceError::UniteratedRepetition {
                            owner: identity.clone(),
                            path: vec!["Key".into()],
                            segment: 0
                        }
                        .into()
                    );
                    let RuntimeError::OwnedSource(source) = &error else {
                        panic!("owned cause")
                    };
                    let cause = std::error::Error::source(&error)
                        .unwrap()
                        .downcast_ref::<OwnedSourceError>()
                        .unwrap();
                    assert!(std::ptr::eq(source, cause));
                }
                let owned = g.walk_owned_source_cohort(owner, &[])?;
                let legacy = if alias {
                    g.walk_source_cohort(&["Alias"])?
                } else {
                    g.walk_source_cohort(&[])?
                };
                assert_eq!(legacy.len(), owned.len());
                let mut members = Vec::new();
                for (index, member) in owned.iter().enumerate() {
                    let m = member.context();
                    let frames = &m.owned.frames;
                    assert_eq!(frames.len(), 2);
                    assert!(frames[0].kind == OwnedOccurrenceKind::GroupMembers);
                    assert!(frames[1].kind == OwnedOccurrenceKind::Item);
                    assert!(frames[0].collection.is_empty() && frames[1].collection.is_empty());
                    let label = m.owned_field(owner, &["Label"], Some(&[]))?;
                    for nonrepeating in [false, true] {
                        assert_eq!(
                            m.owned_field_with_root_pin(
                                owner,
                                &["Label"],
                                Some(&[]),
                                nonrepeating
                            )?,
                            label
                        );
                    }
                    assert_eq!(m.owned_position(owner, Some(&[]))?, index + 1);
                    let raw = legacy.get(index).unwrap().context();
                    assert_eq!(raw.owned_field(owner, &["Label"], Some(&[]))?, label);
                    assert_eq!(raw.owned_position(owner, Some(&[]))?, index + 1);
                    members.push(group([
                        field("Label", scalar(label)),
                        field("Position", scalar(Value::Int((index + 1) as i64))),
                    ]));
                }
                Ok::<_, RuntimeError>(group([
                    field("First", scalar(g.owned_field(owner, &["Label"], None)?)),
                    field(
                        "Position",
                        scalar(Value::Int(g.owned_position(owner, Some(&[]))? as i64)),
                    ),
                    field("Members", repeated(members)),
                ]))
            })
            .unwrap();
        assert_eq!(output, expected_output);
    }
}

#[test]
fn adjacent_root_group_cannot_be_shadowed_by_another_owner_or_rescue_an_absent_owner() {
    let primary = repeated([row(text("A"), "p1", true, 0)]);
    let named = repeated([row(text("A"), "n1", true, 0)]);
    let declarations = [
        DeclaredNamedSourceRoot {
            name: "Alias",
            instance: Some(&named),
        },
        DeclaredNamedSourceRoot {
            name: "Absent",
            instance: None,
        },
    ];
    let roots = BasicSourceRoots::try_new(&primary, &declarations, policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &[])
        .unwrap();
    let mut c = Controls::new();
    c.path = &[];
    let output = parent
        .map_group_adjacent_by_source_cohort(&original, &mut c, &mut |g: &BasicScopeContext<'_>| {
            assert_eq!(
                g.owned_field(SourceOwnerRef::Named("Alias"), &["Label"], Some(&[])),
                Err(OwnedSourceError::InactiveSourceFrame {
                    owner: OwnedSourceIdentity::Named("Alias".into()),
                    collection: Some(vec![])
                }
                .into())
            );
            let named_members = g.walk_owned_source_cohort(SourceOwnerRef::Named("Alias"), &[])?;
            for member in named_members.iter() {
                assert_eq!(
                    member
                        .context()
                        .owned_field(SourceOwnerRef::Primary, &["Key"], Some(&[])),
                    Err(OwnedSourceError::UniteratedRepetition {
                        owner: OwnedSourceIdentity::Primary,
                        path: vec!["Key".into()],
                        segment: 0
                    }
                    .into())
                );
                assert_eq!(
                    member.context().owned_field(
                        SourceOwnerRef::Named("Alias"),
                        &["Label"],
                        Some(&[])
                    )?,
                    text("n1")
                );
            }
            for nonrepeating in [false, true] {
                assert_eq!(
                    g.owned_field_with_root_pin(
                        SourceOwnerRef::Named("Absent"),
                        &["Label"],
                        Some(&[]),
                        nonrepeating
                    ),
                    Err(OwnedSourceError::SourceUnavailable {
                        name: "Absent".into()
                    }
                    .into())
                );
                assert_eq!(
                    g.owned_field_with_root_pin(
                        SourceOwnerRef::Named("Missing"),
                        &["Label"],
                        Some(&[]),
                        nonrepeating
                    ),
                    Err(OwnedSourceError::UnknownOwner {
                        name: "Missing".into()
                    }
                    .into())
                );
            }
            Ok::<_, RuntimeError>(scalar(g.owned_field(
                SourceOwnerRef::Primary,
                &["Label"],
                None,
            )?))
        })
        .unwrap();
    assert_eq!(output, vec![scalar(text("p1"))]);
}

#[test]
fn adjacent_nonempty_group_keeps_declared_root_bypass_and_strict_descendant_repetition() {
    let primary = input([group([
        field("Key", scalar(text("A"))),
        field("Details", repeated([scalar(Value::Int(1))])),
    ])]);
    let named = input([group([
        field("Key", scalar(text("A"))),
        field("Details", repeated([scalar(Value::Int(2))])),
    ])]);
    let declarations = [DeclaredNamedSourceRoot {
        name: "Alias",
        instance: Some(&named),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declarations, policy()).unwrap();
    let parent = roots.root_context().unwrap();
    for (owner, identity) in [
        (SourceOwnerRef::Primary, OwnedSourceIdentity::Primary),
        (
            SourceOwnerRef::Named("Alias"),
            OwnedSourceIdentity::Named("Alias".into()),
        ),
    ] {
        let original = parent.walk_owned_source_cohort(owner, &["Rows"]).unwrap();
        let mut c = Controls::new();
        c.owner = owner;
        let output = parent
            .map_group_adjacent_by_source_cohort(&original, &mut c, &mut |g: &BasicScopeContext<
                '_,
            >| {
                assert_eq!(
                    g.owned_field_with_root_pin(owner, &["Marker"], Some(&[]), false),
                    Err(OwnedSourceError::InactiveSourceFrame {
                        owner: identity.clone(),
                        collection: Some(vec![])
                    }
                    .into())
                );
                assert_eq!(
                    g.owned_field_with_root_pin(owner, &["Marker"], Some(&[]), true)?,
                    text("P.root")
                );
                assert_eq!(
                    g.owned_field_with_root_pin(owner, &["Rows", "Key"], Some(&[]), true),
                    Err(OwnedSourceError::UniteratedRepetition {
                        owner: identity.clone(),
                        path: vec!["Rows".into(), "Key".into()],
                        segment: 1
                    }
                    .into())
                );
                assert_eq!(
                    g.owned_field(owner, &["Rows", "Details"], None),
                    Err(OwnedSourceError::UniteratedRepetition {
                        owner: identity.clone(),
                        path: vec!["Rows".into(), "Details".into()],
                        segment: 2
                    }
                    .into())
                );
                constant_target(g)
            })
            .unwrap();
        assert_eq!(output, vec![scalar(Value::Int(7))]);
    }
}

#[test]
fn adjacent_nonrepeated_primary_and_exact_named_root_have_real_member_observations() {
    let primary = row(text("A"), "p", true, 0);
    let named = row(text("A"), "n", true, 0);
    let declarations = [DeclaredNamedSourceRoot {
        name: "Alias",
        instance: Some(&named),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declarations, policy()).unwrap();
    let parent = roots.root_context().unwrap();
    for (owner, label) in [
        (SourceOwnerRef::Primary, "p"),
        (SourceOwnerRef::Named("Alias"), "n"),
    ] {
        let original = parent.walk_owned_source_cohort(owner, &[]).unwrap();
        let mut c = Controls::new();
        c.owner = owner;
        c.path = &[];
        let output = parent
            .map_group_adjacent_by_source_cohort(&original, &mut c, &mut |g: &BasicScopeContext<
                '_,
            >| {
                assert_eq!(g.owned_position(owner, Some(&[]))?, 1);
                for nonrepeating in [false, true] {
                    assert_eq!(
                        g.owned_field_with_root_pin(owner, &["Label"], Some(&[]), nonrepeating)?,
                        text(label)
                    );
                }
                let members = g.walk_owned_source_cohort(owner, &[])?;
                assert_eq!(members.len(), 1);
                let member = members.get(0).unwrap().context();
                assert!(member.owned.frames.last().unwrap().kind == OwnedOccurrenceKind::Item);
                assert_eq!(
                    member.owned_field(owner, &["Label"], Some(&[]))?,
                    text(label)
                );
                Ok::<_, RuntimeError>(scalar(g.owned_field(owner, &["Label"], None)?))
            })
            .unwrap();
        assert_eq!(output, vec![scalar(text(label))]);
    }
}

#[test]
fn adjacent_crossed_parent_prefix_is_first_members_and_nested_regroup_stays_local() {
    let source = group([field(
        "Batches",
        repeated([
            group([
                field("Name", scalar(text("X"))),
                field(
                    "Rows",
                    repeated([row(text("A"), "x1", true, 0), row(text("B"), "x2", true, 1)]),
                ),
            ]),
            group([
                field("Name", scalar(text("Y"))),
                field(
                    "Rows",
                    repeated([row(text("B"), "y1", true, 2), row(text("A"), "y2", true, 3)]),
                ),
            ]),
        ]),
    )]);
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Batches", "Rows"])
        .unwrap();
    let mut c = Controls::new();
    c.path = &["Batches", "Rows"];
    let mut observed_prefixes = Vec::new();
    let mut nested_observed = Vec::new();
    let mut by_observed = Vec::new();
    let output = parent
        .map_group_adjacent_by_source_cohort(&original, &mut c, &mut |g: &BasicScopeContext<'_>| {
            let prefix = g.owned_field(
                SourceOwnerRef::Primary,
                &["Batches", "Name"],
                Some(&["Batches"]),
            )?;
            let members =
                g.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Batches", "Rows"])?;
            let mut member_prefixes = Vec::new();
            for member in members.iter() {
                member_prefixes.push(member.context().owned_field(
                    SourceOwnerRef::Primary,
                    &["Batches", "Name"],
                    Some(&["Batches"]),
                )?);
            }
            observed_prefixes.push((prefix, member_prefixes));
            let mut constant = Controls::new();
            constant.path = &["Batches", "Rows"];
            constant.key = Key::Constant(Value::Int(0));
            let nested = g.map_group_adjacent_by_source_cohort(
                &members,
                &mut constant,
                &mut |inner: &BasicScopeContext<'_>| {
                    render(inner, SourceOwnerRef::Primary, &["Batches", "Rows"])
                },
            )?;
            let by = g.map_group_by_source_cohort(
                &members,
                &mut constant,
                &mut |inner: &BasicScopeContext<'_>| {
                    render(inner, SourceOwnerRef::Primary, &["Batches", "Rows"])
                },
            )?;
            nested_observed.push(nested);
            by_observed.push(by);
            // The enclosing group ordinal must survive nested member iteration.
            assert_eq!(
                g.owned_position(SourceOwnerRef::Primary, Some(&["Batches", "Rows"]))?,
                observed_prefixes.len()
            );
            render(g, SourceOwnerRef::Primary, &["Batches", "Rows"])
        })
        .unwrap();
    assert_eq!(
        output,
        vec![
            expected("x1", 1, &["x1"]),
            expected("x2", 2, &["x2", "y1"]),
            expected("y2", 3, &["y2"])
        ]
    );
    assert_eq!(
        observed_prefixes,
        vec![
            (text("X"), vec![text("X")]),
            (text("X"), vec![text("X"), text("X")]),
            (text("Y"), vec![text("Y")])
        ]
    );
    let expected_nested = vec![
        vec![expected("x1", 1, &["x1"])],
        vec![expected("x2", 1, &["x2", "y1"])],
        vec![expected("y2", 1, &["y2"])],
    ];
    assert_eq!(nested_observed, expected_nested);
    assert_eq!(by_observed, expected_nested);
}

#[test]
fn adjacent_late_text_ledger_counts_a_later_equal_run_and_all_sort_keys() {
    // n=3: 36 original Rows bytes + 60 template bytes = 96 known bytes.
    // A,B,A each return one byte, including the last, separated A run.
    let source = aba();
    for bytes in [98, 99] {
        let mut limits = small_policy(bytes);
        limits.max_walk_contexts = 3;
        limits.max_path_segments = 2; // Keys/posts read Rows/Key and Rows/Keep.
        let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
        let parent = roots.root_context().unwrap();
        let original = parent
            .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
            .unwrap();
        let mut c = Controls::new();
        c.post = true;
        let result =
            parent.map_group_adjacent_by_source_cohort(&original, &mut c, &mut constant_target);
        if bytes == 98 {
            assert_eq!(
                result.unwrap_err(),
                BasicSourceBridgeError::PairedMetadataLimit {
                    component: "MetadataBytes",
                    used: 98,
                    requested: 1,
                    maximum: 98
                }
                .into()
            );
            assert_eq!(&*c.calls.borrow(), &["key", "post", "key", "post", "key"]);
        } else {
            assert_eq!(result.unwrap(), vec![scalar(Value::Int(7)); 3]);
            assert_eq!(
                &*c.calls.borrow(),
                &["key", "post", "key", "post", "key", "post"]
            );
        }
    }
    let source = group([]);
    for bytes in [3, 4] {
        let limits = OwnedSourceLimits {
            max_roots: 1,
            max_active_frames: 1,
            max_path_segments: 0,
            max_metadata_bytes: bytes,
            max_walk_contexts: 1,
        };
        let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
        let parent = roots.root_context().unwrap();
        let original = parent
            .walk_owned_source_cohort(SourceOwnerRef::Primary, &[])
            .unwrap();
        let mut c = ScalarKeysControls::new(vec![text("é"), text("é")], 1);
        let result =
            parent.map_group_adjacent_by_source_cohort(&original, &mut c, &mut constant_target);
        if bytes == 3 {
            assert_eq!(
                result.unwrap_err(),
                BasicSourceBridgeError::PairedMetadataLimit {
                    component: "MetadataBytes",
                    used: 2,
                    requested: 2,
                    maximum: 3
                }
                .into()
            );
            assert_eq!(&*c.calls.borrow(), &["sort", "sort"]);
        } else {
            assert_eq!(result.unwrap(), vec![scalar(Value::Int(7))]);
            assert_eq!(&*c.calls.borrow(), &["sort", "sort", "bound", "key"]);
        }
    }
    // A raised key wins at its callback frontier, before a successful return
    // could have been charged and before post. Known metadata still fits.
    let source = input([row(text("é"), "x", true, 0)]);
    let mut limits = small_policy(32);
    limits.max_path_segments = 2; // Probe reads Rows/Rank before raising the key.
    let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let mut p = Probe::new();
    p.base.post = true;
    p.fail = Some(("k", 1));
    assert_eq!(
        parent
            .map_group_adjacent_by_source_cohort(&original, &mut p, &mut constant_target)
            .unwrap_err(),
        RuntimeError::MappingException {
            node: 101,
            message: Some("key".into())
        }
    );
    assert_eq!(&*p.log.borrow(), &["k0"]);
}

struct PositionControls {
    base: Controls,
    bool_key: bool,
    post_rank: bool,
    parent_limit: bool,
    positions: Vec<(&'static str, usize)>,
}
impl PositionControls {
    fn new() -> Self {
        let mut base = Controls::new();
        base.directions.push(SortDirection::Ascending);
        Self {
            base,
            bool_key: false,
            post_rank: false,
            parent_limit: false,
            positions: Vec::new(),
        }
    }
}
impl BasicGroupByControls<RuntimeError> for PositionControls {
    fn metadata(&self) -> BasicGroupByMetadata<'_> {
        self.base.metadata()
    }
    fn filter(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        let position = c.owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))?;
        self.positions.push(("filter", position));
        Ok(position > 2)
    }
    fn sort_key(&mut self, key: usize, c: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        self.positions.push((
            "sort",
            c.owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))?,
        ));
        self.base.sort_key(key, c)
    }
    fn window(
        &mut self,
        bound: usize,
        parent: &BasicScopeContext<'_>,
    ) -> Result<SequenceWindow, RuntimeError> {
        if self.parent_limit {
            Ok(SequenceWindow::First(crate::item_count(
                104,
                parent.owned_field(SourceOwnerRef::Primary, &["Limit"], None)?,
            )?))
        } else {
            self.base.window(bound, parent)
        }
    }
    fn group_key(&mut self, c: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        let position = c.owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))?;
        self.positions.push(("key", position));
        if self.bool_key {
            Ok(Value::Bool(position <= 3))
        } else {
            self.base.group_key(c)
        }
    }
    fn retain_member(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        let position = c.owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))?;
        self.positions.push(("post", position));
        if self.post_rank {
            Ok(position == 4)
        } else {
            self.base.retain_member(c)
        }
    }
}

fn position_rows(
    source: &Instance,
    c: &mut PositionControls,
) -> Result<Vec<Instance>, RuntimeError> {
    let roots = BasicSourceRoots::try_new(source, &[], policy())?;
    let parent = roots.root_context()?;
    let original = parent.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])?;
    parent.map_group_adjacent_by_source_cohort(&original, c, &mut |g: &BasicScopeContext<'_>| {
        render(g, SourceOwnerRef::Primary, &["Rows"])
    })
}

#[test]
fn adjacent_sorted_and_filtered_positions_are_selected_ranks_not_group_ordinals() {
    let source = group([
        field("Limit", scalar(Value::Int(2))),
        field(
            "Rows",
            repeated([
                row(text("A"), "a1", false, 30),
                row(text("B"), "b1", true, 10),
                row(text("A"), "a2", false, 20),
                row(text("A"), "a3", true, 25),
                row(text("C"), "c1", false, 40),
                row(text("A"), "a4", true, 50),
            ]),
        ),
    ]);
    let mut c = PositionControls::new();
    assert_eq!(
        position_rows(&source, &mut c).unwrap(),
        vec![
            expected("b1", 1, &["b1"]),
            expected("a2", 2, &["a2", "a3", "a1"]),
            expected("c1", 3, &["c1"]),
            expected("a4", 4, &["a4"])
        ]
    );
    assert_eq!(
        &c.positions[..6],
        &[
            ("sort", 1),
            ("sort", 2),
            ("sort", 3),
            ("sort", 4),
            ("sort", 5),
            ("sort", 6)
        ]
    );
    assert_eq!(
        &c.positions[6..],
        &[
            ("key", 1),
            ("key", 2),
            ("key", 3),
            ("key", 4),
            ("key", 5),
            ("key", 6)
        ]
    );
    c.positions.clear();
    c.base.filter = true;
    c.base.prefilter = true;
    assert_eq!(
        position_rows(&source, &mut c).unwrap(),
        vec![
            expected("a2", 1, &["a2", "a3"]),
            expected("c1", 2, &["c1"]),
            expected("a4", 3, &["a4"])
        ]
    );
    assert_eq!(
        &c.positions[..6],
        &[
            ("filter", 1),
            ("filter", 2),
            ("filter", 3),
            ("filter", 4),
            ("filter", 5),
            ("filter", 6)
        ]
    );
    c.positions.clear();
    c.base.prefilter = false;
    assert_eq!(
        position_rows(&source, &mut c).unwrap(),
        vec![
            expected("a3", 1, &["a3", "a1"]),
            expected("c1", 2, &["c1"]),
            expected("a4", 3, &["a4"])
        ]
    );
    assert_eq!(
        &c.positions[6..],
        &[
            ("filter", 1),
            ("filter", 2),
            ("filter", 3),
            ("key", 3),
            ("filter", 4),
            ("key", 4),
            ("filter", 5),
            ("key", 5),
            ("filter", 6),
            ("key", 6)
        ]
    );
    c.base.post = true;
    c.post_rank = true;
    assert_eq!(
        position_rows(&source, &mut c).unwrap(),
        vec![expected("a3", 1, &["a3", "a1"])]
    );
    c.base.filter = false;
    c.base.post = false;
    c.bool_key = true;
    assert_eq!(
        position_rows(&source, &mut c).unwrap(),
        vec![
            expected("b1", 1, &["b1", "a2", "a3"]),
            expected("a1", 2, &["a1", "c1", "a4"])
        ]
    );
    c.bool_key = false;
    c.parent_limit = true;
    c.base.windows.push(SequenceWindow::First(0));
    assert_eq!(
        position_rows(&source, &mut c).unwrap(),
        vec![
            expected("b1", 1, &["b1"]),
            expected("a2", 2, &["a2", "a3", "a1"])
        ]
    );
}

struct TypedFailureControls {
    base: Controls,
    bad_bound: bool,
}
impl BasicGroupByControls<RuntimeError> for TypedFailureControls {
    fn metadata(&self) -> BasicGroupByMetadata<'_> {
        self.base.metadata()
    }
    fn filter(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        self.base.filter(c)
    }
    fn sort_key(&mut self, key: usize, c: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        self.base.sort_key(key, c)
    }
    fn window(
        &mut self,
        bound: usize,
        _: &BasicScopeContext<'_>,
    ) -> Result<SequenceWindow, RuntimeError> {
        self.base.record("bound")?;
        if self.bad_bound {
            Ok(SequenceWindow::First(crate::item_count(
                104,
                Value::Bool(true),
            )?))
        } else {
            Ok(self.base.windows[bound])
        }
    }
    fn group_key(&mut self, c: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        self.base.group_key(c)
    }
    fn retain_member(&mut self, _: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        self.base.record("post")?;
        crate::require_bool(105, text("not bool"))
    }
}

#[test]
fn adjacent_bool_count_and_later_target_failures_return_exact_errors_not_partial_output() {
    for empty in [false, true] {
        let source = if empty { input([]) } else { aba() };
        let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
        let parent = roots.root_context().unwrap();
        let original = parent
            .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
            .unwrap();
        let mut base = Controls::new();
        base.post = true;
        if empty {
            base.windows.push(SequenceWindow::First(0));
        }
        let mut c = TypedFailureControls {
            base,
            bad_bound: empty,
        };
        let error = parent
            .map_group_adjacent_by_source_cohort(&original, &mut c, &mut constant_target)
            .unwrap_err();
        if empty {
            assert_eq!(
                error,
                RuntimeError::NotAnItemCount {
                    node: 104,
                    found: "bool"
                }
            );
            assert_eq!(&*c.base.calls.borrow(), &["bound"]);
        } else {
            assert_eq!(
                error,
                RuntimeError::NotABool {
                    node: 105,
                    found: "string"
                }
            );
            assert_eq!(&*c.base.calls.borrow(), &["key", "post"]);
        }
    }
    let source = aba();
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let mut targets = 0;
    let error = parent
        .map_group_adjacent_by_source_cohort(
            &original,
            &mut Controls::new(),
            &mut |_: &BasicScopeContext<'_>| {
                targets += 1;
                if targets == 2 {
                    Err(RuntimeError::MappingException {
                        node: 106,
                        message: Some("target".into()),
                    })
                } else {
                    Ok(scalar(Value::Int(7)))
                }
            },
        )
        .unwrap_err();
    assert_eq!(
        error,
        RuntimeError::MappingException {
            node: 106,
            message: Some("target".into())
        }
    );
    assert_eq!(targets, 2);
}

#[test]
fn r08_higher_ranked_target_returns_owned_snapshots_of_three_distinct_runs() {
    // An explicit independently lifetime-generic target can only return an
    // owned Instance; the operation's local payload cannot escape by borrowing.
    fn snapshot(g: &BasicScopeContext<'_>) -> Result<Instance, RuntimeError> {
        render(g, SourceOwnerRef::Primary, &["Rows"])
    }
    let source = aba();
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let output = parent
        .map_group_adjacent_by_source_cohort(&original, &mut Controls::new(), &mut snapshot)
        .unwrap();
    // Inspection occurs after all local group payloads have been dropped.
    assert_eq!(
        output,
        vec![
            expected("a1", 1, &["a1"]),
            expected("b1", 2, &["b1"]),
            expected("a2", 3, &["a2"])
        ]
    );
}

#[test]
fn adjacent_scalar_root_keeps_exact_tags_and_named_rows_keep_exact_endpoint_identity() {
    let source = repeated([
        scalar(Value::Int(1)),
        scalar(text("1")),
        scalar(Value::Int(1)),
    ]);
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &[])
        .unwrap();
    let mut c = Controls::new();
    c.key = Key::Current;
    c.path = &[];
    let output = parent
        .map_group_adjacent_by_source_cohort(&original, &mut c, &mut |g: &BasicScopeContext<'_>| {
            g.owned_field(SourceOwnerRef::Primary, &[], None)
                .map(scalar)
        })
        .unwrap();
    assert_eq!(
        output,
        vec![
            scalar(Value::Int(1)),
            scalar(text("1")),
            scalar(Value::Int(1))
        ]
    );

    let primary = aba();
    let named = input([
        row(text("A"), "n1", true, 0),
        row(text("B"), "n2", true, 1),
        row(text("A"), "n3", true, 2),
    ]);
    let unicode = input([row(text("A"), "unicode", true, 0)]);
    let declarations = [
        DeclaredNamedSourceRoot {
            name: "Rows",
            instance: Some(&named),
        },
        DeclaredNamedSourceRoot {
            name: " Røws💠 ",
            instance: Some(&unicode),
        },
        DeclaredNamedSourceRoot {
            name: "Absent",
            instance: None,
        },
    ];
    let roots = BasicSourceRoots::try_new(&primary, &declarations, policy()).unwrap();
    let parent = roots.root_context().unwrap();
    for (owner, expectation) in [
        (
            SourceOwnerRef::Named("Rows"),
            vec![
                expected("n1", 1, &["n1"]),
                expected("n2", 2, &["n2"]),
                expected("n3", 3, &["n3"]),
            ],
        ),
        (
            SourceOwnerRef::Named(" Røws💠 "),
            vec![expected("unicode", 1, &["unicode"])],
        ),
    ] {
        let original = parent.walk_owned_source_cohort(owner, &["Rows"]).unwrap();
        let mut c = Controls::new();
        c.owner = owner;
        let output = parent
            .map_group_adjacent_by_source_cohort(&original, &mut c, &mut |g: &BasicScopeContext<
                '_,
            >| {
                assert_eq!(
                    g.owned_position(owner, Some(&["Rows"]))?,
                    g.owned_position(owner, None)?
                );
                render(g, owner, &["Rows"])
            })
            .unwrap();
        assert_eq!(output, expectation);
    }
    let mut p = Probe::new();
    let error = parent
        .walk_owned_source_cohort(SourceOwnerRef::Named("Absent"), &["Rows"])
        .and_then(|original| {
            parent.map_group_adjacent_by_source_cohort(&original, &mut p, &mut constant_target)
        })
        .unwrap_err();
    assert_eq!(
        error,
        OwnedSourceError::SourceUnavailable {
            name: "Absent".into()
        }
        .into()
    );
    assert_eq!(p.metadata_reads.get(), 0);
    assert!(p.log.borrow().is_empty());
}

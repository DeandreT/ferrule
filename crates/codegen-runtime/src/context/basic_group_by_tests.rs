use super::*;
use crate::{OwnedSourceIdentity, field, group, repeated, scalar};
use std::cell::RefCell;
use std::rc::Rc;

fn policy() -> OwnedSourceLimits {
    OwnedSourceLimits {
        max_roots: 8,
        max_active_frames: 16,
        max_path_segments: 16,
        max_metadata_bytes: 1_048_576,
        max_walk_contexts: 64,
    }
}
fn text(s: &str) -> Value {
    Value::String(s.to_owned())
}
fn row(key: Value, label: &str, keep: bool, rank: i64) -> Instance {
    group([
        field("Key", scalar(key)),
        field("Label", scalar(text(label))),
        field("Keep", scalar(Value::Bool(keep))),
        field("Rank", scalar(Value::Int(rank))),
    ])
}
fn input(rows: impl IntoIterator<Item = Instance>) -> Instance {
    group([
        field("Marker", scalar(text("P.root"))),
        field("Rows", repeated(rows)),
    ])
}
fn sample() -> Instance {
    input([
        row(text("A"), "a1", false, 30),
        row(text("B"), "b1", true, 10),
        row(text("A"), "a2", true, 20),
        row(text("C"), "c1", false, 40),
        row(text("B"), "b2", false, 15),
    ])
}

#[derive(Clone)]
enum Key {
    Field,
    Constant(Value),
    Position,
    Current,
}
struct Controls {
    owner: SourceOwnerRef<'static>,
    path: &'static [&'static str],
    directions: Vec<SortDirection>,
    windows: Vec<SequenceWindow>,
    virtual_windows: Option<usize>,
    implicit_first: bool,
    prefilter: bool,
    filter: bool,
    post: bool,
    key: Key,
    calls: Rc<RefCell<Vec<&'static str>>>,
    positions: Rc<RefCell<Vec<(&'static str, usize)>>>,
    fail: Option<(&'static str, usize)>,
}
impl Controls {
    fn new() -> Self {
        Self {
            owner: SourceOwnerRef::Primary,
            path: &["Rows"],
            directions: Vec::new(),
            windows: Vec::new(),
            virtual_windows: None,
            implicit_first: false,
            prefilter: false,
            filter: false,
            post: false,
            key: Key::Field,
            calls: Rc::default(),
            positions: Rc::default(),
            fail: None,
        }
    }
    fn record(&self, phase: &'static str) -> Result<(), RuntimeError> {
        let mut calls = self.calls.borrow_mut();
        calls.push(phase);
        if self.fail == Some((phase, calls.iter().filter(|p| **p == phase).count())) {
            return Err(RuntimeError::MappingException {
                node: 441,
                message: Some(phase.to_owned()),
            });
        }
        Ok(())
    }
    fn value(&self, c: &BasicScopeContext<'_>, leaf: &str) -> Result<Value, RuntimeError> {
        let mut path = self.path.to_vec();
        path.push(leaf);
        c.owned_field(self.owner, &path, None)
    }
}
impl BasicGroupByControls<RuntimeError> for Controls {
    fn metadata(&self) -> BasicGroupByMetadata<'_> {
        BasicGroupByMetadata {
            directions: &self.directions,
            filter_before_sort: self.prefilter,
            explicit_window_count: self.virtual_windows.unwrap_or(self.windows.len()),
            implicit_first: self.implicit_first,
            has_filter: self.filter,
            has_post_filter: self.post,
        }
    }
    fn filter(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        self.record("filter")?;
        let position = c.owned_position(self.owner, Some(self.path))?;
        self.positions.borrow_mut().push(("filter", position));
        Ok(position > 1)
    }
    fn sort_key(&mut self, _: usize, c: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        self.record("sort")?;
        self.positions
            .borrow_mut()
            .push(("sort", c.owned_position(self.owner, Some(self.path))?));
        self.value(c, "Rank")
    }
    fn window(
        &mut self,
        bound: usize,
        _: &BasicScopeContext<'_>,
    ) -> Result<SequenceWindow, RuntimeError> {
        self.record("bound")?;
        Ok(self.windows[bound])
    }
    fn group_key(&mut self, c: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        self.record("key")?;
        match &self.key {
            Key::Constant(value) => Ok(value.clone()),
            Key::Field => self.value(c, "Key"),
            Key::Position => Ok(Value::Int(
                c.owned_position(self.owner, Some(self.path))? as i64
            )),
            Key::Current => c.owned_field(self.owner, &[], None),
        }
    }
    fn retain_member(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        self.record("post")?;
        Ok(self.value(c, "Keep")? == Value::Bool(true))
    }
}

fn render(
    c: &BasicScopeContext<'_>,
    owner: SourceOwnerRef<'_>,
    path: &[&str],
) -> Result<Instance, RuntimeError> {
    let mut label = path.to_vec();
    label.push("Label");
    let representative = c.owned_field(owner, &label, None)?;
    assert_eq!(representative, c.owned_field(owner, &label, Some(path))?);
    let mut members = Vec::new();
    for member in c.walk_owned_source_cohort(owner, path)?.iter() {
        members.push(group([
            field(
                "Label",
                scalar(member.context().owned_field(owner, &label, None)?),
            ),
            field(
                "Position",
                scalar(Value::Int(
                    member.context().owned_position(owner, Some(path))? as i64,
                )),
            ),
        ]));
    }
    Ok(group([
        field("First", scalar(representative)),
        field(
            "Position",
            scalar(Value::Int(c.owned_position(owner, Some(path))? as i64)),
        ),
        field("Members", repeated(members)),
    ]))
}
fn expected(first: &str, position: i64, members: &[&str]) -> Instance {
    group([
        field("First", scalar(text(first))),
        field("Position", scalar(Value::Int(position))),
        field(
            "Members",
            repeated(members.iter().enumerate().map(|(i, name)| {
                group([
                    field("Label", scalar(text(name))),
                    field("Position", scalar(Value::Int((i + 1) as i64))),
                ])
            })),
        ),
    ])
}
fn run_rows(source: &Instance, controls: &mut Controls) -> Result<Vec<Instance>, RuntimeError> {
    let roots = BasicSourceRoots::try_new(source, &[], policy())?;
    let parent = roots.root_context()?;
    let original = parent.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])?;
    parent.map_group_by_source_cohort(&original, controls, &mut |c: &BasicScopeContext<'_>| {
        render(c, SourceOwnerRef::Primary, &["Rows"])
    })
}

#[test]
fn exact_tagged_keys_first_representatives_and_member_positions() {
    let source = input([
        row(Value::Int(1), "i1", false, 0),
        row(text("1"), "s1", false, 0),
        row(Value::Int(1), "i2", false, 0),
        row(Value::Float(1.0), "f1", false, 0),
        row(Value::Null, "null", false, 0),
        row(Value::json_null(), "json-null", false, 0),
        row(Value::Float(0.0), "zero", false, 0),
        row(Value::Float(-0.0), "negative-zero", false, 0),
        row(Value::Float(f64::NAN), "nan1", false, 0),
        row(Value::Float(f64::NAN), "nan2", false, 0),
    ]);
    assert_eq!(
        run_rows(&source, &mut Controls::new()).unwrap(),
        vec![
            expected("i1", 1, &["i1", "i2"]),
            expected("s1", 2, &["s1"]),
            expected("f1", 3, &["f1"]),
            expected("null", 4, &["null"]),
            expected("json-null", 5, &["json-null"]),
            expected("zero", 6, &["zero", "negative-zero"]),
            expected("nan1", 7, &["nan1"]),
            expected("nan2", 8, &["nan2"]),
        ]
    );
}

#[test]
fn any_member_retains_false_representative_and_false_members() {
    let mut c = Controls::new();
    c.post = true;
    assert_eq!(
        run_rows(&sample(), &mut c).unwrap(),
        vec![
            expected("a1", 1, &["a1", "a2"]),
            expected("b1", 2, &["b1", "b2"])
        ]
    );
    assert_eq!(c.calls.borrow().iter().filter(|p| **p == "post").count(), 5);
    let source = input([
        row(text("A"), "drop", false, 0),
        row(text("B"), "first", false, 0),
        row(text("B"), "true", true, 0),
    ]);
    assert_eq!(
        run_rows(&source, &mut c).unwrap(),
        vec![expected("first", 1, &["first", "true"])]
    );
    assert!(
        run_rows(&input([row(text("A"), "drop", false, 0)]), &mut c)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn sort_filter_orders_preserve_structural_keys_sorted_selection_and_final_groups() {
    let mut c = Controls::new();
    c.filter = true;
    c.post = true;
    c.directions.push(SortDirection::Ascending);
    c.prefilter = true;
    assert_eq!(
        run_rows(&sample(), &mut c).unwrap(),
        vec![expected("b1", 1, &["b1", "b2"]), expected("a2", 2, &["a2"])]
    );
    assert_eq!(
        &c.positions.borrow()[..5],
        &[
            ("filter", 1),
            ("filter", 2),
            ("filter", 3),
            ("filter", 4),
            ("filter", 5)
        ]
    );
    c.calls.borrow_mut().clear();
    c.positions.borrow_mut().clear();
    c.prefilter = false;
    assert_eq!(
        run_rows(&sample(), &mut c).unwrap(),
        vec![expected("a2", 1, &["a2", "a1"])]
    );
    assert_eq!(
        &c.positions.borrow()[..5],
        &[
            ("sort", 1),
            ("sort", 2),
            ("sort", 3),
            ("sort", 4),
            ("sort", 5)
        ]
    );
    assert_eq!(
        &c.positions.borrow()[5..],
        &[
            ("filter", 1),
            ("filter", 2),
            ("filter", 3),
            ("filter", 4),
            ("filter", 5)
        ]
    );
    c.filter = false;
    c.post = false;
    c.key = Key::Position;
    let output = run_rows(&sample(), &mut c).unwrap();
    assert_eq!(
        output,
        vec![
            expected("b1", 1, &["b1"]),
            expected("b2", 2, &["b2"]),
            expected("a2", 3, &["a2"]),
            expected("a1", 4, &["a1"]),
            expected("c1", 5, &["c1"])
        ]
    );
}

#[test]
fn parent_bounds_precede_candidate_major_key_post_and_zero_window_does_not_skip_them() {
    let mut c = Controls::new();
    c.filter = true;
    c.post = true;
    c.windows.push(SequenceWindow::First(0));
    c.fail = Some(("key", 1));
    assert!(matches!(
        run_rows(&sample(), &mut c),
        Err(RuntimeError::MappingException { node: 441, .. })
    ));
    assert_eq!(&*c.calls.borrow(), &["bound", "filter", "filter", "key"]);
    let mut c = Controls::new();
    c.post = true;
    c.fail = Some(("post", 2));
    let source = input([
        row(text("A"), "yes", true, 0),
        row(text("A"), "later", true, 0),
    ]);
    assert!(run_rows(&source, &mut c).is_err());
    assert_eq!(&*c.calls.borrow(), &["key", "post", "key", "post"]);
    let mut c = Controls::new();
    c.filter = true;
    c.directions.push(SortDirection::Ascending);
    c.prefilter = true;
    c.fail = Some(("filter", 2));
    assert!(run_rows(&sample(), &mut c).is_err());
    assert_eq!(&*c.calls.borrow(), &["filter", "filter"]);
}

#[test]
fn empty_cohort_runs_all_bounds_but_no_member_or_target_callbacks() {
    let mut c = Controls::new();
    c.filter = true;
    c.post = true;
    c.windows = vec![SequenceWindow::First(0), SequenceWindow::Last(1)];
    assert!(run_rows(&input([]), &mut c).unwrap().is_empty());
    assert_eq!(&*c.calls.borrow(), &["bound", "bound"]);
    c.calls.borrow_mut().clear();
    c.fail = Some(("bound", 2));
    assert!(run_rows(&input([]), &mut c).is_err());
    assert_eq!(&*c.calls.borrow(), &["bound", "bound"]);
}

#[test]
fn windows_apply_to_groups_then_compact_group_ordinals_only() {
    let mut c = Controls::new();
    c.windows = vec![SequenceWindow::SkipFirst(1), SequenceWindow::First(1)];
    assert_eq!(
        run_rows(&sample(), &mut c).unwrap(),
        vec![expected("b1", 1, &["b1", "b2"])]
    );
    c.windows = vec![
        SequenceWindow::FromTo { first: 2, last: 3 },
        SequenceWindow::Last(1),
    ];
    assert_eq!(
        run_rows(&sample(), &mut c).unwrap(),
        vec![expected("c1", 1, &["c1"])]
    );
    c.windows.clear();
    c.implicit_first = true;
    assert_eq!(
        run_rows(&sample(), &mut c).unwrap(),
        vec![expected("a1", 1, &["a1", "a2"])]
    );
}

#[test]
fn nonrepeating_primary_and_named_root_extensions_have_authentic_members() {
    let primary = group([field("Marker", scalar(text("P.root")))]);
    let named = group([field("Marker", scalar(text("N.root")))]);
    let declared = [DeclaredNamedSourceRoot {
        name: "Rows",
        instance: Some(&named),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declared, policy()).unwrap();
    let parent = roots.root_context().unwrap();
    for (owner, marker) in [
        (SourceOwnerRef::Primary, "P.root"),
        (SourceOwnerRef::Named("Rows"), "N.root"),
    ] {
        let original = parent.walk_owned_source_cohort(owner, &[]).unwrap();
        let mut c = Controls::new();
        c.key = Key::Constant(Value::Int(1));
        let output = parent
            .map_group_by_source_cohort(&original, &mut c, &mut |context: &BasicScopeContext<
                '_,
            >| {
                assert_eq!(context.owned_position(owner, Some(&[]))?, 1);
                assert_eq!(context.owned_field(owner, &["Marker"], None)?, text(marker));
                assert_eq!(
                    context.owned_field_with_root_pin(owner, &["Marker"], Some(&[]), true)?,
                    text(marker)
                );
                let members = context.walk_owned_source_cohort(owner, &[])?;
                assert_eq!(members.len(), 1);
                assert_eq!(
                    members
                        .get(0)
                        .unwrap()
                        .context()
                        .owned_position(owner, Some(&[]))?,
                    1
                );
                Ok::<_, RuntimeError>(scalar(text(marker)))
            })
            .unwrap();
        assert_eq!(output, vec![scalar(text(marker))]);
    }
}

#[test]
fn repeated_scalar_root_uses_representative_only_for_trusted_group() {
    let source = repeated([
        scalar(Value::Int(7)),
        scalar(Value::Int(7)),
        scalar(Value::Int(8)),
    ]);
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    assert!(matches!(
        parent.owned_field(SourceOwnerRef::Primary, &[], None),
        Err(RuntimeError::OwnedSource(_))
    ));
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &[])
        .unwrap();
    struct ScalarControls(Controls);
    impl BasicGroupByControls<RuntimeError> for ScalarControls {
        fn metadata(&self) -> BasicGroupByMetadata<'_> {
            self.0.metadata()
        }
        fn filter(&mut self, _: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
            unreachable!()
        }
        fn sort_key(&mut self, _: usize, _: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
            unreachable!()
        }
        fn window(
            &mut self,
            _: usize,
            _: &BasicScopeContext<'_>,
        ) -> Result<SequenceWindow, RuntimeError> {
            unreachable!()
        }
        fn group_key(&mut self, c: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
            c.owned_field(SourceOwnerRef::Primary, &[], None)
        }
        fn retain_member(&mut self, _: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
            unreachable!()
        }
    }
    let mut c = ScalarControls(Controls::new());
    let output = parent
        .map_group_by_source_cohort(&original, &mut c, &mut |c: &BasicScopeContext<'_>| {
            let members = c.walk_owned_source_cohort(SourceOwnerRef::Primary, &[])?;
            Ok::<_, RuntimeError>(group([
                field(
                    "First",
                    scalar(c.owned_field(SourceOwnerRef::Primary, &[], None)?),
                ),
                field("Count", scalar(Value::Int(members.len() as i64))),
            ]))
        })
        .unwrap();
    assert_eq!(
        output,
        vec![
            group([
                field("First", scalar(Value::Int(7))),
                field("Count", scalar(Value::Int(2)))
            ]),
            group([
                field("First", scalar(Value::Int(8))),
                field("Count", scalar(Value::Int(1)))
            ])
        ]
    );
}

#[test]
fn named_collision_same_owner_regroup_and_legacy_empty_member_projection() {
    let primary = sample();
    let named = input([
        row(text("A"), "N1", false, 0),
        row(text("A"), "N2", false, 0),
    ]);
    let declared = [DeclaredNamedSourceRoot {
        name: "Rows",
        instance: Some(&named),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declared, policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let output = parent
        .map_group_by_source_cohort(
            &original,
            &mut Controls::new(),
            &mut |outer: &BasicScopeContext<'_>| {
                let own = outer.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])?;
                let legacy = outer.walk_source_cohort(&[])?;
                assert_eq!(own.len(), legacy.len());
                for (index, member) in legacy.iter().enumerate() {
                    assert_eq!(
                        member
                            .context()
                            .owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))?,
                        index + 1
                    );
                }
                let mut constant = Controls::new();
                constant.key = Key::Constant(Value::Int(1));
                let regrouped = outer.map_group_by_source_cohort(
                    &own,
                    &mut constant,
                    &mut |inner: &BasicScopeContext<'_>| {
                        render(inner, SourceOwnerRef::Primary, &["Rows"])
                    },
                )?;
                assert_eq!(regrouped.len(), 1);
                let named =
                    outer.walk_owned_source_cohort(SourceOwnerRef::Named("Rows"), &["Rows"])?;
                let mut c = Controls::new();
                c.owner = SourceOwnerRef::Named("Rows");
                let groups = outer.map_group_by_source_cohort(
                    &named,
                    &mut c,
                    &mut |inner: &BasicScopeContext<'_>| {
                        assert_eq!(
                            inner.owned_field(SourceOwnerRef::Primary, &["Rows", "Label"], None)?,
                            outer.owned_field(SourceOwnerRef::Primary, &["Rows", "Label"], None)?
                        );
                        render(inner, SourceOwnerRef::Named("Rows"), &["Rows"])
                    },
                )?;
                assert_eq!(groups, vec![expected("N1", 1, &["N1", "N2"])]);
                Ok::<_, RuntimeError>(regrouped.into_iter().next().unwrap())
            },
        )
        .unwrap();
    assert_eq!(
        output,
        vec![
            expected("a1", 1, &["a1", "a2"]),
            expected("b1", 1, &["b1", "b2"]),
            expected("c1", 1, &["c1"])
        ]
    );
}

#[test]
fn crossed_parent_prefix_and_retained_ancestor_phase_belong_to_first_member() {
    let batch = |id: &str, label: &str| {
        group([
            field("Id", scalar(text(id))),
            field("Rows", repeated([row(text("A"), label, false, 0)])),
        ])
    };
    let source = group([field(
        "Batch",
        repeated([batch("first", "a1"), batch("second", "a2")]),
    )]);
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Batch", "Rows"])
        .unwrap();
    let mut c = Controls::new();
    c.path = &["Batch", "Rows"];
    let output = parent
        .map_group_by_source_cohort(&original, &mut c, &mut |group: &BasicScopeContext<'_>| {
            assert_eq!(
                group.owned_field(SourceOwnerRef::Primary, &["Batch", "Id"], None)?,
                text("first")
            );
            for member in group
                .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Batch", "Rows"])?
                .iter()
            {
                assert_eq!(
                    member.context().owned_field(
                        SourceOwnerRef::Primary,
                        &["Batch", "Id"],
                        None
                    )?,
                    text("first")
                );
            }
            render(group, SourceOwnerRef::Primary, &["Batch", "Rows"])
        })
        .unwrap();
    assert_eq!(output, vec![expected("a1", 1, &["a1", "a2"])]);
    let batches = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Batch"])
        .unwrap();
    let phased = batches.get(1).unwrap().with_phase_position(8).unwrap();
    let original = phased
        .context()
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Batch", "Rows"])
        .unwrap();
    phased
        .context()
        .map_group_by_source_cohort(&original, &mut c, &mut |group: &BasicScopeContext<'_>| {
            assert_eq!(
                group.owned_position(SourceOwnerRef::Primary, Some(&["Batch"]))?,
                8
            );
            Ok::<_, RuntimeError>(scalar(Value::Int(8)))
        })
        .unwrap();
}

#[test]
fn descendant_repetition_is_not_given_representative_permission() {
    let source = input([group([
        field("Key", scalar(text("A"))),
        field("Nested", repeated([scalar(Value::Int(1))])),
    ])]);
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let result = parent.map_group_by_source_cohort(
        &original,
        &mut Controls::new(),
        &mut |group: &BasicScopeContext<'_>| {
            group
                .owned_field(SourceOwnerRef::Primary, &["Rows", "Nested"], None)
                .map(scalar)
        },
    );
    assert!(matches!(
        result,
        Err(RuntimeError::OwnedSource(
            OwnedSourceError::UniteratedRepetition { .. }
        ))
    ));
}

fn constant_target(_: &BasicScopeContext<'_>) -> Result<Instance, RuntimeError> {
    Ok(scalar(Value::Int(7)))
}
fn small_policy(bytes: usize) -> OwnedSourceLimits {
    OwnedSourceLimits {
        max_roots: 1,
        max_active_frames: 1,
        max_path_segments: 1,
        max_metadata_bytes: bytes,
        max_walk_contexts: 2,
    }
}
fn budget_run(
    rows: usize,
    bytes: usize,
    key: Value,
) -> (Result<Vec<Instance>, RuntimeError>, Vec<&'static str>) {
    let source = input((0..rows).map(|_| row(Value::Int(0), "x", false, 0)));
    let roots = BasicSourceRoots::try_new(&source, &[], small_policy(bytes)).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let mut c = Controls::new();
    c.key = Key::Constant(key);
    let result = parent.map_group_by_source_cohort(&original, &mut c, &mut constant_target);
    let calls = c.calls.borrow().clone();
    (result, calls)
}

#[test]
fn exact_known_text_frontier_is_independent_of_group_key_or_target() {
    // One original Rows path occurs in owned/origin/legacy views (12 bytes).
    // Its group adds four Rows path snapshots plus one wrapper spelling (20).
    let (result, calls) = budget_run(1, 31, Value::Int(0));
    assert_eq!(
        result.unwrap_err(),
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataBytes",
            used: 12,
            requested: 20,
            maximum: 31,
        }
        .into()
    );
    assert!(calls.is_empty());
    assert_eq!(
        budget_run(1, 32, Value::Int(0)).0.unwrap(),
        vec![scalar(Value::Int(7))]
    );
}

#[test]
fn returned_string_key_uses_utf8_and_cumulative_ledger_even_when_equal() {
    let (result, calls) = budget_run(1, 33, text("é"));
    assert_eq!(
        result.unwrap_err(),
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataBytes",
            used: 32,
            requested: 2,
            maximum: 33,
        }
        .into()
    );
    assert_eq!(calls, vec!["key"]);
    assert_eq!(budget_run(1, 34, text("é")).0.unwrap().len(), 1);
    let (result, calls) = budget_run(2, 67, text("é"));
    assert_eq!(
        result.unwrap_err(),
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataBytes",
            used: 66,
            requested: 2,
            maximum: 67,
        }
        .into()
    );
    assert_eq!(calls, vec!["key", "key"]);
    assert_eq!(budget_run(2, 68, text("é")).0.unwrap().len(), 1);
}

#[test]
fn foreign_empty_mismatched_parent_and_duplicate_witnesses_precede_callbacks() {
    let source = input([]);
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let other_roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let other = other_roots.root_context().unwrap();
    let empty = other
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let mut c = Controls::new();
    assert_eq!(
        parent
            .map_group_by_source_cohort(&empty, &mut c, &mut constant_target)
            .unwrap_err(),
        BasicSourceBridgeError::RegistryMismatch.into()
    );
    let empty = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let cloned = parent.try_clone_view().unwrap();
    assert!(matches!(
        cloned.map_group_by_source_cohort(&empty, &mut c, &mut constant_target),
        Err(RuntimeError::BasicSourceBridge(
            BasicSourceBridgeError::InvalidCohortWitness { .. }
        ))
    ));
    assert!(c.calls.borrow().is_empty());
    let source = sample();
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let mut original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    original.candidates[1].ordinal = 0;
    assert_eq!(
        parent
            .map_group_by_source_cohort(&original, &mut c, &mut constant_target)
            .unwrap_err(),
        BasicSourceBridgeError::DuplicatePhaseCandidate {
            input_ordinal: 1,
            candidate_ordinal: 0
        }
        .into()
    );
    assert!(c.calls.borrow().is_empty());
}

#[test]
fn metadata_slots_and_checked_window_inventory_fail_before_callbacks() {
    let source = input([row(Value::Int(0), "x", false, 0)]);
    let roots = BasicSourceRoots::try_new(&source, &[], small_policy(4096)).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let mut c = Controls::new();
    c.virtual_windows = Some(1_000_000);
    assert!(matches!(
        parent.map_group_by_source_cohort(&original, &mut c, &mut constant_target),
        Err(RuntimeError::BasicSourceBridge(
            BasicSourceBridgeError::PairedMetadataLimit {
                component: "MetadataSlots",
                ..
            }
        ))
    ));
    assert!(c.calls.borrow().is_empty());
    c.virtual_windows = Some(usize::MAX);
    c.implicit_first = true;
    assert_eq!(
        parent
            .map_group_by_source_cohort(&original, &mut c, &mut constant_target)
            .unwrap_err(),
        phase_overflow()
    );
    assert!(c.calls.borrow().is_empty());
    let mut overflow = small_policy(4096);
    overflow.max_walk_contexts = usize::MAX;
    assert!(group_maximum(overflow).is_err());
}

#[test]
fn authentic_identity_walk_and_legacy_alias_root_keep_the_selected_endpoint() {
    let primary = group([field("Marker", scalar(text("P")))]);
    let named = group([field("Marker", scalar(text("N")))]);
    let declared = [DeclaredNamedSourceRoot {
        name: "Alias",
        instance: Some(&named),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declared, policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent.walk_source_cohort(&["Alias"]).unwrap();
    let mut c = Controls::new();
    c.key = Key::Constant(Value::Int(1));
    parent
        .map_group_by_source_cohort(&original, &mut c, &mut |group: &BasicScopeContext<'_>| {
            assert_eq!(
                group.owned_field(SourceOwnerRef::Named("Alias"), &["Marker"], None)?,
                text("N")
            );
            let members = group.walk_source_cohort(&["Alias"])?;
            assert_eq!(members.len(), 1);
            assert_eq!(
                members.get(0).unwrap().context().owned_field(
                    SourceOwnerRef::Named("Alias"),
                    &["Marker"],
                    None
                )?,
                text("N")
            );
            assert_eq!(
                members
                    .get(0)
                    .unwrap()
                    .context()
                    .owned_position(SourceOwnerRef::Named("Alias"), Some(&[]))?,
                1
            );
            Ok::<_, RuntimeError>(scalar(text("N")))
        })
        .unwrap();
    let source = sample();
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let rows = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let row = rows.get(2).unwrap().context();
    let identity = row
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let output = row
        .map_group_by_source_cohort(
            &identity,
            &mut Controls::new(),
            &mut |g: &BasicScopeContext<'_>| render(g, SourceOwnerRef::Primary, &["Rows"]),
        )
        .unwrap();
    assert_eq!(output, vec![expected("a2", 1, &["a2"])]);
}

#[test]
fn bounds_observe_receiving_parent_not_the_first_candidate() {
    struct ParentBounds(Controls);
    impl BasicGroupByControls<RuntimeError> for ParentBounds {
        fn metadata(&self) -> BasicGroupByMetadata<'_> {
            self.0.metadata()
        }
        fn filter(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
            self.0.filter(c)
        }
        fn sort_key(&mut self, k: usize, c: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
            self.0.sort_key(k, c)
        }
        fn group_key(&mut self, c: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
            self.0.group_key(c)
        }
        fn retain_member(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
            self.0.retain_member(c)
        }
        fn window(
            &mut self,
            _: usize,
            parent: &BasicScopeContext<'_>,
        ) -> Result<SequenceWindow, RuntimeError> {
            self.0.record("bound")?;
            Ok(SequenceWindow::First(
                parent.owned_position(SourceOwnerRef::Primary, None)?,
            ))
        }
    }
    let batch = || {
        group([field(
            "Rows",
            repeated([
                row(text("A"), "a", true, 0),
                row(text("B"), "b", true, 0),
                row(text("C"), "c", true, 0),
            ]),
        )])
    };
    let source = group([field("Batch", repeated([batch(), batch()]))]);
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let root = roots.root_context().unwrap();
    let batches = root
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Batch"])
        .unwrap();
    let parent = batches.get(1).unwrap().context();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Batch", "Rows"])
        .unwrap();
    let mut inner = Controls::new();
    inner.path = &["Batch", "Rows"];
    inner.windows.push(SequenceWindow::First(0));
    let output = parent
        .map_group_by_source_cohort(
            &original,
            &mut ParentBounds(inner),
            &mut |g: &BasicScopeContext<'_>| {
                assert_eq!(
                    g.owned_position(SourceOwnerRef::Primary, Some(&["Batch"]))?,
                    2
                );
                render(g, SourceOwnerRef::Primary, &["Batch", "Rows"])
            },
        )
        .unwrap();
    assert_eq!(
        output,
        vec![expected("a", 1, &["a"]), expected("b", 2, &["b"])]
    );
}

#[test]
fn source_count_and_group_frame_limits_preserve_typed_pre_callback_refusals() {
    let source = sample();
    let roots = BasicSourceRoots::try_new(&source, &[], small_policy(4096)).unwrap();
    let parent = roots.root_context().unwrap();
    assert!(matches!(
        parent.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"]),
        Err(RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit {
            kind: OwnedSourceLimitKind::WalkContexts,
            ..
        }))
    ));
    let source = group(Vec::new());
    let mut limits = small_policy(4096);
    limits.max_active_frames = 0;
    let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &[])
        .unwrap();
    let mut c = Controls::new();
    c.key = Key::Constant(Value::Int(1));
    assert!(matches!(
        parent.map_group_by_source_cohort(&original, &mut c, &mut constant_target),
        Err(RuntimeError::BasicSourceBridge(
            BasicSourceBridgeError::PairedMetadataLimit {
                component: "frame depth",
                ..
            }
        ))
    ));
    assert!(c.calls.borrow().is_empty());
}

#[test]
fn ordinary_phase_helpers_keep_their_preexisting_positions_and_witnesses() {
    let source = sample();
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let mut output = Vec::new();
    parent
        .visit_filtered_source_cohort::<RuntimeError, _, _>(
            &original,
            |c| Ok(c.owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))? > 1),
            |c| {
                output.push((
                    c.owned_field(SourceOwnerRef::Primary, &["Rows", "Label"], None)?,
                    c.owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))?,
                ));
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(
        output,
        vec![
            (text("b1"), 1),
            (text("a2"), 2),
            (text("c1"), 3),
            (text("b2"), 4)
        ]
    );
    assert_eq!(
        parent
            .map_group_by_source_cohort(
                &original,
                &mut Controls::new(),
                &mut |g: &BasicScopeContext<'_>| render(g, SourceOwnerRef::Primary, &["Rows"])
            )
            .unwrap(),
        vec![
            expected("a1", 1, &["a1", "a2"]),
            expected("b1", 2, &["b1", "b2"]),
            expected("c1", 3, &["c1"])
        ]
    );
}

#[test]
fn zero_segment_named_root_alias_is_raw_only_and_nested_members_keep_canonical_empty() {
    let primary = group(Vec::new());
    let named = repeated([
        scalar(Value::Int(1)),
        scalar(Value::Int(1)),
        scalar(Value::Int(2)),
    ]);
    let declared = [DeclaredNamedSourceRoot {
        name: "Alias",
        instance: Some(&named),
    }];
    let mut limits = policy();
    limits.max_path_segments = 0;
    let roots = BasicSourceRoots::try_new(&primary, &declared, limits).unwrap();
    let parent = roots.root_context().unwrap();
    // The extra segment is not granted to an owned source/query.
    assert!(matches!(
        parent.walk_owned_source_cohort(SourceOwnerRef::Named("Alias"), &["Alias"]),
        Err(RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit {
            kind: OwnedSourceLimitKind::PathSegments,
            ..
        }))
    ));
    let original = parent.walk_source_cohort(&["Alias"]).unwrap();
    let mut controls = Controls::new();
    controls.owner = SourceOwnerRef::Named("Alias");
    controls.path = &[];
    controls.key = Key::Current;
    let output = parent
        .map_group_by_source_cohort(
            &original,
            &mut controls,
            &mut |group: &BasicScopeContext<'_>| {
                let legacy = group.walk_source_cohort(&["Alias"])?;
                let owned = group.walk_owned_source_cohort(SourceOwnerRef::Named("Alias"), &[])?;
                assert_eq!(legacy.len(), owned.len());
                let mut values = Vec::new();
                for (index, member) in legacy.iter().enumerate() {
                    assert_eq!(
                        member
                            .context()
                            .owned_position(SourceOwnerRef::Named("Alias"), Some(&[]))?,
                        index + 1
                    );
                    values.push(scalar(member.context().owned_field(
                        SourceOwnerRef::Named("Alias"),
                        &[],
                        None,
                    )?));
                }
                Ok::<_, RuntimeError>(group_instance(group, values))
            },
        )
        .unwrap();
    assert_eq!(
        output,
        vec![
            group([
                field("First", scalar(Value::Int(1))),
                field(
                    "Members",
                    repeated([scalar(Value::Int(1)), scalar(Value::Int(1))])
                )
            ]),
            group([
                field("First", scalar(Value::Int(2))),
                field("Members", repeated([scalar(Value::Int(2))]))
            ]),
        ]
    );

    fn group_instance(context: &BasicScopeContext<'_>, members: Vec<Instance>) -> Instance {
        group([
            field(
                "First",
                scalar(
                    context
                        .owned_field(SourceOwnerRef::Named("Alias"), &[], None)
                        .unwrap(),
                ),
            ),
            field("Members", repeated(members)),
        ])
    }
}

#[test]
fn explicit_immutable_root_pin_does_not_receive_group_representative_permission() {
    let source = sample();
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let output = parent
        .map_group_by_source_cohort(
            &original,
            &mut Controls::new(),
            &mut |g: &BasicScopeContext<'_>| {
                assert!(matches!(
                    g.owned_field_with_root_pin(
                        SourceOwnerRef::Primary,
                        &["Rows", "Key"],
                        Some(&[]),
                        true
                    ),
                    Err(RuntimeError::OwnedSource(
                        OwnedSourceError::UniteratedRepetition { .. }
                    ))
                ));
                Ok::<_, RuntimeError>(scalar(g.owned_field(
                    SourceOwnerRef::Primary,
                    &["Rows", "Label"],
                    None,
                )?))
            },
        )
        .unwrap();
    assert_eq!(
        output,
        vec![scalar(text("a1")), scalar(text("b1")), scalar(text("c1"))]
    );
}

#[test]
fn typed_post_and_target_errors_are_returned_without_a_partial_output() {
    struct BadPost(Controls);
    impl BasicGroupByControls<RuntimeError> for BadPost {
        fn metadata(&self) -> BasicGroupByMetadata<'_> {
            self.0.metadata()
        }
        fn filter(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
            self.0.filter(c)
        }
        fn sort_key(&mut self, k: usize, c: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
            self.0.sort_key(k, c)
        }
        fn window(
            &mut self,
            k: usize,
            c: &BasicScopeContext<'_>,
        ) -> Result<SequenceWindow, RuntimeError> {
            self.0.window(k, c)
        }
        fn group_key(&mut self, c: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
            self.0.group_key(c)
        }
        fn retain_member(&mut self, _: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
            Err(RuntimeError::NotABool {
                node: 816,
                found: "string",
            })
        }
    }
    let source = sample();
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let mut c = Controls::new();
    c.post = true;
    assert_eq!(
        parent
            .map_group_by_source_cohort(&original, &mut BadPost(c), &mut constant_target)
            .unwrap_err(),
        RuntimeError::NotABool {
            node: 816,
            found: "string"
        }
    );
    let mut reached = 0;
    let error = parent
        .map_group_by_source_cohort(
            &original,
            &mut Controls::new(),
            &mut |_: &BasicScopeContext<'_>| {
                reached += 1;
                if reached == 2 {
                    Err(RuntimeError::MappingException {
                        node: 817,
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
            node: 817,
            message: Some("target".into())
        }
    );
    assert_eq!(reached, 2);
}

#[test]
fn unavailable_colliding_owner_keeps_its_original_typed_source_cause() {
    let source = sample();
    let declarations = [DeclaredNamedSourceRoot {
        name: "Rows",
        instance: None,
    }];
    let roots = BasicSourceRoots::try_new(&source, &declarations, policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let error = parent
        .map_group_by_source_cohort(
            &original,
            &mut Controls::new(),
            &mut |group: &BasicScopeContext<'_>| {
                group
                    .owned_field(SourceOwnerRef::Named("Rows"), &["Rows", "Label"], None)
                    .map(scalar)
            },
        )
        .unwrap_err();
    let RuntimeError::OwnedSource(source) = &error else {
        panic!("original owned source error")
    };
    assert_eq!(
        source,
        &OwnedSourceError::SourceUnavailable {
            name: "Rows".into()
        }
    );
    let cause = std::error::Error::source(&error)
        .unwrap()
        .downcast_ref::<OwnedSourceError>()
        .unwrap();
    assert!(std::ptr::eq(source, cause));
}

fn path_limit(requested: usize, maximum: usize) -> RuntimeError {
    OwnedSourceError::MetadataLimit {
        kind: OwnedSourceLimitKind::PathSegments,
        used: 0,
        requested,
        max: maximum,
    }
    .into()
}

#[test]
fn complete_legacy_selector_rejects_unvisited_suffix_before_empty_null_missing_or_bounds() {
    for rows in [
        Some(repeated([])),
        Some(scalar(Value::Null)),
        Some(scalar(Value::json_null())),
        None,
    ] {
        for bytes in [0, 4096] {
            let source = group(rows.clone().into_iter().map(|rows| field("Rows", rows)));
            let mut limits = small_policy(bytes);
            limits.max_walk_contexts = 1;
            let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
            let parent = roots.root_context().unwrap();
            let mut c = Controls::new();
            c.windows.push(SequenceWindow::First(1));
            let result = parent
                .walk_source_cohort(&["Rows", "Extra"])
                .and_then(|original| {
                    parent.map_group_by_source_cohort(&original, &mut c, &mut constant_target)
                });
            assert_eq!(result.unwrap_err(), path_limit(2, 1));
            assert!(
                c.calls.borrow().is_empty(),
                "no group bound may follow an overlong selector"
            );
        }
    }
}

#[test]
fn empty_primary_collection_cannot_hide_overlong_legacy_selector() {
    let source = group([field("Rows", repeated([]))]);
    let mut limits = small_policy(4096);
    limits.max_walk_contexts = 1;
    let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
    let parent = roots.root_context().unwrap();
    let result = parent.walk_source_cohort(&["Rows", "Extra"]);
    assert_eq!(result.err(), Some(path_limit(2, 1)));
}

#[test]
fn complete_canonical_selector_counts_only_actual_alias_and_active_origin_suffix() {
    let primary = group(Vec::new());
    let named = group([field("Rows", repeated([]))]);
    let declarations = [DeclaredNamedSourceRoot {
        name: "Alias",
        instance: Some(&named),
    }];
    let mut alias_limits = small_policy(4096);
    alias_limits.max_roots = 2;
    let roots = BasicSourceRoots::try_new(&primary, &declarations, alias_limits).unwrap();
    let parent = roots.root_context().unwrap();
    assert!(
        parent
            .walk_source_cohort(&["Alias", "Rows"])
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        parent
            .walk_source_cohort(&["Alias", "Rows", "Extra"])
            .err()
            .unwrap(),
        path_limit(2, 1)
    );

    // Spelling a declaration does not consume it when an actual Primary field wins.
    let primary = group([field("Alias", repeated([]))]);
    let roots = BasicSourceRoots::try_new(&primary, &declarations, alias_limits).unwrap();
    let parent = roots.root_context().unwrap();
    assert_eq!(
        parent.walk_source_cohort(&["Alias", "Rows"]).err().unwrap(),
        path_limit(2, 1)
    );

    for children in [Some(repeated([])), Some(scalar(Value::Null)), None] {
        let item = group(children.into_iter().map(|value| field("Children", value)));
        let source = group([field("Outer", repeated([item]))]);
        let mut limits = small_policy(4096);
        limits.max_path_segments = 2;
        let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
        let root = roots.root_context().unwrap();
        let outer = root
            .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Outer"])
            .unwrap();
        assert_eq!(
            outer
                .get(0)
                .unwrap()
                .context()
                .walk_source_cohort(&["Children", "Extra"])
                .err()
                .unwrap(),
            path_limit(3, 2)
        );
    }
}

#[test]
fn complete_selector_uses_authenticated_group_wrapper_redirect_before_empty_suffix() {
    let source = group([field(
        "Outer",
        repeated([group([field(
            "Rows",
            repeated([row(text("A"), "first", false, 0)]),
        )])]),
    )]);
    let mut limits = policy();
    limits.max_path_segments = 2;
    let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Outer", "Rows"])
        .unwrap();
    let mut c = Controls::new();
    c.key = Key::Constant(Value::Int(1));
    let result = parent
        .map_group_by_source_cohort(&original, &mut c, &mut |g: &BasicScopeContext<'_>| {
            assert_eq!(g.walk_source_cohort(&["Rows"])?.len(), 1);
            assert_eq!(
                g.walk_source_cohort(&["Rows", "Extra"]).err().unwrap(),
                path_limit(3, 2)
            );
            Ok::<_, RuntimeError>(scalar(Value::Int(7)))
        })
        .unwrap();
    assert_eq!(result, vec![scalar(Value::Int(7))]);
}

struct ScalarKeysControls {
    directions: Vec<SortDirection>,
    values: Vec<Value>,
    bounds: usize,
    calls: Rc<RefCell<Vec<&'static str>>>,
}
impl ScalarKeysControls {
    fn new(values: Vec<Value>, bounds: usize) -> Self {
        Self {
            directions: vec![SortDirection::Ascending; values.len()],
            values,
            bounds,
            calls: Rc::default(),
        }
    }
}
impl BasicGroupByControls<RuntimeError> for ScalarKeysControls {
    fn metadata(&self) -> BasicGroupByMetadata<'_> {
        BasicGroupByMetadata {
            directions: &self.directions,
            filter_before_sort: false,
            explicit_window_count: self.bounds,
            implicit_first: false,
            has_filter: false,
            has_post_filter: false,
        }
    }
    fn filter(&mut self, _: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        unreachable!()
    }
    fn retain_member(&mut self, _: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        unreachable!()
    }
    fn sort_key(&mut self, key: usize, _: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        self.calls.borrow_mut().push("sort");
        Ok(self.values[key].clone())
    }
    fn window(
        &mut self,
        _: usize,
        _: &BasicScopeContext<'_>,
    ) -> Result<SequenceWindow, RuntimeError> {
        self.calls.borrow_mut().push("bound");
        Ok(SequenceWindow::First(1))
    }
    fn group_key(&mut self, _: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        self.calls.borrow_mut().push("key");
        Ok(Value::Int(0))
    }
}
fn replace_original_backing(original: &mut RetainedSourceCohort<'_>, capacity: usize) {
    let mut backing = Vec::with_capacity(capacity);
    backing.append(&mut original.candidates);
    original.candidates = backing;
    assert_eq!(original.candidates.capacity(), capacity);
}

#[test]
fn exact_slot_frontier_and_one_extra_retained_cell_have_independent_numeric_oracles() {
    let source = group(Vec::new());
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
    let mut controls = ScalarKeysControls::new(vec![Value::Int(0), Value::Int(0)], 48);

    // Audited finite fixture: seed38; fixed/template/selection26; 48 three-cell
    // windows144. Total210 is exact, not a value copied from the helper return.
    let prepared = prepare::<RuntimeError, _>(&parent, &original, &controls).unwrap();
    assert_eq!((prepared.budget.slots, prepared.budget.maximum), (210, 210));
    assert_eq!(prepared.directions.capacity(), 2);
    assert_eq!(prepared.cache.capacity(), 2);
    assert_eq!(prepared.windows.capacity(), 48);
    assert_eq!(
        (
            prepared.order.capacity(),
            prepared.scratch.capacity(),
            prepared.next.capacity()
        ),
        (1, 1, 1)
    );
    assert_eq!(
        (
            prepared.buckets.capacity(),
            prepared.keys.capacity(),
            prepared.output.capacity()
        ),
        (1, 1, 1)
    );
    assert_eq!(
        (
            prepared.plans.capacity(),
            prepared.templates.capacity(),
            prepared.selection.capacity()
        ),
        (1, 1, 1)
    );
    let template = prepared.templates[0].as_ref().unwrap();
    assert_eq!(
        (
            template.prefix.owned.frames.capacity(),
            template.prefix.legacy.frames.capacity(),
            template.prefix.origins.capacity()
        ),
        (1, 2, 2)
    );
    assert!(controls.calls.borrow().is_empty());
    drop(prepared);
    assert_eq!(
        parent
            .map_group_by_source_cohort(&original, &mut controls, &mut constant_target)
            .unwrap(),
        vec![scalar(Value::Int(7))]
    );
    assert_eq!(
        controls
            .calls
            .borrow()
            .iter()
            .filter(|phase| **phase == "bound")
            .count(),
        48
    );

    controls.calls.borrow_mut().clear();
    replace_original_backing(&mut original, 5);
    assert_eq!(
        parent
            .map_group_by_source_cohort(&original, &mut controls, &mut constant_target)
            .unwrap_err(),
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataSlots",
            used: 39,
            requested: 172,
            maximum: 210
        }
        .into()
    );
    assert!(controls.calls.borrow().is_empty());
}

#[test]
fn simultaneous_slot_and_known_utf8_refusal_keeps_slots_first_at_exact_plus_one() {
    let source = input([row(Value::Int(0), "x", false, 0)]);
    let mut limits = small_policy(31);
    limits.max_walk_contexts = 1;
    let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
    let parent = roots.root_context().unwrap();
    let mut original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let mut controls = ScalarKeysControls::new(Vec::new(), 118);
    replace_original_backing(&mut original, 4);
    // At425 slots exactly, the independent32-byte known path snapshot refuses31.
    assert_eq!(
        parent
            .map_group_by_source_cohort(&original, &mut controls, &mut constant_target)
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
    // Only one retained capacity cell changed; slots426 now wins over known bytes.
    assert_eq!(
        parent
            .map_group_by_source_cohort(&original, &mut controls, &mut constant_target)
            .unwrap_err(),
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataSlots",
            used: 48,
            requested: 378,
            maximum: 425
        }
        .into()
    );
    assert!(controls.calls.borrow().is_empty());
}

#[test]
fn returned_sort_key_utf8_has_exact_cumulative_frontier_before_bounds_or_group_keys() {
    let source = group(Vec::new());
    for (bytes, succeeds) in [(3, false), (4, true)] {
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
        let mut controls = ScalarKeysControls::new(vec![text("é"), text("é")], 1);
        let result =
            parent.map_group_by_source_cohort(&original, &mut controls, &mut constant_target);
        if succeeds {
            assert_eq!(result.unwrap(), vec![scalar(Value::Int(7))]);
            assert_eq!(&*controls.calls.borrow(), &["sort", "sort", "bound", "key"]);
        } else {
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
            assert_eq!(&*controls.calls.borrow(), &["sort", "sort"]);
        }
    }
}

#[test]
fn grouped_multikey_stability_uses_native_nan_and_signed_zero_sort_equivalence() {
    struct MultiKeys {
        calls: Vec<(usize, usize)>,
    }
    impl BasicGroupByControls<RuntimeError> for MultiKeys {
        fn metadata(&self) -> BasicGroupByMetadata<'_> {
            BasicGroupByMetadata {
                directions: &[SortDirection::Ascending, SortDirection::Descending],
                filter_before_sort: false,
                explicit_window_count: 0,
                implicit_first: false,
                has_filter: false,
                has_post_filter: false,
            }
        }
        fn filter(&mut self, _: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
            unreachable!()
        }
        fn retain_member(&mut self, _: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
            unreachable!()
        }
        fn window(
            &mut self,
            _: usize,
            _: &BasicScopeContext<'_>,
        ) -> Result<SequenceWindow, RuntimeError> {
            unreachable!()
        }
        fn sort_key(
            &mut self,
            key: usize,
            c: &BasicScopeContext<'_>,
        ) -> Result<Value, RuntimeError> {
            self.calls.push((
                c.owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))?,
                key,
            ));
            c.owned_field(
                SourceOwnerRef::Primary,
                &["Rows", if key == 0 { "Primary" } else { "Secondary" }],
                None,
            )
        }
        fn group_key(&mut self, _: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
            Ok(Value::Int(0))
        }
    }
    let sort_row = |primary, secondary, label: &str| {
        group([
            field("Primary", scalar(Value::Float(primary))),
            field("Secondary", scalar(Value::Int(secondary))),
            field("Label", scalar(text(label))),
        ])
    };
    let source = input([
        sort_row(f64::NAN, 1, "nan-first"),
        sort_row(-0.0, 2, "zero-first"),
        sort_row(3.0, 1, "three-first"),
        sort_row(0.0, 2, "zero-second"),
        sort_row(f64::NAN, 3, "nan-high"),
        sort_row(-0.0, 1, "zero-low"),
        sort_row(3.0, 1, "three-second"),
        sort_row(f64::NAN, 1, "nan-second"),
    ]);
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let mut controls = MultiKeys { calls: Vec::new() };
    let output = parent
        .map_group_by_source_cohort(&original, &mut controls, &mut |g: &BasicScopeContext<
            '_,
        >| {
            render(g, SourceOwnerRef::Primary, &["Rows"])
        })
        .unwrap();
    assert_eq!(
        output,
        vec![expected(
            "zero-first",
            1,
            &[
                "zero-first",
                "zero-second",
                "zero-low",
                "three-first",
                "three-second",
                "nan-high",
                "nan-first",
                "nan-second",
            ]
        )]
    );
    assert_eq!(
        controls.calls,
        (1..=8)
            .flat_map(|position| [(position, 0), (position, 1)])
            .collect::<Vec<_>>()
    );
}

#[test]
fn baseline_nonalias_raw_limit_keeps_owned_envelope_and_raw_count_under_active_origin() {
    let named = group(Vec::new());
    let declarations = [DeclaredNamedSourceRoot {
        name: "子",
        instance: Some(&named),
    }];
    for primary_wins in [true, false] {
        let child = group(primary_wins.then(|| field("子", repeated([]))));
        let source = group([field("外", repeated([child]))]);
        let mut limits = small_policy(4096);
        limits.max_roots = 2;
        limits.max_path_segments = 2;
        let roots = BasicSourceRoots::try_new(&source, &declarations, limits).unwrap();
        let root = roots.root_context().unwrap();
        let outer = root
            .walk_owned_source_cohort(SourceOwnerRef::Primary, &["外"])
            .unwrap();
        let result = outer
            .get(0)
            .unwrap()
            .context()
            .walk_source_cohort(&["子", "élève", "终"]);
        if primary_wins {
            // 74f19207 OwnedQuery rejected the raw three segments at P2 before
            // adding any active canonical prefix; a declaration spelling is not consumption.
            assert_eq!(result.err(), Some(path_limit(3, 2)));
        } else {
            // Actual named binding consumption removes one raw segment; the two
            // canonical tail segments fit even though the named payload is empty.
            assert!(result.unwrap().is_empty());
        }
    }
}

#[test]
fn baseline_unicode_query_length_and_utf8_precedence_keep_original_owned_errors() {
    let source = group([field("子", repeated([]))]);
    for path_segments in [1, 2] {
        let mut limits = small_policy(0);
        limits.max_path_segments = path_segments;
        let roots = BasicSourceRoots::try_new(&source, &[], limits).unwrap();
        let parent = roots.root_context().unwrap();
        let error = parent.walk_source_cohort(&["子", "终"]).err().unwrap();
        if path_segments == 1 {
            assert_eq!(error, path_limit(2, 1));
        } else {
            // First query segment is three UTF8 bytes, not one character. The
            // original query ledger is reached only after segment admission.
            assert_eq!(
                error,
                RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit {
                    kind: OwnedSourceLimitKind::MetadataBytes,
                    used: 0,
                    requested: 3,
                    max: 0,
                })
            );
        }
    }
}

#[test]
fn g11_alias_root_pin_keeps_registered_repetition_and_original_owned_cause() {
    let primary = group([field("Marker", scalar(text("unused.primary")))]);
    let named = repeated([
        row(text("A"), "a1", false, 0),
        row(text("B"), "b1", false, 0),
        row(text("A"), "a2", false, 0),
    ]);
    let declarations = [DeclaredNamedSourceRoot {
        name: "Alias",
        instance: Some(&named),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declarations, policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent.walk_source_cohort(&["Alias"]).unwrap();
    let mut controls = Controls::new();
    controls.owner = SourceOwnerRef::Named("Alias");
    controls.path = &[];
    let error = parent
        .map_group_by_source_cohort(&original, &mut controls, &mut |g: &BasicScopeContext<
            '_,
        >| {
            g.owned_field_with_root_pin(SourceOwnerRef::Named("Alias"), &["Key"], Some(&[]), false)
                .map(scalar)
        })
        .unwrap_err();
    assert_eq!(
        error,
        RuntimeError::OwnedSource(OwnedSourceError::UniteratedRepetition {
            owner: OwnedSourceIdentity::Named("Alias".into()),
            path: vec!["Key".into()],
            segment: 0,
        })
    );
    let RuntimeError::OwnedSource(source) = &error else {
        panic!("original owned source error")
    };
    let cause = std::error::Error::source(&error)
        .unwrap()
        .downcast_ref::<OwnedSourceError>()
        .unwrap();
    assert!(std::ptr::eq(source, cause));
}

#[test]
fn root_by_group_empty_pin_uses_exact_owner_for_primary_named_and_legacy_alias() {
    let primary = repeated([
        row(text("A"), "p1", false, 0),
        row(text("B"), "p2", false, 0),
        row(text("A"), "p3", false, 0),
    ]);
    let named = repeated([
        row(text("A"), "n1", false, 0),
        row(text("B"), "n2", false, 0),
        row(text("A"), "n3", false, 0),
    ]);
    let declarations = [DeclaredNamedSourceRoot {
        name: "Alias",
        instance: Some(&named),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declarations, policy()).unwrap();
    let parent = roots.root_context().unwrap();
    for (owner, identity, legacy_alias, expected) in [
        (
            SourceOwnerRef::Primary,
            OwnedSourceIdentity::Primary,
            false,
            vec![(text("p1"), 1, 1), (text("p2"), 2, 2)],
        ),
        (
            SourceOwnerRef::Named("Alias"),
            OwnedSourceIdentity::Named("Alias".into()),
            false,
            vec![(text("n1"), 1, 1), (text("n2"), 2, 2)],
        ),
        (
            SourceOwnerRef::Named("Alias"),
            OwnedSourceIdentity::Named("Alias".into()),
            true,
            vec![(text("n1"), 1, 1), (text("n2"), 2, 2)],
        ),
    ] {
        let original = if legacy_alias {
            parent.walk_source_cohort(&["Alias"]).unwrap()
        } else {
            parent.walk_owned_source_cohort(owner, &[]).unwrap()
        };
        let mut controls = Controls::new();
        controls.owner = owner;
        controls.path = &[];
        let mut observed = Vec::new();
        let output = parent
            .map_group_by_source_cohort(&original, &mut controls, &mut |g: &BasicScopeContext<
                '_,
            >| {
                observed.push((
                    g.owned_field(owner, &["Label"], None)?,
                    g.owned_position(owner, Some(&[]))?,
                    g.owned_position(owner, None)?,
                ));
                assert_eq!(
                    g.owned_field(owner, &["Key"], Some(&[])),
                    Err(RuntimeError::OwnedSource(
                        OwnedSourceError::UniteratedRepetition {
                            owner: identity.clone(),
                            path: vec!["Key".into()],
                            segment: 0,
                        }
                    ))
                );
                for declared_nonrepeating in [false, true] {
                    assert_eq!(
                        g.owned_field_with_root_pin(
                            owner,
                            &["Key"],
                            Some(&[]),
                            declared_nonrepeating,
                        ),
                        Err(RuntimeError::OwnedSource(
                            OwnedSourceError::UniteratedRepetition {
                                owner: identity.clone(),
                                path: vec!["Key".into()],
                                segment: 0,
                            }
                        ))
                    );
                }
                constant_target(g)
            })
            .unwrap();
        assert_eq!(observed, expected);
        assert_eq!(output, vec![scalar(Value::Int(7)), scalar(Value::Int(7))]);
    }
}

#[test]
fn ordinary_empty_member_pin_uses_latest_item_even_with_older_root_by_group() {
    let primary = repeated([
        row(text("A"), "p1", false, 0),
        row(text("B"), "p2", false, 0),
        row(text("A"), "p3", false, 0),
    ]);
    let named = repeated([
        row(text("A"), "n1", false, 0),
        row(text("B"), "n2", false, 0),
        row(text("A"), "n3", false, 0),
    ]);
    let declarations = [DeclaredNamedSourceRoot {
        name: "Alias",
        instance: Some(&named),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declarations, policy()).unwrap();
    let parent = roots.root_context().unwrap();
    for (owner, legacy_alias, expected) in [
        (
            SourceOwnerRef::Primary,
            false,
            vec![(text("p1"), 1), (text("p2"), 2), (text("p3"), 3)],
        ),
        (
            SourceOwnerRef::Named("Alias"),
            false,
            vec![(text("n1"), 1), (text("n2"), 2), (text("n3"), 3)],
        ),
        (
            SourceOwnerRef::Named("Alias"),
            true,
            vec![(text("n1"), 1), (text("n2"), 2), (text("n3"), 3)],
        ),
    ] {
        let original = if legacy_alias {
            parent.walk_source_cohort(&["Alias"]).unwrap()
        } else {
            parent.walk_owned_source_cohort(owner, &[]).unwrap()
        };
        let mut controls = Controls::new();
        controls.key = Key::Constant(Value::Int(1));
        let mut observed = Vec::new();
        let output = parent
            .map_group_by_source_cohort(&original, &mut controls, &mut |g: &BasicScopeContext<
                '_,
            >| {
                assert_eq!(g.owned_position(owner, Some(&[]))?, 1);
                let members = g.walk_owned_source_cohort(owner, &[])?;
                assert_eq!(members.len(), 3);
                for member in members.iter() {
                    let c = member.context();
                    // Establish that this is the newest Item over a retained
                    // GroupMembers at the very same empty collection, not a fresh root.
                    let frames = &c.owned.frames;
                    assert_eq!(frames.len(), 2);
                    assert!(frames[0].collection.is_empty());
                    assert!(frames[0].kind == OwnedOccurrenceKind::GroupMembers);
                    assert!(frames[1].collection.is_empty());
                    assert!(frames[1].kind == OwnedOccurrenceKind::Item);
                    let value = c.owned_field(owner, &["Label"], Some(&[]))?;
                    assert_eq!(c.owned_field(owner, &["Label"], None)?, value);
                    for declared_nonrepeating in [false, true] {
                        assert_eq!(
                            c.owned_field_with_root_pin(
                                owner,
                                &["Label"],
                                Some(&[]),
                                declared_nonrepeating,
                            )?,
                            value
                        );
                    }
                    observed.push((value, c.owned_position(owner, Some(&[]))?));
                }
                constant_target(g)
            })
            .unwrap();
        assert_eq!(observed, expected);
        assert_eq!(output, vec![scalar(Value::Int(7))]);
    }
}

#[test]
fn root_by_group_empty_pin_never_rescues_missing_owner_or_inactive_frame() {
    let primary = repeated([row(text("A"), "p1", false, 0)]);
    let idle = repeated([row(text("A"), "idle", false, 0)]);
    let declarations = [
        DeclaredNamedSourceRoot {
            name: "Idle",
            instance: Some(&idle),
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
    let mut controls = Controls::new();
    controls.key = Key::Constant(Value::Int(1));
    let output = parent
        .map_group_by_source_cohort(&original, &mut controls, &mut |g: &BasicScopeContext<
            '_,
        >| {
            for declared_nonrepeating in [false, true] {
                assert_eq!(
                    g.owned_field_with_root_pin(
                        SourceOwnerRef::Named("Missing"),
                        &["Label"],
                        Some(&[]),
                        declared_nonrepeating,
                    ),
                    Err(RuntimeError::OwnedSource(OwnedSourceError::UnknownOwner {
                        name: "Missing".into(),
                    }))
                );
                assert_eq!(
                    g.owned_field_with_root_pin(
                        SourceOwnerRef::Named("Absent"),
                        &["Label"],
                        Some(&[]),
                        declared_nonrepeating,
                    ),
                    Err(RuntimeError::OwnedSource(
                        OwnedSourceError::SourceUnavailable {
                            name: "Absent".into(),
                        }
                    ))
                );
                assert_eq!(
                    g.owned_field_with_root_pin(
                        SourceOwnerRef::Named("Idle"),
                        &["Label"],
                        Some(&[]),
                        declared_nonrepeating,
                    ),
                    Err(RuntimeError::OwnedSource(
                        OwnedSourceError::InactiveSourceFrame {
                            owner: OwnedSourceIdentity::Named("Idle".into()),
                            collection: Some(Vec::new()),
                        }
                    ))
                );
                assert_eq!(
                    g.owned_field_with_root_pin(
                        SourceOwnerRef::Primary,
                        &["Other", "Label"],
                        Some(&["Other"]),
                        declared_nonrepeating,
                    ),
                    Err(RuntimeError::OwnedSource(
                        OwnedSourceError::InactiveSourceFrame {
                            owner: OwnedSourceIdentity::Primary,
                            collection: Some(vec!["Other".into()]),
                        }
                    ))
                );
            }
            constant_target(g)
        })
        .unwrap();
    assert_eq!(output, vec![scalar(Value::Int(7))]);
}

#[test]
fn nonempty_by_group_empty_pin_keeps_existing_declared_nonrepeating_root_rule() {
    let primary = group([
        field("Marker", scalar(text("P.root"))),
        field("Rows", repeated([row(text("A"), "p1", false, 0)])),
    ]);
    let named = group([
        field("Marker", scalar(text("N.root"))),
        field("Rows", repeated([row(text("A"), "n1", false, 0)])),
    ]);
    let declarations = [DeclaredNamedSourceRoot {
        name: "Alias",
        instance: Some(&named),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declarations, policy()).unwrap();
    let parent = roots.root_context().unwrap();
    for (owner, identity, marker, label) in [
        (
            SourceOwnerRef::Primary,
            OwnedSourceIdentity::Primary,
            "P.root",
            "p1",
        ),
        (
            SourceOwnerRef::Named("Alias"),
            OwnedSourceIdentity::Named("Alias".into()),
            "N.root",
            "n1",
        ),
    ] {
        let original = parent.walk_owned_source_cohort(owner, &["Rows"]).unwrap();
        let mut controls = Controls::new();
        controls.key = Key::Constant(Value::Int(1));
        let output = parent
            .map_group_by_source_cohort(&original, &mut controls, &mut |g: &BasicScopeContext<
                '_,
            >| {
                assert_eq!(
                    g.owned_field_with_root_pin(owner, &["Rows", "Label"], Some(&[]), false),
                    Err(RuntimeError::OwnedSource(
                        OwnedSourceError::InactiveSourceFrame {
                            owner: identity.clone(),
                            collection: Some(Vec::new()),
                        }
                    ))
                );
                assert_eq!(
                    g.owned_field_with_root_pin(owner, &["Rows", "Label"], Some(&[]), true),
                    Err(RuntimeError::OwnedSource(
                        OwnedSourceError::UniteratedRepetition {
                            owner: identity.clone(),
                            path: vec!["Rows".into(), "Label".into()],
                            segment: 1,
                        }
                    ))
                );
                assert_eq!(
                    g.owned_field_with_root_pin(owner, &["Marker"], Some(&[]), true)?,
                    text(marker)
                );
                assert_eq!(g.owned_field(owner, &["Rows", "Label"], None)?, text(label));
                assert_eq!(
                    g.owned_field(owner, &["Rows", "Label"], Some(&["Rows"]))?,
                    text(label)
                );
                constant_target(g)
            })
            .unwrap();
        assert_eq!(output, vec![scalar(Value::Int(7))]);
    }
}

#[path = "basic_group_adjacent_by_tests.rs"]
mod basic_group_adjacent_by;

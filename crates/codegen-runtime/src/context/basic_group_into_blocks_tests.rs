//! Literal paired IntoBlocks controls. Expected products and callback logs are
//! written out by hand; shared By helpers only build fields and render groups.
use super::*;
use std::num::NonZeroUsize;

struct BlocksProbe {
    base: Controls,
    log: Rc<RefCell<Vec<String>>>,
    selected: Option<&'static [usize]>,
    size: usize,
    post_ids: Option<&'static [usize]>,
    fail: Option<(&'static str, usize)>,
}
impl BlocksProbe {
    fn new(size: usize) -> Self {
        let mut base = Controls::new();
        base.post = true;
        Self {
            base,
            log: Rc::default(),
            selected: None,
            size,
            post_ids: None,
            fail: None,
        }
    }
    fn id(&self, c: &BasicScopeContext<'_>) -> Result<usize, RuntimeError> {
        let Value::Int(id) = self.base.value(c, "Rank")? else {
            panic!("literal Rank")
        };
        Ok(id as usize)
    }
    fn record(
        &self,
        phase: &'static str,
        suffix: impl std::fmt::Display,
    ) -> Result<(), RuntimeError> {
        let mut events = self.log.borrow_mut();
        events.push(format!("{phase}{suffix}"));
        if self.fail
            == Some((
                phase,
                events.iter().filter(|s| s.starts_with(phase)).count(),
            ))
        {
            let (node, message) = match phase {
                "z" => (103, "size"),
                "f" => (102, "filter"),
                "w" => (104, "bound"),
                "p" => (105, "post"),
                _ => unreachable!("closed fixture phase"),
            };
            return Err(RuntimeError::MappingException {
                node,
                message: Some(message.into()),
            });
        }
        Ok(())
    }
    fn evidence(&self, events: &[&str]) {
        assert_eq!(&*self.log.borrow(), events);
    }
}
impl BasicGroupIntoBlocksControls<RuntimeError> for BlocksProbe {
    fn metadata(&self) -> BasicGroupByMetadata<'_> {
        self.base.metadata()
    }
    fn filter(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        let id = self.id(c)?;
        self.record("f", id)?;
        Ok(self.selected.is_none_or(|ids| ids.contains(&id)))
    }
    fn sort_key(&mut self, key: usize, c: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        let id = self.id(c)?;
        self.record("s", format_args!("{id}{key}"))?;
        // Descending ids reverse the structural order.
        Ok(Value::Int(-(id as i64)))
    }
    fn window(
        &mut self,
        bound: usize,
        parent: &BasicScopeContext<'_>,
    ) -> Result<SequenceWindow, RuntimeError> {
        // Bounds receive the receiving parent, never a candidate.
        assert!(parent.owned.frames.is_empty());
        self.record("w", bound)?;
        Ok(self.base.windows[bound])
    }
    fn block_size(&mut self, parent: &BasicScopeContext<'_>) -> Result<NonZeroUsize, RuntimeError> {
        // The size also receives the receiving parent.
        assert!(parent.owned.frames.is_empty());
        self.record("z", "")?;
        Ok(NonZeroUsize::new(self.size).expect("literal positive size"))
    }
    fn retain_member(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        let id = self.id(c)?;
        self.record("p", id)?;
        Ok(self.post_ids.is_none_or(|ids| ids.contains(&id)))
    }
}

fn six() -> Instance {
    input((0..6).map(|id| row(text("unused"), &format!("id{id}"), true, id)))
}
fn block_rows(source: &Instance, p: &mut BlocksProbe) -> Result<Vec<Instance>, RuntimeError> {
    let roots = BasicSourceRoots::try_new(source, &[], policy())?;
    let parent = roots.root_context()?;
    let original = parent.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])?;
    let log = Rc::clone(&p.log);
    parent.map_group_into_blocks_source_cohort(&original, p, &mut |g: &BasicScopeContext<'_>| {
        log.borrow_mut().push(format!(
            "t{}",
            g.owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))?
        ));
        render(g, SourceOwnerRef::Primary, &["Rows"])
    })
}
fn exception(node: u32, message: &str) -> RuntimeError {
    RuntimeError::MappingException {
        node,
        message: Some(message.into()),
    }
}

#[test]
fn empty_cohort_runs_bounds_then_size_but_no_target() {
    let mut p = BlocksProbe::new(2);
    p.base.windows = vec![SequenceWindow::First(2)];
    assert_eq!(block_rows(&input([]), &mut p), Ok(vec![]));
    p.evidence(&["w0", "z"]);
}

#[test]
fn selected_candidates_fill_blocks_in_order_and_the_last_may_be_partial() {
    let mut p = BlocksProbe::new(2);
    assert_eq!(
        block_rows(&six(), &mut p),
        Ok(vec![
            expected("id0", 1, &["id0", "id1"]),
            expected("id2", 2, &["id2", "id3"]),
            expected("id4", 3, &["id4", "id5"]),
        ])
    );
    p.evidence(&["z", "p0", "p1", "p2", "p3", "p4", "p5", "t1", "t2", "t3"]);
    let mut p = BlocksProbe::new(4);
    assert_eq!(
        block_rows(&six(), &mut p),
        Ok(vec![
            expected("id0", 1, &["id0", "id1", "id2", "id3"]),
            expected("id4", 2, &["id4", "id5"]),
        ])
    );
    let mut p = BlocksProbe::new(10);
    assert_eq!(
        block_rows(&six(), &mut p),
        Ok(vec![expected(
            "id0",
            1,
            &["id0", "id1", "id2", "id3", "id4", "id5"]
        )])
    );
    let mut p = BlocksProbe::new(1);
    assert_eq!(block_rows(&six(), &mut p).map(|groups| groups.len()), Ok(6));
}

#[test]
fn post_retention_drops_whole_blocks_without_refilling_neighbors() {
    let mut p = BlocksProbe::new(2);
    p.post_ids = Some(&[0, 4]);
    assert_eq!(
        block_rows(&six(), &mut p),
        Ok(vec![
            expected("id0", 1, &["id0", "id1"]),
            expected("id4", 2, &["id4", "id5"]),
        ])
    );
}

#[test]
fn selection_filter_sort_and_parent_windows_keep_phase_order() {
    let mut p = BlocksProbe::new(2);
    p.base.filter = true;
    p.selected = Some(&[0, 2, 3, 5]);
    assert_eq!(
        block_rows(&six(), &mut p),
        Ok(vec![
            expected("id0", 1, &["id0", "id2"]),
            expected("id3", 2, &["id3", "id5"]),
        ])
    );
    p.evidence(&[
        "z", "f0", "p0", "f1", "f2", "p2", "f3", "p3", "f4", "f5", "p5", "t1", "t2",
    ]);
    // Sorted candidates (5..0) chunk in sorted order; bounds and then the size
    // run after sort keys, and windows select retained blocks afterward.
    let mut p = BlocksProbe::new(4);
    p.base.directions = vec![SortDirection::Ascending];
    p.base.windows = vec![SequenceWindow::Last(1)];
    assert_eq!(
        block_rows(&six(), &mut p),
        Ok(vec![expected("id1", 1, &["id1", "id0"])])
    );
    p.evidence(&[
        "s00", "s10", "s20", "s30", "s40", "s50", "w0", "z", "p5", "p4", "p3", "p2", "p1", "p0",
        "t1",
    ]);
}

#[test]
fn bound_then_size_failures_precede_candidate_controls() {
    let mut p = BlocksProbe::new(2);
    p.base.filter = true;
    p.base.windows = vec![SequenceWindow::First(1)];
    p.fail = Some(("w", 1));
    assert_eq!(block_rows(&six(), &mut p), Err(exception(104, "bound")));
    p.evidence(&["w0"]);
    let mut p = BlocksProbe::new(2);
    p.base.filter = true;
    p.base.windows = vec![SequenceWindow::First(1)];
    p.fail = Some(("z", 1));
    assert_eq!(block_rows(&six(), &mut p), Err(exception(103, "size")));
    p.evidence(&["w0", "z"]);
    let mut p = BlocksProbe::new(2);
    p.fail = Some(("p", 3));
    assert_eq!(block_rows(&six(), &mut p), Err(exception(105, "post")));
    p.evidence(&["z", "p0", "p1", "p2"]);
}

#[test]
fn block_sizes_leave_reserved_keys_null_and_charge_no_key_text() {
    let source = six();
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let mut p = BlocksProbe::new(3);
    let (output, capacities, null_keys, charged) =
        blocks_run_snapshot_for_test(&parent, &original, &mut p, &mut |g: &BasicScopeContext<
            '_,
        >| {
            render(g, SourceOwnerRef::Primary, &["Rows"])
        })
        .unwrap();
    assert_eq!(
        output,
        vec![
            expected("id0", 1, &["id0", "id1", "id2"]),
            expected("id3", 2, &["id3", "id4", "id5"]),
        ]
    );
    // n = 6 candidates, no sort keys or windows: the shared prepaid layout.
    assert_eq!(capacities, [0, 0, 0, 6, 0, 6, 6, 6, 6, 6, 6, 0]);
    assert!(null_keys);
    assert_eq!(charged, 0);
}

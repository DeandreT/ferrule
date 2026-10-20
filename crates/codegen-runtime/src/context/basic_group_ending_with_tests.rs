//! Literal paired EndingWith controls. Expected products and callback logs are
//! written out by hand; shared By helpers only build fields and render groups.
use super::*;

struct EndingProbe {
    base: Controls,
    log: Rc<RefCell<Vec<String>>>,
    selected: Option<&'static [usize]>,
    ends: &'static [usize],
    post_ids: Option<&'static [usize]>,
    fail: Option<(&'static str, usize)>,
}
impl EndingProbe {
    fn new(ends: &'static [usize]) -> Self {
        let mut base = Controls::new();
        base.post = true;
        Self {
            base,
            log: Rc::default(),
            selected: None,
            ends,
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
                "b" => (101, "boundary"),
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
impl BasicGroupEndingWithControls<RuntimeError> for EndingProbe {
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
    fn ends_group(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        let id = self.id(c)?;
        self.record("b", id)?;
        Ok(self.ends.contains(&id))
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
fn ending_rows(source: &Instance, p: &mut EndingProbe) -> Result<Vec<Instance>, RuntimeError> {
    let roots = BasicSourceRoots::try_new(source, &[], policy())?;
    let parent = roots.root_context()?;
    let original = parent.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])?;
    let log = Rc::clone(&p.log);
    parent.map_group_ending_with_source_cohort(&original, p, &mut |g: &BasicScopeContext<'_>| {
        log.borrow_mut().push(format!(
            "t{}",
            g.owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))?
        ));
        render(g, SourceOwnerRef::Primary, &["Rows"])
    })
}

#[test]
fn empty_cohort_runs_bounds_but_no_boundary_or_target() {
    let mut p = EndingProbe::new(&[0]);
    p.base.windows = vec![SequenceWindow::First(2)];
    assert_eq!(ending_rows(&input([]), &mut p), Ok(vec![]));
    p.evidence(&["w0"]);
}

#[test]
fn true_boundaries_close_groups_and_the_false_suffix_trails() {
    let mut p = EndingProbe::new(&[1, 3]);
    assert_eq!(
        ending_rows(&six(), &mut p),
        Ok(vec![
            expected("id0", 1, &["id0", "id1"]),
            expected("id2", 2, &["id2", "id3"]),
            expected("id4", 3, &["id4", "id5"]),
        ])
    );
    p.evidence(&[
        "b0", "p0", "b1", "p1", "b2", "p2", "b3", "p3", "b4", "p4", "b5", "p5", "t1", "t2", "t3",
    ]);
    // A true last boundary closes the final group without adding an empty one.
    let mut p = EndingProbe::new(&[1, 5]);
    assert_eq!(
        ending_rows(&six(), &mut p),
        Ok(vec![
            expected("id0", 1, &["id0", "id1"]),
            expected("id2", 2, &["id2", "id3", "id4", "id5"]),
        ])
    );
    let mut p = EndingProbe::new(&[]);
    assert_eq!(
        ending_rows(&six(), &mut p),
        Ok(vec![expected(
            "id0",
            1,
            &["id0", "id1", "id2", "id3", "id4", "id5"]
        )])
    );
    let mut p = EndingProbe::new(&[0, 1, 2, 3, 4, 5]);
    assert_eq!(
        ending_rows(&six(), &mut p).map(|groups| groups.len()),
        Ok(6)
    );
}

#[test]
fn post_retention_drops_whole_groups_without_joining_neighbors() {
    let mut p = EndingProbe::new(&[1, 3]);
    p.post_ids = Some(&[0, 4]);
    assert_eq!(
        ending_rows(&six(), &mut p),
        Ok(vec![
            expected("id0", 1, &["id0", "id1"]),
            expected("id4", 2, &["id4", "id5"]),
        ])
    );
}

#[test]
fn selection_filter_sort_and_parent_windows_keep_phase_order() {
    let mut p = EndingProbe::new(&[2]);
    p.base.filter = true;
    p.selected = Some(&[0, 2, 3, 5]);
    assert_eq!(
        ending_rows(&six(), &mut p),
        Ok(vec![
            expected("id0", 1, &["id0", "id2"]),
            expected("id3", 2, &["id3", "id5"]),
        ])
    );
    p.evidence(&[
        "f0", "b0", "p0", "f1", "f2", "b2", "p2", "f3", "b3", "p3", "f4", "f5", "b5", "p5", "t1",
        "t2",
    ]);
    // Sorted candidates (5..0) partition in sorted order; bounds run after sort
    // keys and before every boundary, and select retained groups afterward.
    let mut p = EndingProbe::new(&[4, 1]);
    p.base.directions = vec![SortDirection::Ascending];
    p.base.windows = vec![SequenceWindow::Last(2)];
    assert_eq!(
        ending_rows(&six(), &mut p),
        Ok(vec![
            expected("id3", 1, &["id3", "id2", "id1"]),
            expected("id0", 2, &["id0"]),
        ])
    );
    p.evidence(&[
        "s00", "s10", "s20", "s30", "s40", "s50", "w0", "b5", "p5", "b4", "p4", "b3", "p3", "b2",
        "p2", "b1", "p1", "b0", "p0", "t1", "t2",
    ]);
}

#[test]
fn boundary_precedes_post_and_the_last_boundary_is_still_evaluated() {
    let mut p = EndingProbe::new(&[1]);
    p.fail = Some(("b", 1));
    assert_eq!(
        ending_rows(&six(), &mut p),
        Err(RuntimeError::MappingException {
            node: 101,
            message: Some("boundary".into())
        })
    );
    p.evidence(&["b0"]);
    let mut p = EndingProbe::new(&[1]);
    p.fail = Some(("p", 1));
    assert_eq!(
        ending_rows(&six(), &mut p),
        Err(RuntimeError::MappingException {
            node: 105,
            message: Some("post".into())
        })
    );
    p.evidence(&["b0", "p0"]);
    let mut p = EndingProbe::new(&[1]);
    p.fail = Some(("b", 6));
    assert_eq!(
        ending_rows(&six(), &mut p),
        Err(RuntimeError::MappingException {
            node: 101,
            message: Some("boundary".into())
        })
    );
    p.evidence(&[
        "b0", "p0", "b1", "p1", "b2", "p2", "b3", "p3", "b4", "p4", "b5",
    ]);
}

#[test]
fn boolean_boundaries_leave_reserved_keys_null_and_charge_no_key_text() {
    let source = six();
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let mut p = EndingProbe::new(&[2]);
    let (output, capacities, null_keys, charged) =
        ending_run_snapshot_for_test(&parent, &original, &mut p, &mut |g: &BasicScopeContext<
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

//! SOURCE PROPOSAL, UNRUN. Child of basic_group_adjacent_by_tests, not a Project.
//! Inputs/products/logs are literal. Helpers inherited from By only build/render
//! ordinary fields or authenticate walks; no expected partition/accounting code.
use super::*;

struct StartingProbe {
    base: Controls,
    metadata_reads: Cell<usize>,
    log: Rc<RefCell<Vec<String>>>,
    selected: Option<&'static [usize]>,
    starts: &'static [usize],
    post_ids: Option<&'static [usize]>,
    fail: Option<(&'static str, usize)>,
    constant: bool,
    mutate_directions: bool,
    window_parent: Option<Arc<()>>,
}
impl StartingProbe {
    fn new() -> Self {
        Self { base: Controls::new(), metadata_reads: Cell::new(0), log: Rc::default(),
            selected: None, starts: &[], post_ids: None, fail: None,
            constant: false, mutate_directions: false, window_parent: None }
    }
    fn r04() -> Self {
        let mut p = Self::new();
        p.base.directions = vec![SortDirection::Ascending; 2];
        p.base.windows = vec![SequenceWindow::First(3), SequenceWindow::Last(3)];
        p.base.filter = true; p.base.post = true; p.starts = &[1]; p
    }
    fn id(&self, c: &BasicScopeContext<'_>) -> Result<usize, RuntimeError> {
        if self.constant { return Ok(0); }
        let Value::Int(id) = self.base.value(c, "Rank")? else { panic!("literal Rank") };
        Ok(id as usize)
    }
    fn record(&self, phase: &'static str, suffix: impl std::fmt::Display) -> Result<(), RuntimeError> {
        let mut events = self.log.borrow_mut(); events.push(format!("{phase}{suffix}"));
        if self.fail == Some((phase, events.iter().filter(|s| s.starts_with(phase)).count())) {
            let (node, message) = match phase {
                "b" => (101, "boundary"), "f" => (102, "filter"), "s" => (103, "sort"),
                "w" => (104, "bound"), "p" => (105, "post"), _ => unreachable!(),
            };
            return Err(RuntimeError::MappingException { node, message: Some(message.into()) });
        }
        Ok(())
    }
    fn evidence(&self, metadata: usize, events: &[&str]) {
        assert_eq!(self.metadata_reads.get(), metadata);
        assert_eq!(&*self.log.borrow(), events);
    }
    fn bind_window_parent(&mut self, parent: &BasicScopeContext<'_>) {
        // All bound-window fixtures in this packet receive a root context.
        // Observe the existing private authentication token, not a copied view.
        assert!(parent.owned.frames.is_empty());
        self.window_parent = Some(Arc::clone(&parent.identity));
    }
}
impl BasicGroupStartingWithControls<RuntimeError> for StartingProbe {
    fn metadata(&self) -> BasicGroupByMetadata<'_> {
        self.metadata_reads.set(self.metadata_reads.get() + 1); self.base.metadata()
    }
    fn filter(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        let id = self.id(c)?; self.record("f", id)?;
        Ok(self.selected.is_none_or(|ids| ids.contains(&id)))
    }
    fn sort_key(&mut self, key: usize, c: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        let id = self.id(c)?; self.record("s", format_args!("{id}{key}"))?;
        if self.mutate_directions { self.base.directions.clear(); }
        Ok(Value::Int(id as i64))
    }
    fn window(&mut self, bound: usize, parent: &BasicScopeContext<'_>) -> Result<SequenceWindow, RuntimeError> {
        assert!(Arc::ptr_eq(self.window_parent.as_ref().expect("fixture binds its receiving parent"), &parent.identity));
        assert!(parent.owned.frames.is_empty());
        self.record("w", bound)?;
        Ok(self.base.windows.get(bound).copied().unwrap_or(SequenceWindow::First(1)))
    }
    fn starts_group(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        let id = self.id(c)?; self.record("b", id)?; Ok(self.starts.contains(&id))
    }
    fn retain_member(&mut self, c: &BasicScopeContext<'_>) -> Result<bool, RuntimeError> {
        let id = self.id(c)?; self.record("p", id)?;
        Ok(self.post_ids.is_none_or(|ids| ids.contains(&id)))
    }
}
fn r_source() -> Instance {
    input([row(text("unused"), "id0", true, 0), row(text("unused"), "id1", true, 1),
        row(text("unused"), "id2", true, 2)])
}
fn starting_rows(source: &Instance, p: &mut StartingProbe) -> Result<Vec<Instance>, RuntimeError> {
    let roots = BasicSourceRoots::try_new(source, &[], policy())?;
    let parent = roots.root_context()?;
    let original = parent.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])?;
    p.bind_window_parent(&parent);
    for candidate in original.iter() {
        assert!(!Arc::ptr_eq(&parent.identity, &candidate.context().identity));
        assert_eq!(candidate.context().owned.frames.len(), 1);
    }
    let log = Rc::clone(&p.log);
    parent.map_group_starting_with_source_cohort(&original, p, &mut |g: &BasicScopeContext<'_>| {
        log.borrow_mut().push(format!("t{}", g.owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))? - 1));
        render(g, SourceOwnerRef::Primary, &["Rows"])
    })
}
// Root-only target renderer: an unpinned representative read is distinct from
// an explicit Some([]) field pin into the immutable repeated endpoint root.
// The negative explicit-pin assertions stay at the two caller control sites.
fn render_starting_root(c: &BasicScopeContext<'_>, owner: SourceOwnerRef<'_>) -> Result<Instance, RuntimeError> {
    let representative = c.owned_field(owner, &["Label"], None)?;
    let mut members = Vec::new();
    for member in c.walk_owned_source_cohort(owner, &[])?.iter() {
        members.push(group([
            field("Label", scalar(member.context().owned_field(owner, &["Label"], None)?)),
            field("Position", scalar(Value::Int(member.context().owned_position(owner, Some(&[]))? as i64))),
        ]));
    }
    Ok(group([
        field("First", scalar(representative)),
        field("Position", scalar(Value::Int(c.owned_position(owner, Some(&[]))? as i64))),
        field("Members", repeated(members)),
    ]))
}
fn limit(component: &'static str, used: usize, requested: usize, maximum: usize) -> RuntimeError {
    BasicSourceBridgeError::PairedMetadataLimit { component, used, requested, maximum }.into()
}
fn forbidden_target(_: &BasicScopeContext<'_>) -> Result<Instance, RuntimeError> {
    panic!("refusal/empty control must not invoke target")
}
#[test]
fn private_ordinary_grouped_observer_is_not_an_authenticated_parent_extension() {
    let source=r_source();let roots=BasicSourceRoots::try_new(&source,&[],policy()).unwrap();let parent=roots.root_context().unwrap();
    let rows=ScopeContext::new(&source).walk_source(&["Rows"]).into_iter().map(|c|(c,Value::Int(0))).collect();
    let ordinary=crate::GroupedItems::by(rows,Some("Rows"));let scopes=ordinary.contexts();
    // RR negative-observer seam request, not a public attach constructor. The
    // ordinary grouped selected frame has no matching parent origin witness.
    assert_eq!(ordinary_grouped_walk_rejection_for_test(&parent,&scopes[0],&[]).unwrap_err(),
        RuntimeError::from(BasicSourceBridgeError::RegistryMismatch));
}

#[test]
fn r01_empty_original_runs_all_bounds_and_no_candidate_or_target() {
    let mut p = StartingProbe::r04();
    assert_eq!(starting_rows(&input([]), &mut p).unwrap(), Vec::<Instance>::new());
    p.evidence(1, &["w0", "w1"]);
    let mut p = StartingProbe::r04(); p.fail = Some(("w", 2));
    assert_eq!(starting_rows(&input([]), &mut p).unwrap_err(), RuntimeError::MappingException {
        node: 104, message: Some("bound".into()) });
    p.evidence(1, &["w0", "w1"]);
}
#[test]
fn r02_foreign_registry_and_cloned_parent_empty_are_refused_before_metadata() {
    let source = input([]);
    let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let foreign_roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap(); let foreign = foreign_roots.root_context().unwrap();
    let original = parent.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"]).unwrap();
    let foreign = foreign.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"]).unwrap();
    let mut p = StartingProbe::r04();
    assert_eq!(parent.map_group_starting_with_source_cohort(&foreign, &mut p, &mut forbidden_target).unwrap_err(),
        BasicSourceBridgeError::RegistryMismatch.into());
    let clone = parent.try_clone_view().unwrap();
    assert_eq!(clone.map_group_starting_with_source_cohort(&original, &mut p, &mut forbidden_target).unwrap_err(),
        BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 0 }.into());
    p.evidence(0, &[]);
}
#[test]
fn r03_corrupt_originals_and_terminal_variants_keep_error_and_metadata_order() {
    let source = r_source(); let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    for (variant, error, metadata) in [
        (0, BasicSourceBridgeError::DuplicatePhaseCandidate { input_ordinal: 1, candidate_ordinal: 0 }, 0),
        (1, BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 2 }, 0),
        (2, BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 0 }, 0),
        (3, BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 1 }, 1),
        (4, BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 1 }, 0),
    ] {
        let mut original = parent.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"]).unwrap();
        let donor = parent.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"]).unwrap();
        match variant { 0 => original.candidates[1].ordinal = 0,
            1 => { original.candidates.pop(); }, 2 => original.candidates.swap(0, 1),
            3 => original.candidates[1].terminal.origin_cut = usize::MAX,
            4 => original.candidates[1].cohort = Arc::clone(&donor.candidates[1].cohort), _ => unreachable!() }
        let mut p = StartingProbe::r04();
        assert_eq!(parent.map_group_starting_with_source_cohort(&original, &mut p, &mut forbidden_target).unwrap_err(), error.into());
        p.evidence(metadata, &[]);
    }
    // Pure validator controls reuse the real terminal proof, never a fake public Project.
    // Existing enums/cuts below are actual merged private fields, not invented access.
    // Starting's terminal-after-metadata frontier is independently tested above.
    for variant in 0..10 {
        let mut original = parent.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"]).unwrap();
        let c = &mut original.candidates[0];
        match variant {
            0 => { c.terminal.owner = OwnedOwnerKey::Named(usize::MAX); c.terminal.canonical = TerminalPath::Empty; },
            1 => c.terminal.canonical = TerminalPath::Owned(usize::MAX),
            2 => c.terminal.owned_cut = usize::MAX, 3 => c.terminal.legacy_cut = usize::MAX,
            4 => c.terminal.origin_cut = usize::MAX,
            5 => c.terminal.raw = TerminalRaw::Suffix { start:usize::MAX,head:None },
            6 => c.terminal.raw = TerminalRaw::Legacy(usize::MAX),
            7 => c.terminal.canonical = TerminalPath::Origin(usize::MAX),
            8 => c.terminal.canonical = TerminalPath::Empty,
            9 => c.terminal.item = &source, _ => unreachable!(),
        }
        let mut p = StartingProbe::r04();
        assert_eq!(parent.map_group_starting_with_source_cohort(&original, &mut p, &mut forbidden_target).unwrap_err(),
            BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 0 }.into());
        p.evidence(1, &[]);
    }
}
#[test]
fn r04_r05_r06_r08_literal_products_and_complete_callback_frontiers() {
    for variant in 4..=8 {
        if variant == 7 { continue; }
        let mut p = StartingProbe::r04();
        if variant == 4 { p.mutate_directions = true; }
        if variant == 5 { p.base.prefilter = true; p.selected = Some(&[0, 2]); }
        if variant == 6 { p.selected = Some(&[0, 2]); }
        if variant == 8 { p.base.windows[0] = SequenceWindow::First(0); }
        let result = starting_rows(&r_source(), &mut p).unwrap();
        match variant {
            4 => { assert_eq!(result, vec![expected("id0", 1, &["id0"]), expected("id1", 2, &["id1", "id2"])]);
                p.evidence(1, &["s00","s01","s10","s11","s20","s21","w0","w1","f0","b0","p0","f1","b1","p1","f2","b2","p2","t0","t1"]); },
            5 => { assert_eq!(result, vec![expected("id0", 1, &["id0","id2"])]);
                p.evidence(1, &["f0","f1","f2","s00","s01","s20","s21","w0","w1","b0","p0","b2","p2","t0"]); },
            6 => { assert_eq!(result, vec![expected("id0", 1, &["id0","id2"])]);
                p.evidence(1, &["s00","s01","s10","s11","s20","s21","w0","w1","f0","b0","p0","f1","f2","b2","p2","t0"]); },
            8 => { assert_eq!(result, Vec::<Instance>::new());
                p.evidence(1, &["s00","s01","s10","s11","s20","s21","w0","w1","f0","b0","p0","f1","b1","p1","f2","b2","p2"]); }, _ => unreachable!(),
        }
    }
}
#[test]
fn r07_precise_prepared_storage_and_immutable_publication() {
    let source = r_source(); let roots = BasicSourceRoots::try_new(&source, &[], policy()).unwrap();
    let parent = roots.root_context().unwrap();
    let original = parent.walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"]).unwrap();
    let p = StartingProbe::r04();
    // Proposed RR test-only seam; intentionally unresolved until owner approval.
    // See proposals/PRIVATE-SEAMS.md. It must execute real Starting authentication
    // and preparation, not keyed preparation or an independent resource calculator.
    let (capacities, _, _, null_keys) = starting_prepared_snapshot_for_test(&parent, &original, &p).unwrap();
    assert_eq!(capacities, [2,6,2,3,3,3,3,3,3,3,3,3]); assert!(null_keys);
    p.evidence(1, &[]);
    let mut p = StartingProbe::r04(); p.starts = &[0,1,2];
    p.bind_window_parent(&parent);
    let log = Rc::clone(&p.log);
    let (output, after, still_null) = starting_run_snapshot_for_test(&parent, &original, &mut p, &mut |g:&BasicScopeContext<'_>| {
        log.borrow_mut().push(format!("t{}",g.owned_position(SourceOwnerRef::Primary,Some(&["Rows"]))?-1));
        assert_eq!(g.owned_field_with_root_pin(SourceOwnerRef::Primary,&["Marker"],Some(&[]),false).unwrap_err(),
            OwnedSourceError::InactiveSourceFrame{owner:OwnedSourceIdentity::Primary,collection:Some(vec![])}.into());
        assert_eq!(g.owned_field_with_root_pin(SourceOwnerRef::Primary,&["Rows","Label"],Some(&[]),true).unwrap_err(),
            OwnedSourceError::UniteratedRepetition{owner:OwnedSourceIdentity::Primary,path:vec!["Rows".into(),"Label".into()],segment:1}.into());
        render(g,SourceOwnerRef::Primary,&["Rows"])
    }).unwrap();
    assert_eq!(after,[2,6,2,3,3,3,3,3,3,3,3,3]); assert!(still_null);
    assert_eq!(output, vec![expected("id0",1,&["id0"]), expected("id1",2,&["id1"]), expected("id2",3,&["id2"])]);
    assert_eq!(output[0], expected("id0",1,&["id0"]));
    p.evidence(1, &["s00","s01","s10","s11","s20","s21","w0","w1","f0","b0","p0","f1","b1","p1","f2","b2","p2","t0","t1","t2"]);
}
#[test]
fn r09_boolean_boundary_adds_no_key_text_at_32_or_64() {
    for (n, bytes, used, requested) in [(1,31,12,20),(1,32,12,20),(2,63,24,40),(2,64,24,40)] {
        let source = input((0..n).map(|_| row(Value::Int(0),"x",true,0)));
        let mut limits = small_policy(bytes); limits.max_path_segments = 2;
        let roots = BasicSourceRoots::try_new(&source,&[],limits).unwrap(); let parent = roots.root_context().unwrap();
        let original = parent.walk_owned_source_cohort(SourceOwnerRef::Primary,&["Rows"]).unwrap();
        let mut p = StartingProbe::new(); p.base.post = true;
        let log = Rc::clone(&p.log);
        let result = parent.map_group_starting_with_source_cohort(&original,&mut p,&mut |_: &BasicScopeContext<'_>| {
            log.borrow_mut().push("t0".into()); Ok::<_,RuntimeError>(scalar(Value::Int(7))) });
        if bytes == 31 || bytes == 63 { assert_eq!(result.unwrap_err(),limit("MetadataBytes",used,requested,bytes)); p.evidence(1,&[]); }
        else { assert_eq!(result.unwrap(),vec![scalar(Value::Int(7))]);
            p.evidence(1,if n == 1 { &["b0","p0","t0"] } else { &["b0","p0","b0","p0","t0"] }); }
    }
}
#[test]
fn r10_exact_210_slots_and_retained_capacity_plus_one() {
    let source = group([]); let limits = OwnedSourceLimits { max_roots:1,max_active_frames:1,
        max_path_segments:0,max_metadata_bytes:0,max_walk_contexts:1 };
    let roots = BasicSourceRoots::try_new(&source,&[],limits).unwrap(); let parent = roots.root_context().unwrap();
    let mut original = parent.walk_owned_source_cohort(SourceOwnerRef::Primary,&[]).unwrap();
    replace_original_backing(&mut original,4);
    let mut p = StartingProbe::new(); p.constant = true; p.base.directions = vec![SortDirection::Ascending;2];
    p.base.virtual_windows = Some(48);
    p.bind_window_parent(&parent);
    // Independent literals: seed38 + fixed28 + 3*48=144. Never derive expected from actual.
    let (capacities, slots, maximum, null_keys) = starting_prepared_snapshot_for_test(&parent,&original,&p).unwrap();
    assert_eq!((slots,maximum),(210,210)); assert_eq!(38+28+144,210);
    assert_eq!(capacities,[2,2,48,1,1,1,1,1,1,1,1,1]); assert!(null_keys);
    p.log.borrow_mut().clear(); p.metadata_reads.set(0);
    let target_log=Rc::clone(&p.log);
    assert_eq!(parent.map_group_starting_with_source_cohort(&original,&mut p,&mut |_:&BasicScopeContext<'_>| {
        target_log.borrow_mut().push("t0".into());Ok::<_,RuntimeError>(scalar(Value::Int(7)))
    }).unwrap(),vec![scalar(Value::Int(7))]);
    p.evidence(1,&["s00","s01","w0","w1","w2","w3","w4","w5","w6","w7",
        "w8","w9","w10","w11","w12","w13","w14","w15","w16","w17","w18","w19",
        "w20","w21","w22","w23","w24","w25","w26","w27","w28","w29","w30","w31",
        "w32","w33","w34","w35","w36","w37","w38","w39","w40","w41","w42","w43",
        "w44","w45","w46","w47","b0","t0"]);
    replace_original_backing(&mut original,5); p.log.borrow_mut().clear(); p.metadata_reads.set(0);
    assert_eq!(parent.map_group_starting_with_source_cohort(&original,&mut p,&mut forbidden_target).unwrap_err(),limit("MetadataSlots",39,172,210));
    p.evidence(1,&[]);
}
#[test]
fn r11_425_text_then_426_slot_priority() {
    let source = input([row(Value::Int(0),"x",false,0)]); let mut limits=small_policy(31); limits.max_walk_contexts=1;
    let roots=BasicSourceRoots::try_new(&source,&[],limits).unwrap(); let parent=roots.root_context().unwrap();
    let mut original=parent.walk_owned_source_cohort(SourceOwnerRef::Primary,&["Rows"]).unwrap();
    for (capacity,error) in [(4,limit("MetadataBytes",12,20,31)),(5,limit("MetadataSlots",48,378,425))] {
        replace_original_backing(&mut original,capacity); let mut p=StartingProbe::new(); p.base.virtual_windows=Some(118);
        p.bind_window_parent(&parent);
        assert_eq!(parent.map_group_starting_with_source_cohort(&original,&mut p,&mut forbidden_target).unwrap_err(),error);
        p.evidence(1,&[]);
    }
}
#[test]
fn r14_original_callback_errors_and_zero_windows_do_not_erase_work() {
    for (phase,occurrence,node,message,events) in [
        ("s",4,103,"sort",vec!["s00","s01","s10","s11"]),
        ("b",1,101,"boundary",vec!["s00","s01","s10","s11","s20","s21","w0","w1","f0","b0"]),
        ("p",2,105,"post",vec!["s00","s01","s10","s11","s20","s21","w0","w1","f0","b0","p0","f1","b1","p1"]),
    ] {
        let mut p=StartingProbe::r04(); p.base.windows[0]=SequenceWindow::First(0); p.fail=Some((phase,occurrence));
        assert_eq!(starting_rows(&r_source(),&mut p).unwrap_err(),RuntimeError::MappingException { node,message:Some(message.into()) });
        p.evidence(1,&events);
    }
}
#[test]
fn r15_arithmetic_and_representability_use_original_checked_guards() {
    let source=r_source(); let roots=BasicSourceRoots::try_new(&source,&[],policy()).unwrap(); let parent=roots.root_context().unwrap();
    let original=parent.walk_owned_source_cohort(SourceOwnerRef::Primary,&["Rows"]).unwrap();
    let overflow:RuntimeError=BasicSourceBridgeError::PairedMetadataArithmeticOverflow { component:"MetadataSlots" }.into();
    for (h,implicit) in [(usize::MAX,true),(usize::MAX/3+1,false)] {
        let mut p=StartingProbe::new(); p.base.virtual_windows=Some(h); p.base.implicit_first=implicit;
        assert_eq!(parent.map_group_starting_with_source_cohort(&original,&mut p,&mut forbidden_target).unwrap_err(),overflow); p.evidence(1,&[]);
    }
    for field in 0..4 {
        let mut limits=policy();match field {0=>limits.max_roots=usize::MAX,1=>limits.max_active_frames=usize::MAX,
            2=>limits.max_path_segments=usize::MAX,3=>limits.max_walk_contexts=usize::MAX,_=>unreachable!()}
        assert_eq!(group_maximum(limits).unwrap_err(),overflow);
    }
    assert_eq!(paired_capacity::<Bucket>(isize::MAX as usize/std::mem::size_of::<Bucket>()+1).unwrap_err(),overflow);
    assert_eq!(product(usize::MAX,2).unwrap_err(),overflow);
    assert_eq!(sum(usize::MAX,1).unwrap_err(),overflow);
    // Existing byte-admission checked intermediate, without an impossible string.
    let (mut budget,_)=parent.budget(SourceOwnerRef::Primary,&[],None,false).unwrap();budget.bytes=usize::MAX;
    assert_eq!(charge_text(&mut budget,1).unwrap_err(),BasicSourceBridgeError::PairedMetadataArithmeticOverflow{component:"MetadataBytes"}.into());
    // Reuse exact maintained canonical/raw admission and W/F boundaries unchanged.
    r16_walk_count_and_synthetic_group_depth_keep_typed_exact_plus_one_limits();
    r16_complete_owned_and_raw_alias_selectors_admit_exact_and_reject_unvisited_suffixes();
}
#[test]
fn r15_starting_full_selector_and_frame_walk_limits() {
    for rows in [Some(repeated([])),Some(scalar(Value::Null)),Some(scalar(Value::json_null())),None] {
        let source=group(rows.map(|r|field("Rows",r))); let roots=BasicSourceRoots::try_new(&source,&[],small_policy(4096)).unwrap();
        let parent=roots.root_context().unwrap();
        for legacy in [false,true] {
            let mut p=StartingProbe::new(); p.constant=true; p.base.windows.push(SequenceWindow::First(0));
            let result=if legacy { parent.walk_source_cohort(&["Rows","Extra"]) }
                else { parent.walk_owned_source_cohort(SourceOwnerRef::Primary,&["Rows","Extra"]) }
                .and_then(|original|parent.map_group_starting_with_source_cohort(&original,&mut p,&mut forbidden_target));
            assert_eq!(result.unwrap_err(),path_limit(2,1)); p.evidence(0,&[]);
        }
    }
    let source=group([]);
    for frames in [1,0] {
        let mut limits=small_policy(4096); limits.max_active_frames=frames;
        let roots=BasicSourceRoots::try_new(&source,&[],limits).unwrap(); let parent=roots.root_context().unwrap();
        let original=parent.walk_owned_source_cohort(SourceOwnerRef::Primary,&[]).unwrap(); let mut p=StartingProbe::new(); p.constant=true;
        let log=Rc::clone(&p.log);
        let result=parent.map_group_starting_with_source_cohort(&original,&mut p,&mut |_:&BasicScopeContext<'_>|{
            log.borrow_mut().push("t0".into());Ok::<_,RuntimeError>(scalar(Value::Int(7))) });
        if frames==1 { assert_eq!(result.unwrap(),vec![scalar(Value::Int(7))]); p.evidence(1,&["b0","t0"]); }
        else { assert_eq!(result.unwrap_err(),limit("frame depth",0,1,0)); p.evidence(1,&[]); }
    }
}
#[test]
fn r15_starting_exact_walk_and_consumed_alias_canonical_frontiers() {
    let source=r_source();let mut limits=policy();limits.max_walk_contexts=3;
    let roots=BasicSourceRoots::try_new(&source,&[],limits).unwrap();let parent=roots.root_context().unwrap();
    let original=parent.walk_owned_source_cohort(SourceOwnerRef::Primary,&["Rows"]).unwrap();
    let mut p=StartingProbe::new();p.starts=&[0,1,2];let target_log=Rc::clone(&p.log);let mut ordinal=0;
    let output=parent.map_group_starting_with_source_cohort(&original,&mut p,&mut |_:&BasicScopeContext<'_>|{
        target_log.borrow_mut().push(format!("t{ordinal}"));ordinal+=1;Ok::<_,RuntimeError>(scalar(Value::Int(7)))
    }).unwrap();assert_eq!(output,vec![scalar(Value::Int(7));3]);p.evidence(1,&["b0","b1","b2","t0","t1","t2"]);
    let source=input([row(Value::Int(0),"a",true,0),row(Value::Int(0),"b",true,1),row(Value::Int(0),"c",true,2),row(Value::Int(0),"d",true,3)]);
    let roots=BasicSourceRoots::try_new(&source,&[],limits).unwrap();let parent=roots.root_context().unwrap();let mut p=StartingProbe::new();
    let error=parent.walk_owned_source_cohort(SourceOwnerRef::Primary,&["Rows"]).and_then(|o|parent.map_group_starting_with_source_cohort(&o,&mut p,&mut forbidden_target)).unwrap_err();
    assert_eq!(error,OwnedSourceError::MetadataLimit{kind:OwnedSourceLimitKind::WalkContexts,used:3,requested:1,max:3}.into());p.evidence(0,&[]);
    // Raw alias is one additional selector segment, not one canonical path cell.
    let primary=group([]);let named=group([]);let declarations=[DeclaredNamedSourceRoot{name:"Alias",instance:Some(&named)}];
    limits.max_path_segments=0;let roots=BasicSourceRoots::try_new(&primary,&declarations,limits).unwrap();let parent=roots.root_context().unwrap();
    let original=parent.walk_source_cohort(&["Alias"]).unwrap();let mut p=StartingProbe::new();p.constant=true;p.base.windows=vec![SequenceWindow::First(0)];
    p.bind_window_parent(&parent);
    assert_eq!(parent.map_group_starting_with_source_cohort(&original,&mut p,&mut forbidden_target).unwrap(),Vec::<Instance>::new());p.evidence(1,&["w0","b0"]);
    assert_eq!(parent.walk_owned_source_cohort(SourceOwnerRef::Named("Alias"),&["Alias"]).err().unwrap(),path_limit(1,0));
    // Endpoint-local canonical prefix denial remains separate from raw spelling.
    let source=group([field("",repeated([group([field("Extra",repeated([]))])]))]);
    limits.max_path_segments=1;let roots=BasicSourceRoots::try_new(&source,&[],limits).unwrap();let root=roots.root_context().unwrap();
    let prefix=root.walk_owned_source_cohort(SourceOwnerRef::Primary,&[""]).unwrap();let parent=prefix.get(0).unwrap().context();
    let mut p=StartingProbe::new();let result=parent.walk_source_cohort(&["Extra"]).and_then(|o|parent.map_group_starting_with_source_cohort(&o,&mut p,&mut forbidden_target));
    assert_eq!(result.unwrap_err(),path_limit(2,1));p.evidence(0,&[]);
}
// Test-only metadata observation around the maintained old keyed fixture, not a
// production adapter and not the bool Starting interface.
struct OldMetadataProbe { base: Controls, reads: Cell<usize> }
impl BasicGroupByControls<RuntimeError> for OldMetadataProbe {
    fn metadata(&self)->BasicGroupByMetadata<'_>{self.reads.set(self.reads.get()+1);self.base.metadata()}
    fn filter(&mut self,c:&BasicScopeContext<'_>)->Result<bool,RuntimeError>{self.base.filter(c)}
    fn sort_key(&mut self,k:usize,c:&BasicScopeContext<'_>)->Result<Value,RuntimeError>{self.base.sort_key(k,c)}
    fn window(&mut self,k:usize,c:&BasicScopeContext<'_>)->Result<SequenceWindow,RuntimeError>{self.base.window(k,c)}
    fn group_key(&mut self,c:&BasicScopeContext<'_>)->Result<Value,RuntimeError>{self.base.group_key(c)}
    fn retain_member(&mut self,c:&BasicScopeContext<'_>)->Result<bool,RuntimeError>{self.base.retain_member(c)}
}
fn old_literal_budget(adjacent:bool,n:usize,bytes:usize,key:Value)->(Result<Vec<Instance>,RuntimeError>,Vec<&'static str>,usize){
    let source=input((0..n).map(|_|row(Value::Int(0),"x",true,0)));let mut limits=small_policy(bytes);limits.max_path_segments=2;
    let roots=BasicSourceRoots::try_new(&source,&[],limits).unwrap();let parent=roots.root_context().unwrap();
    let original=parent.walk_owned_source_cohort(SourceOwnerRef::Primary,&["Rows"]).unwrap();
    let mut p=OldMetadataProbe{base:Controls::new(),reads:Cell::new(0)};p.base.key=Key::Constant(key);p.base.post=true;
    let target_log=Rc::clone(&p.base.calls);
    let mut target=|c:&BasicScopeContext<'_>|{target_log.borrow_mut().push("target");constant_target(c)};
    let result=if adjacent{parent.map_group_adjacent_by_source_cohort(&original,&mut p,&mut target)}
        else{parent.map_group_by_source_cohort(&original,&mut p,&mut target)};
    let events=p.base.calls.borrow().clone();(result,events,p.reads.get())
}
#[test]
fn r16_old_by_and_adjacent_duplicate_string_frontiers_are_not_starting_oracles() {
    returned_string_key_uses_utf8_and_cumulative_ledger_even_when_equal();
    returned_sort_key_utf8_has_exact_cumulative_frontier_before_bounds_or_group_keys();
    r10_each_utf8_key_including_duplicate_is_charged_before_its_post();
    adjacent_late_text_ledger_counts_a_later_equal_run_and_all_sort_keys();
    for adjacent in [false,true] {
        for (key,fail_one,pass_one,fail_two,pass_two,width) in [
            (text("é"),33,34,67,68,2),(text("💠"),35,36,71,72,4)] {
            for (n,bytes,fail,used) in [(1,fail_one,true,32),(1,pass_one,false,32),
                (2,fail_two,true,64+width),(2,pass_two,false,64+width)] {
                let (actual,log,metadata)=old_literal_budget(adjacent,n,bytes,key.clone());assert_eq!(metadata,1);
                if fail {assert_eq!(actual.unwrap_err(),limit("MetadataBytes",used,width,bytes));
                    assert_eq!(log,if n==1 {vec!["key"]}else{vec!["key","post","key"]});}
                else {assert_eq!(actual.unwrap(),vec![scalar(Value::Int(7))]);
                    assert_eq!(log,if n==1 {vec!["key","post","target"]}else{vec!["key","post","key","post","target"]});}
            }
        }
    }
}
#[test]
fn private_groupmembers_repeated_root_shadow_and_forged_context_preservation() {
    let source=repeated([row(text("unused"),"a",true,0),row(text("unused"),"b",true,1),
        row(text("unused"),"c",true,2),row(text("unused"),"d",true,3)]);
    let foreign=repeated([row(text("unused"),"foreign",true,0)]);
    let declarations=[DeclaredNamedSourceRoot { name:"Alias",instance:Some(&foreign) },DeclaredNamedSourceRoot { name:"Absent",instance:None }];
    let roots=BasicSourceRoots::try_new(&source,&declarations,policy()).unwrap(); let parent=roots.root_context().unwrap();
    let original=parent.walk_owned_source_cohort(SourceOwnerRef::Primary,&[]).unwrap();
    let mut p=StartingProbe::new(); p.base.path=&[]; p.starts=&[1,2];
    let result=parent.map_group_starting_with_source_cohort(&original,&mut p,&mut |g:&BasicScopeContext<'_>| {
        assert!(g.owned.frames.last().unwrap().kind==OwnedOccurrenceKind::GroupMembers);
        assert!(g.origins.last().unwrap().kind==OriginKind::GroupMembers);
        assert_eq!(g.owned_field(SourceOwnerRef::Primary,&["Label"],Some(&[])).unwrap_err(),
            OwnedSourceError::UniteratedRepetition { owner:OwnedSourceIdentity::Primary,path:vec!["Label".into()],segment:0 }.into());
        for copy in [g.try_clone_view()?] {
            assert!(copy.owned.frames.last().unwrap().kind==OwnedOccurrenceKind::GroupMembers);
            let members=copy.walk_owned_source_cohort(SourceOwnerRef::Primary,&[])?;
            for (i,m) in members.iter().enumerate() {
                let c=m.context(); assert!(c.owned.frames[0].kind==OwnedOccurrenceKind::GroupMembers);
                assert!(c.owned.frames[1].kind==OwnedOccurrenceKind::Item);
                assert_eq!(c.owned_position(SourceOwnerRef::Primary,Some(&[]))?,i+1);
                assert_eq!(c.owned_field(SourceOwnerRef::Primary,&["Label"],Some(&[]))?,c.owned_field(SourceOwnerRef::Primary,&["Label"],None)?);
            }
            let other=copy.walk_owned_source_cohort(SourceOwnerRef::Named("Alias"),&[])?;
            assert_eq!(other.get(0).unwrap().context().owned_field(SourceOwnerRef::Primary,&["Label"],Some(&[])).unwrap_err(),
                OwnedSourceError::UniteratedRepetition { owner:OwnedSourceIdentity::Primary,path:vec!["Label".into()],segment:0 }.into());
            let outer_position=copy.owned_position(SourceOwnerRef::Primary,Some(&[]))?;
            let outer_origin=copy.origins.len()-1;
            let mut nested=StartingProbe::new();nested.constant=true;nested.base.directions=vec![SortDirection::Ascending];
            let nested_log=Rc::clone(&nested.log);
            let inner=copy.map_group_starting_with_source_cohort(&other,&mut nested,&mut |h:&BasicScopeContext<'_>|{
                nested_log.borrow_mut().push("t0".into());
                assert!(h.owned.frames[0].kind==OwnedOccurrenceKind::GroupMembers);
                assert_eq!(h.owned_position(SourceOwnerRef::Primary,Some(&[]))?,outer_position);
                assert!(h.origins[outer_origin].kind==OriginKind::GroupMembers);
                assert_eq!(h.owned_field(SourceOwnerRef::Named("Alias"),&["Label"],Some(&[])).unwrap_err(),
                    OwnedSourceError::UniteratedRepetition { owner:OwnedSourceIdentity::Named("Alias".into()),path:vec!["Label".into()],segment:0 }.into());
                render_starting_root(h,SourceOwnerRef::Named("Alias"))
            })?;
            assert_eq!(inner,vec![expected("foreign",1,&["foreign"])]);nested.evidence(1,&["s00","b0","t0"]);
            assert_eq!(copy.owned_field(SourceOwnerRef::Named("Absent"),&["Label"],Some(&[])).unwrap_err(),
                OwnedSourceError::SourceUnavailable { name:"Absent".into() }.into());
        }
        render_starting_root(g,SourceOwnerRef::Primary)
    }).unwrap();
    assert_eq!(result,vec![expected("a",1,&["a"]),expected("b",2,&["b"]),expected("c",3,&["c","d"])]);
    p.evidence(1,&["b0","b1","b2","b3"]);
    // Foreign ordinary Item never shadows this group's Primary observation.
}

use super::*;
#[test]
fn review84_sparse_origin_growth_must_admit_potential_relocation_before_capacity_changes() {
    let document = roxmltree::Document::parse("<component/>").unwrap();
    let origin = Origin::ActiveMain(RawOwner::of(document.root_element()));
    // Four entries cost 12 logical units with admitted geometric storage. The
    // next entry needs 2 insert + 4 potential copies + 4 new slots = 10, not 9.
    let budget = CountConstruction::for_test(21, 1024 * 1024);
    let mut values = RegisteredVec::with_origin_capture(
        &Registry::default(),
        OriginCapture::Count(budget.clone()),
    );
    for i in 0..4 {
        values.push_origin(i, origin);
    }
    let before = (
        values.count_origins.len(),
        values.count_origins.capacity(),
        budget.remaining_for_test(),
    );
    values.push_origin(4, origin);
    let after = (
        values.count_origins.len(),
        values.count_origins.capacity(),
        budget.remaining_for_test(),
    );
    eprintln!(
        "REVIEW84 sparse growth before={before:?} after={after:?} values={:?}",
        &*values
    );
    assert_eq!(&*values, &[0, 1, 2, 3, 4]);
    assert_eq!(
        values.count_origins.len(),
        before.0,
        "insufficient relocation admission must not retain another origin"
    );
    assert_eq!(
        values.count_origins.capacity(),
        before.1,
        "insufficient relocation admission must not grow capacity"
    );
    assert!(budget.claim(0, 0, 0).is_err());
    values.push_origin(5, origin);
    assert_eq!(values.count_origins.capacity(), before.1);
    assert_eq!(values.count_origin_at(4), Origin::SyntheticOrUnproved);
}

#[test]
fn sparse_capacity_charges_exact_requested_bytes_and_potential_copy_rows() {
    let origin = Origin::SyntheticOrUnproved;
    let tuple = std::mem::size_of::<(usize, Origin)>();
    let budget = CountConstruction::for_test(100, 8 * tuple);
    let mut values = RegisteredVec::with_origin_capture(
        &Registry::default(),
        OriginCapture::Count(budget.clone()),
    );
    for i in 0..5 {
        values.push_origin(i, origin);
    }
    assert_eq!(values.count_origin_capacity.requested, 8);
    assert_eq!(values.count_origin_capacity.reserved_bytes, 8 * tuple);
    assert_eq!(values.count_origin_capacity.relocated_rows, 4);
    assert_eq!(values.count_origin_capacity.growths, 2);
    assert_eq!(budget.remaining_for_test(), (100 - (2 * 5 + 4 + 8), 0));
    assert!(values.count_origins.capacity() >= values.count_origin_capacity.requested);
    eprintln!(
        "sparse origins: requested_capacity={} actual_vec_capacity={} admitted_bytes={} potential_copy_rows={} growths={}",
        values.count_origin_capacity.requested,
        values.count_origins.capacity(),
        values.count_origin_capacity.reserved_bytes,
        values.count_origin_capacity.relocated_rows,
        values.count_origin_capacity.growths
    );
    let short = CountConstruction::for_test(100, 4 * tuple - 1);
    let mut denied = RegisteredVec::with_origin_capture(
        &Registry::default(),
        OriginCapture::Count(short.clone()),
    );
    denied.push_origin(1, origin);
    assert_eq!(denied.count_origins.capacity(), 0);
    assert_eq!(denied.count_origin_capacity.requested, 0);
    assert_eq!(denied.count_origin_capacity.growths, 0);
    assert_eq!(&*denied, &[1]);
    assert!(short.claim(0, 0, 0).is_err());
}

#[test]
fn sparse_growth_exact_work_and_byte_boundaries_keep_registration_identity() {
    let doc =
        roxmltree::Document::parse("<components><component/><component/></components>").unwrap();
    let mut nodes = doc.descendants().filter(|n| n.has_tag_name("component"));
    let first = Origin::ActiveMain(RawOwner::of(nodes.next().unwrap()));
    let second = Origin::PromotedLocalCatalog(RawOwner::of(nodes.next().unwrap()));
    let tuple = std::mem::size_of::<(usize, Origin)>();
    for (work, bytes, succeeds) in [
        (22, 8 * tuple, true),
        (21, 8 * tuple, false),
        (22, 8 * tuple - 1, false),
    ] {
        let budget = CountConstruction::for_test(work, bytes);
        let registry = Registry {
            construction: Some(ConstructionBudget::default()),
            ..Registry::default()
        };
        let mut values =
            RegisteredVec::with_origin_capture(&registry, OriginCapture::Count(budget.clone()));
        for i in 0..5 {
            values.push_origin(i, if i % 2 == 0 { first } else { second });
        }
        assert_eq!(&*values, &[0, 1, 2, 3, 4]);
        assert_eq!(
            values.count_origin_capacity.growths,
            if succeeds { 2 } else { 1 }
        );
        assert_eq!(
            values.count_origin_capacity.requested,
            if succeeds { 8 } else { 4 }
        );
        assert_eq!(
            values.count_origin_capacity.relocated_rows,
            if succeeds { 4 } else { 0 }
        );
        for i in 0..5 {
            let origin = if i % 2 == 0 { first } else { second };
            assert_eq!(values.origin_at(i), origin);
            assert_eq!(
                values.count_origin_at(i),
                if succeeds {
                    origin
                } else {
                    Origin::SyntheticOrUnproved
                }
            );
        }
        assert_eq!(budget.claim(0, 0, 0).is_ok(), succeeds);
        assert!(
            registry
                .construction
                .as_ref()
                .unwrap()
                .claim(0, 0, 0)
                .is_ok()
        );
        if !succeeds {
            let before = (values.count_origins.len(), values.count_origins.capacity());
            for i in 5..32 {
                values.push_origin(i, first);
            }
            assert_eq!(
                (values.count_origins.len(), values.count_origins.capacity()),
                before
            );
            assert_eq!(values.len(), 32);
        }
    }
}

#[test]
fn allocator_spare_capacity_does_not_grant_unadmitted_logical_origin_slots() {
    let mut rows = Vec::with_capacity(32);
    let mut capacity = OriginCapacity::default();
    let budget = CountConstruction::for_test(5, usize::MAX);
    assert!(capacity.admit(&mut rows, &budget).is_err());
    assert_eq!(capacity.requested, 0);
    assert!(rows.is_empty());
    assert_eq!(rows.capacity(), 32);
}

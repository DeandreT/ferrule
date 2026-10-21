use functions::{InvocationCounterError as Error, InvocationCounterLimits, InvocationCounters};

fn counters(count: usize, bytes: usize) -> InvocationCounters {
    InvocationCounters::new(InvocationCounterLimits {
        max_counters: count,
        max_text_bytes: bytes,
    })
}

#[test]
fn invocation_counter_literal_controls() {
    let mut c = counters(16, 1024);
    assert_eq!(c.next("A", 5, 2, Some("")), Ok(5));
    assert_eq!(c.next("A", 999, 3, Some("")), Ok(8));
    assert_eq!(c.next("B", 20, 1, Some("")), Ok(20));
    assert_eq!(c.next("A", 100, 2, Some("new")), Ok(100));
    assert_eq!(c.next("A", 999, 2, Some("new")), Ok(102));
    assert!(c.reset("A"));
    assert_eq!(c.next("A", 7, 5, Some("new")), Ok(7));
    assert_eq!(c.next("B", 999, 1, Some("")), Ok(21));
    assert_eq!(counters(16, 1024).next("A", 3, 1, Some("new")), Ok(3));
    assert_eq!(c.next("overflow", i64::MAX, 1, Some("")), Ok(i64::MAX));
    assert_eq!(c.next("overflow", 0, 1, Some("")), Ok(i64::MIN));
    assert_eq!(c.next("underflow", i64::MIN, -1, Some("")), Ok(i64::MIN));
    assert_eq!(c.next("underflow", 0, -1, Some("")), Ok(i64::MAX));
    assert_eq!(c.next("zero", 4, 0, Some("")), Ok(4));
    assert_eq!(c.next("zero", 100, 0, Some("")), Ok(4));
    assert_eq!(c.next("nullable", 9, 1, None), Ok(9));
    assert_eq!(c.next("nullable", 100, 1, None), Ok(10));
    assert_eq!(c.next("nullable", 22, 1, Some("")), Ok(22));
}

#[test]
fn counter_limits_refuse_without_consuming_values_or_budgets() {
    let mut c = counters(2, 5);
    assert_eq!(c.next("A", 5, 1, Some("xyz")), Ok(5));
    assert_eq!(c.retained_text_bytes(), 4);
    assert_eq!(
        c.next("A", 100, 1, Some("longer")),
        Err(Error::TextLimit { maximum: 5 })
    );
    assert_eq!(c.next("A", 999, 1, Some("xyz")), Ok(6));
    assert_eq!(c.next("B", 10, 1, None), Ok(10));
    assert_eq!(
        c.next("C", 20, 1, None),
        Err(Error::CountLimit { maximum: 2 })
    );
    assert_eq!((c.len(), c.retained_text_bytes()), (2, 5));
    assert!(c.reset("A"));
    assert!(!c.reset("missing"));
    assert_eq!((c.len(), c.retained_text_bytes()), (1, 1));
    assert_eq!(c.next("C", 20, 1, Some("")), Ok(20));
    assert_eq!(c.next("B", 999, 1, None), Ok(11));
    assert_eq!(c.next("C", 30, 1, Some("abc")), Ok(30));
    assert_eq!(c.retained_text_bytes(), 5);
    assert_eq!(c.next("C", 40, 1, None), Ok(40));
    assert_eq!(c.retained_text_bytes(), 2);
}

#[test]
fn identifiers_and_restart_tokens_keep_exact_unicode_identity() {
    let mut c = counters(6, 13);
    for (index, key) in ["a", "A", "é", "e\u{301}", "\0", ""].iter().enumerate() {
        assert_eq!(c.next(key, index as i64, 1, Some("")), Ok(index as i64));
    }
    assert_eq!(c.retained_text_bytes(), 8);
    assert_eq!(c.next("a", 10, 1, Some("🙂")), Ok(10));
    assert_eq!(c.retained_text_bytes(), 12);
    assert_eq!(
        c.next("a", 50, 1, Some("é🙂")),
        Err(Error::TextLimit { maximum: 13 })
    );
    assert_eq!(c.next("a", 999, 1, Some("🙂")), Ok(11));
    assert_eq!(c.next("A", 999, -1, Some("")), Ok(0));
}

#[test]
fn invocation_owner_can_span_documents_without_global_state() {
    let mut c = counters(1, 4);
    let values: Vec<_> = [2, 2]
        .into_iter()
        .map(|count| {
            (0..count)
                .map(|_| c.next("line", 1, 1, None).unwrap())
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(values, [vec![1, 2], vec![3, 4]]);
    let mut independent = counters(1, 4);
    assert_eq!(independent.next("line", 1, 1, None), Ok(1));
    assert_eq!(c.next("line", 99, -5, None), Ok(-1));
    assert!(independent.reset("line"));
    assert!(independent.is_empty());
}

#[test]
fn zero_count_and_text_limits_are_literal() {
    let mut no_keys = counters(0, 100);
    assert_eq!(
        no_keys.next("", 1, 1, None),
        Err(Error::CountLimit { maximum: 0 })
    );
    assert!(no_keys.is_empty());
    let mut no_text = counters(1, 0);
    assert_eq!(
        no_text.next("x", 1, 1, None),
        Err(Error::TextLimit { maximum: 0 })
    );
    assert_eq!(no_text.next("", 1, 1, None), Ok(1));
    assert_eq!(no_text.next("", 5, 1, Some("")), Ok(5));
    assert_eq!(
        no_text.next("", 9, 1, Some("x")),
        Err(Error::TextLimit { maximum: 0 })
    );
    assert_eq!(no_text.next("", 99, 1, Some("")), Ok(6));
}

#[test]
fn stateful_counter_is_not_registered_as_a_pure_scalar_builtin() {
    assert!(!functions::is_known("invocation_counter"));
    assert!(matches!(
        functions::call("invocation_counter", &[]),
        Err(functions::FunctionError::UnknownFunction(_))
    ));
}

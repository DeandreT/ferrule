use codegen_runtime::{InvocationCounterError, InvocationCounterLimits, InvocationCounters};

#[test]
fn generated_consumer_can_own_counter_state_without_runtime_parameters() {
    let mut counters = InvocationCounters::new(InvocationCounterLimits {
        max_counters: 1,
        max_text_bytes: 4,
    });
    assert_eq!(counters.next("line", 1, 1, None), Ok(1));
    assert_eq!(counters.next("line", 999, 2, None), Ok(3));
    assert_eq!(
        counters.next("other", 1, 1, None),
        Err(InvocationCounterError::CountLimit { maximum: 1 })
    );
    assert_eq!(counters.next("line", 999, 2, None), Ok(5));
    assert!(counters.reset("line"));
    assert_eq!(counters.next("line", 10, 1, None), Ok(10));
}

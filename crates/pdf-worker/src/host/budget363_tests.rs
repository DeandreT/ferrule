//! Pure fixed-budget observation regressions; no process, filesystem, or clock actor.

use super::{BudgetObservations, Failure, FailureKind};

fn observed(
    sequence: &[(FailureKind, &'static str)],
) -> (Vec<bool>, Vec<(FailureKind, &'static str)>) {
    let mut observations = BudgetObservations::default();
    let mut decisions = Vec::new();
    let mut accepted = Vec::new();
    for &(kind, operation) in sequence {
        let failure = Failure::new(kind, operation);
        let first = observations.first(&failure);
        eprintln!("budget363 original {failure:#?} first={first}");
        decisions.push(first);
        if first {
            accepted.push((kind, operation));
        }
    }
    (decisions, accepted)
}

#[test]
fn repeated_cancel_at_poll_verify_and_final_boundary_is_one_observation() {
    let cancel = (FailureKind::Cancelled, "cancellation observed");
    let (decisions, accepted) = observed(&[cancel, cancel, cancel]);
    assert_eq!(decisions, [true, false, false]);
    assert_eq!(accepted, [cancel]);
}

#[test]
fn repeated_timeout_at_terminal_boundaries_is_one_observation() {
    let timeout = (FailureKind::TimedOut, "work cutoff");
    let (decisions, accepted) = observed(&[timeout, timeout, timeout]);
    assert_eq!(decisions, [true, false, false]);
    assert_eq!(accepted, [timeout]);
}

#[test]
fn both_budget_kind_orders_retain_each_first_observation() {
    let cancel = (FailureKind::Cancelled, "cancellation observed");
    let timeout = (FailureKind::TimedOut, "work cutoff");
    for (first, second) in [(cancel, timeout), (timeout, cancel)] {
        let (decisions, accepted) = observed(&[first, second, first, second]);
        assert_eq!(decisions, [true, true, false, false]);
        assert_eq!(accepted, [first, second]);
    }
}

#[test]
fn io_primary_does_not_hide_first_later_cancel_or_distinct_closure_errors() {
    let capture = (FailureKind::Io, "capture read");
    let cancel = (FailureKind::Cancelled, "cancellation observed");
    let signal = (FailureKind::Io, "pidfd TERM");
    let wait = (FailureKind::Io, "own wait4 unresolved");
    let receipt = (FailureKind::Io, "receipt durable write");
    let (decisions, accepted) = observed(&[capture, cancel, cancel, signal, wait, receipt]);
    assert_eq!(decisions, [true, true, false, true, true, true]);
    assert_eq!(accepted, [capture, cancel, signal, wait, receipt]);
}

#[test]
fn nonbudget_repetitions_and_unknown_budget_operations_are_not_deduplicated() {
    let sequence = [
        (FailureKind::Io, "capture read"),
        (FailureKind::Io, "capture read"),
        (FailureKind::Receipt, "receipt byte cap"),
        (FailureKind::Receipt, "receipt byte cap"),
        (FailureKind::Cleanup, "cleanup intent cap"),
        (FailureKind::Cleanup, "cleanup intent cap"),
        (FailureKind::Cancelled, "different cancellation operation"),
        (FailureKind::Cancelled, "different cancellation operation"),
        (FailureKind::TimedOut, "different timeout operation"),
        (FailureKind::TimedOut, "different timeout operation"),
    ];
    let (decisions, accepted) = observed(&sequence);
    assert_eq!(decisions, [true; 10]);
    assert_eq!(accepted, sequence);
}

#[test]
fn observation_bits_are_per_job_and_do_not_replace_late_success_refusal() {
    let mut first_job = BudgetObservations::default();
    let cancel = Failure::new(FailureKind::Cancelled, "cancellation observed");
    assert!(first_job.first(&cancel));
    let mut instance_present = true;
    let late_check: Result<(), Failure> = Err(cancel.clone());
    if let Err(failure) = late_check {
        let first_observation = first_job.first(&failure);
        eprintln!("budget363 late-check original {failure:#?} first={first_observation}");
        assert!(!first_observation);
        instance_present = false;
    }
    assert!(!instance_present);
    let mut second_job = BudgetObservations::default();
    assert!(second_job.first(&cancel));
}

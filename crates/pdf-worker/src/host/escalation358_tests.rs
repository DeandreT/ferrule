//! Pure scheduling predicate controls only; no signal, PIDfd, wait or clock actor.
use super::*;

#[test]
fn escalation358_independent_exact_boundary_vectors() {
    // Independent literal expectations: the old one-second predicate must fail
    // the two 900ms vectors. No expected bit is computed from candidate code.
    let rows = [
        ("zero", 0u64, false, false),
        ("below", 899_999_999, false, false),
        ("exact", 900_000_000, false, true),
        ("above", 900_000_001, false, true),
        ("one-second", 1_000_000_000, false, true),
        ("late-stop", 5_000_000_000, false, true),
        ("already-zero", 0, true, false),
        ("already-exact", 900_000_000, true, false),
        ("already-late", 5_000_000_000, true, false),
    ];
    let actual = rows.map(|(key, nanos, attempted, expected)| {
        (
            key,
            nanos,
            attempted,
            expected,
            kill_due(Duration::from_nanos(nanos), attempted),
        )
    });
    eprintln!("358 complete independent boundary originals {actual:#?}");
    for (key, _, _, expected, observed) in actual {
        assert_eq!(observed, expected, "{key}");
    }
}

#[test]
fn escalation358_repeated_polls_preserve_single_attempt() {
    let cases = [
        (
            "approach",
            false,
            vec![0u64, 899_999_999, 900_000_000, 900_000_001, 1_000_000_000],
            vec![false, false, true, false, false],
        ),
        (
            "already-attempted",
            true,
            vec![0, 900_000_000, 5_000_000_000],
            vec![false, false, false],
        ),
        (
            "first-poll-late",
            false,
            vec![1_000_000_000, 5_000_000_000],
            vec![true, false],
        ),
    ];
    let mut actual = Vec::new();
    for (key, initial_attempted, nanos, expected) in cases {
        let mut attempted = initial_attempted;
        let observed = nanos
            .iter()
            .map(|n| {
                let due = kill_due(Duration::from_nanos(*n), attempted);
                if due {
                    attempted = true;
                }
                due
            })
            .collect::<Vec<_>>();
        actual.push((key, initial_attempted, nanos, expected, observed, attempted));
    }
    eprintln!("358 complete repeated-poll originals {actual:#?}");
    for (key, _, _, expected, observed, attempted) in actual {
        assert_eq!(observed, expected, "{key}");
        assert!(attempted);
    }
}

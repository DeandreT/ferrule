//! Sequence-valued computed keys (scope 6): each value of a `tokenize` key
//! contributes its own tuples, in sequence order, with empty and duplicate
//! tokens kept. The `tokens` and `tokens-second` rows of the frozen corpus
//! reproduce their tuples; unobserved sequence forms stay refused.

use super::controls::{AGGREGATE, COUNT};
use super::*;
use crate::{ExecutionBudgetKind, ExecutionBudgets, ExecutionPolicy, RestrictedPolicy};
use mapping::SequenceExpr;

/// `tokenize(input, delimiter)` with a fresh private item.
fn tokenize(keys: &mut Keys, input: NodeId, delimiter: &str) -> SequenceExpr {
    let delimiter = keys.constant(Value::String(delimiter.into()));
    let item = keys.field(&[]);
    SequenceExpr::Tokenize {
        input,
        delimiter,
        item,
    }
}

/// `tokenize(Codes, " ")` as MFD import lowers it: the input is guarded, so
/// an absent or nil field gives no tokens.
fn codes_tokens(keys: &mut Keys) -> SequenceExpr {
    let input = keys.guarded(&["Codes"], |_, field| field);
    tokenize(keys, input, " ")
}

fn sequence_project(
    context: Context,
    key_first: bool,
    key: fn(&mut Keys) -> SequenceExpr,
) -> Project {
    let mut keys = Keys::new();
    let key = Key::Sequence(key(&mut keys));
    fixture_project(
        keys.graph(),
        root_scope(context, plan(&key, "Number", key_first)),
    )
}

fn messages(project: &Project) -> Vec<String> {
    validate(project)
        .into_iter()
        .map(|issue| issue.message)
        .collect()
}

fn assert_refused(project: &Project, expected: &str) {
    let messages = messages(project);
    assert!(
        messages.iter().any(|message| message.contains(expected)),
        "expected `{expected}` in {messages:?}"
    );
}

#[test]
fn tokens_rows_reproduce_their_frozen_tuples_from_either_side() {
    let source = input(INPUT);
    for (id, key_first) in [("tokens", true), ("tokens-second", false)] {
        let case = expected_case(id);
        assert_eq!(case["native"], "observed", "{id}");
        assert_eq!(case["input"], "input.xml", "{id}");
        let project = sequence_project(Context::EachOrder, key_first, codes_tokens);
        assert_eq!(validate(&project), [], "{id}");
        let output = run_fixture(&project, &source).unwrap_or_else(|error| panic!("{id}: {error}"));
        assert_eq!(
            tuples(&output, Context::EachOrder),
            expected_tuples(id),
            "{id}"
        );
    }
}

/// The guard only restates what the sequence already does: an absent input
/// gives no tokens, so an unguarded key yields the same tuples.
#[test]
fn an_unguarded_tokenize_key_gives_the_same_tuples() {
    let source = input(INPUT);
    for (id, key_first) in [("tokens", true), ("tokens-second", false)] {
        let project = sequence_project(Context::EachOrder, key_first, |keys| {
            let codes = keys.field(&["Codes"]);
            tokenize(keys, codes, " ")
        });
        assert_eq!(validate(&project), [], "{id}");
        assert_eq!(
            tuples(&run_fixture(&project, &source).unwrap(), Context::EachOrder),
            expected_tuples(id),
            "{id}"
        );
    }
}

/// At the root a constant sequence is one key for the whole run: duplicate
/// tokens repeat their tuples and an empty token matches the empty number.
#[test]
fn a_root_sequence_key_keeps_duplicates_and_empty_tokens_in_order() {
    let source = input(INPUT);
    for (key_first, expected) in [
        (
            true,
            vec![
                ("C2", "Grace"),
                ("C2", "Hopper"),
                ("", "Blank"),
                ("C1", "Ada"),
                ("C2", "Grace"),
                ("C2", "Hopper"),
            ],
        ),
        (
            false,
            vec![
                ("C1", "Ada"),
                ("C2", "Grace"),
                ("C2", "Grace"),
                ("C2", "Hopper"),
                ("C2", "Hopper"),
                ("", "Blank"),
            ],
        ),
    ] {
        let project = sequence_project(Context::Root, key_first, |keys| {
            let codes = keys.constant(Value::String("C2,,C1,C2".into()));
            tokenize(keys, codes, ",")
        });
        assert_eq!(validate(&project), []);
        let expected = expected
            .into_iter()
            .map(|(number, name)| (number.to_string(), name.to_string()))
            .collect::<Vec<_>>();
        assert_eq!(
            tuples(&run_fixture(&project, &source).unwrap(), Context::Root),
            BTreeMap::from([("Hit".to_string(), expected)]),
            "key_first={key_first}"
        );
    }
}

/// A Null input gives the empty sequence and no tuples, while `""` gives one
/// empty token.
#[test]
fn an_empty_sequence_yields_no_tuples() {
    let source = input(INPUT);
    for (value, expected) in [
        (Value::Null, vec![]),
        (
            Value::String(String::new()),
            vec![(String::new(), "Blank".to_string())],
        ),
    ] {
        let mut keys = Keys::new();
        let input = keys.constant(value.clone());
        let key = Key::Sequence(tokenize(&mut keys, input, " "));
        let project = fixture_project(
            keys.graph(),
            root_scope(Context::Root, plan(&key, "Number", true)),
        );
        assert_eq!(validate(&project), []);
        assert_eq!(
            tuples(&run_fixture(&project, &source).unwrap(), Context::Root),
            BTreeMap::from([("Hit".to_string(), expected)]),
            "{value:?}"
        );
    }
}

/// Joined aggregates over a sequence key reduce every token's tuples, in
/// the order the key's place in the plan gives them.
#[test]
fn joined_aggregates_reduce_sequence_key_tuples() {
    let source = input(INPUT);
    for (id, key_first) in [("tokens", true), ("tokens-second", false)] {
        let mut project =
            super::controls::aggregate_project(|keys| keys.constant(Value::Null), true, true);
        let mut keys = Keys {
            nodes: std::mem::take(&mut project.graph.nodes),
        };
        // Each aggregate owns its own sequence and item.
        for consumer in [AGGREGATE, COUNT] {
            let sequence = codes_tokens(&mut keys);
            let joined = plan(&Key::Sequence(sequence), "Number", key_first);
            if let Some(Node::JoinAggregate { plan, .. }) = keys.nodes.get_mut(&consumer) {
                *plan = joined;
            }
        }
        project.graph = keys.graph();
        assert_eq!(validate(&project), [], "{id}");
        let output = run_fixture(&project, &source).unwrap();
        let summary = output
            .field("Order")
            .and_then(Instance::as_repeated)
            .expect("one target order per source order")
            .iter()
            .map(|order| {
                let count = match order.field("Count").and_then(Instance::as_scalar) {
                    Some(Value::Int(count)) => *count,
                    other => panic!("unexpected count {other:?}"),
                };
                (
                    order.field("Id").map(text).expect("order id"),
                    order.field("Names").map(text).unwrap_or_default(),
                    count,
                )
            })
            .collect::<Vec<_>>();
        let expected = expected_tuples(id)
            .into_iter()
            .map(|(order, rows)| {
                let names = rows
                    .iter()
                    .map(|(_, name)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                (order, names, rows.len() as i64)
            })
            .collect::<Vec<_>>();
        assert_eq!(summary, expected, "{id}");
    }
}

/// Every examined pairing of a token with a customer is one join row, and
/// the tokens count toward the generated-sequence limit.
#[test]
fn sequence_keys_charge_join_rows_and_the_sequence_limit() {
    let source = input(INPUT);
    let project = sequence_project(Context::Root, true, |keys| {
        let codes = keys.constant(Value::String("C2 C1 C2".into()));
        tokenize(keys, codes, " ")
    });
    let run_under = |join_rows| {
        let policy = ExecutionPolicy::Restricted(RestrictedPolicy::new(ExecutionBudgets::new(
            1_000_000, 64, join_rows,
        )));
        let parameters = key_parameters();
        let execution = ExecutionContext::new(Path::new("mapping.json"))
            .with_parameters(&parameters)
            .with_policy(&policy);
        run_with_context(&project, &source, &execution)
    };
    // Three tokens times eight customers.
    assert_eq!(
        run_under(24).unwrap(),
        run_fixture(&project, &source).unwrap()
    );
    assert_eq!(
        run_under(23).unwrap_err(),
        EngineError::ExecutionBudgetExceeded {
            kind: ExecutionBudgetKind::JoinRows,
            limit: 23
        }
    );

    let project = sequence_project(Context::Root, true, |keys| {
        let codes = keys.constant(Value::String(" ".repeat(1_000_000)));
        tokenize(keys, codes, " ")
    });
    assert!(
        matches!(
            run_fixture(&project, &source),
            Err(EngineError::GeneratedSequenceTooLarge {
                requested: 1_000_001,
                ..
            })
        ),
        "the tokens exceed the generated-sequence limit"
    );
}

#[test]
fn sequence_keys_other_than_tokenize_are_refused() {
    let project = sequence_project(Context::EachOrder, true, |keys| {
        let codes = keys.field(&["Codes"]);
        let length = keys.constant(Value::Int(2));
        let item = keys.field(&[]);
        SequenceExpr::TokenizeByLength {
            input: codes,
            length,
            item,
        }
    });
    assert_refused(
        &project,
        "computed key 1 is a `tokenize_by_length` sequence; a sequence-valued key admits only tokenize",
    );
}

/// A tokenize input follows the scalar key rules: at the root, the codes of
/// every order would be a sequence of sequences, and a refused node kind
/// stays refused below the sequence.
#[test]
fn sequence_key_inputs_follow_the_scalar_key_rules() {
    let root = sequence_project(Context::Root, true, |keys| {
        let codes = keys.field(&["Order", "Codes"]);
        tokenize(keys, codes, " ")
    });
    assert_refused(
        &root,
        "`Order/Codes` crosses or ends at a repeating source level",
    );
    let positioned = sequence_project(Context::EachOrder, true, |keys| {
        let position = keys.add(Node::Position {
            collection: path(&["Order"]),
        });
        let text = keys.call("string", vec![position]);
        tokenize(keys, text, " ")
    });
    assert_refused(&positioned, "which a computed key does not admit");
}

/// The key's item is private to the join: no other expression reads it, no
/// other sequence owns it, and the key's own input cannot read it.
#[test]
fn a_sequence_key_item_stays_private_to_its_join() {
    let item_of = |project: &Project| {
        project.root.children[0].children[0]
            .join()
            .and_then(|(_, plan)| plan.computed_source())
            .and_then(|computed| computed.as_sequence())
            .map(SequenceExpr::item)
            .expect("a sequence key")
    };

    let mut read = sequence_project(Context::EachOrder, true, codes_tokens);
    let item = item_of(&read);
    read.root.children[0].children[0].bindings[0].node = item;
    assert_refused(
        &read,
        &format!("references generated sequence item node {item} outside its owning context"),
    );

    let mut shared = sequence_project(Context::EachOrder, true, codes_tokens);
    let item = item_of(&shared);
    let delimiter = 9_000;
    shared.graph.nodes.insert(
        delimiter,
        Node::Const {
            value: Value::String(",".into()),
        },
    );
    shared.root.children[0].children.push(Scope {
        target_field: "Match".into(),
        iteration: ScopeIteration::Sequence(SequenceExpr::Tokenize {
            input: ORDER_ID,
            delimiter,
            item,
        }),
        ..Scope::default()
    });
    assert_refused(
        &shared,
        &format!("sequence item node {item} is already owned"),
    );

    let own = sequence_project(Context::EachOrder, true, |keys| {
        let delimiter = keys.constant(Value::String(" ".into()));
        let item = keys.field(&[]);
        let input = keys.call("string", vec![item]);
        SequenceExpr::Tokenize {
            input,
            delimiter,
            item,
        }
    });
    let item = item_of(&own);
    assert_refused(
        &own,
        &format!("references generated sequence item node {item} outside its owning context"),
    );
}

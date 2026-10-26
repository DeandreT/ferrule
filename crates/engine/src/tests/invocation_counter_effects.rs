use super::*;
use functions::{InvocationCounterLimits, InvocationCounters};

#[test]
fn both_earlier_operand_effects_survive_restart_refusal() {
    assert_retained_effects("restart", "string or missing", 3);
}

#[test]
fn nested_counter_effect_survives_a_later_outer_operand_refusal() {
    assert_retained_effects("step", "integer", 2);
}

fn assert_retained_effects(operand: &'static str, expected: &'static str, next: i64) {
    for outer_key in ["line", "outer"] {
        let restart_failure = operand == "restart";
        let graph = Graph {
            nodes: [
                (
                    1,
                    Node::InvocationCounter {
                        key: outer_key.into(),
                        start: 2,
                        step: if restart_failure { 2 } else { 4 },
                        restart: restart_failure.then_some(4),
                    },
                ),
                (
                    2,
                    Node::InvocationCounter {
                        key: "line".into(),
                        start: 3,
                        step: 3,
                        restart: None,
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::Bool(true),
                    },
                ),
            ]
            .into(),
        };
        let functions = BTreeMap::new();
        let reported = Cell::new(false);
        let counters = RefCell::new(InvocationCounters::new(InvocationCounterLimits {
            max_counters: 2,
            max_text_bytes: 32,
        }));
        let program = EvalProgram::new(&graph, &functions, None, &reported)
            .with_invocation_counters(Some(&counters));
        assert_eq!(
            eval_expr(program, 1, &[], &[], &mut HashSet::new()),
            Err(EngineError::InvocationCounterOperand {
                node: 1,
                operand,
                expected,
                found: "bool"
            })
        );
        assert_eq!(counters.borrow().len(), 1);
        // Restart failure retains both calls of the shared operand node.
        assert_eq!(counters.borrow_mut().next("line", 1, 1, None), Ok(next));
        if outer_key == "outer" {
            assert_eq!(counters.borrow_mut().next("outer", 9, 1, None), Ok(9));
        }
    }
}

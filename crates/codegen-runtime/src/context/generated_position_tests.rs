use super::*;

#[test]
fn generated_origins_are_identity_bound_and_immutable() {
    let source = Instance::Group(Vec::new().into());
    let root = ScopeContext::new(&source);
    assert!(matches!(
        root.generated_position(9, 10),
        Err(RuntimeError::GeneratedPositionUnavailable { node: 9, item: 10 })
    ));
    let values = GeneratedItems::with_owner(vec![Value::Int(8), Value::Int(8)], 10);
    let outer = root.generated_items(&values);
    let selected = outer[1].with_compact_last_position(1);
    assert_eq!(selected.position(&[]), 1);
    assert_eq!(selected.generated_position(9, 10).unwrap(), Value::Int(2));
    let inner_values = GeneratedItems::with_owner(vec![Value::Int(0)], 20);
    let inner = selected.generated_items(&inner_values);
    assert_eq!(inner[0].generated_position(9, 10).unwrap(), Value::Int(2));
    assert_eq!(inner[0].generated_position(9, 20).unwrap(), Value::Int(1));
    assert_eq!(outer[0].generated_position(9, 10).unwrap(), Value::Int(1));
    let unowned = GeneratedItems::new(vec![Value::Int(1)]);
    assert!(
        root.generated_items(&unowned)[0]
            .generated_position(9, 10)
            .is_err()
    );
}

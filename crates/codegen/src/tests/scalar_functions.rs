use mapping::Node;

use super::*;

#[test]
fn lowers_exact_numeric_and_delay_scalar_calls() {
    let cases = [
        ("trim", ScalarFunction::Trim, vec![20]),
        ("upper", ScalarFunction::Upper, vec![20]),
        ("lower", ScalarFunction::Lower, vec![20]),
        ("is_numeric", ScalarFunction::IsNumeric, vec![20]),
        ("to_number", ScalarFunction::ToNumber, vec![20]),
        ("format_number", ScalarFunction::FormatNumber, vec![10, 20]),
        (
            "delay_passthrough",
            ScalarFunction::DelayPassthrough,
            vec![20, 10],
        ),
    ];

    for (index, (name, expected, args)) in cases.into_iter().enumerate() {
        let node = 40 + index as u32;
        let mut project = supported_project();
        project.graph.nodes.insert(
            node,
            Node::Call {
                function: name.to_string(),
                args: args.clone(),
            },
        );
        project.root.bindings[0].node = node;

        let program = lower(&project).expect("the exact scalar call is portable");
        let expression = program
            .expressions
            .iter()
            .find(|expression| expression.id == node)
            .expect("reachable call is retained");
        assert_eq!(
            expression.expression,
            Expression::Call {
                function: expected,
                args,
            }
        );
    }
}

#[test]
fn newly_supported_names_are_closed_and_canonical() {
    for (name, expected) in [
        ("trim", ScalarFunction::Trim),
        ("upper", ScalarFunction::Upper),
        ("lower", ScalarFunction::Lower),
        ("is_numeric", ScalarFunction::IsNumeric),
        ("to_number", ScalarFunction::ToNumber),
        ("format_number", ScalarFunction::FormatNumber),
        ("delay_passthrough", ScalarFunction::DelayPassthrough),
    ] {
        assert_eq!(ScalarFunction::from_name(name), Some(expected));
        assert_eq!(expected.as_str(), name);
        assert!(SUPPORTED_SCALAR_CALLS.contains(&expected));
    }

    assert_eq!(ScalarFunction::from_name("sleep"), None);
}

#[test]
fn lowers_datetime_composition_calls_without_backend_specific_state() {
    for (index, (name, expected, args)) in [
        (
            "datetime_from_date_and_time",
            ScalarFunction::DatetimeFromDateAndTime,
            vec![10, 20],
        ),
        (
            "datetime_from_parts",
            ScalarFunction::DatetimeFromParts,
            vec![10, 20, 10],
        ),
        ("coerce_datetime", ScalarFunction::CoerceDatetime, vec![20]),
    ]
    .into_iter()
    .enumerate()
    {
        let node = 60 + index as u32;
        let mut project = supported_project();
        project.graph.nodes.insert(
            node,
            Node::Call {
                function: name.to_string(),
                args: args.clone(),
            },
        );
        project.root.bindings[0].node = node;

        let program = lower(&project).expect("the datetime composition call is portable");
        let expression = program
            .expressions
            .iter()
            .find(|expression| expression.id == node)
            .expect("reachable call is retained");
        assert_eq!(
            expression.expression,
            Expression::Call {
                function: expected,
                args,
            }
        );
    }
}

#[test]
fn lowers_datetime_picture_calls_without_backend_specific_state() {
    for (index, (name, expected)) in [
        ("parse_date", ScalarFunction::ParseDate),
        ("parse_datetime", ScalarFunction::ParseDatetime),
        ("parse_time", ScalarFunction::ParseTime),
    ]
    .into_iter()
    .enumerate()
    {
        let node = 70 + index as u32;
        let mut project = supported_project();
        project.graph.nodes.insert(
            node,
            Node::Call {
                function: name.to_string(),
                args: vec![10, 20],
            },
        );
        project.root.bindings[0].node = node;

        let program = lower(&project).expect("the datetime picture call is portable");
        let expression = program
            .expressions
            .iter()
            .find(|expression| expression.id == node)
            .expect("reachable call is retained");
        assert_eq!(
            expression.expression,
            Expression::Call {
                function: expected,
                args: vec![10, 20],
            }
        );
        assert_eq!(ScalarFunction::from_name(name), Some(expected));
        assert_eq!(expected.as_str(), name);
    }
}

use mapping::Node;

use super::*;

#[test]
fn lowers_exact_numeric_and_delay_scalar_calls() {
    let cases = [
        ("trim", ScalarFunction::Trim, vec![20]),
        ("left", ScalarFunction::Left, vec![20, 10]),
        ("right", ScalarFunction::Right, vec![20, 10]),
        ("upper", ScalarFunction::Upper, vec![20]),
        ("lower", ScalarFunction::Lower, vec![20]),
        ("matches", ScalarFunction::Matches, vec![20, 20]),
        ("is_numeric", ScalarFunction::IsNumeric, vec![20]),
        ("to_number", ScalarFunction::ToNumber, vec![20]),
        ("format_number", ScalarFunction::FormatNumber, vec![10, 20]),
        (
            "substitute_missing_with_xml_nil",
            ScalarFunction::SubstituteMissingWithXmlNil,
            vec![20],
        ),
        ("get_fileext", ScalarFunction::GetFileext, vec![20]),
        ("weekday", ScalarFunction::Weekday, vec![20]),
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
        ("left", ScalarFunction::Left),
        ("right", ScalarFunction::Right),
        ("upper", ScalarFunction::Upper),
        ("lower", ScalarFunction::Lower),
        ("matches", ScalarFunction::Matches),
        ("is_numeric", ScalarFunction::IsNumeric),
        ("to_number", ScalarFunction::ToNumber),
        ("format_number", ScalarFunction::FormatNumber),
        (
            "substitute_missing_with_xml_nil",
            ScalarFunction::SubstituteMissingWithXmlNil,
        ),
        ("get_fileext", ScalarFunction::GetFileext),
        ("weekday", ScalarFunction::Weekday),
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

#[test]
fn lowers_datetime_arithmetic_and_edifact_calls_without_backend_specific_state() {
    for (index, (name, expected, args)) in [
        (
            "datetime_add",
            ScalarFunction::DatetimeAdd,
            vec![10, 20, 10],
        ),
        (
            "edifact_to_datetime",
            ScalarFunction::EdifactToDatetime,
            vec![10, 20],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let node = 80 + index as u32;
        let mut project = supported_project();
        project.graph.nodes.insert(
            node,
            Node::Call {
                function: name.to_string(),
                args: args.clone(),
            },
        );
        project.root.bindings[0].node = node;

        let program = lower(&project).expect("the date-time scalar call is portable");
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
        assert_eq!(ScalarFunction::from_name(name), Some(expected));
        assert_eq!(expected.as_str(), name);
    }
}

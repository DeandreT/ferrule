use std::collections::BTreeSet;

use ir::Value;

use super::{
    BuiltinArity, BuiltinCategory, BuiltinExposure, BuiltinParameter, ScalarDomain, catalog, find,
};
use crate::{BUILTIN_NAMES, FunctionError, call, is_known};

fn definition(name: &str) -> &'static super::BuiltinDefinition {
    let Some(definition) = find(name) else {
        panic!("catalog does not contain `{name}`");
    };
    definition
}

#[test]
fn catalog_names_are_unique_stable_and_documented() {
    let mut names = BTreeSet::new();

    for definition in catalog() {
        assert!(
            names.insert(definition.native_name),
            "duplicate native name `{}`",
            definition.native_name
        );
        assert!(!definition.display_name.trim().is_empty());
        assert!(!definition.documentation.trim().is_empty());
        assert!(
            definition
                .native_name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
            "native name is not stable snake_case: `{}`",
            definition.native_name
        );
    }
}

#[test]
fn catalog_signatures_have_valid_shapes() {
    for definition in catalog() {
        let arity = definition.arity;
        assert!(arity.accepts(arity.minimum()));

        if let Some(maximum) = arity.maximum() {
            assert!(maximum >= arity.minimum());
            assert_eq!(
                definition.parameters.len(),
                maximum,
                "`{}` does not describe every positional parameter",
                definition.native_name
            );
            assert!(!arity.accepts(maximum.saturating_add(1)));
        } else {
            let Some(step) = arity.step() else {
                panic!("variadic `{}` has no step", definition.native_name);
            };
            assert!(step > 0);
            assert!(definition.parameters.len() >= step);
            assert!(arity.accepts(arity.minimum().saturating_add(step)));
        }
    }

    assert!(std::panic::catch_unwind(|| BuiltinArity::range(2, 1)).is_err());
    assert!(std::panic::catch_unwind(|| BuiltinArity::variadic(0, 0)).is_err());
}

#[test]
fn catalog_and_dispatch_accept_the_same_arities() {
    for definition in catalog() {
        for count in 0..=12 {
            let arguments = vec![Value::Null; count];
            let result = call(definition.native_name, &arguments);

            if definition.accepts_arity(count) {
                assert!(
                    !matches!(
                        result,
                        Err(FunctionError::UnknownFunction(_) | FunctionError::ArityMismatch { .. })
                    ),
                    "`{}` rejected catalog-supported arity {count}: {result:?}",
                    definition.native_name
                );
            } else {
                assert_eq!(
                    result,
                    Err(FunctionError::ArityMismatch {
                        function: definition.native_name,
                        expected: definition.arity.minimum(),
                        got: count,
                    }),
                    "`{}` accepted catalog-rejected arity {count}",
                    definition.native_name
                );
            }
        }
    }
}

#[test]
fn unknown_names_are_rejected_consistently() {
    const UNKNOWN: &str = "not_a_ferrule_builtin";

    assert!(find(UNKNOWN).is_none());
    assert!(!is_known(UNKNOWN));
    assert_eq!(
        call(UNKNOWN, &[]),
        Err(FunctionError::UnknownFunction(UNKNOWN.to_string()))
    );
}

#[test]
fn authoring_names_are_derived_from_catalog_exposure() {
    let expected = catalog()
        .iter()
        .filter(|definition| definition.exposure == BuiltinExposure::Authoring)
        .map(|definition| definition.native_name)
        .collect::<Vec<_>>();
    let actual = BUILTIN_NAMES.into_iter().copied().collect::<Vec<_>>();

    assert_eq!(actual, expected);
    assert!(!actual.contains(&"json_parse_field"));
}

#[test]
fn variadic_arithmetic_documentation_describes_more_than_binary_calls() {
    assert_eq!(
        definition("add").documentation,
        "Adds two or more numeric values."
    );
    assert_eq!(
        definition("subtract").documentation,
        "Subtracts each following numeric value from the first."
    );
    assert_eq!(
        definition("multiply").documentation,
        "Multiplies two or more numeric values."
    );
}

#[test]
fn representative_metadata_covers_each_authoring_domain() {
    assert_metadata(
        "and",
        BuiltinCategory::Boolean,
        &[BuiltinParameter {
            name: "left",
            domain: ScalarDomain::Boolean,
        }],
        ScalarDomain::Boolean,
        BuiltinExposure::Authoring,
    );
    assert_metadata(
        "concat",
        BuiltinCategory::String,
        &[BuiltinParameter {
            name: "value",
            domain: ScalarDomain::Any,
        }],
        ScalarDomain::String,
        BuiltinExposure::Authoring,
    );
    assert_metadata(
        "add",
        BuiltinCategory::Numeric,
        &[BuiltinParameter {
            name: "left",
            domain: ScalarDomain::Numeric,
        }],
        ScalarDomain::Numeric,
        BuiltinExposure::Authoring,
    );
    assert_metadata(
        "datetime_add",
        BuiltinCategory::DateTime,
        &[BuiltinParameter {
            name: "value",
            domain: ScalarDomain::DateTime,
        }],
        ScalarDomain::DateTime,
        BuiltinExposure::Authoring,
    );
    assert_metadata(
        "resolve_filepath",
        BuiltinCategory::Path,
        &[BuiltinParameter {
            name: "base",
            domain: ScalarDomain::Path,
        }],
        ScalarDomain::Path,
        BuiltinExposure::Authoring,
    );
    assert_metadata(
        "json_parse_field",
        BuiltinCategory::Json,
        &[BuiltinParameter {
            name: "input",
            domain: ScalarDomain::JsonText,
        }],
        ScalarDomain::Any,
        BuiltinExposure::Internal,
    );
    assert_metadata(
        "flextext_parse_field",
        BuiltinCategory::FlexText,
        &[BuiltinParameter {
            name: "input",
            domain: ScalarDomain::FlexText,
        }],
        ScalarDomain::Any,
        BuiltinExposure::Internal,
    );

    let generator = definition("create_guid");
    assert_eq!(generator.category, BuiltinCategory::Generator);
    assert_eq!(generator.return_domain, ScalarDomain::String);
    assert!(generator.arity.is_fixed());
    assert!(!generator.pure);
    assert!(!generator.deterministic);
}

fn assert_metadata(
    name: &str,
    category: BuiltinCategory,
    first_parameters: &[BuiltinParameter],
    return_domain: ScalarDomain,
    exposure: BuiltinExposure,
) {
    let definition = definition(name);

    assert_eq!(definition.category, category);
    assert_eq!(
        definition.parameters.get(..first_parameters.len()),
        Some(first_parameters)
    );
    assert_eq!(definition.return_domain, return_domain);
    assert_eq!(definition.exposure, exposure);
    assert!(definition.pure);
    assert!(definition.deterministic);
}

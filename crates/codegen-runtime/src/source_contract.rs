use crate::{
    ClosedPrimarySourcePlan, ExecutionContext, RuntimeError, ScopeContext, SourceContractReason,
    SourceContractViolation,
};

/// Counts only; no host-controlled names or payloads enter the error.
pub fn validate_source_contract_bindings(
    extras: usize,
    parameters: usize,
) -> Result<(), RuntimeError> {
    let reason = if extras != 0 {
        SourceContractReason::UnexpectedBinding
    } else if parameters != 0 {
        SourceContractReason::UnexpectedParameter
    } else {
        return Ok(());
    };
    Err(RuntimeError::SourceContractViolation(
        SourceContractViolation {
            path: Vec::new(),
            reason,
        },
    ))
}

impl ScopeContext<'_> {
    /// Called only at invocation roots, before failures, loaders or expressions.
    pub fn validate_closed_primary_source(
        &self,
        plan: &ClosedPrimarySourcePlan,
    ) -> Result<(), RuntimeError> {
        let (source, extras, parameters) = self.source_contract_input();
        plan.validate_input(source, extras, parameters)
            .map_err(RuntimeError::SourceContractViolation)
    }
}

pub(crate) fn parameter_count(execution: Option<ExecutionContext<'_>>) -> usize {
    execution.map_or(0, ExecutionContext::runtime_parameter_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Instance, ScalarType, SchemaNode, Value, repeated, scalar};

    fn group<const N: usize>(fields: [(&str, Instance); N]) -> Instance {
        crate::group(fields.map(|(name, value)| (name.to_owned(), value)))
    }

    fn plan() -> ClosedPrimarySourcePlan {
        ClosedPrimarySourcePlan::compile(&SchemaNode::group(
            "Input",
            vec![
                SchemaNode::scalar("Text", ScalarType::String).nillable(),
                SchemaNode::group(
                    "Row",
                    vec![SchemaNode::group("Cell", Vec::new()).repeating()],
                )
                .repeating(),
            ],
        ))
        .unwrap()
    }
    fn reason(value: &Instance) -> SourceContractReason {
        let error = ScopeContext::new(value)
            .validate_closed_primary_source(&plan())
            .unwrap_err();
        match error {
            RuntimeError::SourceContractViolation(error) => error.reason,
            _ => panic!("wrong error"),
        }
    }
    #[test]
    fn source_contract_raw_shape_and_scalar_domains() {
        let valid = group([
            ("Text", scalar(Value::xml_nil())),
            ("Row", repeated([group([("Cell", repeated([]))])])),
        ]);
        ScopeContext::new(&valid)
            .validate_closed_primary_source(&plan())
            .unwrap();
        assert_eq!(
            reason(&group([("Row", repeated([group([])]))])),
            SourceContractReason::MissingRepeatedSlot
        );
        assert_eq!(
            reason(&group([("Row", scalar(Value::Null))])),
            SourceContractReason::WrongShape
        );
        assert_eq!(
            reason(&group([
                ("Row", repeated([])),
                ("Text", scalar(Value::json_null()))
            ])),
            SourceContractReason::WrongScalarDomain
        );
        assert_eq!(
            reason(&group([
                ("Row", repeated([])),
                ("Text", scalar(Value::Int(7)))
            ])),
            SourceContractReason::WrongScalarDomain
        );
        assert_eq!(
            reason(&group([
                ("Row", repeated([])),
                ("private-host-name", scalar(Value::String("private".into())))
            ])),
            SourceContractReason::UnexpectedField
        );
        let duplicate = Instance::Group(
            vec![("Row".into(), repeated([])), ("Row".into(), repeated([]))].into(),
        );
        assert_eq!(reason(&duplicate), SourceContractReason::DuplicateField);
        assert_eq!(
            reason(&Instance::MappedSequence(Vec::new())),
            SourceContractReason::WrongShape
        );
    }
    #[test]
    fn source_contract_binding_counts_precede_input_and_do_not_expose_values() {
        use std::path::Path;
        let mut parameters = crate::RuntimeParameters::new();
        parameters
            .insert("private", Value::String("secret".into()))
            .unwrap();
        let execution = ExecutionContext::new(Path::new("mapping")).with_parameters(&parameters);
        let source = scalar(Value::String("unscanned".into()));
        let inputs = [crate::NamedInput {
            name: "unknown",
            instance: &source,
        }];
        let error =
            ScopeContext::with_named_inputs_and_execution_context(&source, &inputs, &execution)
                .validate_closed_primary_source(&plan())
                .unwrap_err();
        assert_eq!(
            error,
            RuntimeError::SourceContractViolation(SourceContractViolation {
                path: Vec::new(),
                reason: SourceContractReason::UnexpectedBinding
            })
        );
        let error = ScopeContext::with_execution_context(&source, &execution)
            .validate_closed_primary_source(&plan())
            .unwrap_err();
        assert_eq!(
            error,
            RuntimeError::SourceContractViolation(SourceContractViolation {
                path: Vec::new(),
                reason: SourceContractReason::UnexpectedParameter
            })
        );
        assert!(!format!("{error:?}").contains("secret"));
    }
    #[test]
    fn source_contract_budgets_schema_slots_per_actual_occurrence() {
        let fields = (0..2000)
            .map(|i| SchemaNode::scalar(format!("F{i:04}"), ScalarType::String))
            .collect();
        let source = SchemaNode::group("Input", vec![SchemaNode::group("Row", fields).repeating()]);
        let plan = ClosedPrimarySourcePlan::compile(&source).unwrap();
        let input = group([("Row", repeated((0..499).map(|_| group([]))))]);
        plan.validate_input(&input, 0, 0).unwrap();
        let input = group([("Row", repeated((0..500).map(|_| group([]))))]);
        assert_eq!(
            plan.validate_input(&input, 0, 0).unwrap_err().reason,
            SourceContractReason::ResourceLimit
        );
    }
}

use crate::{Program, X12BoundaryOptions, X12BoundarySide, X12EnvelopeProfile};
use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::FormatOptions;

/// The physical source and its explicit native decoder are separate from the
/// lexical message schema consumed by the mapping body.
pub struct NativeX12Source<'a> {
    pub schema: &'a SchemaNode,
    pub options: &'a FormatOptions,
    pub message: &'a [String],
    pub encoding: mapping::X12SourceEncoding,
    /// Select the original structural parser separately from the legacy lexical
    /// view. Absence preserves the previously supported source parser contract.
    pub structure: Option<&'a mapping::X12SourceStructure>,
}

/// Constructed only after the complete native source and static-scope proof.
/// This is not a Basic permission and never rewrites an expression to a literal.
pub(crate) struct NativeX12RootAdmission {
    fields: std::collections::BTreeSet<mapping::NodeId>,
}
impl NativeX12RootAdmission {
    pub(crate) fn contains(&self, node: mapping::NodeId) -> bool {
        self.fields.contains(&node)
    }
}

fn native_root_admission(
    program: &Program,
    source: &NativeX12Source<'_>,
) -> Result<NativeX12RootAdmission, MapForceX12SourceError> {
    use crate::Expression;
    use MapForceX12SourceError::Binding;
    prepare_native_x12_source(program, source)?;
    if !program.extra_sources.is_empty()
        || !program.user_functions.is_empty()
        || !program.failure_rules.is_empty()
        || !program.filter_map_v1_sequences().is_empty()
    {
        return Err(Binding(
            "nullable native messages require one source and no isolated functions",
        ));
    }
    fn check_scope(
        target: &crate::TargetScope,
        depth: usize,
    ) -> Result<(), MapForceX12SourceError> {
        use crate::{IterationOutput, IterationSource, TargetConstruction};
        if depth > 128
            || !matches!(
                target.construction,
                TargetConstruction::Group | TargetConstruction::Scalar { .. }
            )
        {
            return Err(Binding(
                "requires bounded ordinary group/scalar target scopes",
            ));
        }
        if let Some(iteration) = &target.iteration
            && (!matches!(iteration.input(), IterationSource::Source(_))
                || iteration.output() != IterationOutput::Repeated
                || iteration.grouping().is_some()
                || iteration.post_group_filter().is_some()
                || iteration.sort().is_some()
                || !iteration.windows().is_empty())
        {
            return Err(Binding(
                "only ordinary repeated source scopes with optional filters are admitted",
            ));
        }
        for child in &target.children {
            check_scope(child, depth + 1)?;
        }
        Ok(())
    }
    check_scope(&program.root, 0)?;
    for target in &program.extra_targets {
        check_scope(&target.root, 0)?;
    }
    let mut fields = std::collections::BTreeSet::new();
    for node in &program.expressions {
        let field = match &node.expression {
            Expression::OwnedSourceField(field) => field,
            Expression::SourceField { .. }
            | Expression::Const { .. }
            | Expression::RuntimeValue { .. }
            | Expression::RuntimeParameter { .. }
            | Expression::Call { .. }
            | Expression::If { .. }
            | Expression::Raise { .. }
            | Expression::ValueMap { .. }
            | Expression::InvocationCounter { .. }
            | Expression::Aggregate { .. }
            | Expression::CollectionFind { .. } => continue,
            _ => {
                return Err(Binding(
                    "expression is outside the native nullable scalar profile",
                ));
            }
        };
        if field.source.owner != mapping::SourceOwner::Primary
            || !field.frame.as_ref().is_some_and(Vec::is_empty)
            || field.source.path.is_empty()
            || field.source.path.len() > 64
            || !ir::primary_root_scalar_path_is_valid(
                &field
                    .source
                    .path
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )
        {
            return Err(Binding(
                "native message read requires an exact logical primary/root scalar",
            ));
        }
        let mut current = &program.source;
        for part in &field.source.path {
            if current.repeating
                || current.dynamic_fields().is_some()
                || current.recursive_ref.is_some()
            {
                return Err(Binding("native message scalar crosses an unproved owner"));
            }
            current = current
                .child(part)
                .ok_or(Binding("native message scalar is absent from its schema"))?;
        }
        if current.repeating || !matches!(current.kind, SchemaKind::Scalar { .. }) {
            return Err(Binding("native message read must end at a singular scalar"));
        }
        fields.insert(node.id);
    }
    // An inherited Source filter may be admitted, but that must not erase a
    // second, expression-local aggregate context. Keep such owned consumers
    // closed rather than relying on the first stored closure reason.
    let expressions = program
        .expressions
        .iter()
        .map(|node| (node.id, &node.expression))
        .collect::<std::collections::BTreeMap<_, _>>();
    for node in &program.expressions {
        if !matches!(
            node.expression,
            Expression::Aggregate { .. } | Expression::CollectionFind { .. }
        ) {
            continue;
        }
        let mut pending = crate::validate::graph_dependencies::of(&node.expression);
        let mut seen = std::collections::BTreeSet::new();
        while let Some(input) = pending.pop() {
            if fields.contains(&input) {
                return Err(Binding(
                    if matches!(node.expression, Expression::CollectionFind { .. }) {
                        "native root reads are not admitted inside find-owned expression contexts"
                    } else {
                        "native root reads are not admitted inside aggregate-owned expression contexts"
                    },
                ));
            }
            if seen.insert(input)
                && let Some(expression) = expressions.get(&input)
            {
                pending.extend(crate::validate::graph_dependencies::of(expression));
            }
        }
    }
    Ok(NativeX12RootAdmission { fields })
}

pub(crate) fn validate_native_x12_program(
    program: &Program,
    source: &NativeX12Source<'_>,
) -> Result<(), crate::ProgramValidationError> {
    let admission = native_root_admission(program, source).map_err(|error| {
        crate::ProgramValidationError::InvalidNativeX12MessageProfile {
            reason: error.to_string(),
        }
    })?;
    crate::validate::validate_native_x12_message_program(program, &admission)
}

/// Explicit native admission; ordinary and Basic validators retain their gates.
pub fn validate_native_x12_nullable_scalars(
    program: &Program,
    source: &NativeX12Source<'_>,
) -> Result<(), MapForceX12SourceError> {
    validate_native_x12_program(program, source).map_err(MapForceX12SourceError::Program)
}

pub fn prepare_native_x12_source(
    program: &Program,
    source: &NativeX12Source<'_>,
) -> Result<String, MapForceX12SourceError> {
    let boundary =
        prepare_mapforce_x12_source(program, source.schema, source.options, source.message)?;
    let mut descriptor = serde_json::json!({"boundary":boundary,"source_encoding":source.encoding});
    if let Some(structure) = source.structure {
        structure
            .validate(source.schema)
            .map_err(MapForceX12SourceError::Binding)?;
        descriptor["structure"] = serde_json::to_value(structure).map_err(|_| {
            MapForceX12SourceError::Binding("native structure serialization failed")
        })?;
    }
    let encoded = serde_json::to_string(&descriptor).map_err(|_| {
        MapForceX12SourceError::Binding("native source descriptor serialization failed")
    })?;
    if encoded.len() > crate::MAX_EMBEDDED_X12_DESCRIPTOR_BYTES {
        return Err(MapForceX12SourceError::Binding(
            "native source descriptor exceeds its byte limit",
        ));
    }
    Ok(encoded)
}

#[derive(Debug)]
pub enum MapForceX12SourceError {
    Program(crate::ProgramValidationError),
    Boundary(crate::X12BoundaryPolicyError),
    Binding(&'static str),
}
impl std::fmt::Display for MapForceX12SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Program(error) => error.fmt(f),
            Self::Boundary(error) => error.fmt(f),
            Self::Binding(reason) => write!(f, "MapForce X12 source: {reason}"),
        }
    }
}
impl std::error::Error for MapForceX12SourceError {}

/// Bind a physical X12 source to one lexical message body. The returned boundary
/// parses the complete source; it does not execute the body or select filenames.
pub fn prepare_mapforce_x12_source(
    program: &Program,
    schema: &SchemaNode,
    options: &FormatOptions,
    message: &[String],
) -> Result<String, MapForceX12SourceError> {
    use MapForceX12SourceError::{Binding, Boundary};
    if message.is_empty() || message.len() > 64 || message.iter().any(String::is_empty) {
        return Err(Binding("invalid message owner path"));
    }
    let node = message
        .iter()
        .try_fold(schema, |node, name| node.child(name))
        .ok_or(Binding("message owner is absent from the physical schema"))?;
    if !node.repeating || !matches!(node.kind, SchemaKind::Group { .. }) {
        return Err(Binding("message owner must be a repeated group"));
    }
    fn lexical(node: &mut SchemaNode) {
        match &mut node.kind {
            SchemaKind::Scalar { ty } => *ty = ScalarType::String,
            SchemaKind::Group { children, .. } => children.iter_mut().for_each(lexical),
            SchemaKind::ScalarUnion { .. } => {}
        }
    }
    let mut selected = node.clone();
    selected.repeating = false;
    lexical(&mut selected);
    if program.source != selected {
        return Err(Binding(
            "body source differs from the selected lexical message",
        ));
    }
    let mut boundary = X12BoundaryOptions::from_format_options(options).map_err(Boundary)?;
    boundary.envelope_profile = X12EnvelopeProfile::MapForceInterchanges;
    if boundary
        .implied_decimals
        .iter()
        .any(|value| value.places() != 0)
    {
        return Err(Binding(
            "fractional implied-decimal source projection is not admitted",
        ));
    }
    crate::x12_boundary::descriptor(schema, Some(&boundary), X12BoundarySide::Source)
        .map_err(Boundary)
}

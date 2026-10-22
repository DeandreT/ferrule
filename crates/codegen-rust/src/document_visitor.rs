use codegen::{ArtifactSet, DocumentVisitorEmitError, Program};

use crate::{EmitError, Options};

/// Add incremental delivery for one statically selected ordinary target.
/// Existing materializing entry points keep their behavior and clock contract.
pub fn emit_with_document_visitor(
    program: &Program,
    options: &Options,
    target: Option<&str>,
) -> Result<ArtifactSet, DocumentVisitorEmitError<EmitError>> {
    let index = codegen::validate_document_visitor(program, target)
        .map_err(DocumentVisitorEmitError::Boundary)?;
    let mut files = crate::emit(program, options)
        .map_err(DocumentVisitorEmitError::Emit)?
        .into_files();
    let source = render(program, index).map_err(DocumentVisitorEmitError::Emit)?;
    for file in &mut files {
        if file.path.as_str() == "src/lib.rs" {
            file.contents.extend_from_slice(source.as_bytes());
        }
    }
    ArtifactSet::new(files).map_err(|error| DocumentVisitorEmitError::Emit(error.into()))
}

fn render(program: &Program, index: Option<usize>) -> Result<String, EmitError> {
    let (name, root) = index.map_or_else(
        || ("scope_root".to_owned(), &program.root),
        |index| {
            (
                format!("scope_extra_{index}"),
                &program.extra_targets[index].root,
            )
        },
    );
    let mut scopes = Vec::new();
    super::collect_scopes(root, name, &mut scopes);
    let (_, _, children, _) = &scopes[0];
    let mut output = String::from(API);
    if !program.filter_map_v1_sequences().is_empty() {
        output.push_str("    let run_context = context.with_filter_map_defaults();\n    let context = &run_context;\n");
    }
    output.push_str("    evaluate_failure_rules(context)?;\n");
    output.push_str(&super::render_iteration_scope_delivery(
        program,
        root,
        root.iteration
            .as_ref()
            .expect("validated visitor iteration"),
        children,
        true,
    )?);
    output.push_str("}\n");
    Ok(output)
}

const API: &str = r#"
/// Completed delivery or a consumer-requested stop. A stopped document is not accepted.
#[derive(Debug, PartialEq, Eq)]
#[must_use = "distinguish complete delivery from a consumer-requested stop"]
pub enum DocumentVisit<B> {
    Complete { accepted: usize },
    Stopped { accepted: usize, reason: B },
}

#[derive(Debug)]
pub struct DocumentVisitError {
    pub accepted: usize,
    pub source: RuntimeError,
}
impl std::fmt::Display for DocumentVisitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "document evaluation failed after {} accepted documents: {}", self.accepted, self.source)
    }
}
impl std::error::Error for DocumentVisitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { Some(&self.source) }
}

/// Evaluate and offer one complete body/path pair before the next body.
/// This does not open files or roll back earlier accepted documents.
pub fn visit_target_documents<'a, B>(
    source: &'a Instance,
    inputs: &[NamedInput<'a>],
    execution: Option<&ExecutionContext<'a>>,
    loader: Option<&'a dyn DynamicSourceLoader>,
    mut consumer: impl FnMut(codegen_runtime::DocumentMember) -> std::ops::ControlFlow<B>,
) -> Result<DocumentVisit<B>, DocumentVisitError> {
    let inputs = validate_named_inputs(inputs).map_err(|source| DocumentVisitError { accepted: 0, source })?;
    let context = match execution {
        Some(execution) => ScopeContext::with_named_inputs_and_execution_context(source, &inputs, execution),
        None => ScopeContext::with_named_inputs(source, &inputs),
    };
    let context = match loader { Some(loader) => context.with_dynamic_source_loader(loader), None => context };
    let mut accepted = 0;
    let mut stopped = None;
    let outcome = {
        let mut offer = |document| match consumer(document) {
            std::ops::ControlFlow::Continue(()) => { accepted += 1; true }
            std::ops::ControlFlow::Break(reason) => { stopped = Some(reason); false }
        };
        visit_document_scope(&context, &mut offer)
    };
    outcome.map_err(|source| DocumentVisitError { accepted, source })?;
    Ok(match stopped {
        Some(reason) => DocumentVisit::Stopped { accepted, reason },
        None => DocumentVisit::Complete { accepted },
    })
}

fn visit_document_scope(
    context: &ScopeContext<'_>,
    offer_document: &mut dyn FnMut(codegen_runtime::DocumentMember) -> bool,
) -> Result<(), RuntimeError> {
"#;

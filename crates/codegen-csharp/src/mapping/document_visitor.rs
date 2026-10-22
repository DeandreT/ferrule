use super::*;

pub(super) fn render(
    program: &Program,
    scope: usize,
    plan: &ScopePlan<'_>,
    output: &mut String,
) -> Result<(), EmitError> {
    output.push_str(API);
    if !program.failure_rules.is_empty() {
        output.push_str("        EvaluateFailureRules(context);\n");
    }
    super::render_iteration_scope_delivery(
        program,
        scope,
        plan.iteration.expect("validated document iteration"),
        false,
        true,
        output,
    )?;
    output.push_str("    }\n");
    Ok(())
}

const API: &str = r#"
    public readonly struct DocumentDecision<B>
    {
        private DocumentDecision(bool stopped, B? reason) { Stopped = stopped; Reason = reason; }
        public bool Stopped { get; }
        public B? Reason { get; }
        public static DocumentDecision<B> Continue => new(false, default);
        public static DocumentDecision<B> Break(B reason) => new(true, reason);
    }

    public sealed record DocumentVisit<B>(bool Completed, int Accepted, B? Reason);

    public sealed class DocumentVisitException : global::System.Exception
    {
        public DocumentVisitException(int accepted, global::Ferrule.Runtime.FerruleRuntimeException source)
            : base($"Document evaluation failed after {accepted} accepted documents: {source.Message}", source)
        { Accepted = accepted; }
        public int Accepted { get; }
    }

    /// <summary>Offers each complete body/path pair before evaluating the next body. No files are opened.</summary>
    public static DocumentVisit<B> VisitTargetDocuments<B>(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> inputs,
        global::Ferrule.Runtime.FerruleExecutionContext? executionContext,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader? loader,
        global::System.Func<global::Ferrule.Runtime.FerruleDocument, DocumentDecision<B>> consumer)
    {
        global::System.ArgumentNullException.ThrowIfNull(consumer);
        var accepted = 0;
        var stopped = false;
        B? reason = default;
        global::System.Exception? consumerError = null;
        try
        {
            var context = CreateContext(source, inputs, executionContext, loader);
            VisitDocumentScope(context, document =>
            {
                DocumentDecision<B> decision;
                try { decision = consumer(document); }
                catch (global::System.Exception error) { consumerError = error; throw; }
                if (decision.Stopped) { stopped = true; reason = decision.Reason; return false; }
                accepted++;
                return true;
            });
        }
        catch (global::Ferrule.Runtime.FerruleRuntimeException error) when (consumerError is null)
        {
            throw new DocumentVisitException(accepted, error);
        }
        return new DocumentVisit<B>(!stopped, accepted, reason);
    }

    private static void VisitDocumentScope(
        global::Ferrule.Runtime.ScopeContext context,
        global::System.Func<global::Ferrule.Runtime.FerruleDocument, bool> offerDocument)
    {
"#;

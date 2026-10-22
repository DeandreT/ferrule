use super::*;

pub(super) fn render(
    program: &Program,
    scope: usize,
    plan: &ScopePlan<'_>,
    clock: bool,
    body_clock: bool,
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
    if clock {
        output.push_str(CLOCK_API);
    }
    if body_clock {
        output.push_str(BODY_CLOCK_API);
    }
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
        => VisitDocuments(source, inputs, executionContext, loader, null, null, consumer);

    private static DocumentVisit<B> VisitDocuments<B>(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> inputs,
        global::Ferrule.Runtime.FerruleExecutionContext? executionContext,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader? loader,
        global::System.Func<string>? filenameClock,
        global::System.Func<string>? bodyClock,
        global::System.Func<global::Ferrule.Runtime.FerruleDocument, DocumentDecision<B>> consumer)
    {
        global::System.ArgumentNullException.ThrowIfNull(consumer);
        var accepted = 0;
        var stopped = false;
        B? reason = default;
        global::System.Exception? consumerError = null;
        try
        {
            var context = CreateContext(source, inputs, executionContext, loader).WithDocumentBodyClock(bodyClock);
            VisitDocumentScope(context, filenameClock, document =>
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
        global::System.Func<string>? filenameClock,
        global::System.Func<global::Ferrule.Runtime.FerruleDocument, bool> offerDocument)
    {
"#;

const CLOCK_API: &str = r#"
    /// <summary>Samples only reached filename date-time nodes, after the complete body.</summary>
    public static DocumentVisit<B> VisitTargetDocumentsWithFilenameClock<B>(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> inputs,
        global::Ferrule.Runtime.FerruleExecutionContext? executionContext,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader? loader,
        global::System.Func<string> clock,
        global::System.Func<global::Ferrule.Runtime.FerruleDocument, DocumentDecision<B>> consumer)
    {
        global::System.ArgumentNullException.ThrowIfNull(clock);
        return VisitDocuments(source, inputs, executionContext, loader, clock, null, consumer);
    }
"#;

const BODY_CLOCK_API: &str = r#"
    /// <summary>Samples body reads at their evaluation points and filenames after complete bodies.</summary>
    public static DocumentVisit<B> VisitTargetDocumentsWithClocks<B>(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> inputs,
        global::Ferrule.Runtime.FerruleExecutionContext? executionContext,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader? loader,
        global::System.Func<string> bodyClock,
        global::System.Func<string> filenameClock,
        global::System.Func<global::Ferrule.Runtime.FerruleDocument, DocumentDecision<B>> consumer)
    {
        global::System.ArgumentNullException.ThrowIfNull(bodyClock);
        global::System.ArgumentNullException.ThrowIfNull(filenameClock);
        return VisitDocuments(source, inputs, executionContext, loader, filenameClock, bodyClock, consumer);
    }
"#;

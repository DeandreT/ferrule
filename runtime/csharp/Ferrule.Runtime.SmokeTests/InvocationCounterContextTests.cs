namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void InvocationCounterContexts()
    {
        var source = new FerruleGroup(new[]
        {
            new FerruleField("Rows", new FerruleRepeated(new FerruleInstance[]
            {
                new FerruleGroup(new[] { new FerruleField("id", new FerruleScalar(FerruleValue.FromInt64(1))) }),
                new FerruleGroup(new[] { new FerruleField("id", new FerruleScalar(FerruleValue.FromInt64(2))) }),
            })),
        });
        var limits = new FerruleInvocationCounterLimits(1, 4);
        var execution = new FerruleExecutionContext("mapping.ferrule").WithInvocationCounterLimits(limits);
        Equal(limits, execution.WithFilterMapLimits(FerruleFilterMapLimits.Default).InvocationCounterLimits);
        var root = ScopeContext.FromSource(source, execution);
        Equal(FerruleValue.FromInt64(1), root.NextInvocationCounter(1, "line", 1, 1, null));
        var children = root.IterateSource("Rows");
        Equal(2, children.Count);
        Equal(FerruleValue.FromInt64(2), children[0].NextInvocationCounter(1, "line", 999, 1, null));
        Equal(FerruleValue.FromInt64(3), children[1].NextInvocationCounter(1, "line", 999, 1, null));
        var groups = root.GroupBy(children, new[] { "Rows" }, child => child.ResolveSourceField(new[] { "id" }));
        Equal(2, groups.Count);
        Equal(FerruleValue.FromInt64(4), groups[0].NextInvocationCounter(1, "line", 999, 1, null));
        Equal(FerruleValue.FromInt64(5), groups[1].NextInvocationCounter(1, "line", 999, 1, null));
        Equal(FerruleValue.FromInt64(1), ScopeContext.FromSource(source, execution).NextInvocationCounter(1, "line", 1, 1, null));
        Equal(FerruleValue.FromInt64(6), root.NextInvocationCounter(1, "line", 999, 1, null));
        try
        {
            root.NextInvocationCounter(9, "other", 1, 1, null);
            throw new InvalidOperationException("Expected counter limit.");
        }
        catch (FerruleRuntimeException error)
        {
            Equal(FerruleRuntimeError.InvocationCounter, error.Error);
            Equal<uint?>(9, error.Node);
            Equal(FerruleInvocationCounterFailure.CountLimit,
                ((FerruleInvocationCounterException)error.InnerException!).Failure);
        }
        try
        {
            ScopeContext.FromSource(source).RequireInvocationCounters(7);
            throw new InvalidOperationException("Expected explicit configuration.");
        }
        catch (FerruleRuntimeException error)
        {
            Equal(FerruleRuntimeError.MissingInvocationCounters, error.Error);
            Equal<uint?>(7, error.Node);
        }
        Equal(3L, FerruleInvocationCounterOperands.RequireInteger(1, "start", FerruleValue.FromInt64(3)));
        Equal<string?>(null, FerruleInvocationCounterOperands.RequireRestart(1, FerruleValue.Null));
        Equal("", FerruleInvocationCounterOperands.RequireRestart(1, FerruleValue.FromString("")));
        try
        {
            FerruleInvocationCounterOperands.RequireInteger(8, "start", FerruleValue.FromString("3"));
            throw new InvalidOperationException("Expected strict integer operand.");
        }
        catch (FerruleRuntimeException error)
        {
            Equal(FerruleRuntimeError.InvocationCounterOperand, error.Error);
            Equal<uint?>(8, error.Node);
            Equal("start", error.Detail);
        }
    }
}

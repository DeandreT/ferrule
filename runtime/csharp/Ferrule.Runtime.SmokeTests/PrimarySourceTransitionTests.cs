using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void PrimarySourceTransitions()
    {
        static FerruleGroup Root(string tag) => new([
            new("Tag", new FerruleScalar(FerruleValue.FromString(tag))),
            new("Rows", new FerruleRepeated([new FerruleGroup([])])),
        ]);
        var a = Root("A"); var b = Root("B");
        var execution = new FerruleExecutionContext("original.mfd", "original.mfd", null, FerruleRuntimeParameters.Empty)
            .WithInvocationCounterLimits(new(2, 128));
        var first = ScopeContext.FromSource(a, execution).WithDocumentBodyClock(() => "2030-01-02T03:04:05");
        Equal(1L, first.NextInvocationCounter(1, "lines", 1, 1, null).Int64Value);
        var second = first.NextPrimarySource(b)!;
        Equal(2L, second.NextInvocationCounter(1, "lines", 1, 1, null).Int64Value);
        Equal(3L, first.NextInvocationCounter(1, "lines", 1, 1, null).Int64Value);
        Equal("A", first.ResolveSourceRootField(2, ["Tag"]).StringValue);
        Equal("B", second.ResolveSourceRootField(2, ["Tag"]).StringValue);
        Equal("2030-01-02T03:04:05", second.ResolveRuntimeValue(FerruleRuntimeValue.CurrentDateTime).StringValue);
        Equal(true, first.IterateSource("Rows").First().NextPrimarySource(b) is null);
        Equal(true, ScopeContext.FromSources(a, [new("Other", b)]).NextPrimarySource(b) is null);

        var parameter = Root("parameter");
        var named = ScopeContext.FromSources(a, [new("Parameters", parameter)], execution)
            .WithDocumentBodyClock(() => "2030-01-02T03:04:05");
        Equal(true, named.NextPrimarySource(b) is null);
        Equal(1L, named.NextInvocationCounter(1, "lines", 1, 1, null).Int64Value);
        var next = named.NextPrimarySourceWithSameInputs(b)!;
        Equal(2L, next.NextInvocationCounter(1, "lines", 1, 1, null).Int64Value);
        Equal("parameter", next.ResolveSourceField(new[] { "Parameters", "Tag" }).StringValue);
        Equal("B", next.ResolveSourceRootField(2, ["Tag"]).StringValue);
        Equal("A", named.ResolveSourceRootField(2, ["Tag"]).StringValue);
        Equal("2030-01-02T03:04:05", next.ResolveRuntimeValue(FerruleRuntimeValue.CurrentDateTime).StringValue);
        var third = next.NextPrimarySourceWithSameInputs(a)!;
        Equal(3L, third.NextInvocationCounter(1, "lines", 1, 1, null).Int64Value);
        Equal("parameter", third.ResolveSourceField(new[] { "Parameters", "Tag" }).StringValue);
        Equal(true, named.IterateSource("Rows").First().NextPrimarySourceWithSameInputs(b) is null);
        Equal(1L, ScopeContext.FromSources(a, [new("Parameters", parameter)], execution)
            .NextInvocationCounter(1, "lines", 1, 1, null).Int64Value);
    }
}

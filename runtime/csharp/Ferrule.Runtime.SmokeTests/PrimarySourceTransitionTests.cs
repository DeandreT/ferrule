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
    }
}

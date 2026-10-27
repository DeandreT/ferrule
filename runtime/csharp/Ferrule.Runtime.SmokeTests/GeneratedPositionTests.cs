using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void GeneratedSourcePositions()
    {
        var root = ScopeContext.FromSource(new FerruleGroup(Array.Empty<FerruleField>()));
        try
        {
            root.GeneratedPosition(9, 10);
            throw new Exception("missing generated owner was accepted");
        }
        catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.GeneratedPositionUnavailable) { }
        var values = new[] { FerruleValue.FromInt64(8), FerruleValue.FromInt64(8) };
        var outer = root.IterateGenerated(values, 10);
        var selected = outer[1].WithCompactedPosition(1);
        Equal(1, selected.Position());
        Equal(FerruleValue.FromInt64(2), selected.GeneratedPosition(9, 10));
        var inner = selected.IterateGenerated(new[] { FerruleValue.FromInt64(0) }, 20);
        Equal(FerruleValue.FromInt64(2), inner[0].GeneratedPosition(9, 10));
        Equal(FerruleValue.FromInt64(1), inner[0].GeneratedPosition(9, 20));
        Equal(FerruleValue.FromInt64(1), outer[0].GeneratedPosition(9, 10));
    }
}

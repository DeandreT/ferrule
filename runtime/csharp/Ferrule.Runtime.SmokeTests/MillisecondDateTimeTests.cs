using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void MillisecondDateTime()
    {
        const string picture = "[Y,4-4]-[M,2-2]-[D,2-2]T[H,2-2]:[m,2-2]:[s,2-2].[f,1-7]Z";
        foreach (var (input, expected) in new[]
        {
            ("2026-01-02T00:01:02", "2026-01-02T00:01:02.000Z"),
            ("2026-01-02T03:04:05.1", "2026-01-02T03:04:05.100Z"),
            ("2026-01-02T03:04:05.12", "2026-01-02T03:04:05.120Z"),
            ("2026-01-02T03:04:05.1234567", "2026-01-02T03:04:05.123Z"),
            ("2026-01-02T03:04:05.0009999", "2026-01-02T03:04:05.000Z"),
            ("2026-01-02T23:59:59.9999999-07:00", "2026-01-02T23:59:59.999Z"),
            ("2026-01-02T03:04:05.1234567+05:30", "2026-01-02T03:04:05.123Z"),
        })
        {
            var result = FerruleFunctions.Call("format_datetime_milliseconds", new[] { FerruleValue.FromString(input), FerruleValue.FromString(picture) });
            if (result.StringValue != expected) throw new InvalidOperationException("Millisecond date-time semantics changed.");
        }
        var normal = FerruleFunctions.Call("format_datetime", new[] { FerruleValue.FromString("2026-01-02T03:04:05.1234567"), FerruleValue.FromString(picture) });
        if (normal.StringValue != "2026-01-02T03:04:05.1234567Z") throw new InvalidOperationException("Ordinary formatter changed.");
    }
}

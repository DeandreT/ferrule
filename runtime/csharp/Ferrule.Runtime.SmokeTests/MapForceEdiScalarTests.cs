using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void MapForceEdiScalars()
    {
        using var cases = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "mapforce-edi-scalars.json")));
        foreach (var item in cases.RootElement.EnumerateArray())
        {
            var kind = Enum.Parse<FerruleMapForceEdiScalarKind>(item.GetProperty("kind").GetString()!, ignoreCase: true);
            var field = new FerruleMapForceEdiScalar(kind, item.GetProperty("min").GetInt32(), item.GetProperty("max").GetInt32());
            var value = FerruleValue.FromString(item.GetProperty("input").GetString()!);
            if (item.TryGetProperty("error", out _))
            {
                try { _ = FerruleMapForceEdiScalars.Format(field, value); }
                catch (Exception e) when (e is FerruleDecimalException or FormatException or ArgumentException) { continue; }
                throw new Exception("Expected mapped EDI scalar failure");
            }
            Equal(item.GetProperty("output").GetString(), FerruleMapForceEdiScalars.Format(field, value));
        }
        var text = new FerruleMapForceEdiScalar(FerruleMapForceEdiScalarKind.Text, 2, 2);
        Equal("", FerruleMapForceEdiScalars.Format(text, FerruleValue.Null));
        Throws<ArgumentException>(() => FerruleMapForceEdiScalars.Format(text, FerruleValue.FromDouble(double.PositiveInfinity)));
        Equal("1.25", FerruleMapForceEdiScalars.Format(text, FerruleValue.FromDouble(1.25)));
        foreach (var (input, count, expected) in new[] { ("😀A",2,"😀"),("😀A",1,"�"),("A😀B",2,"A�"),("A😀B",3,"A😀"),("ABC",2,"AB") })
            Equal(expected, FerruleFunctions.Call("left_utf16_lossy", new[] { FerruleValue.FromString(input), FerruleValue.FromInt64(count) }).StringValue);
        Equal("😀A", FerruleFunctions.Call("left", new[] { FerruleValue.FromString("😀A"), FerruleValue.FromInt64(2) }).StringValue);
    }
}

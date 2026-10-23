using System.Text;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void NativeJsonSource()
    {
        const string shape = """{"kind":"object","properties":[{"name":"v","shape":{"kind":"string"}}]}""";
        static string Value(FerruleJsonSourceDocument document) => document.Root.MembersNamed("v").First().StringValue!;
        foreach (var (raw, expected) in new[] {
            ("{\"v\":\"A\nB\"}", "A\nB"),
            ("""{"v":"\ud800"}""", "�"),
            ("""{"v":"\udfff"}""", "�"),
            ("""{"v":"\ud83d\ude00"}""", "😀"),
            ("{\"v\":\"A\0B\"}", "A\0B"),
        }) Equal(expected, Value(FerruleNativeJsonSource.Parse("doc", Encoding.UTF8.GetBytes(raw), shape)));
        const string source = "{\"v\":\"é😀\"}";
        foreach (Encoding encoding in new Encoding[] { new UTF8Encoding(true), new UnicodeEncoding(false, true), new UnicodeEncoding(true, true), new UTF32Encoding(false, true), new UTF32Encoding(true, true) })
        {
            var bytes = encoding.GetPreamble().Concat(encoding.GetBytes(source)).ToArray();
            Equal("é😀", Value(FerruleNativeJsonSource.Parse("doc", bytes, shape)));
        }
        var tail = Encoding.UTF8.GetBytes(source).Concat(new byte[] { 0xe2, 0x82 }).ToArray();
        Equal("é😀", Value(FerruleNativeJsonSource.Parse("doc", tail, shape)));
        foreach (var invalid in new[] { "{\"v\":\"x\",}", "{\"n\":01}", "{\"n\":+1}", "{}\0", "{}{}" })
            Throws<FerruleJsonSourceException>(() => FerruleNativeJsonSource.Parse("doc", Encoding.UTF8.GetBytes(invalid), shape));
        Equal(8L, FerruleFunctions.Call("length_utf16", new[] { FerruleValue.FromString("20😀1234") }).Int64Value);
        Equal(7L, FerruleFunctions.Call("length", new[] { FerruleValue.FromString("20😀1234") }).Int64Value);
    }
}

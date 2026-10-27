namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void LiteralReplacement()
    {
        static FerruleValue Text(string value) => FerruleValue.FromString(value);
        foreach (var (input, search, replacement, expected) in new[] {
            (@"a\b\c", @"\", "_", "a_b_c"),
            ("a.a", ".", @"$1\", @"a$1\a"),
            ("aaaaa", "aa", "b", "bba"),
            ("AaA", "a", "x", "AxA"),
            ("é😀é", "é", "🦀", "🦀😀🦀"),
            ("e\u0301", "é", "x", "e\u0301"),
        })
            Equal(expected, FerruleFunctions.Call("replace_literal", [Text(input), Text(search), Text(replacement)]).StringValue);
        Equal("b", FerruleFunctions.Call("replace_literal", [Text("aba"), Text("a"), FerruleValue.Null]).StringValue);
        Throws<FerruleRuntimeException>(() => FerruleFunctions.Call("replace_literal", [Text(""), Text(""), Text("x")]));
        Throws<FerruleRuntimeException>(() => FerruleFunctions.Call("replace_literal", [FerruleValue.Null, Text("a"), Text("x")]));
        Throws<FerruleRuntimeException>(() => FerruleFunctions.Call("replace_literal", [Text("a"), FerruleValue.Null, Text("x")]));
    }
}

using System.Text;

namespace Ferrule.Runtime.SmokeTests;

// Independently authored general-contract tests, not vendor mapping qualification.
internal static partial class Program
{
    private static FerruleValue ComponentText(string value) => FerruleValue.FromString(value);
    private static FerruleMapForceJsonString ParseComponent(string value) => FerruleMapForceJsonString.Parse(ComponentText(value));
    private static FerruleMapForceStringInputFallback ComponentMap(params (string Key, string Value)[] entries) =>
        new(entries.Select(x => new KeyValuePair<string, string>(x.Key, x.Value)).ToArray());

    private static void StringComponentSelectionContract()
    {
        foreach (var input in new[] { "{}", "{\"extra\":true}" })
        {
            var parsed = ParseComponent(input);
            Equal(true, parsed.RootObject is not null);
            Equal<string?>(null, parsed.FirstString("key"));
        }
        foreach (var input in new[] { "null", "true", "7", "\"north\"", "[]", "[{}]" })
        {
            var parsed = ParseComponent(input);
            Equal(true, parsed.RootObject is null);
            Equal(0, parsed.Strings("key").Count());
        }
        var ordered = ParseComponent("{\"Key\":\"ignored\",\"key\":null,\"key\":9,\"key\":false,\"key\":{},\"key\":[],\"key\":\"\",\"key\":\"south\"}");
        Equal(true, ordered.Strings("key").SequenceEqual(new[] { "", "south" }));
        Equal("", ordered.FirstString("key"));
        Equal("ignored", ordered.FirstString("Key"));
        Equal("north", ParseComponent("{\"key\":17,\"key\":\"north\",\"key\":\"south\"}").FirstString("key"));
        var deferred = ParseComponent("{\"extra\":{\"number\":1e1000},\"key\":\"north\"}");
        Equal("north", deferred.FirstString("key"));
        var number = deferred.Root.MembersNamed("extra").Single().MembersNamed("number").Single();
        Equal("1e1000", number.NumberText);
        Throws<FerruleDecimalException>(() => number.AsDecimalNumber());
        foreach (var input in new[] { "", "{} {}", "{key:1}", "{\"extra\":NaN}", "{\"key\":1,}", "{/*x*/}" })
            Throws<FerruleJsonSourceException>(() => ParseComponent(input));
        var error = CaptureStringComponentJsonError(() => ParseComponent("{"));
        Equal(FerruleJsonSourceErrorKind.Syntax, error.Kind);
        Equal(true, error.InnerException is not null);
        foreach (var input in new[] { FerruleValue.Null, FerruleValue.FromBoolean(true), FerruleValue.FromInt64(4), ComponentText("\uFEFF{}") })
            Throws<FerruleMapForceStringComponentException>(() => FerruleMapForceJsonString.Parse(input));
        foreach (var input in new[] { "{\"key\":\"\u0001\"}", "{\"key\":\"\\uD800\"}" })
            Throws<FerruleJsonSourceException>(() => ParseComponent(input));
    }

    private static FerruleJsonSourceException CaptureStringComponentJsonError(Action action)
    {
        try { action(); }
        catch (FerruleJsonSourceException error) { return error; }
        throw new Exception("Expected a JSON source failure.");
    }

    private static void StringComponentParserLimits()
    {
        const string input = "{\"key\":[\"é🧭\",null]}";
        var bytes = Encoding.UTF8.GetByteCount(input);
        var exact = new FerruleJsonSourceLimits { DocumentBytes = bytes, ContainerDepth = 2, Values = 4 };
        _ = FerruleMapForceJsonString.Parse(ComponentText(input), exact);
        foreach (var limits in new[] { exact with { DocumentBytes = bytes - 1 }, exact with { ContainerDepth = 1 }, exact with { Values = 3 } })
            Throws<FerruleJsonSourceException>(() => FerruleMapForceJsonString.Parse(ComponentText(input), limits));
        _ = FerruleMapForceJsonString.Parse(ComponentText("7"), new() { DocumentBytes = 1, ContainerDepth = 0, Values = 1 });
        Throws<FerruleJsonSourceException>(() => FerruleMapForceJsonString.Parse(ComponentText("7"), new() { DocumentBytes = 1, ContainerDepth = 0, Values = 0 }));
        var maximum = new FerruleJsonSourceLimits();
        _ = FerruleMapForceJsonString.Parse(ComponentText("{}"), maximum);
        foreach (var limits in new[] { maximum with { DocumentBytes = maximum.DocumentBytes + 1 }, maximum with { ContainerDepth = maximum.ContainerDepth + 1 }, maximum with { Values = maximum.Values + 1 } })
            Equal(FerruleJsonSourceErrorKind.InvalidLimits, CaptureStringComponentJsonError(() => FerruleMapForceJsonString.Parse(ComponentText("{}"), limits)).Kind);
        Equal(FerruleJsonSourceErrorKind.DocumentBytes, CaptureStringComponentJsonError(() => FerruleMapForceJsonString.Parse(ComponentText("{}"), new() { DocumentBytes = 1 })).Kind);
    }

    private static void StringComponentFallbackContract()
    {
        var map = ComponentMap(("north", "up"), ("south", "down"));
        foreach (var (input, expected) in new[] { ("north", "up"), ("south", "down"), ("east", "east"), ("North", "North"), (" north", " north"), ("north ", "north "), ("", ""), ("é🧭", "é🧭") })
            Equal(expected, map.Apply(ComponentText(input)));
        Equal<string?>(null, map.ApplyPresent(null));
        Equal("", map.ApplyPresent(ComponentText("")));
        Throws<FerruleMapForceStringComponentException>(() => map.ApplyPresent(FerruleValue.Null));
        foreach (var value in new[] { FerruleValue.FromBoolean(true), FerruleValue.FromInt64(3) })
            Throws<FerruleMapForceStringComponentException>(() => map.Apply(value));
        _ = ComponentMap(("north", "up"), ("North", "upper"));
        Throws<FerruleMapForceStringComponentException>(() => ComponentMap(("north", "up"), ("north", "down")));
        Equal("north", ComponentMap().Apply(ComponentText("north")));
    }

    private static void StringComponentFallbackLimits()
    {
        var entries = Enumerable.Range(0, 256).Select(i => ($"key{i}", "value")).ToArray();
        _ = ComponentMap(entries);
        Throws<FerruleMapForceStringComponentException>(() => ComponentMap(entries.Append(("another", "value")).ToArray()));
        var below = new string('é', 32_767) + "a";
        var exact = new string('é', 32_768);
        _ = ComponentMap((below, ""));
        _ = ComponentMap((exact, ""));
        Throws<FerruleMapForceStringComponentException>(() => ComponentMap((exact + "a", "")));
        var supplementary = string.Concat(Enumerable.Repeat("🧭", 16_384));
        _ = ComponentMap(("", supplementary));
        Throws<FerruleMapForceStringComponentException>(() => ComponentMap(("a", supplementary)));
        var value = new string('é', 32_764);
        _ = ComponentMap(("north", value), ("s", "é"));
        Throws<FerruleMapForceStringComponentException>(() => ComponentMap(("north", value), ("s", "éa")));
    }
}

using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void ScalarFunctionBatchA()
    {
        CallEquals(Text(string.Empty), "concat");
        CallEquals(
            Text("prefixtrue-7inf-suffix"),
            "concat",
            Text("prefix"),
            FerruleValue.Null,
            FerruleValue.XmlNil,
            Bool(true),
            FerruleValue.FromInt64(-7),
            FerruleValue.FromDouble(double.PositiveInfinity),
            Text("-suffix"));

        CallEquals(
            Text("alpha beta\u00A0gamma"),
            "normalize_space",
            Text(" \talpha\r\n beta\u00A0gamma \n"));
        CallEquals(Text("value \t"), "left_trim", Text(" \t\r\nvalue \t"));
        CallEquals(Text("\v value"), "left_trim", Text("\v value"));
        CallEquals(Text(" \tvalue"), "right_trim", Text(" \tvalue\r\n"));
        CallEquals(Text("value \v"), "right_trim", Text("value \v"));

        CallEquals(FerruleValue.FromInt64(0), "length", FerruleValue.Null);
        CallEquals(FerruleValue.FromInt64(0), "length", FerruleValue.XmlNil);
        CallEquals(FerruleValue.FromInt64(4), "length", Bool(true));
        CallEquals(FerruleValue.FromInt64(3), "length", Text("a\u0301\U0001F642"));

        const string SplitInput = "left\U0001F642middle\U0001F642right";
        CallEquals(Text("left"), "substring_before", Text(SplitInput), Text("\U0001F642"));
        CallEquals(
            Text("middle\U0001F642right"),
            "substring_after",
            Text(SplitInput),
            Text("\U0001F642"));
        CallEquals(Text(string.Empty), "substring_before", Text("value"), Text("missing"));
        CallEquals(Text(string.Empty), "substring_after", Text("value"), Text("missing"));
        CallEquals(Text(string.Empty), "substring_before", Text("value"), Text(string.Empty));
        CallEquals(Text("value"), "substring_after", Text("value"), Text(string.Empty));

        CallEquals(Text(string.Empty), "string", FerruleValue.Null);
        CallEquals(Text(string.Empty), "string", FerruleValue.XmlNil);
        CallEquals(Text("false"), "string", Bool(false));
        CallEquals(Text("-0"), "string", FerruleValue.FromDouble(-0.0));

        CallEquals(Text("replacement"), "substitute_missing", FerruleValue.Null, Text("replacement"));
        CallEquals(Text("replacement"), "substitute_missing", FerruleValue.XmlNil, Text("replacement"));
        CallEquals(Text("present"), "substitute_missing", Text("present"), Text("replacement"));
        CallEquals(Bool(false), "is_xml_nil", FerruleValue.Null);
        CallEquals(Bool(true), "is_xml_nil", FerruleValue.XmlNil);

        foreach (var (path, folder, filename) in new[]
        {
            ("/var/data/file.xml", "/var/data/", "file.xml"),
            (@"C:\data\file.xml", "C:\\data\\", "file.xml"),
            (@"one/two\file.xml", "one/two\\", "file.xml"),
            ("file.xml", string.Empty, "file.xml"),
            ("/var/data/", "/var/data/", string.Empty),
        })
        {
            CallEquals(Text(folder), "get_folder", Text(path));
            CallEquals(Text(filename), "remove_folder", Text(path));
        }

        foreach (var (basePath, path, expected) in new[]
        {
            ("/var/data", "reports/out.xml", "/var/data/reports/out.xml"),
            (@"C:\work\data", @"reports\out.xml", @"C:\work\data\reports\out.xml"),
            ("/var/data/current/", "../out.xml", "/var/data/out.xml"),
            (@"C:\work\data\", @"..\out.xml", @"C:\work\out.xml"),
            ("/", "../out.xml", "/out.xml"),
            (@"C:\", @"..\out.xml", @"C:\out.xml"),
            ("/var/data", "././out.xml", "/var/data/out.xml"),
            ("/ignored/base", "/etc/config.xml", "/etc/config.xml"),
            (@"C:\ignored", @"D:\data\config.xml", @"D:\data\config.xml"),
            (@"C:\ignored", @"\\server\share\config.xml", @"\\server\share\config.xml"),
            ("/ignored/base", "https://example.test/config.xml", "https://example.test/config.xml"),
            (@"C:/work\data", "reports/out.xml", @"C:/work\data/reports/out.xml"),
            ("/var/data", @"reports\out.xml", @"/var/data\reports\out.xml"),
            (@"C:\work", "reports/out.xml", @"C:\work\reports/out.xml"),
        })
        {
            CallEquals(Text(expected), "resolve_filepath", Text(basePath), Text(path));
        }

        AssertFunctionArity("normalize_space", 1, Array.Empty<FerruleValue>());
        AssertFunctionType("left_trim", FerruleValue.FromInt64(1));
        AssertFunctionType("substring_after", Text("value"), FerruleValue.FromInt64(1));
        AssertFunctionArity("string", 1, Text("one"), Text("two"));
        AssertFunctionArity("substitute_missing", 2, FerruleValue.Null);
        AssertFunctionArity("is_xml_nil", 1, FerruleValue.Null, FerruleValue.XmlNil);
        AssertFunctionType("get_folder", Bool(false));
        AssertFunctionArity("resolve_filepath", 2, Text("base"));
        AssertFunctionType("resolve_filepath", Text("base"), Bool(false));
    }

    private static void AssertFunctionArity(
        string function,
        int expected,
        params FerruleValue[] arguments)
    {
        var error = Error(
            FerruleRuntimeError.FunctionArity,
            () => FerruleFunctions.Call(function, arguments));
        Equal(function, error.Function);
        Equal(expected, error.ExpectedArity);
        Equal(arguments.Length, error.ActualArity);
    }

    private static void AssertFunctionType(
        string function,
        params FerruleValue[] arguments)
    {
        var error = Error(
            FerruleRuntimeError.FunctionType,
            () => FerruleFunctions.Call(function, arguments));
        Equal(function, error.Function);
        var invalid = arguments.First(value => value.Kind != FerruleValueKind.String);
        Equal(invalid.Kind, error.FoundKind);
    }
}

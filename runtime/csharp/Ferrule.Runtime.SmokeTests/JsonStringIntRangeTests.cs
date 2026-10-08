using System.Globalization;
using System.Text;
using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static string StringInt101Schema(string profile) => profile switch
    {
        "bounded" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}""",
        "optional-nullable" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}]}}""",
        "required-nullable" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}""",
        "wide" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":9007199254740993,"maximum":9007199254740995}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}""",
        "min-singleton" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":-9223372036854775808,"maximum":-9223372036854775808}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}""",
        "max-singleton" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":9223372036854775807,"maximum":9223372036854775807}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}""",
        "lower-only" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}""",
        "upper-only" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}""",
        "anyOf-nullable" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}""",
        "oneOf-nullable" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}""",
        "finite-enum" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]},"json_allowed_values":[{"type":"json_null"},{"type":"int","value":2},{"type":"int","value":6},{"type":"int","value":100},{"type":"string","value":"1"},{"type":"string","value":"100"},{"type":"string","value":"6"}]}],"required":["value"]}}""",
        "finite-composition-filter" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"nullable":true,"kind":{"kind":"scalar_union","types":["string","int"]},"json_allowed_values":[{"type":"json_null"},{"type":"int","value":2},{"type":"int","value":3},{"type":"string","value":"1"},{"type":"string","value":"100"},{"type":"string","value":"6"}]}],"required":["value"]}}""",
        "array" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"values","repeating":true,"nullable":true,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]},"container_nullable":true}],"required":["values"]}}""",
        "old-unconstrained" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"kind":{"kind":"scalar_union","types":["string","int"]}}],"required":["value"]}}""",
        "independent-string-assertions" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":5}},"kind":{"kind":"scalar_union","types":["string","int"]},"string_length_range":{"minimum":2,"maximum":3},"json_patterns":{"any_of":[["^x+$"]]},"json_formats":["date"]}],"required":["value"]}}""",
        "independent-multiple-of" => """{"name":"Record","repeating":false,"kind":{"kind":"group","children":[{"name":"value","repeating":false,"numeric_range":{"kind":"integer","bounds":{"minimum":2,"maximum":8}},"kind":{"kind":"scalar_union","types":["string","int"]},"json_multiple_of":{"any_of":[[{"coefficient":2,"decimal_exponent":0}]]}}],"required":["value"]}}""",
        _ => throw new InvalidOperationException("Undeclared fixture profile."),
    };

    private sealed record StringInt101Case(
        string Name, string Schema, string? Document, FerruleInstance? ExpectedInput,
        FerruleInstance? OutputInput, string? ExpectedOutput,
        string? InputError, string? OutputError);

    private sealed record StringInt101Outcome(object? Value, Exception? Error);

    private sealed record StringInt101Original(
        StringInt101Case Case, string Api, StringInt101Outcome Outcome);

    private static StringInt101Outcome StringInt101Capture(Func<object> action)
    {
        try { return new(action(), null); }
        catch (Exception error) { return new(null, error); }
    }

    private static void JsonStringIntRangeBoundaries()
    {
        // Complete descriptors and typed expectations are literal, independently
        // authored input to both directions; no reader result feeds the writer.
        StringInt101Case[] cases =
        [
            new("bounded-refuse-1",StringInt101Schema("bounded"),"{\"value\":1}",null,Group(Field("value",Scalar(FerruleValue.FromInt64(1L)))),null,"JSON scalar 'value' is outside its numeric range: 1.","JSON scalar 'value' is outside its numeric range: 1."),
            new("bounded-refuse-6",StringInt101Schema("bounded"),"{\"value\":6}",null,Group(Field("value",Scalar(FerruleValue.FromInt64(6L)))),null,"JSON scalar 'value' is outside its numeric range: 6.","JSON scalar 'value' is outside its numeric range: 6."),
            new("wide-refuse-9007199254740992",StringInt101Schema("wide"),"{\"value\":9007199254740992}",null,Group(Field("value",Scalar(FerruleValue.FromInt64(9007199254740992L)))),null,"JSON scalar 'value' is outside its numeric range: 9007199254740992.","JSON scalar 'value' is outside its numeric range: 9007199254740992."),
            new("wide-refuse-9007199254740996",StringInt101Schema("wide"),"{\"value\":9007199254740996}",null,Group(Field("value",Scalar(FerruleValue.FromInt64(9007199254740996L)))),null,"JSON scalar 'value' is outside its numeric range: 9007199254740996.","JSON scalar 'value' is outside its numeric range: 9007199254740996."),
            new("min-neighbor",StringInt101Schema("min-singleton"),"{\"value\":-9223372036854775807}",null,Group(Field("value",Scalar(FerruleValue.FromInt64(-9223372036854775807L)))),null,"JSON scalar 'value' is outside its numeric range: -9223372036854775807.","JSON scalar 'value' is outside its numeric range: -9223372036854775807."),
            new("max-neighbor",StringInt101Schema("max-singleton"),"{\"value\":9223372036854775806}",null,Group(Field("value",Scalar(FerruleValue.FromInt64(9223372036854775806L)))),null,"JSON scalar 'value' is outside its numeric range: 9223372036854775806.","JSON scalar 'value' is outside its numeric range: 9223372036854775806."),
            new("array-refuse-int",StringInt101Schema("array"),"{\"values\":[6]}",null,Group(Field("values",new FerruleRepeated([Scalar(FerruleValue.FromInt64(6L))]))),null,"JSON scalar 'values' is outside its numeric range: 6.","JSON scalar 'values' is outside its numeric range: 6."),
            new("input-shape-fraction-lexeme",StringInt101Schema("bounded"),"{\"value\":1.0}",null,null,null,"JSON field 'value' expected declared scalar union, got Number.",null),
            new("input-shape-exponent-lexeme",StringInt101Schema("bounded"),"{\"value\":2e0}",null,null,null,"JSON field 'value' expected declared scalar union, got Number.",null),
            new("input-shape-unsigned-overflow",StringInt101Schema("bounded"),"{\"value\":9223372036854775808}",null,null,null,"JSON field 'value' expected declared scalar union, got Number.",null),
            new("input-shape-null",StringInt101Schema("bounded"),"{\"value\":null}",null,null,null,"JSON field 'value' expected declared scalar union, got Null.",null),
            new("output-shape-float",StringInt101Schema("bounded"),null,null,Group(Field("value",Scalar(FerruleValue.FromDouble(1.0)))),null,null,"JSON field 'value' expected declared scalar union, got Double."),
            new("output-shape-null",StringInt101Schema("bounded"),null,null,Group(Field("value",Scalar(FerruleValue.JsonNull))),null,null,"JSON field 'value' expected declared scalar union, got JsonNull."),
            new("output-shape-xml-nil",StringInt101Schema("bounded"),null,null,Group(Field("value",Scalar(FerruleValue.XmlNil))),null,null,"JSON field 'value' expected declared scalar union, got XmlNil."),
            new("required-missing",StringInt101Schema("required-nullable"),"{}",null,Group(Field("value",Scalar(FerruleValue.Null))),null,"JSON object 'Record' requires property 'value'.","JSON object 'Record' requires property 'value'."),
            new("enum-int100-bounded",StringInt101Schema("finite-enum"),"{\"value\":100}",null,Group(Field("value",Scalar(FerruleValue.FromInt64(100L)))),null,"JSON scalar 'value' is outside its numeric range: 100.","JSON scalar 'value' is outside its numeric range: 100."),
            new("independent-string-assertions-short",StringInt101Schema("independent-string-assertions"),"{\"value\":\"x\"}",null,Group(Field("value",Scalar(FerruleValue.FromString("x")))),null,"JSON string 'value' is outside its declared string-length range.","JSON string 'value' is outside its declared string-length range."),
            new("independent-string-assertions-pattern",StringInt101Schema("independent-string-assertions"),"{\"value\":\"yy\"}",null,Group(Field("value",Scalar(FerruleValue.FromString("yy")))),null,"JSON string 'value' does not match its declared JSON patterns.","JSON string 'value' does not match its declared JSON patterns."),
            new("multiple-int3",StringInt101Schema("independent-multiple-of"),"{\"value\":3}",null,Group(Field("value",Scalar(FerruleValue.FromInt64(3L)))),null,"JSON scalar 'value' is outside its JSON multiple-of constraints: 3.","JSON scalar 'value' is outside its JSON multiple-of constraints: 3."),
            new("array-refuse-absence-item",StringInt101Schema("array"),null,null,Group(Field("values",new FerruleRepeated([Scalar(FerruleValue.Null)]))),null,null,"JSON field 'values' expected declared scalar union, got Null."),
            new("string-empty",StringInt101Schema("bounded"),"{\"value\":\"\"}",Group(Field("value",Scalar(FerruleValue.FromString("")))),Group(Field("value",Scalar(FerruleValue.FromString("")))),"{\n  \"value\": \"\"\n}\n",null,null),
            new("string-word",StringInt101Schema("bounded"),"{\"value\":\"abc\"}",Group(Field("value",Scalar(FerruleValue.FromString("abc")))),Group(Field("value",Scalar(FerruleValue.FromString("abc")))),"{\n  \"value\": \"abc\"\n}\n",null,null),
            new("string-below",StringInt101Schema("bounded"),"{\"value\":\"1\"}",Group(Field("value",Scalar(FerruleValue.FromString("1")))),Group(Field("value",Scalar(FerruleValue.FromString("1")))),"{\n  \"value\": \"1\"\n}\n",null,null),
            new("string-above",StringInt101Schema("bounded"),"{\"value\":\"6\"}",Group(Field("value",Scalar(FerruleValue.FromString("6")))),Group(Field("value",Scalar(FerruleValue.FromString("6")))),"{\n  \"value\": \"6\"\n}\n",null,null),
            new("string-fraction",StringInt101Schema("bounded"),"{\"value\":\"1.0\"}",Group(Field("value",Scalar(FerruleValue.FromString("1.0")))),Group(Field("value",Scalar(FerruleValue.FromString("1.0")))),"{\n  \"value\": \"1.0\"\n}\n",null,null),
            new("string-spaces",StringInt101Schema("bounded"),"{\"value\":\" 6 \"}",Group(Field("value",Scalar(FerruleValue.FromString(" 6 ")))),Group(Field("value",Scalar(FerruleValue.FromString(" 6 ")))),"{\n  \"value\": \" 6 \"\n}\n",null,null),
            new("string-overflow",StringInt101Schema("bounded"),"{\"value\":\"9223372036854775808\"}",Group(Field("value",Scalar(FerruleValue.FromString("9223372036854775808")))),Group(Field("value",Scalar(FerruleValue.FromString("9223372036854775808")))),"{\n  \"value\": \"9223372036854775808\"\n}\n",null,null),
            new("int-min",StringInt101Schema("bounded"),"{\"value\":2}",Group(Field("value",Scalar(FerruleValue.FromInt64(2L)))),Group(Field("value",Scalar(FerruleValue.FromInt64(2L)))),"{\n  \"value\": 2\n}\n",null,null),
            new("int-max",StringInt101Schema("bounded"),"{\"value\":5}",Group(Field("value",Scalar(FerruleValue.FromInt64(5L)))),Group(Field("value",Scalar(FerruleValue.FromInt64(5L)))),"{\n  \"value\": 5\n}\n",null,null),
            new("optional-nullable-null",StringInt101Schema("optional-nullable"),"{\"value\":null}",Group(Field("value",Scalar(FerruleValue.JsonNull))),Group(Field("value",Scalar(FerruleValue.JsonNull))),"{\n  \"value\": null\n}\n",null,null),
            new("required-nullable-null",StringInt101Schema("required-nullable"),"{\"value\":null}",Group(Field("value",Scalar(FerruleValue.JsonNull))),Group(Field("value",Scalar(FerruleValue.JsonNull))),"{\n  \"value\": null\n}\n",null,null),
            new("optional-missing",StringInt101Schema("optional-nullable"),"{}",Group(Field("value",Scalar(FerruleValue.Null))),Group(Field("value",Scalar(FerruleValue.Null))),"{}\n",null,null),
            new("wide-9007199254740993",StringInt101Schema("wide"),"{\"value\":9007199254740993}",Group(Field("value",Scalar(FerruleValue.FromInt64(9007199254740993L)))),Group(Field("value",Scalar(FerruleValue.FromInt64(9007199254740993L)))),"{\n  \"value\": 9007199254740993\n}\n",null,null),
            new("wide-9007199254740995",StringInt101Schema("wide"),"{\"value\":9007199254740995}",Group(Field("value",Scalar(FerruleValue.FromInt64(9007199254740995L)))),Group(Field("value",Scalar(FerruleValue.FromInt64(9007199254740995L)))),"{\n  \"value\": 9007199254740995\n}\n",null,null),
            new("exact-i64-min",StringInt101Schema("min-singleton"),"{\"value\":-9223372036854775808}",Group(Field("value",Scalar(FerruleValue.FromInt64(long.MinValue)))),Group(Field("value",Scalar(FerruleValue.FromInt64(long.MinValue)))),"{\n  \"value\": -9223372036854775808\n}\n",null,null),
            new("exact-i64-max",StringInt101Schema("max-singleton"),"{\"value\":9223372036854775807}",Group(Field("value",Scalar(FerruleValue.FromInt64(long.MaxValue)))),Group(Field("value",Scalar(FerruleValue.FromInt64(long.MaxValue)))),"{\n  \"value\": 9223372036854775807\n}\n",null,null),
            new("lower-only-max",StringInt101Schema("lower-only"),"{\"value\":9223372036854775807}",Group(Field("value",Scalar(FerruleValue.FromInt64(long.MaxValue)))),Group(Field("value",Scalar(FerruleValue.FromInt64(long.MaxValue)))),"{\n  \"value\": 9223372036854775807\n}\n",null,null),
            new("upper-only-min",StringInt101Schema("upper-only"),"{\"value\":-9223372036854775808}",Group(Field("value",Scalar(FerruleValue.FromInt64(long.MinValue)))),Group(Field("value",Scalar(FerruleValue.FromInt64(long.MinValue)))),"{\n  \"value\": -9223372036854775808\n}\n",null,null),
            new("finite-enum-string100",StringInt101Schema("finite-enum"),"{\"value\":\"100\"}",Group(Field("value",Scalar(FerruleValue.FromString("100")))),Group(Field("value",Scalar(FerruleValue.FromString("100")))),"{\n  \"value\": \"100\"\n}\n",null,null),
            new("finite-composition-filter-string100",StringInt101Schema("finite-composition-filter"),"{\"value\":\"100\"}",Group(Field("value",Scalar(FerruleValue.FromString("100")))),Group(Field("value",Scalar(FerruleValue.FromString("100")))),"{\n  \"value\": \"100\"\n}\n",null,null),
            new("finite-composition-filter-null",StringInt101Schema("finite-composition-filter"),"{\"value\":null}",Group(Field("value",Scalar(FerruleValue.JsonNull))),Group(Field("value",Scalar(FerruleValue.JsonNull))),"{\n  \"value\": null\n}\n",null,null),
            new("finite-extra-int",StringInt101Schema("finite-composition-filter"),"{\"value\":3}",Group(Field("value",Scalar(FerruleValue.FromInt64(3L)))),Group(Field("value",Scalar(FerruleValue.FromInt64(3L)))),"{\n  \"value\": 3\n}\n",null,null),
            new("old-unbounded-int",StringInt101Schema("old-unconstrained"),"{\"value\":1}",Group(Field("value",Scalar(FerruleValue.FromInt64(1L)))),Group(Field("value",Scalar(FerruleValue.FromInt64(1L)))),"{\n  \"value\": 1\n}\n",null,null),
            new("old-text-tag",StringInt101Schema("old-unconstrained"),"{\"value\":\"1\"}",Group(Field("value",Scalar(FerruleValue.FromString("1")))),Group(Field("value",Scalar(FerruleValue.FromString("1")))),"{\n  \"value\": \"1\"\n}\n",null,null),
            new("array-items",StringInt101Schema("array"),"{\"values\":[2,\"6\",null,5]}",Group(Field("values",new FerruleRepeated([Scalar(FerruleValue.FromInt64(2L)),Scalar(FerruleValue.FromString("6")),Scalar(FerruleValue.JsonNull),Scalar(FerruleValue.FromInt64(5L))]))),Group(Field("values",new FerruleRepeated([Scalar(FerruleValue.FromInt64(2L)),Scalar(FerruleValue.FromString("6")),Scalar(FerruleValue.JsonNull),Scalar(FerruleValue.FromInt64(5L))]))),"{\n  \"values\": [\n    2,\n    \"6\",\n    null,\n    5\n  ]\n}\n",null,null),
            new("array-wrapper-null",StringInt101Schema("array"),"{\"values\":null}",Group(Field("values",Scalar(FerruleValue.JsonNull))),Group(Field("values",Scalar(FerruleValue.JsonNull))),"{\n  \"values\": null\n}\n",null,null),
            new("array-empty",StringInt101Schema("array"),"{\"values\":[]}",Group(Field("values",new FerruleRepeated([]))),Group(Field("values",new FerruleRepeated([]))),"{\n  \"values\": []\n}\n",null,null),
            new("independent-string-assertions-string",StringInt101Schema("independent-string-assertions"),"{\"value\":\"xx\"}",Group(Field("value",Scalar(FerruleValue.FromString("xx")))),Group(Field("value",Scalar(FerruleValue.FromString("xx")))),"{\n  \"value\": \"xx\"\n}\n",null,null),
            new("independent-string-assertions-int",StringInt101Schema("independent-string-assertions"),"{\"value\":2}",Group(Field("value",Scalar(FerruleValue.FromInt64(2L)))),Group(Field("value",Scalar(FerruleValue.FromInt64(2L)))),"{\n  \"value\": 2\n}\n",null,null),
            new("multiple-string3",StringInt101Schema("independent-multiple-of"),"{\"value\":\"3\"}",Group(Field("value",Scalar(FerruleValue.FromString("3")))),Group(Field("value",Scalar(FerruleValue.FromString("3")))),"{\n  \"value\": \"3\"\n}\n",null,null),
            new("multiple-int2",StringInt101Schema("independent-multiple-of"),"{\"value\":2}",Group(Field("value",Scalar(FerruleValue.FromInt64(2L)))),Group(Field("value",Scalar(FerruleValue.FromInt64(2L)))),"{\n  \"value\": 2\n}\n",null,null),
            new("late-good-control",StringInt101Schema("bounded"),"{\"value\":3}",Group(Field("value",Scalar(FerruleValue.FromInt64(3L)))),Group(Field("value",Scalar(FerruleValue.FromInt64(3L)))),"{\n  \"value\": 3\n}\n",null,null),
            new("old-root-int","{\"name\":\"value\",\"repeating\":false,\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"int\"]}}","1",Scalar(FerruleValue.FromInt64(1)),null,null,null,null),
            new("root-string100","{\"name\":\"value\",\"repeating\":false,\"numeric_range\":{\"kind\":\"integer\",\"bounds\":{\"minimum\":2,\"maximum\":5}},\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"int\"]}}","\"100\"",Scalar(FerruleValue.FromString("100")),Scalar(FerruleValue.FromString("100")),"\"100\"\n",null,null),
            new("old-root-string","{\"name\":\"value\",\"repeating\":false,\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"int\"]}}",null,null,Scalar(FerruleValue.FromString("1")),"\"1\"\n",null,null),
            new("old-int-monotype-coercion","{\"name\":\"value\",\"repeating\":false,\"numeric_range\":{\"kind\":\"integer\",\"bounds\":{\"minimum\":2,\"maximum\":5}},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",null,null,Scalar(FerruleValue.FromString("3")),"3\n",null,null),
        ];
        var originals = new List<StringInt101Original>();
        foreach (var @case in cases)
        {
            if (@case.Document is { } document)
            {
                originals.Add(new(@case,"Parse",StringInt101Capture(() => FerruleJson.Parse(@case.Schema,document))));
                originals.Add(new(@case,"ParseBytes",StringInt101Capture(() => FerruleJson.ParseBytes(@case.Schema,Encoding.UTF8.GetBytes(document)))));
            }
            if (@case.OutputInput is { } instance)
            {
                originals.Add(new(@case,"Serialize",StringInt101Capture(() => FerruleJson.Serialize(@case.Schema,instance))));
                originals.Add(new(@case,"SerializeBytes",StringInt101Capture(() => FerruleJson.SerializeBytes(@case.Schema,instance))));
            }
        }
        // Retain and emit all public outcomes before the first assertion.
        // Capture never assumes an expected instance kind or scalar tag.
        Console.WriteLine("STRING_INT101_CSHARP_DIRECT_ORIGINALS " + JsonSerializer.Serialize(originals.Select(original => new
        {
            name = original.Case.Name, api = original.Api, schema_text = original.Case.Schema,
            input_text = original.Case.Document,
            input_utf8 = original.Case.Document is { } text ? Encoding.UTF8.GetBytes(text).Select(value => (int)value).ToArray() : null,
            expected_input = StringInt101Instance(original.Case.ExpectedInput),
            output_input = StringInt101Instance(original.Case.OutputInput),
            expected_output = original.Case.ExpectedOutput,
            input_error = original.Case.InputError, output_error = original.Case.OutputError,
            actual = StringInt101OriginalValue(original.Outcome.Value), error = StringInt101Error(original.Outcome.Error),
        }).ToArray()));
        foreach (var original in originals)
        {
            var input = original.Api is "Parse" or "ParseBytes";
            var expectedError = input ? original.Case.InputError : original.Case.OutputError;
            if (expectedError is { } message)
            {
                var error = original.Outcome.Error as FerruleRuntimeException
                    ?? throw new InvalidOperationException("Expected original public boundary error.");
                Equal(FerruleRuntimeError.JsonBoundary,error.Error);
                Equal(message,error.Message);
                Equal(message,error.Detail);
                Equal<Exception?>(null,error.InnerException);
                Equal<object?>(null,original.Outcome.Value);
                object?[] unusedPayloads =
                [
                    error.Node,error.Function,error.ExpectedArity,error.ActualArity,error.FoundKind,
                    error.AggregateOperation,error.RequestedItems,error.MaximumItems,error.MaximumDepth,
                    error.RuntimeValue,error.FailureRule,error.MappingFailureMessage,error.MappingExceptionMessage,
                    error.Join,error.UserFunction,error.FunctionParameter,error.ExpectedScalarType,
                    error.RuntimeParameter,error.SourceField,error.FoundInstance,error.PrimaryRoot,
                ];
                Equal(true,unusedPayloads.All(value => value is null));
                continue;
            }
            Equal<Exception?>(null,original.Outcome.Error);
            if (input)
            {
                Equal(JsonSerializer.Serialize(StringInt101Instance(original.Case.ExpectedInput)),
                    JsonSerializer.Serialize(StringInt101Instance(original.Outcome.Value as FerruleInstance)));
            }
            else if (original.Api == "Serialize")
            {
                Equal(original.Case.ExpectedOutput,original.Outcome.Value as string);
            }
            else
            {
                var bytes = original.Outcome.Value as byte[]
                    ?? throw new InvalidOperationException("Expected original public UTF-8 output bytes.");
                var expected = original.Case.ExpectedOutput
                    ?? throw new InvalidOperationException("Authored complete expected output is required.");
                Equal(Convert.ToHexString(Encoding.UTF8.GetBytes(expected)),Convert.ToHexString(bytes));
            }
        }
    }

    private static object? StringInt101OriginalValue(object? value) => value switch
    {
        null => null,
        FerruleInstance instance => StringInt101Instance(instance),
        string text => new { kind = "text", text, utf8 = Encoding.UTF8.GetBytes(text).Select(value => (int)value).ToArray() },
        byte[] bytes => new { kind = "bytes", utf8 = bytes.Select(value => (int)value).ToArray(), text = Encoding.UTF8.GetString(bytes) },
        _ => new { kind = value.GetType().FullName, original = value.ToString() },
    };

    private static object StringInt101Value(FerruleValue value)
    {
        object? payload = value.Kind switch
        {
            FerruleValueKind.String => (object)value.StringValue,
            FerruleValueKind.Int64 => value.Int64Value,
            FerruleValueKind.Bool => value.BooleanValue,
            FerruleValueKind.Double => new
            {
                bits = unchecked((ulong)BitConverter.DoubleToInt64Bits(value.DoubleValue)).ToString("x16",CultureInfo.InvariantCulture),
                display = value.DoubleValue.ToString("R",CultureInfo.InvariantCulture),
            },
            _ => null,
        };
        return new { tag = value.Kind.ToString(), payload };
    }

    private static object? StringInt101Instance(FerruleInstance? instance) => instance switch
    {
        null => null,
        FerruleScalar scalar => new { kind = "Scalar", value = StringInt101Value(scalar.Value) },
        FerruleGroup group => new
        {
            kind = "Group", origin_kind = group.XmlTypeOrigin.Kind.ToString(),
            origin_identity = group.XmlTypeOrigin.Identity, origin_literal = group.XmlTypeOrigin.Literal,
            fields = group.Fields.Select(field => new { name = field.Name, value = StringInt101Instance(field.Value) }).ToArray(),
        },
        FerruleRepeated repeated => new { kind = "Repeated", items = repeated.Items.Select(StringInt101Instance).ToArray() },
        FerruleMappedSequence sequence => new { kind = "MappedSequence", items = sequence.Items.Select(StringInt101Instance).ToArray() },
        FerruleDocumentSet documents => new
        {
            kind = "DocumentSet", documents = documents.Documents.Select(document => new
            {
                path = document.Path, resolved_source_path = document.ResolvedSourcePath,
                value = StringInt101Instance(document.Value),
            }).ToArray(),
        },
        _ => new { kind = instance.GetType().FullName, original = instance.ToString() },
    };

    private static object? StringInt101Error(Exception? original)
    {
        if (original is null) { return null; }
        var fields = new Dictionary<string,object?>
        {
            ["type"] = original.GetType().FullName, ["message"] = original.Message,
            ["original"] = original.ToString(), ["hresult"] = original.HResult,
            ["source"] = original.Source, ["stack_trace"] = original.StackTrace,
            ["target_site"] = original.TargetSite?.ToString(),
            ["inner_exception"] = StringInt101Error(original.InnerException),
            ["data"] = original.Data.Keys.Cast<object>().Select(key => new
            {
                key = key.ToString(), value = original.Data[key]?.ToString(),
            }).ToArray(),
        };
        if (original is FerruleRuntimeException error)
        {
            fields["runtime"] = new
            {
                category = error.Error.ToString(), error.Node, error.Function,
                error.ExpectedArity, error.ActualArity, error.FoundKind, error.AggregateOperation,
                error.Detail, error.RequestedItems, error.MaximumItems, error.MaximumDepth, error.RuntimeValue,
                error.FailureRule, error.MappingFailureMessage, error.MappingExceptionMessage,
                error.Join, error.UserFunction, error.FunctionParameter, error.ExpectedScalarType,
                error.RuntimeParameter, error.SourceField, error.FoundInstance, error.PrimaryRoot,
            };
        }
        if (original is JsonException syntax)
        {
            fields["json"] = new { syntax.Path, syntax.LineNumber, syntax.BytePositionInLine };
        }
        return fields;
    }
}

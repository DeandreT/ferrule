using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void MapForceJson5Typed()
    {
        string schema=File.ReadAllText(Path.Combine(AppContext.BaseDirectory,"json5-typed-schema.json"));
        var value=new FerruleGroup(new FerruleField[]{
            new("label",new FerruleScalar(FerruleValue.FromString("fine"))),
            new("qty",new FerruleScalar(FerruleValue.FromDecimal(FerruleDecimal96.Zero))),
            new("line",new FerruleScalar(FerruleValue.FromString("9007199254740993"))),
            new("omit",new FerruleScalar(FerruleValue.Null)),
            new("explicitNull",new FerruleScalar(FerruleValue.JsonNull)),
            new("numbers",new FerruleRepeated(new FerruleInstance[]{
                new FerruleScalar(FerruleValue.FromDecimal(FerruleDecimal96.ParseExact("1.234567890123456785"))),
                new FerruleScalar(FerruleValue.FromDecimal(FerruleDecimal96.ParseExact("79228162514264337593543950335"))),
            })),
        });
        string actual=FerruleJson.SerializeMapForceJson5AsciiKeys(schema,value);
        const string expected="{\r\n\tlabel: \"fine\",\r\n\tqty: 0,\r\n\tline: 9007199254740993,\r\n\texplicitNull: null,\r\n\tnumbers: [\r\n\t\t1.23456789012345679,\r\n\t\t79228162514264337593543950335\r\n\t]\r\n}";
        if(actual!=expected)throw new InvalidOperationException("Typed JSON5 writer changed decimal or absence semantics.");
        bool strictRefused = false;
        try { FerruleJson.SerializeEmbedded(schema, value); }
        catch (FerruleRuntimeException) { strictRefused = true; }
        if (!strictRefused) throw new InvalidOperationException("The strict writer was silently weakened.");
        const string textSchema = "{\"name\":\"text\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}";
        var text = new FerruleScalar(FerruleValue.FromString("é"));
        if (FerruleJson.SerializeMapForceJson5AsciiKeys(textSchema, text, 4) != "\"é\"")
            throw new InvalidOperationException("Typed JSON5 changed its exact byte limit.");
        foreach (var (descriptor, input, maximum, error) in new[]
        {
            (textSchema, text, 3, FerruleMapForceJson5Error.OutputBytes),
            (textSchema, text, -1, FerruleMapForceJson5Error.InvalidLimit),
            (textSchema, new FerruleScalar(FerruleValue.Null), 1024, FerruleMapForceJson5Error.Shape),
            ("{", text, 1024, FerruleMapForceJson5Error.Schema),
            ("{\"name\":\"root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"unused\",\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"int\"]}}]}}",
                (FerruleInstance)new FerruleGroup(Array.Empty<FerruleField>()), 1024, FerruleMapForceJson5Error.Shape),
        })
        {
            try { FerruleJson.SerializeMapForceJson5AsciiKeys(descriptor, input, maximum); }
            catch (FerruleMapForceJson5Exception exception) when (exception.Error == error) { continue; }
            throw new InvalidOperationException($"Typed JSON5 did not refuse {error}.");
        }
    }
}

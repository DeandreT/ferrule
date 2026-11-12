using System.Collections;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

var utf8 = new UTF8Encoding(false, true);
var outcomes = new JsonArray();
int failed = 0, cases = 0, calls = 0;
foreach (JsonNode? library in Read("libraries.json").AsArray())
{
    string name = library!["name"]!.GetValue<string>();
    string identity = library["assembly"]!.GetValue<string>();
    Assembly assembly = Assembly.LoadFrom(Path.GetFullPath($"{name}/bin/Release/net10.0/{identity}.dll"));
    Type map = assembly.GetType("Ferrule.Generated.GeneratedMapping", true)!;
    JsonNode schema = Read($"{name}-schema.json");
    object context = assembly.GetType("Ferrule.Runtime.FerruleExecutionContext")!
        .GetConstructors().Single(ctor => ctor.GetParameters().Length == 4)
        .Invoke(["gs05-control", "gs05-control", library["context"]!.GetValue<string>(), null]);
    foreach (JsonNode? entry in Read($"{name}-cases.json").AsArray())
    {
        JsonNode selected = entry!;
        cases++;
        string label = $"{name}-{selected["label"]!.GetValue<string>()}";
        JsonNode value = selected["input"]!;
        object supplied = Make(assembly, schema, value);
        JsonNode before = Snapshot(supplied);
        bool executeJson = selected["execute_json"]!.GetValue<bool>();
        foreach (bool bytes in new[] { false, true })
        {
            string method = bytes ? "SerializeX12Bytes" : "SerializeX12";
            Check($"{label}-{method}", () => Call(map, method, supplied, context), selected);
            if (executeJson)
            {
                string source = value.ToJsonString();
                method = bytes ? "ExecuteJsonToX12Bytes" : "ExecuteJsonToX12";
                object input = bytes ? utf8.GetBytes(source) : source;
                Check($"{label}-{method}", () => Call(map, method, input, context), selected);
            }
        }
        bool unchanged = JsonNode.DeepEquals(before, Snapshot(supplied));
        outcomes.Add(new JsonObject { ["label"] = label + "-caller-unchanged", ["pass"] = unchanged });
        if (!unchanged) failed++;
        Flush();
    }
}
Console.WriteLine(JsonSerializer.Serialize(new { cases, calls, failed }));
return failed == 0 ? 0 : 1;

JsonNode Read(string path) => JsonNode.Parse(File.ReadAllText(path))!;
void Flush() => File.WriteAllText("complete-outcomes.original.json", outcomes.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
object? Property(object value, string name) => value.GetType().GetProperty(name)!.GetValue(value);
object? Call(Type type, string name, params object?[] arguments)
{
    MethodInfo method = type.GetMethods(BindingFlags.Public | BindingFlags.Static)
        .Single(candidate => candidate.Name == name && candidate.GetParameters().Length == arguments.Length);
    try { return method.Invoke(null, arguments); }
    catch (TargetInvocationException error) when (error.InnerException is not null) { throw error.InnerException; }
}
JsonNode Error(Exception error) => new JsonObject {
    ["type"] = error.GetType().FullName, ["error"] = error.GetType().GetProperty("Error")?.GetValue(error)?.ToString(),
    ["message"] = error.Message, ["segment_index"] = JsonSerializer.SerializeToNode(error.GetType().GetProperty("SegmentIndex")?.GetValue(error)),
    ["field_path"] = JsonSerializer.SerializeToNode(error.GetType().GetProperty("FieldPath")?.GetValue(error)), ["full_original"] = error.ToString(),
};
void Check(string label, Func<object?> action, JsonNode selected)
{
    calls++;
    byte[]? actual = null;
    JsonNode? error = null;
    try { actual = action() switch { string text => utf8.GetBytes(text), byte[] bytes => bytes, _ => throw new InvalidOperationException("Unexpected public result") }; }
    catch (Exception failure) { error = Error(failure); }
    byte[]? expected = selected["expected_wire"] is JsonNode wire ? utf8.GetBytes(wire.GetValue<string>()) : null;
    bool pass = selected["expected_error"] is JsonNode want
        ? error is not null && new[] { "type", "error", "message", "segment_index", "field_path" }
            .All(field => JsonNode.DeepEquals(error[field], want[field]))
        : error is null && actual is not null && expected is not null && actual.AsSpan().SequenceEqual(expected);
    outcomes.Add(new JsonObject {
        ["label"] = label, ["pass"] = pass, ["error"] = error,
        ["actual_bytes_base64"] = actual is null ? null : Convert.ToBase64String(actual),
        ["expected_bytes_base64"] = expected is null ? null : Convert.ToBase64String(expected),
        ["expected_error"] = selected["expected_error"]?.DeepClone(),
    });
    if (!pass) failed++;
    Flush();
}
object Make(Assembly assembly, JsonNode schema, JsonNode? value, bool item = false)
{
    Type instance = assembly.GetType("Ferrule.Runtime.FerruleInstance")!;
    Array TypedArray(Type type, IEnumerable<object> values)
    {
        object[] items = values.ToArray();
        Array array = Array.CreateInstance(type, items.Length);
        for (int index = 0; index < items.Length; index++) array.SetValue(items[index], index);
        return array;
    }
    if (!item && schema["repeating"]?.GetValue<bool>() == true)
        return Activator.CreateInstance(assembly.GetType("Ferrule.Runtime.FerruleRepeated")!,
            TypedArray(instance, value!.AsArray().Select(child => Make(assembly, schema, child, true))))!;
    JsonNode kind = schema["kind"]!;
    if (kind["kind"]!.GetValue<string>() == "group")
    {
        Type field = assembly.GetType("Ferrule.Runtime.FerruleField")!;
        var fields = new List<object>();
        foreach (JsonNode? child in kind["children"]!.AsArray())
            if (value!.AsObject().TryGetPropertyValue(child!["name"]!.GetValue<string>(), out JsonNode? supplied))
                fields.Add(Activator.CreateInstance(field, child["name"]!.GetValue<string>(), Make(assembly, child, supplied))!);
        return Activator.CreateInstance(assembly.GetType("Ferrule.Runtime.FerruleGroup")!, TypedArray(field, fields))!;
    }
    Type scalar = assembly.GetType("Ferrule.Runtime.FerruleValue")!;
    object raw = value is null ? scalar.GetProperty("Null", BindingFlags.Public | BindingFlags.Static)!.GetValue(null)!
        : Call(scalar, "FromString", value.GetValue<string>())!;
    return Activator.CreateInstance(assembly.GetType("Ferrule.Runtime.FerruleScalar")!, raw)!;
}
JsonNode Snapshot(object value)
{
    if (value.GetType().Name == "FerruleGroup")
    {
        var fields = new JsonArray();
        foreach (object field in (IEnumerable)Property(value, "Fields")!)
            fields.Add(new JsonObject { ["name"] = (string)Property(field, "Name")!, ["value"] = Snapshot(Property(field, "Value")!) });
        return new JsonObject { ["kind"] = "Group", ["fields"] = fields };
    }
    if (value.GetType().Name == "FerruleRepeated")
    {
        var items = new JsonArray();
        foreach (object item in (IEnumerable)Property(value, "Items")!) items.Add(Snapshot(item));
        return new JsonObject { ["kind"] = "Repeated", ["items"] = items };
    }
    object raw = Property(value, "Value")!;
    string kind = Property(raw, "Kind")!.ToString()!;
    return new JsonObject { ["kind"] = kind, ["value"] = kind == "String" ? (string)Property(raw, "StringValue")! : null };
}

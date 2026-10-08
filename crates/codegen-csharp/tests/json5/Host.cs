using System.Reflection;
using System.Runtime.Loader;
using System.Text;
using System.Text.Json;

if (args.Length != 1) { throw new ArgumentException("One owned cohort directory is required."); }
var root = Path.GetFullPath(args[0]);
var casesPath = Path.Combine(root, "CASES.json");
using var cases = JsonDocument.Parse(File.ReadAllBytes(casesPath));
var total = 0; var argumentControls = 0;
foreach (var fixture in cases.RootElement.GetProperty("fixtures").EnumerateArray())
{
    var name = fixture.GetString()!;
    var path = Path.Combine(root, name, "bin", "Debug", "net10.0", "Ferrule.Generated.dll");
    var loading = new AssemblyLoadContext("json5-" + name, isCollectible: true);
    try
    {
        var assembly = loading.LoadFromAssemblyPath(path);
        var mapping = assembly.GetType("Ferrule.Generated.GeneratedMapping", throwOnError: true)!;
        var contextType = assembly.GetType("Ferrule.Runtime.FerruleExecutionContext", throwOnError: true)!;
        foreach (var item in cases.RootElement.GetProperty("cases").EnumerateArray())
        {
            if (item.GetProperty("fixture").GetString() != name) { continue; }
            for (var route = 0; route < 4; route++)
            {
                var input = item.GetProperty("input").GetString()!;
                var bytes = route >= 2;
                var withContext = route % 2 != 0;
                var parameterTypes = withContext
                    ? new[] { bytes ? typeof(byte[]) : typeof(string), contextType }
                    : new[] { bytes ? typeof(byte[]) : typeof(string) };
                var methodName = bytes ? "ExecuteJson5Bytes" : "ExecuteJson5";
                var method = mapping.GetMethod(methodName, parameterTypes)
                    ?? throw new InvalidOperationException("Missing exact optional public signature.");
                var supplied = new List<object> { bytes ? Encoding.UTF8.GetBytes(input) : input };
                if (withContext)
                { supplied.Add(Activator.CreateInstance(contextType, new object[] { "/logical/active.json" }) ?? throw new InvalidOperationException("Context construction failed.")); }
                object? output = null; Exception? originalWrapper = null; Exception? boundary = null;
                try { output = method.Invoke(null, supplied.ToArray()); }
                catch (Exception original)
                { originalWrapper = original; boundary = original is TargetInvocationException reflection ? reflection.InnerException : original; }
                // Complete originals and wrappers precede every oracle assertion.
                var evidence = JsonSerializer.Serialize(new
                {
                    fixture = name, id = item.GetProperty("id").GetString(), route, method = method.ToString(),
                    inputUtf16 = input.Select(character => (int)character).ToArray(), inputHex = Convert.ToHexString(Encoding.UTF8.GetBytes(input)),
                    text = output as string, bytesHex = output is byte[] raw ? Convert.ToHexString(raw) : null,
                    suppliedContext = withContext ? "/logical/active.json" : null,
                    originalWrapper = Cause(originalWrapper), actualBoundary = Cause(boundary),
                });
                var stem = name + "-" + item.GetProperty("id").GetString() + "-route" + route;
                File.WriteAllText(Path.Combine(root, stem + "-ORIGINAL.json"), evidence + "\n", new UTF8Encoding(false, true));
                Console.WriteLine(evidence); total++;
                var expected = item.TryGetProperty("with_context", out var contextual)
                    ? withContext ? contextual : item.GetProperty("without_context") : item;
                if (expected.TryGetProperty("output", out var outputValue) && outputValue.ValueKind == JsonValueKind.String)
                {
                    Check(originalWrapper is null && boundary is null, "unexpected public exception");
                    var wanted = outputValue.GetString()!;
                    if (bytes) { Check(output is byte[] actual && actual.SequenceEqual(Encoding.UTF8.GetBytes(wanted)), "complete output byte mismatch"); }
                    else { Check(output is string text && text == wanted, "complete output text mismatch"); }
                }
                else
                {
                    Check(output is null, "failure published a result");
                    Check(originalWrapper is TargetInvocationException && ReferenceEquals(originalWrapper.InnerException, boundary), "actual reflection wrapper/identity missing");
                    Check(boundary?.GetType().FullName == "Ferrule.Runtime.FerruleJson5BoundaryException", "missing optional boundary wrapper");
                    Check(Property(boundary!, "Stage")?.ToString() == expected.GetProperty("stage").GetString(), "wrong stage");
                    var inner = boundary!.InnerException;
                    if (expected.TryGetProperty("syntax_kind", out var syntax) && syntax.ValueKind == JsonValueKind.String)
                    {
                        Check(inner?.GetType().FullName == "Ferrule.Runtime.FerruleJson5SyntaxException", "original syntax cause missing");
                        Check(Property(inner!, "Kind")?.ToString() == syntax.GetString(), "wrong syntax kind");
                        if (expected.TryGetProperty("utf8_offset", out var offset))
                        {
                            Check(Convert.ToInt64(Property(boundary!, "Utf8Offset")) == offset.GetInt64(), "wrong original boundary UTF8 offset");
                            Check(Convert.ToInt64(Property(inner!, "Offset")) == offset.GetInt64(), "wrong original syntax UTF8 offset");
                        }
                    }
                    if (expected.TryGetProperty("input_kind", out var inputKind))
                    {
                        if (inputKind.GetString() == "RootObjectRequired")
                        {
                            Check(inner?.GetType().FullName == "Ferrule.Runtime.FerruleJson5SchemaException", "original root-object cause missing");
                            Check(Property(inner!, "Kind")?.ToString() == "RootObjectRequired", "wrong root-object kind");
                        }
                        else
                        {
                            Check(inner?.GetType().FullName == "Ferrule.Runtime.FerruleRuntimeException", "original projection cause missing");
                            Check(Property(inner!, "Error")?.ToString() == "JsonBoundary", "wrong projection category");
                        }
                    }
                    if (expected.TryGetProperty("runtime_error", out var runtime))
                    {
                        Check(inner?.GetType().FullName == "Ferrule.Runtime.FerruleRuntimeException", "original mapping cause missing");
                        Check(Property(inner!, "Error")?.ToString() == runtime.GetString(), "wrong runtime category");
                        if (expected.TryGetProperty("node", out var node))
                        { Check(Convert.ToUInt32(Property(inner!, "Node")) == node.GetUInt32(), "wrong Raise owner"); }
                        if (expected.TryGetProperty("message", out var message))
                        {
                            var actual = Property(inner!, "MappingExceptionMessage");
                            Check(message.ValueKind == JsonValueKind.Null ? actual is null : actual is string text && text == message.GetString(), "message presence/value changed");
                        }
                        if (name == "Context")
                        { Check(Property(inner!, "RuntimeValue")?.ToString() == "MappingFilePath", "wrong missing context value"); }
                    }
                }
            }
        }
        if (name == "I")
        {
            for (var route = 0; route < 4; route++)
            {
                var bytes = route >= 2; var withContext = route % 2 != 0;
                var method = mapping.GetMethod(bytes ? "ExecuteJson5Bytes" : "ExecuteJson5", withContext
                    ? new[] { bytes ? typeof(byte[]) : typeof(string), contextType }
                    : new[] { bytes ? typeof(byte[]) : typeof(string) })!;
                var arguments = withContext
                    ? new object?[] { null, Activator.CreateInstance(contextType, new object[] { "/logical/active.json" }) }
                    : new object?[] { null };
                NullGuard(method, arguments, "source", "null-source-route" + route);
                if (withContext)
                { NullGuard(method, new object?[] { bytes ? Encoding.UTF8.GetBytes("{}") : "{}", null }, "executionContext", "null-context-route" + route); }
            }
            void NullGuard(MethodInfo method, object?[] arguments, string parameter, string id)
            {
                object? output = null; Exception? wrapper = null;
                try { output = method.Invoke(null, arguments); } catch (Exception error) { wrapper = error; }
                var evidence = JsonSerializer.Serialize(new { id, method = method.ToString(), output, originalWrapper = Cause(wrapper) });
                File.WriteAllText(Path.Combine(root, id + "-ORIGINAL.json"), evidence + "\n"); Console.WriteLine(evidence); argumentControls++;
                Check(output is null && wrapper is TargetInvocationException { InnerException: ArgumentNullException }, "null guard wrapper changed");
                var cause = (ArgumentNullException)wrapper!.InnerException!;
                Check(cause.ParamName == parameter && cause.InnerException is null, "wrong null guard parameter or cause");
            }
        }
    }
    finally { loading.Unload(); }
}
File.WriteAllText(Path.Combine(root, "COMPLETE_CALL_COUNT.json"), JsonSerializer.Serialize(new { total, expected = 92, argumentControls, expectedArgumentControls = 6 }) + "\n");
Check(total == 92, "incomplete public cohort");
Check(argumentControls == 6, "incomplete C#-only argument controls");

static object? Property(object value, string name) => value.GetType().GetProperty(name)?.GetValue(value);
static object? Cause(Exception? error) => error is null ? null : new
{
    type = error.GetType().FullName, fullOriginal = error.ToString(),
    publicFields = error.GetType().GetProperties().Where(property => property.Name != "InnerException" && property.GetIndexParameters().Length == 0)
        .ToDictionary(property => property.Name, property => property.GetValue(error)?.ToString()),
    inner = Cause(error.InnerException),
};
static void Check(bool accepted, string reason) { if (!accepted) { throw new InvalidOperationException(reason); } }

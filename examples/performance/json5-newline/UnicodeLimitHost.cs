// Separate opt-in Unicode cap controls; not part of the six-recipe profile matrix.
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Ferrule.Runtime;

if (args.Length != 4) throw new ArgumentException("exact/plus1, text/bytes, owned output directory and literal expected SHA256 required");
var plus = args[0] == "plus1";
if (!plus && args[0] != "exact") throw new ArgumentException("unknown case");
var bytesRoute = args[1] == "bytes";
if (!bytesRoute && args[1] != "text") throw new ArgumentException("unknown route");
var output = Path.GetFullPath(args[2]);
if (!Directory.Exists(output)) throw new DirectoryNotFoundException(output);
const long M = 67_108_864;
const int Count = 22_369_616;
const string Source = "{\"name\":\"Input\",\"kind\":{\"kind\":\"group\",\"children\":[]}}";
const string Target = "{\"name\":\"Output\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"n\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}}";
var value = new string('雪', Count) + new string('a', plus ? 3 : 2);
var utf8 = new UTF8Encoding(false, true);
FerruleInstance Mapping(FerruleInstance _) => new FerruleGroup([new("n", new FerruleScalar(FerruleValue.FromString(value)))]);
object? actual = null;
Exception? original = null;
try
{
    actual = bytesRoute
        ? FerruleJson5.ExecuteBytes(Source, Target, "{}"u8.ToArray(), Mapping)
        : FerruleJson5.Execute(Source, Target, "{}", Mapping);
}
catch (Exception error) { original = error; }
byte[]? returned = actual switch { string text => utf8.GetBytes(text), byte[] bytes => bytes, null => null, _ => throw new InvalidOperationException("unexpected public result type") };
if (returned is not null) File.WriteAllBytes(Path.Combine(output, "ORIGINAL-RETURNED-UTF8.bin"), returned);
Save("ORIGINAL.json", new { plus, bytesRoute, count=Count, padding=plus?3:2,
    literalOutputBytes=M+(plus?1:0), actualType=actual?.GetType().FullName,
    returnedBytes=returned?.LongLength, returnedSha256=returned is null?null:Hash(returned),
    original=Cause(original), assembly=typeof(FerruleJson5).Assembly.Location,
    assemblySha256=Hash(File.ReadAllBytes(typeof(FerruleJson5).Assembly.Location)) });
if (plus)
{
    Check(actual is null && returned is null, "refusal returned a partial value");
    Check(original is FerruleJson5BoundaryException {
        Stage: FerruleJson5Stage.Output, Resource: FerruleJson5Resource.OutputDocumentBytes,
        Requested: M+1, Maximum: M, InnerException: FerruleJson5ResourceException {
            Resource: FerruleJson5Resource.OutputDocumentBytes, Requested: M+1, Maximum: M, InnerException: null
        }
    }, "Unicode M+1 output cause/fields changed");
}
else
{
    Check(original is null && returned is not null, "Unicode M output refused");
    Check(returned!.LongLength == M && Hash(returned) == args[3], "complete literal output size/hash mismatch");
    Check(returned.AsSpan(0,10).SequenceEqual("{\n  \"n\": \""u8), "prefix mismatch");
    for (var i=0; i<Count; i++)
    {
        var offset=10+i*3;
        Check(returned[offset]==0xe9 && returned[offset+1]==0x9b && returned[offset+2]==0xaa, "full Unicode body mismatch");
    }
    Check(returned.AsSpan(10+Count*3).SequenceEqual("aa\"\n}\n"u8), "complete suffix mismatch");
}
Save("COMPLETE.json", new { calls=1, failures=0, plus, bytesRoute });

void Save(string name, object data) => File.WriteAllText(Path.Combine(output,name), JsonSerializer.Serialize(data,new JsonSerializerOptions { WriteIndented=true })+"\n",utf8);
static string Hash(byte[] bytes) => Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant();
static void Check(bool accepted,string message) { if (!accepted) throw new InvalidOperationException(message); }
static object? Cause(Exception? error) => error is null?null:new {
    type=error.GetType().FullName, fullOriginal=error.ToString(),
    fields=error.GetType().GetProperties().Where(property=>property.Name!="InnerException" && property.GetIndexParameters().Length==0)
        .ToDictionary(property=>property.Name,property=>property.GetValue(error)?.ToString()),
    inner=Cause(error.InnerException)
};

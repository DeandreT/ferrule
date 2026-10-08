using System.Text;
using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string JsonRequiredPrecedenceSchema =
        """
        {"name":"Envelope","kind":{"kind":"group","children":[
          {"name":"id","kind":{"kind":"scalar","ty":"int"}}
        ],"required":["id"]}}
        """;

    private const string JsonRequiredPrecedenceNullableSchema =
        """
        {"name":"Envelope","container_nullable":true,"kind":{"kind":"group","children":[
          {"name":"id","kind":{"kind":"scalar","ty":"int"}}
        ],"required":["id"]}}
        """;

    private sealed record JsonRequiredPrecedenceCase(
        string Name, string Document, string Schema, string? ExpectedMessage);

    private sealed record JsonRequiredPrecedenceOriginal(
        JsonRequiredPrecedenceCase Case, string Api, byte[] Utf8,
        FerruleInstance? Instance, Exception? Error);

    private static void JsonRequiredPropertyPrecedence()
    {
        const string missing = "JSON object 'Envelope' requires property 'id'.";
        const string extra = "JSON object 'Envelope' does not allow property 'extra'.";
        JsonRequiredPrecedenceCase[] cases =
        [
            new("mixed-invalid", """{"extra":1}""", JsonRequiredPrecedenceSchema, missing),
            new("missing-only", "{}", JsonRequiredPrecedenceSchema, missing),
            new("extra-only", """{"id":7,"extra":1}""", JsonRequiredPrecedenceSchema, extra),
            new("valid", """{"id":7}""", JsonRequiredPrecedenceSchema, null),
            new("nullable-object-null", "null", JsonRequiredPrecedenceNullableSchema, null),
            new("malformed-json", """{"extra":1""", JsonRequiredPrecedenceSchema, "JSON input is invalid."),
        ];
        var originals = new List<JsonRequiredPrecedenceOriginal>();
        foreach (var test in cases)
        {
            foreach (var api in new[] { "FerruleJson.Parse", "FerruleJson.ParseBytes" })
            {
                var bytes = Encoding.UTF8.GetBytes(test.Document);
                FerruleInstance? instance = null;
                Exception? error = null;
                try
                {
                    instance = api == "FerruleJson.Parse"
                        ? FerruleJson.Parse(test.Schema, test.Document)
                        : FerruleJson.ParseBytes(test.Schema, bytes);
                }
                catch (Exception original)
                {
                    error = original;
                }
                originals.Add(new(test, api, bytes, instance, error));
            }
        }

        // Every original object and exception remains alive through all assertions.
        // Emit all routes and controls first, including complete public error fields.
        Console.Error.WriteLine("ISSUE171_CSHARP_ORIGINALS " + JsonSerializer.Serialize(
            originals.Select(original => new
            {
                case_name = original.Case.Name,
                api = original.Api,
                schema_text = original.Case.Schema,
                input = original.Case.Document,
                utf8_bytes = original.Utf8.Select(value => (int)value).ToArray(),
                expected_message = original.Case.ExpectedMessage,
                instance = JsonRequiredPrecedenceInstance(original.Instance),
                error = JsonRequiredPrecedenceError(original.Error),
            })));
        Console.Error.Flush();

        foreach (var original in originals.OrderBy(original => original.Case.Name == "mixed-invalid" ? 1 : 0))
        {
            if (original.Case.ExpectedMessage is string expected)
            {
                if (original.Error is not FerruleRuntimeException error)
                {
                    throw new InvalidOperationException(
                        $"{original.Case.Name}/{original.Api}: expected JsonBoundary; " +
                        $"found {original.Error?.ToString() ?? "a successful result"}.");
                }
                Equal(FerruleRuntimeError.JsonBoundary, error.Error);
                Equal(expected, error.Message);
                Equal(expected, error.Detail);
                Equal<FerruleInstance?>(null, original.Instance);
                object?[] unusedRuntimePayloads =
                [
                    error.Node, error.Function, error.ExpectedArity, error.ActualArity,
                    error.FoundKind, error.AggregateOperation, error.RequestedItems,
                    error.MaximumItems, error.MaximumDepth, error.RuntimeValue,
                    error.FailureRule, error.MappingFailureMessage, error.MappingExceptionMessage,
                    error.Join, error.UserFunction, error.FunctionParameter, error.ExpectedScalarType,
                    error.RuntimeParameter, error.SourceField, error.FoundInstance, error.PrimaryRoot,
                ];
                Equal(true, unusedRuntimePayloads.All(value => value is null));
                if (original.Case.Name == "malformed-json")
                {
                    Equal(true, error.InnerException is JsonException);
                }
                else
                {
                    Equal<Exception?>(null, error.InnerException);
                }
                continue;
            }

            Equal<Exception?>(null, original.Error);
            if (original.Case.Name == "valid")
            {
                if (original.Instance is not FerruleGroup group)
                {
                    throw new InvalidOperationException("Valid Envelope must be a group.");
                }
                Equal(1, group.Fields.Count);
                Equal("id", group.Fields[0].Name);
                if (group.Fields[0].Value is not FerruleScalar scalar)
                {
                    throw new InvalidOperationException("Envelope.id must be a scalar.");
                }
                Equal(FerruleValue.FromInt64(7), scalar.Value);
            }
            else
            {
                Equal(true, original.Instance is FerruleScalar { Value.Kind: FerruleValueKind.JsonNull });
            }
        }
    }

    private static object? JsonRequiredPrecedenceInstance(FerruleInstance? instance) => instance switch
    {
        null => null,
        FerruleScalar scalar => new
        {
            kind = "scalar", scalar_kind = scalar.Value.Kind.ToString(), value = scalar.Value.ToString(),
        },
        FerruleGroup group => new
        {
            kind = "group", xml_origin_kind = group.XmlTypeOrigin.Kind.ToString(),
            xml_origin_identity = group.XmlTypeOrigin.Identity, xml_origin_literal = group.XmlTypeOrigin.Literal,
            fields = group.Fields.Select(field => new
            {
                name = field.Name, value = JsonRequiredPrecedenceInstance(field.Value),
            }).ToArray(),
        },
        FerruleRepeated repeated => new
        {
            kind = "repeated", items = repeated.Items.Select(JsonRequiredPrecedenceInstance).ToArray(),
        },
        FerruleMappedSequence sequence => new
        {
            kind = "mapped_sequence", items = sequence.Items.Select(JsonRequiredPrecedenceInstance).ToArray(),
        },
        FerruleDocumentSet documents => new
        {
            kind = "document_set", documents = documents.Documents.Select(document => new
            {
                path = document.Path, resolved_source_path = document.ResolvedSourcePath,
                value = JsonRequiredPrecedenceInstance(document.Value),
            }).ToArray(),
        },
        _ => new { kind = instance.GetType().FullName, original = instance.ToString() },
    };

    private static object? JsonRequiredPrecedenceError(Exception? original)
    {
        if (original is null)
        {
            return null;
        }
        var fields = new Dictionary<string, object?>
        {
            ["type"] = original.GetType().FullName,
            ["message"] = original.Message,
            ["original"] = original.ToString(),
            ["hresult"] = original.HResult,
            ["source"] = original.Source,
            ["stack_trace"] = original.StackTrace,
            ["target_site"] = original.TargetSite?.ToString(),
            ["inner_exception"] = JsonRequiredPrecedenceError(original.InnerException),
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
                error.ExpectedArity, error.ActualArity, error.FoundKind,
                error.AggregateOperation, error.Detail, error.RequestedItems,
                error.MaximumItems, error.MaximumDepth, error.RuntimeValue,
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

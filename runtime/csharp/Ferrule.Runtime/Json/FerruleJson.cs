using System.Buffers;
using System.Globalization;
using System.Numerics;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace Ferrule.Runtime;

/// <summary>Bounded schema-shaped JSON parsing and serialization for generated mappings.</summary>
public static partial class FerruleJson
{
    public const int MaximumSchemaBytes = 1024 * 1024;
    public const int MaximumDocumentBytes = 64 * 1024 * 1024;
    public const int MaximumDepth = 256;
    // Retained for keyword-specific validation and source compatibility;
    // ordinary schema/document traversal has no general node-count cap.
    public const int MaximumNodes = 1_000_000;

    // serde_json's default reader permits 127 nested JSON containers. Keep
    // syntax parsing separate from constructed-instance/output traversal.
    private const int MaximumParsedDepth = 127;

    private const int MaximumJsonFormats = 64;
    private const int MaximumJsonFormatBytes = 1024;
    private const int MaximumJsonFormatTotalBytes = 16 * 1024;
    private const int MaximumJsonPatternAlternatives = 32;
    private const int MaximumJsonPatternTerms = 64;
    private const int MaximumDistinctJsonPatterns = 64;
    private const int MaximumJsonPatternSourceBytes = 256 * 1024;
    private const int MaximumJsonPatternInstructions = 65_536;
    private const int MaximumRecursiveReferences = 64;

    private static readonly JsonSerializerOptions CanonicalJsonOptions = new()
    {
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    };

    private static readonly UTF8Encoding StrictUtf8 = new(false, true);

    public static FerruleInstance ParseBytes(string schemaJson, byte[] document) =>
        ParseBytesCore(schemaJson, document, embedded: false);

    /// <summary>Parses a generated host document with a versioned embedded schema.</summary>
    public static FerruleInstance ParseEmbeddedBytes(string descriptor, byte[] document) =>
        ParseBytesCore(descriptor, document, embedded: true);

    private static FerruleInstance ParseBytesCore(string schemaJson, byte[] document, bool embedded)
    {
        ArgumentNullException.ThrowIfNull(schemaJson);
        ArgumentNullException.ThrowIfNull(document);
        if (document.Length > MaximumDocumentBytes)
        {
            throw Boundary(
                $"JSON input is {document.Length} bytes; maximum is {MaximumDocumentBytes}.");
        }

        string text;
        try
        {
            text = StrictUtf8.GetString(document);
        }
        catch (DecoderFallbackException error)
        {
            throw Boundary("JSON input is not UTF-8.", error);
        }

        return ParseCore(schemaJson, text, enforceHostLimits: true, embedded);
    }

    public static FerruleInstance Parse(string schemaJson, string document)
        => ParseCore(schemaJson, document, enforceHostLimits: true, embedded: false);

    /// <summary>Parses a generated host document with a versioned embedded schema.</summary>
    public static FerruleInstance ParseEmbedded(string descriptor, string document)
        => ParseCore(descriptor, document, enforceHostLimits: true, embedded: true);

    // Graph-level json_parse_field receives ordinary runtime strings, not a
    // generated JSON host document. Native format_json::from_str has no
    // general byte or mapped-node cap for this function.
    internal static FerruleInstance ParseFunctionInput(string schemaJson, string document)
        => ParseCore(schemaJson, document, enforceHostLimits: false, embedded: false);

    private static FerruleInstance ParseCore(
        string schemaJson,
        string document,
        bool enforceHostLimits,
        bool embedded)
    {
        ArgumentNullException.ThrowIfNull(schemaJson);
        ArgumentNullException.ThrowIfNull(document);
        if (enforceHostLimits)
        {
            RequireUtf8Limit(schemaJson, MaximumSchemaBytes, "embedded JSON schema");
            RequireUtf8Limit(document, MaximumDocumentBytes, "JSON input");
        }
        var schema = ParseSchema(schemaJson, embedded);
        try
        {
            var input = document.Length > 0 && document[0] == '\uFEFF'
                ? document[1..]
                : document;
            using var parsed = JsonDocument.Parse(
                input,
                new JsonDocumentOptions
                {
                    MaxDepth = MaximumParsedDepth,
                    CommentHandling = JsonCommentHandling.Disallow,
                    AllowTrailingCommas = false,
                });
            ValidateInputJsonScalars(schema, parsed.RootElement);
            var budget = new NodeBudget();
            if (!schema.Repeating && !schema.IsScalar && !schema.JsonAny &&
                parsed.RootElement.ValueKind == JsonValueKind.Array)
            {
                budget.Visit(0);
                var rows = new List<FerruleInstance>();
                foreach (var row in parsed.RootElement.EnumerateArray())
                {
                    budget.Visit(1);
                    rows.Add(ReadSingleNode(schema, row, budget, 1));
                }

                return new FerruleRepeated(rows);
            }

            return ReadNode(schema, parsed.RootElement, budget, 0);
        }
        catch (FerruleRuntimeException)
        {
            throw;
        }
        catch (Exception error) when (
            error is JsonException or FormatException or InvalidOperationException or OverflowException)
        {
            throw Boundary("JSON input is invalid.", error);
        }
    }

    internal static void ValidateSchema(string schemaJson)
    {
        ArgumentNullException.ThrowIfNull(schemaJson);
        RequireUtf8Limit(schemaJson, MaximumSchemaBytes, "embedded JSON schema");
        _ = ParseSchema(schemaJson);
    }

    internal static void ValidateFunctionSchema(string schemaJson)
    {
        ArgumentNullException.ThrowIfNull(schemaJson);
        _ = ParseSchema(schemaJson);
    }

    public static string Serialize(string schemaJson, FerruleInstance instance)
        => SerializeCore(schemaJson, instance, embedded: false);

    /// <summary>Serializes a generated host result with a versioned embedded schema.</summary>
    public static string SerializeEmbedded(string descriptor, FerruleInstance instance)
        => SerializeCore(descriptor, instance, embedded: true);

    private static string SerializeCore(string schemaJson, FerruleInstance instance, bool embedded)
    {
        ArgumentNullException.ThrowIfNull(schemaJson);
        ArgumentNullException.ThrowIfNull(instance);
        RequireUtf8Limit(schemaJson, MaximumSchemaBytes, "embedded JSON schema");
        var schema = ParseSchema(schemaJson, embedded);
        try
        {
            var buffer = new ArrayBufferWriter<byte>();
            using (var writer = new Utf8JsonWriter(
                       buffer,
                       new JsonWriterOptions
                       {
                           Indented = true,
                           Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
                           MaxDepth = MaximumDepth,
                           SkipValidation = false,
                       }))
            {
                var budget = new NodeBudget();
                if (instance is FerruleRepeated repeated && !schema.Repeating)
                {
                    budget.Visit(0);
                    writer.WriteStartArray();
                    foreach (var item in repeated.Items)
                    {
                        WriteSingleNode(writer, schema, item, budget, 1);
                    }

                    writer.WriteEndArray();
                }
                else
                {
                    WriteNode(writer, schema, instance, budget, 0);
                }
            }

            var canonical = CanonicalizeOutputStrings(buffer.WrittenSpan);
            var outputBytes = checked(StrictUtf8.GetByteCount(canonical) + 1);
            if (outputBytes > MaximumDocumentBytes)
            {
                throw Boundary(
                    $"JSON output is {outputBytes} bytes; maximum is {MaximumDocumentBytes}.");
            }

            return canonical + "\n";
        }
        catch (FerruleRuntimeException)
        {
            throw;
        }
        catch (Exception error) when (
            error is JsonException or FormatException or InvalidOperationException or OverflowException or EncoderFallbackException)
        {
            throw Boundary("JSON output is invalid.", error);
        }
    }

    public static byte[] SerializeBytes(string schemaJson, FerruleInstance instance) =>
        StrictUtf8.GetBytes(Serialize(schemaJson, instance));

    /// <summary>Serializes a generated host result as UTF-8 bytes.</summary>
    public static byte[] SerializeEmbeddedBytes(string descriptor, FerruleInstance instance) =>
        StrictUtf8.GetBytes(SerializeEmbedded(descriptor, instance));

    private static JsonSchemaNode ParseSchema(string schemaJson, bool embedded = false)
    {
        try
        {
            var descriptor = embedded
                ? FerruleEmbeddedSchema.Unwrap(schemaJson, MaximumSchemaBytes)
                : new FerruleEmbeddedSchema.Descriptor(schemaJson, false);
            using var parsed = JsonDocument.Parse(
                descriptor.Payload,
                new JsonDocumentOptions
                {
                    MaxDepth = MaximumParsedDepth,
                    CommentHandling = JsonCommentHandling.Disallow,
                    AllowTrailingCommas = false,
                });
            var budget = new NodeBudget();
            var patterns = new JsonPatternSchemaContext();
            var root = ReadSchemaNode(
                parsed.RootElement, budget, patterns, 0, descriptor.ExactFloatMarkers);
            BindRecursiveReferences(root);
            return root;
        }
        catch (FerruleRuntimeException)
        {
            throw;
        }
        catch (Exception error) when (
            error is JsonException or FormatException or InvalidOperationException or OverflowException)
        {
            throw Boundary("Embedded JSON schema is invalid.", error);
        }
    }

    private static JsonSchemaNode ReadSchemaNode(
        JsonElement element,
        NodeBudget budget,
        JsonPatternSchemaContext patternContext,
        int depth,
        bool exactFloatMarkers)
    {
        budget.Visit(depth);
        RequireKind(element, JsonValueKind.Object, "schema node", "object");
        var name = RequiredString(element, "name");
        var kindElement = RequiredProperty(element, "kind");
        RequireKind(kindElement, JsonValueKind.Object, $"schema node '{name}' kind", "object");
        var kind = RequiredString(kindElement, "kind");
        var repeating = OptionalBoolean(element, "repeating");
        if (OptionalBoolean(element, "xml_optional") &&
            (name.Length == 0 || name is "#text" or "element()" or "attribute()" ||
             name.StartsWith('\u001f') || repeating ||
             OptionalBoolean(element, "attribute") || OptionalBoolean(element, "text") ||
             kind is not ("scalar" or "group")))
        {
            throw Boundary($"Embedded JSON schema node '{name}' has invalid optional XML occurrence metadata.");
        }
        if (OptionalBoolean(element, "xml_attribute_required") &&
            (name.Length == 0 || name is "#text" or "element()" or "attribute()" ||
             name.StartsWith('\u001f') || repeating ||
             !OptionalBoolean(element, "attribute") || OptionalBoolean(element, "text") ||
             kind != "scalar"))
        {
            throw Boundary($"Embedded JSON schema node '{name}' has invalid required XML attribute-use metadata.");
        }
        var recursiveReference = OptionalString(element, "recursive_ref");
        var jsonUniqueItems = ReadJsonUniqueItems(name, element, repeating);
        var scalarDomain = kind switch
        {
            "scalar" => ScalarDomain(
                name,
                RequiredString(kindElement, "ty")),
            "scalar_union" => ReadScalarDomain(name, kindElement),
            "group" => JsonScalarDomain.None,
            _ => throw Boundary(
                $"Embedded JSON schema node '{name}' has unknown kind '{kind}'."),
        };
        var jsonAny = OptionalBoolean(element, "json_any");
        var nullable = OptionalBoolean(element, "nullable");
        var jsonFormats = ReadJsonFormats(name, element, scalarDomain, jsonAny);
        var fixedLexical = OptionalString(element, "fixed");
        FerruleValue? fixedValue = null;
        if (fixedLexical is not null)
        {
            if (!IsSingleScalar(scalarDomain) || jsonAny)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has a fixed value without one concrete scalar type.");
            }
            fixedValue = ParseFixed(name, SingleScalar(scalarDomain), fixedLexical);
        }
        var jsonAllowedValues = ReadJsonAllowedValues(
            name,
            element,
            scalarDomain,
            jsonAny,
            nullable,
            fixedValue,
            exactFloatMarkers);
        var numericRange = ReadNumericRange(
            name,
            element,
            scalarDomain,
            jsonAny,
            fixedValue,
            exactFloatMarkers);
        var jsonMultipleOf = ReadJsonMultipleOf(
            name,
            element,
            scalarDomain,
            jsonAny,
            fixedValue);
        var stringLengthRange = ReadStringLengthRange(
            name,
            element,
            scalarDomain,
            jsonAny,
            fixedValue);
        var jsonPatterns = ReadJsonPatterns(
            name,
            element,
            scalarDomain,
            jsonAny,
            fixedValue,
            patternContext);
        var itemCountRange = ReadItemCountRange(name, element, repeating);
        var jsonContains = ReadJsonContains(
            name,
            element,
            repeating,
            budget,
            patternContext,
            depth,
            exactFloatMarkers);
        var propertyCountRange = ReadPropertyCountRange(
            name,
            element,
            scalarDomain == JsonScalarDomain.None);
        var propertyDependencies = ReadJsonPropertyDependencies(
            name,
            element,
            scalarDomain == JsonScalarDomain.None);
        var dependentSchemas = ReadJsonDependentSchemas(
            name,
            element,
            scalarDomain == JsonScalarDomain.None,
            budget,
            patternContext,
            depth,
            exactFloatMarkers);
        var propertyNames = ReadJsonPropertyNames(
            name,
            element,
            scalarDomain == JsonScalarDomain.None,
            patternContext);
        var patternPropertyNames = ReadJsonPatternPropertyNames(
            name,
            element,
            scalarDomain == JsonScalarDomain.None,
            patternContext);
        var children = new List<JsonSchemaNode>();
        JsonSchemaNode? dynamic = null;
        var alternatives = new List<JsonAlternative>();
        var required = Array.Empty<string>();
        if (scalarDomain == JsonScalarDomain.None)
        {
            if (kindElement.TryGetProperty("children", out var childElements))
            {
                RequireKind(childElements, JsonValueKind.Array, $"schema node '{name}' children", "array");
                foreach (var child in childElements.EnumerateArray())
                {
                    children.Add(ReadSchemaNode(child, budget, patternContext, depth + 1, exactFloatMarkers));
                }
            }

            if (kindElement.TryGetProperty("dynamic", out var dynamicElement) &&
                dynamicElement.ValueKind != JsonValueKind.Null)
            {
                dynamic = ReadSchemaNode(dynamicElement, budget, patternContext, depth + 1, exactFloatMarkers);
            }

            if (kindElement.TryGetProperty("alternatives", out var alternativeElements))
            {
                RequireKind(
                    alternativeElements,
                    JsonValueKind.Array,
                    $"schema node '{name}' alternatives",
                    "array");
                foreach (var alternative in alternativeElements.EnumerateArray())
                {
                    alternatives.Add(ReadAlternative(alternative, exactFloatMarkers));
                }
            }
            if (kindElement.TryGetProperty("required", out _))
            {
                required = RequiredStrings(kindElement, "required");
                var seen = new HashSet<string>(StringComparer.Ordinal);
                foreach (var field in required)
                {
                    if (field.Length == 0 || !seen.Add(field))
                    {
                        throw Boundary(
                            $"Embedded JSON schema node '{name}' has empty or duplicate required fields.");
                    }
                    if (dynamic is null && !children.Any(child =>
                            string.Equals(child.Name, field, StringComparison.Ordinal)))
                    {
                        throw Boundary(
                            $"Embedded JSON schema node '{name}' requires undeclared field '{field}'.");
                    }
                }
            }
            if (dynamic is not null && alternatives.Count != 0)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' combines an open object with closed alternatives.");
            }
            ValidatePropertyCountSchema(
                name,
                propertyCountRange,
                children,
                dynamic,
                required,
                alternatives);
            ValidatePropertyDependencySchema(
                name,
                propertyDependencies,
                children,
                dynamic,
                required,
                alternatives,
                propertyCountRange);
            ValidatePropertyNameSchema(
                name,
                propertyNames,
                children,
                dynamic,
                propertyDependencies,
                required,
                alternatives,
                propertyCountRange);
            ValidatePatternPropertyNameSchema(
                name,
                patternPropertyNames,
                children,
                dynamic,
                propertyDependencies,
                required,
                alternatives,
                patternContext);
        }
        else if (kindElement.TryGetProperty("required", out _))
        {
            throw Boundary(
                $"Embedded JSON scalar schema node '{name}' cannot declare required object fields.");
        }

        var inclusiveAlternatives =
            element.TryGetProperty("alternative_mode", out var mode) &&
            mode.ValueKind == JsonValueKind.String &&
            string.Equals(mode.GetString(), "inclusive", StringComparison.Ordinal);
        var xmlTypeAlternatives = OptionalBoolean(element, "xml_type_alternatives");
        if (xmlTypeAlternatives &&
            (scalarDomain != JsonScalarDomain.None || alternatives.Count == 0 ||
             inclusiveAlternatives ||
             alternatives.Any(alternative =>
                 alternative.Name.Length == 0 || alternative.Required.Count != 0 ||
                 alternative.Constraints.Count != 0) ||
             (element.TryGetProperty("xml_alternative_kind", out var xmlKind) &&
              (xmlKind.ValueKind != JsonValueKind.String ||
               !string.Equals(xmlKind.GetString(), "xsi_type", StringComparison.Ordinal)))))
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has invalid XML type alternatives.");
        }
        var xmlDefaultType = element.TryGetProperty("xml_default_type", out var defaultType)
            && defaultType.ValueKind == JsonValueKind.Null
            ? null : OptionalString(element, "xml_default_type");
        if (xmlDefaultType is not null)
        {
            var declared = alternatives.Find(alternative => alternative.Name == xmlDefaultType);
            var restrictedTypes = new HashSet<string>(StringComparer.Ordinal);
            if (kindElement.TryGetProperty("xml_restricted_alternatives", out var restrictionNames))
            {
                if (restrictionNames.ValueKind != JsonValueKind.Array)
                {
                    throw Boundary($"Embedded JSON schema node '{name}' has invalid XML restriction metadata.");
                }
                foreach (var restriction in restrictionNames.EnumerateArray())
                {
                    if (restriction.ValueKind != JsonValueKind.String ||
                        restriction.GetString() is not { Length: > 0 } identity ||
                        !restrictedTypes.Add(identity) || !alternatives.Any(type => type.Name == identity))
                    {
                        throw Boundary($"Embedded JSON schema node '{name}' has invalid XML restriction metadata.");
                    }
                }
            }
            if (!xmlTypeAlternatives || declared is null ||
                OptionalBoolean(element, "attribute") || OptionalBoolean(element, "text") ||
                recursiveReference is not null || restrictedTypes.Contains(xmlDefaultType) ||
                alternatives.Any(alternative => restrictedTypes.Contains(alternative.Name)
                    ? alternative.Members.Any(member => !declared.Members.Contains(member))
                    : declared.Members.Any(member => !alternative.Members.Contains(member))))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has invalid XML default type metadata.");
            }
        }
        if (recursiveReference is not null &&
            (recursiveReference.Length == 0 || scalarDomain != JsonScalarDomain.None ||
             children.Count != 0 || dynamic is not null || alternatives.Count != 0 ||
             required.Length != 0 || jsonAny || nullable ||
             OptionalBoolean(element, "attribute") || OptionalBoolean(element, "text") ||
             fixedLexical is not null ||
             jsonAllowedValues is not null || numericRange is not null ||
             jsonMultipleOf is not null || stringLengthRange is not null ||
             jsonPatterns is not null || jsonFormats.Length != 0 ||
             propertyCountRange is not null || propertyDependencies.Count != 0 ||
             dependentSchemas.Count != 0 || propertyNames is not null ||
             patternPropertyNames is not null || xmlTypeAlternatives ||
             inclusiveAlternatives ||
             (OptionalString(element, "alternative_mode") is { } modeName &&
              !string.Equals(modeName, "exclusive", StringComparison.Ordinal)) ||
             (OptionalString(element, "xml_alternative_kind") is { } xmlMode &&
              !string.Equals(xmlMode, "xsi_type", StringComparison.Ordinal)) ||
             (kindElement.TryGetProperty("xml_restricted_alternatives", out var restricted) &&
              (restricted.ValueKind != JsonValueKind.Array || restricted.GetArrayLength() != 0))))
        {
            throw Boundary(
                $"Embedded JSON recursive reference '{name}' is not an empty group occurrence.");
        }

        return new JsonSchemaNode(
            name,
            repeating,
            nullable,
            OptionalBoolean(element, "container_nullable"),
            jsonAny,
            jsonFormats,
            scalarDomain,
            fixedLexical,
            fixedValue,
            jsonAllowedValues,
            numericRange,
            jsonMultipleOf,
            stringLengthRange,
            jsonPatterns,
            itemCountRange,
            jsonContains,
            propertyCountRange,
            propertyDependencies,
            dependentSchemas,
            propertyNames,
            patternPropertyNames,
            jsonUniqueItems,
            children,
            dynamic,
            required,
            alternatives,
            inclusiveAlternatives,
            xmlTypeAlternatives,
            recursiveReference);
    }

    private static void BindRecursiveReferences(JsonSchemaNode root)
    {
        var nodes = new List<JsonSchemaNode>();
        var pending = new Stack<JsonSchemaNode>();
        pending.Push(root);
        while (pending.Count != 0)
        {
            var node = pending.Pop();
            nodes.Add(node);
            foreach (var child in node.Children)
            {
                pending.Push(child);
            }
            if (node.Dynamic is { } dynamic)
            {
                pending.Push(dynamic);
            }
        }

        foreach (var occurrence in nodes)
        {
            if (occurrence.RecursiveReference is not { } reference)
            {
                continue;
            }
            var anchors = nodes.Where(node =>
                node.RecursiveReference is null && !node.IsScalar &&
                string.Equals(node.Name, reference, StringComparison.Ordinal)).ToArray();
            if (anchors.Length != 1)
            {
                throw Boundary(
                    $"Embedded JSON recursive reference '{occurrence.Name}' has " +
                    (anchors.Length == 0 ? $"no group anchor '{reference}'." :
                        $"ambiguous group anchor '{reference}'."));
            }
            occurrence.BindRecursiveAnchor(anchors[0]);
        }
    }

    private static string[] ReadJsonFormats(
        string name,
        JsonElement element,
        JsonScalarDomain scalarDomain,
        bool jsonAny)
    {
        if (!element.TryGetProperty("json_formats", out var formatsElement))
        {
            return Array.Empty<string>();
        }
        RequireKind(
            formatsElement,
            JsonValueKind.Array,
            $"schema node '{name}' JSON formats",
            "array");
        var formats = new List<string>();
        var seen = new HashSet<string>(StringComparer.Ordinal);
        var totalBytes = 0;
        foreach (var formatElement in formatsElement.EnumerateArray())
        {
            if (formatElement.ValueKind != JsonValueKind.String)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON formats must contain strings.");
            }
            if (formats.Count == MaximumJsonFormats)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has more than {MaximumJsonFormats} JSON format annotations.");
            }

            var format = formatElement.GetString() ?? string.Empty;
            var bytes = Encoding.UTF8.GetByteCount(format);
            if (bytes > MaximumJsonFormatBytes)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON format annotation is {bytes} UTF-8 bytes; maximum is {MaximumJsonFormatBytes}.");
            }
            totalBytes = checked(totalBytes + bytes);
            if (totalBytes > MaximumJsonFormatTotalBytes)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON format annotations total {totalBytes} UTF-8 bytes; maximum is {MaximumJsonFormatTotalBytes}.");
            }
            if (!seen.Add(format))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has duplicate JSON format annotation '{format}'.");
            }
            formats.Add(format);
        }
        if (formats.Count != 0 &&
            (!scalarDomain.HasFlag(JsonScalarDomain.String) || jsonAny))
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has JSON format annotations without a declared string domain or on json_any.");
        }
        return formats.ToArray();
    }

    private static JsonItemCountRange? ReadItemCountRange(
        string name,
        JsonElement element,
        bool repeating)
    {
        if (!element.TryGetProperty("item_count_range", out var rangeElement) ||
            rangeElement.ValueKind == JsonValueKind.Null)
        {
            return null;
        }
        RequireKind(
            rangeElement,
            JsonValueKind.Object,
            $"schema node '{name}' item-count range",
            "object");
        foreach (var property in rangeElement.EnumerateObject())
        {
            if (property.Name is not ("minimum" or "maximum"))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' item-count range has unknown field '{property.Name}'.");
            }
        }
        var minimum = OptionalUInt64(rangeElement, "minimum", false) ?? 0;
        var maximum = OptionalUInt64(rangeElement, "maximum", true);
        if (!repeating ||
            minimum == 0 && maximum is null ||
            maximum is { } upper && minimum > upper)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has invalid item-count metadata.");
        }
        return new JsonItemCountRange(minimum, maximum);
    }

    private static JsonStringLengthRange? ReadStringLengthRange(
        string name,
        JsonElement element,
        JsonScalarDomain scalarDomain,
        bool jsonAny,
        FerruleValue? fixedValue)
    {
        if (!element.TryGetProperty("string_length_range", out var rangeElement) ||
            rangeElement.ValueKind == JsonValueKind.Null)
        {
            return null;
        }
        RequireKind(
            rangeElement,
            JsonValueKind.Object,
            $"schema node '{name}' string-length range",
            "object");
        var fields = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in rangeElement.EnumerateObject())
        {
            if (property.Name is not ("minimum" or "maximum") ||
                !fields.Add(property.Name))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' string-length range has an unknown or duplicate field '{property.Name}'.");
            }
        }
        var minimum = OptionalUInt64(rangeElement, "minimum", false) ?? 0;
        var maximum = OptionalUInt64(rangeElement, "maximum", true);
        if (!scalarDomain.HasFlag(JsonScalarDomain.String) ||
            jsonAny ||
            minimum == 0 && maximum is null ||
            maximum is { } upper && minimum > upper)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has invalid string-length metadata.");
        }

        var range = new JsonStringLengthRange(minimum, maximum);
        if (fixedValue is { Kind: FerruleValueKind.String } fixedString &&
            !range.Contains(name, fixedString.StringValue))
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has a fixed string outside its string-length range.");
        }
        return range;
    }

    private static JsonPatternConstraints? ReadJsonPatterns(
        string name,
        JsonElement element,
        JsonScalarDomain scalarDomain,
        bool jsonAny,
        FerruleValue? fixedValue,
        JsonPatternSchemaContext context)
    {
        JsonElement? declaredPatterns = null;
        foreach (var property in element.EnumerateObject())
        {
            if (!string.Equals(property.Name, "json_patterns", StringComparison.Ordinal))
            {
                continue;
            }
            if (declaredPatterns is not null)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has duplicate JSON pattern metadata.");
            }
            declaredPatterns = property.Value;
        }
        if (declaredPatterns is not { } patternsElement ||
            patternsElement.ValueKind == JsonValueKind.Null)
        {
            return null;
        }
        RequireKind(
            patternsElement,
            JsonValueKind.Object,
            $"schema node '{name}' JSON patterns",
            "object");

        var sawAnyOf = false;
        foreach (var property in patternsElement.EnumerateObject())
        {
            if (!string.Equals(property.Name, "any_of", StringComparison.Ordinal) ||
                sawAnyOf)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON patterns have an unknown or duplicate field '{property.Name}'.");
            }
            sawAnyOf = true;
        }
        if (!sawAnyOf)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' JSON patterns are missing field 'any_of'.");
        }
        if (!scalarDomain.HasFlag(JsonScalarDomain.String) || jsonAny)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has JSON patterns without a declared string domain or on json_any.");
        }

        var alternativesElement = RequiredProperty(patternsElement, "any_of");
        RequireKind(
            alternativesElement,
            JsonValueKind.Array,
            $"schema node '{name}' JSON pattern alternatives",
            "array");
        var alternatives = new List<JsonPatternAlternative>();
        var canonicalSources = new List<string[]>();
        var distinctSources = new HashSet<string>(StringComparer.Ordinal);
        var sourceBytes = 0;
        var instructions = 0;
        var terms = 0;
        foreach (var alternativeElement in alternativesElement.EnumerateArray())
        {
            if (alternatives.Count == MaximumJsonPatternAlternatives)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has more than {MaximumJsonPatternAlternatives} JSON pattern alternatives.");
            }
            RequireKind(
                alternativeElement,
                JsonValueKind.Array,
                $"schema node '{name}' JSON pattern alternative",
                "array");

            var sources = new List<string>();
            var compiledTerms = new List<FerruleJsonPattern>();
            foreach (var termElement in alternativeElement.EnumerateArray())
            {
                terms = checked(terms + 1);
                if (terms > MaximumJsonPatternTerms)
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' has more than {MaximumJsonPatternTerms} JSON pattern terms.");
                }
                if (termElement.ValueKind != JsonValueKind.String)
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' JSON pattern terms must be strings.");
                }

                var source = termElement.GetString() ?? string.Empty;
                if (sources.Contains(source, StringComparer.Ordinal))
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' has a duplicate JSON pattern term.");
                }

                var compiled = context.GetOrCompile(name, source);
                if (distinctSources.Add(source))
                {
                    sourceBytes = checked(sourceBytes + StrictUtf8.GetByteCount(source));
                    instructions = checked(instructions + compiled.InstructionCount);
                    if (distinctSources.Count > MaximumDistinctJsonPatterns ||
                        sourceBytes > MaximumJsonPatternSourceBytes ||
                        instructions > MaximumJsonPatternInstructions)
                    {
                        throw Boundary(
                            $"Embedded JSON schema node '{name}' exceeds a local JSON pattern metadata limit.");
                    }
                }
                sources.Add(source);
                compiledTerms.Add(compiled);
            }
            if (sources.Count == 0)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has an empty JSON pattern alternative.");
            }
            if (sources.Count != 1 &&
                sources.Contains(string.Empty, StringComparer.Ordinal))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has a noncanonical tautological JSON pattern alternative.");
            }
            if (canonicalSources.Any(previous =>
                    previous.SequenceEqual(sources, StringComparer.Ordinal)))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has a duplicate JSON pattern alternative.");
            }

            canonicalSources.Add(sources.ToArray());
            alternatives.Add(new JsonPatternAlternative(sources.ToArray(), compiledTerms));
        }
        if (alternatives.Count == 0)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has empty JSON pattern constraints.");
        }
        if (canonicalSources.Count != 1 &&
            canonicalSources.Any(sources =>
                sources.Length == 1 && sources[0].Length == 0))
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has noncanonical tautological JSON pattern constraints.");
        }

        var constraints = new JsonPatternConstraints(alternatives);
        if (fixedValue is { Kind: FerruleValueKind.String } fixedString)
        {
            if (!context.FixedMatches(constraints, fixedString.StringValue))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has a fixed string outside its JSON pattern constraints.");
            }
        }
        return constraints;
    }

    private static JsonNumericRange? ReadNumericRange(
        string name,
        JsonElement element,
        JsonScalarDomain scalarDomain,
        bool jsonAny,
        FerruleValue? fixedValue,
        bool exactFloatMarkers)
    {
        if (!element.TryGetProperty("numeric_range", out var rangeElement) ||
            rangeElement.ValueKind == JsonValueKind.Null)
        {
            return null;
        }
        RequireKind(
            rangeElement,
            JsonValueKind.Object,
            $"schema node '{name}' numeric range",
            "object");
        if ((!IsSingleScalar(scalarDomain) && !IsStringIntUnion(scalarDomain)) || jsonAny)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has a numeric range without one concrete numeric scalar type.");
        }

        var kind = RequiredString(rangeElement, "kind");
        var bounds = RequiredProperty(rangeElement, "bounds");
        RequireKind(bounds, JsonValueKind.Object, $"schema node '{name}' numeric bounds", "object");
        JsonNumericRange range = kind switch
        {
            "integer" when scalarDomain == JsonScalarDomain.Int64 || IsStringIntUnion(scalarDomain) =>
                ReadIntegerRange(name, bounds),
            "number" when scalarDomain == JsonScalarDomain.Double =>
                ReadNumberRange(name, bounds, exactFloatMarkers),
            "integer" or "number" => throw Boundary(
                $"Embedded JSON schema node '{name}' numeric range does not match its scalar type."),
            _ => throw Boundary(
                $"Embedded JSON schema node '{name}' has unknown numeric range kind '{kind}'."),
        };
        if (fixedValue is { } constrained && !range.Contains(constrained))
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has a fixed value outside its numeric range.");
        }
        return range;
    }

    private static JsonIntegerRange ReadIntegerRange(string name, JsonElement bounds)
    {
        var minimum = OptionalInt64(bounds, "minimum");
        var maximum = OptionalInt64(bounds, "maximum");
        if (minimum is null && maximum is null ||
            minimum is { } lower && maximum is { } upper && lower > upper)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has an empty or unordered integer range.");
        }
        return new JsonIntegerRange(minimum, maximum);
    }

    private static JsonNumberRange ReadNumberRange(
        string name, JsonElement bounds, bool exactFloatMarkers)
    {
        var minimum = OptionalNumberBound(name, bounds, "minimum", exactFloatMarkers);
        var maximum = OptionalNumberBound(name, bounds, "maximum", exactFloatMarkers);
        if (minimum is null && maximum is null)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has an empty number range declaration.");
        }
        var first = minimum switch
        {
            { Exclusive: true, Value: double.MaxValue } => double.PositiveInfinity,
            { Exclusive: true } bound => double.BitIncrement(bound.Value),
            { } bound => bound.Value,
            null => -double.MaxValue,
        };
        var last = maximum switch
        {
            { Exclusive: true, Value: -double.MaxValue } => double.NegativeInfinity,
            { Exclusive: true } bound => double.BitDecrement(bound.Value),
            { } bound => bound.Value,
            null => double.MaxValue,
        };
        if (!double.IsFinite(first) || !double.IsFinite(last) || first > last)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has an empty number range.");
        }
        return new JsonNumberRange(minimum, maximum);
    }

    private static JsonNumberBound? OptionalNumberBound(
        string name,
        JsonElement bounds,
        string property,
        bool exactFloatMarkers)
    {
        if (!bounds.TryGetProperty(property, out var element))
        {
            return null;
        }
        RequireKind(
            element,
            JsonValueKind.Object,
            $"schema node '{name}' number {property}",
            "object");
        var valueElement = RequiredProperty(element, "value");
        if (!TryReadFiniteMetadataDouble(valueElement, exactFloatMarkers, out var value))
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' number {property} must be finite.");
        }
        return new JsonNumberBound(value, OptionalBoolean(element, "exclusive"));
    }

    private static JsonScalarDomain ReadScalarDomain(
        string nodeName,
        JsonElement kindElement)
    {
        var typeElements = RequiredProperty(kindElement, "types");
        RequireKind(
            typeElements,
            JsonValueKind.Array,
            $"schema node '{nodeName}' scalar union types",
            "array");
        var domain = JsonScalarDomain.None;
        var previousOrder = -1;
        var count = 0;
        foreach (var typeElement in typeElements.EnumerateArray())
        {
            if (typeElement.ValueKind != JsonValueKind.String)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{nodeName}' scalar union types must contain strings.");
            }

            var typeName = typeElement.GetString() ?? string.Empty;
            var scalar = ScalarDomain(nodeName, typeName);
            var order = ScalarOrder(scalar);
            if (domain.HasFlag(scalar))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{nodeName}' scalar union contains duplicate type '{typeName}'.");
            }
            if (order <= previousOrder)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{nodeName}' scalar union types are not in canonical order.");
            }

            domain |= scalar;
            previousOrder = order;
            count = checked(count + 1);
        }

        if (count < 2)
        {
            throw Boundary(
                $"Embedded JSON schema node '{nodeName}' scalar union must contain at least two distinct types.");
        }

        return domain;
    }

    private static JsonScalarDomain ScalarDomain(string nodeName, string typeName) =>
        typeName switch
        {
            "string" => JsonScalarDomain.String,
            "int" => JsonScalarDomain.Int64,
            "float" => JsonScalarDomain.Double,
            "bool" => JsonScalarDomain.Bool,
            _ => throw Boundary(
                $"Embedded JSON schema node '{nodeName}' has unknown scalar type '{typeName}'."),
        };

    private static int ScalarOrder(JsonScalarDomain scalar) => scalar switch
    {
        JsonScalarDomain.String => 0,
        JsonScalarDomain.Int64 => 1,
        JsonScalarDomain.Double => 2,
        JsonScalarDomain.Bool => 3,
        _ => throw Boundary("Embedded JSON schema scalar domain is invalid."),
    };

    private static JsonAlternative ReadAlternative(JsonElement element, bool exactFloatMarkers)
    {
        RequireKind(element, JsonValueKind.Object, "schema alternative", "object");
        var name = element.TryGetProperty("name", out _)
            ? RequiredString(element, "name")
            : string.Empty;
        var members = RequiredStrings(element, "members");
        var required = element.TryGetProperty("required", out _)
            ? RequiredStrings(element, "required")
            : Array.Empty<string>();
        var constraints = new List<JsonConstraint>();
        if (element.TryGetProperty("constraints", out var constraintElements))
        {
            RequireKind(constraintElements, JsonValueKind.Array, "schema constraints", "array");
            foreach (var constraint in constraintElements.EnumerateArray())
            {
                var member = RequiredString(constraint, "member");
                var value = RequiredProperty(constraint, "value");
                var type = RequiredString(value, "type");
                var expected = value.TryGetProperty("value", out var expectedValue)
                    ? expectedValue.Clone()
                    : default;
                double? expectedFloat = null;
                if (type == "float" &&
                    TryReadFiniteMetadataDouble(expected, exactFloatMarkers, out var number))
                {
                    expectedFloat = number;
                }
                var validExpected = type switch
                {
                    "string" => HasValidJsonStringScalar(expected),
                    "int" => TryGetSerdeInt64(expected, out _),
                    "float" => expectedFloat.HasValue,
                    "bool" => expected.ValueKind is JsonValueKind.True or JsonValueKind.False,
                    "json_null" => expected.ValueKind is JsonValueKind.Undefined or JsonValueKind.Null,
                    _ => false,
                };
                if (!validExpected)
                {
                    throw Boundary(
                        $"Embedded JSON alternative has an invalid '{type}' constraint value.");
                }
                constraints.Add(new JsonConstraint(member, type, expected, expectedFloat));
            }
        }

        return new JsonAlternative(name, members, required, constraints);
    }

    private static bool HasValidJsonStringScalar(JsonElement element)
    {
        if (element.ValueKind != JsonValueKind.String ||
            !FerruleUnicode.IsWellFormed(element.GetString() ?? string.Empty))
        {
            return false;
        }

        // JsonDocument can replace an escaped lone surrogate while decoding a
        // string. Rust rejects that schema text, so inspect the original JSON
        // escape sequence before accepting a discriminator.
        var source = element.GetRawText();
        for (var index = 1; index < source.Length - 1;)
        {
            if (source[index] != '\\')
            {
                index++;
                continue;
            }
            if (source[index + 1] != 'u')
            {
                index += 2;
                continue;
            }
            if (!ushort.TryParse(
                    source.AsSpan(index + 2, 4),
                    NumberStyles.HexNumber,
                    CultureInfo.InvariantCulture,
                    out var first))
            {
                return false;
            }
            index += 6;
            if (char.IsLowSurrogate((char)first))
            {
                return false;
            }
            if (!char.IsHighSurrogate((char)first))
            {
                continue;
            }
            if (index + 6 > source.Length - 1 ||
                source[index] != '\\' || source[index + 1] != 'u' ||
                !ushort.TryParse(
                    source.AsSpan(index + 2, 4),
                    NumberStyles.HexNumber,
                    CultureInfo.InvariantCulture,
                    out var second) ||
                !char.IsLowSurrogate((char)second))
            {
                return false;
            }
            index += 6;
        }
        return true;
    }

    private static FerruleInstance ReadNode(
        JsonSchemaNode schema,
        JsonElement element,
        NodeBudget budget,
        int depth)
    {
        budget.Visit(depth);
        if (schema.ContainerNullable && element.ValueKind == JsonValueKind.Null)
        {
            return new FerruleScalar(FerruleValue.JsonNull);
        }

        if (schema.Repeating)
        {
            RequireKind(element, JsonValueKind.Array, schema.Name, "array");
            ValidateItemCount(schema, element.GetArrayLength());
            ValidateUniqueInputItems(schema, element, budget);
            var items = new List<FerruleInstance>();
            foreach (var item in element.EnumerateArray())
            {
                items.Add(ReadSingleNode(schema, item, budget, depth + 1));
            }
            ValidateContainsInputItems(schema, element, budget, depth + 1);

            return new FerruleRepeated(items);
        }

        return ReadSingleNode(schema, element, budget, depth);
    }

    private static FerruleInstance ReadSingleNode(
        JsonSchemaNode schema,
        JsonElement element,
        NodeBudget budget,
        int depth)
    {
        if (schema.RecursiveReference is not null)
        {
            var resolved = schema.ResolvedRecursiveSchema ??
                throw Boundary(
                    $"Embedded JSON recursive reference '{schema.Name}' has no group anchor.");
            budget.EnterRecursiveReference();
            try
            {
                return ReadSingleNode(resolved, element, budget, depth);
            }
            finally
            {
                budget.ExitRecursiveReference();
            }
        }
        if (schema.JsonAny)
        {
            return new FerruleScalar(
                FerruleValue.FromString(budget.OutputNormalizedNumbers
                    ? CanonicalizeOutputAnyJson(element)
                    : CanonicalizeAnyJson(element)));
        }

        if (schema.ContainerNullable && element.ValueKind == JsonValueKind.Null)
        {
            return new FerruleScalar(FerruleValue.JsonNull);
        }

        if (schema.IsScalar)
        {
            return new FerruleScalar(ReadScalar(schema, element, budget));
        }

        RequireKind(element, JsonValueKind.Object, schema.Name, "object");
        var properties = OrderedProperties(element);
        ValidatePropertyCount(schema, properties.Count);
        ValidatePropertyNames(schema, properties, budget);
        ValidateRequired(schema, properties);
        ValidateDeclaredProperties(schema, properties);
        ValidatePatternProperties(schema, properties, budget);
        ValidatePropertyDependencies(schema, properties);
        ValidateAlternatives(schema, properties, budget);
        var fields = new List<FerruleField>();
        if (schema.Dynamic is { } dynamic)
        {
            foreach (var property in properties)
            {
                var child = schema.Child(property.Name) ?? dynamic;
                fields.Add(
                    new FerruleField(
                        property.Name,
                        ReadNode(child, property.Value, budget, depth + 1)));
            }

            foreach (var child in schema.Children)
            {
                if (!properties.Any(property =>
                        string.Equals(property.Name, child.Name, StringComparison.Ordinal)))
                {
                    fields.Add(new FerruleField(child.Name, Missing(child)));
                }
            }
        }
        else
        {
            foreach (var child in schema.Children)
            {
                var property = properties.Find(candidate =>
                    string.Equals(candidate.Name, child.Name, StringComparison.Ordinal));
                fields.Add(
                    new FerruleField(
                        child.Name,
                        property is null
                            ? Missing(child)
                            : ReadNode(child, property.Value, budget, depth + 1)));
            }
        }

        ValidateDependentSchemas(
            schema,
            properties,
            element,
            budget,
            depth);
        return FerruleGroup.FromSchemaFields(fields);
    }

    private static FerruleValue ReadScalar(
        JsonSchemaNode schema,
        JsonElement element,
        NodeBudget budget)
    {
        FerruleValue value;
        if (element.ValueKind == JsonValueKind.Null && schema.Nullable)
        {
            value = FerruleValue.JsonNull;
        }
        else
        {
            var domain = schema.ScalarDomain;
            if (element.ValueKind == JsonValueKind.String &&
                domain.HasFlag(JsonScalarDomain.String))
            {
                value = FerruleValue.FromString(element.GetString() ?? string.Empty);
            }
            else if (element.ValueKind == JsonValueKind.Number &&
                     domain.HasFlag(JsonScalarDomain.Int64) &&
                     TryGetSerdeInt64(element, out var integer))
            {
                value = FerruleValue.FromInt64(integer);
            }
            else if (element.ValueKind == JsonValueKind.Number &&
                     domain.HasFlag(JsonScalarDomain.Double))
            {
                value = ReadDouble(schema.Name, element, budget.OutputNormalizedNumbers);
            }
            else if (element.ValueKind is JsonValueKind.True or JsonValueKind.False &&
                     domain.HasFlag(JsonScalarDomain.Bool))
            {
                value = FerruleValue.FromBoolean(element.GetBoolean());
            }
            else
            {
                throw Shape(
                    schema.Name,
                    ScalarName(domain),
                    element.ValueKind.ToString());
            }
        }
        if (schema.Fixed is { } expected && value != expected)
        {
            throw Boundary(
                $"JSON scalar '{schema.Name}' requires constant {FixedDisplay(expected)}, got {element.GetRawText()}.");
        }
        if (value.Kind != FerruleValueKind.JsonNull &&
            schema.NumericRange is { } range &&
            !NumericRangeContains(schema, range, value))
        {
            throw Boundary(
                $"JSON scalar '{schema.Name}' is outside its numeric range: {element.GetRawText()}.");
        }
        ValidateJsonMultipleOf(schema, value);
        ValidateJsonAllowedValues(schema, value);
        ValidateStringLength(schema, value);
        ValidateJsonPatterns(schema, value, budget);
        return value;
    }

    private static FerruleValue ReadDouble(
        string name,
        JsonElement element,
        bool outputNormalizedNumbers)
    {
        if (TryGetSerdeInt64(element, out var integer))
        {
            if (TryExactDouble(integer, out var converted))
            {
                return FerruleValue.FromDouble(converted);
            }

            throw Shape(name, "number", "integer outside the exact double range");
        }
        if (TryGetSerdeUInt64(element, out var unsignedInteger))
        {
            if (TryExactDouble(unsignedInteger, out var converted))
            {
                return FerruleValue.FromDouble(converted);
            }

            throw Shape(name, "number", "integer outside the exact double range");
        }

        if (outputNormalizedNumbers)
        {
            if (element.TryGetDouble(out var normalized) && double.IsFinite(normalized))
            {
                return FerruleValue.FromDouble(normalized);
            }
            throw Shape(name, "finite number", "non-finite number");
        }

        if (!TryParseSerdeNumber(element.GetRawText(), out var parsed) ||
            parsed.Kind != SerdeNumberKind.Float)
        {
            throw Shape(name, "finite number", "non-finite number");
        }

        return FerruleValue.FromDouble(parsed.FloatValue);
    }

    private static FerruleInstance Missing(JsonSchemaNode schema)
    {
        if (schema.ContainerNullable)
        {
            return new FerruleScalar(FerruleValue.Null);
        }

        if (schema.Repeating)
        {
            return new FerruleRepeated(Array.Empty<FerruleInstance>());
        }

        return schema.IsScalar
            ? new FerruleScalar(FerruleValue.Null)
            : new FerruleGroup(Array.Empty<FerruleField>());
    }

    private static void WriteNode(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        FerruleInstance instance,
        NodeBudget budget,
        int depth)
    {
        budget.Visit(depth);
        if (schema.ContainerNullable &&
            instance is FerruleScalar { Value.Kind: FerruleValueKind.JsonNull })
        {
            writer.WriteNullValue();
            return;
        }

        if (schema.Repeating)
        {
            var items = instance switch
            {
                FerruleRepeated repeated => repeated.Items,
                FerruleMappedSequence mappedArray => mappedArray.Items,
                _ => throw Shape(schema.Name, "array", InstanceKind(instance)),
            };
            ValidateItemCount(schema, items.Count);

            writer.WriteStartArray();
            if (schema.JsonUniqueItems || schema.JsonContains.Count != 0)
            {
                WriteConstrainedOutputItems(
                    writer,
                    schema,
                    items,
                    budget,
                    depth + 1);
            }
            else
            {
                foreach (var item in items)
                {
                    WriteSingleNode(writer, schema, item, budget, depth + 1);
                }
            }

            writer.WriteEndArray();
            return;
        }

        WriteSingleNode(writer, schema, instance, budget, depth);
    }

    private static void ValidateItemCount(JsonSchemaNode schema, int count)
    {
        if (schema.ItemCountRange is { } range && !range.Contains(count))
        {
            throw Boundary(
                $"JSON array '{schema.Name}' has {count} items outside its declared item-count range.");
        }
    }

    private static void WriteSingleNode(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        FerruleInstance instance,
        NodeBudget budget,
        int depth)
    {
        if (schema.RecursiveReference is not null)
        {
            var resolved = schema.ResolvedRecursiveSchema ??
                throw Boundary(
                    $"Embedded JSON recursive reference '{schema.Name}' has no group anchor.");
            budget.EnterRecursiveReference();
            try
            {
                WriteSingleNode(writer, resolved, instance, budget, depth);
            }
            finally
            {
                budget.ExitRecursiveReference();
            }
            return;
        }
        if (instance is FerruleMappedSequence mapped)
        {
            if (mapped.Items.Count != 1)
            {
                throw Shape(schema.Name, "one mapped item", "mapped sequence");
            }

            instance = mapped.Items[0];
        }

        if (schema.JsonAny)
        {
            WriteAny(writer, schema, instance);
            return;
        }

        if (schema.ContainerNullable &&
            instance is FerruleScalar { Value.Kind: FerruleValueKind.JsonNull })
        {
            writer.WriteNullValue();
            return;
        }

        if (schema.IsScalar)
        {
            if (instance is not FerruleScalar value)
            {
                throw Shape(
                    schema.Name,
                    ScalarName(schema.ScalarDomain),
                    InstanceKind(instance));
            }

            if (schema.IsScalarUnion)
            {
                WriteScalarUnion(writer, schema, value.Value, budget);
            }
            else
            {
                WriteScalar(
                    writer,
                    schema,
                    SingleScalar(schema.ScalarDomain),
                    value.Value,
                    budget);
            }
            return;
        }

        if (instance is not FerruleGroup group)
        {
            throw Shape(schema.Name, "object", InstanceKind(instance));
        }

        if (schema.Dynamic is null && schema.DuplicateChildren)
        {
            WriteDuplicateDeclarationObject(writer, schema, group, budget, depth);
            return;
        }

        ValidateOutputRequired(schema, group);
        ValidateOutputPropertyNames(schema, group, budget);
        ValidateOutputPatternProperties(schema, group, budget);
        ValidateOutputPropertyDependencies(schema, group);
        ValidateOutputAlternatives(schema, group);
        ValidateOutputPropertyCount(schema, group);
        if (schema.DependentSchemas.Count != 0)
        {
            WriteDependentObject(writer, schema, group, budget, depth);
            return;
        }
        WriteObject(writer, schema, group, budget, depth);
    }

    private static void WriteObject(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        FerruleGroup group,
        NodeBudget budget,
        int depth)
    {
        writer.WriteStartObject();
        if (schema.Dynamic is { } dynamic)
        {
            var written = new HashSet<string>(StringComparer.Ordinal);
            foreach (var field in group.Fields)
            {
                if (written.Contains(field.Name))
                {
                    throw Boundary($"JSON object '{schema.Name}' has duplicate property '{field.Name}'.");
                }
                var child = schema.Child(field.Name) ?? dynamic;
                if (BoundaryAbsence(child, field.Value))
                {
                    continue;
                }

                written.Add(field.Name);
                writer.WritePropertyName(field.Name);
                WriteNode(writer, child, field.Value, budget, depth + 1);
            }
        }
        else
        {
            foreach (var child in schema.Children)
            {
                if (!group.TryGetField(child.Name, out var value) ||
                    BoundaryAbsence(child, value))
                {
                    continue;
                }

                writer.WritePropertyName(child.Name);
                WriteNode(writer, child, value, budget, depth + 1);
            }
        }

        writer.WriteEndObject();
    }

    // Preserve every declaration's validation and work, then project the last
    // assignment while retaining the first property position, as a native JSON map does.
    private static void WriteDuplicateDeclarationObject(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        FerruleGroup group,
        NodeBudget budget,
        int depth)
    {
        var names = new List<string>();
        var last = new Dictionary<string, int>(StringComparer.Ordinal);
        for (var index = 0; index < schema.Children.Count; index++)
        {
            var child = schema.Children[index];
            if (!group.TryGetField(child.Name, out var value) || BoundaryAbsence(child, value))
            {
                continue;
            }
            if (last.TryAdd(child.Name, index))
            {
                names.Add(child.Name);
            }
            else
            {
                last[child.Name] = index;
            }
        }

        var values = new Dictionary<string, ReadOnlyMemory<byte>>(StringComparer.Ordinal);
        long retainedBytes = 0;
        for (var index = 0; index < schema.Children.Count; index++)
        {
            var child = schema.Children[index];
            if (!group.TryGetField(child.Name, out var value) || BoundaryAbsence(child, value))
            {
                continue;
            }
            var encoded = new BoundedJsonBufferWriter(
                MaximumEscapedDocumentBytes,
                $"Normalized JSON property '{child.Name}'");
            using (var childWriter = new Utf8JsonWriter(
                       encoded,
                       new JsonWriterOptions
                       {
                           Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
                           MaxDepth = MaximumDepth,
                       }))
            {
                WriteNode(childWriter, child, value, budget, depth + 1);
            }
            // Earlier assignments still validate, but are never retained. This
            // prevents repeated aliases from multiplying the projected buffer.
            if (last[child.Name] == index)
            {
                retainedBytes = checked(retainedBytes + encoded.WrittenMemory.Length);
                if (retainedBytes > MaximumEscapedDocumentBytes)
                {
                    throw Boundary($"Normalized JSON object '{schema.Name}' exceeds the bounded output buffer.");
                }
                values.Add(child.Name, encoded.WrittenMemory);
            }
        }

        var buffer = new BoundedJsonBufferWriter(
            MaximumEscapedDocumentBytes,
            $"Normalized JSON object '{schema.Name}'");
        using (var objectWriter = new Utf8JsonWriter(
                   buffer,
                   new JsonWriterOptions
                   {
                       Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
                       MaxDepth = MaximumDepth,
                   }))
        {
            objectWriter.WriteStartObject();
            foreach (var name in names)
            {
                objectWriter.WritePropertyName(name);
                objectWriter.WriteRawValue(values[name].Span, skipInputValidation: true);
            }
            objectWriter.WriteEndObject();
        }
        if (CanonicalOutputUtf8ByteCount(buffer.WrittenSpan) > MaximumDocumentBytes)
        {
            throw Boundary($"Normalized JSON object '{schema.Name}' exceeds the {MaximumDocumentBytes}-byte limit.");
        }
        using var document = JsonDocument.Parse(
            buffer.WrittenMemory,
            new JsonDocumentOptions { MaxDepth = MaximumDepth });
        var properties = OrderedProperties(document.RootElement);
        var matcher = budget.OutputMatcher();
        ValidatePropertyNames(schema, properties, matcher);
        ValidatePatternProperties(schema, properties, matcher);
        ValidateRequired(schema, properties);
        ValidatePropertyDependencies(schema, properties);
        ValidateAlternatives(schema, properties, matcher);
        ValidatePropertyCount(schema, properties.Count);
        ValidateDependentSchemas(schema, properties, document.RootElement, matcher, depth);
        document.RootElement.WriteTo(writer);
    }

    private static void WriteAny(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        FerruleInstance instance)
    {
        if (instance is not FerruleScalar scalar)
        {
            throw Shape(schema.Name, "arbitrary JSON scalar encoding", InstanceKind(instance));
        }

        switch (scalar.Value.Kind)
        {
            case FerruleValueKind.String:
                using (var document = ParseAnyJson(scalar.Value.StringValue))
                {
                    if (document is null ||
                        !TryCanonicalizeAnyJson(document.RootElement, out var canonical))
                    {
                        writer.WriteStringValue(scalar.Value.StringValue);
                    }
                    else
                    {
                        using var normalized = JsonDocument.Parse(
                            canonical,
                            new JsonDocumentOptions { MaxDepth = MaximumParsedDepth });
                        normalized.RootElement.WriteTo(writer);
                    }
                }

                break;
            case FerruleValueKind.Bool:
                writer.WriteBooleanValue(scalar.Value.BooleanValue);
                break;
            case FerruleValueKind.Int64:
                writer.WriteNumberValue(scalar.Value.Int64Value);
                break;
            case FerruleValueKind.Double when double.IsFinite(scalar.Value.DoubleValue):
                WriteFiniteDouble(writer, scalar.Value.DoubleValue);
                break;
            case FerruleValueKind.JsonNull:
                writer.WriteNullValue();
                break;
            default:
                throw Shape(
                    schema.Name,
                    "arbitrary JSON value",
                    scalar.Value.Kind.ToString());
        }
    }

    private static void WriteScalar(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        JsonScalarType scalar,
        FerruleValue value,
        NodeBudget budget)
    {
        ValidateFixedOutput(schema, scalar, value);
        ValidateNumericRangeOutput(schema, scalar, value);
        ValidateJsonMultipleOfOutput(schema, scalar, value);
        ValidateJsonAllowedValuesOutput(schema, scalar, value);
        if (value.Kind == FerruleValueKind.JsonNull && schema.Nullable)
        {
            writer.WriteNullValue();
            return;
        }
        if (scalar == JsonScalarType.String)
        {
            if (!TryOutputString(value, out var outputString))
            {
                throw Shape(schema.Name, "string", value.Kind.ToString());
            }
            ValidateStringLength(schema, outputString);
            ValidateJsonPatterns(schema, outputString, budget);
            writer.WriteStringValue(outputString);
            return;
        }

        switch (scalar, value.Kind)
        {
            case (JsonScalarType.Int64, FerruleValueKind.Int64):
                writer.WriteNumberValue(value.Int64Value);
                return;
            case (JsonScalarType.Int64, FerruleValueKind.String)
                when TryOutputInt64(value, out var integer):
                writer.WriteNumberValue(integer);
                return;
            case (JsonScalarType.Double, FerruleValueKind.Int64)
                when TryExactDouble(value.Int64Value, out _):
                writer.WriteNumberValue(value.Int64Value);
                return;
            case (JsonScalarType.Double, FerruleValueKind.Double)
                when double.IsFinite(value.DoubleValue):
                WriteFiniteDouble(writer, value.DoubleValue);
                return;
            case (JsonScalarType.Double, FerruleValueKind.String)
                when double.TryParse(
                         value.StringValue.Trim(),
                         NumberStyles.Float,
                         CultureInfo.InvariantCulture,
                         out var number) &&
                     double.IsFinite(number):
                WriteFiniteDouble(writer, number);
                return;
            case (JsonScalarType.Bool, FerruleValueKind.Bool):
                writer.WriteBooleanValue(value.BooleanValue);
                return;
            case (JsonScalarType.Bool, FerruleValueKind.String)
                when string.Equals(value.StringValue.Trim(), "true", StringComparison.Ordinal):
                writer.WriteBooleanValue(true);
                return;
            case (JsonScalarType.Bool, FerruleValueKind.String)
                when string.Equals(value.StringValue.Trim(), "false", StringComparison.Ordinal):
                writer.WriteBooleanValue(false);
                return;
            default:
                throw Shape(schema.Name, ScalarName(scalar), value.Kind.ToString());
        }
    }

    private static void ValidateNumericRangeOutput(
        JsonSchemaNode schema,
        JsonScalarType scalar,
        FerruleValue value)
    {
        if (schema.NumericRange is not { } range ||
            value.Kind == FerruleValueKind.JsonNull && schema.Nullable)
        {
            return;
        }
        FerruleValue normalized;
        if (scalar == JsonScalarType.Int64 && TryOutputInt64(value, out var integer))
        {
            normalized = FerruleValue.FromInt64(integer);
        }
        else if (scalar == JsonScalarType.Double && TryOutputDouble(value, out var number))
        {
            normalized = FerruleValue.FromDouble(number);
        }
        else
        {
            return;
        }
        if (!range.Contains(normalized))
        {
            throw Boundary(
                $"JSON scalar '{schema.Name}' is outside its numeric range: {value}.");
        }
    }

    private static FerruleValue ParseFixed(
        string name,
        JsonScalarType scalar,
        string lexical)
    {
        switch (scalar)
        {
            case JsonScalarType.String:
                return FerruleValue.FromString(lexical);
            case JsonScalarType.Int64 when long.TryParse(
                lexical,
                NumberStyles.AllowLeadingSign,
                CultureInfo.InvariantCulture,
                out var integer):
                return FerruleValue.FromInt64(integer);
            case JsonScalarType.Double when double.TryParse(
                lexical,
                NumberStyles.Float,
                CultureInfo.InvariantCulture,
                out var number) && double.IsFinite(number):
                return FerruleValue.FromDouble(number);
            case JsonScalarType.Bool when string.Equals(
                lexical,
                "true",
                StringComparison.Ordinal):
                return FerruleValue.FromBoolean(true);
            case JsonScalarType.Bool when string.Equals(
                lexical,
                "false",
                StringComparison.Ordinal):
                return FerruleValue.FromBoolean(false);
            default:
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has an invalid {ScalarName(scalar)} fixed value.");
        }
    }

    private static void ValidateFixedOutput(
        JsonSchemaNode schema,
        JsonScalarType scalar,
        FerruleValue value)
    {
        if (schema.Fixed is not { } expected)
        {
            return;
        }
        var matches = scalar switch
        {
            JsonScalarType.String =>
                TryOutputString(value, out var actualString) &&
                expected.Kind == FerruleValueKind.String &&
                string.Equals(actualString, expected.StringValue, StringComparison.Ordinal),
            JsonScalarType.Int64 =>
                TryOutputInt64(value, out var actualInteger) &&
                expected.Kind == FerruleValueKind.Int64 &&
                actualInteger == expected.Int64Value,
            JsonScalarType.Double =>
                TryOutputDouble(value, out var actualNumber) &&
                expected.Kind == FerruleValueKind.Double &&
                actualNumber == expected.DoubleValue,
            JsonScalarType.Bool =>
                TryOutputBoolean(value, out var actualBoolean) &&
                expected.Kind == FerruleValueKind.Bool &&
                actualBoolean == expected.BooleanValue,
            _ => false,
        };
        if (!matches)
        {
            throw Boundary(
                $"JSON scalar '{schema.Name}' requires constant {FixedDisplay(expected)}, got {value}.");
        }
    }

    private static string FixedDisplay(FerruleValue value) =>
        value.Kind == FerruleValueKind.String
            ? JsonSerializer.Serialize(value.StringValue, CanonicalJsonOptions)
            : value.ToString();

    private static void WriteScalarUnion(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        FerruleValue value,
        NodeBudget budget)
    {
        if (value.Kind == FerruleValueKind.Int64 &&
            !schema.ScalarDomain.HasFlag(JsonScalarDomain.Int64) &&
            schema.ScalarDomain.HasFlag(JsonScalarDomain.Double) &&
            TryExactDouble(value.Int64Value, out var converted))
        {
            ValidateJsonMultipleOf(schema, FerruleValue.FromDouble(converted));
            ValidateJsonAllowedValues(schema, FerruleValue.FromDouble(converted));
            writer.WriteNumberValue(value.Int64Value);
            return;
        }

        var normalized = NormalizeScalarUnion(schema, value);
        ValidateJsonAllowedValues(schema, normalized);
        if (normalized.Kind == FerruleValueKind.JsonNull)
        {
            writer.WriteNullValue();
            return;
        }
        if (schema.NumericRange is { } range &&
            !NumericRangeContains(schema, range, normalized))
        {
            throw Boundary(
                $"JSON scalar '{schema.Name}' is outside its numeric range: {normalized}.");
        }
        ValidateJsonMultipleOf(schema, normalized);
        ValidateStringLength(schema, normalized);
        ValidateJsonPatterns(schema, normalized, budget);

        WriteAdmittedScalar(writer, schema, normalized);
    }

    private static void ValidateStringLength(
        JsonSchemaNode schema,
        FerruleValue value)
    {
        if (value.Kind == FerruleValueKind.String)
        {
            ValidateStringLength(schema, value.StringValue);
        }
    }

    private static void ValidateStringLength(
        JsonSchemaNode schema,
        string value)
    {
        if (schema.StringLengthRange is { } range &&
            !range.Contains(schema.Name, value))
        {
            throw Boundary(
                $"JSON string '{schema.Name}' is outside its declared string-length range.");
        }
    }

    private static void ValidateJsonPatterns(
        JsonSchemaNode schema,
        FerruleValue value,
        NodeBudget budget)
    {
        if (value.Kind == FerruleValueKind.String)
        {
            ValidateJsonPatterns(schema, value.StringValue, budget);
        }
    }

    private static void ValidateJsonPatterns(
        JsonSchemaNode schema,
        string value,
        NodeBudget budget)
    {
        if (schema.JsonPatterns is { } patterns &&
            !budget.Matches(patterns, value))
        {
            throw Boundary(
                $"JSON string '{schema.Name}' does not match its declared JSON patterns.");
        }
    }

    private static FerruleValue NormalizeScalarUnion(
        JsonSchemaNode schema,
        FerruleValue value)
    {
        var domain = schema.ScalarDomain;
        if (value.Kind == FerruleValueKind.JsonNull && schema.Nullable ||
            ValueDomain(value.Kind) is { } valueDomain && domain.HasFlag(valueDomain))
        {
            return value;
        }
        if (value.Kind == FerruleValueKind.Int64 &&
            domain.HasFlag(JsonScalarDomain.Double))
        {
            if (TryExactDouble(value.Int64Value, out var converted))
            {
                return FerruleValue.FromDouble(converted);
            }

            throw Shape(
                schema.Name,
                "number",
                "int outside the exact double range");
        }

        if (value.Kind == FerruleValueKind.String &&
            !domain.HasFlag(JsonScalarDomain.String))
        {
            var converted = FerruleValue.Null;
            var conversions = 0;
            var text = value.StringValue.Trim();
            if (domain.HasFlag(JsonScalarDomain.Int64) &&
                long.TryParse(
                    text,
                    NumberStyles.AllowLeadingSign,
                    CultureInfo.InvariantCulture,
                    out var integer))
            {
                converted = FerruleValue.FromInt64(integer);
                conversions++;
            }
            if (domain.HasFlag(JsonScalarDomain.Double) &&
                double.TryParse(
                    text,
                    NumberStyles.Float,
                    CultureInfo.InvariantCulture,
                    out var number) &&
                double.IsFinite(number))
            {
                converted = FerruleValue.FromDouble(number);
                conversions++;
            }
            if (domain.HasFlag(JsonScalarDomain.Bool) &&
                string.Equals(text, "true", StringComparison.Ordinal))
            {
                converted = FerruleValue.FromBoolean(true);
                conversions++;
            }
            else if (domain.HasFlag(JsonScalarDomain.Bool) &&
                     string.Equals(text, "false", StringComparison.Ordinal))
            {
                converted = FerruleValue.FromBoolean(false);
                conversions++;
            }

            if (conversions == 1)
            {
                return converted;
            }
            if (conversions > 1)
            {
                throw Shape(
                    schema.Name,
                    "unambiguous declared scalar union",
                    "String");
            }
        }

        throw Shape(
            schema.Name,
            "declared scalar union",
            value.Kind.ToString());
    }

    private static void WriteAdmittedScalar(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        FerruleValue value)
    {
        switch (value.Kind)
        {
            case FerruleValueKind.String:
                writer.WriteStringValue(value.StringValue);
                return;
            case FerruleValueKind.Int64:
                writer.WriteNumberValue(value.Int64Value);
                return;
            case FerruleValueKind.Double when double.IsFinite(value.DoubleValue):
                WriteFiniteDouble(writer, value.DoubleValue);
                return;
            case FerruleValueKind.Bool:
                writer.WriteBooleanValue(value.BooleanValue);
                return;
            default:
                throw Shape(
                    schema.Name,
                    "declared scalar union",
                    value.Kind.ToString());
        }
    }

    private static void WriteFiniteDouble(Utf8JsonWriter writer, double value)
    {
        writer.WriteRawValue(FormatSerdeFloat(value), skipInputValidation: false);
    }

    private static bool BoundaryAbsence(JsonSchemaNode schema, FerruleInstance instance)
    {
        if (instance is FerruleMappedSequence { Items.Count: 0 })
        {
            return !schema.Repeating || schema.ItemCountRange is { Minimum: > 0 };
        }
        if (instance is FerruleRepeated { Items.Count: 0 } &&
            schema.ItemCountRange is { Minimum: > 0 })
        {
            return true;
        }
        return instance is FerruleScalar { Value.Kind: FerruleValueKind.Null } &&
               (schema.ContainerNullable || !schema.Repeating && schema.IsScalar);
    }

    private static void ValidateRequired(
        JsonSchemaNode schema,
        IReadOnlyList<JsonProperty> properties)
    {
        foreach (var required in schema.Required)
        {
            if (!properties.Any(property =>
                    string.Equals(property.Name, required, StringComparison.Ordinal)))
            {
                throw Boundary(
                    $"JSON object '{schema.Name}' requires property '{required}'.");
            }
        }
    }

    private static void ValidateOutputRequired(
        JsonSchemaNode schema,
        FerruleGroup group)
    {
        foreach (var required in schema.Required)
        {
            var child = schema.Child(required) ?? schema.Dynamic;
            if (child is null ||
                !group.TryGetField(required, out var value) ||
                BoundaryAbsence(child, value))
            {
                throw Boundary(
                    $"JSON object '{schema.Name}' requires property '{required}'.");
            }
        }
    }

    private static void ValidateAlternatives(
        JsonSchemaNode schema,
        IReadOnlyList<JsonProperty> properties,
        NodeBudget budget)
    {
        if (schema.Alternatives.Count == 0)
        {
            return;
        }

        var matches = schema.Alternatives.Where(alternative =>
            alternative.Required.All(required =>
            {
                var property = properties.FirstOrDefault(candidate =>
                    string.Equals(candidate.Name, required, StringComparison.Ordinal));
                return property is not null &&
                       (property.Value.ValueKind != JsonValueKind.Null ||
                        schema.Child(required)?.Nullable == true);
            }) &&
            properties.All(property => alternative.Members.Contains(
                property.Name,
                StringComparer.Ordinal)) &&
            alternative.Constraints.All(constraint =>
            {
                var property = properties.FirstOrDefault(candidate =>
                    string.Equals(candidate.Name, constraint.Member, StringComparison.Ordinal));
                return property is null || ConstraintMatches(
                    constraint,
                    property.Value,
                    budget.OutputNormalizedNumbers);
            })).ToArray();
        if (matches.Length == 0)
        {
            throw Boundary($"JSON object '{schema.Name}' matches no declared schema alternative.");
        }

        if (matches.Length > 1 && !schema.InclusiveAlternatives &&
            (!schema.XmlTypeAlternatives || !HasUniqueNarrowestXmlType(matches)))
        {
            throw Boundary(
                $"JSON object '{schema.Name}' matches more than one declared schema alternative.");
        }
    }

    private static void ValidateOutputAlternatives(
        JsonSchemaNode schema,
        FerruleGroup group)
    {
        if (schema.Alternatives.Count == 0)
        {
            return;
        }

        var fields = new List<OutputProperty>();
        foreach (var child in schema.Children)
        {
            if (group.TryGetField(child.Name, out var value) &&
                !BoundaryAbsence(child, value))
            {
                fields.Add(new OutputProperty(child, value));
            }
        }

        var matches = schema.Alternatives.Where(alternative =>
            alternative.Required.All(required =>
                fields.Any(field =>
                    string.Equals(field.Schema.Name, required, StringComparison.Ordinal) &&
                    (!IsExplicitJsonNull(field.Value) || field.Schema.Nullable))) &&
            fields.All(field => alternative.Members.Contains(
                field.Schema.Name,
                StringComparer.Ordinal)) &&
            alternative.Constraints.All(constraint =>
            {
                var field = fields.FirstOrDefault(candidate =>
                    string.Equals(
                        candidate.Schema.Name,
                        constraint.Member,
                        StringComparison.Ordinal));
                return field is null ||
                       OutputConstraintMatches(constraint, field.Schema, field.Value);
            })).ToArray();
        if (matches.Length == 0)
        {
            throw Boundary($"JSON object '{schema.Name}' matches no declared schema alternative.");
        }

        if (matches.Length > 1 && !schema.InclusiveAlternatives &&
            (!schema.XmlTypeAlternatives || !HasUniqueNarrowestXmlType(matches)))
        {
            throw Boundary(
                $"JSON object '{schema.Name}' matches more than one declared schema alternative.");
        }
    }

    private static bool HasUniqueNarrowestXmlType(IReadOnlyList<JsonAlternative> matches)
    {
        for (var candidateIndex = 0; candidateIndex < matches.Count; candidateIndex++)
        {
            var members = matches[candidateIndex].Members;
            var narrowest = true;
            for (var otherIndex = 0; otherIndex < matches.Count; otherIndex++)
            {
                if (candidateIndex == otherIndex)
                {
                    continue;
                }
                var otherMembers = matches[otherIndex].Members;
                if (members.Count >= otherMembers.Count ||
                    members.Any(member => !otherMembers.Contains(member, StringComparer.Ordinal)))
                {
                    narrowest = false;
                    break;
                }
            }
            if (narrowest)
            {
                return true;
            }
        }
        return false;
    }

    private static bool OutputConstraintMatches(
        JsonConstraint constraint,
        JsonSchemaNode schema,
        FerruleInstance instance)
    {
        if (!schema.Repeating && instance is FerruleMappedSequence { Items.Count: 1 } mapped)
        {
            instance = mapped.Items[0];
        }
        if (instance is not FerruleScalar scalar)
        {
            return false;
        }

        var value = scalar.Value;
        if (constraint.Type == "json_null")
        {
            return value.Kind == FerruleValueKind.JsonNull &&
                   (schema.Nullable || schema.ContainerNullable);
        }

        if (!schema.IsScalar)
        {
            return false;
        }

        if (schema.IsScalarUnion)
        {
            try
            {
                return TaggedConstraintMatches(
                    constraint,
                    NormalizeScalarUnion(schema, value));
            }
            catch (FerruleRuntimeException)
            {
                return false;
            }
        }

        var domain = schema.ScalarDomain;
        return constraint.Type switch
        {
            "string" when domain.HasFlag(JsonScalarDomain.String) =>
                TryOutputString(value, out var actualString) &&
                string.Equals(
                    actualString,
                    constraint.Expected.GetString(),
                    StringComparison.Ordinal),
            "int" when domain.HasFlag(JsonScalarDomain.Int64) =>
                TryOutputInt64(value, out var actualInteger) &&
                TryGetSerdeInt64(constraint.Expected, out var expectedInteger) &&
                actualInteger == expectedInteger,
            "float" when domain.HasFlag(JsonScalarDomain.Double) =>
                TryOutputDouble(value, out var actualNumber) &&
                constraint.ExpectedFloat is { } expectedNumber &&
                actualNumber == expectedNumber,
            "bool" when domain.HasFlag(JsonScalarDomain.Bool) =>
                TryOutputBoolean(value, out var actualBoolean) &&
                constraint.Expected.ValueKind is JsonValueKind.True or JsonValueKind.False &&
                actualBoolean == constraint.Expected.GetBoolean(),
            _ => false,
        };
    }

    private static bool TaggedConstraintMatches(
        JsonConstraint constraint,
        FerruleValue value) =>
        (constraint.Type, value.Kind) switch
        {
            ("string", FerruleValueKind.String) =>
                string.Equals(
                    value.StringValue,
                    constraint.Expected.GetString(),
                    StringComparison.Ordinal),
            ("int", FerruleValueKind.Int64) =>
                TryGetSerdeInt64(constraint.Expected, out var expectedInteger) &&
                value.Int64Value == expectedInteger,
            ("float", FerruleValueKind.Double) =>
                constraint.ExpectedFloat is { } expectedNumber &&
                value.DoubleValue == expectedNumber,
            ("bool", FerruleValueKind.Bool) =>
                constraint.Expected.ValueKind is
                    JsonValueKind.True or JsonValueKind.False &&
                value.BooleanValue == constraint.Expected.GetBoolean(),
            _ => false,
        };

    private static bool TryOutputString(FerruleValue value, out string output)
    {
        output = value.Kind switch
        {
            FerruleValueKind.String => value.StringValue,
            FerruleValueKind.Bool => value.BooleanValue ? "true" : "false",
            FerruleValueKind.Int64 => value.Int64Value.ToString(CultureInfo.InvariantCulture),
            FerruleValueKind.Double when double.IsFinite(value.DoubleValue) =>
                FerruleValueMaps.RustFloatText(value.DoubleValue),
            _ => string.Empty,
        };
        return value.Kind is FerruleValueKind.String or
            FerruleValueKind.Bool or
            FerruleValueKind.Int64 ||
            value.Kind == FerruleValueKind.Double && double.IsFinite(value.DoubleValue);
    }

    private static bool TryOutputInt64(FerruleValue value, out long output)
    {
        if (value.Kind == FerruleValueKind.Int64)
        {
            output = value.Int64Value;
            return true;
        }

        output = 0;
        return value.Kind == FerruleValueKind.String &&
               TryExactDecimalInt64(value.StringValue.AsSpan(), out output);
    }

    // Match the XML writer's exact decimal-string output conversion without
    // passing through double or decimal, either of which can lose i64 precision.
    private static bool TryExactDecimalInt64(ReadOnlySpan<char> source, out long output)
    {
        output = 0;
        var text = source.Trim();
        if (long.TryParse(
                text,
                NumberStyles.AllowLeadingSign,
                CultureInfo.InvariantCulture,
                out output))
        {
            return true;
        }
        if (text.IsEmpty)
        {
            return false;
        }

        var negative = text[0] == '-';
        var unsigned = text[0] is '-' or '+' ? text[1..] : text;
        var exponentMarker = -1;
        for (var index = 0; index < unsigned.Length; index++)
        {
            if (unsigned[index] is 'e' or 'E')
            {
                if (exponentMarker >= 0)
                {
                    return false;
                }
                exponentMarker = index;
            }
        }
        var mantissa = exponentMarker >= 0 ? unsigned[..exponentMarker] : unsigned;
        var exponent = 0L;
        if (exponentMarker >= 0 &&
            !long.TryParse(
                unsigned[(exponentMarker + 1)..],
                NumberStyles.AllowLeadingSign,
                CultureInfo.InvariantCulture,
                out exponent))
        {
            return false;
        }

        var dot = -1;
        for (var index = 0; index < mantissa.Length; index++)
        {
            if (mantissa[index] == '.')
            {
                if (dot >= 0)
                {
                    return false;
                }
                dot = index;
            }
        }
        var whole = dot >= 0 ? mantissa[..dot] : mantissa;
        var fraction = dot >= 0 ? mantissa[(dot + 1)..] : ReadOnlySpan<char>.Empty;
        var totalDigits = whole.Length + fraction.Length;
        if (totalDigits == 0)
        {
            return false;
        }
        var firstNonzero = -1;
        for (var index = 0; index < totalDigits; index++)
        {
            var digit = DecimalDigitAt(whole, fraction, index);
            if (digit is < '0' or > '9')
            {
                return false;
            }
            if (firstNonzero < 0 && digit != '0')
            {
                firstNonzero = index;
            }
        }
        if (firstNonzero < 0)
        {
            return true;
        }

        long scale;
        try
        {
            scale = checked((long)fraction.Length - exponent);
        }
        catch (OverflowException)
        {
            return false;
        }
        var significantDigits = totalDigits - firstNonzero;
        var keptDigits = significantDigits;
        var appendedZeros = 0;
        if (scale > 0)
        {
            if (scale > significantDigits)
            {
                return false;
            }
            for (var index = totalDigits - (int)scale; index < totalDigits; index++)
            {
                if (DecimalDigitAt(whole, fraction, index) != '0')
                {
                    return false;
                }
            }
            keptDigits -= (int)scale;
        }
        else if (scale < 0)
        {
            if (scale < -19)
            {
                return false;
            }
            appendedZeros = (int)-scale;
        }
        if (keptDigits + appendedZeros > 19)
        {
            return false;
        }

        var limit = negative ? (ulong)long.MaxValue + 1UL : (ulong)long.MaxValue;
        var magnitude = 0UL;
        for (var index = firstNonzero; index < firstNonzero + keptDigits; index++)
        {
            var digit = (ulong)(DecimalDigitAt(whole, fraction, index) - '0');
            if (magnitude > (limit - digit) / 10UL)
            {
                return false;
            }
            magnitude = magnitude * 10UL + digit;
        }
        for (var index = 0; index < appendedZeros; index++)
        {
            if (magnitude > limit / 10UL)
            {
                return false;
            }
            magnitude *= 10UL;
        }
        output = negative
            ? magnitude == (ulong)long.MaxValue + 1UL ? long.MinValue : -(long)magnitude
            : (long)magnitude;
        return true;
    }

    private static char DecimalDigitAt(
        ReadOnlySpan<char> whole,
        ReadOnlySpan<char> fraction,
        int index) =>
        index < whole.Length ? whole[index] : fraction[index - whole.Length];

    private static bool TryOutputDouble(FerruleValue value, out double output)
    {
        if (value.Kind == FerruleValueKind.Int64 &&
            TryExactDouble(value.Int64Value, out output))
        {
            return true;
        }
        if (value.Kind == FerruleValueKind.Double && double.IsFinite(value.DoubleValue))
        {
            output = value.DoubleValue;
            return true;
        }

        output = 0;
        return value.Kind == FerruleValueKind.String &&
               double.TryParse(
                   value.StringValue.Trim(),
                   NumberStyles.Float,
                   CultureInfo.InvariantCulture,
                   out output) &&
               double.IsFinite(output);
    }

    private static bool TryReadExactDouble(
        JsonElement value,
        bool outputNormalizedNumbers,
        out double output)
    {
        output = 0;
        if (value.ValueKind != JsonValueKind.Number)
        {
            return false;
        }
        if (TryGetSerdeInt64(value, out var integer))
        {
            return TryExactDouble(integer, out output);
        }
        if (TryGetSerdeUInt64(value, out var unsignedInteger))
        {
            return TryExactDouble(unsignedInteger, out output);
        }

        if (outputNormalizedNumbers)
        {
            return value.TryGetDouble(out output) && double.IsFinite(output);
        }

        if (!TryParseSerdeNumber(value.GetRawText(), out var parsed) ||
            parsed.Kind != SerdeNumberKind.Float)
        {
            return false;
        }
        output = parsed.FloatValue;
        return true;
    }

    // Embedded f64 metadata follows Rust's typed scalar deserializer, which
    // rounds integer tokens. Runtime integer-to-number adaptation remains exact.
    private static bool TryReadFiniteMetadataDouble(JsonElement value, out double output)
    {
        output = 0;
        if (value.ValueKind != JsonValueKind.Number ||
            !TryParseSerdeNumber(value.GetRawText(), out var parsed))
        {
            return false;
        }
        output = parsed.AsDouble();
        return double.IsFinite(output);
    }

    private static bool TryReadFiniteMetadataDouble(
        JsonElement value, bool exactFloatMarkers, out double output) =>
        exactFloatMarkers
            ? FerruleEmbeddedSchema.TryReadFloat(value, out output)
            : TryReadFiniteMetadataDouble(value, out output);

    private static bool TryExactDouble(long value, out double output)
    {
        output = value;
        return new BigInteger(output) == new BigInteger(value);
    }

    private static bool TryExactDouble(ulong value, out double output)
    {
        output = value;
        return new BigInteger(output) == new BigInteger(value);
    }

    private static bool TryOutputBoolean(FerruleValue value, out bool output)
    {
        if (value.Kind == FerruleValueKind.Bool)
        {
            output = value.BooleanValue;
            return true;
        }
        if (value.Kind == FerruleValueKind.String &&
            string.Equals(value.StringValue.Trim(), "true", StringComparison.Ordinal))
        {
            output = true;
            return true;
        }
        if (value.Kind == FerruleValueKind.String &&
            string.Equals(value.StringValue.Trim(), "false", StringComparison.Ordinal))
        {
            output = false;
            return true;
        }

        output = false;
        return false;
    }

    private static bool IsExplicitJsonNull(FerruleInstance instance) =>
        instance is FerruleScalar { Value.Kind: FerruleValueKind.JsonNull } or
            FerruleMappedSequence
            {
                Items: [FerruleScalar { Value.Kind: FerruleValueKind.JsonNull }],
            };

    private static bool ConstraintMatches(
        JsonConstraint constraint,
        JsonElement value,
        bool outputNormalizedNumbers) =>
        constraint.Type switch
        {
            "string" => value.ValueKind == JsonValueKind.String &&
                        string.Equals(
                            constraint.Expected.GetString(),
                            value.GetString(),
                            StringComparison.Ordinal),
            "int" => value.ValueKind == JsonValueKind.Number &&
                     TryGetSerdeInt64(value, out var actualInteger) &&
                     TryGetSerdeInt64(constraint.Expected, out var expectedInteger) &&
                     actualInteger == expectedInteger,
            "float" => value.ValueKind == JsonValueKind.Number &&
                       TryReadExactDouble(value, outputNormalizedNumbers, out var actualNumber) &&
                       constraint.ExpectedFloat is { } expectedNumber &&
                       actualNumber == expectedNumber,
            "bool" => value.ValueKind is JsonValueKind.True or JsonValueKind.False &&
                      constraint.Expected.ValueKind is JsonValueKind.True or JsonValueKind.False &&
                      value.GetBoolean() == constraint.Expected.GetBoolean(),
            "json_null" => value.ValueKind == JsonValueKind.Null,
            _ => false,
        };

    private static List<JsonProperty> OrderedProperties(JsonElement element)
    {
        var properties = new List<JsonProperty>();
        var indexes = new Dictionary<string, int>(StringComparer.Ordinal);
        foreach (var property in element.EnumerateObject())
        {
            var item = new JsonProperty(property.Name, property.Value);
            if (indexes.TryGetValue(property.Name, out var index))
            {
                properties[index] = item;
            }
            else
            {
                indexes.Add(property.Name, properties.Count);
                properties.Add(item);
            }
        }

        return properties;
    }

    private static string[] RequiredStrings(JsonElement element, string name)
    {
        var values = RequiredProperty(element, name);
        RequireKind(values, JsonValueKind.Array, name, "array");
        return values.EnumerateArray().Select(value =>
        {
            if (value.ValueKind != JsonValueKind.String)
            {
                throw Boundary($"Embedded JSON schema field '{name}' must contain strings.");
            }

            return value.GetString() ?? string.Empty;
        }).ToArray();
    }

    private static JsonElement RequiredProperty(JsonElement element, string name)
    {
        JsonElement? value = null;
        foreach (var property in element.EnumerateObject())
        {
            if (!property.NameEquals(name))
            {
                continue;
            }
            if (value is not null)
            {
                throw Boundary($"Embedded JSON schema has duplicate field '{name}'.");
            }
            value = property.Value;
        }
        return value ?? throw Boundary($"Embedded JSON schema is missing field '{name}'.");
    }

    private static string RequiredString(JsonElement element, string name)
    {
        var value = RequiredProperty(element, name);
        return value.ValueKind == JsonValueKind.String
            ? value.GetString() ?? string.Empty
            : throw Boundary($"Embedded JSON schema field '{name}' must be a string.");
    }

    private static string? OptionalString(JsonElement element, string name)
    {
        if (!element.TryGetProperty(name, out var value))
        {
            return null;
        }
        return value.ValueKind == JsonValueKind.String
            ? value.GetString() ?? string.Empty
            : throw Boundary($"Embedded JSON schema field '{name}' must be a string.");
    }

    private static long? OptionalInt64(JsonElement element, string name)
    {
        if (!element.TryGetProperty(name, out var value))
        {
            return null;
        }
        return value.ValueKind == JsonValueKind.Number && TryGetSerdeInt64(value, out var integer)
            ? integer
            : throw Boundary($"Embedded JSON schema field '{name}' must be a signed integer.");
    }

    private static ulong? OptionalUInt64(
        JsonElement element,
        string name,
        bool allowNull)
    {
        if (!element.TryGetProperty(name, out var value))
        {
            return null;
        }
        if (allowNull && value.ValueKind == JsonValueKind.Null)
        {
            return null;
        }
        return value.ValueKind == JsonValueKind.Number && TryGetSerdeUInt64(value, out var integer)
            ? integer
            : throw Boundary(
                $"Embedded JSON schema field '{name}' must be a non-negative integer.");
    }

    private static bool OptionalBoolean(JsonElement element, string name) =>
        element.TryGetProperty(name, out var value) &&
        value.ValueKind switch
        {
            JsonValueKind.True => true,
            JsonValueKind.False => false,
            _ => throw Boundary($"Embedded JSON schema field '{name}' must be a boolean."),
        };

    private static void RequireKind(
        JsonElement element,
        JsonValueKind expected,
        string name,
        string expectedName)
    {
        if (element.ValueKind != expected)
        {
            throw Shape(name, expectedName, element.ValueKind.ToString());
        }
    }

    private static void RequireUtf8Limit(string value, int maximum, string label)
    {
        int bytes;
        try
        {
            bytes = StrictUtf8.GetByteCount(value);
        }
        catch (EncoderFallbackException error)
        {
            throw Boundary($"{label} contains an unpaired UTF-16 surrogate.", error);
        }
        if (bytes > maximum)
        {
            throw Boundary($"{label} is {bytes} bytes; maximum is {maximum}.");
        }
    }

    // JsonDocument defers decoding strings and permits numbers outside the
    // finite double range. Validate the complete syntax tree before projecting
    // fields or collapsing duplicates, matching the Rust JSON value boundary.
    private static void ValidateJsonScalars(JsonElement element)
    {
        try
        {
            switch (element.ValueKind)
            {
                case JsonValueKind.String:
                    _ = element.GetString();
                    break;
                case JsonValueKind.Number:
                    if (!TryParseSerdeNumber(element.GetRawText(), out _))
                    {
                        throw new JsonException("JSON number is outside the finite number range.");
                    }
                    break;
                case JsonValueKind.Array:
                    foreach (var item in element.EnumerateArray())
                    {
                        ValidateJsonScalars(item);
                    }
                    break;
                case JsonValueKind.Object:
                    foreach (var property in element.EnumerateObject())
                    {
                        _ = property.Name;
                        ValidateJsonScalars(property.Value);
                    }
                    break;
            }
        }
        catch (Exception error) when (
            error is InvalidOperationException or FormatException or OverflowException)
        {
            throw new JsonException("JSON scalar is invalid.", error);
        }
    }

    private static void ValidateInputJsonScalars(JsonSchemaNode schema, JsonElement element)
    {
        try
        {
            ValidateJsonScalars(element);
        }
        catch (JsonException)
        {
            // Rust checks exact raw uniqueItems before decoding JSON numbers.
            // Preserve that error precedence without repeating uniqueness work
            // for valid documents or consuming additional pattern-match work.
            var budget = new NodeBudget();
            if (!schema.Repeating && !schema.IsScalar && !schema.JsonAny &&
                element.ValueKind == JsonValueKind.Array)
            {
                foreach (var row in element.EnumerateArray())
                {
                    ValidateRawUniqueSingle(schema, row, budget, 1);
                }
            }
            else
            {
                ValidateRawUniqueNode(schema, element, budget, 0);
            }
            throw;
        }
    }

    private static void ValidateRawUniqueNode(
        JsonSchemaNode schema,
        JsonElement element,
        NodeBudget budget,
        int depth)
    {
        budget.Visit(depth);
        if (schema.ContainerNullable && element.ValueKind == JsonValueKind.Null)
        {
            return;
        }
        if (schema.Repeating && element.ValueKind == JsonValueKind.Array)
        {
            ValidateUniqueInputItems(schema, element, budget);
            foreach (var item in element.EnumerateArray())
            {
                ValidateRawUniqueSingle(schema, item, budget, depth + 1);
            }
        }
        else if (!schema.Repeating)
        {
            ValidateRawUniqueSingle(schema, element, budget, depth);
        }
    }

    private static void ValidateRawUniqueSingle(
        JsonSchemaNode schema,
        JsonElement element,
        NodeBudget budget,
        int depth)
    {
        if (schema.RecursiveReference is not null)
        {
            var resolved = schema.ResolvedRecursiveSchema ??
                throw Boundary($"Embedded JSON recursive reference '{schema.Name}' has no group anchor.");
            budget.EnterRecursiveReference();
            try
            {
                ValidateRawUniqueSingle(resolved, element, budget, depth);
            }
            finally
            {
                budget.ExitRecursiveReference();
            }
            return;
        }
        if (schema.JsonAny || schema.IsScalar || element.ValueKind != JsonValueKind.Object)
        {
            return;
        }
        foreach (var child in schema.Children)
        {
            if (element.TryGetProperty(child.Name, out var value))
            {
                ValidateRawUniqueNode(child, value, budget, depth + 1);
            }
        }
        if (schema.Dynamic is { } dynamic)
        {
            foreach (var property in OrderedProperties(element).OrderBy(
                         property => property.Name,
                         Comparer<string>.Create(CompareUtf8Ordinal)))
            {
                if (schema.Child(property.Name) is null)
                {
                    ValidateRawUniqueNode(dynamic, property.Value, budget, depth + 1);
                }
            }
        }
    }

    private static JsonDocument? ParseAnyJson(string value)
    {
        JsonDocument? document = null;
        try
        {
            document = JsonDocument.Parse(
                value,
                new JsonDocumentOptions
                {
                    MaxDepth = MaximumParsedDepth,
                    CommentHandling = JsonCommentHandling.Disallow,
                    AllowTrailingCommas = false,
                });
            ValidateJsonScalars(document.RootElement);
            return document;
        }
        catch (JsonException)
        {
            document?.Dispose();
            return null;
        }
    }

    private static FerruleRuntimeException Shape(string name, string expected, string found) =>
        Boundary($"JSON field '{name}' expected {expected}, got {found}.");

    private static FerruleRuntimeException Boundary(string message) =>
        new(FerruleRuntimeError.JsonBoundary, message, detail: message);

    private static FerruleRuntimeException Boundary(string message, Exception innerException) =>
        new(
            FerruleRuntimeError.JsonBoundary,
            message,
            innerException,
            detail: message);

    private static string ScalarName(JsonScalarType scalar) => scalar switch
    {
        JsonScalarType.String => "string",
        JsonScalarType.Int64 => "integer",
        JsonScalarType.Double => "number",
        JsonScalarType.Bool => "bool",
        _ => "scalar",
    };

    private static string ScalarName(JsonScalarDomain domain) =>
        IsSingleScalar(domain)
            ? ScalarName(SingleScalar(domain))
            : "declared scalar union";

    private static JsonScalarType SingleScalar(JsonScalarDomain domain) => domain switch
    {
        JsonScalarDomain.String => JsonScalarType.String,
        JsonScalarDomain.Int64 => JsonScalarType.Int64,
        JsonScalarDomain.Double => JsonScalarType.Double,
        JsonScalarDomain.Bool => JsonScalarType.Bool,
        _ => throw Boundary("Embedded JSON schema scalar domain is invalid."),
    };

    private static JsonScalarDomain? ValueDomain(FerruleValueKind kind) => kind switch
    {
        FerruleValueKind.String => JsonScalarDomain.String,
        FerruleValueKind.Int64 => JsonScalarDomain.Int64,
        FerruleValueKind.Double => JsonScalarDomain.Double,
        FerruleValueKind.Bool => JsonScalarDomain.Bool,
        _ => null,
    };

    private static bool IsSingleScalar(JsonScalarDomain domain)
    {
        var bits = (int)domain;
        return bits != 0 && (bits & (bits - 1)) == 0;
    }

    private static bool IsStringIntUnion(JsonScalarDomain domain) =>
        domain == (JsonScalarDomain.String | JsonScalarDomain.Int64);

    private static bool NumericRangeContains(
        JsonSchemaNode schema,
        JsonNumericRange range,
        FerruleValue value) =>
        (IsStringIntUnion(schema.ScalarDomain) &&
         range is JsonIntegerRange &&
         value.Kind == FerruleValueKind.String) || range.Contains(value);

    private static string InstanceKind(FerruleInstance instance) => instance switch
    {
        FerruleScalar scalar => scalar.Value.Kind.ToString(),
        FerruleGroup => "object",
        FerruleRepeated => "array",
        FerruleMappedSequence => "mapped sequence",
        FerruleDocumentSet => "document set",
        _ => "unknown",
    };

    private enum JsonScalarType
    {
        String,
        Int64,
        Double,
        Bool,
    }

    [Flags]
    private enum JsonScalarDomain
    {
        None = 0,
        String = 1 << 0,
        Int64 = 1 << 1,
        Double = 1 << 2,
        Bool = 1 << 3,
    }

    private sealed record JsonProperty(string Name, JsonElement Value);

    private sealed record OutputProperty(JsonSchemaNode Schema, FerruleInstance Value);

    private sealed record JsonConstraint(
        string Member, string Type, JsonElement Expected, double? ExpectedFloat);

    private sealed record JsonAlternative(
        string Name,
        IReadOnlyList<string> Members,
        IReadOnlyList<string> Required,
        IReadOnlyList<JsonConstraint> Constraints);

    private abstract record JsonNumericRange
    {
        public abstract bool Contains(FerruleValue value);
    }

    private sealed record JsonIntegerRange(long? Minimum, long? Maximum) : JsonNumericRange
    {
        public override bool Contains(FerruleValue value) =>
            value.Kind == FerruleValueKind.Int64 &&
            (Minimum is null || value.Int64Value >= Minimum) &&
            (Maximum is null || value.Int64Value <= Maximum);
    }

    private sealed record JsonNumberBound(double Value, bool Exclusive);

    private sealed record JsonNumberRange(
        JsonNumberBound? Minimum,
        JsonNumberBound? Maximum) : JsonNumericRange
    {
        public override bool Contains(FerruleValue value)
        {
            if (value.Kind != FerruleValueKind.Double || !double.IsFinite(value.DoubleValue))
            {
                return false;
            }
            var number = value.DoubleValue;
            return (Minimum is null ||
                    number > Minimum.Value ||
                    !Minimum.Exclusive && number == Minimum.Value) &&
                   (Maximum is null ||
                    number < Maximum.Value ||
                    !Maximum.Exclusive && number == Maximum.Value);
        }
    }

    private sealed record JsonItemCountRange(ulong Minimum, ulong? Maximum)
    {
        public bool Contains(int count)
        {
            var value = (ulong)count;
            return value >= Minimum && (Maximum is null || value <= Maximum);
        }
    }

    private sealed record JsonStringLengthRange(ulong Minimum, ulong? Maximum)
    {
        public bool Contains(string name, string value)
        {
            ulong count = 0;
            var remaining = value.AsSpan();
            while (!remaining.IsEmpty)
            {
                var status = Rune.DecodeFromUtf16(
                    remaining,
                    out _,
                    out var charsConsumed);
                if (status != OperationStatus.Done)
                {
                    throw Boundary(
                        $"JSON string '{name}' contains an unpaired UTF-16 surrogate.");
                }
                count = checked(count + 1);
                remaining = remaining[charsConsumed..];
            }
            return count >= Minimum && (Maximum is null || count <= Maximum);
        }
    }

    private sealed record JsonPatternAlternative(
        IReadOnlyList<string> Sources,
        IReadOnlyList<FerruleJsonPattern> Terms);

    private sealed record JsonPatternConstraints(
        IReadOnlyList<JsonPatternAlternative> AnyOf)
    {
        public bool IsMatch(string value, ref ulong remainingWork)
        {
            foreach (var alternative in AnyOf)
            {
                var matches = true;
                foreach (var pattern in alternative.Terms)
                {
                    if (!pattern.IsMatch(value, ref remainingWork))
                    {
                        matches = false;
                        break;
                    }
                }
                if (matches)
                {
                    return true;
                }
            }
            return false;
        }
    }

    private sealed class JsonPatternSchemaContext
    {
        private readonly Dictionary<string, FerruleJsonPattern> _compiled =
            new(StringComparer.Ordinal);
        private int _sourceBytes;
        private int _instructions;
        private ulong _remainingFixedPatternWork = FerruleJsonPattern.MaximumBoundaryWork;

        public FerruleJsonPattern GetOrCompile(string nodeName, string source)
        {
            if (_compiled.TryGetValue(source, out var existing))
            {
                return existing;
            }
            if (_compiled.Count == MaximumDistinctJsonPatterns)
            {
                throw Boundary(
                    $"Embedded JSON schema exceeds the {MaximumDistinctJsonPatterns}-distinct-pattern limit at node '{nodeName}'.");
            }

            var compiled = FerruleJsonPattern.Compile(source);
            _sourceBytes = checked(_sourceBytes + StrictUtf8.GetByteCount(source));
            _instructions = checked(_instructions + compiled.InstructionCount);
            if (_sourceBytes > MaximumJsonPatternSourceBytes)
            {
                throw Boundary(
                    $"Embedded JSON schema JSON pattern sources exceed the {MaximumJsonPatternSourceBytes}-byte limit.");
            }
            if (_instructions > MaximumJsonPatternInstructions)
            {
                throw Boundary(
                    $"Embedded JSON schema JSON patterns exceed the {MaximumJsonPatternInstructions}-instruction limit.");
            }

            _compiled.Add(source, compiled);
            return compiled;
        }

        public bool FixedMatches(JsonPatternConstraints constraints, string value) =>
            constraints.IsMatch(value, ref _remainingFixedPatternWork);

        public bool FixedMatches(
            JsonPatternPropertyNames selectors,
            string value) =>
            selectors.IsMatch(value, ref _remainingFixedPatternWork);
    }

    private sealed class JsonSchemaNode
    {
        public JsonSchemaNode(
            string name,
            bool repeating,
            bool nullable,
            bool containerNullable,
            bool jsonAny,
            IReadOnlyList<string> jsonFormats,
            JsonScalarDomain scalarDomain,
            string? fixedLexical,
            FerruleValue? fixedValue,
            JsonAllowedValues? jsonAllowedValues,
            JsonNumericRange? numericRange,
            JsonMultipleOfConstraints? jsonMultipleOf,
            JsonStringLengthRange? stringLengthRange,
            JsonPatternConstraints? jsonPatterns,
            JsonItemCountRange? itemCountRange,
            IReadOnlyList<JsonContainsConstraint> jsonContains,
            JsonPropertyCountRange? propertyCountRange,
            IReadOnlyList<JsonPropertyDependency> propertyDependencies,
            IReadOnlyList<JsonDependentSchema> dependentSchemas,
            JsonPropertyNameConstraints? propertyNames,
            JsonPatternPropertyNames? patternPropertyNames,
            bool jsonUniqueItems,
            IReadOnlyList<JsonSchemaNode> children,
            JsonSchemaNode? dynamic,
            IReadOnlyList<string> required,
            IReadOnlyList<JsonAlternative> alternatives,
            bool inclusiveAlternatives,
            bool xmlTypeAlternatives,
            string? recursiveReference)
        {
            Name = name;
            Repeating = repeating;
            Nullable = nullable;
            ContainerNullable = containerNullable;
            JsonAny = jsonAny;
            JsonFormats = jsonFormats;
            ScalarDomain = scalarDomain;
            FixedLexical = fixedLexical;
            Fixed = fixedValue;
            JsonAllowedValues = jsonAllowedValues;
            NumericRange = numericRange;
            JsonMultipleOf = jsonMultipleOf;
            StringLengthRange = stringLengthRange;
            JsonPatterns = jsonPatterns;
            ItemCountRange = itemCountRange;
            JsonContains = jsonContains;
            PropertyCountRange = propertyCountRange;
            PropertyDependencies = propertyDependencies;
            DependentSchemas = dependentSchemas;
            PropertyNames = propertyNames;
            PatternPropertyNames = patternPropertyNames;
            JsonUniqueItems = jsonUniqueItems;
            Children = children;
            DuplicateChildren = children.Select(child => child.Name).Distinct(StringComparer.Ordinal).Count() != children.Count;
            Dynamic = dynamic;
            Required = required;
            Alternatives = alternatives;
            InclusiveAlternatives = inclusiveAlternatives;
            XmlTypeAlternatives = xmlTypeAlternatives;
            RecursiveReference = recursiveReference;
        }

        public string Name { get; private set; }

        public bool Repeating { get; private set; }

        public bool Nullable { get; }

        public bool ContainerNullable { get; private set; }

        public bool JsonAny { get; }

        public IReadOnlyList<string> JsonFormats { get; }

        public JsonScalarDomain ScalarDomain { get; }

        public string? FixedLexical { get; }

        public FerruleValue? Fixed { get; }

        public JsonAllowedValues? JsonAllowedValues { get; }

        public JsonNumericRange? NumericRange { get; }

        public JsonMultipleOfConstraints? JsonMultipleOf { get; }

        public JsonStringLengthRange? StringLengthRange { get; }

        public JsonPatternConstraints? JsonPatterns { get; }

        public JsonItemCountRange? ItemCountRange { get; private set; }

        public IReadOnlyList<JsonContainsConstraint> JsonContains { get; private set; }

        public JsonPropertyCountRange? PropertyCountRange { get; }

        public IReadOnlyList<JsonPropertyDependency> PropertyDependencies { get; }

        public IReadOnlyList<JsonDependentSchema> DependentSchemas { get; }

        public JsonPropertyNameConstraints? PropertyNames { get; }

        public JsonPatternPropertyNames? PatternPropertyNames { get; }

        public bool JsonUniqueItems { get; private set; }

        public string? RecursiveReference { get; }

        public JsonSchemaNode? ResolvedRecursiveSchema { get; private set; }

        public bool IsScalar => ScalarDomain != JsonScalarDomain.None;

        public bool IsScalarUnion => IsScalar && !IsSingleScalar(ScalarDomain);

        public IReadOnlyList<JsonSchemaNode> Children { get; }

        public bool DuplicateChildren { get; }

        public JsonSchemaNode? Dynamic { get; }

        public IReadOnlyList<string> Required { get; }

        public IReadOnlyList<JsonAlternative> Alternatives { get; }

        public bool InclusiveAlternatives { get; }

        public bool XmlTypeAlternatives { get; }

        public JsonSchemaNode? Child(string name) =>
            Children.FirstOrDefault(child =>
                string.Equals(child.Name, name, StringComparison.Ordinal));

        public void BindRecursiveAnchor(JsonSchemaNode anchor)
        {
            var resolved = (JsonSchemaNode)anchor.MemberwiseClone();
            resolved.Name = Name;
            resolved.Repeating = Repeating;
            resolved.ContainerNullable = ContainerNullable;
            resolved.ItemCountRange = ItemCountRange;
            resolved.JsonContains = JsonContains;
            resolved.JsonUniqueItems = JsonUniqueItems;
            ResolvedRecursiveSchema = resolved;
        }
    }

    private sealed class NodeBudget
    {
        private readonly PatternWorkBudget _patternWork;
        private readonly bool _outputNormalizedNumbers;
        private int _recursiveReferences;
        private bool _fatalTraversalLimit;
        private NodeBudget? _matcher;
        private NodeBudget? _outputMatcher;

        public NodeBudget()
            : this(new PatternWorkBudget(), false)
        {
        }

        private NodeBudget(PatternWorkBudget patternWork, bool outputNormalizedNumbers)
        {
            _patternWork = patternWork;
            _outputNormalizedNumbers = outputNormalizedNumbers;
        }

        public bool OutputNormalizedNumbers => _outputNormalizedNumbers;

        public bool HasFatalLimit =>
            _fatalTraversalLimit || _patternWork.HasFatalLimit;

        public NodeBudget Matcher() =>
            _matcher ??= new NodeBudget(_patternWork, _outputNormalizedNumbers);

        public NodeBudget OutputMatcher() =>
            _outputMatcher ??= new NodeBudget(_patternWork, true);

        public void MarkFatalTraversalLimit()
        {
            _fatalTraversalLimit = true;
        }

        public void EnterRecursiveReference()
        {
            if (_recursiveReferences >= MaximumRecursiveReferences)
            {
                _fatalTraversalLimit = true;
                throw Boundary(
                    $"JSON recursive references exceed the {MaximumRecursiveReferences}-level limit.");
            }
            _recursiveReferences++;
        }

        public void ExitRecursiveReference()
        {
            _recursiveReferences--;
        }

        public void Visit(int depth)
        {
            if (depth > MaximumDepth)
            {
                _fatalTraversalLimit = true;
                throw Boundary($"JSON nesting exceeds the {MaximumDepth}-level limit.");
            }
        }

        public bool Matches(JsonPatternConstraints constraints, string value) =>
            _patternWork.Matches(constraints, value);

        public bool Matches(FerruleJsonPattern pattern, string value) =>
            _patternWork.Matches(pattern, value);
    }

    private sealed class PatternWorkBudget
    {
        private ulong _remaining = FerruleJsonPattern.MaximumBoundaryWork;

        public bool HasFatalLimit { get; private set; }

        public bool Matches(JsonPatternConstraints constraints, string value)
        {
            try
            {
                return constraints.IsMatch(value, ref _remaining);
            }
            catch (FerruleRuntimeException)
            {
                HasFatalLimit = true;
                throw;
            }
        }

        public bool Matches(FerruleJsonPattern pattern, string value)
        {
            try
            {
                return pattern.IsMatch(value, ref _remaining);
            }
            catch (FerruleRuntimeException)
            {
                HasFatalLimit = true;
                throw;
            }
        }
    }
}

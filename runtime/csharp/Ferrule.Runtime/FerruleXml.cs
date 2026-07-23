using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Xml;

namespace Ferrule.Runtime;

public sealed partial class ScopeContext
{
    /// <summary>Resolves one complete structured value for XML serialization.</summary>
    public FerruleInstance ResolveXmlInstance(
        IReadOnlyList<string>? frame,
        IReadOnlyList<string> path)
    {
        ArgumentNullException.ThrowIfNull(path);
        ValidatePath(path);
        if (frame is not null)
        {
            ValidatePath(frame);
            for (var index = _collections.Count - 1; index >= 0; index--)
            {
                var collection = _collections[index];
                if (!FrameMatches(frame, collection.Path))
                {
                    continue;
                }
                if (TryResolveXmlInstance(collection.Item, path, 0, out var framed))
                {
                    return framed;
                }
                break;
            }
            throw MissingXmlInstance(frame.Concat(path));
        }

        for (var index = _collections.Count - 1; index >= 0; index--)
        {
            var collection = _collections[index];
            if (collection.Path.Count != 0 &&
                StartsWith(path, collection.Path) &&
                TryResolveXmlInstance(
                    collection.Item,
                    path,
                    collection.Path.Count,
                    out var active))
            {
                return active;
            }
        }
        for (var index = _frames.Count - 1; index >= 0; index--)
        {
            if (TryResolveXmlInstance(_frames[index], path, 0, out var value))
            {
                return value;
            }
        }
        throw MissingXmlInstance(path);
    }

    private static bool TryResolveXmlInstance(
        FerruleInstance source,
        IReadOnlyList<string> path,
        int pathIndex,
        out FerruleInstance value)
    {
        var current = source;
        for (var index = pathIndex; index < path.Count; index++)
        {
            if (current is FerruleRepeated repeated)
            {
                if (repeated.Items.Count == 0)
                {
                    value = source;
                    return false;
                }
                current = repeated.Items[0];
            }
            if (!TryGetField(current, path[index], out var next))
            {
                value = source;
                return false;
            }
            current = next;
        }
        if (current is FerruleRepeated terminal)
        {
            if (terminal.Items.Count == 0)
            {
                value = source;
                return false;
            }
            current = terminal.Items[0];
        }
        value = current;
        return true;
    }

    private static FerruleRuntimeException MissingXmlInstance(IEnumerable<string> path) =>
        new(
            FerruleRuntimeError.MissingSourceField,
            $"Structured source '{string.Join('/', path)}' does not exist in the active scope context.");
}

/// <summary>Bounded package-free XML serialization for generated mappings.</summary>
public static class FerruleXml
{
    public const int MaximumEmbeddedSchemaBytes = 8 * 1024 * 1024;
    public const int MaximumOutputBytes = 64 * 1024 * 1024;
    private const int MaximumSchemaDepth = 256;
    private const int MaximumRecursiveDepth = 64;
    private const string XsiNamespace = "http://www.w3.org/2001/XMLSchema-instance";

    public static FerruleValue Serialize(
        uint node,
        string schemaJson,
        FerruleInstance instance,
        bool declaration,
        bool indent,
        string? defaultNamespace)
    {
        ArgumentNullException.ThrowIfNull(schemaJson);
        ArgumentNullException.ThrowIfNull(instance);
        if (defaultNamespace is { Length: 0 })
        {
            throw Error(node, "default namespace cannot be empty");
        }
        if (Encoding.UTF8.GetByteCount(schemaJson) > MaximumEmbeddedSchemaBytes)
        {
            throw Error(node, $"embedded schema exceeds {MaximumEmbeddedSchemaBytes} bytes");
        }

        try
        {
            using var document = JsonDocument.Parse(
                schemaJson,
                new JsonDocumentOptions { MaxDepth = MaximumSchemaDepth });
            var schema = XmlSchemaNode.Parse(document.RootElement, 0);
            if (schema.Repeating)
            {
                throw new InvalidOperationException(
                    "XML serializer schema must describe one document element");
            }
            var writer = new XmlOutput(indent);
            if (declaration)
            {
                writer.Append("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
                if (indent)
                {
                    writer.Append('\n');
                }
            }
            writer.WriteNode(schema, schema, instance, true, 0, 0, defaultNamespace);
            var output = writer.ToString();
            if (Encoding.UTF8.GetByteCount(output) > MaximumOutputBytes)
            {
                throw new InvalidOperationException(
                    $"serialized output exceeds {MaximumOutputBytes} bytes");
            }
            return FerruleValue.FromString(output);
        }
        catch (FerruleRuntimeException)
        {
            throw;
        }
        catch (Exception exception) when (
            exception is JsonException or InvalidOperationException or
            FormatException or OverflowException or ArgumentException or XmlException)
        {
            throw Error(node, exception.Message, exception);
        }
    }

    private static FerruleRuntimeException Error(
        uint node,
        string detail,
        Exception? inner = null)
    {
        var message = $"node {node}: XML serialization failed: {detail}";
        return inner is null
            ? new FerruleRuntimeException(
                FerruleRuntimeError.XmlSerialization,
                message,
                node: node,
                detail: detail)
            : new FerruleRuntimeException(
                FerruleRuntimeError.XmlSerialization,
                message,
                inner,
                node: node,
                detail: detail);
    }

    private enum XmlScalarType
    {
        String,
        Int,
        Float,
        Bool,
    }

    private sealed record XmlSchemaNode(
        string Name,
        bool Repeating,
        string? RecursiveReference,
        bool Attribute,
        bool Text,
        bool Nillable,
        string? Fixed,
        XmlScalarType? ScalarType,
        IReadOnlyList<XmlSchemaNode> Children)
    {
        internal static XmlSchemaNode Parse(JsonElement element, int depth)
        {
            if (depth >= MaximumSchemaDepth || element.ValueKind != JsonValueKind.Object)
            {
                throw new InvalidOperationException("embedded XML schema exceeds its depth limit");
            }
            var name = RequiredString(element, "name");
            if (name.Length == 0)
            {
                throw new InvalidOperationException("embedded XML schema name cannot be empty");
            }
            _ = XmlConvert.VerifyName(name);
            var kind = Required(element, "kind");
            var kindName = RequiredString(kind, "kind");
            XmlScalarType? scalarType = null;
            var children = Array.Empty<XmlSchemaNode>();
            if (kindName == "scalar")
            {
                scalarType = RequiredString(kind, "ty") switch
                {
                    "string" => XmlScalarType.String,
                    "int" => XmlScalarType.Int,
                    "float" => XmlScalarType.Float,
                    "bool" => XmlScalarType.Bool,
                    var value => throw new InvalidOperationException(
                        $"unsupported XML scalar type '{value}'"),
                };
            }
            else if (kindName == "group")
            {
                var childElements = Required(kind, "children");
                if (childElements.ValueKind != JsonValueKind.Array)
                {
                    throw new InvalidOperationException("XML group children must be an array");
                }
                children = childElements
                    .EnumerateArray()
                    .Select(child => Parse(child, depth + 1))
                    .ToArray();
            }
            else
            {
                throw new InvalidOperationException($"unsupported XML schema kind '{kindName}'");
            }
            return new XmlSchemaNode(
                name,
                OptionalBoolean(element, "repeating"),
                OptionalString(element, "recursive_ref"),
                OptionalBoolean(element, "attribute"),
                OptionalBoolean(element, "text"),
                OptionalBoolean(element, "nillable"),
                OptionalString(element, "fixed"),
                scalarType,
                children);
        }

        private static JsonElement Required(JsonElement element, string name) =>
            element.TryGetProperty(name, out var value)
                ? value
                : throw new InvalidOperationException(
                    $"embedded XML schema is missing '{name}'");

        private static string RequiredString(JsonElement element, string name)
        {
            var value = Required(element, name);
            return value.ValueKind == JsonValueKind.String
                ? value.GetString() ?? string.Empty
                : throw new InvalidOperationException(
                    $"embedded XML schema '{name}' must be a string");
        }

        private static string? OptionalString(JsonElement element, string name) =>
            element.TryGetProperty(name, out var value) && value.ValueKind != JsonValueKind.Null
                ? value.GetString()
                : null;

        private static bool OptionalBoolean(JsonElement element, string name) =>
            element.TryGetProperty(name, out var value) && value.GetBoolean();
    }

    private sealed class XmlOutput
    {
        private readonly StringBuilder _output = new();
        private readonly bool _indent;

        internal XmlOutput(bool indent)
        {
            _indent = indent;
        }

        internal void Append(string value) => _output.Append(value);

        internal void Append(char value) => _output.Append(value);

        public override string ToString() => _output.ToString();

        internal void WriteNode(
            XmlSchemaNode schema,
            XmlSchemaNode rootSchema,
            FerruleInstance instance,
            bool isRoot,
            int recursionDepth,
            int outputDepth,
            string? defaultNamespace)
        {
            schema = Resolve(schema, rootSchema, recursionDepth);
            if (instance is FerruleMappedSequence mapped)
            {
                if (isRoot || schema.Repeating || schema.ScalarType is not null)
                {
                    throw Shape(schema, "one non-repeating element group", instance);
                }
                for (var index = 0; index < mapped.Items.Count; index++)
                {
                    if (_indent && index != 0)
                    {
                        NewLine(outputDepth);
                    }
                    WriteSingle(
                        schema,
                        rootSchema,
                        mapped.Items[index],
                        recursionDepth,
                        outputDepth,
                        null);
                }
                return;
            }
            if (schema.Repeating && !isRoot)
            {
                if (instance is not FerruleRepeated repeated)
                {
                    throw Shape(schema, "repeating elements", instance);
                }
                for (var index = 0; index < repeated.Items.Count; index++)
                {
                    if (_indent && index != 0)
                    {
                        NewLine(outputDepth);
                    }
                    WriteSingle(
                        schema,
                        rootSchema,
                        repeated.Items[index],
                        recursionDepth,
                        outputDepth,
                        null);
                }
                return;
            }
            if (instance is FerruleRepeated)
            {
                throw Shape(schema, isRoot ? "one document root" : "one element", instance);
            }
            WriteSingle(
                schema,
                rootSchema,
                instance,
                recursionDepth,
                outputDepth,
                isRoot ? defaultNamespace : null);
        }

        private void WriteSingle(
            XmlSchemaNode schema,
            XmlSchemaNode rootSchema,
            FerruleInstance instance,
            int recursionDepth,
            int outputDepth,
            string? defaultNamespace)
        {
            if (schema.ScalarType is { } scalarType)
            {
                if (instance is not FerruleScalar scalar)
                {
                    throw Shape(schema, "a scalar", instance);
                }
                if (scalar.Value.Kind == FerruleValueKind.XmlNil)
                {
                    if (!schema.Nillable)
                    {
                        throw new InvalidOperationException(
                            $"element '{schema.Name}' does not permit XML nil");
                    }
                    Start(schema.Name, defaultNamespace);
                    Attribute("xmlns:xsi", XsiNamespace);
                    Attribute("xsi:nil", "true");
                    _output.Append("/>");
                    return;
                }
                Start(schema.Name, defaultNamespace);
                _output.Append('>');
                Text(FormatScalar(schema, scalarType, scalar.Value));
                End(schema.Name);
                return;
            }

            if (instance is not FerruleGroup group)
            {
                throw Shape(schema, "an element group", instance);
            }
            ValidateFields(schema, group);
            Start(schema.Name, defaultNamespace);
            foreach (var attribute in schema.Children.Where(child => child.Attribute))
            {
                if (!group.TryGetField(attribute.Name, out var field))
                {
                    continue;
                }
                if (field is not FerruleScalar scalar)
                {
                    throw Shape(attribute, "an attribute scalar", field);
                }
                if (scalar.Value.Kind != FerruleValueKind.Null)
                {
                    var type = attribute.ScalarType ?? throw new InvalidOperationException(
                        $"attribute '{attribute.Name}' must have a scalar schema");
                    Attribute(attribute.Name, FormatScalar(attribute, type, scalar.Value));
                }
            }

            var textChildren = schema.Children.Where(child => child.Text).ToArray();
            if (textChildren.Length != 0 && !HasSerializedContent(schema, group))
            {
                _output.Append("/>");
                return;
            }
            _output.Append('>');
            foreach (var textChild in textChildren)
            {
                if (!group.TryGetField(textChild.Name, out var field))
                {
                    continue;
                }
                if (field is not FerruleScalar scalar)
                {
                    throw Shape(textChild, "a text scalar", field);
                }
                if (scalar.Value.Kind != FerruleValueKind.Null)
                {
                    var type = textChild.ScalarType ?? throw new InvalidOperationException(
                        $"text field '{textChild.Name}' must have a scalar schema");
                    Text(FormatScalar(textChild, type, scalar.Value));
                }
            }

            var wroteElement = false;
            foreach (var child in schema.Children.Where(child => !child.Attribute && !child.Text))
            {
                if (!group.TryGetField(child.Name, out var field) || !WillWrite(child, field))
                {
                    continue;
                }
                var childDepth = recursionDepth + (child.RecursiveReference is null ? 0 : 1);
                if (_indent)
                {
                    NewLine(outputDepth + 1);
                }
                WriteNode(
                    child,
                    rootSchema,
                    field,
                    false,
                    childDepth,
                    outputDepth + 1,
                    null);
                wroteElement = true;
            }
            if (_indent && wroteElement)
            {
                NewLine(outputDepth);
            }
            End(schema.Name);
        }

        private static XmlSchemaNode Resolve(
            XmlSchemaNode schema,
            XmlSchemaNode root,
            int recursionDepth)
        {
            if (schema.RecursiveReference is null)
            {
                return schema;
            }
            if (recursionDepth >= MaximumRecursiveDepth)
            {
                throw new InvalidOperationException(
                    $"XML recursion exceeds {MaximumRecursiveDepth} groups");
            }
            var anchor = FindAnchor(root, schema.RecursiveReference) ??
                throw new InvalidOperationException(
                    $"XML recursive schema anchor '{schema.RecursiveReference}' does not exist");
            return anchor with
            {
                Name = schema.Name,
                Repeating = schema.Repeating,
                Nillable = schema.Nillable,
            };
        }

        private static XmlSchemaNode? FindAnchor(XmlSchemaNode schema, string name)
        {
            if (schema.RecursiveReference is null &&
                schema.ScalarType is null &&
                schema.Name == name)
            {
                return schema;
            }
            foreach (var child in schema.Children)
            {
                var found = FindAnchor(child, name);
                if (found is not null)
                {
                    return found;
                }
            }
            return null;
        }

        private void Start(string name, string? defaultNamespace)
        {
            _output.Append('<').Append(name);
            if (defaultNamespace is not null)
            {
                Attribute("xmlns", defaultNamespace);
            }
        }

        private void End(string name) => _output.Append("</").Append(name).Append('>');

        private void Attribute(string name, string value)
        {
            _output.Append(' ').Append(name).Append("=\"");
            foreach (var character in value)
            {
                _output.Append(character switch
                {
                    '&' => "&amp;",
                    '<' => "&lt;",
                    '"' => "&quot;",
                    '\t' => "&#x9;",
                    '\n' => "&#xA;",
                    '\r' => "&#xD;",
                    _ => character.ToString(),
                });
            }
            _output.Append('"');
        }

        private void Text(string value)
        {
            foreach (var character in value)
            {
                _output.Append(character switch
                {
                    '&' => "&amp;",
                    '<' => "&lt;",
                    '>' => "&gt;",
                    '\'' => "&apos;",
                    '"' => "&quot;",
                    _ => character.ToString(),
                });
            }
        }

        private void NewLine(int depth)
        {
            _output.Append('\n').Append(' ', depth * 2);
        }

        private static bool WillWrite(XmlSchemaNode schema, FerruleInstance instance) =>
            instance switch
            {
                FerruleScalar scalar when schema.ScalarType is not null =>
                    schema.Repeating || scalar.Value.Kind != FerruleValueKind.Null,
                FerruleScalar => true,
                FerruleRepeated repeated => repeated.Items.Count != 0,
                FerruleMappedSequence mapped => mapped.Items.Count != 0,
                _ => true,
            };

        private static bool HasSerializedContent(XmlSchemaNode schema, FerruleGroup group)
        {
            foreach (var child in schema.Children.Where(child => !child.Attribute))
            {
                if (!group.TryGetField(child.Name, out var field))
                {
                    continue;
                }
                if (field is FerruleScalar scalar && scalar.Value.Kind == FerruleValueKind.Null)
                {
                    continue;
                }
                if (field is FerruleScalar { Value.Kind: FerruleValueKind.String } text &&
                    text.Value.StringValue.Length == 0 && child.Text)
                {
                    continue;
                }
                if (field is FerruleRepeated repeated && repeated.Items.Count == 0)
                {
                    continue;
                }
                if (field is FerruleMappedSequence mapped && mapped.Items.Count == 0)
                {
                    continue;
                }
                return true;
            }
            return false;
        }

        private static void ValidateFields(XmlSchemaNode schema, FerruleGroup group)
        {
            foreach (var field in group.Fields)
            {
                if (!schema.Children.Any(child => child.Name == field.Name))
                {
                    throw new InvalidOperationException(
                        $"group '{schema.Name}' contains unexpected field '{field.Name}'");
                }
            }
        }

        private static string FormatScalar(
            XmlSchemaNode schema,
            XmlScalarType type,
            FerruleValue value)
        {
            var text = type switch
            {
                XmlScalarType.String => value.Kind switch
                {
                    FerruleValueKind.Bool => value.BooleanValue ? "true" : "false",
                    FerruleValueKind.Int64 => value.Int64Value.ToString(CultureInfo.InvariantCulture),
                    FerruleValueKind.Double when double.IsFinite(value.DoubleValue) =>
                        FerruleValueMaps.RustFloatText(value.DoubleValue),
                    FerruleValueKind.String => value.StringValue,
                    _ => throw ValueType(schema, type, value),
                },
                XmlScalarType.Int => value.Kind switch
                {
                    FerruleValueKind.Int64 => value.Int64Value.ToString(CultureInfo.InvariantCulture),
                    FerruleValueKind.Double when ExactInt64(value.DoubleValue, out var integer) =>
                        integer.ToString(CultureInfo.InvariantCulture),
                    FerruleValueKind.String when LexicalInt64(value.StringValue, out var integer) =>
                        integer.ToString(CultureInfo.InvariantCulture),
                    _ => throw ValueType(schema, type, value),
                },
                XmlScalarType.Float => value.Kind switch
                {
                    FerruleValueKind.Double when double.IsFinite(value.DoubleValue) =>
                        FerruleValueMaps.RustFloatText(value.DoubleValue),
                    FerruleValueKind.Int64 when ExactDouble(value.Int64Value) =>
                        value.Int64Value.ToString(CultureInfo.InvariantCulture),
                    FerruleValueKind.String when
                        double.TryParse(
                            value.StringValue.AsSpan().Trim(),
                            NumberStyles.Float,
                            CultureInfo.InvariantCulture,
                            out var number) && double.IsFinite(number) =>
                        FerruleValueMaps.RustFloatText(number),
                    _ => throw ValueType(schema, type, value),
                },
                XmlScalarType.Bool => value.Kind switch
                {
                    FerruleValueKind.Bool => value.BooleanValue ? "true" : "false",
                    FerruleValueKind.String when value.StringValue.AsSpan().Trim() is "true" or "1" =>
                        "true",
                    FerruleValueKind.String when value.StringValue.AsSpan().Trim() is "false" or "0" =>
                        "false",
                    _ => throw ValueType(schema, type, value),
                },
                _ => throw new InvalidOperationException("unsupported XML scalar type"),
            };
            if (schema.Fixed is null)
            {
                return text;
            }
            if (ParseLexical(type, text) != ParseLexical(type, schema.Fixed))
            {
                throw new InvalidOperationException(
                    $"element or attribute '{schema.Name}' requires fixed value '{schema.Fixed}', got '{text}'");
            }
            return schema.Fixed;
        }

        private static FerruleValue ParseLexical(XmlScalarType type, string text) => type switch
        {
            XmlScalarType.String => FerruleValue.FromString(text),
            XmlScalarType.Int when LexicalInt64(text, out var integer) =>
                FerruleValue.FromInt64(integer),
            XmlScalarType.Float when
                double.TryParse(
                    text.AsSpan().Trim(),
                    NumberStyles.Float,
                    CultureInfo.InvariantCulture,
                    out var number) && double.IsFinite(number) =>
                FerruleValue.FromDouble(number),
            XmlScalarType.Bool when text.AsSpan().Trim() is "true" or "1" =>
                FerruleValue.FromBoolean(true),
            XmlScalarType.Bool when text.AsSpan().Trim() is "false" or "0" =>
                FerruleValue.FromBoolean(false),
            _ => throw new InvalidOperationException(
                $"fixed XML value '{text}' is invalid for {type}"),
        };

        private static InvalidOperationException ValueType(
            XmlSchemaNode schema,
            XmlScalarType expected,
            FerruleValue value) =>
            new($"element or attribute '{schema.Name}' expected {expected}, got {value.Kind}");

        private static InvalidOperationException Shape(
            XmlSchemaNode schema,
            string expected,
            FerruleInstance instance) =>
            new($"element '{schema.Name}' expected {expected}, got {InstanceKind(instance)}");

        private static string InstanceKind(FerruleInstance instance) => instance switch
        {
            FerruleScalar => "a scalar",
            FerruleGroup => "an element group",
            FerruleRepeated => "repeating elements",
            FerruleMappedSequence => "a mapped element sequence",
            FerruleDocumentSet => "a document set",
            _ => "an unknown instance",
        };

        private static bool ExactDouble(long value)
        {
            var number = (double)value;
            return number >= (double)long.MinValue &&
                number < -(double)long.MinValue &&
                (long)number == value;
        }

        private static bool ExactInt64(double value, out long integer)
        {
            if (double.IsFinite(value) &&
                Math.Truncate(value) == value &&
                value >= (double)long.MinValue &&
                value < -(double)long.MinValue)
            {
                integer = (long)value;
                return true;
            }
            integer = 0;
            return false;
        }

        private static bool LexicalInt64(string value, out long integer)
        {
            var text = value.AsSpan().Trim();
            if (long.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out integer))
            {
                return true;
            }
            if (decimal.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var number) &&
                decimal.Truncate(number) == number &&
                number >= long.MinValue &&
                number <= long.MaxValue)
            {
                integer = (long)number;
                return true;
            }
            integer = 0;
            return false;
        }
    }
}

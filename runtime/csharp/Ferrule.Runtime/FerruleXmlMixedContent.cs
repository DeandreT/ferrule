using System.Collections.ObjectModel;
using System.Text;

namespace Ferrule.Runtime;

/// <summary>One statically validated direct-element replacement.</summary>
public sealed class FerruleXmlMixedContentReplacement
{
    public FerruleXmlMixedContentReplacement(
        string element,
        IEnumerable<string> collection,
        Func<ScopeContext, FerruleValue> expression)
    {
        if (string.IsNullOrEmpty(element))
        {
            throw new ArgumentException("Element name cannot be empty.", nameof(element));
        }
        ArgumentNullException.ThrowIfNull(collection);
        ArgumentNullException.ThrowIfNull(expression);
        Element = element;
        Collection = new ReadOnlyCollection<string>(collection.ToArray());
        Expression = expression;
    }

    public string Element { get; }

    public IReadOnlyList<string> Collection { get; }

    public Func<ScopeContext, FerruleValue> Expression { get; }
}

/// <summary>One direct-child rename retained in a constructed mixed-content stream.</summary>
public sealed class FerruleXmlMixedContentElement
{
    public FerruleXmlMixedContentElement(string source, string target)
    {
        if (string.IsNullOrEmpty(source))
        {
            throw new ArgumentException("Source element name cannot be empty.", nameof(source));
        }
        if (string.IsNullOrEmpty(target))
        {
            throw new ArgumentException("Target element name cannot be empty.", nameof(target));
        }
        Source = source;
        Target = target;
    }

    public string Source { get; }

    public string Target { get; }
}

/// <summary>Engine-compatible ordered XML mixed-content atomization.</summary>
public static class FerruleXmlMixedContent
{
    private const string MixedContentField = "\u001fferrule-xml-mixed-content";
    private const string MixedValueField = "\u001fferrule-xml-mixed-value";
    private const string NodeNameField = "NodeName";
    private const string TextField = "#text";

    public static FerruleValue Evaluate(
        ScopeContext context,
        IReadOnlyList<string>? frame,
        IReadOnlyList<string> path,
        IReadOnlyList<FerruleXmlMixedContentReplacement> replacements)
    {
        ArgumentNullException.ThrowIfNull(context);
        ArgumentNullException.ThrowIfNull(path);
        ArgumentNullException.ThrowIfNull(replacements);
        var source = context.ResolveXmlInstance(frame, path);
        if (source is not FerruleGroup group)
        {
            return FerruleValue.Null;
        }
        if (!group.TryGetField(MixedContentField, out var retained) ||
            retained is not FerruleRepeated items)
        {
            return group.TryGetField(TextField, out var text) &&
                text is FerruleScalar scalar
                    ? scalar.Value
                    : FerruleValue.Null;
        }

        var output = new StringBuilder();
        var occurrences = new Dictionary<string, int>(StringComparer.Ordinal);
        foreach (var item in items.Items)
        {
            var name = StringField(item, NodeNameField);
            var text = StringField(item, TextField);
            var replacement = replacements.FirstOrDefault(rule =>
                string.Equals(rule.Element, name, StringComparison.Ordinal));
            if (replacement is null)
            {
                output.Append(text);
                continue;
            }

            var expressionContext = context;
            if (replacement.Collection.Count != 0 &&
                item is FerruleGroup itemGroup &&
                itemGroup.TryGetField(MixedValueField, out var value))
            {
                occurrences.TryGetValue(name, out var occurrence);
                occurrence++;
                occurrences[name] = occurrence;
                expressionContext = context.WithXmlMixedContentValue(
                    value,
                    replacement.Collection,
                    occurrence);
            }
            Append(output, replacement.Expression(expressionContext));
        }
        return FerruleValue.FromString(output.ToString());
    }

    public static FerruleGroup Preserve(
        ScopeContext context,
        FerruleGroup output,
        IReadOnlyList<FerruleXmlMixedContentElement> elements)
    {
        ArgumentNullException.ThrowIfNull(context);
        ArgumentNullException.ThrowIfNull(output);
        ArgumentNullException.ThrowIfNull(elements);
        if (context.Frames.Count == 0 ||
            !TrySourceField(context.Frames[^1], MixedContentField, out var retained) ||
            retained is not FerruleRepeated sourceItems)
        {
            return output;
        }

        var items = new List<FerruleInstance>();
        var occurrences = new Dictionary<string, int>(StringComparer.Ordinal);
        foreach (var item in sourceItems.Items)
        {
            var name = OptionalStringField(item, NodeNameField);
            if (name is null)
            {
                continue;
            }
            if (name.Length == 0)
            {
                items.Add(item);
                continue;
            }
            var element = elements.FirstOrDefault(candidate =>
                string.Equals(candidate.Source, name, StringComparison.Ordinal));
            if (element is null ||
                !output.TryGetField(element.Target, out var field) ||
                field is not FerruleRepeated values)
            {
                continue;
            }
            occurrences.TryGetValue(element.Target, out var occurrence);
            if (occurrence >= values.Items.Count)
            {
                continue;
            }
            var value = values.Items[occurrence];
            occurrences[element.Target] = occurrence + 1;
            var text = value is FerruleScalar scalar
                ? FerruleFunctions.ScalarText(scalar.Value)
                : string.Empty;
            items.Add(new FerruleGroup([
                new(NodeNameField, new FerruleScalar(FerruleValue.FromString(element.Target))),
                new(TextField, new FerruleScalar(FerruleValue.FromString(text))),
                new(MixedValueField, value),
            ]));
        }
        if (items.Count == 0)
        {
            return output;
        }

        var fields = new List<FerruleField>(output.Fields)
        {
            new(MixedContentField, new FerruleRepeated(items)),
        };
        return new FerruleGroup(fields);
    }

    private static bool TrySourceField(
        FerruleInstance source,
        string name,
        out FerruleInstance? value)
    {
        if (source is FerruleGroup group)
        {
            return group.TryGetField(name, out value);
        }
        if (source is FerruleDocumentSet { Documents.Count: > 0 } documents)
        {
            return TrySourceField(documents.Documents[0].Value, name, out value);
        }
        value = null;
        return false;
    }

    private static string StringField(FerruleInstance item, string name) =>
        OptionalStringField(item, name) ?? string.Empty;

    private static string? OptionalStringField(FerruleInstance item, string name) =>
        item is FerruleGroup group &&
        group.TryGetField(name, out var field) &&
        field is FerruleScalar { Value.Kind: FerruleValueKind.String } scalar
            ? scalar.Value.StringValue
            : null;

    private static void Append(StringBuilder output, FerruleValue value)
    {
        switch (value.Kind)
        {
            case FerruleValueKind.Null:
            case FerruleValueKind.JsonNull:
            case FerruleValueKind.XmlNil:
                return;
            case FerruleValueKind.Bool:
                output.Append(value.BooleanValue ? "true" : "false");
                return;
            case FerruleValueKind.Int64:
            case FerruleValueKind.Double:
            case FerruleValueKind.String:
                output.Append(FerruleFunctions.ScalarText(value));
                return;
            default:
                throw new InvalidOperationException($"Unsupported scalar kind {value.Kind}.");
        }
    }
}

public sealed partial class ScopeContext
{
    internal ScopeContext WithXmlMixedContentValue(
        FerruleInstance value,
        IReadOnlyList<string> collection,
        int index)
    {
        ArgumentNullException.ThrowIfNull(value);
        ArgumentNullException.ThrowIfNull(collection);
        if (index <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(index));
        }
        ValidatePath(collection);
        var frames = new List<FerruleInstance>(_frames) { value };
        var collections = new List<CollectionIdentity>(_collections)
        {
            new(collection.ToArray(), value, index),
        };
        return new ScopeContext(
            new ReadOnlyCollection<FerruleInstance>(frames),
            new ReadOnlyCollection<CollectionIdentity>(collections),
            _executionContext);
    }
}

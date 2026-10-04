using System.Globalization;

namespace Ferrule.Runtime;

public static partial class FerruleXml
{
    private static bool StructuredElementMatches(StructuredSchema schema, StructuredDom node) =>
        node.Kind == StructuredDomKind.Element && node.Name == schema.Name &&
        (!schema.NamespaceIsExplicit || node.Namespace == (schema.NamespaceUri ?? ""));

    private static string? StructuredAttributeValue(StructuredSchema schema, StructuredDom node,
        StructuredBudget? budget = null)
    {
        budget?.Work(node.Attributes.Count);
        var ns = schema.NamespaceIsExplicit ? schema.NamespaceUri ?? "" : "";
        foreach (var attribute in node.Attributes)
            if (attribute.Name == schema.Name &&
                (!schema.NamespaceIsExplicit || attribute.Namespace == ns)) return attribute.Value;
        return null;
    }

    private static bool StructuredIsNil(StructuredSchema schema, StructuredDom node,
        StructuredBudget? budget = null)
    {
        string? value = null;
        foreach (var attribute in node.Attributes)
            if (attribute.Name == "nil" && attribute.Namespace == XsiNamespace) { value = attribute.Value; break; }
        if (value is null or "false" or "0") return false;
        if (value is not ("true" or "1")) throw StructuredInput("invalid XML nil annotation");
        budget?.Work(node.Children.Count);
        if (!schema.Nillable || schema.ScalarType is null)
            throw StructuredInput("XML nil is not allowed for this element");
        foreach (var child in node.Children)
        {
            // Native Node.text() also exposes comment bodies; PI has no text.
            if (child.Kind == StructuredDomKind.Element ||
                (child.Kind is StructuredDomKind.Text or StructuredDomKind.Comment) &&
                child.TextParts.Any(part => !string.IsNullOrWhiteSpace(part)))
                throw StructuredInput("XML nil element contains content");
        }
        return true;
    }

    private static IReadOnlyList<string> StructuredNativeText(StructuredDom node) =>
        node.Children.Count != 0 && node.Children[0].Kind == StructuredDomKind.Text
            ? node.Children[0].TextParts : Array.Empty<string>();

    private static IEnumerable<string> StructuredDirectText(StructuredDom node, StructuredBudget? budget = null)
    {
        foreach (var child in node.Children)
        {
            budget?.Work();
            if (child.Kind != StructuredDomKind.Text) continue;
            // Apply the indentation rule to a whole coalesced native text node,
            // not separately to individual CDATA/reference parser events.
            if (child.TextParts.All(string.IsNullOrWhiteSpace) &&
                child.TextParts.Any(part => part.Contains('\n') || part.Contains('\r'))) continue;
            foreach (var part in child.TextParts) yield return part;
        }
    }

    private static IEnumerable<StructuredDom> StructuredElements(StructuredSchema schema,
        StructuredDom parent, StructuredBudget? budget = null)
    {
        foreach (var child in parent.Children)
        {
            budget?.Work();
            if (StructuredElementMatches(schema, child)) yield return child;
        }
    }

    private static void PreflightStructuredNode(StructuredSchema schema, StructuredDom node, StructuredBudget budget)
    {
        budget.Node();
        budget.Work(node.Attributes.Count);
        var nil = StructuredIsNil(schema, node, budget);
        if (schema.ScalarType is not null)
        {
            foreach (var child in node.Children)
            {
                budget.Work();
                if (child.Kind == StructuredDomKind.Element)
                    throw StructuredInput("scalar XML element contains child elements");
            }
            if (!nil) PreflightStructuredScalar(schema, StructuredNativeText(node), budget);
            return;
        }
        if (schema.Children.Any(child => child.Text))
        {
            foreach (var child in node.Children)
            {
                budget.Work();
                if (child.Kind == StructuredDomKind.Element)
                    throw StructuredInput("simple-content XML group contains element children");
            }
        }
        foreach (var field in schema.Children)
        {
            budget.Work();
            if (field.Attribute)
            {
                var value = StructuredAttributeValue(field, node, budget);
                budget.Bytes(InputUtf8.GetByteCount(field.Name));
                budget.Node(); // Missing attributes still emit Scalar(Null).
                if (value is not null) PreflightStructuredScalar(field, new[] { value }, budget);
            }
            else if (field.Text)
            {
                budget.Bytes(InputUtf8.GetByteCount(field.Name));
                budget.Node();
                // Charge all direct-child inspections even for numeric #text.
                // Scalar lexical validation follows the complete result budget.
                PreflightStructuredScalar(field, StructuredDirectText(node, budget), budget,
                    temporarySimpleContent: true);
            }
            else if (field.Repeating)
            {
                budget.Bytes(InputUtf8.GetByteCount(field.Name));
                budget.Node(); // Repeated container, including zero occurrences.
                foreach (var child in StructuredElements(field, node, budget))
                    PreflightStructuredNode(field, child, budget);
            }
            else
            {
                var child = StructuredElements(field, node, budget).FirstOrDefault();
                if (child is not null)
                {
                    budget.Bytes(InputUtf8.GetByteCount(field.Name));
                    PreflightStructuredNode(field, child, budget);
                }
                else if (field.ScalarType is not null)
                {
                    budget.Bytes(InputUtf8.GetByteCount(field.Name));
                    budget.Node(); // Missing scalar is Null; missing Group omitted.
                }
            }
        }
    }

    private static void PreflightStructuredScalar(StructuredSchema schema, IEnumerable<string> parts,
        StructuredBudget budget, bool temporarySimpleContent = false)
    {
        if (schema.ScalarType == StructuredScalarType.String || temporarySimpleContent)
            foreach (var part in parts) budget.Bytes(InputUtf8.GetByteCount(part));
        else foreach (var _ in parts) { }
    }

    private static (long Integer, double Number, bool Boolean) ParseStructuredNumeric(StructuredSchema schema, string raw)
    {
        var lexical = raw.Trim();
        switch (schema.ScalarType)
        {
            case StructuredScalarType.Int:
                // Unlike writer adaptation, XML input rejects decimal/exponent
                // lexical forms for an Int even when they denote an integer.
                if (long.TryParse(lexical, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var integer))
                    return (integer, 0, false);
                break;
            case StructuredScalarType.Float:
                if (double.TryParse(lexical, NumberStyles.Float, CultureInfo.InvariantCulture, out var number) && double.IsFinite(number))
                    return (0, number, false);
                break;
            case StructuredScalarType.Bool:
                if (lexical is "true" or "1") return (0, 0, true);
                if (lexical is "false" or "0") return (0, 0, false);
                break;
        }
        throw StructuredInput($"XML scalar '{schema.Name}' has an invalid lexical value");
    }

    private static FerruleValue MaterializeStructuredScalar(StructuredSchema schema, IEnumerable<string> parts)
    {
        var lexical = string.Concat(parts);
        if (schema.ScalarType == StructuredScalarType.String) return FerruleValue.FromString(lexical);
        var number = ParseStructuredNumeric(schema, lexical);
        return schema.ScalarType switch
        {
            StructuredScalarType.Int => FerruleValue.FromInt64(number.Integer),
            StructuredScalarType.Float => FerruleValue.FromDouble(number.Number),
            StructuredScalarType.Bool => FerruleValue.FromBoolean(number.Boolean),
            _ => throw StructuredInput("unsupported XML scalar"),
        };
    }

    private static FerruleInstance MaterializeStructuredNode(StructuredSchema schema, StructuredDom node)
    {
        if (StructuredIsNil(schema, node)) return new FerruleScalar(FerruleValue.XmlNil);
        if (schema.ScalarType is not null)
            return new FerruleScalar(MaterializeStructuredScalar(schema, StructuredNativeText(node)));
        return FerruleGroup.FromSchemaFields(MaterializeStructuredFields(schema, node));
    }

    private static IEnumerable<FerruleField> MaterializeStructuredFields(StructuredSchema schema, StructuredDom node)
    {
        foreach (var field in schema.Children)
        {
            if (field.Attribute)
            {
                var value = StructuredAttributeValue(field, node);
                yield return new FerruleField(field.Name, new FerruleScalar(value is null
                    ? FerruleValue.Null : MaterializeStructuredScalar(field, new[] { value })));
            }
            else if (field.Text)
                yield return new FerruleField(field.Name, new FerruleScalar(
                    MaterializeStructuredScalar(field, StructuredDirectText(node))));
            else if (field.Repeating)
                yield return new FerruleField(field.Name, new FerruleRepeated(StructuredElements(field, node)
                    .Select(child => MaterializeStructuredNode(field, child))));
            else
            {
                var child = StructuredElements(field, node).FirstOrDefault();
                if (child is not null) yield return new FerruleField(field.Name, MaterializeStructuredNode(field, child));
                else if (field.ScalarType is not null)
                    yield return new FerruleField(field.Name, new FerruleScalar(FerruleValue.Null));
            }
        }
    }
}

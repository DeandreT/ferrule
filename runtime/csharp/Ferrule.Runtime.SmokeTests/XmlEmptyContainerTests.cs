using Ferrule.Runtime;
using System.Text.Json;
using System.Xml.Linq;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static string EmptyContainerSchema(string child, bool textParent) =>
        "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[" +
        (textParent ? "{\"name\":\"#text\",\"text\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}," : "") +
        child + "]}}";

    private static void XmlEmptyContainerShapeErrors()
    {
        foreach (var (child, value, expected) in new (string, FerruleInstance, string)[]
        {
            ("""{"name":"Child","kind":{"kind":"group","children":[]}}""", new FerruleRepeated([]), "expected one element"),
            ("""{"name":"Child","repeating":true,"kind":{"kind":"group","children":[]}}""", new FerruleMappedSequence([]), "expected one non-repeating element group"),
            ("""{"name":"Child","kind":{"kind":"scalar","ty":"string"}}""", new FerruleMappedSequence([]), "expected one non-repeating element group"),
            ("""{"name":"Child","kind":{"kind":"scalar","ty":"string"}}""", new FerruleRepeated([]), "expected one element"),
        })
        {
            foreach (var textParent in new[] { false, true })
            {
                var schema = EmptyContainerSchema(child, textParent);
                var instance = textParent
                    ? Group(Field("#text", Scalar(Text(""))), Field("Child", value))
                    : Group(Field("Child", value));
                foreach (var nested in new[] { false, true })
                {
                    var actualSchema = nested
                        ? "{\"name\":\"Outer\",\"kind\":{\"kind\":\"group\",\"children\":[" + schema + "]}}"
                        : schema;
                    var actualInstance = nested ? Group(Field("Root", instance)) : instance;
                    foreach (var indent in new[] { false, true })
                    {
                        var error = Error(FerruleRuntimeError.XmlSerialization, () =>
                            FerruleXml.SerializeEmbedded(81, actualSchema, actualInstance, false, indent, null));
                        Equal((uint?)81, error.Node);
                        Equal(true, error.Detail!.Contains("element 'Child'", StringComparison.Ordinal));
                        Equal(true, error.Detail.Contains(expected, StringComparison.Ordinal));
                    }
                }
            }
        }
        foreach (var container in new FerruleInstance[] { new FerruleRepeated([]), new FerruleMappedSequence([]) })
        {
            const string schema = """{"name":"Root","kind":{"kind":"group","children":[{"name":"#text","text":true,"kind":{"kind":"scalar","ty":"string"}}]}}""";
            foreach (var indent in new[] { false, true })
            {
                var error = Error(FerruleRuntimeError.XmlSerialization, () =>
                    FerruleXml.SerializeEmbedded(82, schema, Group(Field("#text", container)), false, indent, null));
                Equal((uint?)82, error.Node);
                Equal(true, error.Detail!.Contains("expected a text scalar", StringComparison.Ordinal));
            }
        }
    }

    private static void XmlEmptyContainerLegalCollections()
    {
        foreach (var (child, value, childCount) in new (string, FerruleInstance, int)[]
        {
            ("""{"name":"Child","kind":{"kind":"group","children":[]}}""", new FerruleMappedSequence([]), 0),
            ("""{"name":"Child","repeating":true,"kind":{"kind":"group","children":[]}}""", new FerruleRepeated([]), 0),
            ("""{"name":"Child","repeating":true,"kind":{"kind":"scalar","ty":"string"}}""", new FerruleRepeated([]), 0),
            ("""{"name":"Child","kind":{"kind":"scalar","ty":"string"}}""", Scalar(FerruleValue.Null), 0),
            ("""{"name":"Child","kind":{"kind":"group","children":[]}}""", Group(), 1),
            ("""{"name":"Child","repeating":true,"kind":{"kind":"group","children":[]}}""", new FerruleRepeated([Group()]), 1),
        })
        {
            foreach (var textParent in new[] { false, true })
            {
                var schema = EmptyContainerSchema(child, textParent);
                var instance = textParent
                    ? Group(Field("#text", Scalar(Text(""))), Field("Child", value))
                    : Group(Field("Child", value));
                foreach (var indent in new[] { false, true })
                {
                    var xml = FerruleXml.SerializeEmbedded(83, schema, instance, false, indent, null).StringValue;
                    Equal(childCount, XElement.Parse(xml).Elements("Child").Count());
                }
            }
        }
    }

    private static void XmlEmptyContainerRecursiveRoles()
    {
        const string recursive = """{"name":"Root","kind":{"kind":"group","children":[{"name":"Child","repeating":true,"recursive_ref":"Root","kind":{"kind":"group","children":[]}}]}}""";
        foreach (var indent in new[] { false, true })
        {
            FerruleXml.SerializeEmbedded(84, recursive, RecursiveContainerChain(63), false, indent, null);
            var limit = Error(FerruleRuntimeError.XmlSerialization, () =>
                FerruleXml.SerializeEmbedded(84, recursive, RecursiveContainerChain(64), false, indent, null));
            Equal((uint?)84, limit.Node);
            Equal("XML recursion exceeds 64 groups", limit.Detail);
        }
        foreach (var textParent in new[] { false, true })
        {
            foreach (var indent in new[] { false, true })
            {
                foreach (var (repeating, value) in new (bool, FerruleInstance)[]
                {
                    (true, new FerruleRepeated([])), (false, new FerruleMappedSequence([])),
                })
                {
                    var child = "{\"name\":\"Child\",\"recursive_ref\":\"Root\",\"repeating\":" +
                        JsonSerializer.Serialize(repeating) + ",\"kind\":{\"kind\":\"group\",\"children\":[]}}";
                    var schema = EmptyContainerSchema(child, textParent);
                    var instance = textParent
                        ? Group(Field("#text", Scalar(Text(""))), Field("Child", value))
                        : Group(Field("Child", value));
                    Equal(0, XElement.Parse(FerruleXml.SerializeEmbedded(84, schema, instance, false, indent, null).StringValue).Elements().Count());
                    var malformed = schema.Replace("\"recursive_ref\":\"Root\"", "\"recursive_ref\":\"Missing\"", StringComparison.Ordinal);
                    var error = Error(FerruleRuntimeError.XmlSerialization, () =>
                        FerruleXml.SerializeEmbedded(84, malformed, instance, false, indent, null));
                    Equal((uint?)84, error.Node);
                    Equal("XML recursive schema anchor 'Missing' does not exist", error.Detail);
                }
                var invalidChild = """{"name":"Child","repeating":true,"recursive_ref":"Root","kind":{"kind":"group","children":[]}}""";
                var invalidSchema = EmptyContainerSchema(invalidChild, textParent);
                var invalidInstance = textParent
                    ? Group(Field("#text", Scalar(Text(""))), Field("Child", new FerruleMappedSequence([])))
                    : Group(Field("Child", new FerruleMappedSequence([])));
                Error(FerruleRuntimeError.XmlSerialization, () =>
                    FerruleXml.SerializeEmbedded(84, invalidSchema, invalidInstance, false, indent, null));
            }
        }
    }

    private static FerruleGroup RecursiveContainerChain(int depth)
    {
        var group = Group(Field("Child", new FerruleRepeated([])));
        for (var index = 1; index < depth; index++)
        {
            group = Group(Field("Child", new FerruleRepeated([group])));
        }
        return group;
    }

    private static void XmlEmptyContainerOrderedContent()
    {
        const string schema = """{"name":"Root","xml_repeating_choices":[{"required":false,"repeating":true,"members":["Code","Amount"]}],"kind":{"kind":"group","children":[{"name":"#text","text":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Code","repeating":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Amount","repeating":true,"kind":{"kind":"scalar","ty":"string"}}]}}""";
        foreach (var indent in new[] { false, true })
        {
            foreach (var malformed in new FerruleInstance[] { Scalar(Text("")), Group(), new FerruleMappedSequence([]) })
            {
                var error = Error(FerruleRuntimeError.XmlSerialization, () => FerruleXml.SerializeEmbedded(
                    85, schema, Group(Field("#text", Scalar(Text(""))), Field("\u001fferrule-xml-mixed-content", malformed)), false, indent, null));
                Equal((uint?)85, error.Node);
                Equal("group 'Root' ordered XML content must be repeated", error.Detail);
            }
            var empty = Group(Field("#text", Scalar(Text(""))), Field("\u001fferrule-xml-mixed-content", new FerruleRepeated([])));
            Equal(0, XElement.Parse(FerruleXml.SerializeEmbedded(85, schema, empty, false, indent, null).StringValue).Elements().Count());
            var ordered = new FerruleRepeated([
                Group(Field("NodeName", Scalar(Text("Amount"))), Field("\u001fferrule-xml-mixed-value", Scalar(Text("2")))),
                Group(Field("NodeName", Scalar(Text("Code"))), Field("\u001fferrule-xml-mixed-value", Scalar(Text("a")))),
            ]);
            var populated = Group(Field("#text", Scalar(Text(""))), Field("\u001fferrule-xml-mixed-content", ordered));
            Equal("Amount,Code", string.Join(',', XElement.Parse(FerruleXml.SerializeEmbedded(85, schema, populated, false, indent, null).StringValue).Elements().Select(e => e.Name.LocalName)));
        }
    }
}

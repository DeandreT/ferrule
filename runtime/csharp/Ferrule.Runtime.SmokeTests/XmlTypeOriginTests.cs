using Ferrule.Runtime;
using System.Text.Json;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string TypeOriginField = "\u001fferrule-xml-type-origin";

    private static void XmlTypeOriginSelectionAndPrivacy()
    {
        foreach (var indent in new[] { false, true })
        foreach (var mode in new[] { 0, 1, 2 })
        foreach (var origin in new[] { FerruleXmlTypeOrigin.Absent, FerruleXmlTypeOrigin.Explicit("A"), FerruleXmlTypeOrigin.Explicit("B") })
        {
            var legacy = Group(Field("Value", Scalar(Text("x"))), Field("Extra", Scalar(Text("e"))));
            var annotated = legacy.WithXmlTypeOrigin(origin);
            Equal(DeclaredDefaultSerialize(DeclaredDefaultSchema, legacy, indent, mode), DeclaredDefaultSerialize(DeclaredDefaultSchema, annotated, indent, mode));
            var copy = ScopeContext.FromSource(annotated).CopyCurrentGroup();
            Equal(origin.Kind, copy.XmlTypeOrigin.Kind);
            Equal(origin.Identity, copy.XmlTypeOrigin.Identity);
            Equal(FerruleXmlTypeOriginKind.Unknown,legacy.XmlTypeOrigin.Kind);
        }
        const string jsonSchema = """{"name":"Root","json_property_count":{"minimum":1,"maximum":1},"kind":{"kind":"group","children":[],"dynamic":{"name":"value","kind":{"kind":"scalar","ty":"string"}}}}""";
        var value = Group(Field("Code",Scalar(Text("a")))).WithXmlTypeOrigin(FerruleXmlTypeOrigin.Explicit("{urn:types}B"));
        using var output = JsonDocument.Parse(FerruleJson.Serialize(jsonSchema,value));
        Equal(1,output.RootElement.EnumerateObject().Count());
        Equal("a",output.RootElement.GetProperty("Code").GetString());
        Equal(false,output.RootElement.TryGetProperty(TypeOriginField,out _));
        var item = Group(Field("Code",Scalar(Text("a")))).WithXmlTypeOrigin(FerruleXmlTypeOrigin.Explicit("B"));
        var source = Group(Field("file",new FerruleRepeated([item]))).WithXmlTypeOrigin(FerruleXmlTypeOrigin.Absent);
        var projected = (FerruleGroup)FerruleRecursiveFilter.Apply(ScopeContext.FromSource(source),"directory","file",1,_=>Bool(true));
        Equal(FerruleXmlTypeOriginKind.Unknown,projected.XmlTypeOrigin.Kind);
        Equal(true,projected.TryGetField("file",out var retained));
        var retainedItem = (FerruleGroup)((FerruleRepeated)retained!).Items[0];
        Equal(FerruleXmlTypeOriginKind.Explicit,retainedItem.XmlTypeOrigin.Kind);
        Equal("B",retainedItem.XmlTypeOrigin.Identity);
    }

    private static void XmlTypeOriginMalformedPayloads()
    {
        try { _ = FerruleXmlTypeOrigin.Explicit(string.Empty); throw new Exception("empty origin accepted"); }
        catch (ArgumentException) { }
        const string dynamicSchema = """{"name":"Root","kind":{"kind":"group","children":[],"dynamic":{"name":"value","kind":{"kind":"scalar","ty":"string"}}}}""";
        var physicalSchema = """{"name":"Root","kind":{"kind":"group","children":[{"name":"\u001fferrule-xml-type-origin","kind":{"kind":"scalar","ty":"string"}}]}}""";
        var input = JsonSerializer.Serialize(new Dictionary<string,string> { [TypeOriginField] = "ordinary JSON value" });
        foreach (var schema in new[] { dynamicSchema,physicalSchema })
        {
            var value = (FerruleGroup)FerruleJson.Parse(schema,input);
            Equal(FerruleXmlTypeOriginKind.Unknown,value.XmlTypeOrigin.Kind);
            Equal(true,value.TryGetField(TypeOriginField,out var data));
            Equal("ordinary JSON value",((FerruleScalar)data!).Value.StringValue);
            using var output = JsonDocument.Parse(FerruleJson.Serialize(schema,value));
            Equal("ordinary JSON value",output.RootElement.GetProperty(TypeOriginField).GetString());
            Equal(1,output.RootElement.EnumerateObject().Count());
            Equal(FerruleXmlTypeOriginKind.Unknown,((FerruleGroup)FerruleJson.ParseBytes(schema,System.Text.Encoding.UTF8.GetBytes(input))).XmlTypeOrigin.Kind);
        }
    }
    private static void XmlTypeOriginOrderedWrapperOwnership()
    {
        const string ordered = "\u001fferrule-xml-mixed-content";
        const string valueField = "\u001fferrule-xml-mixed-value";
        var item = Group(Field("Code", Scalar(Text("a")))).WithXmlTypeOrigin(FerruleXmlTypeOrigin.Explicit("item"));
        var unrelated = Group(Field("NodeName",Scalar(Text("unrelated"))),
            Field(valueField,Group().WithXmlTypeOrigin(FerruleXmlTypeOrigin.Absent)))
            .WithXmlTypeOrigin(FerruleXmlTypeOrigin.Explicit("unchanged-wrapper"));
        var replaced = Group(Field("NodeName",Scalar(Text("file"))),Field(valueField,item))
            .WithXmlTypeOrigin(FerruleXmlTypeOrigin.Explicit("replaced-wrapper"));
        var source = Group(Field("file",new FerruleRepeated([item])),Field(ordered,new FerruleRepeated([unrelated,replaced])))
            .WithXmlTypeOrigin(FerruleXmlTypeOrigin.Explicit("parent"));
        var output = (FerruleGroup)FerruleRecursiveFilter.Apply(ScopeContext.FromSource(source),"directory","file",1,_=>Bool(true));
        Equal(FerruleXmlTypeOriginKind.Unknown,output.XmlTypeOrigin.Kind);
        Equal(true,output.TryGetField(ordered,out var stream));
        var entries = ((FerruleRepeated)stream!).Items;
        var unchanged = (FerruleGroup)entries[0];
        Equal(FerruleXmlTypeOriginKind.Explicit,unchanged.XmlTypeOrigin.Kind);
        Equal("unchanged-wrapper",unchanged.XmlTypeOrigin.Identity);
        Equal(true,unchanged.TryGetField(valueField,out var unchangedValue));
        Equal(FerruleXmlTypeOriginKind.Absent,((FerruleGroup)unchangedValue!).XmlTypeOrigin.Kind);
        var changed = (FerruleGroup)entries[1];
        Equal(FerruleXmlTypeOriginKind.Unknown,changed.XmlTypeOrigin.Kind);
        Equal(true,changed.TryGetField(valueField,out var child));
        Equal("item",((FerruleGroup)child!).XmlTypeOrigin.Identity);
        Equal("replaced-wrapper",replaced.XmlTypeOrigin.Identity);
        Equal("parent",source.XmlTypeOrigin.Identity);
    }

    private static void XmlTypeOriginMixedNoopsAndAttachment()
    {
        const string ordered = "\u001fferrule-xml-mixed-content";
        const string valueField = "\u001fferrule-xml-mixed-value";
        var child = Group(Field("Code",Scalar(Text("a")))).WithXmlTypeOrigin(FerruleXmlTypeOrigin.Explicit("child"));
        var output = Group(Field("file",new FerruleRepeated([child]))).WithXmlTypeOrigin(FerruleXmlTypeOrigin.Explicit("output"));
        var noStream = Group().WithXmlTypeOrigin(FerruleXmlTypeOrigin.Absent);
        Equal(true,ReferenceEquals(output,FerruleXmlMixedContent.Preserve(ScopeContext.FromSource(noStream),output,[])));
        var source = Group(Field(ordered,new FerruleRepeated([Group(Field("NodeName",Scalar(Text("file"))),
            Field("Text",Scalar(Text(""))),Field(valueField,child))]))).WithXmlTypeOrigin(FerruleXmlTypeOrigin.Absent);
        Equal(true,ReferenceEquals(output,FerruleXmlMixedContent.Preserve(ScopeContext.FromSource(source),output,[])));
        var attached = FerruleXmlMixedContent.Preserve(ScopeContext.FromSource(source),output,[new("file","file")]);
        Equal(FerruleXmlTypeOriginKind.Unknown,attached.XmlTypeOrigin.Kind);
        Equal(true,attached.TryGetField(ordered,out var stream));
        var entry = (FerruleGroup)((FerruleRepeated)stream!).Items[0];
        Equal(FerruleXmlTypeOriginKind.Unknown,entry.XmlTypeOrigin.Kind);
        Equal(true,entry.TryGetField(valueField,out var retained));
        Equal("child",((FerruleGroup)retained!).XmlTypeOrigin.Identity);
        Equal("output",output.XmlTypeOrigin.Identity);
    }

}

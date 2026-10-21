using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void SourceOriginIdentityAndBounds()
    {
        var first = new FerruleSourceDocumentOrigin(" ./same.csv ", "C:/resolved/same.csv");
        var second = new FerruleSourceDocumentOrigin(" ./same.csv ", "C:/resolved/same.csv");
        Equal(" ./same.csv ", first.Path);
        Equal("C:/resolved/same.csv", first.SourcePath);
        Equal("opaque:anywhere", new FerruleSourceDocumentOrigin("opaque:anywhere").SourcePath);
        Equal(true, first.SameDocument(first));
        Equal(false, first.SameDocument(second));
        Equal(false, first.SameDocument(null));
        Equal(true, first.Row(1).SameOccurrence(first.Row(1)));
        Equal(false, first.Row(1).SameOccurrence(first.Row(2)));
        Equal(false, first.Row(1).SameOccurrence(second.Row(1)));
        Equal(false, first.Row(1).SameOccurrence(null));
        Equal(true, first.Row(1).Equals((object)first.Row(1)));
        Equal(first.Row(1).GetHashCode(), first.Row(1).GetHashCode());
        Throws<ArgumentOutOfRangeException>(() => first.Row(0));
        Equal(ulong.MaxValue, first.Row(ulong.MaxValue).Position);
        foreach (var path in new[] { "", "a\0b", "\ud800", new string('x', 4097), new string('é', 2049) })
        {
            Throws<ArgumentException>(() => new FerruleSourceDocumentOrigin(path));
            Throws<ArgumentException>(() => new FerruleSourceDocumentOrigin("valid", path));
        }
        foreach (var path in new[] { new string('x', 4096), new string('é', 2048) })
            Equal(path, new FerruleSourceDocumentOrigin(path, path).SourcePath);
    }

    private static void SourceOriginCopyAndRebuild()
    {
        var document = new FerruleSourceDocumentOrigin("private/source.csv");
        var plain = Group(Field("Value", Scalar(Text("A"))));
        var marked = plain.WithSourceOrigin(document.Row(1));
        Equal(true, plain.SourceOrigin is null);
        Equal(JsonSerializer.Serialize(plain), JsonSerializer.Serialize(marked));
        var copy = ScopeContext.FromSource(marked).CopyCurrentGroup();
        Equal(false, ReferenceEquals(marked, copy));
        Equal(true, copy.SourceOrigin!.SameOccurrence(document.Row(1)));
        var nested = Group(Field("Child", marked)).WithSourceOrigin(document.Row(2));
        var nestedCopy = ScopeContext.FromSource(nested).CopyCurrentGroup();
        Equal(true, nestedCopy.SourceOrigin!.SameOccurrence(document.Row(2)));
        Equal(true, ((FerruleGroup)nestedCopy.Fields[0].Value).SourceOrigin!.SameOccurrence(document.Row(1)));
        var rebuilt = new FerruleGroup(marked.Fields);
        Equal(true, rebuilt.SourceOrigin is null);
        var filtered = (FerruleGroup)FerruleRecursiveFilter.Apply(ScopeContext.FromSource(marked),
            "Children", "Items", 1, _ => FerruleValue.FromBoolean(true));
        Equal(true, filtered.SourceOrigin is null);
        var filteredParent = (FerruleGroup)FerruleRecursiveFilter.Apply(ScopeContext.FromSource(nested),
            "Children", "Items", 1, _ => FerruleValue.FromBoolean(true));
        Equal(true, filteredParent.SourceOrigin is null);
        Equal(true, ((FerruleGroup)filteredParent.Fields[0].Value).SourceOrigin!.SameOccurrence(document.Row(1)));
        var xml = marked.WithXmlTypeOrigin(FerruleXmlTypeOrigin.Absent);
        Equal(true, xml.SourceOrigin!.SameOccurrence(document.Row(1)));
        Equal(FerruleXmlTypeOriginKind.Absent, xml.XmlTypeOrigin.Kind);
        Equal(true, xml.WithXmlTypeOrigin(FerruleXmlTypeOrigin.Unknown).SourceOrigin!.SameOccurrence(document.Row(1)));
    }
}

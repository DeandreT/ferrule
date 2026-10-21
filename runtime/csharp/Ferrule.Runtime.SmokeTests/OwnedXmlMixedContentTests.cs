using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string MixedStream = "\u001fferrule-xml-mixed-content";
    private const string MixedValue = "\u001fferrule-xml-mixed-value";

    private static FerruleGroup MixedItem(string name, string text) => Group(
        Field("NodeName", Scalar(Text(name))),
        Field("#text", Scalar(Text(text))),
        Field(MixedValue, Scalar(Text(text))));

    // `Para` holds `prefix <b>…</b>|` for each bold text.
    private static FerruleGroup MixedInput(string title, string prefix, params string[] bold)
    {
        var stream = new List<FerruleInstance> { MixedItem("", prefix) };
        foreach (var value in bold)
        {
            stream.Add(MixedItem("b", value));
            stream.Add(MixedItem("", "|"));
        }
        return Group(
            Field("Title", Scalar(Text(title))),
            Field("Para", Group(
                Field("b", new FerruleRepeated(bold.Select(value => (FerruleInstance)Scalar(Text(value))))),
                Field(MixedStream, new FerruleRepeated(stream)))));
    }

    private static void OwnedXmlMixedContentExactOwners()
    {
        var primary = MixedInput("P", "p ", "x", "y");
        var alias = MixedInput("A", "a ", "z");
        var declared = new[] { new FerruleDeclaredNamedSourceRoot("Para", alias) };
        var context = FerruleBasicSourceRoots.Create(primary, declared, BasicLimits).RootContext();
        var para = new[] { "Para" };
        var bold = new[] { "Para", "b" };
        var occurrence = new FerruleOwnedXmlMixedContentReplacement("b", true, item =>
            Text($"[{item.OwnedPosition(FerruleOwnedSourceOwner.Primary, bold)}:" +
                $"{item.ResolveOwnedField(FerruleOwnedSourceOwner.Primary, bold).StringValue}]"));
        Equal("p [1:x]|[2:y]|", context.OwnedXmlMixedContent(4, FerruleOwnedSourceOwner.Primary,
            para, null, false, new[] { occurrence }).StringValue);
        // A parent rule reads the receiving context; the alias owner keeps its own group.
        var parent = new FerruleOwnedXmlMixedContentReplacement("b", false,
            item => item.ResolveOwnedField(FerruleOwnedSourceOwner.Named("Para"), new[] { "Title" }));
        Equal("a A|", context.OwnedXmlMixedContent(4, FerruleOwnedSourceOwner.Named("Para"),
            para, null, false, new[] { parent }).StringValue);
        // The primary occurrence never serves another owner's read.
        var foreign = new FerruleOwnedXmlMixedContentReplacement("b", true,
            item => item.ResolveOwnedField(FerruleOwnedSourceOwner.Named("Para"), bold));
        var crossing = Error(FerruleRuntimeError.OwnedSource, () => context.OwnedXmlMixedContent(
            4, FerruleOwnedSourceOwner.Primary, para, null, false, new[] { foreign }));
        Equal(FerruleOwnedSourceErrorKind.UniteratedRepetition, crossing.OwnedSourceFailure!.Error);
        Equal<uint?>(null, crossing.Node);

        // A missing group is attributed to the atomizer and never borrowed.
        var missing = FerruleBasicSourceRoots.Create(Group(Field("Title", Scalar(Text("P")))), declared,
            BasicLimits).RootContext();
        var absent = Error(FerruleRuntimeError.OwnedSource, () => missing.OwnedXmlMixedContent(
            4, FerruleOwnedSourceOwner.Primary, para, null, false,
            Array.Empty<FerruleOwnedXmlMixedContentReplacement>()));
        Equal<uint?>(4, absent.Node);
        Equal(FerruleOwnedSourceTraversalKind.MissingInstance,
            ((FerruleOwnedSourceTraversalException)absent.OwnedSourceFailure!.InnerException!).Error);
        // A group without a retained stream returns its text value.
        var plain = FerruleBasicSourceRoots.Create(Group(Field("Para", Group(Field("#text", Scalar(Text(" t "))))) ),
            Array.Empty<FerruleDeclaredNamedSourceRoot>(), BasicLimits).RootContext();
        Equal(" t ", plain.OwnedXmlMixedContent(4, FerruleOwnedSourceOwner.Primary, para, null, false,
            Array.Empty<FerruleOwnedXmlMixedContentReplacement>()).StringValue);
    }
}

using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void PrimaryRootAnnotationStates()
    {
        foreach (var (origin, expected) in new[] {
            (FerruleXmlTypeOrigin.Absent, false),
            (FerruleXmlTypeOrigin.Explicit("Derived"), true),
            (FerruleXmlTypeOrigin.Explicit("Other"), false),
            (FerruleXmlTypeOrigin.Explicit("{urn:exact}Derived"), false) })
        {
            var root = Group(Field("\u001fferrule-xml-type", Scalar(Text("Derived"))))
                .WithXmlTypeOrigin(origin);
            Equal(Bool(expected), ScopeContext.FromSource(root).ResolveSourceRootXmlTypeEquals(42, "Derived"));
        }
        var exact = Group().WithXmlTypeOrigin(FerruleXmlTypeOrigin.Explicit("{urn:exact}Derived"));
        Equal(true, FerrulePrimaryRoot.XmlTypeEquals(exact, "{urn:exact}Derived"));
    }

    private static FerruleRuntimeException PrimaryRootFailure(FerrulePrimaryRootError expected, Action action)
    {
        var error = Error(FerruleRuntimeError.PrimaryRoot, action);
        Equal<uint?>(42, error.Node);
        Equal(expected, error.PrimaryRoot!.Error);
        return error;
    }

    private static void PrimaryRootUnknownAndIdentityErrors()
    {
        var unknown = ScopeContext.FromSource(Group());
        PrimaryRootFailure(FerrulePrimaryRootError.UnknownXmlTypeOrigin,
            () => unknown.ResolveSourceRootXmlTypeEquals(42, "Derived"));
        var malformed = Group().WithXmlTypeOrigin(FerruleXmlTypeOrigin.Explicit("p:Derived"));
        PrimaryRootFailure(FerrulePrimaryRootError.InvalidTypeIdentity,
            () => FerrulePrimaryRoot.XmlTypeEquals(malformed, "Derived", 42));
        foreach (var identity in new[] { "", "{}Derived", "p:Derived", "{a b}Derived", "9Bad" })
            PrimaryRootFailure(FerrulePrimaryRootError.InvalidTypeIdentity,
                () => unknown.ResolveSourceRootXmlTypeEquals(42, identity));
        PrimaryRootFailure(FerrulePrimaryRootError.MissingOwner,
            () => FerrulePrimaryRoot.XmlTypeEquals(null, "Derived", 42));
    }

    private static void PrimaryRootScalarShapes()
    {
        var root = Group(Field("Null", Scalar(FerruleValue.Null)),
            Field("Child", Group(Field("Code", Scalar(Text("child"))))),
            Field("Rows", new FerruleRepeated([Scalar(Text("first"))])));
        var context = ScopeContext.FromSource(root);
        foreach (var path in new[] { new[] { "Missing" }, ["Missing", "Code"], ["Null"] })
            Equal(FerruleValue.Null, context.ResolveSourceRootField(42, path));
        Equal(Text("child"), context.ResolveSourceRootField(42, ["Child", "Code"]));
        var intermediate = PrimaryRootFailure(FerrulePrimaryRootError.ExpectedGroupAt,
            () => context.ResolveSourceRootField(42, ["Null", "Code"]));
        Equal("Null", string.Join('/', intermediate.PrimaryRoot!.Path));
        Equal("scalar", intermediate.PrimaryRoot.Found);
        var terminal = PrimaryRootFailure(FerrulePrimaryRootError.ExpectedScalar,
            () => context.ResolveSourceRootField(42, ["Rows"]));
        Equal("Rows", string.Join('/', terminal.PrimaryRoot!.Path));
        Equal("repeated", terminal.PrimaryRoot.Found);
        PrimaryRootFailure(FerrulePrimaryRootError.ExpectedScalar,
            () => context.ResolveSourceRootField(42, ["Child"]));
    }

    private static void PrimaryRootNeverUnwrapsOwner()
    {
        var item = Group(Field("Code", Scalar(Text("first"))))
            .WithXmlTypeOrigin(FerruleXmlTypeOrigin.Explicit("Derived"));
        foreach (var (root, found) in new (FerruleInstance, string)[] {
            (Scalar(Text("root")), "scalar"), (new FerruleRepeated([item]), "repeated"),
            (new FerruleDocumentSet([new FerruleDocument("first.xml", item)]), "document set"),
            (new FerruleMappedSequence([item]), "mapped sequence") })
        {
            var context = ScopeContext.FromSource(root);
            Equal(found, PrimaryRootFailure(FerrulePrimaryRootError.ExpectedGroup,
                () => context.ResolveSourceRootField(42, ["Code"])).PrimaryRoot!.Found);
            Equal(found, PrimaryRootFailure(FerrulePrimaryRootError.ExpectedGroup,
                () => context.ResolveSourceRootXmlTypeEquals(42, "Derived")).PrimaryRoot!.Found);
        }
    }

    private static void PrimaryRootDuplicatesAndLimits()
    {
        const string duplicateSchema = """{"name":"Root","kind":{"kind":"group","children":[{"name":"Code","kind":{"kind":"scalar","ty":"string"}},{"name":"Code","kind":{"kind":"scalar","ty":"string"}}]}}""";
        var duplicate = FerruleJson.Parse(duplicateSchema, """{"Code":"first"}""");
        var error = PrimaryRootFailure(FerrulePrimaryRootError.DuplicateField,
            () => FerrulePrimaryRoot.Scalar(duplicate, ["Code"], 42));
        Equal("Code", string.Join('/', error.PrimaryRoot!.Path));
        var oversized = new FerruleGroup(Enumerable.Range(0,4097).Select(i => Field($"F{i}",Scalar(Text("x")))));
        PrimaryRootFailure(FerrulePrimaryRootError.FieldLimit,
            () => FerrulePrimaryRoot.Scalar(oversized, ["Missing"], 42));
        foreach (var path in new[] { Array.Empty<string>(), Enumerable.Repeat("a",17).ToArray(),
            new[] { "\u001fferrule-xml-type" }, new[] { "element()" } })
            PrimaryRootFailure(FerrulePrimaryRootError.InvalidScalarPath,
                () => FerrulePrimaryRoot.Scalar(Group(), path, 42));
    }

    private static void PrimaryRootOwnerCollisions()
    {
        var item = Group(Field("Code",Scalar(Text("item")))).WithXmlTypeOrigin(FerruleXmlTypeOrigin.Explicit("Other"));
        var primary = Group(Field("Code",Scalar(Text("primary"))),Field("Rows",new FerruleRepeated([item])))
            .WithXmlTypeOrigin(FerruleXmlTypeOrigin.Explicit("Derived"));
        var named = Group(Field("Code",Scalar(Text("named")))).WithXmlTypeOrigin(FerruleXmlTypeOrigin.Explicit("Other"));
        var context = ScopeContext.FromSources(primary,[Field("Named",named)]);
        foreach (var nested in context.IterateSource("Rows"))
        {
            Equal(Text("primary"), nested.ResolveSourceRootField(42,["Code"]));
            Equal(Bool(true), nested.ResolveSourceRootXmlTypeEquals(42,"Derived"));
        }
        Equal(FerruleValue.Null, context.ResolveSourceRootField(42,["Named","Code"]));
    }

    private static void PrimaryRootOrdinaryDataAndUnicodeBounds()
    {
        var root = Group(Field(TypeOriginField,Scalar(Text("ordinary"))));
        Equal(Text("ordinary"),FerrulePrimaryRoot.Scalar(root,[TypeOriginField]));
        foreach (var identity in new[] { "Derived", "{urn:types}Derived", "_a", "é", "\U00010000", new string('a',4096) })
            Equal(true,FerrulePrimaryRoot.TypeIdentityIsValid(identity));
        foreach (var identity in new[] { "9a", "a:b", "{}a", "{a\u0085b}a", "{a}a}", "\u037e", "\U000F0000", new string('a',4097), new string('é',2049), "\ud800" })
            Equal(false,FerrulePrimaryRoot.TypeIdentityIsValid(identity));
        Equal(true,FerrulePrimaryRoot.TypeIdentityIsValid(new string('é',2048)));
        Equal(true,FerrulePrimaryRoot.ScalarPathIsValid([new string('é',2048)]));
        Equal(false,FerrulePrimaryRoot.ScalarPathIsValid([new string('é',2049)]));
    }
}

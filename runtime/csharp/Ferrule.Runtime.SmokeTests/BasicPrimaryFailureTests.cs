using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static object PrimaryFailureLimits(FerruleOwnedSourceLimits limits) => new
    {
        roots = (ulong)limits.MaxRoots, frames = (ulong)limits.MaxActiveFrames,
        segments = (ulong)limits.MaxPathSegments, bytes = (ulong)limits.MaxMetadataBytes,
        contexts = (ulong)limits.MaxWalkContexts
    };
    private static Exception PrimaryFailureError(string id, Action subject, Func<object> facts)
    {
        try { subject(); }
        catch (Exception error) { BasicErrorRaw(id, error, facts()); return error; }
        throw new InvalidOperationException("Expected a primary failure refusal.");
    }
    private static FerruleBasicSourceBridgeException PrimaryFailureBridge(Exception error,
        FerruleBasicSourceBridgeErrorKind kind)
    {
        var outer = (FerruleRuntimeException)error;
        Equal(FerruleRuntimeError.BasicSourceBridge, outer.Error);
        var cause = outer.BasicSourceBridgeFailure!;
        Equal(true, ReferenceEquals(cause, outer.InnerException)); Equal(kind, cause.Error);
        Equal<uint?>(null, outer.Node); Equal<FerruleOwnedSourceException?>(null, outer.OwnedSourceFailure);
        Equal("basic source bridge failed", outer.Message); Equal("basic source bridge refused", cause.Message);
        if (kind == FerruleBasicSourceBridgeErrorKind.UnsupportedPrimaryFailureContext)
        {
            Equal<FerruleBasicSourceBridgeComponent?>(null, cause.Component);
            Equal<nuint?>(null, cause.Used); Equal<nuint?>(null, cause.Requested); Equal<nuint?>(null, cause.Maximum);
            Equal<nuint?>(null, cause.InputOrdinal); Equal<nuint?>(null, cause.CandidateOrdinal);
            Equal<nuint?>(null, cause.DeclarationIndex); Equal<nuint?>(null, cause.DeclaredCount);
            Equal<FerruleOwnedSourceInstanceKind?>(null, cause.Found); Equal<Exception?>(null, cause.InnerException);
        }
        return cause;
    }
    private static void BasicPrimaryFailureRows()
    {
        var path = new[] { "Rows" }; var owner = OwnedPrimaryOwner;
        var a = BasicValue("A"); var b = BasicValue("B");
        var absent = Scalar(FerruleValue.Null); var jsonNull = Scalar(FerruleValue.JsonNull);
        var primary = Group(Field("Value", Scalar(Text("root"))), Field("Rows", Repeated(absent, a, jsonNull, b)));
        var root = BasicRoot(primary, new FerruleDeclaredNamedSourceRoot("Alias", primary));
        var reached = new List<FerruleBasicScopeContext>();
        root.VisitPrimaryFailureRows(path, reached.Add);
        OwnedRaw("N809.rows", new { input = BasicInstanceSnapshot(primary), rows = reached.Select(BasicContextSnapshot).ToArray() });
        Equal(4, reached.Count);
        var items = new FerruleInstance[] { absent, a, jsonNull, b };
        for (var i = 0; i < reached.Count; i++)
        {
            var context = reached[i];
            Equal(true, ReferenceEquals(items[i], context.LegacyView.Frames[^1]));
            Equal(true, ReferenceEquals(items[i], context.OwnedView.FrameAt(0).Item));
            Equal(0, context.OriginAt(context.OriginCount - 1).OwnerKey);
            OwnedPath(path, context.OriginAt(context.OriginCount - 1).Canonical);
            Equal<nuint>((nuint)i + 1, context.OwnedPosition(owner));
            var empty = BasicOwnedError("N809.rows.empty-is-exact", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
                () => context.OwnedPosition(owner, Array.Empty<string>()));
            OwnedPath(Array.Empty<string>(), empty.Collection);
            Equal<nuint>((nuint)i + 1, context.OwnedPosition(owner, path));
            Equal((long)i + 1, context.LegacyPosition(path));
            Equal(Text("root"), context.ResolveOwnedFieldWithRootPin(OwnedNamedOwner("Alias"), new[] { "Value" }, Array.Empty<string>(), true));
            Equal(i == 1 ? Text("A") : i == 3 ? Text("B") : FerruleValue.Null,
                context.ResolveOwnedField(owner, new[] { "Rows", "Value" }, path));
        }
        var singleton = BasicRoot(Group(Field("Rows", BasicValue("single"))));
        var plain = new List<FerruleBasicScopeContext>(); singleton.VisitPrimaryFailureRows(path, plain.Add);
        OwnedRaw("N809.singleton", new { rows = plain.Select(BasicContextSnapshot).ToArray() });
        Equal(1, plain.Count); Equal(0, plain[0].OwnedView.FrameCount);
        Equal(Text("single"), plain[0].ResolveOwnedField(owner, new[] { "Rows", "Value" }));
        BasicOwnedError("N809.singleton.no-position", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
            () => plain[0].OwnedPosition(owner, path));
        var calls = 0; BasicRoot(Group()).VisitPrimaryFailureRows(path, _ => calls++); Equal(0, calls);
    }
    private static void BasicPrimaryFailureBaseRefusals()
    {
        var path = new[] { "Rows" }; var calls = 0;
        var primary = BasicValue("same-physical-payload");
        // Legacy fallback selects the outer projection Group, not its aliased child.
        var alias = BasicRoot(primary, new FerruleDeclaredNamedSourceRoot("Rows", primary));
        var error = PrimaryFailureError("N809.named-fallback-alias", () => alias.VisitPrimaryFailureRows(path, _ => calls++),
            () => new { calls, primary = BasicInstanceSnapshot(primary), parent = BasicContextSnapshot(alias) });
        PrimaryFailureBridge(error, FerruleBasicSourceBridgeErrorKind.UnsupportedPrimaryFailureContext); Equal(0, calls);
        var tight = new FerruleOwnedSourceLimits(2, 0, 1, 4, 0);
        var tightAlias = FerruleBasicSourceRoots.Create(primary, new[] { new FerruleDeclaredNamedSourceRoot("Rows", primary) }, tight).RootContext();
        error = PrimaryFailureError("N809.named-fallback-before-work", () => tightAlias.VisitPrimaryFailureRows(path, _ => calls++),
            () => new { calls, limits = PrimaryFailureLimits(tight), parent = BasicContextSnapshot(tightAlias) });
        PrimaryFailureBridge(error, FerruleBasicSourceBridgeErrorKind.UnsupportedPrimaryFailureContext); Equal(0, calls);
        var unavailable = BasicRoot(primary, new FerruleDeclaredNamedSourceRoot("Rows", null));
        unavailable.VisitPrimaryFailureRows(path, _ => calls++); Equal(0, calls);
        // A real Primary field wins over the same supplied name. Its insufficient
        // work bytes remain an owned metadata error, not a fallback refusal.
        var ownRows = Group(Field("Rows", Repeated()));
        var tightPrimary = FerruleBasicSourceRoots.Create(ownRows, new[] { new FerruleDeclaredNamedSourceRoot("Rows", primary) }, tight).RootContext();
        var work = BasicOwnedError("N809.primary-before-named", FerruleOwnedSourceErrorKind.MetadataLimit,
            () => tightPrimary.VisitPrimaryFailureRows(path, _ => calls++));
        Equal<FerruleOwnedSourceLimitKind?>(FerruleOwnedSourceLimitKind.MetadataBytes, work.LimitKind); Equal(0, calls);
        var authentic = BasicRoot(Group(Field("Rows", Repeated(BasicValue("real")))));
        var impostor = Group(Field("Rows", Repeated(BasicValue("forged"))));
        var forged = FerruleBasicScopeContext.Create(authentic.Roots, ScopeContext.FromSource(impostor), authentic.OwnedView,
            new[] { new BasicFrameOrigin(0, Array.Empty<string>()) }, 1);
        error = PrimaryFailureError("N809.forged-base", () => forged.VisitPrimaryFailureRows(path, _ => calls++),
            () => new { calls, parent = BasicContextSnapshot(forged) });
        PrimaryFailureBridge(error, FerruleBasicSourceBridgeErrorKind.UnsupportedPrimaryFailureContext); Equal(0, calls);
        var nested = BasicRoot(Group(Field("Rows", Repeated(Group(Field("Rows", Repeated(BasicValue("nested"))))))));
        var child = nested.WalkSourceCohort(path)[0].Context;
        error = PrimaryFailureError("N809.child-base", () => child.VisitPrimaryFailureRows(path, _ => calls++),
            () => new { calls, parent = BasicContextSnapshot(child) });
        PrimaryFailureBridge(error, FerruleBasicSourceBridgeErrorKind.UnsupportedPrimaryFailureContext); Equal(0, calls);
        foreach (var bad in new[] { Array.Empty<string>(), new[] { "" }, new[] { "Rows", "Value" } })
        {
            error = PrimaryFailureError("N809.path", () => authentic.VisitPrimaryFailureRows(bad, _ => calls++), () => new { calls, path = bad });
            PrimaryFailureBridge(error, FerruleBasicSourceBridgeErrorKind.UnsupportedPrimaryFailureContext);
        }
        var repeated = BasicRoot(Repeated(BasicValue("not-group")));
        error = PrimaryFailureError("N809.repeated-root", () => repeated.VisitPrimaryFailureRows(path, _ => calls++), () => new { calls });
        PrimaryFailureBridge(error, FerruleBasicSourceBridgeErrorKind.UnsupportedPrimaryFailureContext); Equal(0, calls);
    }
    private static void BasicPrimaryFailureReachedTerminals()
    {
        var path = new[] { "Rows" };
        var special = new FerruleInstance[] { new FerruleMappedSequence(new[] { BasicValue("mapped") }),
            new FerruleDocumentSet(new[] { new FerruleDocument("doc", BasicValue("document")) }) };
        foreach (var terminal in special)
        {
            var expectedKind = terminal is FerruleDocumentSet ? FerruleOwnedSourceInstanceKind.DocumentSet : FerruleOwnedSourceInstanceKind.MappedSequence;
            var root = BasicRoot(Group(Field("Rows", Repeated(BasicValue("first"), terminal))));
            var calls = 0; var selected = FerruleFailures.MappingFailure(1, Text("first wins"));
            var error = PrimaryFailureError("N809.prior-selected", () => root.VisitPrimaryFailureRows(path, _ => { calls++; throw selected; }),
                () => new { calls, terminal = terminal.GetType().FullName, selected = BasicErrorSnapshot(selected) });
            Equal(true, ReferenceEquals(selected, error)); Equal(1, calls);
            calls = 0;
            error = PrimaryFailureError("N809.later-special", () => root.VisitPrimaryFailureRows(path, _ => calls++), () => new { calls, terminal = terminal.GetType().FullName });
            Equal<FerruleOwnedSourceInstanceKind?>(expectedKind, PrimaryFailureBridge(error, FerruleBasicSourceBridgeErrorKind.UnsupportedBasicContext).Found); Equal(1, calls);
            var first = BasicRoot(Group(Field("Rows", Repeated(terminal, BasicValue("later"))))); calls = 0;
            error = PrimaryFailureError("N809.first-special", () => first.VisitPrimaryFailureRows(path, _ => calls++), () => new { calls });
            Equal<FerruleOwnedSourceInstanceKind?>(expectedKind, PrimaryFailureBridge(error, FerruleBasicSourceBridgeErrorKind.UnsupportedBasicContext).Found); Equal(0, calls);
            var singleton = BasicRoot(Group(Field("Rows", terminal)));
            error = PrimaryFailureError("N809.singleton-special", () => singleton.VisitPrimaryFailureRows(path, _ => calls++), () => new { calls });
            Equal<FerruleOwnedSourceInstanceKind?>(expectedKind, PrimaryFailureBridge(error, FerruleBasicSourceBridgeErrorKind.UnsupportedBasicContext).Found); Equal(0, calls);
        }
        var lazy = BasicRoot(Group(Field("Rows", Repeated(BasicValue("safe"), BasicValue("unused")))), new FerruleDeclaredNamedSourceRoot("Missing", null));
        var visits = 0; var failure = FerruleFailures.MappingFailure(2, Text("lazy"));
        var reached = PrimaryFailureError("N809.lazy-message", () => lazy.VisitPrimaryFailureRows(path, context =>
        {
            visits++;
            var choose = context.OwnedPosition(OwnedPrimaryOwner, path) == 1;
            var value = choose ? Text("lazy") : context.ResolveOwnedField(OwnedNamedOwner("Missing"), new[] { "Value" });
            if (choose) { Equal(Text("lazy"), value); throw failure; }
        }), () => new { visits });
        Equal(true, ReferenceEquals(failure, reached)); Equal(1, visits);
    }
    private static void BasicPrimaryFailureBudgets()
    {
        var path = new[] { "Rows" }; var calls = 0;
        var primary = Group(Field("Rows", Repeated(BasicValue("A"), BasicValue("B"))));
        FerruleBasicScopeContext Root(nuint bytes, nuint count) => FerruleBasicSourceRoots.Create(primary,
            Array.Empty<FerruleDeclaredNamedSourceRoot>(), new(1, 1, 1, bytes, count)).RootContext();
        var shortCount = Root(4096, 1);
        var error = PrimaryFailureError("N809.complete-count", () => shortCount.VisitPrimaryFailureRows(path, _ => calls++), () => new { calls });
        var cause = PrimaryFailureBridge(error, FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit);
        Equal<FerruleBasicSourceBridgeComponent?>(FerruleBasicSourceBridgeComponent.WalkContexts, cause.Component);
        Equal<nuint?>(0, cause.Used); Equal<nuint?>(2, cause.Requested); Equal<nuint?>(1, cause.Maximum); Equal(0, calls);
        var shortBytes = Root(51, 2);
        error = PrimaryFailureError("N809.complete-bytes", () => shortBytes.VisitPrimaryFailureRows(path, _ => calls++), () => new { calls });
        var outer = (FerruleRuntimeException)error; var owned = outer.OwnedSourceFailure!;
        Equal(FerruleRuntimeError.OwnedSource, outer.Error); Equal<uint?>(null, outer.Node);
        Equal(true, ReferenceEquals(owned, outer.InnerException)); Equal(FerruleOwnedSourceErrorKind.MetadataLimit, owned.Error);
        Equal<FerruleOwnedSourceLimitKind?>(FerruleOwnedSourceLimitKind.MetadataBytes, owned.LimitKind);
        Equal<nuint?>(48, owned.Used); Equal<nuint?>(4, owned.Requested); Equal<nuint?>(51, owned.Maximum); Equal(0, calls);
        var exact = Root(52, 2); exact.VisitPrimaryFailureRows(path, _ => calls++);
        OwnedRaw("N809.exact-bytes", new { calls, limits = PrimaryFailureLimits(exact.Roots.Limits) }); Equal(2, calls);
    }
}

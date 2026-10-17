using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private sealed class BasicWindowBoundProbe(int count) : IReadOnlyList<Func<FerruleBasicScopeContext, FerruleSequenceWindow>>
    {
        internal int Reads { get; private set; }
        public int Count => count;
        public Func<FerruleBasicScopeContext, FerruleSequenceWindow> this[int index]
        { get { Reads++; throw new InvalidOperationException("Inadmissible window getter reached."); } }
        public IEnumerator<Func<FerruleBasicScopeContext, FerruleSequenceWindow>> GetEnumerator() => throw new InvalidOperationException("Windows snapshot by index.");
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
    }
    private static void BasicBoundedOwnedWindowControls()
    {
        var owner = OwnedPrimaryOwner; var named = OwnedNamedOwner("Ref");
        var rows = new[] { "Rows" }; var empty = Array.Empty<string>();
        var noKeys = Array.Empty<FerruleSortKey<FerruleBasicScopeContext>>();
        var noBounds = Array.Empty<Func<FerruleBasicScopeContext, FerruleSequenceWindow>>();
        FerruleInstance Item(FerruleBasicScopeContext context) => context.LegacyView.Frames[^1];
        object Input(FerruleBasicScopeContext parent) => new
        {
            parent = BasicContextSnapshot(parent),
            declarations = Enumerable.Range(0, (int)parent.Roots.OwnedRoots.RegistryCount).Select(i => new
            { key = i, name = parent.Roots.OwnedRoots.Name(i), source = BasicInstanceSnapshot(parent.Roots.OwnedRoots.Instance(i)) }).ToArray(),
            limits = new { roots = (ulong)parent.Roots.Limits.MaxRoots, frames = (ulong)parent.Roots.Limits.MaxActiveFrames,
                segments = (ulong)parent.Roots.Limits.MaxPathSegments, bytes = (ulong)parent.Roots.Limits.MaxMetadataBytes,
                contexts = (ulong)parent.Roots.Limits.MaxWalkContexts }
        };
        void Record(string id, FerruleBasicScopeContext parent, FerruleRetainedSourceCohort original,
            IReadOnlyList<FerruleInstance> result, object facts) => OwnedRaw(id, new
            { classification = "complete_window_input_and_reached_result_before_asserts", input = Input(parent),
                original = original.Select(BasicCandidateSnapshot).ToArray(), result = result.Select(BasicInstanceSnapshot).ToArray(), facts });
        Exception Refusal(string id, Action subject, Func<object> facts)
        {
            try { subject(); }
            catch (Exception error) { BasicErrorRaw(id, error, facts()); return error; }
            throw new InvalidOperationException("Expected a window control refusal.");
        }
        FerruleBasicSourceBridgeException Bridge(Exception error, FerruleBasicSourceBridgeErrorKind kind)
        {
            var outer = error as FerruleRuntimeException;
            var cause = error as FerruleBasicSourceBridgeException ?? outer?.BasicSourceBridgeFailure;
            Equal(true, cause is not null); Equal(kind, cause!.Error);
            if (outer is not null)
            { Equal(FerruleRuntimeError.BasicSourceBridge, outer.Error); Equal(true, ReferenceEquals(cause, outer.InnerException)); Equal<uint?>(null, outer.Node); }
            return cause;
        }
        var a = BasicValue("A"); var b = BasicValue("B"); var c = BasicValue("C"); var d = BasicValue("D");
        var primary = Group(Field("Rows", Repeated(a, b, c, d)));
        var root = BasicRoot(primary); var original = root.WalkOwnedSourceCohort(owner, rows);
        var cases = new (string Id, FerruleSequenceWindow[] Windows, int[] Ids, bool First)[]
        {
            ("N703.skip", new[] { FerruleSequenceWindow.SkipFirst(2) }, new[] { 2, 3 }, false),
            ("N703.first", new[] { FerruleSequenceWindow.First(2) }, new[] { 0, 1 }, false),
            ("N703.from", new[] { FerruleSequenceWindow.From(2) }, new[] { 1, 2, 3 }, false),
            ("N703.from-to", new[] { FerruleSequenceWindow.FromTo(2, 3) }, new[] { 1, 2 }, false),
            ("N703.last", new[] { FerruleSequenceWindow.Last(2) }, new[] { 2, 3 }, false),
            ("N703.ordered-first", new[] { FerruleSequenceWindow.SkipFirst(1), FerruleSequenceWindow.Last(2) }, new[] { 2 }, true),
            ("N703.zero-reversed", new[] { FerruleSequenceWindow.From(0), FerruleSequenceWindow.FromTo(4, 2) }, Array.Empty<int>(), false),
            ("N703.oversized", new[] { FerruleSequenceWindow.SkipFirst(ulong.MaxValue), FerruleSequenceWindow.Last(ulong.MaxValue) }, Array.Empty<int>(), false)
        };
        foreach (var test in cases)
        {
            var bounds = test.Windows.Select(window => new Func<FerruleBasicScopeContext, FerruleSequenceWindow>(_ => window)).ToArray();
            var contexts = new List<FerruleBasicScopeContext>(); var boundCalls = 0;
            for (var i = 0; i < bounds.Length; i++) { var bound = bounds[i]; bounds[i] = parent => { Equal(true, ReferenceEquals(root, parent)); boundCalls++; return bound(parent); }; }
            var result = root.VisitWindowedSourceCohort(original, noKeys, false, null, bounds, test.First,
                context => { contexts.Add(context); return Item(context); });
            Record(test.Id, root, original, result, new { boundCalls, contexts = contexts.Select(BasicContextSnapshot).ToArray(),
                positions = contexts.Select(context => (ulong)context.OwnedPosition(owner, rows)).ToArray(),
                samePhysicalItems = test.Ids.Select((id, index) => ReferenceEquals(original[id].Context.LegacyView.Frames[^1], result[index])).ToArray() });
            Equal(test.Windows.Length, boundCalls); Equal(test.Ids.Length, result.Count);
            for (var i = 0; i < result.Count; i++)
            { Equal(true, ReferenceEquals(original[test.Ids[i]].Context.LegacyView.Frames[^1], result[i])); Equal<nuint>((nuint)i + 1, contexts[i].OwnedPosition(owner, rows)); }
        }
        var plainOriginal = root.WalkSourceCohort(rows); var plainContexts = new List<FerruleBasicScopeContext>();
        var plainResult = root.VisitWindowedSourceCohort(plainOriginal, noKeys, false, null,
            new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[] { _ => FerruleSequenceWindow.SkipFirst(1) }, false,
            context => { plainContexts.Add(context); return Item(context); });
        Record("N703.plain", root, plainOriginal, plainResult, new { contexts = plainContexts.Select(BasicContextSnapshot).ToArray() });
        Equal(3, plainResult.Count); Equal(true, ReferenceEquals(b, plainResult[0])); Equal(true, ReferenceEquals(c, plainResult[1])); Equal(true, ReferenceEquals(d, plainResult[2]));
        for (var i = 0; i < plainContexts.Count; i++) { Equal<nuint>((nuint)i + 1, plainContexts[i].OwnedPosition(owner, rows)); Equal((long)i + 1, plainContexts[i].LegacyPosition(rows)); }
        var scalarSource = Scalar(FerruleValue.FromInt64(7)); var scalarRoot = BasicRoot(scalarSource); var scalarOriginal = scalarRoot.WalkOwnedSourceCohort(owner, empty);
        var scalarResult = scalarRoot.VisitWindowedSourceCohort(scalarOriginal, noKeys, false, null,
            new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[] { _ => FerruleSequenceWindow.First(1) }, false, Item);
        Record("N703.scalar-source", scalarRoot, scalarOriginal, scalarResult, new { sameScalar = ReferenceEquals(scalarSource, scalarResult[0]), cursor = scalarOriginal[0].HasNewCursor });
        Equal(true, ReferenceEquals(scalarSource, scalarResult[0])); Equal(false, scalarOriginal[0].HasNewCursor);
        var scored = BasicRoot(Group(Field("Rows", Repeated(
            Group(Field("Value", Scalar(Text("A"))), Field("Score", Scalar(FerruleValue.FromInt64(30)))),
            Group(Field("Value", Scalar(Text("B"))), Field("Score", Scalar(FerruleValue.FromInt64(10)))),
            Group(Field("Value", Scalar(Text("C"))), Field("Score", Scalar(FerruleValue.FromInt64(20))))))));
        var scoreOriginal = scored.WalkOwnedSourceCohort(owner, rows);
        foreach (var before in new[] { true, false })
        {
            var events = new List<string>(); var positions = new List<ulong>();
            var result = scored.VisitWindowedSourceCohort(scoreOriginal,
                new FerruleSortKey<FerruleBasicScopeContext>[] { new(context => { events.Add("k" + context.ResolveLegacyField(new[] { "Value" }).StringValue); return context.ResolveLegacyField(new[] { "Score" }); }) }, before,
                context => { events.Add("p" + context.ResolveLegacyField(new[] { "Value" }).StringValue + context.OwnedPosition(owner, rows)); return context.OwnedPosition(owner, rows) == 2; },
                new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[] { parent => { Equal(true, ReferenceEquals(scored, parent)); events.Add("b"); return FerruleSequenceWindow.First(2); } }, false,
                context => { events.Add("t" + context.ResolveLegacyField(new[] { "Value" }).StringValue); positions.Add((ulong)context.OwnedPosition(owner, rows)); return Item(context); });
            Record(before ? "N703.phase.fts" : "N703.phase.stf", scored, scoreOriginal, result, new { events, positions });
            Equal(before ? "pA1,pB2,pC3,kB,b,tB" : "kA,kB,kC,b,pB1,pC2,pA3,tC", string.Join(',', events));
            Equal(1, result.Count); Equal(before ? "B" : "C", ((FerruleScalar)((FerruleGroup)result[0]).Fields.First(field => field.Name == "Value").Value).Value.StringValue);
            Equal(1UL, positions.Single());
        }
        var outer = original[1].WithPhasePosition(2).Context;
        var identity = outer.WalkOwnedSourceCohort(owner, rows); var identityContexts = new List<FerruleBasicScopeContext>();
        var identityResult = outer.VisitWindowedSourceCohort(identity, noKeys, false, null,
            new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[] { parent => { Equal(true, ReferenceEquals(outer, parent)); return FerruleSequenceWindow.First((ulong)parent.OwnedPosition(owner, rows)); } }, true,
            context => { identityContexts.Add(context); return Item(context); });
        Record("N703.identity", outer, identity, identityResult, new { contexts = identityContexts.Select(BasicContextSnapshot).ToArray(), sameItem = ReferenceEquals(b, identityResult[0]),
            originalCursor = identity[0].HasNewCursor, inheritedPosition = (ulong)identityContexts[0].OwnedPosition(owner, rows) });
        Equal(false, identity[0].HasNewCursor); Equal(true, ReferenceEquals(b, identityResult[0])); Equal<nuint>(2, identityContexts[0].OwnedPosition(owner, rows));
        var namedRoot = BasicRoot(primary, new FerruleDeclaredNamedSourceRoot("Ref", Group(Field("Rows", Repeated(BasicValue("N1"), BasicValue("N2"))))));
        var namedParent = namedRoot.WalkOwnedSourceCohort(owner, rows)[2].WithPhasePosition(2).Context;
        var namedOriginal = namedParent.WalkOwnedSourceCohort(named, rows); var namedContexts = new List<FerruleBasicScopeContext>();
        var namedResult = namedParent.VisitWindowedSourceCohort(namedOriginal, noKeys, false, null,
            new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[] { parent => FerruleSequenceWindow.Last((ulong)parent.OwnedPosition(owner, rows) - 1) }, false,
            context => { namedContexts.Add(context); return Item(context); });
        Record("N703.nested", namedParent, namedOriginal, namedResult, new { contexts = namedContexts.Select(BasicContextSnapshot).ToArray(),
            sameOuter = ReferenceEquals(c, namedContexts[0].OwnedView.FrameAt(0).Item), outerPosition = (ulong)namedContexts[0].OwnedPosition(owner, rows),
            innerPosition = (ulong)namedContexts[0].OwnedPosition(named, rows), root = BasicInstanceSnapshot(namedContexts[0].Roots.OwnedRoots.Instance(0)) });
        Equal<nuint>(2, namedContexts[0].OwnedPosition(owner, rows)); Equal<nuint>(1, namedContexts[0].OwnedPosition(named, rows)); Equal(true, ReferenceEquals(c, namedContexts[0].OwnedView.FrameAt(0).Item));
        var boundEvents = new List<string>(); var predicateCalls = 0;
        var zero = root.VisitWindowedSourceCohort(original, noKeys, false, _ => { predicateCalls++; return true; },
            new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[] { _ => { boundEvents.Add("first"); return FerruleSequenceWindow.First(0); }, _ => { boundEvents.Add("last"); return FerruleSequenceWindow.Last(4); } }, true,
            _ => throw new InvalidOperationException("zero window target"));
        Record("N703.zero-eager", root, original, zero, new { boundEvents, predicateCalls }); Equal("first,last", string.Join(',', boundEvents)); Equal(4, predicateCalls); Equal(0, zero.Count);
        var emptyRoot = BasicRoot(Group(Field("Rows", Repeated()))); var emptyOriginal = emptyRoot.WalkOwnedSourceCohort(owner, rows); var emptyCalls = 0;
        var emptyResult = emptyRoot.VisitWindowedSourceCohort(emptyOriginal, noKeys, false, _ => throw new InvalidOperationException("empty predicate"),
            new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[] { parent => { Equal(true, ReferenceEquals(emptyRoot, parent)); emptyCalls++; return FerruleSequenceWindow.First(0); }, _ => { emptyCalls++; return FerruleSequenceWindow.Last(1); } }, false, Item);
        Record("N703.empty-eager", emptyRoot, emptyOriginal, emptyResult, new { emptyCalls }); Equal(2, emptyCalls); Equal(0, emptyResult.Count);
        var marker = new FerruleRuntimeException(FerruleRuntimeError.MappingException, "window marker", node: uint.MaxValue, mappingExceptionMessage: "window marker");
        var laterBound = Refusal("N703.zero-later-error", () => root.VisitWindowedSourceCohort(original, noKeys, false, _ => throw new InvalidOperationException("predicate not reached"),
            new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[] { _ => FerruleSequenceWindow.First(0), _ => throw marker }, false, Item),
            () => new { input = Input(root), original = original.Select(BasicCandidateSnapshot).ToArray(), expected = BasicErrorSnapshot(marker) });
        Equal(true, ReferenceEquals(marker, laterBound));
        var eventsPriority = new List<string>();
        var predicateError = Refusal("N703.predicate-before-target", () => root.VisitWindowedSourceCohort(original, noKeys, false,
            context => { eventsPriority.Add("p" + context.OwnedPosition(owner, rows)); if (context.OwnedPosition(owner, rows) == 3) throw marker; return true; },
            new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[] { _ => { eventsPriority.Add("b"); return FerruleSequenceWindow.First(1); } }, false,
            _ => { eventsPriority.Add("t"); throw new InvalidOperationException("target lost"); }), () => new { eventsPriority, expected = BasicErrorSnapshot(marker) });
        Equal(true, ReferenceEquals(marker, predicateError)); Equal("b,p1,p2,p3", string.Join(',', eventsPriority));
        eventsPriority.Clear();
        var firstEndpoint = Refusal("N703.from-to-first-error", () => root.VisitWindowedSourceCohort(original, noKeys, false, null,
            new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[] { _ => FerruleSequenceWindow.FromTo(
                FerruleSequences.ItemCount(0, FerruleValue.FromBoolean(false)), FerruleSequences.ItemCount(uint.MaxValue, FerruleValue.XmlNil)) }, false, Item),
            () => new { input = Input(root), original = original.Select(BasicCandidateSnapshot).ToArray() });
        var endpoint = (FerruleRuntimeException)firstEndpoint; Equal(FerruleRuntimeError.NotAnItemCount, endpoint.Error); Equal<uint?>(0, endpoint.Node); Equal<FerruleValueKind?>(FerruleValueKind.Bool, endpoint.FoundKind); Equal<Exception?>(null, endpoint.InnerException);
        var validCounts = new[] { FerruleValue.FromInt64(-2), FerruleValue.FromDouble(-1.8), FerruleValue.FromDouble(2.9), Text("  +3  "), FerruleValue.FromDouble(double.MaxValue) };
        var counts = validCounts.Select(value => FerruleSequences.ItemCount(uint.MaxValue, value)).ToArray();
        OwnedRaw("N703.count-domain", new { values = validCounts.Select(BasicValueSnapshot).ToArray(), counts });
        Equal(true, counts.SequenceEqual(new ulong[] { 0, 0, 2, 3, long.MaxValue }));
        var invalidCounts = new[] { FerruleValue.Null, FerruleValue.JsonNull, FerruleValue.XmlNil, FerruleValue.FromBoolean(true), FerruleValue.FromDouble(double.NaN), FerruleValue.FromDouble(double.PositiveInfinity), Text("2.5"), Text("9223372036854775808") };
        var countErrors = invalidCounts.Select(value => { try { FerruleSequences.ItemCount(uint.MaxValue, value); throw new InvalidOperationException("count accepted"); }
            catch (FerruleRuntimeException error) { return error; } }).ToArray();
        OwnedRaw("N703.invalid-counts", new { values = invalidCounts.Select(BasicValueSnapshot).ToArray(), errors = countErrors.Select(BasicErrorSnapshot).ToArray() });
        for (var i = 0; i < countErrors.Length; i++) { Equal(FerruleRuntimeError.NotAnItemCount, countErrors[i].Error); Equal<uint?>(uint.MaxValue, countErrors[i].Node); Equal<FerruleValueKind?>(invalidCounts[i].Kind, countErrors[i].FoundKind); }
        var foreign = BasicRoot(primary); var callbackCalls = 0;
        var registry = Refusal("N703.registry", () => foreign.VisitWindowedSourceCohort(original, noKeys, false, null,
            new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[] { _ => { callbackCalls++; return FerruleSequenceWindow.First(1); } }, false, Item), () => new { input = Input(foreign), callbackCalls });
        Bridge(registry, FerruleBasicSourceBridgeErrorKind.RegistryMismatch); Equal(0, callbackCalls);
        var wrongParent = root.CloneView();
        var parentError = Refusal("N703.parent", () => wrongParent.VisitWindowedSourceCohort(original, noKeys, false, null, noBounds, true,
            _ => { callbackCalls++; return a; }), () => new { input = Input(wrongParent), callbackCalls });
        Equal<nuint?>(0, Bridge(parentError, FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness).InputOrdinal); Equal(0, callbackCalls);
        var bytesRoot = BasicABC(71); var bytesOriginal = bytesRoot.WalkOwnedSourceCohort(owner, rows); var byteCalls = 0;
        var byteError = Refusal("N703.bytes-short", () => bytesRoot.VisitWindowedSourceCohort(bytesOriginal, noKeys, false, null,
            new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[] { _ => { byteCalls++; return FerruleSequenceWindow.First(0); } }, false, Item),
            () => new { input = Input(bytesRoot), original = bytesOriginal.Select(BasicCandidateSnapshot).ToArray(), byteCalls });
        var byteCause = Bridge(byteError, FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit);
        Equal<FerruleBasicSourceBridgeComponent?>(FerruleBasicSourceBridgeComponent.MetadataBytes, byteCause.Component); Equal<nuint?>(60, byteCause.Used); Equal<nuint?>(12, byteCause.Requested); Equal<nuint?>(71, byteCause.Maximum); Equal(0, byteCalls);
        var exactRoot = BasicABC(72); var exactOriginal = exactRoot.WalkOwnedSourceCohort(owner, rows); var exactCalls = 0;
        var exactResult = exactRoot.VisitWindowedSourceCohort(exactOriginal, noKeys, false, null,
            new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[] { _ => { exactCalls++; return FerruleSequenceWindow.First(0); } }, false, Item);
        Record("N703.bytes-exact", exactRoot, exactOriginal, exactResult, new { exactCalls }); Equal(1, exactCalls); Equal(0, exactResult.Count);
        var singleton = BasicValue("S"); var singletonRoot = BasicRoot(primary, new FerruleDeclaredNamedSourceRoot("Ref", singleton));
        var singletonParent = singletonRoot.WalkOwnedSourceCohort(owner, rows)[2].WithPhasePosition(2).Context;
        var singletonOriginal = singletonParent.WalkOwnedSourceCohort(named, empty); var singletonContexts = new List<FerruleBasicScopeContext>();
        var singletonResult = singletonParent.VisitWindowedSourceCohort(singletonOriginal, noKeys, false, null,
            new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[] { parent => FerruleSequenceWindow.First((ulong)parent.OwnedPosition(owner, rows)) }, true,
            context => { singletonContexts.Add(context); return Group(Field("Owned", Scalar(context.ResolveOwnedFieldWithRootPin(named, new[] { "Value" }, empty, true))),
                Field("Relative", Scalar(context.ResolveLegacyField(new[] { "Value" }))), Field("Outer", Scalar(FerruleValue.FromInt64((long)context.OwnedPosition(owner, rows))))); });
        Record("N703.fresh-singleton", singletonParent, singletonOriginal, singletonResult, new { contexts = singletonContexts.Select(BasicContextSnapshot).ToArray(),
            originalCursor = singletonOriginal[0].HasNewCursor, sameLegacyItems = singletonContexts[0].LegacyView.Frames.Select((frame, index) => ReferenceEquals(frame, singletonParent.LegacyView.Frames[index])).ToArray(),
            sameNamedRoot = ReferenceEquals(singleton, singletonContexts[0].Roots.OwnedRoots.Instance(1)) });
        Equal(false, singletonOriginal[0].HasNewCursor); Equal<nuint>(2, singletonContexts[0].OwnedPosition(owner, rows)); Equal(2L, singletonContexts[0].LegacyPosition(empty));
        Equal(true, ReferenceEquals(c, singletonContexts[0].LegacyView.Frames[^1])); Equal(Text("S"), singletonContexts[0].ResolveOwnedFieldWithRootPin(named, new[] { "Value" }, empty, true));
        Equal(Text("C"), singletonContexts[0].ResolveLegacyField(new[] { "Value" }));
        var mutableKeys = new FerruleSortKey<FerruleBasicScopeContext>[] { new(context => context.ResolveLegacyField(new[] { "Score" })) };
        var snapshotEvents = new List<string>();
        var mutableBounds = new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[]
        { _ => { snapshotEvents.Add("b0"); return FerruleSequenceWindow.First(2); }, _ => { snapshotEvents.Add("b1"); return FerruleSequenceWindow.Last(2); } };
        var snapshotResult = scored.VisitWindowedSourceCohort(scoreOriginal, mutableKeys, true,
            _ => { mutableKeys[0] = new(_ => throw new InvalidOperationException("mutated key reached")); mutableBounds[1] = _ => throw new InvalidOperationException("mutated bound reached"); return true; },
            mutableBounds, false, Item);
        Record("N703.descriptor-snapshot", scored, scoreOriginal, snapshotResult, new { snapshotEvents });
        Equal("b0,b1", string.Join(',', snapshotEvents)); Equal(true, ReferenceEquals(scoreOriginal[1].Context.LegacyView.Frames[^1], snapshotResult[0])); Equal(true, ReferenceEquals(scoreOriginal[2].Context.LegacyView.Frames[^1], snapshotResult[1]));
        foreach (var stage in new[] { "fts-predicate", "stf-key", "stf-bound" })
        {
            var priority = new List<string>(); var before = stage == "fts-predicate";
            var reached = Refusal("N703.frontier." + stage, () => scored.VisitWindowedSourceCohort(scoreOriginal,
                new FerruleSortKey<FerruleBasicScopeContext>[] { new(context => { priority.Add("k" + context.ResolveLegacyField(new[] { "Value" }).StringValue); if (stage == "stf-key") throw marker; return context.ResolveLegacyField(new[] { "Score" }); }) }, before,
                context => { priority.Add("p" + context.ResolveLegacyField(new[] { "Value" }).StringValue); if (before) throw marker; return true; },
                new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[] { _ => { priority.Add("b"); throw marker; } }, false,
                _ => throw new InvalidOperationException("frontier target reached")), () => new { stage, priority, expected = BasicErrorSnapshot(marker) });
            Equal(true, ReferenceEquals(marker, reached)); Equal(stage == "fts-predicate" ? "pA" : stage == "stf-key" ? "kA" : "kA,kB,kC,b", string.Join(',', priority));
        }
        var capacityKeys = new BasicSortKeyProbe(int.MaxValue);
        var capacity = Refusal("N703.key-capacity", () => root.VisitWindowedSourceCohort(original, capacityKeys, false, null, noBounds, true, Item),
            () => new { input = Input(root), keyCount = capacityKeys.Count, capacityKeys.Reads });
        var capacityCause = Bridge(capacity, FerruleBasicSourceBridgeErrorKind.PairedMetadataArithmeticOverflow);
        Equal<FerruleBasicSourceBridgeComponent?>(FerruleBasicSourceBridgeComponent.MetadataSlots, capacityCause.Component); Equal<nuint?>(null, capacityCause.Used); Equal<nuint?>(null, capacityCause.Requested); Equal<nuint?>(null, capacityCause.Maximum); Equal(0, capacityKeys.Reads);
        var workspaceBounds = new BasicWindowBoundProbe(100_000); var workspaceRoot = BasicABC(); var workspaceOriginal = workspaceRoot.WalkOwnedSourceCohort(owner, rows);
        var workspaceError = Refusal("N703.window-workspace", () => workspaceRoot.VisitWindowedSourceCohort(workspaceOriginal, noKeys, false, null, workspaceBounds, false, Item),
            () => new { input = Input(workspaceRoot), original = workspaceOriginal.Select(BasicCandidateSnapshot).ToArray(), boundCount = workspaceBounds.Count, workspaceBounds.Reads });
        var workspaceCause = Bridge(workspaceError, FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit);
        Equal<FerruleBasicSourceBridgeComponent?>(FerruleBasicSourceBridgeComponent.MetadataSlots, workspaceCause.Component); Equal<nuint?>(73, workspaceCause.Used); Equal<nuint?>(600_062, workspaceCause.Requested); Equal<nuint?>(36_842, workspaceCause.Maximum); Equal(0, workspaceBounds.Reads);
        GC.KeepAlive(original); GC.KeepAlive(scoreOriginal); GC.KeepAlive(identity); GC.KeepAlive(namedOriginal); GC.KeepAlive(emptyOriginal); GC.KeepAlive(bytesOriginal); GC.KeepAlive(exactOriginal);
    }
}

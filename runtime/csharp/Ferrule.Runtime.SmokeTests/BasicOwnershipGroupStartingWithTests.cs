#if FERRULE_COMPONENT_ORACLES
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

// SOURCE-ONLY proposal. Future bool APIs/private marker names are unimplemented.
// All delegates below are fixture controls, never production partition/calculation code.
internal static partial class Program
{
    private sealed class StartingControls : IFerruleBasicGroupStartingWithControls
    {
        internal FerruleBasicGroupByMetadata Description = new(Array.Empty<bool>(), false, 0, false, false, false);
        internal int MetadataCalls, FilterCalls, SortCalls, WindowCalls, BoundaryCalls, PostCalls;
        internal Func<FerruleBasicScopeContext, bool> Predicate = _ => true, Start = _ => false, Post = _ => true;
        internal Func<int, FerruleBasicScopeContext, FerruleValue> Key = (_, _) => FerruleValue.FromInt64(0);
        internal Func<int, FerruleBasicScopeContext, FerruleSequenceWindow> Bound = (_, _) => FerruleSequenceWindow.First(ulong.MaxValue);
        public FerruleBasicGroupByMetadata Metadata() { MetadataCalls++; return Description; }
        public bool Filter(FerruleBasicScopeContext c) { FilterCalls++; return Predicate(c); }
        public FerruleValue SortKey(int key, FerruleBasicScopeContext c) { SortCalls++; return Key(key, c); }
        public FerruleSequenceWindow Window(int bound, FerruleBasicScopeContext parent) { WindowCalls++; return Bound(bound, parent); }
        public bool StartsGroup(FerruleBasicScopeContext c) { BoundaryCalls++; return Start(c); }
        public bool RetainMember(FerruleBasicScopeContext c) { PostCalls++; return Post(c); }
        internal void Counts(int metadata, int filter, int sort, int windows, int boundary, int post)
        {
            Equal(metadata, MetadataCalls); Equal(filter, FilterCalls); Equal(sort, SortCalls);
            Equal(windows, WindowCalls); Equal(boundary, BoundaryCalls); Equal(post, PostCalls);
        }
    }
    private static FerruleGroup StartingSource() => Group(Field("Marker", Scalar(Text("P.root"))),
        Field("Rows", Repeated(ByRow("unused", "id0", true, 0), ByRow("unused", "id1", true, 1), ByRow("unused", "id2", true, 2))));
    private static int StartingId(FerruleBasicScopeContext c) => (int)ByRead(c, OwnedPrimaryOwner, "Rank").Int64Value;
    private static FerruleInstance StartingForbiddenTarget(FerruleBasicScopeContext _) =>
        throw new InvalidOperationException("Refusal control must not call a target.");
    private static StartingControls StartingFour(FerruleBasicScopeContext root, List<string> log, int variant = 4)
    {
        var directions = new ByDirections(2);
        var c = new StartingControls { Description = new(directions, variant == 5, 2, false, true, true) };
        c.Predicate = x => { var id = StartingId(x); log.Add("f" + id); return variant is not (5 or 6) || id != 1; };
        c.Key = (k, x) => { var id = StartingId(x); log.Add($"s{id}{k}"); directions.Descending = true; return FerruleValue.FromInt64(id); };
        c.Bound = (i, parent) => { Equal(true, ReferenceEquals(root, parent)); log.Add("w" + i);
            return i == 0 ? FerruleSequenceWindow.First(variant == 8 ? 0UL : 3UL) : FerruleSequenceWindow.Last(3); };
        c.Start = x => { var id = StartingId(x); log.Add("b" + id); return id == 1; };
        c.Post = x => { log.Add("p" + StartingId(x)); return true; };
        return c;
    }
    private static FerruleBasicSourceBridgeException StartingBridge(string id, Action run,
        FerruleBasicSourceBridgeErrorKind kind, FerruleBasicSourceBridgeComponent? component = null,
        nuint? used = null, nuint? requested = null, nuint? maximum = null, nuint? input = null, nuint? candidate = null)
    {
        var error = AdjFailure(id, run); var cause = ByBridge(error, kind, component);
        if (error is FerruleRuntimeException outer) StartingOuter(outer, FerruleRuntimeError.BasicSourceBridge);
        Equal(used, cause.Used); Equal(requested, cause.Requested); Equal(maximum, cause.Maximum);
        Equal(input, cause.InputOrdinal); Equal(candidate, cause.CandidateOrdinal);
        Equal<nuint?>(null, cause.DeclarationIndex); Equal<nuint?>(null, cause.DeclaredCount);
        Equal<FerruleOwnedSourceInstanceKind?>(null, cause.Found);
        return cause;
    }
    private static void StartingOuter(FerruleRuntimeException e, FerruleRuntimeError kind)
    {
        Equal(kind,e.Error);Equal<uint?>(null,e.Node);
        Equal(true,e.Function is null && e.ExpectedArity is null && e.ActualArity is null && e.FoundKind is null &&
            e.AggregateOperation is null && e.Detail is null && e.RequestedItems is null && e.MaximumItems is null &&
            e.MaximumDepth is null && e.RuntimeValue is null && e.FailureRule is null && e.MappingFailureMessage is null &&
            e.MappingExceptionMessage is null && e.Join is null && e.UserFunction is null && e.FunctionParameter is null &&
            e.ExpectedScalarType is null && e.RuntimeParameter is null && e.SourceField is null && e.FoundInstance is null && e.PrimaryRoot is null);
        if(kind==FerruleRuntimeError.BasicSourceBridge)Equal<FerruleOwnedSourceException?>(null,e.OwnedSourceFailure);
        else Equal<FerruleBasicSourceBridgeException?>(null,e.BasicSourceBridgeFailure);
    }
    private static void StartingOwnedError(FerruleRuntimeException error,FerruleOwnedSourceErrorKind kind,
        bool hasOwner=false,string? ownerName=null,string? requestedName=null,string[]? path=null,string[]? collection=null,nuint? segment=null,
        FerruleOwnedSourceLimitKind? limit=null,nuint? used=null,nuint? requested=null,nuint? maximum=null)
    {
        StartingOuter(error,FerruleRuntimeError.OwnedSource);var cause=error.OwnedSourceFailure!;
        Equal(true,ReferenceEquals(cause,error.InnerException));Equal<Exception?>(null,cause.InnerException);Equal(kind,cause.Error);
        Equal(hasOwner,cause.Owner is not null);if(hasOwner){Equal(ownerName,cause.Owner!.Name);Equal(ownerName is null,cause.Owner.IsPrimary);}
        Equal(requestedName,cause.RequestedName);Equal<IReadOnlyList<int>?>(null,cause.DeclarationIndices);
        Equal(path is null,cause.Path is null);if(path is not null){Equal(path.Length,cause.Path!.Count);for(var i=0;i<path.Length;i++)Equal(path[i],cause.Path[i]);}
        Equal<IReadOnlyList<string>?>(null,cause.Frame);
        Equal(collection is null,cause.Collection is null);if(collection is not null){Equal(collection.Length,cause.Collection!.Count);for(var i=0;i<collection.Length;i++)Equal(collection[i],cause.Collection[i]);}
        Equal(segment,cause.Segment);
        Equal<FerruleOwnedSourceInstanceKind?>(null,cause.FoundInstance);Equal<nuint?>(null,cause.RequestedIndex);
        Equal(limit,cause.LimitKind);Equal(used,cause.Used);Equal(requested,cause.Requested);Equal(maximum,cause.Maximum);
    }
    private static void BasicOwnedStartingR01R02()
    {
        var source = Group(Field("Rows", Repeated())); var root = BasicRoot(source);
        var original = root.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" }); var events = new List<string>();
        var c = StartingFour(root, events); var targets = 0;
        AdjExpect("G443.R01", root.MapGroupStartingWithSourceCohort(original, c, _ => { targets++; throw new InvalidOperationException("empty target"); }));
        c.Counts(1, 0, 0, 2, 0, 0); Equal(0, targets); Equal("w0,w1", string.Join(',', events));
        var foreignRegistry = BasicRoot(source);
        Equal(false, ReferenceEquals(foreignRegistry.Roots, root.Roots));
        var foreignControls = new StartingControls();
        StartingBridge("G443.R02", () => foreignRegistry.MapGroupStartingWithSourceCohort(original, foreignControls, StartingForbiddenTarget),
            FerruleBasicSourceBridgeErrorKind.RegistryMismatch);
        foreignControls.Counts(0, 0, 0, 0, 0, 0);

        var clonedParent = root.CloneView();
        Equal(false, ReferenceEquals(clonedParent, root));
        Equal(true, ReferenceEquals(clonedParent.Roots, root.Roots));
        var cloneControls = new StartingControls();
        StartingBridge("G443.R02", () => clonedParent.MapGroupStartingWithSourceCohort(original, cloneControls, StartingForbiddenTarget),
            FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness, input: 0);
        cloneControls.Counts(0, 0, 0, 0, 0, 0);
    }
    private static void BasicOwnedStartingR03()
    {
        var root = BasicRoot(StartingSource()); var rows = new[] { "Rows" };
        for (var variant = 0; variant < 6; variant++)
        {
            var original = root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows);
            // Proposed guarded private cohort fault seam (not implemented). It
            // changes genuine fixture bytes WITHOUT resealing/replacing its witness.
            var donor = root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows)[1];
            original.StartingWitnessFaultForTests(variant, donor);
            var c = new StartingControls();
            StartingBridge("G443.R03-" + variant, () => root.MapGroupStartingWithSourceCohort(original, c, StartingForbiddenTarget),
                variant == 0 ? FerruleBasicSourceBridgeErrorKind.DuplicatePhaseCandidate : FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness,
                input: variant == 0 ? 1u : variant == 1 ? 2u : variant == 2 ? 0u : 1u,
                candidate: variant == 0 ? 0u : null);
            // C# Seed validates terminal before metadata, unlike Rust.
            c.Counts(0, 0, 0, 0, 0, 0);
        }
        BasicOwnedByMalformedTerminalWitnesses(); // original full20-field table unchanged
    }
    private static void BasicOwnedStartingR04R05R06R08()
    {
        foreach (var variant in new[] { 4, 5, 6, 8 })
        {
            var root = BasicRoot(StartingSource()); var events = new List<string>(); var c = StartingFour(root, events, variant);
            var targets = 0;
            var output = root.MapGroupStartingWithSourceCohort(root.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" }), c, x =>
            { events.Add("t" + targets++); return ByOutput(x, OwnedPrimaryOwner, false); });
            if (variant == 4)
            {
                AdjExpect("G443.R04", output, AdjExpectedGroup(1, "id0", "P.root", "id0"), AdjExpectedGroup(2, "id1", "P.root", "id1", "id2"));
                Equal("s00,s01,s10,s11,s20,s21,w0,w1,f0,b0,p0,f1,b1,p1,f2,b2,p2,t0,t1", string.Join(',', events));
                c.Counts(1, 3, 6, 2, 3, 3); Equal(2, targets);
            }
            if (variant == 5)
            {
                AdjExpect("G443.R05", output, AdjExpectedGroup(1, "id0", "P.root", "id0", "id2"));
                Equal("f0,f1,f2,s00,s01,s20,s21,w0,w1,b0,p0,b2,p2,t0", string.Join(',', events));
                c.Counts(1, 3, 4, 2, 2, 2); Equal(1, targets);
            }
            if (variant == 6)
            {
                AdjExpect("G443.R06", output, AdjExpectedGroup(1, "id0", "P.root", "id0", "id2"));
                Equal("s00,s01,s10,s11,s20,s21,w0,w1,f0,b0,p0,f1,f2,b2,p2,t0", string.Join(',', events));
                c.Counts(1, 3, 6, 2, 2, 2); Equal(1, targets);
            }
            if (variant == 8)
            {
                AdjExpect("G443.R08", output); Equal(0, targets); c.Counts(1, 3, 6, 2, 3, 3);
                Equal("s00,s01,s10,s11,s20,s21,w0,w1,f0,b0,p0,f1,b1,p1,f2,b2,p2", string.Join(',', events));
            }
        }
    }
    private static void BasicOwnedStartingR07()
    {
        var root = BasicRoot(StartingSource()); var rows = new[] { "Rows" }; var held = new List<FerruleBasicScopeContext>();
        var events = new List<string>(); var c = StartingFour(root, events);
        c.Start = x => { events.Add("b" + StartingId(x)); return true; };
        var original = root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows);
        // A minimal runtime-owner test-only seam exposes the SAME operation's
        // pre/post state. No public constructor or reflected private field access.
        var receipt = FerruleBasicGroupByOperation.RunStartingWithSnapshotForTests(root, original, c, x =>
        {
            events.Add("t" + held.Count); held.Add(x);
            var inactive=(FerruleRuntimeException)AdjFailure("G443.nonempty-does-not-grant-root-pin",()=>x.ResolveOwnedFieldWithRootPin(OwnedPrimaryOwner,new[]{"Marker"},Array.Empty<string>(),false));
            StartingOwnedError(inactive,FerruleOwnedSourceErrorKind.InactiveSourceFrame,hasOwner:true,collection:Array.Empty<string>());
            var strict=(FerruleRuntimeException)AdjFailure("G443.root-bypass-keeps-strict-Rows",()=>x.ResolveOwnedFieldWithRootPin(OwnedPrimaryOwner,new[]{"Rows","Label"},Array.Empty<string>(),true));
            StartingOwnedError(strict,FerruleOwnedSourceErrorKind.UniteratedRepetition,hasOwner:true,path:new[]{"Rows","Label"},segment:1);
            return ByOutput(x, OwnedPrimaryOwner, false);
        });
        Equal((2, 6, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3), receipt.Capacities);
        Equal(true, receipt.KeysInitiallyNull); Equal(true, receipt.KeysFinallyNull);
        var output = receipt.Output;
        AdjExpect("G443.R07", output, AdjExpectedGroup(1, "id0", "P.root", "id0"),
            AdjExpectedGroup(2, "id1", "P.root", "id1"), AdjExpectedGroup(3, "id2", "P.root", "id2"));
        Equal("s00,s01,s10,s11,s20,s21,w0,w1,f0,b0,p0,f1,b1,p1,f2,b2,p2,t0,t1,t2", string.Join(',', events));
        c.Counts(1, 3, 6, 2, 3, 3);
        for (var i = 0; i < 3; i++)
        {
            var x = held[i]; Equal<nuint>((nuint)i + 1, x.OwnedPosition(OwnedPrimaryOwner, rows));
            AdjEqualTree(AdjExpectedGroup(i + 1, "id" + i, "P.root", "id" + i), ByOutput(x, OwnedPrimaryOwner, false));
            AdjEqualTree(output[i], ByOutput(x.CloneView(), OwnedPrimaryOwner, true));
            Equal(true, x.OwnedView.FrameAt(x.OwnedView.FrameCount - 1).GroupMembers);
            x.LegacyView.RequireBasicContext();
            var members = ((FerruleRepeated)x.LegacyView.Frames[^1]).Items;
            Equal(true, ((ICollection<FerruleInstance>)members).IsReadOnly);
            var mutation = AdjFailure("G443.R07-readonly-publication", () => ((IList<FerruleInstance>)members)[0] = Group());
            Equal(typeof(NotSupportedException), mutation.GetType());
            Equal(true, ReferenceEquals(original[i].GroupTerminal!.Value.Item, members[0]));
            for (var j = 0; j < i; j++)
            {
                Equal(false, ReferenceEquals(x, held[j])); Equal(false, ReferenceEquals(x.OwnedView, held[j].OwnedView));
                Equal(false, ReferenceEquals(x.LegacyView.Frames, held[j].LegacyView.Frames));
                Equal(false, ReferenceEquals(members, ((FerruleRepeated)held[j].LegacyView.Frames[^1]).Items));
                Equal(false, ReferenceEquals(x.OwnedView.FrameAt(x.OwnedView.FrameCount - 1).Path,
                    held[j].OwnedView.FrameAt(held[j].OwnedView.FrameCount - 1).Path));
            }
        }
        AdjEqualTree(AdjExpectedGroup(1, "id0", "P.root", "id0"), ByOutput(held[0], OwnedPrimaryOwner, false));
    }
    private static FerruleInstance StartingAccountingOutput(FerruleBasicScopeContext c) => Group(
        Field("First", Scalar(c.ResolveOwnedField(OwnedNamedOwner("Ω"), new[] { "巣", "行" }))),
        Field("Position", Scalar(FerruleValue.FromInt64((long)c.OwnedPosition(OwnedNamedOwner("Ω"), new[] { "巣", "行" })))),
        // Observe the published member chain without starting a second budgeted walk.
        Field("Members", (FerruleRepeated)c.LegacyView.Frames[^1]));
    private static FerruleInstance StartingAccountingExpected() => Group(Field("First", Scalar(FerruleValue.FromInt64(1))),
        Field("Position", Scalar(FerruleValue.FromInt64(1))), Field("Members", Repeated(Scalar(FerruleValue.FromInt64(1)), Scalar(FerruleValue.FromInt64(2)))));
    private static StartingControls StartingAccounting(List<string> log) => new()
    {
        Description = new(new[] { false, false }, false, 1, true, false, true),
        Key = (k, _) => { log.Add("s" + k); return Text("é"); },
        Bound = (_, _) => { log.Add("w"); return FerruleSequenceWindow.First(10); },
        Start = _ => { log.Add("b"); return false; }, Post = _ => { log.Add("p"); return true; }
    };
    private static void BasicOwnedStartingR12()
    {
        // Independently frozen enumeration, not Footprint-derived expected values:
        // Seed90/64 incl exactly2 setup; T36/40 each; U31/31 each;
        // fixed177/142; total267/206; checked ceiling3637.
        Equal(2, BasicGroupAccounting.LedgerSetupRecords); BasicGroupAccounting.AdmitLedgerSetup(2);
        StartingBridge("G443.R12-setup", () => BasicGroupAccounting.AdmitLedgerSetup(1),
            FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.MetadataSlots, 0, 2, 1);
        var root = ByAccountingRoot(206); var original = root.WalkSourceCohort(new[] { "Ω", "巣", "行" });
        var log = new List<string>(); var c = StartingAccounting(log);
        c.Key = (k, _) => { log.Add("s" + k); return FerruleValue.FromInt64(0); };
        c.Bound = (_, _) => { log.Add("w"); return FerruleSequenceWindow.First(0); };
        var seed = FerruleBasicGroupByOperation.Seed(root, original);
        var cost = FerruleBasicGroupByOperation.Footprint(original, c.Description, 2, out var keyCells, out var windows);
        Equal<nuint>(90, seed.Slots.Used); Equal<nuint>(64, seed.Bytes.Used);
        Equal<nuint>(177, cost.Slots); Equal<nuint>(142, cost.Bytes); Equal<nuint>(3637, seed.Slots.Maximum);
        Equal<nuint>(267, seed.Slots.Used + cost.Slots); Equal<nuint>(206, seed.Bytes.Used + cost.Bytes);
        Equal(4, keyCells); Equal(2, windows);
        foreach (var item in original)
        {
            var t = item.GroupTerminal!.Value;
            Equal((1, 3, 3, 1), (t.OwnedCut, t.LegacyCut, t.OriginCut, t.CollectionCut));
            Equal(2, t.Canonical.Count); Equal(3, t.RawCount); Equal("行", t.Wrapper);
            var template = BasicGroupPrefixTemplate.Cost(item.Context, t); var selection = BasicGroupPrefixTemplate.SelectionCost(item.Context);
            Equal<nuint>(36, template.Slots); Equal<nuint>(40, template.Bytes);
            Equal<nuint>(31, selection.Slots); Equal<nuint>(31, selection.Bytes);
        }
        var output = root.MapGroupStartingWithSourceCohort(original, c, _ => throw new InvalidOperationException("First0 target"));
        AdjExpect("G443.R12-exact", output);
        Equal("s0,s1,s0,s1,w,b,p,b,p", string.Join(',', log)); c.Counts(1, 0, 4, 1, 2, 2);
        root = ByAccountingRoot(205); original = root.WalkSourceCohort(new[] { "Ω", "巣", "行" });
        var directions = new ByDirections(1126); var excessive = new StartingControls { Description = new(directions, false, 1, true, false, true) };
        StartingBridge("G443.R12-slots", () => root.MapGroupStartingWithSourceCohort(original, excessive, StartingForbiddenTarget),
            FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.MetadataSlots, 90, 3549, 3637);
        excessive.Counts(1, 0, 0, 0, 0, 0); Equal(1, directions.CountReads); Equal(0, directions.IndexReads);
        var fitting = new ByDirections(2); var known = new StartingControls { Description = excessive.Description with { Descending = fitting } };
        StartingBridge("G443.R12-known-text", () => root.MapGroupStartingWithSourceCohort(original, known, StartingForbiddenTarget),
            FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.MetadataBytes, 64, 142, 205);
        known.Counts(1, 0, 0, 0, 0, 0); Equal(1, fitting.CountReads); Equal(0, fitting.IndexReads);
    }
    private static void BasicOwnedStartingR13R14()
    {
        foreach (var maximum in new nuint[] { 213, 214 })
        {
            var root = ByAccountingRoot(maximum); var original = root.WalkSourceCohort(new[] { "Ω", "巣", "行" });
            var log = new List<string>(); var c = StartingAccounting(log); var targets = 0;
            FerruleInstance Target(FerruleBasicScopeContext x) { targets++; log.Add("t"); return StartingAccountingOutput(x); }
            if (maximum == 213)
            {
                StartingBridge("G443.R13-fourth-sort-return", () => root.MapGroupStartingWithSourceCohort(original, c, Target),
                    FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.MetadataBytes, 212, 2, 213);
                Equal("s0,s1,s0,s1", string.Join(',', log)); c.Counts(1, 0, 4, 0, 0, 0); Equal(0, targets);
            }
            else
            {
                AdjExpect("G443.R13-known206-sort8-no-bool-text", BasicReached("G443.R13-complete-subject",
                    () => root.MapGroupStartingWithSourceCohort(original, c, Target)), StartingAccountingExpected());
                Equal("s0,s1,s0,s1,w,b,p,b,p,t", string.Join(',', log)); c.Counts(1, 0, 4, 1, 2, 2); Equal(1, targets);
            }
        }
        foreach (var boundary in new[] { false, true })
        {
            var root = ByAccountingRoot(boundary ? 214u : 213u); var original = root.WalkSourceCohort(new[] { "Ω", "巣", "行" });
            var log = new List<string>(); var c = StartingAccounting(log); var returns = 0;
            var error = new FerruleRuntimeException(FerruleRuntimeError.MappingException, boundary ? "boundary" : "sort", node: boundary ? 101u : 103u);
            c.Key = (k, _) => { log.Add("s" + k); if (!boundary && ++returns == 4) throw error; return Text("é"); };
            if (boundary) c.Start = _ => { log.Add("b"); throw error; };
            Equal(true, ReferenceEquals(error, AdjFailure("G443.R14-original-callback", () => root.MapGroupStartingWithSourceCohort(original, c, _ => throw new InvalidOperationException("no target")))));
            Equal(boundary ? "s0,s1,s0,s1,w,b" : "s0,s1,s0,s1", string.Join(',', log));
            c.Counts(1, 0, 4, boundary ? 1 : 0, boundary ? 1 : 0, 0);
        }
    }
    private static void BasicOwnedStartingR15()
    {
        var root = ByAccountingRoot(206); var original = root.WalkSourceCohort(new[] { "Ω", "巣", "行" });
        foreach (var windows in new[] { (int.MaxValue, true), (int.MaxValue, false) })
        {
            var c = new StartingControls { Description = new(Array.Empty<bool>(), false, windows.Item1, windows.Item2, false, false) };
            StartingBridge("G443.R15-window-representation", () => root.MapGroupStartingWithSourceCohort(original, c, StartingForbiddenTarget),
                FerruleBasicSourceBridgeErrorKind.PairedMetadataArithmeticOverflow, FerruleBasicSourceBridgeComponent.MetadataSlots);
            c.Counts(1, 0, 0, 0, 0, 0);
        }
        foreach (Action action in new Action[] {
            () => BasicGroupAccounting.Add(nuint.MaxValue, 1), () => BasicGroupAccounting.Mul(nuint.MaxValue, 2),
            () => BasicGroupAccounting.Sub(0, 1),
            () => BasicGroupAccounting.Maximum(BasicLimits with { MaxRoots = nuint.MaxValue }),
            () => BasicGroupAccounting.Maximum(BasicLimits with { MaxActiveFrames = nuint.MaxValue }),
            () => BasicGroupAccounting.Maximum(BasicLimits with { MaxPathSegments = nuint.MaxValue }),
            () => BasicGroupAccounting.Maximum(BasicLimits with { MaxWalkContexts = nuint.MaxValue }),
            () => BasicGroupAccounting.Capacity<BasicGroupTerminal>((nuint)Array.MaxLength + 1),
            () => BasicGroupAccounting.Capacity<FerruleValue>(nuint.MaxValue) })
            StartingBridge("G443.R15-checked-guard", action, FerruleBasicSourceBridgeErrorKind.PairedMetadataArithmeticOverflow, FerruleBasicSourceBridgeComponent.MetadataSlots);
        BasicGroupAccounting.AdmitLedgerSetup(nuint.MaxValue);
        var arithmeticLedger=new BasicSourceBudget(new FerruleOwnedSourceLimits(1,1,0,nuint.MaxValue,1),nuint.MaxValue);
        arithmeticLedger.Bytes.Charge(nuint.MaxValue);
        // Preserve the original C# phase-budget lifting: text arithmetic overflow
        // uses BasicSourceAccounting.Overflow's MetadataSlots component, NOT the
        // Rust charge_text MetadataBytes component. No fake giant string exists.
        StartingBridge("G443.R15-text-overflow-original-lifting",()=>arithmeticLedger.Text(1),
            FerruleBasicSourceBridgeErrorKind.PairedMetadataArithmeticOverflow,FerruleBasicSourceBridgeComponent.MetadataSlots);
        // Count guard precedes registry authentication, as in the original API.
        var tiny = FerruleBasicSourceRoots.Create(Group(), Array.Empty<FerruleDeclaredNamedSourceRoot>(),
            new FerruleOwnedSourceLimits(2, 2, 2, 4096, 1)).RootContext(); var never = new StartingControls();
        StartingBridge("G443.R15-walk-count-before-registry", () => tiny.MapGroupStartingWithSourceCohort(original, never, _ => throw new InvalidOperationException("target")),
            FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.WalkContexts, 0, 2, 1);
        never.Counts(0,0,0,0,0,0);
        foreach(var frames in new nuint[]{1,0})
        {
            var identity = FerruleBasicSourceRoots.Create(Group(), Array.Empty<FerruleDeclaredNamedSourceRoot>(),
                new FerruleOwnedSourceLimits(1,frames,1,4096,1)).RootContext();
            var singleton = identity.WalkOwnedSourceCohort(OwnedPrimaryOwner,Array.Empty<string>()); var controls = new StartingControls();var events = new List<string>();
            controls.Start = _ => {events.Add("b0");return false;};
            FerruleInstance Target(FerruleBasicScopeContext _) {events.Add("t0");return Scalar(FerruleValue.FromInt64(7));}
            if(frames==1)
            {
                AdjExpect("G443.R15-frame-exact",identity.MapGroupStartingWithSourceCohort(singleton,controls,Target),Scalar(FerruleValue.FromInt64(7)));
                Equal("b0,t0",string.Join(',',events));controls.Counts(1,0,0,0,1,0);
            }
            else
            {
                StartingBridge("G443.R15-frame-plus-one",()=>identity.MapGroupStartingWithSourceCohort(singleton,controls,Target),
                    FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit,FerruleBasicSourceBridgeComponent.ActiveFrames,0,1,0);
                Equal("",string.Join(',',events));controls.Counts(1,0,0,0,0,0);
            }
        }
        foreach (var source in new FerruleInstance[] { Group(), Group(Field("Rows", Repeated())),
            Group(Field("Rows", Scalar(FerruleValue.Null))), Group(Field("Rows", Scalar(FerruleValue.JsonNull))) })
        {
            foreach (var named in new[] { false, true })
            {
                var limited = FerruleBasicSourceRoots.Create(named ? Group() : source,
                    named ? new[] { new FerruleDeclaredNamedSourceRoot("Alias", source) } : Array.Empty<FerruleDeclaredNamedSourceRoot>(),
                    new FerruleOwnedSourceLimits(2, 4, 1, 4096, 4)).RootContext();
                foreach (var legacy in new[] { false, true })
                {
                    var c = new StartingControls();
                    var error = (FerruleRuntimeException)AdjFailure("G443.R15-complete-selector", () => limited.MapGroupStartingWithSourceCohort(
                        legacy ? limited.WalkSourceCohort(named ? new[] { "Alias", "Rows", "Tail" } : new[] { "Rows", "Tail" }) :
                        limited.WalkOwnedSourceCohort(named ? OwnedNamedOwner("Alias") : OwnedPrimaryOwner, new[] { "Rows", "Tail" }), c, StartingForbiddenTarget));
                    Equal(FerruleRuntimeError.OwnedSource, error.Error); Equal<uint?>(null, error.Node);
                    var cause = error.OwnedSourceFailure!; Equal(true, ReferenceEquals(cause, error.InnerException));
                    Equal(FerruleOwnedSourceErrorKind.MetadataLimit, cause.Error); Equal<FerruleOwnedSourceLimitKind?>(FerruleOwnedSourceLimitKind.PathSegments, cause.LimitKind);
                    Equal<nuint?>(0, cause.Used);
                    Equal<nuint?>(named && legacy ? 3u : 2u, cause.Requested);
                    Equal<nuint?>(named && legacy ? 2u : 1u, cause.Maximum);
                    StartingOwnedError(error,FerruleOwnedSourceErrorKind.MetadataLimit,limit:FerruleOwnedSourceLimitKind.PathSegments,
                        used:0,requested:named&&legacy?3u:2u,maximum:named&&legacy?2u:1u);
                    c.Counts(0, 0, 0, 0, 0, 0);
                }
            }
        }
        // These maintained guard suites retain all old exact W/F/P, alias and
        // checked-intermediate controls; they are not Starting public coverage.
        BasicOwnedByEmptyWitnessesAndCapacity(); BasicOwnedByCompleteCanonicalSelectors(); BasicOwnedAdjacentMetadataRefusalAndWitnessClosure();
        var limits = new FerruleOwnedSourceLimits(2, 4, 1, 0, 8);
        var prefixedRoot = FerruleBasicSourceRoots.Create(Group(Field("", Repeated(Group(Field("Extra", Repeated()))))),
            Array.Empty<FerruleDeclaredNamedSourceRoot>(), limits).RootContext();
        var prefixed = prefixedRoot.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "" })[0].Context;
        var canonical = new StartingControls();
        StartingBridge("G443.R15-canonical-before-query-bytes", () => prefixed.MapGroupStartingWithSourceCohort(
            prefixed.WalkSourceCohort(new[] { "Extra" }), canonical, _ => throw new InvalidOperationException("target")),
            FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.PathSegments, 0, 2, 1);
        canonical.Counts(0, 0, 0, 0, 0, 0);
        foreach (var namedRoot in new FerruleInstance[] { Group(), Repeated() })
        {
            var alias = FerruleBasicSourceRoots.Create(Group(), new[] { new FerruleDeclaredNamedSourceRoot("Alias", namedRoot) },
                limits with { MaxPathSegments = 0, MaxMetadataBytes = 4096 }).RootContext();
            var originalAlias = alias.WalkSourceCohort(new[] { "Alias" }); var log = new List<string>();
            var controls = new StartingControls { Description = new(Array.Empty<bool>(), false, 1, false, false, false),
                Bound = (_, _) => { log.Add("w0"); return FerruleSequenceWindow.First(0); }, Start = _ => { log.Add("b0"); return false; } };
            AdjExpect("G443.R15-consumed-alias-canonical-empty", alias.MapGroupStartingWithSourceCohort(originalAlias, controls, _ => throw new InvalidOperationException("First0 target")));
            controls.Counts(1, 0, 0, 1, namedRoot is FerruleGroup ? 1 : 0, 0);
            Equal(namedRoot is FerruleGroup ? "w0,b0" : "w0", string.Join(',', log));
        }
    }
    private static void BasicOwnedStartingPrivateMarkersAndOneShot()
    {
        var root = BasicRoot(StartingSource()); var original = root.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" });
        var first = original[0]; var terminal = first.GroupTerminal!.Value;
        void Invalid(string id, Action action) => StartingBridge(id, action, FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness, input: 0);
        var noClaim = new BasicGroupPrefixTemplate(first.Context, terminal);
        Invalid("G443.template-no-claim", () => noClaim.ConsumeMembers(1));
        Invalid("G443.template-wrapper-before-members", () => noClaim.ConsumeWrapper());
        Invalid("G443.template-finish-before-members", () => noClaim.Finish(Repeated(), 1));
        Invalid("G443.template-zero-claim", () => noClaim.Claim(0));
        var template = new BasicGroupPrefixTemplate(first.Context, terminal); template.Claim(1);
        Invalid("G443.template-double-claim", () => template.Claim(1));
        Invalid("G443.template-wrong-count", () => template.ConsumeMembers(2));
        var payload = FerruleRepeated.FromBasicGroupMembers(new[] { terminal.Item }, template);
        Invalid("G443.template-double-adopt", () => template.ConsumeMembers(1));
        Invalid("G443.template-zero-output-ordinal", () => template.Finish(payload, 0));
        var published = template.Finish(payload, 1);
        Invalid("G443.template-double-wrapper", () => template.ConsumeWrapper());
        Invalid("G443.template-double-finish", () => template.Finish(payload, 2));
        Equal(true, published.OwnedView.FrameAt(published.OwnedView.FrameCount - 1).GroupMembers);
        AdjEqualTree(AdjExpectedGroup(1, "id0", "P.root", "id0"), ByOutput(published, OwnedPrimaryOwner, false));
        Equal(true, ((ICollection<FerruleInstance>)payload.Items).IsReadOnly);
        var ordinary = root.LegacyView.GroupAdjacentBy(root.LegacyView.IterateSource(new[] { "Rows" }), new[] { "Rows" }, _ => Text("unused"))[0];
        StartingBridge("G443.ordinary-grouped-forged", () => ordinary.RequireBasicContext(), FerruleBasicSourceBridgeErrorKind.UnsupportedBasicContext);
        // Actual Starting root publications; new Item exact-owner shadows only itself.
        var repeated = Repeated(ByRow("unused", "a", true, 0), ByRow("unused", "b", true, 1), ByRow("unused", "c", true, 2), ByRow("unused", "d", true, 3));
        var foreign = Repeated(ByRow("unused", "foreign", true, 0)); var empty = Array.Empty<string>();
        root = BasicRoot(repeated, new FerruleDeclaredNamedSourceRoot("Alias", foreign), new FerruleDeclaredNamedSourceRoot("Absent", null));
        var rootLog = new List<string>();
        var c = new StartingControls { Start = x => { var id=x.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Rank" }).Int64Value;
            rootLog.Add("b"+id);return id is 1 or 2; } };
        var output = root.MapGroupStartingWithSourceCohort(root.WalkOwnedSourceCohort(OwnedPrimaryOwner, empty), c, x =>
        {
            rootLog.Add("t"+(x.OwnedPosition(OwnedPrimaryOwner,empty)-1));
            foreach (var view in new[] { x, x.CloneView() })
            {
                Equal(true, view.OwnedView.FrameAt(view.OwnedView.FrameCount - 1).GroupMembers);
                foreach (var nonrepeating in new[] { false, true })
                {
                    var err = (FerruleRuntimeException)AdjFailure("G443.root-uniterated", () => view.ResolveOwnedFieldWithRootPin(OwnedPrimaryOwner, new[] { "Label" }, empty, nonrepeating));
                    Equal(FerruleOwnedSourceErrorKind.UniteratedRepetition, err.OwnedSourceFailure!.Error);
                    Equal("Label", string.Join('/', err.OwnedSourceFailure.Path!)); Equal<nuint?>(0, err.OwnedSourceFailure.Segment);
                    StartingOwnedError(err,FerruleOwnedSourceErrorKind.UniteratedRepetition,hasOwner:true,path:new[]{"Label"},segment:0);
                }
                var members = view.WalkOwnedSourceCohort(OwnedPrimaryOwner, empty);
                for (var i = 0; i < members.Count; i++)
                {
                    var m = members[i].Context; Equal(false, m.OwnedView.FrameAt(m.OwnedView.FrameCount - 1).GroupMembers);
                    Equal<nuint>((nuint)i + 1, m.OwnedPosition(OwnedPrimaryOwner, empty));
                    Equal(m.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Label" }), m.ResolveOwnedFieldWithRootPin(OwnedPrimaryOwner, new[] { "Label" }, empty, false));
                    Equal(1, m.WalkOwnedSourceCohort(OwnedPrimaryOwner, empty).Count);
                }
                var other = view.WalkOwnedSourceCohort(OwnedNamedOwner("Alias"), empty)[0].Context;
                var foreignError = (FerruleRuntimeException)AdjFailure("G443.foreign-item-no-shadow", () => other.ResolveOwnedFieldWithRootPin(OwnedPrimaryOwner, new[] { "Label" }, empty, false));
                Equal(FerruleOwnedSourceErrorKind.UniteratedRepetition, foreignError.OwnedSourceFailure!.Error);
                StartingOwnedError(foreignError,FerruleOwnedSourceErrorKind.UniteratedRepetition,hasOwner:true,path:new[]{"Label"},segment:0);
                var nestedLog=new List<string>();var nested=new StartingControls {Description=new(new[]{false},false,0,false,false,false),
                    Key=(_,_)=>{nestedLog.Add("s00");return FerruleValue.FromInt64(0);},Start=_=>{nestedLog.Add("b0");return false;} };
                var outerPosition=view.OwnedPosition(OwnedPrimaryOwner,empty);
                var outerOrigin=view.OriginCount-1;var outerCollection=view.LegacyView.BasicCollectionCount-1;
                var inner=view.MapGroupStartingWithSourceCohort(view.WalkOwnedSourceCohort(OwnedNamedOwner("Alias"),empty),nested,h=>
                {
                    nestedLog.Add("t0");Equal(true,h.OwnedView.FrameAt(0).GroupMembers);
                    Equal(BasicOriginKind.GroupMembers,h.OriginAt(outerOrigin).Kind);
                    Equal(true,h.LegacyView.BasicCollectionAt(outerCollection).BasicGroupMembers);
                    Equal(outerPosition,h.OwnedPosition(OwnedPrimaryOwner,empty));Equal<nuint>(1,h.OwnedPosition(OwnedNamedOwner("Alias"),empty));
                    Equal(BasicOriginKind.GroupMembers,h.OriginAt(h.OriginCount-1).Kind);
                    return h.LegacyView.Frames[^1];
                });
                AdjExpect("G443.sorted-named-prefix-preservation",inner,Repeated(ByRow("unused","foreign",true,0)));
                Equal("s00,b0,t0",string.Join(',',nestedLog));nested.Counts(1,0,1,0,1,0);
                foreach(var absent in new[]{"Absent","Missing"})
                {
                    var error=(FerruleRuntimeException)AdjFailure("G443.owner-before-root-pin",()=>view.ResolveOwnedFieldWithRootPin(OwnedNamedOwner(absent),new[]{"Label"},empty,false));
                    Equal(absent,error.OwnedSourceFailure!.RequestedName);
                    Equal(absent=="Absent"?FerruleOwnedSourceErrorKind.SourceUnavailable:FerruleOwnedSourceErrorKind.UnknownOwner,error.OwnedSourceFailure.Error);
                    StartingOwnedError(error,absent=="Absent"?FerruleOwnedSourceErrorKind.SourceUnavailable:FerruleOwnedSourceErrorKind.UnknownOwner,
                        hasOwner:true,ownerName:absent,requestedName:absent);
                }
            }
            return Group(Field("First", Scalar(x.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Label" }))),
                Field("Members", Repeated(x.WalkOwnedSourceCohort(OwnedPrimaryOwner, empty).Select(m => Scalar(m.Context.ResolveOwnedField(OwnedPrimaryOwner, new[] { "Label" }))).ToArray())));
        });
        AdjExpect("G443.root-members", output,
            Group(Field("First", Scalar(Text("a"))), Field("Members", Repeated(Scalar(Text("a"))))),
            Group(Field("First", Scalar(Text("b"))), Field("Members", Repeated(Scalar(Text("b"))))),
            Group(Field("First", Scalar(Text("c"))), Field("Members", Repeated(Scalar(Text("c")), Scalar(Text("d"))))));
        c.Counts(1, 0, 0, 0, 4, 0);
        Equal("b0,b1,b2,b3,t0,t1,t2",string.Join(',',rootLog));
    }
    private static void BasicOwnedStartingR16OldProfiles()
    {
        // Actual old operations, with their own literal errors/UTF8 frontiers.
        BasicOwnedByCumulativeSortAndGroupUtf8();
        BasicOwnedAdjacentCumulativeUtf8Frontier();
        // Independent literal copies of the old returned-key frontiers. Do not
        // obtain a threshold from Seed/Footprint or substitute Starting here.
        foreach(var adjacent in new[]{false,true})
        foreach(var scenario in new[]{(Key:"é",Short:(nuint)217,Exact:(nuint)218,Used:(nuint)216,Width:(nuint)2),
            (Key:"💠",Short:(nuint)221,Exact:(nuint)222,Used:(nuint)218,Width:(nuint)4)})
        foreach(var bytes in new[]{scenario.Short,scenario.Exact})
        {
            var root=ByAccountingRoot(bytes);var original=root.WalkSourceCohort(new[]{"Ω","巣","行"});var events=new List<string>();var targets=0;var posts=0;
            var controls=new ByControls {Description=new(new[]{false,false},false,1,true,false,true),
                Key=(k,_)=>{events.Add("s"+k);return Text("é");},Bound=(_,_)=>{events.Add("w");return FerruleSequenceWindow.First(10);},
                Group=_=>{events.Add("g");return Text(scenario.Key);},Post=_=>{events.Add("p");posts++;return true;}};
            FerruleInstance Target(FerruleBasicScopeContext c){events.Add("t");targets++;return StartingAccountingOutput(c);}
            IReadOnlyList<FerruleInstance> Run()=>adjacent?root.MapGroupAdjacentBySourceCohort(original,controls,Target):root.MapGroupBySourceCohort(original,controls,Target);
            if(bytes==scenario.Short)
            {
                StartingBridge("G443.R16-old-key-return",()=>Run(),FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit,
                    FerruleBasicSourceBridgeComponent.MetadataBytes,scenario.Used,scenario.Width,scenario.Short);
                Equal("s0,s1,s0,s1,w,g,p,g",string.Join(',',events));Equal(1,posts);Equal(0,targets);Equal(8,controls.SemanticCalls);
            }
            else
            {
                AdjExpect("G443.R16-old-key-exact",BasicReached("G443.R16-complete-subject",Run),StartingAccountingExpected());
                Equal("s0,s1,s0,s1,w,g,p,g,p,t",string.Join(',',events));Equal(2,posts);Equal(1,targets);Equal(9,controls.SemanticCalls);
            }
            Equal(1,controls.MetadataCalls);
        }
    }
}
#endif

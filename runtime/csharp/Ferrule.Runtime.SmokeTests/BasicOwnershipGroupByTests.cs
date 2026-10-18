using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private sealed class ByDirections(int count) : IReadOnlyList<bool>
    {
        internal int CountReads, IndexReads;
        internal bool Descending;
        public int Count { get { CountReads++; return count; } }
        public bool this[int index] { get { IndexReads++; return Descending; } }
        public IEnumerator<bool> GetEnumerator() => throw new InvalidOperationException("Directions must not be enumerated.");
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
    }
    private sealed class ByControls : IFerruleBasicGroupByControls
    {
        internal FerruleBasicGroupByMetadata Description = new(Array.Empty<bool>(), false, 0, false, false, false);
        internal int MetadataCalls, SemanticCalls;
        internal Func<FerruleBasicScopeContext, bool> Predicate = _ => true, Post = _ => true;
        internal Func<int, FerruleBasicScopeContext, FerruleValue> Key = (_, _) => FerruleValue.FromInt64(0);
        internal Func<int, FerruleBasicScopeContext, FerruleSequenceWindow> Bound = (_, _) => FerruleSequenceWindow.First(ulong.MaxValue);
        internal Func<FerruleBasicScopeContext, FerruleValue> Group = _ => FerruleValue.FromInt64(0);
        public FerruleBasicGroupByMetadata Metadata() { MetadataCalls++; return Description; }
        public bool Filter(FerruleBasicScopeContext context) { SemanticCalls++; return Predicate(context); }
        public FerruleValue SortKey(int key, FerruleBasicScopeContext context) { SemanticCalls++; return Key(key, context); }
        public FerruleSequenceWindow Window(int bound, FerruleBasicScopeContext parent) { SemanticCalls++; return Bound(bound, parent); }
        public FerruleValue GroupKey(FerruleBasicScopeContext context) { SemanticCalls++; return Group(context); }
        public bool RetainMember(FerruleBasicScopeContext context) { SemanticCalls++; return Post(context); }
    }
    private static FerruleGroup ByRow(string key, string label, bool keep, int rank) => Group(
        Field("Key", Scalar(Text(key))), Field("Label", Scalar(Text(label))),
        Field("Keep", Scalar(FerruleValue.FromBoolean(keep))), Field("Rank", Scalar(FerruleValue.FromInt64(rank))));
    private static FerruleGroup BySource(string prefix) => Group(Field("Marker", Scalar(Text(prefix + ".root"))),
        Field("Rows", Repeated(ByRow("A", prefix + "-a1", false, 30), ByRow("B", prefix + "-b1", true, 10),
            ByRow("A", prefix + "-a2", true, 20), ByRow("C", prefix + "-c1", false, 40), ByRow("B", prefix + "-b2", false, 15))));
    private static FerruleValue ByRead(FerruleBasicScopeContext c, FerruleOwnedSourceOwner owner, string field) =>
        c.ResolveOwnedField(owner, new[] { "Rows", field });
    private static ByControls ByKey(FerruleOwnedSourceOwner owner) => new() { Group = c => ByRead(c, owner, "Key") };
    private static Exception ByFailure(string id, Action subject)
    {
        try { subject(); }
        catch (Exception e) { BasicErrorRaw(id, e, new { classification = "group_by_failure_before_assertion" }); return e; }
        throw new InvalidOperationException("Expected group-by refusal.");
    }
    private static FerruleBasicSourceBridgeException ByBridge(Exception error, FerruleBasicSourceBridgeErrorKind kind,
        FerruleBasicSourceBridgeComponent? component = null)
    {
        var outer = error as FerruleRuntimeException;
        var cause = outer?.BasicSourceBridgeFailure ?? error as FerruleBasicSourceBridgeException;
        Equal(true, cause is not null); Equal(kind, cause!.Error); Equal(component, cause.Component);
        if (outer is not null) { Equal(FerruleRuntimeError.BasicSourceBridge, outer.Error); Equal(true, ReferenceEquals(cause, outer.InnerException)); }
        return cause;
    }
    private static FerruleInstance ByOutput(FerruleBasicScopeContext c, FerruleOwnedSourceOwner owner, bool legacyMembers)
    {
        var members = legacyMembers ? c.WalkSourceCohort(Array.Empty<string>()) : c.WalkOwnedSourceCohort(owner, new[] { "Rows" });
        return Group(Field("First", Scalar(ByRead(c, owner, "Label"))),
            Field("GroupPosition", Scalar(FerruleValue.FromInt64((long)c.OwnedPosition(owner, new[] { "Rows" })))),
            Field("RootMarker", Scalar(c.ResolveOwnedFieldWithRootPin(owner, new[] { "Marker" }, Array.Empty<string>(), true))),
            Field("Members", Repeated(members.Select(member => Group(
                Field("Label", Scalar(ByRead(member.Context, owner, "Label"))),
                Field("MemberPosition", Scalar(FerruleValue.FromInt64((long)member.Context.OwnedPosition(owner, new[] { "Rows" })))),
                Field("LegacyPosition", Scalar(FerruleValue.FromInt64(member.Context.LegacyPosition(new[] { "Rows" })))))).ToArray())));
    }
    private static string ByField(FerruleInstance result, string field) =>
        ((FerruleScalar)((FerruleGroup)result).Fields.Single(f => f.Name == field).Value).Value.StringValue;

    private static void BasicOwnedByRepresentativesAndMembers()
    {
        var root = BasicRoot(BySource("P"), new("Rows", BySource("N")), new(" Røws💠 ", BySource("U")));
        foreach (var owner in new[] { OwnedPrimaryOwner, OwnedNamedOwner("Rows"), OwnedNamedOwner(" Røws💠 ") })
        foreach (var legacyMembers in new[] { false, true })
        {
            var controls = ByKey(owner); var contexts = new List<FerruleBasicScopeContext>();
            controls.Description = controls.Description with { HasPostFilter = true };
            var posts = 0; controls.Post = c => { posts++; return ByRead(c, owner, "Keep").BooleanValue; };
            var original = root.WalkOwnedSourceCohort(owner, new[] { "Rows" });
            var result = root.MapGroupBySourceCohort(original, controls,
                c => { contexts.Add(c); return ByOutput(c, owner, legacyMembers); });
            OwnedRaw("G441.representative-members", new { owner = owner.Name, legacyMembers,
                input = BasicContextSnapshot(root), actual = result.Select(BasicInstanceSnapshot).ToArray(), posts });
            Equal(5, posts); Equal(2, result.Count); Equal(1, controls.MetadataCalls);
            var prefix = owner.Name is null ? "P" : owner.Name == "Rows" ? "N" : "U";
            Equal(prefix + "-a1", ByField(result[0], "First")); Equal(prefix + "-b1", ByField(result[1], "First"));
            Equal(prefix + ".root", ByField(result[0], "RootMarker"));
            for (var i = 0; i < contexts.Count; i++)
            {
                var c = contexts[i]; Equal<nuint>((nuint)i + 1, c.OwnedPosition(owner, new[] { "Rows" }));
                Equal((long)i + 1, c.LegacyPosition(new[] { "Rows" }));
                var children = c.WalkOwnedSourceCohort(owner, new[] { "Rows" }); Equal(2, children.Count);
                Equal<nuint>(1, children[0].Context.OwnedPosition(owner, new[] { "Rows" }));
                Equal<nuint>(2, children[1].Context.OwnedPosition(owner, new[] { "Rows" }));
                Equal(true, ReferenceEquals(root.LegacyView.BasicRunStateIdentity, c.LegacyView.BasicRunStateIdentity));
            }
            // Retaining earlier target contexts cannot observe later group publication.
            Equal(prefix + "-a1", ByRead(contexts[0], owner, "Label").StringValue);
            Equal<nuint>(1, contexts[0].OwnedPosition(owner, new[] { "Rows" }));
            Equal(prefix + "-a2", ByRead(contexts[0].WalkSourceCohort(Array.Empty<string>())[1].Context, owner, "Label").StringValue);
            var other = owner.Name is null ? OwnedNamedOwner("Rows") : OwnedPrimaryOwner;
            Equal(Text(owner.Name is null ? "N.root" : "P.root"), contexts[0].ResolveOwnedFieldWithRootPin(other,
                new[] { "Marker" }, Array.Empty<string>(), true));
            var inactive = (FerruleRuntimeException)ByFailure("G441.other-owner-remains-inactive", () => contexts[0].OwnedPosition(other, new[] { "Rows" }));
            Equal(FerruleOwnedSourceErrorKind.InactiveSourceFrame, inactive.OwnedSourceFailure!.Error);
            var repeated = (FerruleRuntimeException)ByFailure("G441.other-owner-not-representative", () => ByRead(contexts[0], other, "Label"));
            Equal(FerruleOwnedSourceErrorKind.UniteratedRepetition, repeated.OwnedSourceFailure!.Error);
        }
    }

    private static void BasicOwnedByPhasesAndErrorOrder()
    {
        var owner = OwnedPrimaryOwner; var rows = new[] { "Rows" }; var root = BasicRoot(BySource("P"));
        foreach (var before in new[] { true, false })
        {
            var events = new List<string>(); var directions = new ByDirections(1); var held = new List<FerruleBasicScopeContext>();
            var controls = ByKey(owner);
            controls.Description = new(directions, before, 1, false, true, true);
            controls.Predicate = c => { events.Add("f" + c.OwnedPosition(owner, rows)); held.Add(c); return c.OwnedPosition(owner, rows) > 1; };
            controls.Key = (_, c) => { events.Add("s" + ByRead(c, owner, "Label").StringValue); directions.Descending = true; return ByRead(c, owner, "Rank"); };
            controls.Bound = (_, parent) => { Equal(true, ReferenceEquals(root, parent)); events.Add("w"); return FerruleSequenceWindow.First(10); };
            controls.Group = c => { events.Add("g" + ByRead(c, owner, "Label").StringValue); return ByRead(c, owner, "Key"); };
            controls.Post = c => { events.Add("p" + ByRead(c, owner, "Label").StringValue); return ByRead(c, owner, "Keep").BooleanValue; };
            var result = root.MapGroupBySourceCohort(root.WalkOwnedSourceCohort(owner, rows), controls, c => ByOutput(c, owner, false));
            OwnedRaw("G441.phase", new { before, events, result = result.Select(BasicInstanceSnapshot).ToArray() });
            Equal(before ? 2 : 1, result.Count); Equal(before ? "P-b1" : "P-a2", ByField(result[0], "First"));
            if (before) Equal("P-a2", ByField(result[1], "First"));
            Equal(1, directions.CountReads); Equal(1, directions.IndexReads);
            Equal(before ? "f1,f2,f3,f4,f5,sP-b1,sP-a2,sP-c1,sP-b2,w,gP-b1,pP-b1,gP-b2,pP-b2,gP-a2,pP-a2,gP-c1,pP-c1" :
                "sP-a1,sP-b1,sP-a2,sP-c1,sP-b2,w,f1,f2,gP-b2,pP-b2,f3,gP-a2,pP-a2,f4,gP-a1,pP-a1,f5,gP-c1,pP-c1", string.Join(',', events));
            Equal<nuint>(1, held[0].OwnedPosition(owner, rows));
        }
        var trace = new List<string>(); var plain = ByKey(owner);
        plain.Description = plain.Description with { HasFilter = true, HasPostFilter = true, ExplicitWindowCount = 1 };
        plain.Bound = (_, _) => { trace.Add("w"); return FerruleSequenceWindow.First(0); };
        plain.Predicate = c => { trace.Add("f" + c.OwnedPosition(owner, rows)); return true; };
        plain.Group = c => { trace.Add("g" + c.OwnedPosition(owner, rows)); return ByRead(c, owner, "Key"); };
        var marker = new FerruleRuntimeException(FerruleRuntimeError.MappingException, "post marker", node: 441);
        plain.Post = c => { trace.Add("p" + c.OwnedPosition(owner, rows)); if (c.OwnedPosition(owner, rows) == 2) throw marker; return true; };
        var error = ByFailure("G441.post-before-next-filter-and-first-zero", () => root.MapGroupBySourceCohort(
            root.WalkOwnedSourceCohort(owner, rows), plain, _ => throw new InvalidOperationException("No target")));
        Equal(true, ReferenceEquals(marker, error)); Equal("w,f1,g1,p1,f2,g2,p2", string.Join(',', trace));
        plain.Post = c => { if (c.OwnedPosition(owner, rows) == 3) throw marker; return true; };
        error = ByFailure("G441.no-short-circuit-true-bucket", () => root.MapGroupBySourceCohort(
            root.WalkOwnedSourceCohort(owner, rows), plain, _ => throw new InvalidOperationException("No target")));
        Equal(true, ReferenceEquals(marker, error));
        plain.Post = _ => true; plain.Group = _ => throw marker;
        error = ByFailure("G441.key-before-post", () => root.MapGroupBySourceCohort(root.WalkOwnedSourceCohort(owner, rows), plain, _ => Group()));
        Equal(true, ReferenceEquals(marker, error));
        plain.Bound = (_, _) => throw marker; plain.Group = _ => throw new InvalidOperationException("Key must not run");
        error = ByFailure("G441.bound-before-key", () => root.MapGroupBySourceCohort(root.WalkOwnedSourceCohort(owner, rows), plain, _ => Group()));
        Equal(true, ReferenceEquals(marker, error));
        plain.Description = plain.Description with { Descending = new[] { false } }; plain.Key = (_, _) => throw marker;
        plain.Bound = (_, _) => throw new InvalidOperationException("Bound must not run");
        error = ByFailure("G441.sort-before-bound", () => root.MapGroupBySourceCohort(root.WalkOwnedSourceCohort(owner, rows), plain, _ => Group()));
        Equal(true, ReferenceEquals(marker, error));
        plain.Description = plain.Description with { FilterBeforeSort = true }; plain.Predicate = _ => throw marker;
        plain.Key = (_, _) => throw new InvalidOperationException("Sort must not run");
        error = ByFailure("G441.prefilter-before-sort", () => root.MapGroupBySourceCohort(root.WalkOwnedSourceCohort(owner, rows), plain, _ => Group()));
        Equal(true, ReferenceEquals(marker, error));
    }

    private static void BasicOwnedByGroupWindowsAndRawGaps()
    {
        var owner = OwnedPrimaryOwner; var rows = new[] { "Rows" }; var root = BasicRoot(BySource("P"));
        var cases = new (FerruleSequenceWindow Window, string[] Labels)[]
        {
            (FerruleSequenceWindow.SkipFirst(1), new[] { "P-b1", "P-c1" }),
            (FerruleSequenceWindow.First(2), new[] { "P-a1", "P-b1" }),
            (FerruleSequenceWindow.From(2), new[] { "P-b1", "P-c1" }),
            (FerruleSequenceWindow.FromTo(2, 2), new[] { "P-b1" }),
            (FerruleSequenceWindow.Last(1), new[] { "P-c1" }),
            (FerruleSequenceWindow.SkipFirst(ulong.MaxValue), Array.Empty<string>()),
        };
        foreach (var test in cases)
        {
            var controls = ByKey(owner); controls.Description = controls.Description with { ExplicitWindowCount = 1 };
            controls.Bound = (_, parent) => { Equal(true, ReferenceEquals(root, parent)); return test.Window; };
            var positions = new List<nuint>();
            var result = root.MapGroupBySourceCohort(root.WalkOwnedSourceCohort(owner, rows), controls,
                c => { positions.Add(c.OwnedPosition(owner, rows)); return Scalar(ByRead(c, owner, "Label")); });
            Equal(test.Labels.Length, result.Count);
            for (var i = 0; i < result.Count; i++)
            { Equal(test.Labels[i], ((FerruleScalar)result[i]).Value.StringValue); Equal<nuint>((nuint)i + 1, positions[i]); }
        }
        var ordered = ByKey(owner); ordered.Description = ordered.Description with { ExplicitWindowCount = 2, ImplicitFirst = true };
        ordered.Bound = (i, _) => i == 0 ? FerruleSequenceWindow.SkipFirst(1) : FerruleSequenceWindow.Last(2);
        var selected = root.MapGroupBySourceCohort(root.WalkOwnedSourceCohort(owner, rows), ordered, c => Scalar(ByRead(c, owner, "Label")));
        Equal(1, selected.Count); Equal(Text("P-b1"), ((FerruleScalar)selected[0]).Value);
        var gaps = BasicRoot(Group(Field("Rows", Repeated(Scalar(FerruleValue.Null), BasicValue("raw2"), Scalar(FerruleValue.JsonNull), BasicValue("raw4")))));
        var observed = new List<nuint>(); var gapControls = new ByControls();
        gapControls.Group = c => { var position = c.OwnedPosition(owner, rows); observed.Add(position); return FerruleValue.FromInt64((long)position); };
        var gapOriginal = gaps.WalkOwnedSourceCohort(owner, rows);
        var output = gaps.MapGroupBySourceCohort(gapOriginal, gapControls, c =>
        {
            var clone = c.CloneView(); Equal(c.OwnedPosition(owner, rows), clone.OwnedPosition(owner, rows));
            Equal(1, c.WalkSourceCohort(Array.Empty<string>()).Count); return Group();
        });
        Equal(2, output.Count); Equal(true, observed.SequenceEqual(new nuint[] { 2, 4 }));
        Equal<nuint?>(2, gapOriginal[0].RawSourceOrdinal); Equal<nuint?>(4, gapOriginal[1].RawSourceOrdinal);
        var stable = ByKey(owner); stable.Description = stable.Description with { Descending = new[] { false } };
        var stableResults = root.MapGroupBySourceCohort(root.WalkOwnedSourceCohort(owner, rows), stable, c => Scalar(ByRead(c, owner, "Label")));
        Equal(Text("P-a1"), ((FerruleScalar)stableResults[0]).Value); Equal(Text("P-b1"), ((FerruleScalar)stableResults[1]).Value);
    }

    private static void BasicOwnedByRootIdentityAliasesAndTags()
    {
        var owner = OwnedPrimaryOwner; var empty = Array.Empty<string>();
        var single = Group(Field("Marker", Scalar(Text("root"))));
        var root = BasicRoot(single, new FerruleDeclaredNamedSourceRoot("Named", BasicValue("named-root")));
        var original = root.WalkOwnedSourceCohort(owner, empty);
        var result = root.MapGroupBySourceCohort(original, new ByControls(), c =>
        {
            Equal<nuint>(1, c.OwnedPosition(owner, empty));
            Equal(Text("root"), c.ResolveOwnedFieldWithRootPin(owner, new[] { "Marker" }, empty, true));
            var members = c.WalkOwnedSourceCohort(owner, empty); Equal(1, members.Count);
            Equal<nuint>(1, members[0].Context.OwnedPosition(owner, empty));
            Equal(true, ReferenceEquals(single, members[0].GroupTerminal!.Value.Item)); return c.LegacyView.Frames[^1];
        });
        Equal(1, result.Count);
        var named = OwnedNamedOwner("Named");
        foreach (var legacyAlias in new[] { false, true })
        {
            var cohort = legacyAlias ? root.WalkSourceCohort(new[] { "Named" }) : root.WalkOwnedSourceCohort(named, empty);
            root.MapGroupBySourceCohort(cohort, new ByControls(), c =>
            {
                Equal(Text("named-root"), c.ResolveOwnedField(named, new[] { "Value" }));
                var children = c.WalkSourceCohort(empty); Equal(1, children.Count);
                Equal(Text("named-root"), children[0].Context.ResolveOwnedField(named, new[] { "Value" }));
                Equal<nuint>(1, children[0].Context.OwnedPosition(named, empty));
                if (legacyAlias) Equal(1, c.WalkSourceCohort(new[] { "Named" }).Count);
                return Group();
            });
        }
        var aliasLimits = new FerruleOwnedSourceLimits(2, 2, 0, 4096, 1);
        var aliasRoot = FerruleBasicSourceRoots.Create(single, new[] { new FerruleDeclaredNamedSourceRoot("Named", single) }, aliasLimits).RootContext();
        var aliasOriginal = aliasRoot.WalkSourceCohort(new[] { "Named" });
        Equal(0, aliasOriginal[0].GroupTerminal!.Value.Canonical.Count); Equal(1, aliasOriginal[0].GroupTerminal!.Value.RawCount);
        Equal(1, aliasRoot.MapGroupBySourceCohort(aliasOriginal, new ByControls(), _ => Group()).Count);
        var namedRepeated = FerruleBasicSourceRoots.Create(single,
            new[] { new FerruleDeclaredNamedSourceRoot("Named", Repeated(Scalar(FerruleValue.FromInt64(7)), Scalar(FerruleValue.FromInt64(8)))) },
            aliasLimits with { MaxWalkContexts = 2 }).RootContext();
        namedRepeated.MapGroupBySourceCohort(namedRepeated.WalkSourceCohort(new[] { "Named" }), new ByControls(), c =>
        {
            Equal(FerruleValue.FromInt64(7), c.ResolveOwnedField(named, empty));
            var members = c.WalkSourceCohort(empty); Equal(2, members.Count);
            Equal(FerruleValue.FromInt64(8), members[1].Context.ResolveOwnedField(named, empty));
            Equal<nuint>(2, members[1].Context.OwnedPosition(named, empty)); return Group();
        });
        var rowRoot = BasicRoot(BySource("P")); var row = rowRoot.WalkOwnedSourceCohort(owner, new[] { "Rows" })[2].Context;
        var identity = row.WalkOwnedSourceCohort(owner, new[] { "Rows" });
        Equal(false, identity[0].HasNewCursor);
        row.MapGroupBySourceCohort(identity, ByKey(owner), c =>
        {
            Equal(2, c.OwnedView.FrameCount); Equal<nuint>(3, c.OwnedView.FrameAt(0).Index);
            Equal<nuint>(1, c.OwnedPosition(owner, new[] { "Rows" }));
            Equal(1, c.WalkOwnedSourceCohort(owner, new[] { "Rows" }).Count); return Group();
        });
        FerruleValue[] keys = [FerruleValue.FromInt64(1), Text("1"), FerruleValue.FromInt64(1), FerruleValue.FromDouble(1),
            FerruleValue.Null, FerruleValue.JsonNull, FerruleValue.FromDouble(0), FerruleValue.FromDouble(-0.0),
            FerruleValue.FromDouble(double.NaN), FerruleValue.FromDouble(double.NaN)];
        var scalarRoot = BasicRoot(Repeated(keys.Select(Scalar).ToArray()));
        // Owned iteration prunes Null/JsonNull payloads. Keys below are evaluated
        // against non-null ordinary rows so both null key TAGS remain observable.
        var tagRoot = BasicRoot(Group(Field("Rows", Repeated(keys.Select((_, i) => BasicValue(i.ToString())).ToArray()))));
        var tags = new ByControls { Group = c => keys[(int)c.OwnedPosition(owner, new[] { "Rows" }) - 1] };
        var grouped = tagRoot.MapGroupBySourceCohort(tagRoot.WalkOwnedSourceCohort(owner, new[] { "Rows" }), tags, c => c.LegacyView.Frames[^1]);
        Equal(8, grouped.Count); Equal(2, ((FerruleRepeated)grouped[0]).Items.Count); Equal(2, ((FerruleRepeated)grouped[5]).Items.Count);
        var scalarGroups = scalarRoot.MapGroupBySourceCohort(scalarRoot.WalkOwnedSourceCohort(owner, empty),
            new ByControls { Group = c => c.ResolveOwnedField(owner, empty) }, c => c.LegacyView.Frames[^1]);
        Equal(6, scalarGroups.Count);
        var nested = BasicRoot(Group(Field("Rows", Repeated(Group(Field("Nested", Repeated(Scalar(Text("x")))))))));
        nested.MapGroupBySourceCohort(nested.WalkOwnedSourceCohort(owner, new[] { "Rows" }), new ByControls(), c =>
        {
            var refusal = ByFailure("G441.strict-descendant-repeated", () => c.ResolveOwnedField(owner, new[] { "Rows", "Nested" }));
            Equal(FerruleOwnedSourceErrorKind.UniteratedRepetition, ((FerruleRuntimeException)refusal).OwnedSourceFailure!.Error); return Group();
        });
    }

    private static void BasicOwnedByNestedOwnersAndPrefixes()
    {
        var owner = OwnedPrimaryOwner; var named = OwnedNamedOwner("Rows"); var rows = new[] { "Rows" };
        var root = BasicRoot(BySource("P"), new FerruleDeclaredNamedSourceRoot("Rows", BySource("N")));
        root.MapGroupBySourceCohort(root.WalkOwnedSourceCohort(owner, rows), ByKey(owner), outer =>
        {
            var ordinal = outer.OwnedPosition(owner, rows); var label = ByRead(outer, owner, "Label");
            var nested = outer.MapGroupBySourceCohort(outer.WalkOwnedSourceCohort(named, rows), ByKey(named), inner =>
            {
                Equal(ordinal, inner.OwnedPosition(owner, rows)); Equal(label, ByRead(inner, owner, "Label"));
                var members = inner.WalkOwnedSourceCohort(named, rows);
                for (var i = 0; i < members.Count; i++)
                { Equal(ordinal, members[i].Context.OwnedPosition(owner, rows)); Equal<nuint>((nuint)i + 1, members[i].Context.OwnedPosition(named, rows)); }
                return Group();
            });
            Equal(3, nested.Count);
            var regroup = outer.MapGroupBySourceCohort(outer.WalkOwnedSourceCohort(owner, rows), ByKey(owner), c => c.LegacyView.Frames[^1]);
            Equal(1, regroup.Count); return Group();
        });
        var batchSource = Group(Field("Batch", Repeated(
            Group(Field("Id", Scalar(Text("first"))), Field("Rows", Repeated(ByRow("A", "a1", true, 1)))),
            Group(Field("Id", Scalar(Text("second"))), Field("Rows", Repeated(ByRow("A", "a2", true, 2)))))));
        var batches = BasicRoot(batchSource); var path = new[] { "Batch", "Rows" };
        var controls = new ByControls { Group = c => c.ResolveOwnedField(owner, new[] { "Batch", "Rows", "Key" }) };
        var retained = new List<FerruleBasicScopeContext>();
        batches.MapGroupBySourceCohort(batches.WalkOwnedSourceCohort(owner, path), controls, c =>
        {
            retained.Add(c); Equal(Text("first"), c.ResolveOwnedField(owner, new[] { "Batch", "Id" }));
            var members = c.WalkSourceCohort(Array.Empty<string>()); Equal(2, members.Count);
            for (var i = 0; i < members.Count; i++)
            {
                Equal(Text("first"), members[i].Context.ResolveOwnedField(owner, new[] { "Batch", "Id" }));
                Equal<nuint>(1, members[i].Context.OwnedPosition(owner, new[] { "Batch" }));
                Equal<nuint>((nuint)i + 1, members[i].Context.OwnedPosition(owner, path));
            }
            return Group();
        });
        Equal(Text("first"), retained[0].ResolveOwnedField(owner, new[] { "Batch", "Id" }));
        var perBatch = BasicRoot(Group(Field("Batch", Repeated(
            Group(Field("Limit", Scalar(FerruleValue.FromInt64(1))), Field("Rows", Repeated(ByRow("A", "a", true, 1), ByRow("B", "b", true, 2)))),
            Group(Field("Limit", Scalar(FerruleValue.FromInt64(2))), Field("Rows", Repeated(ByRow("A", "a", true, 1), ByRow("B", "b", true, 2))))))));
        var parents = perBatch.WalkOwnedSourceCohort(owner, new[] { "Batch" });
        for (var i = 0; i < parents.Count; i++)
        {
            var parent = parents[i].Context; var bounded = new ByControls();
            bounded.Description = bounded.Description with { ExplicitWindowCount = 1 };
            bounded.Group = c => c.ResolveOwnedField(owner, new[] { "Batch", "Rows", "Key" });
            bounded.Bound = (_, receiving) =>
            {
                Equal(true, ReferenceEquals(parent, receiving));
                return FerruleSequenceWindow.First((ulong)receiving.ResolveOwnedField(owner, new[] { "Batch", "Limit" }).Int64Value);
            };
            Equal(i + 1, parent.MapGroupBySourceCohort(parent.WalkOwnedSourceCohort(owner, path), bounded, _ => Group()).Count);
        }
    }

    private static void BasicOwnedByExactBudgetsAndUtf8()
    {
        var owner = OwnedPrimaryOwner; var rows = new[] { "Rows" };
        var limits = new FerruleOwnedSourceLimits(1, 2, 2, 1_048_576, 2);
        var source = Group(Field("Rows", Repeated(BasicValue("one"), BasicValue("two"))));
        var root = FerruleBasicSourceRoots.Create(source, Array.Empty<FerruleDeclaredNamedSourceRoot>(), limits).RootContext();
        var original = root.WalkOwnedSourceCohort(owner, rows); var simple = new ByControls();
        var seed = FerruleBasicGroupByOperation.Seed(root, original);
        var fixedCost = FerruleBasicGroupByOperation.Footprint(original, simple.Description, 0, out _, out _);
        var known = seed.Bytes.Used + fixedCost.Bytes;
        var sanityLimits = new FerruleOwnedSourceLimits(1, 1, 0, 0, 1);
        var sanity = FerruleBasicSourceRoots.Create(Group(), Array.Empty<FerruleDeclaredNamedSourceRoot>(), sanityLimits).RootContext();
        var sanityRows = sanity.WalkOwnedSourceCohort(owner, Array.Empty<string>());
        var sanityCost = FerruleBasicGroupByOperation.Footprint(sanityRows, simple.Description, 0, out _, out _);
        Equal<nuint>(28, sanityCost.Slots); Equal<nuint>(0, sanityCost.Bytes);
        // The unchanged incoming D1 footprint is 28. Two ledger setup objects
        // now make Seed=30, total actual=58 and introduced delta=30, not 28.
        var sanitySeed = FerruleBasicGroupByOperation.Seed(sanity, sanityRows);
        Equal<nuint>(30, sanitySeed.Slots.Used); Equal<nuint>(58, sanitySeed.Slots.Used + sanityCost.Slots);
        Equal<nuint>(64, BasicGroupAccounting.Maximum(sanityLimits) - BasicSourceAccounting.Maximum(sanityLimits, true));
        Equal(1, sanity.MapGroupBySourceCohort(sanityRows, new ByControls(), _ => Group()).Count);
        OwnedRaw("G441.r2-footprint", new { seedSlots = (ulong)seed.Slots.Used, seedBytes = (ulong)seed.Bytes.Used,
            slots = (ulong)fixedCost.Slots, bytes = (ulong)fixedCost.Bytes, maximum = (ulong)BasicGroupAccounting.Maximum(limits) });
        FerruleBasicScopeContext AtBytes(nuint bytes) => FerruleBasicSourceRoots.Create(source,
            Array.Empty<FerruleDeclaredNamedSourceRoot>(), limits with { MaxMetadataBytes = bytes }).RootContext();
        var exact = AtBytes(known); var exactRows = exact.WalkOwnedSourceCohort(owner, rows);
        Equal(1, exact.MapGroupBySourceCohort(exactRows, new ByControls(), _ => Group()).Count);
        var shortRoot = AtBytes(known - 1); var shortRows = shortRoot.WalkOwnedSourceCohort(owner, rows); var calls = new ByControls();
        var failure = ByFailure("G441.known-byte-minus-one", () => shortRoot.MapGroupBySourceCohort(shortRows, calls, _ => Group()));
        var cause = ByBridge(failure, FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.MetadataBytes);
        Equal<nuint?>(seed.Bytes.Used, cause.Used); Equal<nuint?>(fixedCost.Bytes, cause.Requested); Equal<nuint?>(known - 1, cause.Maximum); Equal(0, calls.SemanticCalls);
        var late = AtBytes(known + 7); var lateRows = late.WalkOwnedSourceCohort(owner, rows); var posts = 0;
        var utf8 = new ByControls { Group = _ => Text("💠"), Post = _ => { posts++; return true; } };
        utf8.Description = utf8.Description with { HasPostFilter = true };
        failure = ByFailure("G441.duplicate-key-cumulative-UTF8", () => late.MapGroupBySourceCohort(lateRows, utf8, _ => Group()));
        cause = ByBridge(failure, FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.MetadataBytes);
        Equal<nuint?>(known + 4, cause.Used); Equal<nuint?>(4, cause.Requested); Equal<nuint?>(known + 7, cause.Maximum); Equal(1, posts);
        var fit = AtBytes(known + 8); var fitControls = new ByControls { Group = _ => Text("💠") };
        Equal(1, fit.MapGroupBySourceCohort(fit.WalkOwnedSourceCohort(owner, rows), fitControls, _ => Group()).Count);
        var tooMany = new ByDirections(100_000); calls = new ByControls(); calls.Description = calls.Description with { Descending = tooMany };
        failure = ByFailure("G441.slots-before-known-bytes", () => shortRoot.MapGroupBySourceCohort(shortRows, calls, _ => Group()));
        ByBridge(failure, FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.MetadataSlots);
        Equal(0, calls.SemanticCalls); Equal(1, tooMany.CountReads); Equal(0, tooMany.IndexReads);
        var emptyLimits = new FerruleOwnedSourceLimits(1, 0, 1, 4096, 0);
        var empty = FerruleBasicSourceRoots.Create(Group(Field("Rows", Repeated())), Array.Empty<FerruleDeclaredNamedSourceRoot>(), emptyLimits).RootContext();
        var emptyRows = empty.WalkOwnedSourceCohort(owner, rows); var emptySeed = FerruleBasicGroupByOperation.Seed(empty, emptyRows);
        var keyCount = checked((int)(BasicGroupAccounting.Maximum(emptyLimits) - emptySeed.Slots.Used - 3));
        var exactDirections = new ByDirections(keyCount); calls = new ByControls(); calls.Description = calls.Description with { Descending = exactDirections };
        Equal(0, empty.MapGroupBySourceCohort(emptyRows, calls, _ => Group()).Count); Equal(0, calls.SemanticCalls);
        Equal(1, exactDirections.CountReads); Equal(keyCount, exactDirections.IndexReads);
        var overDirections = new ByDirections(keyCount + 1); calls = new ByControls(); calls.Description = calls.Description with { Descending = overDirections };
        failure = ByFailure("G441.exact-slot-plus-one", () => empty.MapGroupBySourceCohort(emptyRows, calls, _ => Group()));
        cause = ByBridge(failure, FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.MetadataSlots);
        Equal<nuint?>(emptySeed.Slots.Used, cause.Used); Equal<nuint?>((nuint)keyCount + 4, cause.Requested); Equal<nuint?>(BasicGroupAccounting.Maximum(emptyLimits), cause.Maximum);
        Equal(0, overDirections.IndexReads);
        var d1Limits = new FerruleOwnedSourceLimits(1, 0, 0, 4096, 1);
        var d1 = FerruleBasicSourceRoots.Create(Group(), Array.Empty<FerruleDeclaredNamedSourceRoot>(), d1Limits).RootContext();
        calls = new ByControls(); failure = ByFailure("G441.D1-frame-admission", () => d1.MapGroupBySourceCohort(d1.WalkOwnedSourceCohort(owner, Array.Empty<string>()), calls, _ => Group()));
        ByBridge(failure, FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.ActiveFrames); Equal(0, calls.SemanticCalls);
    }

    private static void BasicOwnedByEmptyWitnessesAndCapacity()
    {
        var owner = OwnedPrimaryOwner; var rows = new[] { "Rows" }; var emptySource = Group(Field("Rows", Repeated()));
        var root = BasicRoot(emptySource); var original = root.WalkOwnedSourceCohort(owner, rows); var controls = new ByControls();
        controls.Description = controls.Description with { HasFilter = true, HasPostFilter = true, ExplicitWindowCount = 2, ImplicitFirst = true };
        var windows = 0; controls.Bound = (_, parent) => { Equal(true, ReferenceEquals(root, parent)); windows++; return FerruleSequenceWindow.First(0); };
        Equal(0, root.MapGroupBySourceCohort(original, controls, _ => throw new InvalidOperationException("Empty target")).Count); Equal(2, windows);
        var marker = new FerruleRuntimeException(FerruleRuntimeError.NotAnItemCount, "empty invalid bound", node: 441);
        controls.Bound = (_, _) => throw marker;
        Equal(true, ReferenceEquals(marker, ByFailure("G441.empty-bound-error", () => root.MapGroupBySourceCohort(original, controls, _ => Group()))));
        foreach (var foreign in new[] { root.CloneView(), BasicRoot(emptySource) })
        {
            var probe = new ByControls(); var failure = ByFailure("G441.empty-authentication", () => foreign.MapGroupBySourceCohort(original, probe, _ => Group()));
            ByBridge(failure, ReferenceEquals(root.Roots, foreign.Roots) ? FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness : FerruleBasicSourceBridgeErrorKind.RegistryMismatch);
            Equal(0, probe.MetadataCalls); Equal(0, probe.SemanticCalls);
        }
        foreach (var action in new Action[]
        {
            () => BasicGroupAccounting.Maximum(BasicLimits with { MaxPathSegments = nuint.MaxValue }),
            () => BasicGroupAccounting.Maximum(BasicLimits with { MaxWalkContexts = nuint.MaxValue }),
            () => BasicGroupAccounting.Capacity<BasicGroupTerminal>((nuint)Array.MaxLength + 1),
            () => BasicGroupAccounting.Capacity<FerruleValue>(nuint.MaxValue),
            () => BasicGroupAccounting.Sub(0, 1),
        }) ByBridge(ByFailure("G441.closed-layout-overflow", action), FerruleBasicSourceBridgeErrorKind.PairedMetadataArithmeticOverflow, FerruleBasicSourceBridgeComponent.MetadataSlots);
        var populated = BasicRoot(BySource("P")); var huge = new ByDirections(int.MaxValue); var capacity = new ByControls();
        capacity.Description = capacity.Description with { Descending = huge };
        ByBridge(ByFailure("G441.native-cache-capacity", () => populated.MapGroupBySourceCohort(populated.WalkOwnedSourceCohort(owner, rows), capacity, _ => Group())),
            FerruleBasicSourceBridgeErrorKind.PairedMetadataArithmeticOverflow, FerruleBasicSourceBridgeComponent.MetadataSlots);
        Equal(0, huge.IndexReads); Equal(0, capacity.SemanticCalls);
        var tiny = FerruleBasicSourceRoots.Create(BySource("P"), Array.Empty<FerruleDeclaredNamedSourceRoot>(), BasicLimits with { MaxWalkContexts = 1 }).RootContext();
        var complete = populated.WalkOwnedSourceCohort(owner, rows); var capControls = new ByControls();
        var countCause = ByBridge(ByFailure("G441.complete-original-count-before-registry", () => tiny.MapGroupBySourceCohort(complete, capControls, _ => Group())),
            FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.WalkContexts);
        Equal<nuint?>(5, countCause.Requested); Equal<nuint?>(1, countCause.Maximum); Equal(0, capControls.MetadataCalls);
        // The ordinary holder is not an authenticated paired By constructor.
        var ordinary = populated.LegacyView.GroupBy(populated.LegacyView.IterateSource(rows), rows, _ => Text("A"))[0];
        ByBridge(ByFailure("G441.ordinary-group-attach-closed", () => ordinary.RequireBasicContext()), FerruleBasicSourceBridgeErrorKind.UnsupportedBasicContext);
        var post = new ByControls { Post = _ => false }; post.Description = post.Description with { HasPostFilter = true };
        Equal(0, populated.MapGroupBySourceCohort(populated.WalkOwnedSourceCohort(owner, rows), post, _ => throw new InvalidOperationException("Dropped bucket target")).Count);
    }

    private static void BasicOwnedByCompleteCanonicalSelectors()
    {
        var owner = OwnedPrimaryOwner;
        var limits = new FerruleOwnedSourceLimits(2, 4, 1, 16_384, 8);
        ByControls UnreachedControls() => new()
        {
            Description = new(Array.Empty<bool>(), false, 1, false, false, false),
            Bound = (_, _) => throw new InvalidOperationException("An inadmissible selector reached group bounds."),
        };
        void RejectCanonical(string id, FerruleBasicScopeContext parent, string[] selector, nuint requested, nuint maximum)
        {
            var controls = UnreachedControls();
            OwnedRaw(id + ".input", new { input = BasicContextSnapshot(parent), selector, maximum = (ulong)maximum });
            var failure = ByFailure(id, () => parent.MapGroupBySourceCohort(parent.WalkSourceCohort(selector), controls, _ => Group()));
            var cause = ByBridge(failure, FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit,
                FerruleBasicSourceBridgeComponent.PathSegments);
            Equal<nuint?>(0, cause.Used); Equal<nuint?>(requested, cause.Requested); Equal<nuint?>(maximum, cause.Maximum);
            Equal(0, controls.MetadataCalls); Equal(0, controls.SemanticCalls);
        }
        void RejectOriginalRaw(string id, FerruleBasicScopeContext parent, string[] selector, nuint requested, nuint maximum)
        {
            var controls = UnreachedControls();
            var error = (FerruleRuntimeException)ByFailure(id, () => parent.MapGroupBySourceCohort(parent.WalkSourceCohort(selector), controls, _ => Group()));
            Equal(FerruleRuntimeError.OwnedSource, error.Error);
            var cause = error.OwnedSourceFailure!;
            Equal(true, ReferenceEquals(cause, error.InnerException));
            Equal(FerruleOwnedSourceErrorKind.MetadataLimit, cause.Error);
            Equal<FerruleOwnedSourceLimitKind?>(FerruleOwnedSourceLimitKind.PathSegments, cause.LimitKind);
            Equal<nuint?>(0, cause.Used); Equal<nuint?>(requested, cause.Requested); Equal<nuint?>(maximum, cause.Maximum);
            Equal(0, controls.MetadataCalls); Equal(0, controls.SemanticCalls);
        }
        FerruleInstance[] noSuffixTraversal =
        [
            Group(Field("Rows", Repeated())),
            Group(Field("Rows", Scalar(FerruleValue.Null))),
            Group(Field("Rows", Scalar(FerruleValue.JsonNull))),
            Group(),
            Scalar(FerruleValue.Null),
        ];
        for (var i = 0; i < noSuffixTraversal.Length; i++)
        {
            var root = FerruleBasicSourceRoots.Create(noSuffixTraversal[i], Array.Empty<FerruleDeclaredNamedSourceRoot>(), limits).RootContext();
            RejectOriginalRaw("G441.complete-selector-empty-missing-null." + i, root, new[] { "Rows", "Extra" }, 2, 1);
        }
        var zeroBytes = FerruleBasicSourceRoots.Create(Group(Field("Rows", Repeated())),
            Array.Empty<FerruleDeclaredNamedSourceRoot>(), limits with { MaxMetadataBytes = 0 }).RootContext();
        RejectOriginalRaw("G441.original-raw-P-before-zero-byte-budget", zeroBytes, new[] { "Rows", "Extra" }, 2, 1);
        RejectOriginalRaw("G441.original-raw-P-before-invalid-Unicode-tail", zeroBytes, new[] { "Rows", "\ud800" }, 2, 1);
        RejectOriginalRaw("G441.original-raw-P-before-invalid-Unicode-first", zeroBytes, new[] { "\ud800", "Extra" }, 2, 1);
        RejectOriginalRaw("G441.original-raw-P-before-null-first", zeroBytes, new string[] { null!, "Extra" }, 2, 1);
        var unicode = ByFailure("G441.original-Unicode-before-query-bytes", () => zeroBytes.WalkSourceCohort(new[] { "\ud800" }));
        Equal(true, unicode is ArgumentException { ParamName: "path" });

        // An empty-spelled ancestor path costs zero UTF8, so B=0 still admits
        // this parent. Its full canonical prefix must beat the later query text.
        var prefixedRoot = FerruleBasicSourceRoots.Create(Group(Field("", Repeated(Group(Field("Extra", Repeated()))))),
            Array.Empty<FerruleDeclaredNamedSourceRoot>(), limits with { MaxMetadataBytes = 0 }).RootContext();
        var prefixed = prefixedRoot.WalkOwnedSourceCohort(owner, new[] { "" })[0].Context;
        RejectCanonical("G441.complete-prefix-before-zero-query-byte-budget", prefixed, new[] { "Extra" }, 2, 1);
        RejectOriginalRaw("G441.original-raw-P-even-with-active-prefix", prefixed, new[] { "Extra", "Tail" }, 2, 1);
        foreach (var source in new FerruleInstance[] { Group(), Repeated() })
        {
            var aliasRoot = FerruleBasicSourceRoots.Create(Group(),
                new[] { new FerruleDeclaredNamedSourceRoot("Alias", source) }, limits with { MaxPathSegments = 0 }).RootContext();
            var original = aliasRoot.WalkSourceCohort(new[] { "Alias" });
            var bounds = 0; var controls = new ByControls();
            controls.Description = controls.Description with { ExplicitWindowCount = 1 };
            controls.Bound = (_, _) => { bounds++; return FerruleSequenceWindow.First(0); };
            Equal(0, aliasRoot.MapGroupBySourceCohort(original, controls, _ => throw new InvalidOperationException("First(0) target")).Count);
            Equal(1, controls.MetadataCalls); Equal(1, bounds);
            for (var i = 0; i < original.Count; i++) Equal(0, original[i].GroupTerminal!.Value.Canonical.Count);
        }
        var collision = FerruleBasicSourceRoots.Create(Group(Field("Alias", Repeated())),
            new[] { new FerruleDeclaredNamedSourceRoot("Alias", Group()) }, limits with { MaxPathSegments = 0 }).RootContext();
        RejectOriginalRaw("G441.alias-spelling-without-alias-consumption", collision, new[] { "Alias" }, 1, 0);

        var namedRows = Repeated(Group(Field("Rows", Repeated(Group(Field("Extra", Repeated()))))));
        var namedRoot = FerruleBasicSourceRoots.Create(Group(),
            new[] { new FerruleDeclaredNamedSourceRoot("Alias", namedRows) }, limits).RootContext();
        var namedParent = namedRoot.WalkSourceCohort(new[] { "Alias", "Rows" })[0].Context;
        RejectCanonical("G441.repeated-alias-complete-relative-tail", namedParent, new[] { "Extra" }, 2, 1);
        RejectCanonical("G441.repeated-alias-complete-missing-tail", namedParent, new[] { "Missing" }, 2, 1);

        var groupedRoot = FerruleBasicSourceRoots.Create(Group(Field("Rows", Repeated(Group(Field("Extra", Repeated()))))),
            Array.Empty<FerruleDeclaredNamedSourceRoot>(), limits).RootContext();
        groupedRoot.MapGroupBySourceCohort(groupedRoot.WalkOwnedSourceCohort(owner, new[] { "Rows" }), new ByControls(), grouped =>
        {
            RejectCanonical("G441.wrapper-redirect-complete-tail", grouped, new[] { "Rows", "Extra" }, 2, 1);
            RejectCanonical("G441.members-complete-relative-tail", grouped, new[] { "Extra" }, 2, 1);
            Equal(1, grouped.WalkSourceCohort(Array.Empty<string>()).Count);
            return Group();
        });

        // Preserve the earlier raw-Q admission error when the raw selector itself
        // already exceeds Q, before a selected base or alias can be observed.
        var emptyNamed = FerruleBasicSourceRoots.Create(Group(),
            new[] { new FerruleDeclaredNamedSourceRoot("Alias", Repeated()) }, limits).RootContext();
        var rawControls = UnreachedControls();
        var rawError = (FerruleRuntimeException)ByFailure("G441.raw-Q-before-canonical-alias-selection", () => emptyNamed.MapGroupBySourceCohort(
            emptyNamed.WalkSourceCohort(new[] { "Alias", "Rows", "Extra" }), rawControls, _ => Group()));
        Equal(FerruleRuntimeError.OwnedSource, rawError.Error);
        var rawCause = rawError.OwnedSourceFailure!;
        Equal(true, ReferenceEquals(rawCause, rawError.InnerException));
        Equal(FerruleOwnedSourceErrorKind.MetadataLimit, rawCause.Error);
        Equal<FerruleOwnedSourceLimitKind?>(FerruleOwnedSourceLimitKind.PathSegments, rawCause.LimitKind);
        Equal<nuint?>(0, rawCause.Used); Equal<nuint?>(3, rawCause.Requested); Equal<nuint?>(2, rawCause.Maximum);
        Equal(0, rawControls.MetadataCalls); Equal(0, rawControls.SemanticCalls);

        var directControls = UnreachedControls();
        var directError = (FerruleRuntimeException)ByFailure("G441.direct-owned-P-unchanged", () => groupedRoot.MapGroupBySourceCohort(
            groupedRoot.WalkOwnedSourceCohort(owner, new[] { "Rows", "Extra" }), directControls, _ => Group()));
        Equal(FerruleRuntimeError.OwnedSource, directError.Error);
        var directCause = directError.OwnedSourceFailure!;
        Equal(true, ReferenceEquals(directCause, directError.InnerException));
        Equal(FerruleOwnedSourceErrorKind.MetadataLimit, directCause.Error);
        Equal<FerruleOwnedSourceLimitKind?>(FerruleOwnedSourceLimitKind.PathSegments, directCause.LimitKind);
        Equal<nuint?>(0, directCause.Used); Equal<nuint?>(2, directCause.Requested); Equal<nuint?>(1, directCause.Maximum);
        Equal(0, directControls.MetadataCalls); Equal(0, directControls.SemanticCalls);
    }

    private static FerruleBasicScopeContext ByAccountingRoot(nuint bytes)
    {
        var named = Group(Field("巣", Repeated(Group(Field("行", Repeated(
            Scalar(FerruleValue.FromInt64(1)), Scalar(FerruleValue.FromInt64(2))))))));
        return FerruleBasicSourceRoots.Create(Group(),
            new[] { new FerruleDeclaredNamedSourceRoot("Ω", named) },
            new FerruleOwnedSourceLimits(2, 2, 2, bytes, 2)).RootContext();
    }

    private static void BasicOwnedByLedgerSetupAndNumericOracle()
    {
        BasicGroupAccounting.AdmitLedgerSetup(2);
        var refused = ByFailure("G441.two-ledger-records-before-allocation", () => BasicGroupAccounting.AdmitLedgerSetup(1));
        var setup = ByBridge(refused, FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.MetadataSlots);
        Equal<nuint?>(0, setup.Used); Equal<nuint?>(2, setup.Requested); Equal<nuint?>(1, setup.Maximum);
        Equal(2, BasicGroupAccounting.LedgerSetupRecords);

        var root = ByAccountingRoot(206); var original = root.WalkSourceCohort(new[] { "Ω", "巣", "行" });
        var controls = new ByControls { Description = new(new[] { false, false }, false, 1, true, false, true) };
        controls.Bound = (_, _) => FerruleSequenceWindow.First(0);
        // Hand-enumerated, not computed from production accounting helpers:
        // n=2,k=2,h=2,s=1,a=2. Each source has O=2,L=4,Q=4,T=2,
        // Jsel=3+5+3=11, so U=6+2+4+4+4+11=31 cells.
        // Each cut is (o,l,q,t,w)=(1,3,3,1,1), J=1+2+1+6+3=13;
        // T=7+2+5+5+4+13=36 cells. UTF8 lengths Ω=2,巣=3,行=3:
        // T bytes=3+5+3+18+8+3=40, U bytes=9+13+9=31.
        // Fixed non-template request=12+12+2+14+3=43, hence 177 cells,
        // 142 bytes for both T/U sets. Existing S0=88 cells/64 bytes;
        // the two NEW ledger setup objects make Seed=90 and total=267/206.
        Equal(2, original.Count);
        for (var i = 0; i < original.Count; i++)
        {
            var c = original[i]; var t = c.GroupTerminal!.Value;
            Equal(1, t.OwnedCut); Equal(3, t.LegacyCut); Equal(3, t.OriginCut); Equal(1, t.CollectionCut);
            Equal(2, t.Canonical.Count); Equal(3, t.RawCount); Equal("行", t.Wrapper);
            var template = BasicGroupPrefixTemplate.Cost(c.Context, t);
            var selection = BasicGroupPrefixTemplate.SelectionCost(c.Context);
            Equal<nuint>(36, template.Slots); Equal<nuint>(40, template.Bytes);
            Equal<nuint>(31, selection.Slots); Equal<nuint>(31, selection.Bytes);
        }
        var seed = FerruleBasicGroupByOperation.Seed(root, original);
        var fixedCost = FerruleBasicGroupByOperation.Footprint(original, controls.Description, 2, out var keyCells, out var windows);
        OwnedRaw("G441.independent-wrapper-sorted-oracle", new { seedSlots = (ulong)seed.Slots.Used,
            seedBytes = (ulong)seed.Bytes.Used, fixedSlots = (ulong)fixedCost.Slots, fixedBytes = (ulong)fixedCost.Bytes,
            maximum = (ulong)seed.Slots.Maximum, keyCells, windows });
        Equal<nuint>(90, seed.Slots.Used); Equal<nuint>(64, seed.Bytes.Used);
        Equal<nuint>(177, fixedCost.Slots); Equal<nuint>(142, fixedCost.Bytes);
        Equal<nuint>(267, seed.Slots.Used + fixedCost.Slots); Equal<nuint>(206, seed.Bytes.Used + fixedCost.Bytes);
        Equal(4, keyCells); Equal(2, windows);
        // Frozen old E_C=3322. Jmax=34,H_C=71 and the revised envelope
        // adds 2*2*71 +13*2 +5=315, so M_C=3637.
        Equal<nuint>(3322, BasicSourceAccounting.Maximum(root.Roots.Limits, true));
        Equal<nuint>(3637, seed.Slots.Maximum);
        Equal(0, root.MapGroupBySourceCohort(original, controls, _ => throw new InvalidOperationException("First(0) target")).Count);
        // With k=1126 the independently enumerated request is 3*k+171=3549.
        // Both setup records must appear in Used=90 and in the ceiling=3637.
        var directions = new ByDirections(1126);
        var excessive = new ByControls { Description = controls.Description with { Descending = directions } };
        var slots = ByBridge(ByFailure("G441.numeric-slot-refusal-includes-ledgers", () => root.MapGroupBySourceCohort(original, excessive, _ => Group())),
            FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.MetadataSlots);
        Equal<nuint?>(90, slots.Used); Equal<nuint?>(3549, slots.Requested); Equal<nuint?>(3637, slots.Maximum);
        Equal(1, directions.CountReads); Equal(0, directions.IndexReads); Equal(0, excessive.SemanticCalls);
    }

    private static void BasicOwnedByCumulativeSortAndGroupUtf8()
    {
        foreach (var maximum in new nuint[] { 221, 222 })
        {
            var root = ByAccountingRoot(maximum); var original = root.WalkSourceCohort(new[] { "Ω", "巣", "行" });
            var sorts = 0; var groups = 0; var posts = 0; var targets = 0; var events = new List<string>();
            var controls = new ByControls { Description = new(new[] { false, false }, false, 1, true, false, true) };
            controls.Key = (key, _) => { sorts++; events.Add("s" + key); return Text("é"); };
            controls.Bound = (_, _) => { events.Add("w"); return FerruleSequenceWindow.First(10); };
            controls.Group = _ => { groups++; events.Add("g"); return Text("💠"); };
            controls.Post = _ => { posts++; events.Add("p"); return true; };
            FerruleInstance Target(FerruleBasicScopeContext context)
            {
                targets++;
                Equal(FerruleValue.FromInt64(1), context.ResolveOwnedField(OwnedNamedOwner("Ω"), new[] { "巣", "行" }));
                return Group();
            }
            // Independently counted known metadata=206 bytes. Four produced sort
            // Strings add 4*2=8, then EACH group String adds 4, including the
            // duplicate. At B=221 the second group key must fail at Used=218,
            // before its post or any target; B=222 admits the identical sequence.
            if (maximum == 221)
            {
                var error = ByFailure("G441.sort-then-group-cumulative-UTF8", () => root.MapGroupBySourceCohort(original, controls, Target));
                var cause = ByBridge(error, FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.MetadataBytes);
                Equal<nuint?>(218, cause.Used); Equal<nuint?>(4, cause.Requested); Equal<nuint?>(221, cause.Maximum);
                Equal(1, posts); Equal(0, targets); Equal("s0,s1,s0,s1,w,g,p,g", string.Join(',', events));
            }
            else
            {
                var result = root.MapGroupBySourceCohort(original, controls, Target);
                OwnedRaw("G441.sort-then-group-cumulative-UTF8-exact", new { events, result = result.Select(BasicInstanceSnapshot).ToArray() });
                Equal(1, result.Count); Equal(2, posts); Equal(1, targets); Equal("s0,s1,s0,s1,w,g,p,g,p", string.Join(',', events));
            }
            Equal(4, sorts); Equal(2, groups); Equal(1, controls.MetadataCalls);
        }
    }

    private static void BasicOwnedByMalformedTerminalWitnesses()
    {
        var root = BasicRoot(BySource("P")); var original = root.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" });
        var genuine = original[0]; var t = genuine.GroupTerminal!.Value; var c = genuine.Context;
        FerruleBasicGroupByOperation.ValidateTerminal(genuine, 17);
        BasicGroupTerminal[] malformed =
        [
            t with { Owner = -1 },
            t with { Owner = 1 },
            t with { Item = null! },
            t with { Canonical = null! },
            t with { Raw = null! },
            t with { OwnedCut = -1 },
            t with { OwnedCut = c.OwnedView.FrameCount + 1 },
            t with { LegacyCut = -1 },
            t with { OriginCut = t.OriginCut + 1 },
            t with { CollectionCut = c.LegacyView.BasicCollectionCount + 1 },
            t with { RawOffset = -1 },
            t with { RawCount = -1 },
            t with { RawCount = int.MaxValue },
            t with { Wrapper = "wrong" },
            t with { Raw = new[] { "wrong" }, Wrapper = "wrong" },
            t with { Alias = "forged" },
            t with { Canonical = Array.Empty<string>() },
            t with { Item = Group() },
            t with { CollectionCut = c.LegacyView.BasicCollectionCount },
            t with { OwnedCut = c.OwnedView.FrameCount, LegacyCut = c.LegacyView.Frames.Count,
                OriginCut = c.OriginCount, CollectionCut = c.LegacyView.BasicCollectionCount },
        ];
        foreach (var bad in malformed)
        {
            // Friend-assembly tests exercise the existing pure internal validator.
            // No public constructor, mutation hook or caller-supplied ownership API.
            var candidate = new FerruleBasicSourceCandidate(c, genuine.SourceWitness, genuine.SourceId, genuine.Cursor, bad);
            var cause = ByBridge(ByFailure("G441.malformed-terminal-proof", () => FerruleBasicGroupByOperation.ValidateTerminal(candidate, 17)),
                FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness);
            Equal<nuint?>(17, cause.InputOrdinal); Equal<nuint?>(null, cause.CandidateOrdinal);
            Equal<nuint?>(null, cause.Used); Equal<nuint?>(null, cause.Requested); Equal<nuint?>(null, cause.Maximum);
        }
        var noProof = new FerruleBasicSourceCandidate(c, genuine.SourceWitness, genuine.SourceId, genuine.Cursor);
        ByBridge(ByFailure("G441.missing-terminal-proof", () => FerruleBasicGroupByOperation.ValidateTerminal(noProof, 17)),
            FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness);

        var namedRoot = BasicRoot(Group(), new FerruleDeclaredNamedSourceRoot("Alias", Group()));
        var fresh = namedRoot.WalkOwnedSourceCohort(OwnedNamedOwner("Alias"), Array.Empty<string>())[0];
        FerruleBasicGroupByOperation.ValidateTerminal(fresh, 17);
        var forgedAlias = fresh.GroupTerminal!.Value with { Alias = "Alias", RawCount = 1, Wrapper = "Alias" };
        var forged = new FerruleBasicSourceCandidate(fresh.Context, fresh.SourceWitness, fresh.SourceId, fresh.Cursor, forgedAlias);
        ByBridge(ByFailure("G441.identity-cannot-invent-legacy-alias", () => FerruleBasicGroupByOperation.ValidateTerminal(forged, 17)),
            FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness);
        // Neither rejection modified the genuine immutable source candidate.
        FerruleBasicGroupByOperation.ValidateTerminal(genuine, 17);
    }

    private static void BasicOwnedByRepeatedRootExplicitPinAndMemberShadow()
    {
        var empty = Array.Empty<string>();
        foreach (var named in new[] { false, true })
        foreach (var scalar in new[] { false, true })
        {
            var owner = named ? OwnedNamedOwner(" Root💠 ") : OwnedPrimaryOwner;
            var first = named ? "N.first" : "P.first"; var second = named ? "N.second" : "P.second";
            var repeated = scalar ? Repeated(Scalar(Text(first)), Scalar(Text(second))) :
                Repeated(BasicValue(first), BasicValue(second));
            var path = scalar ? empty : new[] { "Value" };
            var root = named ? BasicRoot(BasicValue("unrelated-primary"),
                new FerruleDeclaredNamedSourceRoot(" Root💠 ", repeated)) : BasicRoot(repeated);
            var original = root.WalkOwnedSourceCohort(owner, empty);
            var targets = 0;
            var output = root.MapGroupBySourceCohort(original, new ByControls(), grouped =>
            {
                targets++;
                Equal<nuint>(1, grouped.OwnedPosition(owner, empty));
                Equal(Text(first), grouped.ResolveOwnedField(owner, path));
                for (var flag = 0; flag < 2; flag++)
                {
                    var declaredNonrepeating = flag != 0;
                    // None remains a representative read regardless of the
                    // declaration flag. Some([]) is the grouped-only root pin.
                    Equal(Text(first), grouped.ResolveOwnedFieldWithRootPin(owner, path, null, declaredNonrepeating));
                    var error = (FerruleRuntimeException)ByFailure("G441.repeated-root-explicit-pin", () =>
                        grouped.ResolveOwnedFieldWithRootPin(owner, path, empty, declaredNonrepeating));
                    Equal(FerruleRuntimeError.OwnedSource, error.Error);
                    Equal<uint?>(null, error.Node);
                    var cause = error.OwnedSourceFailure!;
                    Equal(true, ReferenceEquals(cause, error.InnerException));
                    Equal(FerruleOwnedSourceErrorKind.UniteratedRepetition, cause.Error);
                    var identity = cause.Owner ?? throw new InvalidOperationException("Root-pin error omitted its owner.");
                    Equal(!named, identity.IsPrimary); Equal(owner.Name, identity.Name);
                    Equal(owner.Name, cause.RequestedName);
                    Equal(true, cause.Path is not null && cause.Path.SequenceEqual(path));
                    Equal<nuint?>(0, cause.Segment);
                    Equal<IReadOnlyList<string>?>(null, cause.Frame); Equal<IReadOnlyList<string>?>(null, cause.Collection);
                    Equal<FerruleOwnedSourceInstanceKind?>(null, cause.FoundInstance);
                    Equal<FerruleOwnedSourceLimitKind?>(null, cause.LimitKind);
                    Equal<nuint?>(null, cause.Used); Equal<nuint?>(null, cause.Requested); Equal<nuint?>(null, cause.Maximum);
                    Equal<Exception?>(null, cause.InnerException);
                }
                var members = grouped.WalkOwnedSourceCohort(owner, empty);
                Equal(2, members.Count);
                for (var i = 0; i < members.Count; i++)
                {
                    var member = members[i].Context; var value = i == 0 ? first : second;
                    Equal(true, member.OwnedView.FrameAt(0).KeyedGroup);
                    Equal(false, member.OwnedView.FrameAt(member.OwnedView.FrameCount - 1).KeyedGroup);
                    Equal(Text(value), member.ResolveOwnedField(owner, path, empty));
                    Equal(Text(value), member.ResolveOwnedFieldWithRootPin(owner, path, empty, false));
                    Equal(Text(value), member.ResolveOwnedFieldWithRootPin(owner, path, empty, true));
                    Equal<nuint>((nuint)i + 1, member.OwnedPosition(owner, empty));
                    Equal((long)i + 1, member.LegacyPosition(empty));
                }
                OwnedRaw("G441.repeated-root-pin-and-latest-member", new
                {
                    named, scalar, owner = owner.Name, path,
                    representative = BasicValueSnapshot(grouped.ResolveOwnedField(owner, path)),
                    groupPosition = (ulong)grouped.OwnedPosition(owner, empty),
                    members = members.Select(member => new
                    {
                        value = BasicValueSnapshot(member.Context.ResolveOwnedFieldWithRootPin(owner, path, empty, false)),
                        position = (ulong)member.Context.OwnedPosition(owner, empty),
                    }).ToArray()
                });
                Equal(Text(first), grouped.ResolveOwnedField(owner, path));
                Equal<nuint>(1, grouped.OwnedPosition(owner, empty));
                return Group();
            });
            Equal(1, output.Count); Equal(1, targets);
        }
    }
}

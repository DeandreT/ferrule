using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleGroup AdjRow(string key, string label, bool keep, int rank) => Group(
        Field("Key", Scalar(Text(key))), Field("Label", Scalar(Text(label))), Field("Keep", Scalar(FerruleValue.FromBoolean(keep))),
        Field("Rank", Scalar(FerruleValue.FromInt64(rank))), Field("Limit", Scalar(FerruleValue.FromInt64(0))));

    private static FerruleGroup AdjSource(string prefix, bool named = false) => Group(
        Field("Marker", Scalar(Text(prefix + ".root"))),
        Field("Limit", Scalar(FerruleValue.FromInt64(named ? 1 : 2))),
        Field("Rows", Repeated(
            AdjRow("A", prefix + "-a1", named, 30), AdjRow("B", prefix + "-b1", !named, 10),
            AdjRow("A", prefix + "-a2", named, 20), AdjRow("A", prefix + "-a3", !named, 25),
            AdjRow("C", prefix + "-c1", named, 40), AdjRow("A", prefix + "-a4", !named, 50))));

    // Literal output constructor only: callers supply every run and ordinal.
    // It never partitions input or derives expectations from actual output.
    private static FerruleGroup AdjExpectedGroup(int ordinal, string first, string marker, params string[] labels) => Group(
        Field("First", Scalar(Text(first))), Field("GroupPosition", Scalar(FerruleValue.FromInt64(ordinal))),
        Field("RootMarker", Scalar(Text(marker))), Field("Members", Repeated(labels.Select((label, i) => Group(
            Field("Label", Scalar(Text(label))), Field("MemberPosition", Scalar(FerruleValue.FromInt64(i + 1))),
            Field("LegacyPosition", Scalar(FerruleValue.FromInt64(i + 1))))).ToArray())));

    private static void AdjEqualTree(FerruleInstance expected, FerruleInstance actual)
    {
        Equal(expected.GetType(), actual.GetType());
        switch (expected)
        {
            case FerruleScalar scalar:
                var value = ((FerruleScalar)actual).Value;
                Equal(scalar.Value.Kind, value.Kind);
                if (value.Kind == FerruleValueKind.Double)
                    Equal(BitConverter.DoubleToInt64Bits(scalar.Value.DoubleValue), BitConverter.DoubleToInt64Bits(value.DoubleValue));
                else Equal(scalar.Value, value);
                break;
            case FerruleRepeated repeated:
                var items = ((FerruleRepeated)actual).Items;
                Equal(repeated.Items.Count, items.Count);
                for (var i = 0; i < items.Count; i++) AdjEqualTree(repeated.Items[i], items[i]);
                break;
            case FerruleGroup group:
                var found = (FerruleGroup)actual;
                Equal(FerruleXmlTypeOriginKind.Unknown, found.XmlTypeOrigin.Kind);
                Equal(group.Fields.Count, found.Fields.Count);
                for (var i = 0; i < group.Fields.Count; i++)
                {
                    Equal(group.Fields[i].Name, found.Fields[i].Name);
                    AdjEqualTree(group.Fields[i].Value, found.Fields[i].Value);
                }
                break;
            default: throw new InvalidOperationException("Unexpected adjacent smoke tree kind.");
        }
    }

    private static void AdjExpect(string id, IReadOnlyList<FerruleInstance> actual, params FerruleInstance[] expected)
    {
        OwnedRaw(id, new { actual = actual.Select(BasicInstanceSnapshot).ToArray() });
        AdjEqualTree(Repeated(expected), Repeated(actual.ToArray()));
    }

    private static Exception AdjFailure(string id, Action subject)
    {
        try { subject(); }
        catch (Exception error) { BasicErrorRaw(id, error); return error; }
        throw new InvalidOperationException("Expected adjacent grouping refusal.");
    }

    private static void BasicOwnedAdjacentLiteralRunsAndImmutablePublications()
    {
        var rows = new[] { "Rows" };
        var root = BasicRoot(AdjSource("P"), new("Rows", AdjSource("N", true)),
            new(" Røws💠 ", Group(Field("Marker", Scalar(Text("U.root"))), Field("Rows", Repeated(ByRow("A", "U-only", true, 7))))));
        foreach (var owner in new[] { OwnedPrimaryOwner, OwnedNamedOwner("Rows"), OwnedNamedOwner(" Røws💠 ") })
        foreach (var legacyMembers in new[] { false, true })
        {
            var controls = ByKey(owner); var held = new List<FerruleBasicScopeContext>();
            var output = root.MapGroupAdjacentBySourceCohort(root.WalkOwnedSourceCohort(owner, rows), controls,
                context => { held.Add(context); return ByOutput(context, owner, legacyMembers); });
            if (owner.Name == " Røws💠 ")
                AdjExpect("G442.A03", output, AdjExpectedGroup(1, "U-only", "U.root", "U-only"));
            else
            {
                var p = owner.Name is null ? "P" : "N";
                AdjExpect("G442.A01-A02", output,
                    AdjExpectedGroup(1, p + "-a1", p + ".root", p + "-a1"),
                    AdjExpectedGroup(2, p + "-b1", p + ".root", p + "-b1"),
                    AdjExpectedGroup(3, p + "-a2", p + ".root", p + "-a2", p + "-a3"),
                    AdjExpectedGroup(4, p + "-c1", p + ".root", p + "-c1"),
                    AdjExpectedGroup(5, p + "-a4", p + ".root", p + "-a4"));
            }
            Equal(1, controls.MetadataCalls);
            // R08: inspect every retained target AFTER all later runs were published.
            for (var i = 0; i < held.Count; i++)
            {
                var c = held[i]; Equal<nuint>((nuint)i + 1, c.OwnedPosition(owner, rows));
                AdjEqualTree(output[i], ByOutput(c, owner, legacyMembers));
                Equal(true, c.OwnedView.FrameAt(c.OwnedView.FrameCount - 1).KeyedGroup);
                c.LegacyView.RequireBasicContext();
                var payload = (FerruleRepeated)c.LegacyView.Frames[^1];
                Equal(true, ((ICollection<FerruleInstance>)payload.Items).IsReadOnly);
                for (var j = 0; j < i; j++)
                {
                    Equal(false, ReferenceEquals(c, held[j]));
                    Equal(false, ReferenceEquals(c.OwnedView, held[j].OwnedView));
                    Equal(false, ReferenceEquals(c.LegacyView.Frames, held[j].LegacyView.Frames));
                    Equal(false, ReferenceEquals(payload.Items, ((FerruleRepeated)held[j].LegacyView.Frames[^1]).Items));
                    Equal(false, ReferenceEquals(c.OwnedView.FrameAt(c.OwnedView.FrameCount - 1).Path,
                        held[j].OwnedView.FrameAt(held[j].OwnedView.FrameCount - 1).Path));
                }
                var clone = c.CloneView(); AdjEqualTree(output[i], ByOutput(clone, owner, legacyMembers));
            }
        }

        // Minimal n-run worst case. Each published singleton consumes a DIFFERENT
        // candidate template, and total member cells remain exactly n=3, not n^2.
        var tiny = BasicRoot(Group(Field("Rows", Repeated(ByRow("A", "a1", true, 0),
            ByRow("B", "b1", true, 0), ByRow("A", "a2", true, 0)))));
        var original = tiny.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows);
        var adjacent = tiny.MapGroupAdjacentBySourceCohort(original, ByKey(OwnedPrimaryOwner), c => c.LegacyView.Frames[^1]);
        Equal(3, adjacent.Count);
        for (var i = 0; i < 3; i++)
        {
            var members = ((FerruleRepeated)adjacent[i]).Items; Equal(1, members.Count);
            Equal(true, ReferenceEquals(original[i].GroupTerminal!.Value.Item, members[0]));
        }
        var by = tiny.MapGroupBySourceCohort(original, ByKey(OwnedPrimaryOwner), c => c.LegacyView.Frames[^1]);
        Equal(2, by.Count); Equal(2, ((FerruleRepeated)by[0]).Items.Count);
        Equal(true, ReferenceEquals(original[2].GroupTerminal!.Value.Item, ((FerruleRepeated)by[0]).Items[1]));
    }

    private static void BasicOwnedAdjacentRetentionAndGroupWindows()
    {
        var owner = OwnedPrimaryOwner; var rows = new[] { "Rows" }; var root = BasicRoot(AdjSource("P"));
        var original = root.WalkOwnedSourceCohort(owner, rows); var controls = ByKey(owner); var posts = 0;
        controls.Description = controls.Description with { HasPostFilter = true };
        controls.Post = c => { posts++; return ByRead(c, owner, "Keep").BooleanValue; };
        var retained = root.MapGroupAdjacentBySourceCohort(original, controls, c => ByOutput(c, owner, false));
        AdjExpect("G442.A05-no-coalescing-after-dropped-C", retained,
            AdjExpectedGroup(1, "P-b1", "P.root", "P-b1"),
            AdjExpectedGroup(2, "P-a2", "P.root", "P-a2", "P-a3"),
            AdjExpectedGroup(3, "P-a4", "P.root", "P-a4"));
        Equal(6, posts);
        controls.Post = _ => false;
        Equal(0, root.MapGroupAdjacentBySourceCohort(original, controls, _ => throw new InvalidOperationException("Dropped target")).Count);
        controls.Description = controls.Description with { HasPostFilter = false, HasFilter = true };
        controls.Predicate = c => ByRead(c, owner, "Key").StringValue == "A";
        AdjExpect("G442.A07-selection-removes-separators", root.MapGroupAdjacentBySourceCohort(original, controls, c => ByOutput(c, owner, false)),
            AdjExpectedGroup(1, "P-a1", "P.root", "P-a1", "P-a2", "P-a3", "P-a4"));
        var constant = new ByControls { Group = _ => Text("A") };
        AdjExpect("G442.A04-constant-key", root.MapGroupAdjacentBySourceCohort(original, constant, c => ByOutput(c, owner, false)),
            AdjExpectedGroup(1, "P-a1", "P.root", "P-a1", "P-b1", "P-a2", "P-a3", "P-c1", "P-a4"));
        controls.Description = controls.Description with { HasFilter = false, ExplicitWindowCount = 2 };
        controls.Bound = (i, parent) =>
        {
            Equal(true, ReferenceEquals(root, parent));
            return i == 0 ? FerruleSequenceWindow.SkipFirst(1) : FerruleSequenceWindow.Last(2);
        };
        AdjExpect("G442.A15", root.MapGroupAdjacentBySourceCohort(original, controls, c => ByOutput(c, owner, false)),
            AdjExpectedGroup(1, "P-c1", "P.root", "P-c1"), AdjExpectedGroup(2, "P-a4", "P.root", "P-a4"));
        controls.Bound = (i, _) => i == 0 ? FerruleSequenceWindow.Last(2) : FerruleSequenceWindow.SkipFirst(1);
        AdjExpect("G442.A16", root.MapGroupAdjacentBySourceCohort(original, controls, c => ByOutput(c, owner, false)),
            AdjExpectedGroup(1, "P-a4", "P.root", "P-a4"));
        controls.Description = controls.Description with { ExplicitWindowCount = 1 };
        controls.Bound = (_, _) => FerruleSequenceWindow.FromTo(3, 4);
        AdjExpect("G442.A17", root.MapGroupAdjacentBySourceCohort(original, controls, c => ByOutput(c, owner, false)),
            AdjExpectedGroup(1, "P-a2", "P.root", "P-a2", "P-a3"), AdjExpectedGroup(2, "P-c1", "P.root", "P-c1"));
        controls.Description = controls.Description with { Descending = new[] { false } };
        controls.Key = (_, c) => ByRead(c, owner, "Rank");
        controls.Bound = (_, parent) =>
        {
            Equal(true, ReferenceEquals(root, parent));
            return FerruleSequenceWindow.First((ulong)parent.ResolveOwnedFieldWithRootPin(
                owner, new[] { "Limit" }, Array.Empty<string>(), true).Int64Value);
        };
        AdjExpect("G442.A14-parent-bound-after-sort", root.MapGroupAdjacentBySourceCohort(original, controls, c => ByOutput(c, owner, false)),
            AdjExpectedGroup(1, "P-b1", "P.root", "P-b1"), AdjExpectedGroup(2, "P-a2", "P.root", "P-a2", "P-a3", "P-a1"));
        controls.Description = controls.Description with { ExplicitWindowCount = 0, Descending = Array.Empty<bool>(), ImplicitFirst = true };
        AdjExpect("G442.A18-implicit-first", root.MapGroupAdjacentBySourceCohort(original, controls, c => ByOutput(c, owner, false)),
            AdjExpectedGroup(1, "P-a1", "P.root", "P-a1"));
        var gaps = BasicRoot(Group(Field("Marker", Scalar(Text("P.root"))), Field("Rows", Repeated(
            ByRow("A", "a1", true, 0), Scalar(FerruleValue.Null), ByRow("A", "a2", true, 0),
            Scalar(FerruleValue.JsonNull), ByRow("B", "b1", true, 0)))));
        var log = new List<string>(); var gapControls = ByKey(owner);
        gapControls.Description = gapControls.Description with { HasPostFilter = true };
        gapControls.Group = c => { log.Add("k" + c.OwnedPosition(owner, rows)); return ByRead(c, owner, "Key"); };
        gapControls.Post = c => { log.Add("p" + c.OwnedPosition(owner, rows)); return true; };
        AdjExpect("G442.A21-null-slots-preserve-gaps", gaps.MapGroupAdjacentBySourceCohort(gaps.WalkOwnedSourceCohort(owner, rows), gapControls, c => ByOutput(c, owner, false)),
            AdjExpectedGroup(1, "a1", "P.root", "a1", "a2"), AdjExpectedGroup(2, "b1", "P.root", "b1"));
        Equal("k1,p1,k3,p3,k5,p5", string.Join(',', log));
    }

    private static void BasicOwnedAdjacentExactTaggedEquality()
    {
        var owner = OwnedPrimaryOwner; var rows = new[] { "Rows" };
        var nan = BitConverter.Int64BitsToDouble(0x7ff8000000000042);
        FerruleValue[] keys = [FerruleValue.FromInt64(1), FerruleValue.FromDouble(1), FerruleValue.FromInt64(1),
            FerruleValue.FromDouble(0), FerruleValue.FromDouble(-0.0), FerruleValue.FromDouble(nan), FerruleValue.FromDouble(nan),
            FerruleValue.Null, FerruleValue.JsonNull, FerruleValue.Null, Text("A"), Text("a"), Text("A")];
        string[] labels = ["i1", "f1", "i2", "zp", "zn", "nan1", "nan2", "n1", "jn", "n2", "sA1", "sa", "sA2"];
        var root = BasicRoot(Group(Field("Rows", Repeated(keys.Select((key, i) => Group(
            Field("Key", Scalar(key)), Field("Label", Scalar(Text(labels[i]))))).ToArray()))));
        var result = root.MapGroupAdjacentBySourceCohort(root.WalkOwnedSourceCohort(owner, rows), ByKey(owner), c => c.LegacyView.Frames[^1]);
        int[][] literalRuns = [[0], [1], [2], [3, 4], [5], [6], [7], [8], [9], [10], [11], [12]];
        Equal(12, result.Count);
        for (var i = 0; i < literalRuns.Length; i++)
            AdjEqualTree(Repeated(literalRuns[i].Select(j => Group(Field("Key", Scalar(keys[j])),
                Field("Label", Scalar(Text(labels[j]))))).ToArray()), result[i]);
        var unicode = BasicRoot(Group(Field("Rows", Repeated(ByRow("é", "1", true, 0), ByRow("é", "2", true, 0),
            ByRow("e\u0301", "3", true, 0), ByRow("é", "4", true, 0)))));
        var contexts = new List<FerruleBasicScopeContext>();
        unicode.MapGroupAdjacentBySourceCohort(unicode.WalkOwnedSourceCohort(owner, rows), ByKey(owner), c => { contexts.Add(c); return Group(); });
        Equal(3, contexts.Count);
        string[][] unicodeRuns = [["1", "2"], ["3"], ["4"]];
        for (var i = 0; i < contexts.Count; i++)
        {
            Equal<nuint>((nuint)i + 1, contexts[i].OwnedPosition(owner, rows));
            var members = contexts[i].WalkOwnedSourceCohort(owner, rows); Equal(unicodeRuns[i].Length, members.Count);
            for (var j = 0; j < members.Count; j++)
            {
                Equal(Text(unicodeRuns[i][j]), ByRead(members[j].Context, owner, "Label"));
                Equal<nuint>((nuint)j + 1, members[j].Context.OwnedPosition(owner, rows));
            }
        }
    }

    private static void BasicOwnedAdjacentCandidatePhases()
    {
        var owner = OwnedPrimaryOwner; var rows = new[] { "Rows" };
        var root = BasicRoot(Group(Field("Rows", Repeated(ByRow("A", "0", true, 0), ByRow("B", "1", true, 0), ByRow("A", "2", true, 0)))));
        foreach (var before in new[] { false, true })
        foreach (var excludeMiddle in new[] { false, true })
        foreach (var firstZero in new[] { false, true })
        {
            var log = new List<string>(); var directions = new ByDirections(2); var controls = ByKey(owner);
            controls.Description = new(directions, before, 2, false, true, true);
            string Id(FerruleBasicScopeContext c) => ByRead(c, owner, "Label").StringValue;
            controls.Predicate = c => { log.Add("f" + Id(c)); return !excludeMiddle || Id(c) != "1"; };
            controls.Key = (k, c) =>
            {
                Equal<nuint>(Id(c) == "0" ? 1U : Id(c) == "1" ? 2U : 3U, c.OwnedPosition(owner, rows));
                log.Add("s" + Id(c) + k); directions.Descending = true; return FerruleValue.FromInt64(0);
            };
            controls.Bound = (i, parent) =>
            {
                Equal(true, ReferenceEquals(root, parent)); log.Add("w" + i);
                return i == 0 ? FerruleSequenceWindow.First(firstZero ? 0UL : 10UL) : FerruleSequenceWindow.Last(10);
            };
            controls.Group = c =>
            {
                Equal<nuint>(Id(c) == "0" ? 1U : Id(c) == "1" || (before && excludeMiddle) ? 2U : 3U, c.OwnedPosition(owner, rows));
                log.Add("k" + Id(c)); return ByRead(c, owner, "Key");
            };
            controls.Post = c => { log.Add("p" + Id(c)); return true; };
            var result = root.MapGroupAdjacentBySourceCohort(root.WalkOwnedSourceCohort(owner, rows), controls,
                c => { log.Add("t" + Id(c)); return c.LegacyView.Frames[^1]; });
            string expected;
            if (before)
                expected = excludeMiddle ? "f0,f1,f2,s00,s01,s20,s21,w0,w1,k0,p0,k2,p2" :
                    "f0,f1,f2,s00,s01,s10,s11,s20,s21,w0,w1,k0,p0,k1,p1,k2,p2";
            else
                expected = excludeMiddle ? "s00,s01,s10,s11,s20,s21,w0,w1,f0,k0,p0,f1,f2,k2,p2" :
                    "s00,s01,s10,s11,s20,s21,w0,w1,f0,k0,p0,f1,k1,p1,f2,k2,p2";
            if (!firstZero) expected += excludeMiddle ? ",t0" : ",t0,t1,t2";
            OwnedRaw("G442.R04-R07", new { before, excludeMiddle, firstZero, log, actual = result.Select(BasicInstanceSnapshot).ToArray() });
            Equal(expected, string.Join(',', log)); Equal(firstZero ? 0 : excludeMiddle ? 1 : 3, result.Count);
            Equal(1, controls.MetadataCalls); Equal(1, directions.CountReads); Equal(2, directions.IndexReads);
            Equal(!excludeMiddle ? 17 : before ? 13 : 15, controls.SemanticCalls);
        }

        // Structural sort positions, selected-rank key/post positions, and gaps.
        var sorted = BasicRoot(AdjSource("P")); var trace = new List<string>(); var selected = ByKey(owner);
        selected.Description = new(new[] { false }, false, 1, false, true, true);
        selected.Key = (_, c) => { trace.Add("s" + c.OwnedPosition(owner, rows)); return ByRead(c, owner, "Rank"); };
        selected.Bound = (_, _) => { trace.Add("w"); return FerruleSequenceWindow.First(10); };
        selected.Predicate = c => { trace.Add("f" + c.OwnedPosition(owner, rows)); return c.OwnedPosition(owner, rows) > 2; };
        selected.Group = c => { trace.Add("k" + c.OwnedPosition(owner, rows)); return ByRead(c, owner, "Key"); };
        selected.Post = c => { trace.Add("p" + c.OwnedPosition(owner, rows)); return c.OwnedPosition(owner, rows) == 4; };
        AdjExpect("G442.A11", sorted.MapGroupAdjacentBySourceCohort(sorted.WalkOwnedSourceCohort(owner, rows), selected, c => ByOutput(c, owner, true)),
            AdjExpectedGroup(1, "P-a3", "P.root", "P-a3", "P-a1"));
        Equal("s1,s2,s3,s4,s5,s6,w,f1,f2,f3,k3,p3,f4,k4,p4,f5,k5,p5,f6,k6,p6", string.Join(',', trace));
    }

    private static void BasicOwnedAdjacentTypedErrorFrontiers()
    {
        var owner = OwnedPrimaryOwner; var rows = new[] { "Rows" }; var root = BasicRoot(AdjSource("P"));
        var original = root.WalkOwnedSourceCohort(owner, rows); var log = new List<string>(); var controls = ByKey(owner);
        var keyError = new FerruleRuntimeException(FerruleRuntimeError.MappingException, "key", node: 101);
        var filterError = new FerruleRuntimeException(FerruleRuntimeError.MappingException, "filter", node: 102);
        var sortError = new FerruleRuntimeException(FerruleRuntimeError.MappingException, "sort", node: 103);
        var boundError = new FerruleRuntimeException(FerruleRuntimeError.MappingException, "bound", node: 104);
        var postError = new FerruleRuntimeException(FerruleRuntimeError.MappingException, "post", node: 105);
        controls.Description = new(Array.Empty<bool>(), true, 1, true, true, true);
        controls.Bound = (_, parent) => { Equal(true, ReferenceEquals(root, parent)); log.Add("w"); return FerruleSequenceWindow.First(0); };
        controls.Predicate = c => { log.Add("f" + c.OwnedPosition(owner, rows)); if (c.OwnedPosition(owner, rows) == 2) throw filterError; return true; };
        controls.Group = _ => { log.Add("k"); throw keyError; };
        void Refuses(Exception expected, string events, string id)
        {
            var found = AdjFailure(id, () => root.MapGroupAdjacentBySourceCohort(original, controls,
                _ => throw new InvalidOperationException("Target must remain lazy")));
            Equal(true, ReferenceEquals(expected, found)); Equal(events, string.Join(',', log)); log.Clear();
        }
        // Without sorting FilterBeforeSort is NOT an eager predicate phase.
        Refuses(keyError, "w,f1,k", "G442.E01-E06");
        controls.Group = _ => { log.Add("k"); return Text("A"); };
        controls.Post = _ => { log.Add("p"); throw postError; };
        Refuses(postError, "w,f1,k,p", "G442.E02-E07");
        controls.Predicate = c => { log.Add("f" + c.OwnedPosition(owner, rows)); return true; };
        controls.Post = c => { log.Add("p"); if (c.OwnedPosition(owner, rows) == 2) throw postError; return true; };
        Refuses(postError, "w,f1,k,p,f2,k,p", "G442.E08-no-short-circuit-after-true");
        controls.Description = controls.Description with { Descending = new[] { false } };
        controls.Predicate = c => { log.Add("f" + c.OwnedPosition(owner, rows)); if (c.OwnedPosition(owner, rows) == 2) throw filterError; return true; };
        controls.Key = (_, _) => { log.Add("s"); throw sortError; };
        Refuses(filterError, "f1,f2", "G442.E03");
        controls.Description = controls.Description with { FilterBeforeSort = false };
        Refuses(sortError, "s", "G442.E04");
        controls.Key = (_, _) => FerruleValue.FromInt64(0);
        controls.Bound = (_, _) => { log.Add("w"); throw boundError; };
        Refuses(boundError, "w", "G442.E05");

        controls.Description = new(Array.Empty<bool>(), false, 2, false, false, false);
        controls.Bound = (i, _) => { log.Add("w" + i); if (i == 1) throw boundError; return FerruleSequenceWindow.First(0); };
        Refuses(boundError, "w0,w1", "G442.E11");
        controls.Description = new(Array.Empty<bool>(), false, 0, false, false, true);
        controls.Post = _ => FerruleFunctions.RequireBoolean(Text("not bool"), 105);
        var boolean = (FerruleRuntimeException)AdjFailure("G442.E09", () => root.MapGroupAdjacentBySourceCohort(original, controls, _ => Group()));
        Equal(FerruleRuntimeError.NotABool, boolean.Error); Equal<uint?>(105, boolean.Node); Equal<FerruleValueKind?>(FerruleValueKind.String, boolean.FoundKind);

        var empty = BasicRoot(Group(Field("Rows", Repeated()))); var emptyRows = empty.WalkOwnedSourceCohort(owner, rows);
        var untouched = new ByControls { Description = new(new[] { false }, true, 2, true, true, true) };
        untouched.Predicate = _ => throw new InvalidOperationException("Empty filter");
        untouched.Key = (_, _) => throw new InvalidOperationException("Empty sort");
        untouched.Group = _ => throw new InvalidOperationException("Empty key");
        untouched.Post = _ => throw new InvalidOperationException("Empty post");
        untouched.Bound = (i, parent) => { Equal(true, ReferenceEquals(empty, parent)); log.Add("w" + i); return FerruleSequenceWindow.First(0); };
        log.Clear(); Equal(0, empty.MapGroupAdjacentBySourceCohort(emptyRows, untouched, _ => throw new InvalidOperationException("Empty target")).Count);
        Equal("w0,w1", string.Join(',', log)); Equal(1, untouched.MetadataCalls); Equal(2, untouched.SemanticCalls);
        untouched.Description = untouched.Description with { ExplicitWindowCount = 1 };
        untouched.Bound = (_, _) => FerruleSequenceWindow.First(FerruleSequences.ItemCount(104, FerruleValue.FromBoolean(true)));
        var count = (FerruleRuntimeException)AdjFailure("G442.E10-empty-count", () => empty.MapGroupAdjacentBySourceCohort(emptyRows, untouched, _ => Group()));
        Equal(FerruleRuntimeError.NotAnItemCount, count.Error); Equal<uint?>(104, count.Node); Equal<FerruleValueKind?>(FerruleValueKind.Bool, count.FoundKind);

        var filters = 0; var bounds = 0;
        var filtered = new ByControls { Description = new(Array.Empty<bool>(), true, 1, false, true, true),
            Predicate = _ => { filters++; return false; }, Group = _ => throw keyError, Post = _ => throw postError,
            Bound = (_, _) => { bounds++; return FerruleSequenceWindow.First(10); } };
        Equal(0, root.MapGroupAdjacentBySourceCohort(original, filtered, _ => throw new InvalidOperationException("Filtered target")).Count);
        Equal(6, filters); Equal(1, bounds);
    }

    private static void BasicOwnedAdjacentRootPinsAndLatestItems()
    {
        var empty = Array.Empty<string>(); var labelPath = new[] { "Label" }; var keyPath = new[] { "Key" };
        foreach (var route in new[] { 0, 1, 2 })
        {
            var owner = route == 0 ? OwnedPrimaryOwner : OwnedNamedOwner("Alias");
            var repeated = Repeated(ByRow("A", "a1", true, 0), ByRow("B", "b1", true, 0),
                ByRow("A", "a2", true, 0), ByRow("A", "a3", true, 0));
            var other = new FerruleDeclaredNamedSourceRoot("Other", Repeated(BasicValue("foreign-item")));
            var root = route == 0 ? BasicRoot(repeated, other) : BasicRoot(BasicValue("primary-collision"),
                new FerruleDeclaredNamedSourceRoot("Alias", repeated), other);
            var cohort = route == 2 ? root.WalkSourceCohort(new[] { "Alias" }) : root.WalkOwnedSourceCohort(owner, empty);
            var controls = new ByControls { Group = c => c.ResolveOwnedField(owner, keyPath) };
            string[][] membersExpected = [["a1"], ["b1"], ["a2", "a3"]];
            var ordinal = 0;
            var output = root.MapGroupAdjacentBySourceCohort(cohort, controls, grouped =>
            {
                var index = ordinal++;
                Equal(Text(membersExpected[index][0]), grouped.ResolveOwnedField(owner, labelPath));
                Equal<nuint>((nuint)index + 1, grouped.OwnedPosition(owner, empty));
                foreach (var clone in new[] { grouped, grouped.CloneView() })
                foreach (var declaredNonrepeating in new[] { false, true })
                {
                    var error = (FerruleRuntimeException)AdjFailure("G442.N09", () => clone.ResolveOwnedFieldWithRootPin(owner, keyPath, empty, declaredNonrepeating));
                    var cause = error.OwnedSourceFailure!;
                    Equal(FerruleRuntimeError.OwnedSource, error.Error); Equal(true, ReferenceEquals(cause, error.InnerException));
                    Equal(FerruleOwnedSourceErrorKind.UniteratedRepetition, cause.Error);
                    var identity = cause.Owner ?? throw new InvalidOperationException("Root pin omitted exact owner.");
                    Equal(owner.Name, identity.Name); Equal(owner.Name, cause.RequestedName);
                    Equal(route == 0, identity.IsPrimary); Equal<nuint?>(0, cause.Segment);
                    Equal(true, cause.Path!.SequenceEqual(keyPath));
                }
                var ranked = grouped.OwnedView.WithPosition(owner, empty, 7);
                Equal<nuint>(7, ranked.Position(owner, empty));
                var rankedError = (FerruleOwnedSourceException)AdjFailure("G442.root-pin-after-owned-rank", () => ranked.ResolveField(owner, keyPath, empty));
                Equal(FerruleOwnedSourceErrorKind.UniteratedRepetition, rankedError.Error);
                foreach (var legacy in new[] { false, true })
                {
                    var members = legacy ? grouped.WalkSourceCohort(empty) : grouped.WalkOwnedSourceCohort(owner, empty);
                    Equal(membersExpected[index].Length, members.Count);
                    for (var i = 0; i < members.Count; i++)
                    {
                        var member = members[i].Context;
                        Equal(true, member.OwnedView.FrameAt(0).KeyedGroup);
                        Equal(false, member.OwnedView.FrameAt(member.OwnedView.FrameCount - 1).KeyedGroup);
                        Equal(Text(membersExpected[index][i]), member.ResolveOwnedField(owner, labelPath, empty));
                        foreach (var declaredNonrepeating in new[] { false, true })
                            Equal(Text(membersExpected[index][i]), member.CloneView().ResolveOwnedFieldWithRootPin(owner, labelPath, empty, declaredNonrepeating));
                        Equal<nuint>((nuint)i + 1, member.OwnedPosition(owner, empty)); Equal((long)i + 1, member.LegacyPosition(empty));
                    }
                }
                // A newer OTHER-owner ordinary root Item cannot shadow this group.
                var foreign = grouped.WalkOwnedSourceCohort(OwnedNamedOwner("Other"), empty)[0].Context;
                var foreignError = (FerruleRuntimeException)AdjFailure("G442.N11", () => foreign.ResolveOwnedFieldWithRootPin(owner, keyPath, empty, true));
                Equal(FerruleOwnedSourceErrorKind.UniteratedRepetition, foreignError.OwnedSourceFailure!.Error);
                if (route == 2) Equal(membersExpected[index].Length, grouped.WalkSourceCohort(new[] { "Alias" }).Count);
                return Group();
            });
            Equal(3, output.Count); Equal(3, ordinal);
            var ordinary = root.WalkOwnedSourceCohort(owner, empty);
            string[] literalLabels = ["a1", "b1", "a2", "a3"];
            for (var i = 0; i < ordinary.Count; i++)
                Equal(Text(literalLabels[i]), ordinary[i].Context.ResolveOwnedFieldWithRootPin(owner, labelPath, empty, false));
        }

        var nonempty = BasicRoot(AdjSource("P")); var rows = new[] { "Rows" };
        nonempty.MapGroupAdjacentBySourceCohort(nonempty.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), ByKey(OwnedPrimaryOwner), c =>
        {
            var inactive = (FerruleRuntimeException)AdjFailure("G442.N12-nonempty-group-no-root-exception", () =>
                c.ResolveOwnedFieldWithRootPin(OwnedPrimaryOwner, new[] { "Marker" }, empty, false));
            Equal(FerruleOwnedSourceErrorKind.InactiveSourceFrame, inactive.OwnedSourceFailure!.Error);
            Equal(true, inactive.OwnedSourceFailure.Collection!.SequenceEqual(empty));
            Equal(Text("P.root"), c.ResolveOwnedFieldWithRootPin(OwnedPrimaryOwner, new[] { "Marker" }, empty, true));
            var strict = (FerruleRuntimeException)AdjFailure("G442.N12-root-bypass-keeps-descendant-strict", () =>
                c.ResolveOwnedFieldWithRootPin(OwnedPrimaryOwner, new[] { "Rows", "Label" }, empty, true));
            Equal(FerruleOwnedSourceErrorKind.UniteratedRepetition, strict.OwnedSourceFailure!.Error);
            Equal<nuint?>(1, strict.OwnedSourceFailure.Segment); return Group();
        });
        foreach (var named in new[] { false, true })
        {
            var owner = named ? OwnedNamedOwner("Alias") : OwnedPrimaryOwner; var singleton = ByRow("A", "single", true, 0);
            var root = named ? BasicRoot(Group(), new FerruleDeclaredNamedSourceRoot("Alias", singleton)) : BasicRoot(singleton);
            Equal(1, root.MapGroupAdjacentBySourceCohort(root.WalkOwnedSourceCohort(owner, empty), new ByControls(), c =>
            {
                Equal(Text("single"), c.ResolveOwnedFieldWithRootPin(owner, labelPath, empty, false));
                Equal(Text("single"), c.WalkOwnedSourceCohort(owner, empty)[0].Context.ResolveOwnedField(owner, labelPath, empty)); return Group();
            }).Count);
        }
    }

    private static void BasicOwnedAdjacentFirstPrefixAndNestedGroups()
    {
        var owner = OwnedPrimaryOwner; var named = OwnedNamedOwner("Rows"); var path = new[] { "Batches", "Rows" };
        var root = BasicRoot(Group(Field("Batches", Repeated(
            Group(Field("BatchName", Scalar(Text("X"))), Field("Rows", Repeated(ByRow("A", "x1", true, 0), ByRow("B", "x2", true, 0)))),
            Group(Field("BatchName", Scalar(Text("Y"))), Field("Rows", Repeated(ByRow("B", "y1", true, 0), ByRow("A", "y2", true, 0))))))),
            new FerruleDeclaredNamedSourceRoot("Rows", AdjSource("N", true)));
        var controls = new ByControls { Group = c => c.ResolveOwnedField(owner, new[] { "Batches", "Rows", "Key" }) };
        string[][] labels = [["x1"], ["x2", "y1"], ["y2"]]; string[] prefixes = ["X", "X", "Y"];
        var ordinal = 0;
        var result = root.MapGroupAdjacentBySourceCohort(root.WalkOwnedSourceCohort(owner, path), controls, group =>
        {
            var index = ordinal++; var position = group.OwnedPosition(owner, path);
            Equal<nuint>((nuint)index + 1, position);
            foreach (var copy in new[] { group, group.CloneView() })
            {
                Equal(Text(prefixes[index]), copy.ResolveOwnedField(owner, new[] { "Batches", "BatchName" }));
                var members = copy.WalkSourceCohort(Array.Empty<string>()); Equal(labels[index].Length, members.Count);
                for (var i = 0; i < members.Count; i++)
                {
                    Equal(Text(labels[index][i]), members[i].Context.ResolveOwnedField(owner, new[] { "Batches", "Rows", "Label" }));
                    Equal(Text(prefixes[index]), members[i].Context.ResolveOwnedField(owner, new[] { "Batches", "BatchName" }));
                    Equal<nuint>((nuint)i + 1, members[i].Context.OwnedPosition(owner, path));
                }
            }
            var innerControls = ByKey(named); innerControls.Description = innerControls.Description with { Descending = new[] { false } };
            innerControls.Key = (_, c) => ByRead(c, named, "Rank");
            // The sort-selection clones and prefix templates must both retain the
            // outer keyed-group marker, ordinal, exact owner and first Batch pin.
            var inner = group.MapGroupAdjacentBySourceCohort(group.WalkOwnedSourceCohort(named, new[] { "Rows" }), innerControls, c =>
            {
                Equal(position, c.OwnedPosition(owner, path));
                Equal(Text(prefixes[index]), c.ResolveOwnedField(owner, new[] { "Batches", "BatchName" }));
                Equal(Text(labels[index][0]), c.ResolveOwnedField(owner, new[] { "Batches", "Rows", "Label" })); return Group();
            });
            Equal(4, inner.Count);
            var same = group.WalkOwnedSourceCohort(owner, path);
            Equal(labels[index].Length, same.Count);
            var regrouped = group.MapGroupBySourceCohort(same, new ByControls(), c => c.LegacyView.Frames[^1]);
            Equal(1, regrouped.Count); Equal(labels[index].Length, ((FerruleRepeated)regrouped[0]).Items.Count);
            var adjacent = group.MapGroupAdjacentBySourceCohort(same, new ByControls(), c => c.LegacyView.Frames[^1]);
            Equal(1, adjacent.Count); AdjEqualTree(regrouped[0], adjacent[0]);
            Equal(position, group.OwnedPosition(owner, path)); return Group();
        });
        Equal(3, result.Count); Equal(3, ordinal);
    }

    private static void BasicOwnedAdjacentIndependentMetadataOracle()
    {
        // R13: an independent, unchanged layout oracle, NOT a threshold found by
        // repeated execution or recomputed expected values from Footprint/Seed.
        // R2/F2/P2/W2,n2,k2,h2,s1,a2; UTF8 Ω=2,巣=3,行=3.
        // Each T=36 cells/40 bytes; each U=31 cells/31 bytes. Fixed other
        // storage=43 cells; fixed total177/142. Incoming S0=88/64 and two
        // explicit ledger records yield Seed90/64, total267/206, ceiling3637.
        BasicGroupAccounting.AdmitLedgerSetup(2);
        var setup = ByBridge(AdjFailure("G442.R13-ledger-setup", () => BasicGroupAccounting.AdmitLedgerSetup(1)),
            FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.MetadataSlots);
        Equal<nuint?>(0, setup.Used); Equal<nuint?>(2, setup.Requested); Equal<nuint?>(1, setup.Maximum);
        Equal(2, BasicGroupAccounting.LedgerSetupRecords);

        var path = new[] { "Ω", "巣", "行" }; var root = ByAccountingRoot(206); var original = root.WalkSourceCohort(path);
        var controls = new ByControls { Description = new(new[] { false, false }, false, 1, true, false, true) };
        controls.Bound = (_, _) => FerruleSequenceWindow.First(0);
        var seed = FerruleBasicGroupByOperation.Seed(root, original);
        var cost = FerruleBasicGroupByOperation.Footprint(original, controls.Description, 2, out var keyCells, out var windows);
        Equal<nuint>(90, seed.Slots.Used); Equal<nuint>(64, seed.Bytes.Used);
        Equal<nuint>(177, cost.Slots); Equal<nuint>(142, cost.Bytes);
        Equal<nuint>(267, seed.Slots.Used + cost.Slots); Equal<nuint>(206, seed.Bytes.Used + cost.Bytes);
        Equal<nuint>(3637, seed.Slots.Maximum); Equal(4, keyCells); Equal(2, windows);
        for (var i = 0; i < original.Count; i++)
        {
            var c = original[i]; var t = c.GroupTerminal!.Value;
            Equal(1, t.OwnedCut); Equal(3, t.LegacyCut); Equal(3, t.OriginCut); Equal(1, t.CollectionCut);
            Equal(2, t.Canonical.Count); Equal(3, t.RawCount); Equal("行", t.Wrapper);
            var template = BasicGroupPrefixTemplate.Cost(c.Context, t); var selection = BasicGroupPrefixTemplate.SelectionCost(c.Context);
            Equal<nuint>(36, template.Slots); Equal<nuint>(40, template.Bytes);
            Equal<nuint>(31, selection.Slots); Equal<nuint>(31, selection.Bytes);
        }
        Equal(0, root.MapGroupAdjacentBySourceCohort(original, controls, _ => throw new InvalidOperationException("First(0) target")).Count);
        Equal(1, controls.MetadataCalls); Equal(9, controls.SemanticCalls); // sort4,bound1,key2,post2
    }

    private static void BasicOwnedAdjacentMetadataRefusalAndWitnessClosure()
    {
        var path = new[] { "Ω", "巣", "行" }; var root = ByAccountingRoot(205); var original = root.WalkSourceCohort(path);
        var directions = new ByDirections(1126);
        var excessive = new ByControls { Description = new(directions, false, 1, true, false, true) };
        // R13: request=3*1126+171=3549, before known bytes (which also fail),
        // before directions indices and before ANY semantic callbacks.
        var slots = ByBridge(AdjFailure("G442.R13-slots-before-known-text", () => root.MapGroupAdjacentBySourceCohort(original, excessive, _ => Group())),
            FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.MetadataSlots);
        Equal<nuint?>(90, slots.Used); Equal<nuint?>(3549, slots.Requested); Equal<nuint?>(3637, slots.Maximum);
        Equal(1, excessive.MetadataCalls); Equal(1, directions.CountReads); Equal(0, directions.IndexReads); Equal(0, excessive.SemanticCalls);
        var fittingDirections = new ByDirections(2);
        var knownText = new ByControls { Description = excessive.Description with { Descending = fittingDirections } };
        var bytes = ByBridge(AdjFailure("G442.known-text-before-directions-and-semantics", () => root.MapGroupAdjacentBySourceCohort(original, knownText, _ => Group())),
            FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.MetadataBytes);
        Equal<nuint?>(64, bytes.Used); Equal<nuint?>(142, bytes.Requested); Equal<nuint?>(205, bytes.Maximum);
        Equal(1, knownText.MetadataCalls); Equal(1, fittingDirections.CountReads); Equal(0, fittingDirections.IndexReads); Equal(0, knownText.SemanticCalls);
        var tooManyWindows = new ByControls { Description = new(Array.Empty<bool>(), false, int.MaxValue, true, false, false) };
        ByBridge(AdjFailure("G442.R15-unrepresentable-windows-before-allocation", () => root.MapGroupAdjacentBySourceCohort(original, tooManyWindows, _ => Group())),
            FerruleBasicSourceBridgeErrorKind.PairedMetadataArithmeticOverflow, FerruleBasicSourceBridgeComponent.MetadataSlots);
        Equal(1, tooManyWindows.MetadataCalls); Equal(0, tooManyWindows.SemanticCalls);

        var emptySource = Group(Field("Rows", Repeated())); var empty = BasicRoot(emptySource);
        var emptyRows = empty.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows" });
        foreach (var foreign in new[] { empty.CloneView(), BasicRoot(emptySource) })
        {
            var noMetadata = new ByControls();
            var cause = ByBridge(AdjFailure("G442.R02-empty-original-not-a-bypass", () => foreign.MapGroupAdjacentBySourceCohort(emptyRows, noMetadata, _ => Group())),
                ReferenceEquals(foreign.Roots, empty.Roots) ? FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness : FerruleBasicSourceBridgeErrorKind.RegistryMismatch);
            if (ReferenceEquals(foreign.Roots, empty.Roots)) Equal<nuint?>(0, cause.InputOrdinal);
            Equal(0, noMetadata.MetadataCalls); Equal(0, noMetadata.SemanticCalls);
        }

        foreach (var source in new FerruleInstance[] { Group(), Group(Field("Rows", Repeated())),
            Group(Field("Rows", Scalar(FerruleValue.Null))), Group(Field("Rows", Scalar(FerruleValue.JsonNull))) })
        {
            var limited = FerruleBasicSourceRoots.Create(source, Array.Empty<FerruleDeclaredNamedSourceRoot>(),
                new FerruleOwnedSourceLimits(1, 4, 1, 4096, 4)).RootContext();
            var noMetadata = new ByControls();
            var error = (FerruleRuntimeException)AdjFailure("G442.R16-complete-selector-before-empty-missing-null", () => limited.MapGroupAdjacentBySourceCohort(
                limited.WalkOwnedSourceCohort(OwnedPrimaryOwner, new[] { "Rows", "Tail" }), noMetadata, _ => Group()));
            Equal(FerruleRuntimeError.OwnedSource, error.Error); var cause = error.OwnedSourceFailure!;
            Equal(FerruleOwnedSourceErrorKind.MetadataLimit, cause.Error);
            Equal<FerruleOwnedSourceLimitKind?>(FerruleOwnedSourceLimitKind.PathSegments, cause.LimitKind);
            Equal<nuint?>(0, cause.Used); Equal<nuint?>(2, cause.Requested); Equal<nuint?>(1, cause.Maximum);
            Equal(0, noMetadata.MetadataCalls); Equal(0, noMetadata.SemanticCalls);
        }
        var populated = BasicRoot(AdjSource("P")); var rows = new[] { "Rows" };
        var ordinary = populated.LegacyView.GroupAdjacentBy(populated.LegacyView.IterateSource(rows), rows, _ => Text("A"))[0];
        ByBridge(AdjFailure("G442.ordinary-adjacent-is-not-authenticated", () => ordinary.RequireBasicContext()),
            FerruleBasicSourceBridgeErrorKind.UnsupportedBasicContext);
        var absent = BasicRoot(AdjSource("P"), new FerruleDeclaredNamedSourceRoot("Rows", null)); var never = new ByControls();
        var unavailable = (FerruleRuntimeException)AdjFailure("G442.E15-owner-before-grouping", () => absent.MapGroupAdjacentBySourceCohort(
            absent.WalkOwnedSourceCohort(OwnedNamedOwner("Rows"), rows), never, _ => Group()));
        Equal(FerruleOwnedSourceErrorKind.SourceUnavailable, unavailable.OwnedSourceFailure!.Error);
        Equal("Rows", unavailable.OwnedSourceFailure.RequestedName); Equal(0, never.MetadataCalls); Equal(0, never.SemanticCalls);
    }

    private static void BasicOwnedAdjacentCumulativeUtf8Frontier()
    {
        foreach (var maximum in new nuint[] { 221, 222 })
        {
            var root = ByAccountingRoot(maximum); var original = root.WalkSourceCohort(new[] { "Ω", "巣", "行" });
            var log = new List<string>(); var posts = 0; var targets = 0;
            var controls = new ByControls { Description = new(new[] { false, false }, false, 1, true, false, true) };
            controls.Key = (key, _) => { log.Add("s" + key); return Text("é"); };
            controls.Bound = (_, _) => { log.Add("w"); return FerruleSequenceWindow.First(10); };
            controls.Group = _ => { log.Add("g"); return Text("💠"); };
            controls.Post = _ => { posts++; log.Add("p"); return true; };
            FerruleInstance Target(FerruleBasicScopeContext c)
            {
                targets++; log.Add("t"); Equal(2, ((FerruleRepeated)c.LegacyView.Frames[^1]).Items.Count);
                Equal(FerruleValue.FromInt64(1), c.ResolveOwnedField(OwnedNamedOwner("Ω"), new[] { "巣", "行" })); return Group();
            }
            // R14 hand count: known206 + 4*UTF8(é)=8 + 2*UTF8(💠)=8.
            // Equality/retention do not erase the SECOND returned key's charge.
            if (maximum == 221)
            {
                var cause = ByBridge(AdjFailure("G442.R14-duplicate-key-late-UTF8", () => root.MapGroupAdjacentBySourceCohort(original, controls, Target)),
                    FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit, FerruleBasicSourceBridgeComponent.MetadataBytes);
                Equal<nuint?>(218, cause.Used); Equal<nuint?>(4, cause.Requested); Equal<nuint?>(221, cause.Maximum);
                Equal("s0,s1,s0,s1,w,g,p,g", string.Join(',', log)); Equal(1, posts); Equal(0, targets);
            }
            else
            {
                Equal(1, root.MapGroupAdjacentBySourceCohort(original, controls, Target).Count);
                Equal("s0,s1,s0,s1,w,g,p,g,p,t", string.Join(',', log)); Equal(2, posts); Equal(1, targets);
            }
            Equal(1, controls.MetadataCalls);
        }
        // A callback failure wins over unknown returned text at the same frontier.
        var shortRoot = ByAccountingRoot(221); var shortRows = shortRoot.WalkSourceCohort(new[] { "Ω", "巣", "行" });
        var error = new FerruleRuntimeException(FerruleRuntimeError.MappingException, "key", node: 101); var keys = 0; var retained = 0;
        var callback = new ByControls { Description = new(new[] { false, false }, false, 1, true, false, true),
            Key = (_, _) => Text("é"), Group = _ => { if (++keys == 2) throw error; return Text("💠"); },
            Post = _ => { retained++; return true; } };
        Equal(true, ReferenceEquals(error, AdjFailure("G442.callback-error-before-UTF8-charge", () => shortRoot.MapGroupAdjacentBySourceCohort(shortRows, callback, _ => Group()))));
        Equal(2, keys); Equal(1, retained);
    }
}

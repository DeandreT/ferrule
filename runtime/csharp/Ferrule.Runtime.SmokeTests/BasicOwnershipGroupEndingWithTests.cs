using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

// Literal EndingWith products and callback logs. Fixture delegates only read
// fields; expected groups are written out by hand.
internal static partial class Program
{
    private sealed class EndingControls : IFerruleBasicGroupEndingWithControls
    {
        internal FerruleBasicGroupByMetadata Description = new(Array.Empty<bool>(), false, 0, false, false, false);
        internal FerruleOwnedSourceOwner Owner = OwnedPrimaryOwner;
        internal readonly List<string> Log = new();
        internal Func<FerruleBasicScopeContext, bool> Predicate = _ => true, End = _ => false, Post = _ => true;
        internal Func<int, FerruleBasicScopeContext, FerruleValue> Key = (_, _) => FerruleValue.FromInt64(0);
        internal Func<int, FerruleBasicScopeContext, FerruleSequenceWindow> Bound = (_, _) => FerruleSequenceWindow.First(ulong.MaxValue);
        public FerruleBasicGroupByMetadata Metadata() => Description;
        public bool Filter(FerruleBasicScopeContext c) { Log.Add("f" + EndingLabel(c, Owner)); return Predicate(c); }
        public FerruleValue SortKey(int key, FerruleBasicScopeContext c) { Log.Add("s" + EndingLabel(c, Owner)); return Key(key, c); }
        public FerruleSequenceWindow Window(int bound, FerruleBasicScopeContext parent) { Log.Add("w" + bound); return Bound(bound, parent); }
        public bool EndsGroup(FerruleBasicScopeContext c) { Log.Add("b" + EndingLabel(c, Owner)); return End(c); }
        public bool RetainMember(FerruleBasicScopeContext c) { Log.Add("p" + EndingLabel(c, Owner)); return Post(c); }
    }
    // Fixture labels are "P-a1" etc.; logs keep only the row suffix.
    private static string EndingLabel(FerruleBasicScopeContext c, FerruleOwnedSourceOwner owner) => ByRead(c, owner, "Label").StringValue[2..];
    private static string EndingLabel(FerruleBasicScopeContext c) => EndingLabel(c, OwnedPrimaryOwner);
    private static bool EndingKeep(FerruleBasicScopeContext c, FerruleOwnedSourceOwner owner) => ByRead(c, owner, "Keep").BooleanValue;

    private static void BasicOwnedEndingLiteralRunsForEveryOwner()
    {
        var rows = new[] { "Rows" };
        var root = BasicRoot(BySource("P"), new("Rows", BySource("N")), new(" Røws💠 ", BySource("U")));
        foreach (var (owner, prefix) in new[] { (OwnedPrimaryOwner, "P"), (OwnedNamedOwner("Rows"), "N"), (OwnedNamedOwner(" Røws💠 "), "U") })
        foreach (var legacyMembers in new[] { false, true })
        {
            // Keep F,T,T,F,F: b1 and a2 close groups; c1,b2 trail.
            var controls = new EndingControls { Owner = owner, End = c => EndingKeep(c, owner) };
            var output = root.MapGroupEndingWithSourceCohort(root.WalkOwnedSourceCohort(owner, rows), controls,
                c => ByOutput(c, owner, legacyMembers));
            var marker = prefix + ".root";
            AdjExpect("G444.owners-" + prefix, output,
                AdjExpectedGroup(1, prefix + "-a1", marker, prefix + "-a1", prefix + "-b1"),
                AdjExpectedGroup(2, prefix + "-a2", marker, prefix + "-a2"),
                AdjExpectedGroup(3, prefix + "-c1", marker, prefix + "-c1", prefix + "-b2"));
        }
        var all = new EndingControls { End = _ => true };
        AdjExpect("G444.all-true", root.MapGroupEndingWithSourceCohort(root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), all,
                c => ByOutput(c, OwnedPrimaryOwner, false)),
            AdjExpectedGroup(1, "P-a1", "P.root", "P-a1"), AdjExpectedGroup(2, "P-b1", "P.root", "P-b1"),
            AdjExpectedGroup(3, "P-a2", "P.root", "P-a2"), AdjExpectedGroup(4, "P-c1", "P.root", "P-c1"),
            AdjExpectedGroup(5, "P-b2", "P.root", "P-b2"));
        var none = new EndingControls();
        AdjExpect("G444.all-false", root.MapGroupEndingWithSourceCohort(root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), none,
                c => ByOutput(c, OwnedPrimaryOwner, false)),
            AdjExpectedGroup(1, "P-a1", "P.root", "P-a1", "P-b1", "P-a2", "P-c1", "P-b2"));
    }

    private static void BasicOwnedEndingPhasesRetentionAndWindows()
    {
        var rows = new[] { "Rows" };
        var root = BasicRoot(BySource("P"));
        // Post drops [a1,b1] whole; it never joins the remaining groups.
        var post = new EndingControls { Description = new(Array.Empty<bool>(), false, 0, false, false, true) };
        post.End = c => EndingKeep(c, OwnedPrimaryOwner);
        post.Post = c => EndingLabel(c) is not ("a1" or "b1");
        var targets = new List<string>();
        AdjExpect("G444.post", root.MapGroupEndingWithSourceCohort(root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), post,
                c => { targets.Add("t" + EndingLabel(c)); return ByOutput(c, OwnedPrimaryOwner, false); }),
            AdjExpectedGroup(1, "P-a2", "P.root", "P-a2"), AdjExpectedGroup(2, "P-c1", "P.root", "P-c1", "P-b2"));
        Equal("ba1,pa1,bb1,pb1,ba2,pa2,bc1,pc1,bb2,pb2", string.Join(',', post.Log));
        Equal("ta2,tc1", string.Join(',', targets));

        // Bounds run once in the parent before boundaries; selected groups follow.
        var windowed = new EndingControls { Description = new(Array.Empty<bool>(), false, 1, false, true, false) };
        windowed.Predicate = c => EndingLabel(c) != "b1";
        windowed.End = c => EndingKeep(c, OwnedPrimaryOwner);
        windowed.Bound = (_, parent) => { Equal(true, ReferenceEquals(root, parent)); return FerruleSequenceWindow.Last(1); };
        AdjExpect("G444.window", root.MapGroupEndingWithSourceCohort(root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), windowed,
                c => ByOutput(c, OwnedPrimaryOwner, false)),
            AdjExpectedGroup(1, "P-c1", "P.root", "P-c1", "P-b2"));
        Equal("w0,fa1,ba1,fb1,fa2,ba2,fc1,bc1,fb2,bb2", string.Join(',', windowed.Log));

        // Sorted by Rank: b1,b2,a2,a1,c1. Keep closes after b1 and a2.
        var sorted = new EndingControls { Description = new(new[] { false }, false, 0, false, false, false) };
        sorted.Key = (_, c) => ByRead(c, OwnedPrimaryOwner, "Rank");
        sorted.End = c => EndingKeep(c, OwnedPrimaryOwner);
        AdjExpect("G444.sorted", root.MapGroupEndingWithSourceCohort(root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), sorted,
                c => ByOutput(c, OwnedPrimaryOwner, false)),
            AdjExpectedGroup(1, "P-b1", "P.root", "P-b1"), AdjExpectedGroup(2, "P-b2", "P.root", "P-b2", "P-a2"),
            AdjExpectedGroup(3, "P-a1", "P.root", "P-a1", "P-c1"));
        Equal("sa1,sb1,sa2,sc1,sb2,bb1,bb2,ba2,ba1,bc1", string.Join(',', sorted.Log));
    }

    private static void BasicOwnedEndingEmptyCohortAndCallbackErrors()
    {
        var rows = new[] { "Rows" };
        var empty = BasicRoot(Group(Field("Marker", Scalar(Text("P.root"))), Field("Rows", Repeated())));
        var bounds = new EndingControls { Description = new(Array.Empty<bool>(), false, 1, false, false, true) };
        AdjExpect("G444.empty", empty.MapGroupEndingWithSourceCohort(empty.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), bounds,
            _ => throw new InvalidOperationException("empty target")));
        Equal("w0", string.Join(',', bounds.Log));

        var root = BasicRoot(BySource("P"));
        var first = new EndingControls { Description = new(Array.Empty<bool>(), false, 0, false, false, true) };
        first.End = _ => throw new InvalidOperationException("boundary");
        var error = AdjFailure("G444.boundary-first", () => root.MapGroupEndingWithSourceCohort(
            root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), first, _ => throw new InvalidOperationException("target")));
        Equal("boundary", error.Message);
        Equal("ba1", string.Join(',', first.Log));

        // The last boundary cannot change the partition but is still evaluated.
        var last = new EndingControls();
        last.End = c => EndingLabel(c) == "b2" ? throw new InvalidOperationException("last") : false;
        error = AdjFailure("G444.boundary-last", () => root.MapGroupEndingWithSourceCohort(
            root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), last, _ => throw new InvalidOperationException("target")));
        Equal("last", error.Message);
        Equal("ba1,bb1,ba2,bc1,bb2", string.Join(',', last.Log));
    }
}

using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

// Literal IntoBlocks products and callback logs. Fixture delegates only read
// fields; expected groups are written out by hand.
internal static partial class Program
{
    private sealed class BlocksControls : IFerruleBasicGroupIntoBlocksControls
    {
        internal FerruleBasicGroupByMetadata Description = new(Array.Empty<bool>(), false, 0, false, false, false);
        internal FerruleOwnedSourceOwner Owner = OwnedPrimaryOwner;
        internal readonly List<string> Log = new();
        internal Func<FerruleBasicScopeContext, bool> Predicate = _ => true, Post = _ => true;
        internal Func<FerruleBasicScopeContext, ulong> Size = _ => 2;
        internal Func<int, FerruleBasicScopeContext, FerruleValue> Key = (_, _) => FerruleValue.FromInt64(0);
        internal Func<int, FerruleBasicScopeContext, FerruleSequenceWindow> Bound = (_, _) => FerruleSequenceWindow.First(ulong.MaxValue);
        public FerruleBasicGroupByMetadata Metadata() => Description;
        public bool Filter(FerruleBasicScopeContext c) { Log.Add("f" + EndingLabel(c, Owner)); return Predicate(c); }
        public FerruleValue SortKey(int key, FerruleBasicScopeContext c) { Log.Add("s" + EndingLabel(c, Owner)); return Key(key, c); }
        public FerruleSequenceWindow Window(int bound, FerruleBasicScopeContext parent) { Log.Add("w" + bound); return Bound(bound, parent); }
        public ulong BlockSize(FerruleBasicScopeContext parent) { Log.Add("z"); return Size(parent); }
        public bool RetainMember(FerruleBasicScopeContext c) { Log.Add("p" + EndingLabel(c, Owner)); return Post(c); }
    }

    private static void BasicOwnedBlocksLiteralRunsForEveryOwner()
    {
        var rows = new[] { "Rows" };
        var root = BasicRoot(BySource("P"), new("Rows", BySource("N")), new(" Røws💠 ", BySource("U")));
        foreach (var (owner, prefix) in new[] { (OwnedPrimaryOwner, "P"), (OwnedNamedOwner("Rows"), "N"), (OwnedNamedOwner(" Røws💠 "), "U") })
        foreach (var legacyMembers in new[] { false, true })
        {
            // Five rows in blocks of two: the trailing block holds one member.
            var controls = new BlocksControls { Owner = owner };
            var output = root.MapGroupIntoBlocksSourceCohort(root.WalkOwnedSourceCohort(owner, rows), controls,
                c => ByOutput(c, owner, legacyMembers));
            var marker = prefix + ".root";
            AdjExpect("G445.owners-" + prefix, output,
                AdjExpectedGroup(1, prefix + "-a1", marker, prefix + "-a1", prefix + "-b1"),
                AdjExpectedGroup(2, prefix + "-a2", marker, prefix + "-a2", prefix + "-c1"),
                AdjExpectedGroup(3, prefix + "-b2", marker, prefix + "-b2"));
        }
        var three = new BlocksControls { Size = _ => 3 };
        AdjExpect("G445.three", root.MapGroupIntoBlocksSourceCohort(root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), three,
                c => ByOutput(c, OwnedPrimaryOwner, false)),
            AdjExpectedGroup(1, "P-a1", "P.root", "P-a1", "P-b1", "P-a2"),
            AdjExpectedGroup(2, "P-c1", "P.root", "P-c1", "P-b2"));
        var single = new BlocksControls { Size = _ => 1 };
        AdjExpect("G445.single", root.MapGroupIntoBlocksSourceCohort(root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), single,
                c => ByOutput(c, OwnedPrimaryOwner, false)),
            AdjExpectedGroup(1, "P-a1", "P.root", "P-a1"), AdjExpectedGroup(2, "P-b1", "P.root", "P-b1"),
            AdjExpectedGroup(3, "P-a2", "P.root", "P-a2"), AdjExpectedGroup(4, "P-c1", "P.root", "P-c1"),
            AdjExpectedGroup(5, "P-b2", "P.root", "P-b2"));
        var large = new BlocksControls { Size = _ => ulong.MaxValue };
        AdjExpect("G445.large", root.MapGroupIntoBlocksSourceCohort(root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), large,
                c => ByOutput(c, OwnedPrimaryOwner, false)),
            AdjExpectedGroup(1, "P-a1", "P.root", "P-a1", "P-b1", "P-a2", "P-c1", "P-b2"));
    }

    private static void BasicOwnedBlocksPhasesRetentionAndWindows()
    {
        var rows = new[] { "Rows" };
        var root = BasicRoot(BySource("P"));
        // Post drops [a1,b1] whole; it never refills the remaining blocks.
        var post = new BlocksControls { Description = new(Array.Empty<bool>(), false, 0, false, false, true) };
        post.Post = c => EndingLabel(c) is not ("a1" or "b1");
        var targets = new List<string>();
        AdjExpect("G445.post", root.MapGroupIntoBlocksSourceCohort(root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), post,
                c => { targets.Add("t" + EndingLabel(c)); return ByOutput(c, OwnedPrimaryOwner, false); }),
            AdjExpectedGroup(1, "P-a2", "P.root", "P-a2", "P-c1"), AdjExpectedGroup(2, "P-b2", "P.root", "P-b2"));
        Equal("z,pa1,pb1,pa2,pc1,pb2", string.Join(',', post.Log));
        Equal("ta2,tb2", string.Join(',', targets));

        // Bounds then the size run once in the parent; selected candidates fill blocks.
        var windowed = new BlocksControls { Description = new(Array.Empty<bool>(), false, 1, false, true, false) };
        windowed.Predicate = c => EndingLabel(c) != "b1";
        windowed.Bound = (_, parent) => { Equal(true, ReferenceEquals(root, parent)); return FerruleSequenceWindow.Last(1); };
        windowed.Size = parent => { Equal(true, ReferenceEquals(root, parent)); return 2; };
        AdjExpect("G445.window", root.MapGroupIntoBlocksSourceCohort(root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), windowed,
                c => ByOutput(c, OwnedPrimaryOwner, false)),
            AdjExpectedGroup(1, "P-c1", "P.root", "P-c1", "P-b2"));
        Equal("w0,z,fa1,fb1,fa2,fc1,fb2", string.Join(',', windowed.Log));

        // Sorted by Rank: b1,b2,a2,a1,c1, chunked in sorted order after the keys.
        var sorted = new BlocksControls { Description = new(new[] { false }, false, 0, false, false, false) };
        sorted.Key = (_, c) => ByRead(c, OwnedPrimaryOwner, "Rank");
        AdjExpect("G445.sorted", root.MapGroupIntoBlocksSourceCohort(root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), sorted,
                c => ByOutput(c, OwnedPrimaryOwner, false)),
            AdjExpectedGroup(1, "P-b1", "P.root", "P-b1", "P-b2"), AdjExpectedGroup(2, "P-a2", "P.root", "P-a2", "P-a1"),
            AdjExpectedGroup(3, "P-c1", "P.root", "P-c1"));
        Equal("sa1,sb1,sa2,sc1,sb2,z", string.Join(',', sorted.Log));
    }

    private static void BasicOwnedBlocksEmptyCohortAndCallbackErrors()
    {
        var rows = new[] { "Rows" };
        var empty = BasicRoot(Group(Field("Marker", Scalar(Text("P.root"))), Field("Rows", Repeated())));
        var bounds = new BlocksControls { Description = new(Array.Empty<bool>(), false, 1, false, false, true) };
        AdjExpect("G445.empty", empty.MapGroupIntoBlocksSourceCohort(empty.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), bounds,
            _ => throw new InvalidOperationException("empty target")));
        Equal("w0,z", string.Join(',', bounds.Log));

        // A size failure follows the bounds and precedes every candidate control.
        var root = BasicRoot(BySource("P"));
        var size = new BlocksControls { Description = new(Array.Empty<bool>(), false, 1, false, true, true) };
        size.Size = _ => throw new InvalidOperationException("size");
        var error = AdjFailure("G445.size", () => root.MapGroupIntoBlocksSourceCohort(
            root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), size, _ => throw new InvalidOperationException("target")));
        Equal("size", error.Message);
        Equal("w0,z", string.Join(',', size.Log));

        // Generated controls report typed sizes; a zero from a host is a contract violation.
        var zero = new BlocksControls { Size = _ => 0 };
        error = AdjFailure("G445.zero", () => root.MapGroupIntoBlocksSourceCohort(
            root.WalkOwnedSourceCohort(OwnedPrimaryOwner, rows), zero, _ => throw new InvalidOperationException("target")));
        Equal(true, error is InvalidOperationException);
        Equal("z", string.Join(',', zero.Log));
        var typed = AdjFailure("G445.typed", () => FerruleSequences.PositiveBlockSize(7, FerruleValue.FromInt64(-1)));
        Equal(FerruleRuntimeError.InvalidBlockSize, ((FerruleRuntimeException)typed).Error);
    }
}

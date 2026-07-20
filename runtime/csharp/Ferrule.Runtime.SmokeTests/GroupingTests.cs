using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void GroupingContexts()
    {
        var source = Group(Field(
            "Departments",
            new FerruleRepeated(new FerruleInstance[]
            {
                Group(
                    Field("Name", Scalar(Text("Engineering"))),
                    Field("Rows", new FerruleRepeated(new FerruleInstance[]
                    {
                        Group(
                            Field("Key", Scalar(FerruleValue.FromInt64(1))),
                            Field("Value", Scalar(FerruleValue.FromInt64(10))),
                            Field("Start", Scalar(Bool(false)))),
                        Group(
                            Field("Key", Scalar(Text("1"))),
                            Field("Value", Scalar(FerruleValue.FromInt64(20))),
                            Field("Start", Scalar(Bool(true)))),
                        Group(
                            Field("Key", Scalar(FerruleValue.FromInt64(1))),
                            Field("Value", Scalar(FerruleValue.FromInt64(30))),
                            Field("Start", Scalar(Bool(false)))),
                        Group(
                            Field("Key", Scalar(Text("2"))),
                            Field("Value", Scalar(FerruleValue.FromInt64(40))),
                            Field("Start", Scalar(Bool(true)))),
                    })))
            })));
        var root = ScopeContext.FromSource(source);
        var path = new[] { "Departments", "Rows" };
        var candidates = root.IterateSource(path);

        Equal(1L, candidates[0].Position("Departments", "Rows"));
        Equal(3L, candidates[2].Position("Departments", "Rows"));
        var rawPositions = new List<long>();
        var byKey = root.GroupBy(
            candidates,
            path,
            item =>
            {
                rawPositions.Add(item.Position(path));
                return item.ResolveScalar("Key");
            });
        Equal("1,2,3,4", string.Join(',', rawPositions));
        Equal(3, byKey.Count);
        Equal(1L, byKey[0].Position(path));
        Equal(2L, byKey[1].Position(path));
        Equal(FerruleValue.FromInt64(10), byKey[0].ResolveScalar("Value"));
        Equal(Text("Engineering"), byKey[0].ResolveScalar("Departments", "Name"));

        var firstMembers = byKey[0].AggregateItems("Rows");
        Equal(2, firstMembers.Count);
        Equal(FerruleValue.FromInt64(10), firstMembers[0].AggregateCurrentScalar("Value"));
        Equal(FerruleValue.FromInt64(30), firstMembers[1].AggregateCurrentScalar("Value"));

        var childMembers = byKey[0].IterateSource(Array.Empty<string>());
        Equal(2, childMembers.Count);
        Equal(1L, childMembers[0].Position(path));
        Equal(2L, childMembers[1].Position(path));
        Equal(FerruleValue.FromInt64(30), childMembers[1].ResolveScalar("Value"));

        var adjacent = root.GroupAdjacentBy(candidates, path, item => item.ResolveScalar("Key"));
        Equal(4, adjacent.Count);
        Equal("10", MemberValues(adjacent[0]));
        Equal("20", MemberValues(adjacent[1]));
        Equal("30", MemberValues(adjacent[2]));
        Equal("40", MemberValues(adjacent[3]));

        var predicateCalls = 0;
        var starting = root.GroupStartingWith(
            candidates,
            path,
            item =>
            {
                predicateCalls++;
                return item.ResolveScalar("Start").BooleanValue;
            });
        Equal(candidates.Count, predicateCalls);
        Equal(3, starting.Count);
        Equal("10", MemberValues(starting[0]));
        Equal("20,30", MemberValues(starting[1]));
        Equal("40", MemberValues(starting[2]));

        var endingCalls = 0;
        var ending = root.GroupEndingWith(
            candidates,
            path,
            item =>
            {
                endingCalls++;
                return item.ResolveScalar("Value").Int64Value == 20;
            });
        Equal(candidates.Count, endingCalls);
        Equal(2, ending.Count);
        Equal("10,20", MemberValues(ending[0]));
        Equal("30,40", MemberValues(ending[1]));

        var blocks = root.GroupIntoBlocks(candidates, path, 3);
        Equal(2, blocks.Count);
        Equal("10,20,30", MemberValues(blocks[0]));
        Equal("40", MemberValues(blocks[1]));
        Equal(3UL, FerruleSequences.PositiveBlockSize(91, Text("3")));
        var invalid = Error(
            FerruleRuntimeError.InvalidBlockSize,
            () => FerruleSequences.PositiveBlockSize(91, FerruleValue.FromInt64(0)));
        Equal(91U, invalid.Node);
        Error(
            FerruleRuntimeError.NotAnItemCount,
            () => FerruleSequences.PositiveBlockSize(92, FerruleValue.Null));
        Throws<ArgumentOutOfRangeException>(() => root.GroupIntoBlocks(candidates, path, 0));
    }

    private static string MemberValues(ScopeContext group) => string.Join(
        ',',
        group.AggregateItems("Rows").Select(item =>
            item.AggregateCurrentScalar("Value").Int64Value));
}

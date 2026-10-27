using System.Text;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string GroupingPlan = """{"collection":["Root","rows","Items"],"keys":["key"]}""";
    private const string GroupingShape = """{"kind":"object","properties":[{"name":"rows","shape":{"kind":"array","item":{"kind":"object","properties":[{"name":"key","shape":{"kind":"string"}},{"name":"payload","shape":{"kind":"string"}},{"name":"__group_key","shape":{"kind":"string"}},{"name":"__group_member","shape":{"kind":"string"}}]}}}]}""";

    private static FerruleInstance GroupingField(FerruleInstance value, string name) => ((FerruleGroup)value).Fields.Single(f => f.Name == name).Value;
    private static IReadOnlyList<FerruleInstance> GroupingMembers(FerruleInstance value) => ((FerruleRepeated)GroupingField(((FerruleRepeated)GroupingField(((FerruleRepeated)GroupingField(value, "Root")).Items[0], "rows")).Items[0], "Items")).Items;
    private static FerruleInstance GroupingProject(string text, string shape = GroupingShape) => FerruleJsonSourceProjection.Parse(shape).ProjectDocument(FerruleJsonSourceDocument.Parse("doc", Encoding.UTF8.GetBytes(text)));

    private static void JsonGroupingPreparation()
    {
        var plan = FerruleJsonGroupingPreparation.Parse(GroupingPlan);
        var original = GroupingProject("""{"rows":[{"key":"B","key":"A","key":"B","key":"","payload":"é😀","__group_key":"original-key","__group_member":"original-member"},{"key":"A"},{"key":null},{}]}""");
        var prepared = plan.Prepare(original);
        var members = GroupingMembers(prepared); Equal(4, members.Count);
        string[] keys = ["B", "A", "", "A"];
        for (var i = 0; i < keys.Length; i++)
        {
            Equal(keys[i], ((FerruleScalar)GroupingField(members[i], FerruleJsonGroupingPreparation.KeyField)).Value.StringValue);
            if (!ReferenceEquals(GroupingField(members[i], FerruleJsonGroupingPreparation.MemberField), GroupingMembers(original)[i == 3 ? 1 : 0])) throw new Exception("original member not retained");
        }
        if (!ReferenceEquals(GroupingField(original, "Path"), GroupingField(prepared, "Path"))) throw new Exception("document identity changed");
        foreach (var invalid in new[] { "null", "{}", """{"collection":["Root"],"keys":[]}""", """{"collection":["Elsewhere"],"keys":["key"]}""", """{"collection":["Root"],"keys":["\u0000"]}""", """{"collection":["Root"],"collection":["Root"],"keys":["key"]}""", """{"collection":["Root"],"keys":["key"],"extra":0}""" })
            Throws<FerruleJsonGroupingException>(() => FerruleJsonGroupingPreparation.Parse(invalid));
        Throws<FerruleJsonGroupingException>(() => plan.Prepare(new FerruleGroup([])));
        var simpleShape = GroupingShape.Replace(",{\"name\":\"payload\",\"shape\":{\"kind\":\"string\"}},{\"name\":\"__group_key\",\"shape\":{\"kind\":\"string\"}},{\"name\":\"__group_member\",\"shape\":{\"kind\":\"string\"}}", "");
        var simple = GroupingProject("""{"rows":[{"key":"A"}]}""", simpleShape);
        plan.Prepare(simple, new() { Values = 17, NameBytes = 58 });
        Throws<FerruleJsonGroupingException>(() => plan.Prepare(simple, new() { Values = 16, NameBytes = 58 }));
        Throws<FerruleJsonGroupingException>(() => plan.Prepare(simple, new() { Values = 17, NameBytes = 57 }));
        Throws<FerruleJsonGroupingException>(() => plan.Prepare(simple, new() { Values = -1, NameBytes = 58 }));
        var large = GroupingProject("{\"rows\":[{\"key\":\"A\",\"key\":\"B\",\"payload\":\"" + new string('x', 22 * 1024 * 1024) + "\"}]}");
        Throws<FerruleJsonGroupingException>(() => plan.Prepare(large));
        var loaded = new List<string>();
        var numeric = FerruleJsonSourceProjection.Parse(simpleShape.Replace("\"string\"", "\"number\""));
        Throws<FerruleJsonGroupingException>(() => numeric.Acquire(() => throw new Exception("filename evaluated"), _ => [], _ => throw new Exception("loaded"), grouping: plan));
        var acquisition = FerruleJsonSourceProjection.Parse(simpleShape).Acquire(() => "*.json", _ => ["first", "second"], path => { loaded.Add(path); return FerruleJsonSourceDocument.Parse(path, Encoding.UTF8.GetBytes("""{"rows":[{"key":"A"}]}""")); }, 2, new() { Values = 16, NameBytes = 1024 }, plan);
        using var cursor = acquisition.GetEnumerator();
        Throws<FerruleJsonGroupingException>(() => cursor.MoveNext());
        Equal(false, cursor.MoveNext()); Equal(1, loaded.Count); Equal("first", loaded[0]);
    }
}

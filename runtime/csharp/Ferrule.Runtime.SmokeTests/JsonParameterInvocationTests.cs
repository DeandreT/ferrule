namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleJsonParameterInvocationPlan ParameterInvocationPlan(string iteration) =>
        FerruleJsonParameterInvocationPlan.Parse("{\"parameter\":\"Variables\",\"key_member\":\"key\",\"iteration\":\"" + iteration + "\",\"key_parameter\":\"__invocation_key\",\"projection\":{\"kind\":\"object\",\"properties\":[{\"name\":\"key\",\"shape\":{\"kind\":\"string\"}}]}}");

    private static void JsonParameterInvocationSelection()
    {
        const string text = "{\"key\":\"A\",\"key\":17,\"key\":\"A\",\"key\":\"a\",\"key\":\"\",\"extra\":{\"n\":1e1000}}";
        foreach (var (policy, expected) in new[] {
            ("root_string_occurrences", new[] { "A", "A", "a", "" }),
            ("distinct_root_string_keys", new[] { "A", "a", "" }),
        })
        {
            var prepared = FerrulePreparedJsonParameterInvocations.Prepare(ParameterInvocationPlan(policy), FerruleValue.FromString(text));
            var items = prepared.Invocations().ToArray();
            Equal(true, items.Select(item => item.Key).SequenceEqual(expected));
            Equal(expected.Length, prepared.Count);
            foreach (var item in items)
            {
                Equal(true, ReferenceEquals(items[0].ParameterObject, item.ParameterObject));
                Equal(5, item.ParameterObject.MembersNamed("key").Count());
                Equal(1, item.ParameterObject.MembersNamed("extra").Count());
                Equal("__invocation_key", item.KeyBinding.Key);
                Equal(item.Key, item.KeyBinding.Value.StringValue);
            }
        }
        foreach (var input in new[] { "null", "[]", "42", "true", "\"text\"", "{}", "{\"key\":null}", "{\"key\":1,\"key\":false,\"key\":[],\"key\":{}}" })
        {
            var empty = FerrulePreparedJsonParameterInvocations.Prepare(ParameterInvocationPlan("distinct_root_string_keys"), FerruleValue.FromString(input));
            Equal(0, empty.Count);
            empty.Visit(_ => throw new Exception("Absent driver reached host"));
        }
    }

    private static void JsonParameterInvocationAdmission()
    {
        var input = FerruleValue.FromString("{\"key\":\"é\",\"key\":\"é\"}");
        var limit = new FerruleJsonParameterInvocationLimits { Invocations = 1, KeyBytes = 2 };
        Equal(1, FerrulePreparedJsonParameterInvocations.Prepare(ParameterInvocationPlan("distinct_root_string_keys"), input, limits: limit).Count);
        Throws<FerruleJsonParameterInvocationException>(() => FerrulePreparedJsonParameterInvocations.Prepare(ParameterInvocationPlan("root_string_occurrences"), input, limits: limit));
        Throws<FerruleJsonParameterInvocationException>(() => FerrulePreparedJsonParameterInvocations.Prepare(ParameterInvocationPlan("distinct_root_string_keys"), input, limits: limit with { KeyBytes = 1 }));
        Throws<FerruleJsonParameterInvocationException>(() => FerrulePreparedJsonParameterInvocations.Prepare(ParameterInvocationPlan("distinct_root_string_keys"), input, limits: limit with { Invocations = 4097 }));
        Throws<FerruleJsonSourceException>(() => FerrulePreparedJsonParameterInvocations.Prepare(ParameterInvocationPlan("distinct_root_string_keys"), FerruleValue.FromString("{\"key\":\"A\",\"broken\": }")));
        Throws<FerruleJsonParameterInvocationException>(() => ParameterInvocationPlan("unknown"));
        const string wrongType = "{\"parameter\":\"Variables\",\"key_member\":\"key\",\"iteration\":\"distinct_root_string_keys\",\"key_parameter\":\"__invocation_key\",\"projection\":{\"kind\":\"object\",\"properties\":[{\"name\":\"key\",\"shape\":{\"kind\":\"number\"}}]}}";
        Throws<FerruleJsonParameterInvocationException>(() => FerruleJsonParameterInvocationPlan.Parse(wrongType));
    }

    private static void JsonParameterInvocationFailureOrder()
    {
        var prepared = FerrulePreparedJsonParameterInvocations.Prepare(ParameterInvocationPlan("distinct_root_string_keys"), FerruleValue.FromString("{\"key\":\"A\",\"key\":\"B\",\"key\":\"C\"}"));
        var visited = new List<string>();
        var accepted = new List<string>();
        var expected = new InvalidOperationException("second invocation failed");
        try
        {
            prepared.Visit(item => {
                visited.Add(item.Key);
                if (item.Key == "B") throw expected;
                accepted.Add(item.Key);
            });
            throw new Exception("Expected callback failure");
        }
        catch (InvalidOperationException actual)
        {
            Equal(true, ReferenceEquals(expected, actual));
        }
        Equal(true, visited.SequenceEqual(new[] { "A", "B" }));
        Equal(true, accepted.SequenceEqual(new[] { "A" }));
        Equal(3, prepared.Count);
    }
}

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void InvocationCounters()
    {
        var c = new FerruleInvocationCounters(16, 1024);
        Equal(5L, c.Next("A", 5, 2, ""));
        Equal(8L, c.Next("A", 999, 3, ""));
        Equal(20L, c.Next("B", 20, 1, ""));
        Equal(100L, c.Next("A", 100, 2, "new"));
        Equal(102L, c.Next("A", 999, 2, "new"));
        Equal(true, c.Reset("A"));
        Equal(7L, c.Next("A", 7, 5, "new"));
        Equal(21L, c.Next("B", 999, 1, ""));
        Equal(3L, new FerruleInvocationCounters(16, 1024).Next("A", 3, 1, "new"));
        Equal(long.MaxValue, c.Next("overflow", long.MaxValue, 1, ""));
        Equal(long.MinValue, c.Next("overflow", 0, 1, ""));
        Equal(long.MinValue, c.Next("underflow", long.MinValue, -1, ""));
        Equal(long.MaxValue, c.Next("underflow", 0, -1, ""));
        Equal(4L, c.Next("zero", 4, 0, ""));
        Equal(4L, c.Next("zero", 100, 0, ""));
        Equal(9L, c.Next("nullable", 9, 1, null));
        Equal(10L, c.Next("nullable", 100, 1, null));
        Equal(22L, c.Next("nullable", 22, 1, ""));

        c = new FerruleInvocationCounters(2, 5);
        Equal(5L, c.Next("A", 5, 1, "xyz"));
        Equal(4L, c.RetainedTextBytes);
        CounterFailure(() => c.Next("A", 100, 1, "longer"), FerruleInvocationCounterFailure.TextLimit, 5);
        Equal(6L, c.Next("A", 999, 1, "xyz"));
        Equal(10L, c.Next("B", 10, 1, null));
        CounterFailure(() => c.Next("C", 20, 1, null), FerruleInvocationCounterFailure.CountLimit, 2);
        Equal(2, c.Count);
        Equal(5L, c.RetainedTextBytes);
        Equal(true, c.Reset("A"));
        Equal(false, c.Reset("missing"));
        Equal(1L, c.RetainedTextBytes);
        Equal(20L, c.Next("C", 20, 1, ""));
        Equal(11L, c.Next("B", 999, 1, null));
        Equal(30L, c.Next("C", 30, 1, "abc"));
        Equal(5L, c.RetainedTextBytes);
        Equal(40L, c.Next("C", 40, 1, null));
        Equal(2L, c.RetainedTextBytes);

        c = new FerruleInvocationCounters(6, 13);
        string[] keys = ["a", "A", "é", "e\u0301", "\0", ""];
        for (int i = 0; i < keys.Length; i++) Equal((long)i, c.Next(keys[i], i, 1, ""));
        Equal(8L, c.RetainedTextBytes);
        Equal(10L, c.Next("a", 10, 1, "🙂"));
        Equal(12L, c.RetainedTextBytes);
        CounterFailure(() => c.Next("a", 50, 1, "é🙂"), FerruleInvocationCounterFailure.TextLimit, 13);
        Equal(11L, c.Next("a", 999, 1, "🙂"));
        Equal(0L, c.Next("A", 999, -1, ""));

        c = new FerruleInvocationCounters(1, 4);
        foreach (long expected in new long[] { 1, 2, 3, 4 }) Equal(expected, c.Next("line", 1, 1, null));
        Equal(1L, new FerruleInvocationCounters(1, 4).Next("line", 1, 1, null));
        Equal(-1L, c.Next("line", 99, -5, null));
        CounterFailure(() => c.Next("line", 100, 1, "\ud800"), FerruleInvocationCounterFailure.InvalidText, null);
        Equal(0L, c.Next("line", 99, 1, null));

        c = new FerruleInvocationCounters(0, 100);
        CounterFailure(() => c.Next("", 1, 1, null), FerruleInvocationCounterFailure.CountLimit, 0);
        Equal(0, c.Count);
        c = new FerruleInvocationCounters(1, 0);
        CounterFailure(() => c.Next("x", 1, 1, null), FerruleInvocationCounterFailure.TextLimit, 0);
        Equal(1L, c.Next("", 1, 1, null));
        Equal(5L, c.Next("", 5, 1, ""));
        CounterFailure(() => c.Next("", 9, 1, "x"), FerruleInvocationCounterFailure.TextLimit, 0);
        Equal(6L, c.Next("", 99, 1, ""));
    }

    private static void CounterFailure(Action action, FerruleInvocationCounterFailure failure, long? maximum)
    {
        try { action(); }
        catch (FerruleInvocationCounterException error)
        {
            Equal(failure, error.Failure);
            Equal(maximum, error.Maximum);
            return;
        }
        throw new InvalidOperationException("Expected an invocation counter refusal.");
    }
}

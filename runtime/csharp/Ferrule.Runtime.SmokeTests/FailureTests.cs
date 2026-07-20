using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void MappingFailures()
    {
        var absent = FerruleFailures.MappingFailure(2, null);
        Equal(FerruleRuntimeError.MappingFailure, absent.Error);
        Equal((int?)2, absent.FailureRule);
        Equal<string?>(null, absent.MappingFailureMessage);
        Equal("mapping failure rule 2: mapping exception was raised", absent.Message);

        FailureMessage(FerruleValue.Null, string.Empty);
        FailureMessage(FerruleValue.XmlNil, string.Empty);
        FailureMessage(Bool(false), "false");
        FailureMessage(FerruleValue.FromInt64(-7), "-7");
        FailureMessage(FerruleValue.FromDouble(1e20), "100000000000000000000");
        FailureMessage(FerruleValue.FromDouble(double.NaN), "NaN");
        FailureMessage(FerruleValue.FromDouble(double.PositiveInfinity), "inf");
        FailureMessage(Text("blocked"), "blocked");

        Throws<ArgumentOutOfRangeException>(() => FerruleFailures.MappingFailure(0, null));
    }

    private static void FailureMessage(FerruleValue value, string expected)
    {
        var failure = FerruleFailures.MappingFailure(3, value);
        Equal(FerruleRuntimeError.MappingFailure, failure.Error);
        Equal((int?)3, failure.FailureRule);
        Equal(expected, failure.MappingFailureMessage);
        Equal($"mapping failure rule 3: {expected}", failure.Message);
    }
}

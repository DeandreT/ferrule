using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    // Independently authored expectations, mirrored by crates/functions
    // casing tests. Escapes keep each scalar explicit.
    private static readonly (string Input, string Expected)[] UpperCasingVectors =
    [
        ("", ""),
        ("ascii 1-z", "ASCII 1-Z"),
        ("straße", "STRASSE"),
        ("ẞ", "ẞ"),
        ("ﬁ ﬀ ﬃ", "FI FF FFI"),
        ("ŉ", "ʼN"),
        ("ǰ", "J̌"),
        ("ΐ", "Ϊ́"),
        ("ᾳ ᾼ", "ΑΙ ΑΙ"),
        ("ǅ ǆ Ǆ", "Ǆ Ǆ Ǆ"),
        ("ı i İ", "I I İ"),
        ("ſ µ ÿ ͅ", "S Μ Ÿ Ι"),
        ("σς", "ΣΣ"),
        ("\U00010428\U0001E922\U000104D8", "\U00010400\U0001E900\U000104B0"),
        ("\U00016EBB꟏", "\U00016EA0꟎"),
        ("漢\U0001F642́", "漢\U0001F642́"),
    ];

    private static readonly (string Input, string Expected)[] LowerCasingVectors =
    [
        ("", ""),
        ("ASCII 1-Z", "ascii 1-z"),
        ("I İ", "i i̇"),
        ("ẞ SS", "ß ss"),
        ("ǅ Ǆ", "ǆ ǆ"),
        ("KΩÅ", "kωå"),
        ("\U00010400\U0001E900\U000104B0", "\U00010428\U0001E922\U000104D8"),
        ("\U00016EA0꟎", "\U00016EBB꟏"),
        ("Σ", "σ"),
        ("ΣΑ", "σα"),
        ("ΟΔΟΣ", "οδος"),
        ("ΟΔΟΣ ΟΔΟΣ.", "οδος οδος."),
        ("ΑΣΣ", "ασς"),
        ("AΣB", "aσb"),
        ("AΣ'", "aς'"),
        ("A'Σ", "a'ς"),
        ("AΣ'B", "aσ'b"),
        ("AΣ́", "aς́"),
        ("1Σ", "1σ"),
        ("漢Σ", "漢σ"),
        ("\U00010400Σ", "\U00010428ς"),
        ("ΑΣ\U00010400", "ασ\U00010428"),
        ("ʰΣ", "ʰσ"),
        ("AʰΣ", "aʰς"),
        ("ΑΣʰ", "αςʰ"),
        ("漢\U0001F642", "漢\U0001F642"),
    ];

    private static void PortableUnicodeCasing()
    {
        foreach (var (input, expected) in UpperCasingVectors)
        {
            CallEquals(Text(expected), "upper", Text(input));
        }
        foreach (var (input, expected) in LowerCasingVectors)
        {
            CallEquals(Text(expected), "lower", Text(input));
        }
        AssertFunctionArity("upper", 1, Array.Empty<FerruleValue>());
        AssertFunctionType("lower", FerruleValue.FromInt64(1));
    }
}

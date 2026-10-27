using System.Text;

namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
    private static FerruleValue ReplaceLiteral(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "replace_literal";
        const int maximum = 64 * 1024 * 1024;
        RequireArity(function, arguments, 3);
        static string Text(FerruleValue value) => value.Kind == FerruleValueKind.String
            ? value.StringValue : throw Type(function, value);
        var input = Text(arguments[0]);
        var search = Text(arguments[1]);
        var replacement = arguments[2].Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull
            ? string.Empty : Text(arguments[2]);
        if (search.Length == 0) throw InvalidArgument(function, "search must not be empty");
        if (Encoding.UTF8.GetByteCount(input) > maximum || Encoding.UTF8.GetByteCount(search) > maximum
            || Encoding.UTF8.GetByteCount(replacement) > maximum)
            throw InvalidArgument(function, "text exceeds 64 MiB");
        long bytes = Encoding.UTF8.GetByteCount(input);
        var difference = Encoding.UTF8.GetByteCount(replacement) - Encoding.UTF8.GetByteCount(search);
        for (var end = 0; ;)
        {
            var index = input.IndexOf(search, end, StringComparison.Ordinal);
            if (index < 0) break;
            bytes += difference;
            if (bytes > maximum) throw InvalidArgument(function, "result exceeds 64 MiB");
            end = index + search.Length;
        }
        return FerruleValue.FromString(input.Replace(search, replacement, StringComparison.Ordinal));
    }
}

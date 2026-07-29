using System.Globalization;
using System.Numerics;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    private const int MaximumJsonMultipleOfAlternatives = 32;
    private const int MaximumJsonMultipleOfTerms = 64;

    private static JsonMultipleOfConstraints? ReadJsonMultipleOf(
        string name,
        JsonElement element,
        JsonScalarDomain scalarDomain,
        bool jsonAny,
        FerruleValue? fixedValue)
    {
        JsonElement? declaredConstraints = null;
        foreach (var property in element.EnumerateObject())
        {
            if (!string.Equals(property.Name, "json_multiple_of", StringComparison.Ordinal))
            {
                continue;
            }
            if (declaredConstraints is not null)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has duplicate JSON multiple-of metadata.");
            }
            declaredConstraints = property.Value;
        }
        if (declaredConstraints is not { } constraintsElement ||
            constraintsElement.ValueKind == JsonValueKind.Null)
        {
            return null;
        }
        RequireKind(
            constraintsElement,
            JsonValueKind.Object,
            $"schema node '{name}' JSON multiple-of constraints",
            "object");

        var sawAnyOf = false;
        foreach (var property in constraintsElement.EnumerateObject())
        {
            if (!string.Equals(property.Name, "any_of", StringComparison.Ordinal) ||
                sawAnyOf)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON multiple-of constraints have an unknown or duplicate field '{property.Name}'.");
            }
            sawAnyOf = true;
        }
        if (!sawAnyOf)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' JSON multiple-of constraints are missing field 'any_of'.");
        }
        if ((scalarDomain & (JsonScalarDomain.Int64 | JsonScalarDomain.Double)) == 0 ||
            jsonAny)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has JSON multiple-of constraints without a declared numeric domain or on json_any.");
        }

        var alternativesElement = RequiredProperty(constraintsElement, "any_of");
        RequireKind(
            alternativesElement,
            JsonValueKind.Array,
            $"schema node '{name}' JSON multiple-of alternatives",
            "array");
        var alternatives = new List<JsonMultipleOfAlternative>();
        var canonicalAlternatives = new List<JsonMultipleOf[]>();
        var termCount = 0;
        foreach (var alternativeElement in alternativesElement.EnumerateArray())
        {
            if (alternatives.Count == MaximumJsonMultipleOfAlternatives)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has more than {MaximumJsonMultipleOfAlternatives} JSON multiple-of alternatives.");
            }
            RequireKind(
                alternativeElement,
                JsonValueKind.Array,
                $"schema node '{name}' JSON multiple-of alternative",
                "array");
            var terms = new List<JsonMultipleOf>();
            foreach (var termElement in alternativeElement.EnumerateArray())
            {
                termCount = checked(termCount + 1);
                if (termCount > MaximumJsonMultipleOfTerms)
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' has more than {MaximumJsonMultipleOfTerms} JSON multiple-of terms.");
                }
                RequireKind(
                    termElement,
                    JsonValueKind.Object,
                    $"schema node '{name}' JSON multiple-of term",
                    "object");
                var fields = new HashSet<string>(StringComparer.Ordinal);
                foreach (var property in termElement.EnumerateObject())
                {
                    if (property.Name is not ("coefficient" or "decimal_exponent") ||
                        !fields.Add(property.Name))
                    {
                        throw Boundary(
                            $"Embedded JSON schema node '{name}' JSON multiple-of term has an unknown or duplicate field '{property.Name}'.");
                    }
                }
                var coefficientElement = RequiredProperty(termElement, "coefficient");
                if (coefficientElement.ValueKind != JsonValueKind.Number ||
                    !coefficientElement.TryGetUInt64(out var coefficient) ||
                    coefficient == 0 ||
                    coefficient % 10 == 0)
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' JSON multiple-of coefficient must be a positive canonical unsigned integer.");
                }
                var exponentElement = RequiredProperty(termElement, "decimal_exponent");
                if (exponentElement.ValueKind != JsonValueKind.Number ||
                    !exponentElement.TryGetInt32(out var exponent) ||
                    exponent is < short.MinValue or > short.MaxValue)
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' JSON multiple-of decimal exponent must be a signed 16-bit integer.");
                }
                var term = new JsonMultipleOf(coefficient, (short)exponent);
                if (!term.IsPositiveFinite())
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' JSON multiple-of divisor must be a positive finite decimal.");
                }
                if (terms.Contains(term))
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' has a duplicate JSON multiple-of term.");
                }
                terms.Add(term);
            }
            if (terms.Count == 0)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has an empty JSON multiple-of alternative.");
            }
            var canonical = terms.ToArray();
            if (canonicalAlternatives.Any(previous => previous.SequenceEqual(canonical)))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has a duplicate JSON multiple-of alternative.");
            }
            canonicalAlternatives.Add(canonical);
            alternatives.Add(new JsonMultipleOfAlternative(canonical));
        }
        if (alternatives.Count == 0)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has empty JSON multiple-of constraints.");
        }

        var constraints = new JsonMultipleOfConstraints(alternatives);
        if (fixedValue is { } fixedScalar &&
            fixedScalar.Kind is FerruleValueKind.Int64 or FerruleValueKind.Double &&
            !constraints.Matches(fixedScalar))
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has a fixed number outside its JSON multiple-of constraints.");
        }
        return constraints;
    }

    private static void ValidateJsonMultipleOf(
        JsonSchemaNode schema,
        FerruleValue value)
    {
        if (schema.JsonMultipleOf is { } constraints &&
            value.Kind is FerruleValueKind.Int64 or FerruleValueKind.Double &&
            !constraints.Matches(value))
        {
            throw Boundary(
                $"JSON scalar '{schema.Name}' is outside its JSON multiple-of constraints: {value}.");
        }
    }

    private static void ValidateJsonMultipleOfOutput(
        JsonSchemaNode schema,
        JsonScalarType scalar,
        FerruleValue value)
    {
        if (schema.JsonMultipleOf is null ||
            value.Kind == FerruleValueKind.JsonNull && schema.Nullable)
        {
            return;
        }
        if (scalar == JsonScalarType.Int64 && TryOutputInt64(value, out var integer))
        {
            ValidateJsonMultipleOf(schema, FerruleValue.FromInt64(integer));
        }
        else if (scalar == JsonScalarType.Double && TryOutputDouble(value, out var number))
        {
            ValidateJsonMultipleOf(schema, FerruleValue.FromDouble(number));
        }
    }

    private readonly record struct JsonMultipleOf(
        ulong Coefficient,
        short DecimalExponent)
    {
        public bool IsPositiveFinite()
        {
            var lexical = string.Concat(
                Coefficient.ToString(CultureInfo.InvariantCulture),
                "e",
                DecimalExponent.ToString(CultureInfo.InvariantCulture));
            return double.TryParse(
                       lexical,
                       NumberStyles.Float,
                       CultureInfo.InvariantCulture,
                       out var value) &&
                   double.IsFinite(value) &&
                   value > 0.0;
        }

        public bool Divides(FerruleValue value)
        {
            var (coefficient, decimalExponent) = value.Kind switch
            {
                FerruleValueKind.Int64 =>
                    (new BigInteger(value.Int64Value), 0),
                FerruleValueKind.Double when double.IsFinite(value.DoubleValue) =>
                    CanonicalDouble(value.DoubleValue),
                _ => (BigInteger.Zero, int.MinValue),
            };
            if (decimalExponent == int.MinValue)
            {
                return false;
            }
            if (coefficient.IsZero)
            {
                return true;
            }

            var divisorRest = Coefficient;
            var divisorTwos = 0;
            while (divisorRest % 2 == 0)
            {
                divisorRest /= 2;
                divisorTwos++;
            }
            var divisorFives = 0;
            while (divisorRest % 5 == 0)
            {
                divisorRest /= 5;
                divisorFives++;
            }

            var quotient = BigInteger.Abs(coefficient);
            if (quotient % divisorRest != 0)
            {
                return false;
            }
            quotient /= divisorRest;
            var quotientTwos = RemoveFactor(ref quotient, 2);
            var quotientFives = RemoveFactor(ref quotient, 5);
            var exponentDifference = decimalExponent - DecimalExponent;
            return quotientTwos + exponentDifference >= divisorTwos &&
                   quotientFives + exponentDifference >= divisorFives;
        }

        private static (BigInteger Coefficient, int DecimalExponent) CanonicalDouble(double value)
        {
            var lexical = FerruleValueMaps.RustFloatText(value);
            var decimalIndex = lexical.IndexOf('.', StringComparison.Ordinal);
            if (decimalIndex < 0)
            {
                return (
                    BigInteger.Parse(lexical, CultureInfo.InvariantCulture),
                    0);
            }
            var coefficient = string.Concat(
                lexical.AsSpan(0, decimalIndex),
                lexical.AsSpan(decimalIndex + 1));
            return (
                BigInteger.Parse(coefficient, CultureInfo.InvariantCulture),
                -(lexical.Length - decimalIndex - 1));
        }

        private static int RemoveFactor(ref BigInteger value, int factor)
        {
            var count = 0;
            while (value % factor == 0)
            {
                value /= factor;
                count++;
            }
            return count;
        }
    }

    private sealed record JsonMultipleOfAlternative(
        IReadOnlyList<JsonMultipleOf> Terms);

    private sealed record JsonMultipleOfConstraints(
        IReadOnlyList<JsonMultipleOfAlternative> AnyOf)
    {
        public bool Matches(FerruleValue value) =>
            AnyOf.Any(alternative =>
                alternative.Terms.All(term => term.Divides(value)));
    }
}

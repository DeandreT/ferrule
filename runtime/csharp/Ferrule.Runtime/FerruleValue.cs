using System.Globalization;

namespace Ferrule.Runtime;

/// <summary>The scalar tags supported by Ferrule mappings.</summary>
public enum FerruleValueKind
{
    Null,
    XmlNil,
    Bool,
    Int64,
    Double,
    String,
}

/// <summary>
/// A closed tagged scalar value. The private constructor prevents a tag from
/// being paired with the wrong payload; the default value is valid Null.
/// </summary>
public readonly struct FerruleValue : IEquatable<FerruleValue>
{
    private readonly object? _payload;

    private FerruleValue(FerruleValueKind kind, object? payload)
    {
        Kind = kind;
        _payload = payload;
    }

    public FerruleValueKind Kind { get; }

    public static FerruleValue Null => default;

    public static FerruleValue XmlNil => new(FerruleValueKind.XmlNil, null);

    public static FerruleValue FromBoolean(bool value) => new(FerruleValueKind.Bool, value);

    public static FerruleValue FromInt64(long value) => new(FerruleValueKind.Int64, value);

    public static FerruleValue FromDouble(double value)
    {
        if (!double.IsFinite(value))
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.NonFiniteDouble,
                "Ferrule double values must be finite.");
        }

        return new FerruleValue(FerruleValueKind.Double, value);
    }

    public static FerruleValue FromString(string value) =>
        new(FerruleValueKind.String, value ?? throw new ArgumentNullException(nameof(value)));

    public bool BooleanValue => Payload<bool>(FerruleValueKind.Bool);

    public long Int64Value => Payload<long>(FerruleValueKind.Int64);

    public double DoubleValue => Payload<double>(FerruleValueKind.Double);

    public string StringValue => Payload<string>(FerruleValueKind.String);

    public bool Equals(FerruleValue other) =>
        Kind == other.Kind && Kind switch
        {
            FerruleValueKind.Null or FerruleValueKind.XmlNil => true,
            FerruleValueKind.Bool => BooleanValue == other.BooleanValue,
            FerruleValueKind.Int64 => Int64Value == other.Int64Value,
            FerruleValueKind.Double => DoubleValue.Equals(other.DoubleValue),
            FerruleValueKind.String => string.Equals(StringValue, other.StringValue, StringComparison.Ordinal),
            _ => false,
        };

    public override bool Equals(object? obj) => obj is FerruleValue other && Equals(other);

    public override int GetHashCode()
    {
        unchecked
        {
            var payloadHash = Kind switch
            {
                FerruleValueKind.Null or FerruleValueKind.XmlNil => 0,
                FerruleValueKind.Bool => BooleanValue ? 1 : 0,
                FerruleValueKind.Int64 => Fold64(Int64Value),
                FerruleValueKind.Double => Fold64(BitConverter.DoubleToInt64Bits(DoubleValue)),
                FerruleValueKind.String => StableStringHash(StringValue),
                _ => 0,
            };
            return ((int)Kind * 397) ^ payloadHash;
        }
    }

    public override string ToString() => Kind switch
    {
        FerruleValueKind.Null => "null",
        FerruleValueKind.XmlNil => "xml:nil",
        FerruleValueKind.Bool => BooleanValue ? "true" : "false",
        FerruleValueKind.Int64 => Int64Value.ToString(CultureInfo.InvariantCulture),
        FerruleValueKind.Double => DoubleValue.ToString("R", CultureInfo.InvariantCulture),
        FerruleValueKind.String => StringValue,
        _ => string.Empty,
    };

    public static bool operator ==(FerruleValue left, FerruleValue right) => left.Equals(right);

    public static bool operator !=(FerruleValue left, FerruleValue right) => !left.Equals(right);

    private T Payload<T>(FerruleValueKind expected)
    {
        if (Kind != expected || _payload is not T payload)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.ValueKindMismatch,
                $"Expected a {expected} value, found {Kind}.");
        }

        return payload;
    }

    private static int Fold64(long value) => unchecked((int)value ^ (int)(value >> 32));

    private static int StableStringHash(string value)
    {
        unchecked
        {
            var hash = 2166136261U;
            foreach (var character in value)
            {
                hash = (hash ^ character) * 16777619U;
            }

            return (int)hash;
        }
    }
}

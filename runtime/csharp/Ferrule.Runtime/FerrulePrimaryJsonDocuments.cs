namespace Ferrule.Runtime;

/// <summary>A per-document or aggregate primary JSON input byte-limit failure.</summary>
public sealed class FerrulePrimaryJsonInputException(long bytes, long maximum)
    : Exception($"JSON input is {bytes} bytes; maximum is {maximum}.")
{
    public long Bytes { get; } = bytes;
    public long Maximum { get; } = maximum;
}

/// <summary>Charges each document before parsing, without accepting partial input.</summary>
public sealed class FerrulePrimaryJsonInputBudget
{
    public const long MaximumTotalBytes = 256L * 1024 * 1024;
    private long total;

    public void Charge(int bytes)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(bytes);
        if (bytes > FerruleJson.MaximumDocumentBytes)
            throw new FerrulePrimaryJsonInputException(bytes, FerruleJson.MaximumDocumentBytes);
        long next = total + bytes;
        if (next > MaximumTotalBytes)
            throw new FerrulePrimaryJsonInputException(next, MaximumTotalBytes);
        total = next;
    }
}

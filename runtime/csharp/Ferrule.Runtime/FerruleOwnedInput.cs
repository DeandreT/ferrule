namespace Ferrule.Runtime;

/// <summary>An already acquired input with host-defined bounded reading and
/// fallible close. A successful Close must mean the host has released the input.
/// Its opener remains responsible for cleanup if acquisition fails partway.</summary>
public interface IFerruleOwnedInput
{
    byte[] ReadAll();
    void Close();
}

public enum FerruleOwnedInputFailureStage { Read, Parse }

/// <summary>Close is the primary failure and InnerException. Any prior read or
/// parse exception, including its original inner chain, is retained separately.</summary>
public sealed class FerruleInputCloseException : Exception
{
    internal FerruleInputCloseException(Exception closeError,
        FerruleOwnedInputFailureStage? priorFailureStage, Exception? priorFailure)
        : base("Input close failed.", closeError)
    {
        CloseError = closeError;
        PriorFailureStage = priorFailureStage;
        PriorFailure = priorFailure;
    }
    public Exception CloseError { get; }
    public FerruleOwnedInputFailureStage? PriorFailureStage { get; }
    public Exception? PriorFailure { get; }
}

public static class FerruleOwnedInput
{
    /// <summary>Read all bytes, parse while the input is owned, and close once.
    /// Close failure remains primary and retains any read/parse failure as an
    /// explicit secondary cause. The callback must not
    /// publish: its result becomes available only after successful Close.
    /// Argument validation precedes transfer of input ownership.</summary>
    public static T ReadParseClose<T>(IFerruleOwnedInput source, Func<byte[], T> parse)
    {
        ArgumentNullException.ThrowIfNull(source);
        ArgumentNullException.ThrowIfNull(parse);
        Exception? priorFailure = null;
        var stage = FerruleOwnedInputFailureStage.Read;
        try
        {
            byte[] bytes = source.ReadAll();
            stage = FerruleOwnedInputFailureStage.Parse;
            return parse(bytes);
        }
        catch (Exception error) { priorFailure = error; throw; }
        finally
        {
            try { source.Close(); }
            catch (Exception closeError)
            {
                throw new FerruleInputCloseException(closeError,
                    priorFailure is null ? null : stage, priorFailure);
            }
        }
    }
}

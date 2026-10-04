namespace Ferrule.Runtime;

/// <summary>The host resolves and confines paths; generated libraries perform no I/O.</summary>
public interface IFerruleDynamicXmlSourceLoader
{
    byte[] Load(string sourceName, string logicalPath);
}

public sealed record FerruleXmlDynamicInputRequest(
    int DeclarationIndex, string Source, string Path, ulong Ordinal, bool CallbackInvoked);

public sealed record FerruleXmlDynamicSourcePolicy(int DeclarationIndex, string Source, string Schema);

/// <summary>One execution's byte admission and explicit product-error recovery channel.</summary>
/// <remarks>
/// Create a fresh adapter for each execution. After any failed load, including a
/// host failure, this adapter is terminal: do not retry or reuse it. Only the
/// first failed load may be recovered, synchronously, with its original private
/// exception reference preserved by the typed loader. Generated entry points
/// already abort and recover immediately on failure.
/// </remarks>
public sealed class FerruleXmlDynamicSourceAdapter : IFerruleDynamicSourceLoader
{
    private readonly IFerruleDynamicXmlSourceLoader _loader;
    private readonly FerruleXmlDynamicSourcePolicy? _policy;
    private readonly FerruleXmlDynamicSourcePolicy[]? _policies;
    private readonly FerruleXmlInputSetBudget _budget;
    private ulong _ordinal;
    private FerruleXmlExecutionException? _failure;
    private Exception? _marker;

    public FerruleXmlDynamicSourceAdapter(IFerruleDynamicXmlSourceLoader loader,
        FerruleXmlDynamicSourcePolicy policy, FerruleXmlInputSetBudget budget)
    {
        ArgumentNullException.ThrowIfNull(loader);
        ArgumentNullException.ThrowIfNull(policy);
        ArgumentNullException.ThrowIfNull(budget);
        _loader = loader;
        _policy = policy;
        _budget = budget;
    }

    /// <summary>Owns trusted validated declaration policies for one execution.</summary>
    /// <remarks>The caller must supply validated declarations with unique exact
    /// source names and their original indices and schemas. All sources share one
    /// budget, ordinal and first-failure channel.
    /// As with the single-policy constructor, any failed load is terminal.</remarks>
    public static FerruleXmlDynamicSourceAdapter ForSources(IFerruleDynamicXmlSourceLoader loader,
        IReadOnlyList<FerruleXmlDynamicSourcePolicy> sourcePolicies, FerruleXmlInputSetBudget budget)
    {
        ArgumentNullException.ThrowIfNull(loader);
        ArgumentNullException.ThrowIfNull(sourcePolicies);
        ArgumentNullException.ThrowIfNull(budget);
        var policies = new FerruleXmlDynamicSourcePolicy[sourcePolicies.Count];
        for (var index = 0; index < policies.Length; index++)
        {
            var policy = sourcePolicies[index];
            ArgumentNullException.ThrowIfNull(policy);
            policies[index] = policy;
        }
        return new FerruleXmlDynamicSourceAdapter(loader, policies, budget, true);
    }

    private FerruleXmlDynamicSourceAdapter(IFerruleDynamicXmlSourceLoader loader,
        FerruleXmlDynamicSourcePolicy[] policies, FerruleXmlInputSetBudget budget, bool _)
    {
        _loader = loader;
        _policies = policies;
        _budget = budget;
    }

    /// <summary>Only this adapter's explicit marker restores its original Input boundary.</summary>
    /// <remarks>To restore a product refusal, pass the first load failure with
    /// its original private exception reference still wrapped by the typed loader.
    /// Recovery does not permit another load or reuse of this adapter.</remarks>
    public FerruleXmlExecutionException Recover(FerruleRuntimeException error)
    {
        var original = _failure;
        var request = original?.Request;
        if (original is not null && request is not null && _marker is not null &&
            error.Error == FerruleRuntimeError.DynamicSourceLoad &&
            ReferenceEquals(error.InnerException, _marker) &&
            string.Equals(error.SourceField, request.Source, StringComparison.Ordinal) &&
            string.Equals(error.Detail, request.Path, StringComparison.Ordinal))
        {
            _failure = null;
            _marker = null;
            return original;
        }
        return FerruleXmlExecutionException.Unowned(new FerruleXmlBoundaryException(
            FerruleXmlBoundaryErrorKind.Mapping, error.Message, error));
    }

    private Exception Refuse(FerruleXmlDynamicInputRequest request, FerruleXmlBoundaryException boundary)
    {
        if (_failure is null)
        {
            _failure = FerruleXmlExecutionException.ForDynamicInput(request, boundary);
            _marker = new InvalidOperationException("generated XML input adapter refused a dynamic document", boundary);
        }
        return _marker!;
    }

    public FerruleInstance Load(string sourceName, string logicalPath)
    {
        var policy = _policy;
        if (policy is null)
        {
            foreach (var candidate in _policies!)
            {
                if (string.Equals(sourceName, candidate.Source, StringComparison.Ordinal))
                {
                    policy = candidate;
                    break;
                }
            }
        }
        if (policy is null || !string.Equals(sourceName, policy.Source, StringComparison.Ordinal))
            throw new InvalidOperationException($"undeclared dynamic XML source '{sourceName}'");
        var owner = FerruleXmlInputSource.Named(policy.DeclarationIndex, policy.Source);
        var ordinal = _ordinal == ulong.MaxValue ? ulong.MaxValue : _ordinal + 1;
        var request = new FerruleXmlDynamicInputRequest(policy.DeclarationIndex, policy.Source,
            logicalPath, ordinal, false);
        try { _budget.Reserve(owner); }
        catch (FerruleXmlExecutionException error) { throw Refuse(request, error.Boundary); }
        _ordinal = ordinal;
        request = request with { CallbackInvoked = true };
        // No external loader exception is caught by a product-parser handler.
        var document = _loader.Load(sourceName, logicalPath) ??
            throw new InvalidOperationException("loader returned null");
        try
        {
            FerruleXmlInputSetBudget.RequireDocumentSize(owner, document.LongLength);
            _budget.Charge(owner, document.LongLength);
            return FerruleXml.ParseStructuredEmbeddedBytes(policy.Schema, document);
        }
        catch (FerruleXmlExecutionException error) { throw Refuse(request, error.Boundary); }
        catch (FerruleXmlBoundaryException error) { throw Refuse(request, error); }
    }
}

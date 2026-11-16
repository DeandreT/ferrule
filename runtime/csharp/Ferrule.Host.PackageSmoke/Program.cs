using Ferrule.Host;

using var project = FerruleProject.Load(File.ReadAllBytes(args[0]));
InspectionReport inspection = project.Inspect();
if (inspection.Counts.SchemaNodes != 2
    || inspection.Boundaries[0] is not
        { Name.Text: "Ledger", NativeLocation: NativeLocation.JsonPointer { Pointer: "/document/source" } })
{
    throw new InvalidOperationException("The package returned an unexpected inspection report.");
}

ValidationReport validation = project.Validate(ValidationLimits.Default with { MaximumDiagnostics = 1 });
if (validation.IsValid || validation.TotalIssueCount != 2 || !validation.Truncated
    || validation.EmittedIssueCount != 1
    || validation.Diagnostics[0].Owner is not ValidationOwner.GraphNode
        { NodeId: 17, FunctionId: 9_007_199_254_741_027 })
{
    throw new InvalidOperationException("The package returned an unexpected validation report.");
}

Console.WriteLine("Ferrule.Host package loaded, inspected, and validated the project.");

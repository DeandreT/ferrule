using System.Text;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void CsvFileSet()
    {
        int loads = 0;
        var input = FerruleCsvFileSet.Acquire(() => "pattern", pattern => new[] { "empty.csv", "rows.csv" }, path => {
            loads++;
            return new FerruleCsvSourceData(Encoding.UTF8.GetBytes(path == "empty.csv" ? "" : "a\nb\n"), " actual/" + path + " ");
        }, new[] { new FerruleCsvReadField("Value", FerruleScalarType.String) }, "Rows",
            new FerruleCsvReadOptions { HasHeaders = false }, FerruleCsvByteDecodingPolicy.StrictUtf8, 2);
        var files = (FerruleRepeated)((FerruleGroup)input).Fields.Single().Value;
        if (loads != 2 || files.Items.Count != 2) throw new Exception("File-set membership changed.");
        var empty = (FerruleGroup)files.Items[0];
        if (((FerruleScalar)empty.Fields[0].Value).Value.StringValue != " actual/empty.csv " || ((FerruleRepeated)empty.Fields[1].Value).Items.Count != 0)
            throw new Exception("Empty physical file was discarded.");
        var rows = (FerruleRepeated)((FerruleGroup)files.Items[1]).Fields[1].Value;
        for (int i = 0; i < rows.Items.Count; i++)
        {
            var origin = ((FerruleGroup)rows.Items[i]).SourceOrigin;
            if (origin?.Position != (ulong)i + 1 || origin.Document.SourcePath != " actual/rows.csv ") throw new Exception("Row origin changed.");
        }
        try
        {
            FerruleCsvFileSet.Acquire(() => throw new Exception("invalid schema evaluated filename"), _ => Array.Empty<string>(),
                _ => throw new Exception("invalid schema loaded input"), Array.Empty<FerruleCsvReadField>(), "Path",
                new FerruleCsvReadOptions(), FerruleCsvByteDecodingPolicy.StrictUtf8, 0);
        }
        catch (FerruleCsvAcquisitionException error) when (error.Error == FerruleCsvAcquisitionError.InvalidFileSetSchema) { return; }
        throw new Exception("Invalid file-set schema was accepted.");
    }
}

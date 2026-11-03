using Ferrule.Generated;
using Ferrule.Runtime;
using G = Ferrule.Generated.GeneratedMapping;
using U = SourceSafety.ExactNames.UnicodeMapping;
using X = SourceSafety.XmlDocs.XmlMapping;
using J = SourceSafety.JsonDocs.JsonMapping;
using R = Ferrule.Runtime.FerruleSourceContractReason;

static class Program
{
    static int Cases;
    static int Loads;
    sealed class Loader : IFerruleDynamicSourceLoader, IFerruleDynamicJsonSourceLoader, IFerrulePrimarySourceHost
    {
        FerruleInstance IFerruleDynamicSourceLoader.Load(string source, string path) { Loads++; throw new Exception("loader ran"); }
        byte[] IFerruleDynamicJsonSourceLoader.Load(string source, string path) { Loads++; throw new Exception("JSON loader ran"); }
        IReadOnlyList<string> IFerrulePrimarySourceHost.Select(string filename, FerrulePrimaryFileSelection selection) { Loads++; throw new Exception("primary selection ran"); }
        FerruleCsvSourceData IFerrulePrimarySourceHost.Load(string request) { Loads++; throw new Exception("primary loader ran"); }
    }
    sealed class Shifty<T>(T injected, bool firstNonempty = false) : IReadOnlyList<T>
    {
        public int Enumerations { get; private set; }
        public int Count => 0;
        public T this[int index] => throw new IndexOutOfRangeException();
        public IEnumerator<T> GetEnumerator()
        {
            Enumerations++;
            return (firstNonempty || Enumerations > 1 ? new[] { injected } : Enumerable.Empty<T>()).GetEnumerator();
        }
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
    }
    static FerruleScalar S(string text) => new(FerruleValue.FromString(text));
    static FerruleGroup Group(params FerruleField[] fields) => new(fields);
    static FerruleRepeated Repeated(params FerruleInstance[] items) => new(items);
    static FerruleField F(string name, FerruleInstance value) => new(name, value);
    static FerruleGroup Input(bool sparse = false, bool hidden = false, FerruleInstance? text = null, FerruleInstance? cells = null)
    {
        var row = new List<FerruleField> { F("Value", S("OUTER1")) };
        if (!sparse) row.Add(F("Cell", cells ?? Repeated(Group(F("Value", S("INNER1"))), Group(F("Value", S("INNER2"))))));
        if (hidden) row.Add(F("Text", S("ROOT")));
        return Group(F("UnicodeSource", S("B")), F("AsciiSource", S("A")), F("Text", text ?? S("ROOT")),
            F("Row", Repeated(new FerruleGroup(row), Group(F("Value", S("OUTER2")), F("Cell", Repeated())))),
            F("Cell", Repeated(Group(F("Value", S("ROOT-CELL"))))));
    }
    static FerruleGroup Expected() => Group(F("Root", S("ROOT")), F("UnicodeA", S("A")), F("UnicodeB", S("B")),
        F("Rows", Repeated(
            Group(F("Root", S("ROOT")), F("Result", S("OUTER1")), F("Child", Repeated(Group(F("Result", S("OUTER1"))), Group(F("Result", S("OUTER1")))))),
            Group(F("Root", S("ROOT")), F("Result", S("OUTER2")), F("Child", Repeated())))));
    static void Tree(FerruleInstance actual, FerruleInstance expected)
    {
        if (actual is FerruleScalar a && expected is FerruleScalar b && a.Value.Equals(b.Value)) return;
        if (actual is FerruleRepeated ar && expected is FerruleRepeated br && ar.Items.Count == br.Items.Count)
        { for (var i = 0; i < ar.Items.Count; i++) Tree(ar.Items[i], br.Items[i]); return; }
        if (actual is FerruleGroup ag && expected is FerruleGroup bg && ag.Fields.Count == bg.Fields.Count)
        {
            for (var i = 0; i < ag.Fields.Count; i++)
            { if (ag.Fields[i].Name != bg.Fields[i].Name) throw new Exception("whole-output field differs"); Tree(ag.Fields[i].Value, bg.Fields[i].Value); }
            return;
        }
        throw new Exception("whole output differs");
    }
    static FerruleInstance ExpectedWithoutRoot(FerruleInstance value) => value switch
    {
        FerruleGroup group => new FerruleGroup(group.Fields.Select(field => field.Name == "Root"
            ? F(field.Name, new FerruleScalar(FerruleValue.Null)) : F(field.Name, ExpectedWithoutRoot(field.Value)))),
        FerruleRepeated repeated => new FerruleRepeated(repeated.Items.Select(ExpectedWithoutRoot)),
        _ => value,
    };
    static void Reject(Action action, R reason)
    {
        try { action(); }
        catch (Exception error)
        {
            for (Exception? current = error; current is not null; current = current.InnerException)
                if (current is FerruleRuntimeException runtime && runtime.SourceContractViolation is { } violation)
                {
                    if (runtime.Error != FerruleRuntimeError.SourceContractViolation || violation.Reason != reason)
                        throw new Exception("wrong typed guard reason", error);
                    if (violation.Path.Any(name => name.Contains("secret", StringComparison.Ordinal))) throw new Exception("private path");
                    Cases++; return;
                }
            throw new Exception("no typed guard violation", error);
        }
        throw new Exception("guard was bypassed");
    }
    const string Json = "{\"Text\":\"ROOT\",\"AsciiSource\":\"A\",\"UnicodeSource\":\"B\",\"Row\":[{\"Value\":\"OUTER1\",\"Cell\":[{\"Value\":\"INNER1\"},{\"Value\":\"INNER2\"}]},{\"Value\":\"OUTER2\",\"Cell\":[]}],\"Cell\":[{\"Value\":\"ROOT-CELL\"}]}";
    const string Xml = "<Input><Text>ROOT</Text><AsciiSource>A</AsciiSource><UnicodeSource>B</UnicodeSource><Row><Value>OUTER1</Value><Cell><Value>INNER1</Value></Cell><Cell><Value>INNER2</Value></Cell></Row><Row><Value>OUTER2</Value></Row><Cell><Value>ROOT-CELL</Value></Cell></Input>";
    static void Main()
    {
        var unicode = Group(F("X\ud800\udc00", S("B")), F("X\uff21", S("A")));
        Tree(U.Execute(unicode), Group(F("A", S("A")), F("B", S("B")))); Cases++;
        Reject(() => U.Execute(Group(F("X\uff22", S("secret")))), R.UnexpectedField);
        var source = Input(); var loader = new Loader();
        var bytes = System.Text.Encoding.UTF8.GetBytes(Json); var xmlBytes = System.Text.Encoding.UTF8.GetBytes(Xml);
        var primary = FerruleTargetSelection.Primary(); var named = FerruleTargetSelection.Named("audit");
        var absent = new FerruleGroup(source.Fields.Where(field => field.Name != "Text"));
        for (var route = 0; route < 4; route++)
        {
            var supplied = new Shifty<NamedInput>(new("Text", S("FOREIGN")));
            FerruleInstance actual = route switch {
                0 => G.ExecuteWithSources(absent, supplied),
                1 => G.ExecuteOutputsWithSources(absent, supplied).Primary,
                2 => G.ExecuteWithSourcesAndDynamicSourceLoader(absent, supplied, loader),
                _ => ((SelectedTargetOutput.Primary)G.ExecuteSelectedTargetWithHost(absent, primary, supplied, new FerruleExecutionContext("mapping"), loader)).Instance,
            };
            Tree(actual, ExpectedWithoutRoot(Expected()));
            if (supplied.Enumerations != 1) throw new Exception("bindings were re-enumerated");
            Cases++;
        }
        Reject(() => G.ExecuteWithSources(source, new Shifty<NamedInput>(new("Text", S("FOREIGN")), firstNonempty: true)), R.UnexpectedBinding);
        var jsonShifty = new Shifty<NamedJsonInput>(new("Text", "FOREIGN"));
        _ = G.ExecuteJsonOutputsWithSources(Json, jsonShifty);
        var xmlShifty = new Shifty<NamedXmlInput>(new("Text", "FOREIGN"));
        _ = G.ExecuteXmlOutputsWithSources(Xml, xmlShifty);
        var selectedShifty = new Shifty<NamedXmlBytesInput>(new("Text", [255]));
        _ = G.ExecuteXmlBytesSelectedTargetWithHost(xmlBytes, primary, selectedShifty);
        if (jsonShifty.Enumerations != 1 || xmlShifty.Enumerations != 1 || selectedShifty.Enumerations != 1)
            throw new Exception("document bindings were re-enumerated");
        Cases += 3;
        for (var repeat = 0; repeat < 3; repeat++)
        {
            Tree(G.Execute(source), Expected());
            var outputs = G.ExecuteOutputs(source); Tree(outputs.Primary, Expected());
            if (outputs.Extras.Count != 1 || outputs.Extras[0].Name != "audit") throw new Exception("whole outputs differ");
            Tree(outputs.Extras[0].Instance, Group(F("Root", S("ROOT"))));
            var selected = (SelectedTargetOutput.Named)G.ExecuteSelectedTarget(source, named);
            Tree(selected.Output.Instance, Group(F("Root", S("ROOT")))); Cases += 3;
        }
        Reject(() => G.Execute(Input(sparse: true)), R.MissingRepeatedSlot);
        Reject(() => G.Execute(Input(hidden: true)), R.UnexpectedField);
        Reject(() => G.Execute(Input(cells: new FerruleScalar(FerruleValue.Null))), R.WrongShape);
        foreach (var wrong in new FerruleInstance[] { S("ROOT"), Repeated(), new FerruleMappedSequence([]), new FerruleDocumentSet([]) })
            Reject(() => G.Execute(wrong), R.WrongShape);
        foreach (var wrong in new[] { FerruleValue.JsonNull, FerruleValue.FromInt64(1), FerruleValue.FromDouble(double.NaN), FerruleValue.FromDecimal(FerruleDecimal96.ParseExact("1")) })
            Reject(() => G.Execute(Input(text: new FerruleScalar(wrong))), R.WrongScalarDomain);
        Reject(() => G.Execute(new FerruleGroup(source.Fields.Append(F("X\uff22", S("secret"))))), R.UnexpectedField);
        // The public immutable host model itself rejects duplicate fields before
        // execution; duplicates in schema-origin groups are covered by runtime smoke tests.
        try { _ = new FerruleGroup(source.Fields.Append(F("AsciiSource", S("A")))); throw new Exception("duplicate accepted"); }
        catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.DuplicateField) { Cases++; }
        var originalParameters = new Dictionary<string, FerruleValue> { ["undeclared"] = FerruleValue.FromString("secret") };
        var parameters = new FerruleRuntimeParameters(originalParameters);
        originalParameters.Clear();
        var execution = FerruleExecutionContext.WithParameters("mapping", parameters).WithFilterMapLimits(FerruleFilterMapLimits.Default);
        var empty = new FerruleExecutionContext("mapping");
        Reject(() => X.EvaluatePrimaryFilename(execution), R.UnexpectedParameter);
        Reject(() => G.EvaluateOutputFilename(execution), R.UnexpectedParameter);
        Reject(() => J.EvaluatePrimaryFilename(execution), R.UnexpectedParameter);
        Reject(() => G.EvaluatePrimaryFilename(execution), R.UnexpectedParameter);
        Reject(() => X.ExecutePrimaryXmlDocumentBytes(execution, loader, 1), R.UnexpectedParameter);
        Reject(() => J.ExecutePrimaryJsonDocumentBytes(execution, loader, 1), R.UnexpectedParameter);
        if (X.EvaluatePrimaryFilename(empty) != "input.xml" || G.EvaluateOutputFilename(empty) != "output.xml") throw new Exception("filename control");
        foreach (var n in new[] { 1, 2 })
        {
            var inputs = Enumerable.Repeat(new NamedInput("secret", source), n).ToArray();
            Reject(() => G.ExecuteWithSources(source, inputs), R.UnexpectedBinding);
            Reject(() => G.ExecuteOutputsWithSources(source, inputs), R.UnexpectedBinding);
            Reject(() => G.ExecuteWithSources(source, inputs, empty), R.UnexpectedBinding);
            Reject(() => G.ExecuteOutputsWithSources(source, inputs, empty), R.UnexpectedBinding);
            Reject(() => G.ExecuteWithSourcesAndDynamicSourceLoader(source, inputs, loader), R.UnexpectedBinding);
            Reject(() => G.ExecuteOutputsWithSourcesAndDynamicSourceLoader(source, inputs, loader), R.UnexpectedBinding);
            Reject(() => G.ExecuteWithSourcesContextAndDynamicSourceLoader(source, inputs, empty, loader), R.UnexpectedBinding);
            Reject(() => G.ExecuteOutputsWithSourcesContextAndDynamicSourceLoader(source, inputs, empty, loader), R.UnexpectedBinding);
            Reject(() => G.ExecuteSelectedTargetWithHost(source, named, inputs, empty, loader), R.UnexpectedBinding);
            var jsonInputs = Enumerable.Repeat(new NamedJsonInput("secret", "invalid-json"), n).ToArray();
            var jsonBytesInputs = Enumerable.Repeat(new NamedJsonBytesInput("secret", [255]), n).ToArray();
            Reject(() => G.ExecuteJsonWithSources(Json, jsonInputs), R.UnexpectedBinding);
            Reject(() => G.ExecuteJsonOutputsWithSources(Json, jsonInputs), R.UnexpectedBinding);
            Reject(() => G.ExecuteJsonWithSources(Json, jsonInputs, empty), R.UnexpectedBinding);
            Reject(() => G.ExecuteJsonOutputsWithSources(Json, jsonInputs, empty), R.UnexpectedBinding);
            Reject(() => G.ExecuteJsonBytesWithSources(bytes, jsonBytesInputs), R.UnexpectedBinding);
            Reject(() => G.ExecuteJsonBytesOutputsWithSources(bytes, jsonBytesInputs), R.UnexpectedBinding);
            Reject(() => G.ExecuteJsonBytesWithSources(bytes, jsonBytesInputs, empty), R.UnexpectedBinding);
            Reject(() => G.ExecuteJsonBytesOutputsWithSources(bytes, jsonBytesInputs, empty), R.UnexpectedBinding);
            Reject(() => G.ExecuteJsonSelectedTargetWithHost(Json, named, jsonInputs, empty, loader), R.UnexpectedBinding);
            Reject(() => G.ExecuteJsonBytesSelectedTargetWithHost(bytes, primary, jsonBytesInputs, empty, loader), R.UnexpectedBinding);
            var xmlInputs = Enumerable.Repeat(new NamedXmlInput("secret", "invalid-xml"), n).ToArray();
            var xmlBytesInputs = Enumerable.Repeat(new NamedXmlBytesInput("secret", [255]), n).ToArray();
            Reject(() => G.ExecuteXmlWithSources(Xml, xmlInputs), R.UnexpectedBinding);
            Reject(() => G.ExecuteXmlOutputsWithSources(Xml, xmlInputs), R.UnexpectedBinding);
            Reject(() => G.ExecuteXmlWithSources(Xml, xmlInputs, empty), R.UnexpectedBinding);
            Reject(() => G.ExecuteXmlOutputsWithSources(Xml, xmlInputs, empty), R.UnexpectedBinding);
            Reject(() => G.ExecuteXmlBytesWithSources(xmlBytes, xmlBytesInputs), R.UnexpectedBinding);
            Reject(() => G.ExecuteXmlBytesOutputsWithSources(xmlBytes, xmlBytesInputs), R.UnexpectedBinding);
            Reject(() => G.ExecuteXmlBytesWithSources(xmlBytes, xmlBytesInputs, empty), R.UnexpectedBinding);
            Reject(() => G.ExecuteXmlBytesOutputsWithSources(xmlBytes, xmlBytesInputs, empty), R.UnexpectedBinding);
            Reject(() => G.ExecuteXmlBytesSelectedTargetWithHost(xmlBytes, named, xmlBytesInputs, empty), R.UnexpectedBinding);
        }
        Reject(() => G.Execute(source, execution), R.UnexpectedParameter);
        Reject(() => G.ExecuteOutputs(source, execution), R.UnexpectedParameter);
        Reject(() => G.ExecuteWithSources(source, [], execution), R.UnexpectedParameter);
        Reject(() => G.ExecuteOutputsWithSources(source, [], execution), R.UnexpectedParameter);
        Reject(() => G.ExecuteWithSourcesContextAndDynamicSourceLoader(source, [], execution, loader), R.UnexpectedParameter);
        Reject(() => G.ExecuteOutputsWithSourcesContextAndDynamicSourceLoader(source, [], execution, loader), R.UnexpectedParameter);
        Reject(() => G.ExecuteSelectedTargetWithHost(source, named, [], execution, loader), R.UnexpectedParameter);
        Reject(() => G.ExecuteWithDynamicSourceLoader(Input(hidden: true), loader), R.UnexpectedField);
        Reject(() => G.ExecuteOutputsWithDynamicSourceLoader(Input(hidden: true), loader), R.UnexpectedField);
        Reject(() => G.ExecuteJson(Json, execution), R.UnexpectedParameter);
        Reject(() => G.ExecuteJsonOutputs(Json, execution), R.UnexpectedParameter);
        Reject(() => G.ExecuteJsonBytes(bytes, execution), R.UnexpectedParameter);
        Reject(() => G.ExecuteJsonBytesOutputs(bytes, execution), R.UnexpectedParameter);
        Reject(() => G.ExecuteJsonSelectedTargetWithHost(Json, primary, [], execution, loader), R.UnexpectedParameter);
        Reject(() => G.ExecuteJsonBytesSelectedTargetWithHost(bytes, named, [], execution, loader), R.UnexpectedParameter);
        Reject(() => G.ExecuteXml(Xml, execution), R.UnexpectedParameter);
        Reject(() => G.ExecuteXmlOutputs(Xml, execution), R.UnexpectedParameter);
        Reject(() => G.ExecuteXmlBytes(xmlBytes, execution), R.UnexpectedParameter);
        Reject(() => G.ExecuteXmlBytesOutputs(xmlBytes, execution), R.UnexpectedParameter);
        Reject(() => G.ExecuteXmlBytesSelectedTargetWithHost(xmlBytes, primary, [], execution), R.UnexpectedParameter);
        if (!G.ExecuteJson(Json).Contains("OUTER1", StringComparison.Ordinal) || !G.ExecuteXml(Xml).Contains("OUTER1", StringComparison.Ordinal)) throw new Exception("transport control");
        if (Loads != 0) throw new Exception("loader callback ran");
        Console.WriteLine($"guard-host-ok cases={Cases} loads=0");
    }
}

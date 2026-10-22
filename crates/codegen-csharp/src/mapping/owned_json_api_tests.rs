use std::{fs, process::Command};

use ::mapping::{OwnedSourceField, OwnedSourceRef, SourceOwner};
use codegen::{
    ArtifactSet, Binding, Expression, ExpressionNode, NamedSourceProgram, NamedTargetProgram,
    Program, TargetScope,
};
use ir::{ScalarType, SchemaNode, Value};

use crate::{generated_artifact_evidence::Evidence, owned_basic};

fn scope(expression: u32) -> TargetScope {
    TargetScope {
        target_field: String::new(),
        repeating: false,
        iteration: None,
        construction: Default::default(),
        bindings: vec![Binding {
            target_field: "Value".into(),
            expression,
            target_domain: ScalarType::String.into(),
            repeating: false,
        }],
        children: vec![],
    }
}

fn owned(owner: SourceOwner, path: &[&str]) -> Expression {
    Expression::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path.iter().map(|part| (*part).into()).collect(),
        },
        frame: None,
    })
}

fn program() -> Program {
    let target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    Program {
        xml_boundary: None,
        source: SchemaNode::group(
            "Input",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        extra_sources: vec![
            NamedSourceProgram {
                name: "Reserved".into(),
                source: SchemaNode::scalar("Dynamic", ScalarType::Bool),
                dynamic: Some(codegen::DynamicSourceProgram {
                    path: 3,
                    driver: codegen::SourceIteration::new(vec![]).into(),
                }),
            },
            NamedSourceProgram {
                name: "Ref".into(),
                source: SchemaNode::scalar("Reference", ScalarType::String),
                dynamic: None,
            },
            NamedSourceProgram {
                name: "ref".into(),
                source: SchemaNode::scalar("Optional", ScalarType::Int),
                dynamic: None,
            },
        ],
        target: target.clone(),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: owned(SourceOwner::Primary, &["Value"]),
            },
            ExpressionNode {
                id: 2,
                expression: owned(SourceOwner::Named { name: "Ref".into() }, &[]),
            },
            ExpressionNode {
                id: 3,
                expression: Expression::Const {
                    value: Value::String("unused.json".into()),
                },
            },
        ],
        user_functions: vec![],
        failure_rules: vec![],
        root: scope(1),
        extra_targets: vec![NamedTargetProgram {
            name: " Ref 雪 ".into(),
            target,
            root: scope(2),
        }],
    }
}

fn mapping(artifacts: &ArtifactSet) -> &str {
    let mapping = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .expect("mapping artifact");
    std::str::from_utf8(&mapping.contents).expect("UTF-8 mapping")
}

fn companion(source: &str) -> &str {
    &source[source
        .find("\npublic sealed record NamedJsonInput(")
        .expect("JSON types")..]
}

#[test]
fn basic_owned_json520_companions_are_append_only_and_profile_specific() {
    let program = program();
    let evidence = Evidence::new("csharp-owned-json520-profiles");
    evidence.debug("PROGRAM.original.txt", &program);
    let before = owned_basic::render_public_basic_owned_mapping(&program).unwrap();
    let actual = crate::emit_with_basic_owned_sources(&program);
    evidence.artifacts("explicit-basic", &actual);
    let actual = actual.unwrap();
    assert_eq!(
        actual,
        crate::emit_with_basic_owned_sources(&program).unwrap()
    );
    assert!(mapping(&actual).starts_with(&before));
    assert_eq!(
        companion(mapping(&actual))
            .matches("public static ")
            .count(),
        5
    );
    for preset in [
        codegen::OwnedPreset::Filters,
        codegen::OwnedPreset::Sort,
        codegen::OwnedPreset::Windows,
        codegen::OwnedPreset::RowFailures,
    ] {
        let mapping = owned_basic::render_public_basic_owned_capability_mapping(
            &program,
            preset.capabilities(),
        );
        assert!(!mapping.unwrap().contains("NamedJsonInput"));
    }
    let mut legacy = program;
    legacy.expressions[0].expression = Expression::SourceField {
        frame: None,
        path: vec!["Value".into()],
    };
    legacy.expressions[1].expression = Expression::Const {
        value: Value::String("ordinary".into()),
    };
    let ordinary = crate::emit(&legacy).unwrap();
    assert!(!mapping(&ordinary).contains("ExecuteJsonOutputsWithBasicOwnedSources"));
    assert!(mapping(&ordinary).contains("ExecuteJsonOutputsWithSources"));
}

#[test]
fn basic_owned_json520_methods_require_policy_and_preserve_boundary_order() {
    let artifacts = crate::emit_with_basic_owned_sources(&program()).unwrap();
    let source = companion(mapping(&artifacts));
    for (stem, document, input) in [
        ("Json", "string", "NamedJsonInput"),
        ("JsonBytes", "byte[]", "NamedJsonBytesInput"),
    ] {
        for selected in [false, true] {
            let name = if selected {
                format!("Execute{stem}SelectedTargetWithBasicOwnedSources")
            } else {
                format!("Execute{stem}OutputsWithBasicOwnedSources")
            };
            assert_eq!(source.matches(&format!(" {name}(")).count(), 1);
            let method = &source[source.find(&format!(" {name}(")).unwrap()..];
            let signature = &method[..method.find("\n    {").unwrap()];
            assert!(signature.contains(&format!("{document} source,")));
            assert!(signature.contains(&format!("IReadOnlyList<{input}> inputs,")));
            assert!(signature.contains("FerruleOwnedSourceLimits limits,"));
            assert!(signature.contains("FerruleExecutionContext? execution)"));
            assert!(!signature.contains('='));
            assert!(!signature.contains("FerruleOwnedSourceLimits?"));
            let method = &method[..method
                .find("finally { global::System.GC.KeepAlive(parsed); }")
                .unwrap()];
            let source_guard = method.find("ThrowIfNull(source)").unwrap();
            let inputs_guard = method.find("ThrowIfNull(inputs)").unwrap();
            let admit = method
                .find("FerruleBasicJsonInputAdmission.Admit(")
                .unwrap();
            let primary = method
                .find("var primary = global::Ferrule.Runtime.FerruleJson.ParseEmbedded")
                .unwrap();
            let named = method
                .find("admission.ParseNamedWith(ParseBasicOwnedNamedJsonDocument)")
                .unwrap();
            let roots = method.find("parsed.BasicRoots(primary)").unwrap();
            let context = method.find("roots.RootContext(execution)").unwrap();
            let evaluate = method.find("ExecuteCheckedBasicContext(context,").unwrap();
            let serialize = method.find("return SerializeBasicOwned").unwrap();
            assert!(source_guard < inputs_guard && inputs_guard < admit && admit < primary);
            assert!(primary < named && named < roots && roots < context);
            assert!(context < evaluate && evaluate < serialize);
            assert_eq!(method.matches(".BasicRoots(").count(), 1);
            assert_eq!(method.matches(".RootContext(").count(), 1);
            assert_eq!(method.matches("ExecuteCheckedBasicContext(").count(), 1);
            if selected {
                let selection_guard = method.find("ThrowIfNull(selection)").unwrap();
                let resolve = method.find("ResolveOwnedBasicTarget(selection)").unwrap();
                assert!(
                    source_guard < selection_guard
                        && selection_guard < resolve
                        && resolve < inputs_guard
                );
                assert!(method.contains("0 => CheckedPrimaryInputs"));
                assert!(method.contains("1 => CheckedNamedInputs_0"));
            } else {
                assert!(method.contains("CheckedAllInputs, inputs,"));
            }
        }
    }
    for forbidden in [
        "new NamedInput[",
        "new global::System.Collections.Generic.List<",
        "inputs.Count",
        "inputs[",
        "foreach",
        "ExecuteWithBasicOwnedSources(",
        "ExecuteSelectedWithBasicOwnedSources(",
        "CreateFromNamedInputs(",
        "catch (",
        "SerializeEmbeddedBytes(",
        "SerializeEmbedded(",
    ] {
        assert!(
            !source.contains(forbidden),
            "unexpected companion route: {forbidden}"
        );
    }
    assert!(source.contains("FerruleJson.SerializeEmbeddedCompact("));
    assert!(source.contains("FerruleJson.SerializeEmbeddedCompactBytes("));
}

#[test]
fn basic_owned_json520_accessors_capture_closed_handles_and_original_ordinals() {
    let artifacts = crate::emit_with_basic_owned_sources(&program()).unwrap();
    let source = companion(mapping(&artifacts));
    assert!(source.is_ascii());
    assert_eq!(
        source
            .matches("sealed class BasicOwnedJsonInputAccessor")
            .count(),
        1
    );
    assert_eq!(
        source
            .matches("sealed class BasicOwnedJsonBytesInputAccessor")
            .count(),
        1
    );
    assert_eq!(
        source
            .matches("internal static readonly BasicOwnedJson")
            .count(),
        2
    );
    assert!(source.contains("IFerruleNamedJsonDocumentAccessor<NamedJsonInput>"));
    assert!(source.contains("IFerruleNamedJsonDocumentAccessor<NamedJsonBytesInput>"));
    assert!(source.contains("FerruleJsonDocumentRef.FromText(item.Document)"));
    assert!(source.contains("FerruleJsonDocumentRef.FromUtf8(item.Document)"));
    assert!(source.contains("Read(NamedJsonInput? item)"));
    assert!(source.contains("Read(NamedJsonBytesInput? item)"));
    for ordinal in 0..3 {
        assert!(source.contains(&format!(
            "{ordinal} => BasicOwnedSourceJsonSchema_{ordinal}"
        )));
    }
    assert!(source.contains("string schema = declarationIndex switch"));
    assert!(!source.contains("switch (item.Name)"));
    assert!(!source.contains("StringComparer"));
    let names = mapping(&artifacts);
    assert!(names.contains("new(\"Reserved\", global::Ferrule.Runtime.FerruleBasicNamedSourceMode.ReservedUnavailable), // declaration 0"));
    assert!(names.contains("new(\"Ref\", global::Ferrule.Runtime.FerruleBasicNamedSourceMode.Optional), // declaration 1"));
    assert!(names.contains("new(\"ref\", global::Ferrule.Runtime.FerruleBasicNamedSourceMode.Optional), // declaration 2"));
}

#[test]
fn basic_owned_json520_empty_catalogue_needs_no_named_schema_or_parser_switch() {
    let mut program = program();
    program.source = SchemaNode::scalar("Input", ScalarType::String);
    program.expressions[0].expression = owned(SourceOwner::Primary, &[]);
    program.extra_sources.clear();
    program.extra_targets.clear();
    program.expressions.truncate(1);
    let artifacts = crate::emit_with_basic_owned_sources(&program).unwrap();
    let source = companion(mapping(&artifacts));
    assert!(!source.contains("declarationIndex switch"));
    assert!(source.contains("throw new global::System.InvalidOperationException(\"Admitted JSON declaration is invalid.\");"));
    assert!(!source.contains("BasicOwnedSourceJsonSchema_0"));
    assert_eq!(source.matches("parsed.BasicRoots(primary)").count(), 4);
}

#[test]
#[ignore = "requires the coordinator's serialized .NET compiler lane"]
fn basic_owned_json520_generated_consumer_checks_capture_compact_bytes_and_policy() {
    let program = program();
    let evidence = Evidence::new("csharp-owned-json520-consumer");
    evidence.debug("PROGRAM.original.txt", &program);
    let artifacts = crate::emit_with_basic_owned_sources(&program);
    evidence.artifacts("generated", &artifacts);
    let artifacts = artifacts.expect("complete emission or refusal retained");
    evidence.bytes(
        "consumer/Consumer.csproj",
        &Ok::<_, std::io::Error>(CONSUMER_PROJECT),
    );
    evidence.bytes("consumer/Program.cs", &Ok::<_, std::io::Error>(CONSUMER));
    let mut build = consumer_build_command(&evidence, "consumer/Consumer.csproj");
    evidence.debug("BUILD_COMMAND.original.txt", &build);
    let built = build.output();
    evidence.debug("BUILD.original.txt", &built);
    let built = built.expect("build result retained");
    evidence.bytes("BUILD.stdout.bin", &Ok::<_, std::io::Error>(&built.stdout));
    evidence.bytes("BUILD.stderr.bin", &Ok::<_, std::io::Error>(&built.stderr));
    assert!(
        built.status.success(),
        "complete build at {}",
        evidence.path().display()
    );
    let assembly = evidence
        .path()
        .join("consumer/bin/Debug/net10.0/Consumer.dll");
    assert!(fs::metadata(&assembly).unwrap().is_file());
    let run = Command::new("dotnet")
        .arg(assembly)
        .current_dir(evidence.path())
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .env("DOTNET_PROCESSOR_COUNT", "2")
        .output();
    evidence.debug("RUN.original.txt", &run);
    let run = run.expect("consumer result retained");
    evidence.bytes("RUN.stdout.bin", &Ok::<_, std::io::Error>(&run.stdout));
    evidence.bytes("RUN.stderr.bin", &Ok::<_, std::io::Error>(&run.stderr));
    assert!(
        run.status.success(),
        "complete consumer at {}",
        evidence.path().display()
    );

    // Compile these four independent omissions only after the positive project
    // has built and run. Both projects reference the very same emitted files;
    // neither the positive host nor the artifact is rewritten for this control.
    evidence.bytes(
        "negative-consumer/NegativeConsumer.csproj",
        &Ok::<_, std::io::Error>(CONSUMER_PROJECT),
    );
    evidence.bytes(
        "negative-consumer/Program.cs",
        &Ok::<_, std::io::Error>(OMITTED_LIMITS_CONSUMER),
    );
    let mut negative =
        consumer_build_command(&evidence, "negative-consumer/NegativeConsumer.csproj");
    // Command Debug retains argv, controlled environment, and working directory.
    // Cache locations are inherited, not redirected into an empty evidence cache.
    evidence.debug("NEGATIVE_BUILD_COMMAND.original.txt", &negative);
    evidence.debug(
        "NEGATIVE_BUILD_INHERITED_CACHE_ENV.original.txt",
        &[
            ("HOME", std::env::var_os("HOME")),
            ("USERPROFILE", std::env::var_os("USERPROFILE")),
            ("NUGET_PACKAGES", std::env::var_os("NUGET_PACKAGES")),
        ],
    );
    let negative = negative.output();
    evidence.debug("NEGATIVE_BUILD.original.txt", &negative);
    let negative = negative.expect("negative build result retained");
    evidence.bytes(
        "NEGATIVE_BUILD.stdout.bin",
        &Ok::<_, std::io::Error>(&negative.stdout),
    );
    evidence.bytes(
        "NEGATIVE_BUILD.stderr.bin",
        &Ok::<_, std::io::Error>(&negative.stderr),
    );
    evidence.assert_retained("generated", &artifacts);
    assert_eq!(
        fs::read(evidence.path().join("consumer/Program.cs")).unwrap(),
        CONSUMER.as_bytes(),
    );
    assert_eq!(
        fs::read(evidence.path().join("negative-consumer/Program.cs")).unwrap(),
        OMITTED_LIMITS_CONSUMER.as_bytes(),
    );
    assert!(
        !negative.status.success(),
        "omitted limits unexpectedly compiled; originals at {}",
        evidence.path().display(),
    );
    let stdout = String::from_utf8_lossy(&negative.stdout);
    let stderr = String::from_utf8_lossy(&negative.stderr);
    let diagnostics = stdout
        .lines()
        .chain(stderr.lines())
        .filter(|line| line.contains("error "))
        .collect::<Vec<_>>();
    let methods = [
        "ExecuteJsonOutputsWithBasicOwnedSources",
        "ExecuteJsonBytesOutputsWithBasicOwnedSources",
        "ExecuteJsonSelectedTargetWithBasicOwnedSources",
        "ExecuteJsonBytesSelectedTargetWithBasicOwnedSources",
    ];
    assert!(
        !diagnostics.is_empty()
            && diagnostics.iter().all(|line| {
                line.contains("error CS7036:")
                    && line.contains("required parameter 'limits'")
                    && methods
                        .iter()
                        .any(|method| line.contains(&format!("{method}(")))
            }),
        "only missing-limits CS7036 diagnostics are accepted, not unresolved APIs/types or restore failures; originals at {}",
        evidence.path().display(),
    );
    for method in methods {
        assert!(
            diagnostics
                .iter()
                .any(|line| line.contains(&format!("{method}("))),
            "missing CS7036 required-parameter 'limits' witness for {method}; originals at {}",
            evidence.path().display(),
        );
    }
}

fn consumer_build_command(evidence: &Evidence, project: &str) -> Command {
    let mut command = Command::new("dotnet");
    command
        .args([
            "build",
            project,
            "--nologo",
            "--disable-build-servers",
            "-m:1",
            "-p:UseSharedCompilation=false",
            "-p:BuildInParallel=false",
            "-p:NuGetAudit=false",
        ])
        .current_dir(evidence.path())
        .env("DOTNET_CLI_HOME", evidence.path().join(".dotnet-home"))
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
        .env("DOTNET_CLI_UI_LANGUAGE", "en-US")
        .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
        .env("MSBUILDDISABLENODEREUSE", "1")
        .env("DOTNET_PROCESSOR_COUNT", "2");
    command
}

const CONSUMER_PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit>
  </PropertyGroup>
  <ItemGroup><ProjectReference Include="../generated/artifacts/Ferrule.Generated.csproj" /></ItemGroup>
</Project>
"#;

const OMITTED_LIMITS_CONSUMER: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

_ = GeneratedMapping.ExecuteJsonOutputsWithBasicOwnedSources(
    "{}", Array.Empty<NamedJsonInput>(), execution: null);
_ = GeneratedMapping.ExecuteJsonBytesOutputsWithBasicOwnedSources(
    Array.Empty<byte>(), Array.Empty<NamedJsonBytesInput>(), execution: null);
_ = GeneratedMapping.ExecuteJsonSelectedTargetWithBasicOwnedSources(
    "{}", FerruleTargetSelection.Primary(), Array.Empty<NamedJsonInput>(), execution: null);
_ = GeneratedMapping.ExecuteJsonBytesSelectedTargetWithBasicOwnedSources(
    Array.Empty<byte>(), FerruleTargetSelection.Primary(), Array.Empty<NamedJsonBytesInput>(), execution: null);
"#;

const CONSUMER: &str = r#"using System.Collections;
using System.Reflection;
using System.Text;
using System.Text.Json;
using Ferrule.Generated;
using Ferrule.Runtime;

static class Consumer
{
    private static void Check(bool value, string expected)
    { if (!value) throw new InvalidOperationException(expected); }
    private static void Error(Func<object> call, FerruleRuntimeError kind)
    {
        try { Console.WriteLine(JsonSerializer.Serialize(call())); }
        catch (FerruleRuntimeException error)
        {
            Console.WriteLine(error.ToString());
            Check(error.Error == kind, "exact failure category"); return;
        }
        throw new InvalidOperationException("expected failure without a result");
    }
    private sealed class Probe<T>(T[] values) : IReadOnlyList<T>
    {
        public int CountReads, IndexReads;
        private readonly bool[] _read = new bool[values.Length];
        public int Count { get { CountReads++; Check(CountReads == 1, "Count read once"); return values.Length; } }
        public T this[int index] { get { IndexReads++; Check(!_read[index], "index read once"); _read[index] = true; return values[index]; } }
        public IEnumerator<T> GetEnumerator() => throw new InvalidOperationException("caller enumeration forbidden");
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
    }
    public static void Main()
    {
        var methods = typeof(GeneratedMapping).GetMethods(BindingFlags.Public | BindingFlags.Static | BindingFlags.DeclaredOnly);
        Console.WriteLine(JsonSerializer.Serialize(methods.Select(method => new {method.Name,
            parameters = method.GetParameters().Select(parameter => new {parameter.Name,
                type = parameter.ParameterType.FullName, parameter.IsOptional, parameter.HasDefaultValue})})));
        Check(methods.Length == 6, "two typed plus four document methods only");
        Check(methods.SelectMany(method => method.GetParameters()).All(parameter => !parameter.IsOptional && !parameter.HasDefaultValue), "all parameters mandatory");
        var limits = new FerruleOwnedSourceLimits(8,8,16,1_048_576,64);
        const string input = "{\"Value\":\"P\"}";
        var text = new Probe<NamedJsonInput>([new("ref", "7"), new("Ref", "\"N\"")]);
        var all = GeneratedMapping.ExecuteJsonOutputsWithBasicOwnedSources(input, text, limits, null);
        Console.WriteLine(JsonSerializer.Serialize(all));
        Check(all.Primary == "{\"Value\":\"P\"}" && all.Extras.Count == 1 && all.Extras[0].Name == " Ref 雪 " && all.Extras[0].Document == "{\"Value\":\"N\"}", "whole compact all outputs with original schemas");
        Check(text.CountReads == 1 && text.IndexReads == 2, "capture only, no parse/bind reread");
        var bytes = new Probe<NamedJsonBytesInput>([new("Ref", Encoding.UTF8.GetBytes("\"N\""))]);
        var allBytes = GeneratedMapping.ExecuteJsonBytesOutputsWithBasicOwnedSources(Encoding.UTF8.GetBytes(input), bytes, limits, null);
        Console.WriteLine(JsonSerializer.Serialize(allBytes));
        Check(allBytes.Primary.SequenceEqual(Encoding.UTF8.GetBytes(all.Primary)) && allBytes.Extras[0].Document.SequenceEqual(Encoding.UTF8.GetBytes(all.Extras[0].Document)), "exact compact UTF8, no LF or BOM");
        var primary = GeneratedMapping.ExecuteJsonSelectedTargetWithBasicOwnedSources(input, FerruleTargetSelection.Primary(), Array.Empty<NamedJsonInput>(), limits, null);
        Check(primary is SelectedJsonTargetOutput.Primary selected && selected.Document == all.Primary, "selected primary does not evaluate absent named owner");
        var named = GeneratedMapping.ExecuteJsonBytesSelectedTargetWithBasicOwnedSources(Encoding.UTF8.GetBytes(input), FerruleTargetSelection.Named(" Ref 雪 "), [new NamedJsonBytesInput("Ref", Encoding.UTF8.GetBytes("\"N\""))], limits, null);
        Check(named is SelectedJsonBytesTargetOutput.Named selectedNamed && selectedNamed.Output.Name == " Ref 雪 " && selectedNamed.Output.Document.SequenceEqual(allBytes.Extras[0].Document), "exact selected named bytes");
        var unread = new Probe<NamedJsonInput>([]);
        Error(() => GeneratedMapping.ExecuteJsonSelectedTargetWithBasicOwnedSources("{", FerruleTargetSelection.Named("missing"), unread, default, null), FerruleRuntimeError.UnknownTarget);
        Check(unread.CountReads == 0 && unread.IndexReads == 0, "unknown target before policy/list/parser");
        Error(() => GeneratedMapping.ExecuteJsonSelectedTargetWithBasicOwnedSources(input, FerruleTargetSelection.Primary(), [new NamedJsonInput("ref", "{")], limits, null), FerruleRuntimeError.JsonBoundary);
        Console.WriteLine("Basic JSON520 focused consumer passed");
    }
}
"#;

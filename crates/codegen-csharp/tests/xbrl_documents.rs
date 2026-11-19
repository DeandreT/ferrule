use codegen::{
    IterationPlan, NamedSourceProgram, NamedTargetProgram, Program, TargetConstruction, TargetScope,
};
use codegen_csharp::xbrl_documents::{Policy, emit};
use ir::{ScalarType, SchemaNode};
use mapping::{XbrlBoundaryOptions as Options, XbrlNamespaceBinding as Binding};
use std::{fs, process::Command};
use test_support::DotnetTemp;
const I: &str = "http://www.xbrl.org/2003/instance";
fn schema() -> SchemaNode {
    let s = |n: &str| SchemaNode::scalar(n, ScalarType::String);
    SchemaNode::group(
        "Report",
        vec![
            SchemaNode::group(
                "items",
                vec![
                    SchemaNode::group(
                        "identifier",
                        vec![s("scheme").attribute(), s("#text").text()],
                    ),
                    SchemaNode::group("period", vec![s("instant")]),
                    SchemaNode::scalar("Quantity", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    )
}
fn options(input: bool, ns: &str) -> Options {
    let o = if input {
        Options::external_source("https://must-not-resolve.invalid/test.xsd")
    } else {
        Options::external_target("https://must-not-resolve.invalid/test.xsd", None)
    }
    .unwrap();
    o.with_namespace_bindings(
        [
            ("identifier", I),
            ("period", I),
            ("period/instant", I),
            ("Quantity", ns),
        ]
        .into_iter()
        .map(|(path, u)| {
            Binding::new(
                std::iter::once("items")
                    .chain(path.split('/'))
                    .map(str::to_owned)
                    .collect(),
                u,
            )
            .unwrap()
        })
        .collect(),
    )
    .unwrap()
}
fn program() -> Program {
    let copy = TargetScope {
        target_field: String::new(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::CopyCurrentSource,
        bindings: vec![],
        children: vec![],
    };
    let named = TargetScope {
        construction: TargetConstruction::Group,
        children: vec![TargetScope {
            target_field: "items".into(),
            repeating: true,
            iteration: Some(IterationPlan::source(vec!["Peer".into(), "items".into()])),
            ..copy.clone()
        }],
        ..copy.clone()
    };
    Program {
        execution_contract: None,
        xml_boundary: None,
        source: schema(),
        extra_sources: vec![NamedSourceProgram {
            name: "Peer".into(),
            source: schema(),
            dynamic: None,
        }],
        target: schema(),
        root: copy,
        extra_targets: vec![NamedTargetProgram {
            name: "Other".into(),
            target: schema(),
            root: named,
        }],
        expressions: vec![],
        user_functions: vec![],
        failure_rules: vec![],
    }
}
#[test]
fn refuses_inexact_option_names_and_unsupported_shapes() {
    let source = options(true, "urn:s");
    let target = options(false, "urn:t");
    let mut p = program();
    let policy = Policy {
        source: &source,
        target: &target,
        extra_sources: &[("Peer", &source)],
        extra_targets: &[("Other", &target)],
    };
    assert!(emit(&p, &policy).is_ok());
    p.extra_sources[0].name = "peer".into();
    assert!(emit(&p, &policy).is_err());
    p.extra_sources[0].name = "Peer".into();
    p.target = SchemaNode::scalar("wrong", ScalarType::Bool);
    assert!(emit(&p, &policy).is_err());
}
#[test]
#[ignore = "requires the .NET 10 SDK"]
fn generated_selected_and_complete_bytes_match_native_xbrl() {
    let p = program();
    let a = options(true, "urn:first");
    let b = options(true, "urn:second");
    let c = options(false, "urn:primary-out");
    let d = options(false, "urn:named-out");
    let lib = emit(
        &p,
        &Policy {
            source: &a,
            target: &c,
            extra_sources: &[("Peer", &b)],
            extra_targets: &[("Other", &d)],
        },
    )
    .unwrap();
    let root = std::env::temp_dir().join(format!(
        "ferrule-xbrl-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    for f in lib.files() {
        let path = root.join("library").join(f.path.as_str());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &f.contents).unwrap();
    }
    for (label, ns, input, output, amount) in [
        ("first", "urn:first", &a, &c, "0042"),
        ("second", "urn:second", &b, &d, "-923"),
    ] {
        let xml = format!(
            r#"<i:xbrl xmlns:i="{I}" xmlns:f="{ns}" xmlns:l="http://www.xbrl.org/2003/linkbase" xmlns:x="http://www.w3.org/1999/xlink"><l:schemaRef x:type="simple" x:href="file:///must-not-resolve"/><f:Quantity contextRef="k" unitRef="u">{amount}</f:Quantity><i:context id="k"><i:entity><i:identifier scheme="urn:id">{label} &amp; others</i:identifier></i:entity><i:period><i:instant>2026-11-01</i:instant></i:period></i:context><i:unit id="u"><i:measure>i:pure</i:measure></i:unit></i:xbrl>"#
        );
        let model =
            format_xbrl::from_bytes_bounded(xml.as_bytes(), &schema(), input, &Default::default())
                .unwrap();
        let expected =
            format_xbrl::to_bytes_bounded(&schema(), &model, output, &Default::default()).unwrap();
        fs::write(root.join(label), xml).unwrap();
        fs::write(root.join(format!("{label}.expected")), expected).unwrap();
    }
    fs::create_dir(root.join("host")).unwrap();
    fs::write(root.join("host/host.csproj"),r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings></PropertyGroup><ItemGroup><ProjectReference Include="../library/Ferrule.Generated.csproj"/></ItemGroup></Project>"#).unwrap();
    fs::write(
        root.join("host/Program.cs"),
        include_str!("xbrl_host.cs.in"),
    )
    .unwrap();
    fs::write(
        root.join("NuGet.Config"),
        "<configuration><packageSources><clear/></packageSources></configuration>",
    )
    .unwrap();
    let status = Command::new("dotnet")
        .current_dir(&root)
        .dotnet_temp(&root)
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .env("DOTNET_CLI_HOME", root.join("home"))
        .args([
            "run",
            "--project",
            "host",
            "-p:UseSharedCompilation=false",
            "-p:NuGetAudit=false",
        ])
        .status()
        .unwrap();
    assert!(
        status.success(),
        "generated XBRL host failed at {}",
        root.display()
    );
    fs::remove_dir_all(root).unwrap();
}

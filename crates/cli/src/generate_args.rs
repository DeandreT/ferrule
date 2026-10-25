//! Identity and compatibility flags of `ferrule generate`.

use anyhow::bail;

/// Generated library names and the optional compatibility manifest. Omitted
/// names keep their defaults, so output without these flags is unchanged.
#[derive(Debug, Clone, Default, clap::Args)]
pub(crate) struct GenerateIdentityArgs {
    /// Package name of the generated Rust crate. Default: ferrule-generated-mapping.
    #[arg(long, value_name = "NAME")]
    pub(crate) rust_package_name: Option<String>,
    /// Assembly and project-file name of the generated C# library.
    /// Default: Ferrule.Generated.
    #[arg(long, value_name = "NAME")]
    pub(crate) csharp_assembly_name: Option<String>,
    /// Root namespace of the generated C# library. Default: Ferrule.Generated.
    #[arg(long, value_name = "NAMESPACE")]
    pub(crate) csharp_namespace: Option<String>,
    /// Name of the generated C# mapping class; it must end with "Mapping".
    /// Default: GeneratedMapping.
    #[arg(long, value_name = "NAME")]
    pub(crate) csharp_class: Option<String>,
    /// Add ferrule-manifest.json and a generated check that refuses an
    /// incompatible runtime. A Rust runtime path must then match the
    /// generator's runtime identity.
    #[arg(long)]
    pub(crate) compatibility_manifest: bool,
}

impl GenerateIdentityArgs {
    /// Validates the flags for the selected language before anything is read.
    pub(crate) fn settings(self, rust: bool) -> anyhow::Result<cli::GenerateSettings> {
        let csharp = [
            ("--csharp-assembly-name", &self.csharp_assembly_name),
            ("--csharp-namespace", &self.csharp_namespace),
            ("--csharp-class", &self.csharp_class),
        ];
        if rust {
            if let Some((flag, _)) = csharp.iter().find(|(_, value)| value.is_some()) {
                bail!("{flag} applies only to --language csharp");
            }
        } else if self.rust_package_name.is_some() {
            bail!("--rust-package-name applies only to --language rust");
        }
        let rust_package_name = self
            .rust_package_name
            .as_deref()
            .map(codegen_rust::PackageName::new)
            .transpose()?;
        let csharp_identity = if csharp.iter().any(|(_, value)| value.is_some()) {
            let default = codegen_csharp::Identity::default();
            Some(codegen_csharp::Identity::new(
                self.csharp_assembly_name
                    .as_deref()
                    .unwrap_or(default.assembly_name()),
                self.csharp_namespace
                    .as_deref()
                    .unwrap_or(default.root_namespace()),
                self.csharp_class
                    .as_deref()
                    .unwrap_or(default.mapping_class()),
            )?)
        } else {
            None
        };
        Ok(cli::GenerateSettings {
            rust_package_name,
            csharp_identity,
            manifest: if self.compatibility_manifest {
                codegen::ManifestSelection::Included
            } else {
                codegen::ManifestSelection::Omitted
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(
        rust_package_name: Option<&str>,
        csharp: [Option<&str>; 3],
        compatibility_manifest: bool,
    ) -> GenerateIdentityArgs {
        GenerateIdentityArgs {
            rust_package_name: rust_package_name.map(str::to_owned),
            csharp_assembly_name: csharp[0].map(str::to_owned),
            csharp_namespace: csharp[1].map(str::to_owned),
            csharp_class: csharp[2].map(str::to_owned),
            compatibility_manifest,
        }
    }

    #[test]
    fn omitted_flags_keep_the_default_settings() {
        for rust in [false, true] {
            assert_eq!(
                GenerateIdentityArgs::default().settings(rust).unwrap(),
                cli::GenerateSettings::default()
            );
        }
    }

    #[test]
    fn identities_are_validated_per_language_with_typed_refusals() {
        let settings = args(
            None,
            [
                Some("Acme.Orders"),
                Some("Acme.Orders"),
                Some("OrderMapping"),
            ],
            true,
        )
        .settings(false)
        .unwrap();
        let identity = settings.csharp_identity.unwrap();
        assert_eq!(identity.assembly_name(), "Acme.Orders");
        assert_eq!(settings.manifest, codegen::ManifestSelection::Included);
        let partial = args(None, [None, None, Some("OrderMapping")], false)
            .settings(false)
            .unwrap();
        let partial = partial.csharp_identity.unwrap();
        assert_eq!(partial.root_namespace(), "Ferrule.Generated");
        assert_eq!(partial.mapping_class(), "OrderMapping");
        let rust = args(Some("orders-mapping"), [None; 3], false)
            .settings(true)
            .unwrap();
        assert_eq!(
            rust.rust_package_name.unwrap().crate_name(),
            "orders_mapping"
        );

        let error = args(None, [None, Some("Acme.class"), None], false)
            .settings(false)
            .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<codegen::IdentityError>(),
            Some(codegen::IdentityError {
                kind: codegen::IdentityKind::CSharpNamespace,
                ..
            })
        ));
        let error = args(Some("std"), [None; 3], false)
            .settings(true)
            .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<codegen::IdentityError>(),
            Some(codegen::IdentityError {
                kind: codegen::IdentityKind::RustPackageName,
                ..
            })
        ));
        assert_eq!(
            args(Some("orders"), [None; 3], false)
                .settings(false)
                .unwrap_err()
                .to_string(),
            "--rust-package-name applies only to --language rust"
        );
        assert_eq!(
            args(None, [None, Some("Acme"), None], false)
                .settings(true)
                .unwrap_err()
                .to_string(),
            "--csharp-namespace applies only to --language csharp"
        );
    }
}

//! Records the identity of the `codegen-runtime` sources this generator
//! targets as `FERRULE_GENERATOR_RUNTIME_IDENTITY`.

#[path = "../codegen-runtime/identity.rs"]
mod identity;

fn main() -> std::io::Result<()> {
    let manifest =
        std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "CARGO_MANIFEST_DIR is unset")
        })?);
    let runtime = manifest.join("../codegen-runtime");
    for path in ["Cargo.toml", "build.rs", "identity.rs", "src"] {
        println!("cargo:rerun-if-changed={}", runtime.join(path).display());
    }
    let identity = identity::runtime_source_identity(&runtime)?;
    println!("cargo:rustc-env=FERRULE_GENERATOR_RUNTIME_IDENTITY={identity}");
    Ok(())
}

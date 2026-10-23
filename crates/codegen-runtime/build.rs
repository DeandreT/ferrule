//! Exports the source identity of this runtime as `FERRULE_RUNTIME_IDENTITY`.

#[path = "identity.rs"]
mod identity;

fn main() -> std::io::Result<()> {
    let root =
        std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "CARGO_MANIFEST_DIR is unset")
        })?);
    for path in ["Cargo.toml", "build.rs", "identity.rs", "src"] {
        println!("cargo:rerun-if-changed={path}");
    }
    let identity = identity::runtime_source_identity(&root)?;
    println!("cargo:rustc-env=FERRULE_RUNTIME_IDENTITY={identity}");
    Ok(())
}

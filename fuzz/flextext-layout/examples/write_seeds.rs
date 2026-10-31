//! Regenerates `seeds/` from `flextext_layout_fuzz::seeds::all`.

use std::path::Path;

fn main() -> std::io::Result<()> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("seeds");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir(&dir)?;
    for (name, bytes) in flextext_layout_fuzz::seeds::all() {
        std::fs::write(dir.join(format!("{name}.bin")), bytes)?;
    }
    Ok(())
}

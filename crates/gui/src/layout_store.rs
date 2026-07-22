use std::path::{Path, PathBuf};

use mapping::Project;

use crate::app::{CanvasLayout, LAYOUT_VERSION};

pub fn project_fingerprint(project: &Project) -> String {
    let json = serde_json::to_vec(project).expect("Project serialization cannot fail");
    let hash = json.into_iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

pub fn layout_path(project_path: &Path) -> PathBuf {
    let mut path = project_path.to_path_buf();
    path.set_extension("layout.json");
    path
}

pub fn read_layout(project_path: &Path) -> anyhow::Result<Option<CanvasLayout>> {
    let path = layout_path(project_path);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let layout: CanvasLayout = serde_json::from_str(&text)?;
    anyhow::ensure!(
        (1..=LAYOUT_VERSION).contains(&layout.version),
        "unsupported canvas layout version {}",
        layout.version
    );
    Ok(Some(layout))
}

pub fn write_layout(project_path: &Path, layout: &CanvasLayout) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(layout)?;
    std::fs::write(layout_path(project_path), json)?;
    Ok(())
}

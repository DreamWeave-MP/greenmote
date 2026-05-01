use std::path::{Path, PathBuf};

#[must_use]
pub fn normalize_mesh_for_copy(mesh: &str) -> Option<String> {
    let normalized = mesh.replace('/', "\\").to_ascii_lowercase();

    if normalized.starts_with("grass\\") {
        None
    } else {
        Some(normalized)
    }
}

#[must_use]
pub fn grass_prefixed_mesh(mesh: &str) -> String {
    let normalized = mesh.replace('/', "\\");

    if normalized.to_ascii_lowercase().starts_with("grass\\") {
        normalized
    } else {
        format!("grass\\{normalized}")
    }
}

#[must_use]
pub fn mesh_output_path(output_directory: &Path, normalized_mesh: &str) -> PathBuf {
    normalized_mesh.split('\\').fold(
        output_directory.join("Meshes").join("grass"),
        |path, part| path.join(part),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_copy_normalization_skips_existing_grass_meshes() {
        assert_eq!(normalize_mesh_for_copy("Grass\\foo.nif"), None);
        assert_eq!(normalize_mesh_for_copy("grass/foo.nif"), None);
    }

    #[test]
    fn mesh_copy_normalization_lowercases_non_grass_meshes() {
        assert_eq!(
            normalize_mesh_for_copy("Flora/Foo.NIF"),
            Some("flora\\foo.nif".to_owned())
        );
    }

    #[test]
    fn grass_prefix_is_not_added_twice() {
        assert_eq!(
            grass_prefixed_mesh("flora/foo.nif"),
            "grass\\flora\\foo.nif"
        );
        assert_eq!(grass_prefixed_mesh("Grass\\foo.nif"), "Grass\\foo.nif");
    }
}

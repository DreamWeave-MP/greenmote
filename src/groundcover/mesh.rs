use std::{
    io,
    path::{Path, PathBuf},
};

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

/// Builds the output path for a normalized mesh under `Meshes/grass`.
///
/// # Errors
///
/// Returns invalid input if the mesh path contains empty, current-directory, parent-directory,
/// or drive-prefixed components.
pub fn mesh_output_path(output_directory: &Path, normalized_mesh: &str) -> io::Result<PathBuf> {
    let mut path = output_directory.join("Meshes").join("grass");

    for part in normalized_mesh.split('\\') {
        if unsafe_mesh_component(part) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("mesh path {normalized_mesh:?} contains unsafe component {part:?}"),
            ));
        }

        path = path.join(part);
    }

    Ok(path)
}

fn unsafe_mesh_component(part: &str) -> bool {
    part.is_empty() || part == "." || part == ".." || part.contains(':')
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

    #[test]
    fn mesh_output_path_rejects_unsafe_components() {
        for mesh in [
            "..\\evil.nif",
            "flora\\..\\evil.nif",
            "flora\\.\\evil.nif",
            "flora\\\\evil.nif",
            "c:\\evil.nif",
        ] {
            assert!(mesh_output_path(Path::new("out"), mesh).is_err());
        }
    }
}

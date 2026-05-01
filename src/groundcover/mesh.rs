use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MeshCopyPath {
    pub source: String,
    pub target: String,
}

/// Normalizes a source mesh path into separate VFS lookup and output-relative paths.
///
/// # Errors
///
/// Returns invalid input if the mesh path contains empty, current-directory, parent-directory,
/// or drive-prefixed components.
pub fn normalize_mesh_for_copy(mesh: &str) -> io::Result<MeshCopyPath> {
    let normalized = mesh.replace('/', "\\").to_ascii_lowercase();
    validate_mesh_path_components(&normalized)?;
    let target = normalized
        .strip_prefix("grass\\")
        .unwrap_or(&normalized)
        .to_owned();
    validate_mesh_path_components(&target)?;

    Ok(MeshCopyPath {
        source: normalized,
        target,
    })
}

/// Normalizes a source mesh path and ensures it is rooted under `grass\`.
///
/// # Errors
///
/// Returns invalid input if the final mesh path contains empty, current-directory,
/// parent-directory, or drive-prefixed components.
pub fn grass_prefixed_mesh(mesh: &str) -> io::Result<String> {
    let normalized = mesh.replace('/', "\\");
    let prefixed = if normalized.to_ascii_lowercase().starts_with("grass\\") {
        normalized
    } else {
        format!("grass\\{normalized}")
    };

    validate_mesh_path_components(&prefixed)?;

    Ok(prefixed)
}

/// Builds the output path for a normalized mesh under `Meshes/grass`.
///
/// # Errors
///
/// Returns invalid input if the mesh path contains empty, current-directory, parent-directory,
/// or drive-prefixed components.
pub fn mesh_output_path(output_directory: &Path, normalized_mesh: &str) -> io::Result<PathBuf> {
    let mut path = output_directory.join("Meshes").join("grass");

    validate_mesh_path_components(normalized_mesh)?;

    for part in normalized_mesh.split('\\') {
        path = path.join(part);
    }

    Ok(path)
}

fn validate_mesh_path_components(mesh_path: &str) -> io::Result<()> {
    for part in mesh_path.split('\\') {
        if unsafe_mesh_component(part) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("mesh path {mesh_path:?} contains unsafe component {part:?}"),
            ));
        }
    }

    Ok(())
}

fn unsafe_mesh_component(part: &str) -> bool {
    part.is_empty() || part == "." || part == ".." || part.contains(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_copy_normalization_strips_existing_grass_prefix_from_target_only() {
        assert_eq!(
            normalize_mesh_for_copy("Grass\\Foo.nif").unwrap(),
            MeshCopyPath {
                source: "grass\\foo.nif".to_owned(),
                target: "foo.nif".to_owned(),
            }
        );
        assert_eq!(
            normalize_mesh_for_copy("grass/Sky/foo.nif").unwrap(),
            MeshCopyPath {
                source: "grass\\sky\\foo.nif".to_owned(),
                target: "sky\\foo.nif".to_owned(),
            }
        );
    }

    #[test]
    fn mesh_copy_normalization_lowercases_non_grass_meshes() {
        assert_eq!(
            normalize_mesh_for_copy("Flora/Foo.NIF").unwrap(),
            MeshCopyPath {
                source: "flora\\foo.nif".to_owned(),
                target: "flora\\foo.nif".to_owned(),
            }
        );
    }

    #[test]
    fn grass_prefix_is_not_added_twice() {
        assert_eq!(
            grass_prefixed_mesh("flora/foo.nif").unwrap(),
            "grass\\flora\\foo.nif"
        );
        assert_eq!(
            grass_prefixed_mesh("Grass\\foo.nif").unwrap(),
            "Grass\\foo.nif"
        );
    }

    #[test]
    fn existing_grass_meshes_still_reject_unsafe_components() {
        for mesh in [
            "grass\\..\\evil.nif",
            "grass\\.\\evil.nif",
            "grass\\\\evil.nif",
            "grass\\c:\\evil.nif",
        ] {
            assert!(normalize_mesh_for_copy(mesh).is_err());
            assert!(grass_prefixed_mesh(mesh).is_err());
        }
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

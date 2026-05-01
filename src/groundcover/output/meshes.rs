use std::{
    collections::BTreeSet,
    fs::{File, create_dir_all},
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use rayon::prelude::*;
use vfstool_lib::{VFS, VfsFile};

use crate::groundcover::{mesh, progress::CancellationToken};

#[derive(Debug)]
pub struct MeshCopyJob {
    pub source: VfsFile,
    pub target_path: PathBuf,
}

pub fn resolve_mesh_copy_jobs(
    vfs: &VFS,
    mesh_paths: &BTreeSet<mesh::MeshCopyPath>,
    output_directory: &Path,
) -> io::Result<Vec<MeshCopyJob>> {
    let mut missing = Vec::new();
    let jobs = mesh_paths
        .iter()
        .map(|mesh_path| {
            let backslash_key = format!("Meshes\\{}", mesh_path.source);
            let slash_key = format!("Meshes/{}", mesh_path.source.replace('\\', "/"));
            let source = vfs
                .get_file(&backslash_key)
                .or_else(|| vfs.get_file(&slash_key));

            if let Some(source) = source {
                Ok(Some(MeshCopyJob {
                    source: source.clone(),
                    target_path: mesh::mesh_output_path(output_directory, &mesh_path.target)?,
                }))
            } else {
                missing.push(backslash_key);
                Ok(None)
            }
        })
        .collect::<io::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();

    if missing.is_empty() {
        Ok(jobs)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "missing required meshes in OpenMW VFS:\n{}",
                missing.join("\n")
            ),
        ))
    }
}

pub fn copy_meshes(
    jobs: &[MeshCopyJob],
    progress: &(dyn Fn(usize, usize) + Sync),
    cancellation: &CancellationToken,
) -> io::Result<()> {
    let total = jobs.len();
    let completed = AtomicUsize::new(0);

    jobs.par_iter().try_for_each(|job| {
        if cancellation.is_cancelled() {
            return Err(cancelled_error());
        }

        if let Some(parent) = job.target_path.parent() {
            create_dir_all(parent)?;
        }
        let mut source = job.source.open()?;
        let mut target = File::create(&job.target_path)?;
        io::copy(&mut source, &mut target)?;
        let current = completed.fetch_add(1, Ordering::Relaxed) + 1;
        progress(current, total);
        Ok(())
    })
}

fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "conversion cancelled")
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "greenmote-output-test-{name}-{}-{}",
                std::process::id(),
                NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn mesh_jobs_use_source_for_lookup_and_target_for_output() {
        let data_dir = TempDir::new("mesh-source-target-data");
        let output_dir = TempDir::new("mesh-source-target-output");
        let mesh_dir = data_dir.path().join("Meshes").join("grass");
        std::fs::create_dir_all(&mesh_dir).unwrap();
        std::fs::write(mesh_dir.join("sky_flora.nif"), b"mesh bytes").unwrap();
        let vfs = VFS::from_directories([data_dir.path()], None);
        let mesh_paths = BTreeSet::from([mesh::MeshCopyPath {
            source: "grass\\sky_flora.nif".to_owned(),
            target: "sky_flora.nif".to_owned(),
        }]);

        let jobs = resolve_mesh_copy_jobs(&vfs, &mesh_paths, output_dir.path()).unwrap();

        assert_eq!(jobs.len(), 1);
        assert_eq!(
            jobs[0].target_path,
            output_dir
                .path()
                .join("Meshes")
                .join("grass")
                .join("sky_flora.nif")
        );
    }

    #[test]
    fn missing_mesh_jobs_are_fatal() {
        let data_dir = TempDir::new("missing-mesh-data");
        let output_dir = TempDir::new("missing-mesh-output");
        let vfs = VFS::from_directories([data_dir.path()], None);
        let mesh_paths = BTreeSet::from([mesh::MeshCopyPath {
            source: "flora\\missing.nif".to_owned(),
            target: "flora\\missing.nif".to_owned(),
        }]);

        let error = resolve_mesh_copy_jobs(&vfs, &mesh_paths, output_dir.path()).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("Meshes\\flora\\missing.nif"));
    }
}

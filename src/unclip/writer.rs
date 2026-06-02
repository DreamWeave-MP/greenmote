// SPDX-License-Identifier: GPL-3.0-only

use std::{fs, io, path::Path, path::PathBuf};

use tes3::esp::Plugin;

use super::write_plan::{WritePlan, WriteReport};

pub(crate) fn save_plugin_with_backup(
    plugin: &mut Plugin,
    source_path: &Path,
    destination_path: &Path,
    plan: WritePlan,
) -> io::Result<WriteReport> {
    if let Some(parent) = destination_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let backup = prepare_plugin_backup(source_path, destination_path)?;

    let temp_path = next_temp_plugin_path(destination_path);
    if let Err(error) = plugin.save_path(&temp_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(io::Error::new(
            error.kind(),
            format!(
                "failed to write temporary plugin {}: {error}",
                temp_path.display()
            ),
        ));
    }

    let had_destination = path_entry_exists(destination_path);
    if let Err(error) = replace_with_temp(
        &temp_path,
        destination_path,
        backup.path().filter(|_| had_destination),
    ) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    Ok(WriteReport::written(destination_path, backup.path(), plan))
}

fn replace_with_temp(
    temp_path: &Path,
    destination_path: &Path,
    backup_path: Option<&Path>,
) -> io::Result<()> {
    if path_entry_exists(destination_path) {
        fs::remove_file(destination_path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "failed to remove {} before replacement: {error}",
                    destination_path.display()
                ),
            )
        })?;
    }

    if let Err(error) = rename_with_context(temp_path, destination_path) {
        if let Some(backup_path) = backup_path {
            restore_backup_to_destination(backup_path, destination_path).map_err(|restore_error| {
                io::Error::new(
                    error.kind(),
                    format!(
                        "{error}; additionally failed to restore backup {} to {}: {restore_error}",
                        backup_path.display(),
                        destination_path.display()
                    ),
                )
            })?;
        }
        return Err(error);
    }

    Ok(())
}

fn restore_backup_to_destination(backup_path: &Path, destination_path: &Path) -> io::Result<()> {
    let restore_temp_path = next_restore_temp_plugin_path(destination_path);
    if let Err(error) = fs::copy(backup_path, &restore_temp_path) {
        let _ = fs::remove_file(&restore_temp_path);
        return Err(io::Error::new(
            error.kind(),
            format!(
                "failed to copy backup {} to restore temp {}: {error}",
                backup_path.display(),
                restore_temp_path.display()
            ),
        ));
    }
    if let Err(error) = rename_with_context(&restore_temp_path, destination_path) {
        let _ = fs::remove_file(&restore_temp_path);
        return Err(error);
    }
    Ok(())
}

enum BackupAction {
    None,
    Copied { path: PathBuf },
}

impl BackupAction {
    fn path(&self) -> Option<&Path> {
        match self {
            Self::None => None,
            Self::Copied { path } => Some(path),
        }
    }
}

fn prepare_plugin_backup(source_path: &Path, destination_path: &Path) -> io::Result<BackupAction> {
    let backup_path = next_numbered_backup_path(destination_path);
    if path_entry_exists(destination_path) {
        copy_backup(destination_path, &backup_path)?;
        return Ok(BackupAction::Copied { path: backup_path });
    }

    if !same_path(source_path, destination_path) && path_entry_exists(source_path) {
        copy_backup(source_path, &backup_path)?;
        return Ok(BackupAction::Copied { path: backup_path });
    }

    Ok(BackupAction::None)
}

fn copy_backup(source: &Path, backup_path: &Path) -> io::Result<()> {
    fs::copy(source, backup_path).map(|_| ()).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to copy backup {} to {}: {error}",
                source.display(),
                backup_path.display()
            ),
        )
    })
}

fn rename_with_context(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to move {} to {}: {error}",
                source.display(),
                destination.display()
            ),
        )
    })
}

fn next_temp_plugin_path(plugin_path: &Path) -> PathBuf {
    for index in 0.. {
        let candidate = append_path_suffix(plugin_path, &format!(".greenmote-tmp.{index}"));
        if !path_entry_exists(&candidate) {
            return candidate;
        }
    }

    unreachable!("unbounded temp suffix search should always find a candidate")
}

fn next_restore_temp_plugin_path(plugin_path: &Path) -> PathBuf {
    for index in 0.. {
        let candidate = append_path_suffix(plugin_path, &format!(".greenmote-restore-tmp.{index}"));
        if !path_entry_exists(&candidate) {
            return candidate;
        }
    }

    unreachable!("unbounded restore temp suffix search should always find a candidate")
}

fn next_numbered_backup_path(plugin_path: &Path) -> PathBuf {
    for index in 1.. {
        let candidate = append_path_suffix(plugin_path, &format!(".{index:03}"));
        if !path_entry_exists(&candidate) {
            return candidate;
        }
    }

    unreachable!("unbounded backup suffix search should always find a candidate")
}

fn append_path_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut file_name = path
        .file_name()
        .expect("plugin path should have filename")
        .to_os_string();
    file_name.push(suffix);
    path.with_file_name(file_name)
}

fn path_entry_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left == right
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{next_numbered_backup_path, prepare_plugin_backup, replace_with_temp};

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "greenmote-unclip-writer-test-{name}-{}-{}",
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
    fn numbered_plugin_backups_append_suffix_to_full_filename() {
        let temp = TempDir::new("numbered-backup");
        let plugin = temp.path().join("plugin.omwaddon");
        std::fs::write(&plugin, b"current").unwrap();
        std::fs::write(temp.path().join("plugin.omwaddon.001"), b"old").unwrap();

        assert_eq!(
            next_numbered_backup_path(&plugin),
            temp.path().join("plugin.omwaddon.002")
        );
    }

    #[test]
    fn vfs_destination_without_existing_file_gets_source_backup_copy() {
        let temp = TempDir::new("copy-source-backup");
        let source = temp.path().join("source").join("plugin.omwaddon");
        let destination = temp.path().join("data-local").join("plugin.omwaddon");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(&source, b"original").unwrap();

        let backup = prepare_plugin_backup(&source, &destination).unwrap();
        let backup_path = backup.path().unwrap();

        assert_eq!(
            backup_path,
            temp.path().join("data-local/plugin.omwaddon.001")
        );
        assert_eq!(std::fs::read(backup_path).unwrap(), b"original");
        assert!(!destination.exists());
    }

    #[test]
    fn existing_destination_is_copied_to_backup() {
        let temp = TempDir::new("copy-destination-backup");
        let plugin = temp.path().join("plugin.omwaddon");
        std::fs::write(&plugin, b"modified").unwrap();

        let backup = prepare_plugin_backup(&plugin, &plugin).unwrap();
        let backup_path = backup.path().unwrap();

        assert_eq!(backup_path, temp.path().join("plugin.omwaddon.001"));
        assert_eq!(std::fs::read(backup_path).unwrap(), b"modified");
        assert_eq!(std::fs::read(plugin).unwrap(), b"modified");
    }

    #[test]
    fn replacement_overwrites_existing_destination_after_backup() {
        let temp = TempDir::new("replace-existing");
        let plugin = temp.path().join("plugin.omwaddon");
        let temp_plugin = temp.path().join("plugin.omwaddon.greenmote-tmp.0");
        let backup = temp.path().join("plugin.omwaddon.001");
        std::fs::write(&plugin, b"old").unwrap();
        std::fs::write(&backup, b"old").unwrap();
        std::fs::write(&temp_plugin, b"new").unwrap();

        replace_with_temp(&temp_plugin, &plugin, Some(&backup)).unwrap();

        assert_eq!(std::fs::read(&plugin).unwrap(), b"new");
        assert_eq!(std::fs::read(&backup).unwrap(), b"old");
        assert!(!temp_plugin.exists());
    }

    #[test]
    fn failed_replacement_without_prior_destination_does_not_restore_backup() {
        let temp = TempDir::new("replace-missing-destination-fails");
        let plugin = temp.path().join("missing").join("plugin.omwaddon");
        let temp_plugin = temp.path().join("plugin.omwaddon.greenmote-tmp.0");
        let backup = temp.path().join("plugin.omwaddon.001");
        std::fs::write(&temp_plugin, b"new").unwrap();
        std::fs::write(&backup, b"original").unwrap();

        let result = replace_with_temp(&temp_plugin, &plugin, None);

        assert!(result.is_err());
        assert!(!plugin.exists());
        assert_eq!(std::fs::read(&backup).unwrap(), b"original");
    }
}

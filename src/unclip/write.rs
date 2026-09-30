// SPDX-License-Identifier: MIT OR Apache-2.0

//! Turning verdicts into a rewritten plugin on disk.
//!
//! The changes are applied to the loaded source plugin, which is then written back over the
//! source (keeping backups) or, with `--output-plugin`, to another path as a complete copy. Own
//! references that are deleted are removed outright; references the source inherited from a
//! master are marked deleted so the master's placement stays hidden.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::Serialize;
use tes3::esp::{Cell, Plugin};

use super::decide::{RefVerdict, Verdict};

/// Copy of the source made before the first in-place write. Never overwritten afterwards.
pub(crate) const ORIGINAL_BACKUP_SUFFIX: &str = ".greenmote-original";
/// Copy of the previous file, refreshed on every write.
pub(crate) const PREVIOUS_BACKUP_SUFFIX: &str = ".bak";

/// Marker wrapped around any error raised while writing or verifying the rewritten plugin.
///
/// Errors before this point (loading, measuring, deciding) leave the plugin untouched, so a
/// batch can safely move on to its next target. A failure during the write itself may have
/// left backups or a partial output behind, which is why callers stop the batch on it.
#[derive(Debug)]
pub(crate) struct WriteFailure(io::Error);

impl WriteFailure {
    pub(crate) fn wrap(error: io::Error) -> io::Error {
        let kind = error.kind();
        io::Error::new(kind, Self(error))
    }

    /// Whether `error` came out of the write phase.
    #[cfg(feature = "gui")]
    pub(crate) fn is_write_failure(error: &io::Error) -> bool {
        match error.get_ref() {
            Some(inner) => inner.is::<Self>(),
            None => false,
        }
    }
}

impl std::fmt::Display for WriteFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for WriteFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// Where the rewritten plugin goes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OutputMode {
    /// Replace the source plugin.
    InPlace,
    /// Write the complete rewritten plugin to another path; the source is untouched.
    Copy { path: PathBuf },
}

/// What a write produced.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct WriteOutcome {
    pub(crate) path: PathBuf,
    pub(crate) replaced_source: bool,
    pub(crate) backups: Vec<PathBuf>,
    pub(crate) refs_fixed: usize,
    pub(crate) refs_deleted: usize,
    pub(crate) cells: usize,
    pub(crate) verified: bool,
}

/// Applies verdicts to the loaded plugin. Returns how many references changed.
pub(crate) fn apply_changes(source: &mut Plugin, verdicts: &[RefVerdict]) -> usize {
    let mut applied = 0;
    for cell in source.objects_of_type_mut::<Cell>() {
        if !cell.is_exterior() {
            continue;
        }
        let grid = cell.data.grid;
        for entry in verdicts
            .iter()
            .filter(|entry| entry.cell == [grid.0, grid.1])
        {
            let key = (entry.key[0], entry.key[1]);
            match &entry.verdict {
                Verdict::Fix(fix) => {
                    if let Some(reference) = cell.references.get_mut(&key) {
                        reference.translation = fix.translation;
                        reference.rotation = fix.rotation;
                        applied += 1;
                    }
                }
                Verdict::Delete { .. } => {
                    if key.0 == 0 {
                        if cell.references.remove(&key).is_some() {
                            applied += 1;
                        }
                    } else if let Some(reference) = cell.references.get_mut(&key) {
                        reference.deleted = Some(true);
                        applied += 1;
                    }
                }
                Verdict::Keep { .. } | Verdict::Skip { .. } => {}
            }
        }
    }
    applied
}

/// Writes a plugin atomically, keeping backups of whatever it replaces.
///
/// With `keep_original`, the first write also keeps a pristine copy that later writes never
/// touch.
///
/// # Errors
///
/// Returns filesystem errors. A failed replacement leaves the previous file in place.
pub(crate) fn write_plugin(
    plugin: &mut Plugin,
    destination: &Path,
    keep_original: bool,
) -> io::Result<Vec<PathBuf>> {
    if let Some(parent) = destination.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let mut backups = Vec::new();
    if fs::symlink_metadata(destination).is_ok() {
        if keep_original {
            let original = with_suffix(destination, ORIGINAL_BACKUP_SUFFIX);
            if fs::symlink_metadata(&original).is_err() {
                copy_with_context(destination, &original)?;
                backups.push(original);
            }
        }
        let previous = with_suffix(destination, PREVIOUS_BACKUP_SUFFIX);
        copy_with_context(destination, &previous)?;
        backups.push(previous);
    }

    let temp = with_suffix(destination, ".greenmote-tmp");
    if let Err(error) = plugin.save_path(&temp) {
        let _ = fs::remove_file(&temp);
        return Err(io::Error::new(
            error.kind(),
            format!("failed to write {}: {error}", temp.display()),
        ));
    }
    if let Err(error) = fs::rename(&temp, destination) {
        let _ = fs::remove_file(&temp);
        return Err(io::Error::new(
            error.kind(),
            format!(
                "failed to move {} to {}: {error}",
                temp.display(),
                destination.display()
            ),
        ));
    }
    Ok(backups)
}

/// Reloads a written plugin and checks that every change landed.
///
/// # Errors
///
/// Returns an error describing the first mismatch.
pub(crate) fn verify_written(destination: &Path, verdicts: &[RefVerdict]) -> io::Result<()> {
    let written = Plugin::from_path(destination).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "written plugin {} does not load back: {error}",
                destination.display()
            ),
        )
    })?;
    let mismatch = |entry: &RefVerdict, what: &str| {
        io::Error::other(format!(
            "verification failed for ref {}:{} in cell {:?} of {}: {what}",
            entry.key[0],
            entry.key[1],
            entry.cell,
            destination.display()
        ))
    };
    for entry in verdicts
        .iter()
        .filter(|entry| entry.verdict.changes_plugin())
    {
        let cell = written
            .objects_of_type::<Cell>()
            .find(|cell| cell.is_exterior() && cell.data.grid == (entry.cell[0], entry.cell[1]))
            .ok_or_else(|| mismatch(entry, "cell missing"))?;
        let reference = cell.references.get(&(entry.key[0], entry.key[1]));
        match (&entry.verdict, reference) {
            (Verdict::Fix(fix), Some(reference)) => {
                if reference.deleted.is_some()
                    || !close(reference.translation, fix.translation)
                    || !close(reference.rotation, fix.rotation)
                {
                    return Err(mismatch(entry, "position not applied"));
                }
            }
            (Verdict::Delete { .. }, Some(reference)) => {
                if reference.deleted != Some(true) {
                    return Err(mismatch(entry, "not deleted"));
                }
            }
            (Verdict::Delete { .. }, None) if entry.key[0] == 0 => {}
            (Verdict::Delete { .. } | Verdict::Fix(_), None) => {
                return Err(mismatch(entry, "reference missing"));
            }
            (Verdict::Keep { .. } | Verdict::Skip { .. }, _) => {}
        }
    }
    Ok(())
}

fn close(left: [f32; 3], right: [f32; 3]) -> bool {
    left.iter()
        .zip(right)
        .all(|(left, right)| (left - right).abs() <= 1e-4)
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

fn copy_with_context(from: &Path, to: &Path) -> io::Result<()> {
    fs::copy(from, to).map(|_| ()).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to back up {} to {}: {error}",
                from.display(),
                to.display()
            ),
        )
    })
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use std::path::PathBuf;

    use tes3::esp::{Cell, CellData, Header, Plugin, Reference, TES3Object};

    use crate::unclip::decide::{DeleteReason, Fix, RefVerdict, Verdict};

    use super::{apply_changes, verify_written, write_plugin};

    fn reference(mast: u32, refr: u32, z: f32) -> ((u32, u32), Reference) {
        (
            (mast, refr),
            Reference {
                mast_index: mast,
                refr_index: refr,
                id: "flora_grass_01".to_owned(),
                temporary: true,
                translation: [100.0, 200.0, z],
                rotation: [0.0, 0.0, 1.0],
                scale: Some(1.5),
                ..Reference::default()
            },
        )
    }

    fn source_plugin() -> Plugin {
        let mut cell = Cell {
            name: String::new(),
            data: CellData {
                grid: (0, 0),
                ..CellData::default()
            },
            ..Cell::default()
        };
        cell.references.extend([
            reference(0, 1, 10.0),
            reference(0, 2, 20.0),
            reference(1, 5, 30.0),
        ]);
        Plugin {
            objects: vec![
                TES3Object::Header(Header {
                    masters: vec![("Morrowind.esm".to_owned(), 1234)],
                    ..Header::default()
                }),
                TES3Object::Cell(cell),
            ],
        }
    }

    fn fix(key: (u32, u32), z: f32) -> RefVerdict {
        RefVerdict {
            cell: [0, 0],
            key: [key.0, key.1],
            id: "flora_grass_01".to_owned(),
            verdict: Verdict::Fix(Fix {
                translation: [100.0, 200.0, z],
                rotation: [0.1, 0.2, 1.0],
                grounded: true,
                oriented: true,
                moved: false,
                gap_before: 5.0,
                gap_after: -2.0,
                moved_from: None,
                occluder: None,
            }),
            measured: None,
        }
    }

    fn delete(key: (u32, u32)) -> RefVerdict {
        RefVerdict {
            cell: [0, 0],
            key: [key.0, key.1],
            id: "flora_grass_01".to_owned(),
            verdict: Verdict::Delete {
                reason: DeleteReason::Water {
                    original_z: 20.0,
                    terrain_z: -5.0,
                },
            },
            measured: None,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "greenmote-write-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn apply_removes_own_refs_and_marks_inherited_refs_deleted() {
        let mut source = source_plugin();
        let applied = apply_changes(
            &mut source,
            &[fix((0, 1), 8.0), delete((0, 2)), delete((1, 5))],
        );
        assert_eq!(applied, 3);
        let cell = source.objects_of_type::<Cell>().next().unwrap();
        assert_eq!(cell.references[&(0, 1)].translation[2], 8.0);
        assert_eq!(cell.references[&(0, 1)].scale, Some(1.5));
        assert!(!cell.references.contains_key(&(0, 2)));
        assert_eq!(cell.references[&(1, 5)].deleted, Some(true));
    }

    #[test]
    fn write_verify_and_backups_round_trip() {
        let dir = temp_dir("roundtrip");
        let source_path = dir.join("Source.esp");
        source_plugin().save_path(&source_path).unwrap();
        let verdicts = [fix((0, 1), 8.0), delete((1, 5))];

        // A copy elsewhere leaves the source alone and keeps no original.
        let mut copy = Plugin::from_path(&source_path).unwrap();
        apply_changes(&mut copy, &verdicts);
        let copy_path = dir.join("out/Source_unclipped.esp");
        assert!(
            write_plugin(&mut copy, &copy_path, false)
                .unwrap()
                .is_empty()
        );
        verify_written(&copy_path, &verdicts).unwrap();
        assert_eq!(
            write_plugin(&mut copy, &copy_path, false).unwrap(),
            vec![dir.join("out/Source_unclipped.esp.bak")]
        );
        assert!(verify_written(&source_path, &verdicts).is_err());

        // In place keeps the pristine original once and a .bak every time.
        let mut source = Plugin::from_path(&source_path).unwrap();
        apply_changes(&mut source, &verdicts);
        assert_eq!(
            write_plugin(&mut source, &source_path, true).unwrap(),
            vec![
                dir.join("Source.esp.greenmote-original"),
                dir.join("Source.esp.bak")
            ]
        );
        verify_written(&source_path, &verdicts).unwrap();
        assert_eq!(
            write_plugin(&mut source, &source_path, true).unwrap(),
            vec![dir.join("Source.esp.bak")]
        );
        assert!(verify_written(&source_path, &[fix((0, 1), 999.0)]).is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }
}

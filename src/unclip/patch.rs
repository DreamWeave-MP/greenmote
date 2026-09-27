// SPDX-License-Identifier: GPL-3.0-only

//! Turning verdicts into a plugin on disk.
//!
//! The default output is a patch plugin that lists the source groundcover plugin as a master and
//! contains only the changed references: moved refs as overrides and removed refs as `DELE`
//! records. `OpenMW`'s groundcover loader keys references by their resolved `RefNum`, so a patch
//! listed after its source in `groundcover=` replaces exactly those refs and nothing else.
//!
//! `--in-place` edits the source plugin instead. Own references are removed outright; references
//! the source inherited from a master are marked deleted so the master's placement stays hidden.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::Serialize;
use tes3::esp::{Cell, FileType, FixedString, Header, Plugin, Reference, TES3Object};

use super::decide::{RefVerdict, Verdict};

pub(crate) const PATCH_AUTHOR: &str = "greenmote";
pub(crate) const PATCH_SUFFIX: &str = "_unclip.omwaddon";
/// Copy of the source made before the first in-place write. Never overwritten afterwards.
pub(crate) const ORIGINAL_BACKUP_SUFFIX: &str = ".greenmote-original";
/// Copy of the previous file, refreshed on every write.
pub(crate) const PREVIOUS_BACKUP_SUFFIX: &str = ".bak";

/// Where the changes go.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OutputMode {
    /// Write a patch plugin next to nothing else; the source is untouched.
    Patch { path: PathBuf },
    /// Rewrite the source plugin.
    InPlace,
}

/// What a write produced.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct WriteOutcome {
    pub(crate) path: PathBuf,
    pub(crate) in_place: bool,
    pub(crate) backups: Vec<PathBuf>,
    pub(crate) refs_fixed: usize,
    pub(crate) refs_deleted: usize,
    pub(crate) cells: usize,
    pub(crate) masters: Vec<String>,
    pub(crate) verified: bool,
}

/// Default patch path for a source plugin: `<directory>/<stem>_unclip.omwaddon`.
#[must_use]
pub(crate) fn default_patch_path(directory: &Path, source_path: &Path) -> PathBuf {
    let stem = source_path.file_stem().map_or_else(
        || "groundcover".to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    directory.join(format!("{stem}{PATCH_SUFFIX}"))
}

/// Builds the patch plugin for a source and its verdicts.
///
/// # Errors
///
/// Returns an error when a changed reference points at a master index the source does not declare.
pub(crate) fn build_patch(
    source: &Plugin,
    source_file_name: &str,
    source_size: u64,
    verdicts: &[RefVerdict],
) -> io::Result<Plugin> {
    let source_masters = source
        .header()
        .map(|header| header.masters.clone())
        .unwrap_or_default();
    let mut masters = MasterTable::new(source_file_name, source_size, &source_masters);
    let mut cells: Vec<Cell> = Vec::new();

    for source_cell in source.objects_of_type::<Cell>() {
        if !source_cell.is_exterior() {
            continue;
        }
        let grid = source_cell.data.grid;
        let mut patch_cell: Option<Cell> = None;
        for entry in verdicts
            .iter()
            .filter(|entry| entry.cell == [grid.0, grid.1] && entry.verdict.changes_plugin())
        {
            let key = (entry.key[0], entry.key[1]);
            let Some(reference) = source_cell.references.get(&key) else {
                continue;
            };
            let mast_index = masters.remap(key.0)?;
            let patched = match &entry.verdict {
                Verdict::Fix(fix) => Reference {
                    mast_index,
                    refr_index: key.1,
                    id: reference.id.clone(),
                    temporary: reference.temporary,
                    translation: fix.translation,
                    rotation: fix.rotation,
                    scale: reference.scale,
                    ..Reference::default()
                },
                Verdict::Delete { .. } => Reference {
                    mast_index,
                    refr_index: key.1,
                    id: reference.id.clone(),
                    temporary: reference.temporary,
                    deleted: Some(true),
                    ..Reference::default()
                },
                Verdict::Keep { .. } | Verdict::Skip { .. } => continue,
            };
            patch_cell
                .get_or_insert_with(|| Cell {
                    name: source_cell.name.clone(),
                    data: source_cell.data.clone(),
                    region: source_cell.region.clone(),
                    ..Cell::default()
                })
                .references
                .insert((mast_index, key.1), patched);
        }
        if let Some(cell) = patch_cell {
            cells.push(cell);
        }
    }

    let header = Header {
        version: source.header().map_or(1.3, |header| header.version),
        file_type: FileType::Esp,
        author: FixedString(PATCH_AUTHOR.to_owned()),
        description: FixedString(format!(
            "Unclip patch for {source_file_name}.\nGenerated by greenmote unclip. Load after the source groundcover plugin."
        )),
        masters: masters.into_list(),
        ..Header::default()
    };
    let mut objects = Vec::with_capacity(cells.len() + 1);
    objects.push(TES3Object::Header(header));
    objects.extend(cells.into_iter().map(TES3Object::Cell));
    let mut patch = Plugin { objects };
    finalize_own_refs(&mut patch);
    Ok(patch)
}

/// Applies verdicts to the source plugin itself.
pub(crate) fn apply_in_place(source: &mut Plugin, verdicts: &[RefVerdict]) -> usize {
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
pub(crate) fn verify_written(
    destination: &Path,
    verdicts: &[RefVerdict],
    in_place: bool,
) -> io::Result<()> {
    let written = Plugin::from_path(destination).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "written plugin {} does not load back: {error}",
                destination.display()
            ),
        )
    })?;
    let masters = written
        .header()
        .map(|header| header.masters.clone())
        .unwrap_or_default();
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
        let reference = cell.references.iter().find(|((mast, refr), _)| {
            *refr == entry.key[1]
                && if in_place {
                    *mast == entry.key[0]
                } else {
                    true
                }
        });
        match (&entry.verdict, reference) {
            (Verdict::Fix(fix), Some((_, reference))) => {
                if reference.deleted.is_some()
                    || !close(reference.translation, fix.translation)
                    || !close(reference.rotation, fix.rotation)
                {
                    return Err(mismatch(entry, "position not applied"));
                }
            }
            (Verdict::Delete { .. }, Some((_, reference))) => {
                if reference.deleted != Some(true) {
                    return Err(mismatch(entry, "not deleted"));
                }
            }
            (Verdict::Delete { .. }, None) if in_place && entry.key[0] == 0 => {}
            (Verdict::Delete { .. } | Verdict::Fix(_), None) => {
                return Err(mismatch(entry, "reference missing"));
            }
            (Verdict::Keep { .. } | Verdict::Skip { .. }, _) => {}
        }
    }
    if !in_place && masters.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("patch {} has no masters", destination.display()),
        ));
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

/// Master list of the patch: the source's own masters that changed refs need, then the source.
struct MasterTable<'a> {
    source_file_name: &'a str,
    source_size: u64,
    source_masters: &'a [(String, u64)],
    /// Patch master index (1-based) for each source master index (1-based), once needed.
    used: Vec<Option<u32>>,
    list: Vec<(String, u64)>,
}

impl<'a> MasterTable<'a> {
    fn new(
        source_file_name: &'a str,
        source_size: u64,
        source_masters: &'a [(String, u64)],
    ) -> Self {
        Self {
            source_file_name,
            source_size,
            source_masters,
            used: vec![None; source_masters.len()],
            list: Vec::new(),
        }
    }

    fn remap(&mut self, source_mast_index: u32) -> io::Result<u32> {
        if source_mast_index == 0 {
            return Ok(0);
        }
        let slot = usize::try_from(source_mast_index - 1)
            .ok()
            .filter(|slot| *slot < self.used.len());
        let Some(slot) = slot else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "reference uses master index {source_mast_index} but {} declares only {} masters",
                    self.source_file_name,
                    self.used.len()
                ),
            ));
        };
        if let Some(index) = self.used[slot] {
            return Ok(index);
        }
        self.list.push(self.source_masters[slot].clone());
        let index = u32::try_from(self.list.len()).expect("master count fits u32");
        self.used[slot] = Some(index);
        Ok(index)
    }

    /// Finalizes the list with the source as the last master and rewrites index 0 to point at it.
    fn into_list(mut self) -> Vec<(String, u64)> {
        self.list
            .push((self.source_file_name.to_owned(), self.source_size));
        self.list
    }
}

/// Rewrites the patch so refs the source owned point at the source master entry.
///
/// `MasterTable::remap` returns 0 for own refs while the master list is still growing; once the
/// source has been appended as the last master those keys are rewritten to point at it.
fn finalize_own_refs(patch: &mut Plugin) {
    let source_index = patch.header().map_or(0, |header| {
        u32::try_from(header.masters.len()).expect("master count fits u32")
    });
    for cell in patch.objects_of_type_mut::<Cell>() {
        let own = cell
            .references
            .keys()
            .filter(|(mast, _)| *mast == 0)
            .copied()
            .collect::<Vec<_>>();
        for key in own {
            if let Some(mut reference) = cell.references.remove(&key) {
                reference.mast_index = source_index;
                cell.references.insert((source_index, key.1), reference);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use std::path::PathBuf;

    use tes3::esp::{Cell, CellData, Header, Plugin, Reference, TES3Object};

    use crate::unclip::decide::{DeleteReason, Fix, RefVerdict, Verdict};

    use super::{apply_in_place, build_patch, default_patch_path, verify_written, write_plugin};

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
                reason: DeleteReason::Water { terrain_z: -5.0 },
            },
            measured: None,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "greenmote-patch-{name}-{}-{}",
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
    fn patch_contains_only_changed_refs_with_remapped_masters() {
        let source = source_plugin();
        let verdicts = [fix((0, 1), 8.0), delete((1, 5))];
        let patch = build_patch(&source, "Rem_AI.esp", 999, &verdicts).unwrap();

        let header = patch.header().unwrap();
        assert_eq!(
            header.masters,
            vec![
                ("Morrowind.esm".to_owned(), 1234),
                ("Rem_AI.esp".to_owned(), 999)
            ]
        );
        let cells = patch.objects_of_type::<Cell>().collect::<Vec<_>>();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].references.len(), 2);
        let moved = &cells[0].references[&(2, 1)];
        assert_eq!(moved.translation, [100.0, 200.0, 8.0]);
        assert_eq!(moved.rotation, [0.1, 0.2, 1.0]);
        assert_eq!(moved.scale, Some(1.5));
        assert!(moved.temporary);
        let deleted = &cells[0].references[&(1, 5)];
        assert_eq!(deleted.deleted, Some(true));
    }

    #[test]
    fn patch_skips_unreferenced_source_masters() {
        let source = source_plugin();
        let patch = build_patch(&source, "Rem_AI.esp", 999, &[fix((0, 2), 1.0)]).unwrap();
        assert_eq!(
            patch.header().unwrap().masters,
            vec![("Rem_AI.esp".to_owned(), 999)]
        );
        let cell = patch.objects_of_type::<Cell>().next().unwrap();
        assert!(cell.references.contains_key(&(1, 2)));
    }

    #[test]
    fn patch_rejects_out_of_range_master_index() {
        let mut source = source_plugin();
        source
            .objects_of_type_mut::<Cell>()
            .next()
            .unwrap()
            .references
            .extend([reference(7, 1, 0.0)]);
        assert!(build_patch(&source, "x.esp", 1, &[fix((7, 1), 0.0)]).is_err());
    }

    #[test]
    fn in_place_removes_own_refs_and_marks_inherited_refs_deleted() {
        let mut source = source_plugin();
        let applied = apply_in_place(
            &mut source,
            &[fix((0, 1), 8.0), delete((0, 2)), delete((1, 5))],
        );
        assert_eq!(applied, 3);
        let cell = source.objects_of_type::<Cell>().next().unwrap();
        assert_eq!(cell.references[&(0, 1)].translation[2], 8.0);
        assert!(!cell.references.contains_key(&(0, 2)));
        assert_eq!(cell.references[&(1, 5)].deleted, Some(true));
    }

    #[test]
    fn write_verify_and_backups_round_trip() {
        let dir = temp_dir("roundtrip");
        let source_path = dir.join("Source.esp");
        source_plugin().save_path(&source_path).unwrap();

        let verdicts = [fix((0, 1), 8.0), delete((1, 5))];
        let source = Plugin::from_path(&source_path).unwrap();
        let mut patch = build_patch(&source, "Source.esp", 1, &verdicts).unwrap();
        let patch_path = default_patch_path(&dir, &source_path);
        assert!(patch_path.ends_with("Source_unclip.omwaddon"));
        let backups = write_plugin(&mut patch, &patch_path, false).unwrap();
        assert!(backups.is_empty());
        verify_written(&patch_path, &verdicts, false).unwrap();

        // Second write of the patch keeps a .bak of the previous patch.
        let backups = write_plugin(&mut patch, &patch_path, false).unwrap();
        assert_eq!(backups, vec![dir.join("Source_unclip.omwaddon.bak")]);

        // In-place keeps the pristine original once and a .bak every time.
        let mut source = Plugin::from_path(&source_path).unwrap();
        apply_in_place(&mut source, &verdicts);
        let backups = write_plugin(&mut source, &source_path, true).unwrap();
        assert_eq!(
            backups,
            vec![
                dir.join("Source.esp.greenmote-original"),
                dir.join("Source.esp.bak")
            ]
        );
        verify_written(&source_path, &verdicts, true).unwrap();
        let backups = write_plugin(&mut source, &source_path, true).unwrap();
        assert_eq!(backups, vec![dir.join("Source.esp.bak")]);

        // A wrong expectation is caught by verification.
        let wrong = [fix((0, 1), 999.0)];
        assert!(verify_written(&source_path, &wrong, true).is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }
}

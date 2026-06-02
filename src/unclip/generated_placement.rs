// SPDX-License-Identifier: GPL-3.0-only

use std::{collections::BTreeMap, io, path::Path};

use tes3::esp::Plugin;

use crate::groundcover::CancellationToken;

use super::{
    mesh::{StaticMesh, StaticMeshIndex, normalize_mesh_key},
    target::TargetRefIndex,
    terrain::TerrainIndex,
};

const DEFAULT_ORIGIN_TOLERANCE: f32 = 4.0;
const MIN_INFERRED_SAMPLES: usize = 16;
const MIN_HINTED_MODE_SHARE: f32 = 0.20;
const MIN_ORIGIN_MODE_SHARE: f32 = 0.35;

#[derive(Clone, Debug, Default)]
pub(crate) struct GeneratedPlacementIndex {
    placements_by_mesh: BTreeMap<String, GeneratedPlacement>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GeneratedPlacement {
    pub(crate) z_offset: f32,
    pub(crate) tolerance: f32,
}

#[derive(Default)]
struct IniSection {
    z_offset: Option<f32>,
    meshes: Vec<String>,
}

impl GeneratedPlacementIndex {
    pub(crate) fn build(
        meshgenerator_ini: Option<&Path>,
        plugin: &Plugin,
        target_refs: &TargetRefIndex,
        terrain: &TerrainIndex,
        static_index: &StaticMeshIndex,
        cancellation: &CancellationToken,
    ) -> io::Result<Self> {
        let hints =
            meshgenerator_ini.map_or_else(|| Ok(BTreeMap::new()), read_meshgenerator_hints)?;
        let samples = OriginPlacementSamples::collect(
            plugin,
            target_refs,
            terrain,
            static_index,
            &hints,
            cancellation,
        )?;
        let placements_by_mesh = samples
            .groups
            .iter()
            .filter_map(|(mesh_key, group)| {
                inferred_placement(group).map(|placement| (mesh_key.clone(), placement))
            })
            .collect();

        Ok(Self { placements_by_mesh })
    }

    pub(crate) fn get(&self, static_mesh: &StaticMesh) -> Option<GeneratedPlacement> {
        self.placements_by_mesh.get(static_mesh.mesh_key()).copied()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.placements_by_mesh.len()
    }

    #[cfg(test)]
    fn from_meshgenerator_ini_str(contents: &str) -> io::Result<Self> {
        Ok(Self {
            placements_by_mesh: parse_meshgenerator_hints(contents)?
                .into_iter()
                .map(|(mesh, z_offset)| {
                    (
                        mesh,
                        GeneratedPlacement {
                            z_offset,
                            tolerance: DEFAULT_ORIGIN_TOLERANCE,
                        },
                    )
                })
                .collect(),
        })
    }
}

#[derive(Clone, Debug)]
struct OriginPlacementSamples {
    groups: BTreeMap<String, OriginPlacementSampleGroup>,
}

#[derive(Clone, Debug, Default)]
struct OriginPlacementSampleGroup {
    origin_residuals: Vec<f32>,
    rounded_origin_residuals: BTreeMap<i32, usize>,
    hinted: bool,
}

impl OriginPlacementSamples {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::too_many_arguments
    )]
    fn collect(
        plugin: &Plugin,
        target_refs: &TargetRefIndex,
        terrain: &TerrainIndex,
        static_index: &StaticMeshIndex,
        hints: &BTreeMap<String, f32>,
        cancellation: &CancellationToken,
    ) -> io::Result<Self> {
        let mut groups = BTreeMap::<String, OriginPlacementSampleGroup>::new();
        for (_, _, reference) in target_refs.iter_refs(plugin) {
            super::check_cancellation(cancellation)?;
            if reference.deleted == Some(true) {
                continue;
            }
            let Some(static_mesh) = static_index.get(&reference.id) else {
                continue;
            };
            let mesh_key = static_mesh.mesh_key().to_owned();
            let group = groups.entry(mesh_key.clone()).or_default();
            group.hinted |= hints.contains_key(&mesh_key);

            let Some(origin_terrain_z) =
                terrain.height_at(reference.translation[0], reference.translation[1])
            else {
                continue;
            };
            let origin_residual = reference.translation[2] - origin_terrain_z;
            group.origin_residuals.push(origin_residual);
            *group
                .rounded_origin_residuals
                .entry(origin_residual.round() as i32)
                .or_default() += 1;
        }

        Ok(Self { groups })
    }
}

#[allow(clippy::cast_precision_loss)]
fn inferred_placement(group: &OriginPlacementSampleGroup) -> Option<GeneratedPlacement> {
    if group.origin_residuals.len() < MIN_INFERRED_SAMPLES {
        return None;
    }
    let (z_offset, mode_count) = modal_origin_residual(group)?;
    let mode_share = mode_count as f32 / group.origin_residuals.len() as f32;
    let minimum_mode_share = if group.hinted {
        MIN_HINTED_MODE_SHARE
    } else {
        MIN_ORIGIN_MODE_SHARE
    };
    (mode_share >= minimum_mode_share).then_some(GeneratedPlacement {
        z_offset: z_offset as f32,
        tolerance: DEFAULT_ORIGIN_TOLERANCE,
    })
}

fn modal_origin_residual(group: &OriginPlacementSampleGroup) -> Option<(i32, usize)> {
    group
        .rounded_origin_residuals
        .iter()
        .max_by(|(left_offset, left_count), (right_offset, right_count)| {
            left_count
                .cmp(right_count)
                .then_with(|| right_offset.cmp(left_offset))
        })
        .map(|(&offset, &count)| (offset, count))
}

fn read_meshgenerator_hints(path: &Path) -> io::Result<BTreeMap<String, f32>> {
    parse_meshgenerator_hints(&std::fs::read_to_string(path)?)
}

fn parse_meshgenerator_hints(contents: &str) -> io::Result<BTreeMap<String, f32>> {
    let mut global_z_offset = 0.0;
    let mut current_section = String::new();
    let mut sections = BTreeMap::<String, IniSection>::new();

    for (line_index, line) in contents.lines().enumerate() {
        let line = trim_ini_line(line);
        if line.is_empty() {
            continue;
        }
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            section.trim().clone_into(&mut current_section);
            sections.entry(current_section.clone()).or_default();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key.eq_ignore_ascii_case("iZPositionModifier") {
            let z_offset = parse_f32(value, line_index + 1, key)?;
            if current_section.eq_ignore_ascii_case("global") || current_section.is_empty() {
                global_z_offset = z_offset;
            } else {
                sections
                    .entry(current_section.clone())
                    .or_default()
                    .z_offset = Some(z_offset);
            }
        } else if key.starts_with("sMesh") && !current_section.eq_ignore_ascii_case("global") {
            sections
                .entry(current_section.clone())
                .or_default()
                .meshes
                .push(value.trim_matches('"').to_owned());
        }
    }

    let mut hints = BTreeMap::new();
    for section in sections.values() {
        let z_offset = section.z_offset.unwrap_or(global_z_offset);
        for mesh in &section.meshes {
            let key = normalize_mesh_key(mesh);
            if let Some(existing) = hints.insert(key.clone(), z_offset)
                && (existing - z_offset).abs() > f32::EPSILON
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("meshgenerator INI assigns mesh {mesh} multiple Z offsets"),
                ));
            }
        }
    }

    Ok(hints)
}

fn trim_ini_line(line: &str) -> &str {
    line.split([';', '#']).next().unwrap_or(line).trim()
}

fn parse_f32(value: &str, line: usize, key: &str) -> io::Result<f32> {
    value.parse::<f32>().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid {key} on meshgenerator INI line {line}: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{GeneratedPlacementIndex, OriginPlacementSampleGroup, inferred_placement};
    use crate::unclip::mesh::StaticMesh;

    #[test]
    fn parses_global_meshgenerator_offset_for_meshes() {
        let index = GeneratedPlacementIndex::from_meshgenerator_ini_str(
            r"
            [global]
            iZPositionModifier=5

            [Grass:Region]
            sMesh0=grass\foo.nif
            sChance0=100
            ",
        )
        .unwrap();

        let placement = index
            .get(&StaticMesh::new_for_test("grass", "Meshes/Grass/Foo.nif"))
            .unwrap();

        assert_eq!(index.len(), 1);
        assert!((placement.z_offset - 5.0).abs() < f32::EPSILON);
        assert!((placement.tolerance - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn section_offset_overrides_global_offset() {
        let index = GeneratedPlacementIndex::from_meshgenerator_ini_str(
            r"
            [global]
            iZPositionModifier=5

            [Grass:Region]
            iZPositionModifier=-2
            sMesh0=grass\foo.nif
            ",
        )
        .unwrap();

        let placement = index
            .get(&StaticMesh::new_for_test("grass", "grass/foo.nif"))
            .unwrap();

        assert!((placement.z_offset + 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn meshgenerator_hints_strip_lowercase_meshes_prefix() {
        let index = GeneratedPlacementIndex::from_meshgenerator_ini_str(
            r"
            [Grass:Region]
            iZPositionModifier=7
            sMesh0=meshes/grass/foo.nif
            ",
        )
        .unwrap();

        let placement = index
            .get(&StaticMesh::new_for_test("grass", "Grass/Foo.nif"))
            .unwrap();

        assert_eq!(index.len(), 1);
        assert!((placement.z_offset - 7.0).abs() < f32::EPSILON);
    }

    #[test]
    fn meshgenerator_hints_strip_mixed_case_meshes_prefix() {
        let index = GeneratedPlacementIndex::from_meshgenerator_ini_str(
            r"
            [Grass:Region]
            iZPositionModifier=9
            sMesh0=MeShEs\Grass\Foo.nif
            ",
        )
        .unwrap();

        let placement = index
            .get(&StaticMesh::new_for_test("grass", "meshes/grass/foo.nif"))
            .unwrap();

        assert_eq!(index.len(), 1);
        assert!((placement.z_offset - 9.0).abs() < f32::EPSILON);
    }

    #[test]
    fn origin_model_infers_modal_origin_residual() {
        let mut group = OriginPlacementSampleGroup::default();
        for _ in 0..40 {
            group.origin_residuals.push(21.1);
            *group.rounded_origin_residuals.entry(21).or_default() += 1;
        }

        let placement = inferred_placement(&group).unwrap();

        assert!((placement.z_offset - 21.0).abs() < f32::EPSILON);
    }

    #[test]
    fn origin_model_rejects_weak_modal_origin_residual() {
        let mut group = OriginPlacementSampleGroup::default();
        for offset in 0_i16..40 {
            let residual = f32::from(offset);
            group.origin_residuals.push(residual);
            *group
                .rounded_origin_residuals
                .entry(i32::from(offset))
                .or_default() += 1;
        }

        assert!(inferred_placement(&group).is_none());
    }

    #[test]
    fn forced_origin_uses_modal_origin_residual() {
        let mut group = OriginPlacementSampleGroup::default();
        for (residual, rounded) in [(19.9, 20); 8].into_iter().chain([(21.1, 21); 24]) {
            group.origin_residuals.push(residual);
            *group.rounded_origin_residuals.entry(rounded).or_default() += 1;
        }

        let placement = inferred_placement(&group).unwrap();

        assert!((placement.z_offset - 21.0).abs() < f32::EPSILON);
    }
}

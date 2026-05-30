use std::{collections::BTreeMap, io, path::Path};

use tes3::esp::Plugin;

use super::{
    args::PlacementModelArg,
    mesh::{MeshCache, StaticMesh, StaticMeshIndex},
    target::TargetRefIndex,
    terrain::TerrainIndex,
};

const DEFAULT_ORIGIN_TOLERANCE: f32 = 4.0;
const MIN_INFERRED_SAMPLES: usize = 16;
const MIN_AUTO_MODE_SHARE: f32 = 0.35;
const MIN_HINTED_MODE_SHARE: f32 = 0.20;
const MAX_AUTO_ORIGIN_P05_P95_SPREAD: f32 = DEFAULT_ORIGIN_TOLERANCE * 2.0;
const MIN_CONTACT_P05_P95_SPREAD: f32 = 12.0;

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
        placement_model: PlacementModelArg,
        meshgenerator_ini: Option<&Path>,
        plugin: &Plugin,
        target_refs: &TargetRefIndex,
        terrain: &TerrainIndex,
        static_index: &StaticMeshIndex,
        mesh_contacts: &mut MeshCache<'_>,
    ) -> io::Result<Self> {
        if placement_model == PlacementModelArg::Contact {
            return Ok(Self::default());
        }

        let hints =
            meshgenerator_ini.map_or_else(|| Ok(BTreeMap::new()), read_meshgenerator_hints)?;
        let mut samples = OriginPlacementSamples::collect(
            plugin,
            target_refs,
            terrain,
            static_index,
            mesh_contacts,
            &hints,
        );
        let placements_by_mesh = samples
            .groups
            .iter_mut()
            .filter_map(|(mesh_key, group)| {
                inferred_placement(placement_model, group)
                    .map(|placement| (mesh_key.clone(), placement))
            })
            .collect();

        Ok(Self { placements_by_mesh })
    }

    pub(crate) fn get(&self, static_mesh: &StaticMesh) -> Option<GeneratedPlacement> {
        self.placements_by_mesh
            .get(&normalize_mesh_key(&static_mesh.mesh_path))
            .copied()
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
    contact_residuals: Vec<f32>,
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
        mesh_contacts: &mut MeshCache<'_>,
        hints: &BTreeMap<String, f32>,
    ) -> Self {
        let mut groups = BTreeMap::<String, OriginPlacementSampleGroup>::new();
        for (_, _, reference) in target_refs.iter_refs(plugin) {
            if reference.deleted == Some(true) {
                continue;
            }
            let Some(static_mesh) = static_index.get(&reference.id) else {
                continue;
            };
            let mesh_key = normalize_mesh_key(&static_mesh.mesh_path);
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

            if let Ok(geometry) = mesh_contacts.geometry(static_mesh) {
                let contact = geometry.contact.world_position(
                    reference.translation,
                    reference.rotation,
                    reference.scale,
                );
                if let Some(contact_terrain_z) = terrain.height_at(contact[0], contact[1]) {
                    group.contact_residuals.push(contact[2] - contact_terrain_z);
                }
            }
        }

        Self { groups }
    }
}

#[allow(clippy::cast_precision_loss)]
fn inferred_placement(
    placement_model: PlacementModelArg,
    group: &mut OriginPlacementSampleGroup,
) -> Option<GeneratedPlacement> {
    if group.origin_residuals.len() < MIN_INFERRED_SAMPLES {
        return None;
    }
    let (z_offset, mode_count) = modal_origin_residual(group)?;
    let mode_share = mode_count as f32 / group.origin_residuals.len() as f32;
    let inferred = match placement_model {
        PlacementModelArg::Contact => false,
        PlacementModelArg::Origin => true,
        PlacementModelArg::Auto => origin_model_is_better(group, mode_share),
    };
    inferred.then_some(GeneratedPlacement {
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

fn origin_model_is_better(group: &mut OriginPlacementSampleGroup, mode_share: f32) -> bool {
    let minimum_mode_share = if group.hinted {
        MIN_HINTED_MODE_SHARE
    } else {
        MIN_AUTO_MODE_SHARE
    };
    if mode_share < minimum_mode_share {
        return false;
    }
    let origin_spread = percentile_spread(&mut group.origin_residuals, 0.05, 0.95);
    if origin_spread > MAX_AUTO_ORIGIN_P05_P95_SPREAD {
        return false;
    }
    let contact_spread = percentile_spread(&mut group.contact_residuals, 0.05, 0.95);
    group.contact_residuals.len() < MIN_INFERRED_SAMPLES
        || contact_spread >= MIN_CONTACT_P05_P95_SPREAD
        || contact_spread > origin_spread * 2.0
}

fn percentile_spread(samples: &mut [f32], low: f32, high: f32) -> f32 {
    if samples.is_empty() {
        return f32::INFINITY;
    }
    samples.sort_by(f32::total_cmp);
    percentile_sorted(samples, high) - percentile_sorted(samples, low)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn percentile_sorted(samples: &[f32], percentile: f32) -> f32 {
    let max_index = samples.len() - 1;
    let index = (max_index as f32 * percentile).round() as usize;
    samples[index.min(max_index)]
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

fn normalize_mesh_key(mesh_path: &str) -> String {
    mesh_path
        .trim_start_matches("Meshes\\")
        .trim_start_matches("Meshes/")
        .replace('/', "\\")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{GeneratedPlacementIndex, OriginPlacementSampleGroup, inferred_placement};
    use crate::unclip::args::PlacementModelArg;
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
    fn auto_infers_origin_model_when_origin_residuals_are_tight() {
        let mut group = OriginPlacementSampleGroup::default();
        for _ in 0..40 {
            group.origin_residuals.push(21.1);
            *group.rounded_origin_residuals.entry(21).or_default() += 1;
            group.contact_residuals.push(-30.0);
            group.contact_residuals.push(30.0);
        }

        let placement = inferred_placement(PlacementModelArg::Auto, &mut group).unwrap();

        assert!((placement.z_offset - 21.0).abs() < f32::EPSILON);
    }

    #[test]
    fn auto_keeps_contact_model_when_origin_residuals_are_broad() {
        let mut group = OriginPlacementSampleGroup::default();
        for offset in 0_i16..40 {
            let residual = f32::from(offset);
            group.origin_residuals.push(residual);
            *group
                .rounded_origin_residuals
                .entry(i32::from(offset))
                .or_default() += 1;
            group.contact_residuals.push(0.1);
        }

        assert!(inferred_placement(PlacementModelArg::Auto, &mut group).is_none());
    }

    #[test]
    fn forced_origin_uses_modal_origin_residual() {
        let mut group = OriginPlacementSampleGroup::default();
        for (residual, rounded) in [(19.9, 20); 8].into_iter().chain([(21.1, 21); 24]) {
            group.origin_residuals.push(residual);
            *group.rounded_origin_residuals.entry(rounded).or_default() += 1;
        }

        let placement = inferred_placement(PlacementModelArg::Origin, &mut group).unwrap();

        assert!((placement.z_offset - 21.0).abs() < f32::EPSILON);
    }
}

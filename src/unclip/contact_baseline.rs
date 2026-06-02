// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeMap;

use tes3::esp::Plugin;

use crate::groundcover::CancellationToken;

use super::{
    mesh::{MeshCache, StaticMeshIndex},
    target::TargetRefIndex,
    terrain::TerrainIndex,
};

const MIN_BASELINE_SAMPLES: usize = 16;

#[derive(Clone, Debug, Default)]
pub(super) struct ContactBaselineIndex {
    baselines: BTreeMap<String, ContactBaseline>,
    meshes: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ContactBaseline {
    pub(super) delta: f32,
    pub(super) samples: usize,
    pub(super) min_delta: f32,
    pub(super) p05_delta: f32,
    pub(super) p95_delta: f32,
    pub(super) max_delta: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ContactBaselineDiagnostic {
    pub(super) id: String,
    pub(super) mesh: String,
    pub(super) baseline: ContactBaseline,
}

impl ContactBaseline {
    pub(super) fn is_calibrated(self) -> bool {
        self.samples > 0
    }
}

impl ContactBaselineIndex {
    pub(super) fn get(&self, id: &str) -> ContactBaseline {
        self.baselines
            .get(&id.to_lowercase())
            .copied()
            .unwrap_or(ContactBaseline {
                delta: 0.0,
                samples: 0,
                min_delta: 0.0,
                p05_delta: 0.0,
                p95_delta: 0.0,
                max_delta: 0.0,
            })
    }

    pub(super) fn diagnostics(&self) -> Vec<ContactBaselineDiagnostic> {
        self.baselines
            .iter()
            .map(|(id, &baseline)| ContactBaselineDiagnostic {
                id: id.clone(),
                mesh: self.meshes.get(id).cloned().unwrap_or_default(),
                baseline,
            })
            .collect()
    }
}

pub(super) fn build_contact_baselines(
    plugin: &Plugin,
    target_refs: &TargetRefIndex,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshCache<'_>,
    cancellation: &CancellationToken,
) -> std::io::Result<ContactBaselineIndex> {
    let mut samples = BTreeMap::<String, Vec<f32>>::new();
    let mut meshes = BTreeMap::<String, String>::new();

    for (_, _, reference) in target_refs.iter_refs(plugin) {
        super::check_cancellation(cancellation)?;
        if reference.deleted == Some(true) {
            continue;
        }
        let Some(static_mesh) = static_index.get(&reference.id) else {
            continue;
        };
        let Ok(geometry) = mesh_contacts.geometry(static_mesh) else {
            continue;
        };
        let contact_position = geometry.contact.world_position(
            reference.translation,
            reference.rotation,
            reference.scale,
        );
        let Some(terrain_z) = terrain.height_at(reference.translation[0], reference.translation[1])
        else {
            continue;
        };
        samples
            .entry(static_mesh.static_id.to_lowercase())
            .or_default()
            .push(contact_position[2] - terrain_z);
        meshes
            .entry(static_mesh.static_id.to_lowercase())
            .or_insert_with(|| static_mesh.mesh_path.clone());
    }

    let baselines = samples
        .into_iter()
        .filter_map(|(id, mut samples)| {
            (samples.len() >= MIN_BASELINE_SAMPLES).then(|| {
                samples.sort_by(f32::total_cmp);
                let baseline = ContactBaseline {
                    delta: median(&samples),
                    samples: samples.len(),
                    min_delta: samples[0],
                    p05_delta: percentile_sorted(&samples, 0.05),
                    p95_delta: percentile_sorted(&samples, 0.95),
                    max_delta: samples[samples.len() - 1],
                };
                (id, baseline)
            })
        })
        .collect();

    Ok(ContactBaselineIndex { baselines, meshes })
}

fn median(samples: &[f32]) -> f32 {
    let middle = samples.len() / 2;
    if samples.len().is_multiple_of(2) {
        (samples[middle - 1] + samples[middle]) * 0.5
    } else {
        samples[middle]
    }
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::median;
    use super::{ContactBaseline, ContactBaselineIndex};

    #[test]
    fn median_uses_middle_sample_for_odd_counts() {
        assert_close(median(&[-10.0, 2.0, 8.0]), 2.0);
    }

    #[test]
    fn median_averages_middle_samples_for_even_counts() {
        assert_close(median(&[-10.0, 2.0, 8.0, 20.0]), 5.0);
    }

    #[test]
    fn baseline_lookup_is_case_insensitive() {
        let index = ContactBaselineIndex {
            baselines: BTreeMap::from([(
                "grass_static".to_owned(),
                ContactBaseline {
                    delta: -40.0,
                    samples: 16,
                    min_delta: -42.0,
                    p05_delta: -41.0,
                    p95_delta: -39.0,
                    max_delta: -38.0,
                },
            )]),
            meshes: BTreeMap::from([("grass_static".to_owned(), "grass\\mesh.nif".to_owned())]),
        };

        assert_close(index.get("GRASS_STATIC").delta, -40.0);
        assert!(index.get("GRASS_STATIC").is_calibrated());
    }

    #[test]
    fn missing_baseline_defaults_to_zero_delta() {
        let index = ContactBaselineIndex::default();

        assert_close(index.get("grass_static").delta, 0.0);
        assert!(!index.get("grass_static").is_calibrated());
    }

    #[test]
    fn diagnostics_include_sorted_baseline_ids() {
        let index = ContactBaselineIndex {
            baselines: BTreeMap::from([
                (
                    "b".to_owned(),
                    ContactBaseline {
                        delta: 2.0,
                        samples: 16,
                        min_delta: 1.0,
                        p05_delta: 1.0,
                        p95_delta: 3.0,
                        max_delta: 3.0,
                    },
                ),
                (
                    "a".to_owned(),
                    ContactBaseline {
                        delta: 1.0,
                        samples: 16,
                        min_delta: 0.0,
                        p05_delta: 0.0,
                        p95_delta: 2.0,
                        max_delta: 2.0,
                    },
                ),
            ]),
            meshes: BTreeMap::new(),
        };

        let diagnostics = index.diagnostics();

        assert_eq!(diagnostics[0].id, "a");
        assert_eq!(diagnostics[1].id, "b");
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }
}

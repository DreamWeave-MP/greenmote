use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, HashSet},
    io,
    path::PathBuf,
};

#[cfg(test)]
use tes3::esp::Header;
use tes3::esp::{Cell, Plugin, Static};

use crate::groundcover::mesh;

pub struct LoadedPlugin {
    pub load_index: usize,
    pub plugin_name: String,
    pub plugin_path: PathBuf,
    pub plugin: Plugin,
}

impl LoadedPlugin {
    #[must_use]
    pub fn source_master(&self) -> MasterSpec {
        MasterSpec::from_path(&self.plugin_path)
    }

    #[must_use]
    #[cfg(test)]
    pub fn header_masters(&self) -> Vec<MasterSpec> {
        self.plugin
            .objects_of_type::<Header>()
            .next()
            .map_or_else(Vec::new, |header| {
                header
                    .masters
                    .iter()
                    .map(|(name, size)| MasterSpec {
                        name: name.clone(),
                        size: *size,
                    })
                    .collect()
            })
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MasterSpec {
    pub name: String,
    pub size: u64,
}

impl MasterSpec {
    #[must_use]
    pub fn from_path(path: &std::path::Path) -> Self {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().to_string(),
        );
        let size = std::fs::metadata(path).map_or(0, |metadata| metadata.len());

        Self { name, size }
    }

    #[must_use]
    pub fn as_header_master(&self) -> (String, u64) {
        (self.name.clone(), self.size)
    }
}

#[derive(Debug)]
pub struct StaticPlan {
    pub source_load_index: usize,
    pub source_plugin_name: String,
    // Kept for diagnostics and test fixtures even when a particular build path only needs the
    // source master identity.
    #[allow(dead_code)]
    pub source_plugin_path: PathBuf,
    // Kept with the static plan so generated IDs and output summaries can retain source identity
    // without re-reading plugin headers.
    #[allow(dead_code)]
    pub source_master: MasterSpec,
    pub source_static: Static,
    pub generated_id: String,
}

impl StaticPlan {
    #[must_use]
    pub fn id_key(&self) -> String {
        self.source_static.id.to_ascii_lowercase()
    }

    /// Builds the generated static record for a used source static.
    ///
    /// # Errors
    ///
    /// Returns invalid input if the source mesh path cannot be safely rooted under `grass\`.
    pub fn output_static(&self) -> io::Result<Static> {
        let mut output_static = self.source_static.clone();
        output_static.id.clone_from(&self.generated_id);
        output_static.mesh = mesh::grass_prefixed_mesh(&output_static.mesh)?;

        Ok(output_static)
    }

    /// Builds the VFS lookup/output path pair for a used source static mesh.
    ///
    /// # Errors
    ///
    /// Returns invalid input if the source mesh path contains unsafe components.
    pub fn mesh_copy_path(&self) -> io::Result<mesh::MeshCopyPath> {
        mesh::normalize_mesh_for_copy(&self.source_static.mesh)
    }
}

#[derive(Debug)]
pub struct PluginCellPlan {
    pub load_index: usize,
    pub plugin_name: String,
    // Kept for diagnostics and tests; output generation currently needs the plugin name and master
    // identity more often than the path.
    #[allow(dead_code)]
    pub plugin_path: PathBuf,
    pub source_master: MasterSpec,
    pub header_masters: Vec<MasterSpec>,
    pub groundcover_cells: Vec<Cell>,
    pub touched_refs: usize,
    pub used_static_ids: BTreeSet<String>,
}

impl PluginCellPlan {
    #[must_use]
    pub fn master_for_source_index(&self, mast_index: u32) -> Option<&MasterSpec> {
        if mast_index == 0 {
            return Some(&self.source_master);
        }

        let index = usize::try_from(mast_index - 1).ok()?;
        self.header_masters.get(index)
    }

    #[must_use]
    pub fn is_used(&self) -> bool {
        self.touched_refs > 0
    }
}

#[derive(Debug)]
pub struct ConversionPlan {
    pub static_plans: Vec<StaticPlan>,
    pub cell_plans: Vec<PluginCellPlan>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub matched_static_ids: HashSet<String>,
    pub used_static_ids: BTreeSet<String>,
}

impl ConversionPlan {
    #[must_use]
    pub fn used_static_plans(&self) -> Vec<&StaticPlan> {
        let static_plans_by_id = self
            .static_plans
            .iter()
            .map(|static_plan| (static_plan.id_key(), static_plan))
            .collect::<BTreeMap<_, _>>();

        self.used_static_ids
            .iter()
            .filter_map(|id| static_plans_by_id.get(id).copied())
            .collect()
    }

    #[must_use]
    pub fn generated_static_ids_by_source_id(&self) -> BTreeMap<String, String> {
        self.static_plans
            .iter()
            .map(|static_plan| (static_plan.id_key(), static_plan.generated_id.clone()))
            .collect()
    }

    /// Builds the mesh copy set for generated static records.
    ///
    /// # Errors
    ///
    /// Returns invalid input if a used static contains an unsafe mesh path.
    pub fn used_mesh_paths(&self) -> io::Result<BTreeSet<mesh::MeshCopyPath>> {
        self.used_static_plans()
            .into_iter()
            .map(StaticPlan::mesh_copy_path)
            .collect()
    }
}

#[derive(Debug)]
pub struct StaticConversionPlan {
    pub static_plans: Vec<StaticPlan>,
    pub matched_static_ids: HashSet<String>,
}

impl StaticConversionPlan {
    #[must_use]
    pub fn with_cell_plans(self, mut cell_plans: Vec<PluginCellPlan>) -> ConversionPlan {
        cell_plans.sort_by_key(|cell_plan| Reverse(cell_plan.load_index));
        let used_static_ids = cell_plans
            .iter()
            .flat_map(|cell_plan| cell_plan.used_static_ids.iter().cloned())
            .collect();

        ConversionPlan {
            static_plans: self.static_plans,
            cell_plans,
            matched_static_ids: self.matched_static_ids,
            used_static_ids,
        }
    }
}

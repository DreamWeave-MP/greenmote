use std::{collections::HashSet, hash::BuildHasher};

use rayon::prelude::*;

use crate::groundcover::records;

use super::{LoadedPlugin, PluginCellPlan};

#[must_use]
pub fn scan_cells_parallel<S: BuildHasher + Sync>(
    loaded_plugins: &[LoadedPlugin],
    matched_static_ids: &HashSet<String, S>,
) -> Vec<PluginCellPlan> {
    loaded_plugins
        .par_iter()
        .map(|loaded| {
            let (groundcover_cells, deleted_cells, touched_refs, used_static_ids) =
                records::process_exterior_cells(&loaded.plugin, matched_static_ids);

            PluginCellPlan {
                load_index: loaded.load_index,
                plugin_name: loaded.plugin_name.clone(),
                plugin_path: loaded.plugin_path.clone(),
                source_master: loaded.source_master(),
                header_masters: loaded.header_masters(),
                groundcover_cells,
                deleted_cells,
                touched_refs,
                used_static_ids,
            }
        })
        .collect()
}

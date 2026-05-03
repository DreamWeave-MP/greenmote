use std::{
    collections::HashSet,
    hash::BuildHasher,
    io,
    sync::atomic::{AtomicUsize, Ordering},
};

use rayon::prelude::*;

use crate::groundcover::{progress::CancellationToken, records};

use super::{LoadedPlugin, PluginCellPlan};

/// Scans loaded exterior cells for references to planned statics.
///
/// # Errors
///
/// Returns `Interrupted` if cancellation is requested before queued work starts.
pub fn scan_cells_parallel<S: BuildHasher + Sync>(
    loaded_plugins: &[LoadedPlugin],
    matched_static_ids: &HashSet<String, S>,
    progress: &(dyn Fn(usize, usize) + Sync),
    cancellation: &CancellationToken,
) -> io::Result<Vec<PluginCellPlan>> {
    let total = loaded_plugins.len();
    let completed = AtomicUsize::new(0);

    loaded_plugins
        .par_iter()
        .map(|loaded| {
            if cancellation.is_cancelled() {
                return Err(cancelled_error());
            }

            let (groundcover_cells, touched_refs, used_static_ids) =
                records::process_exterior_cells(&loaded.plugin, matched_static_ids);

            let cell_plan = PluginCellPlan {
                load_index: loaded.load_index,
                plugin_name: loaded.plugin_name.clone(),
                plugin_path: loaded.plugin_path.clone(),
                source_master: loaded.source_master(),
                header_masters: loaded.header_masters(),
                groundcover_cells,
                touched_refs,
                used_static_ids,
            };

            let current = completed.fetch_add(1, Ordering::Relaxed) + 1;
            progress(current, total);
            Ok(cell_plan)
        })
        .collect()
}

fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "conversion cancelled")
}

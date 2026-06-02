// SPDX-License-Identifier: GPL-3.0-only

#[cfg(test)]
mod cells;
mod ids;
mod model;
mod statics;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub use cells::scan_cells_parallel;
#[cfg(test)]
pub use model::SourceRecordKind;
pub use model::{
    ConversionPlan, LoadedPlugin, MasterSpec, PluginCellPlan, SourceRecord, StaticConversionPlan,
    StaticPlan,
};
pub use statics::build_static_conversion_plan;

#[cfg(test)]
use crate::groundcover::{GroundcoverConfig, progress::CancellationToken};

/// Builds the full conversion plan from already-loaded plugins.
///
/// Mesh path validation is deferred until output work is derived from used source records, so
/// unused matching records do not fail or pay mesh-copy costs.
///
/// # Panics
///
/// Panics only if the uncancelled default cancellation token reports cancellation.
#[must_use]
#[cfg(test)]
pub fn build_conversion_plan(
    loaded_plugins: &[LoadedPlugin],
    config: &GroundcoverConfig,
) -> ConversionPlan {
    let static_plan = build_static_conversion_plan(loaded_plugins, config);
    let cell_plans = if static_plan.matched_source_ids.is_empty() {
        Vec::new()
    } else {
        scan_cells_parallel(
            loaded_plugins,
            &static_plan.matched_source_ids,
            &|_, _| {},
            &CancellationToken::default(),
        )
        .expect("default cancellation token is never cancelled")
    };

    static_plan.with_cell_plans(cell_plans)
}

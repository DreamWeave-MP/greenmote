mod cells;
mod ids;
mod model;
mod statics;

#[cfg(test)]
mod tests;

pub use cells::scan_cells_parallel;
pub use model::{
    ConversionPlan, LoadedPlugin, MasterSpec, PluginCellPlan, StaticConversionPlan, StaticPlan,
};
pub use statics::build_static_conversion_plan;

use crate::groundcover::GroundcoverConfig;

/// Builds the full conversion plan from already-loaded plugins.
///
/// Mesh path validation is deferred until output work is derived from used statics, so unused
/// matching statics do not fail or pay mesh-copy costs.
#[must_use]
pub fn build_conversion_plan(
    loaded_plugins: &[LoadedPlugin],
    config: &GroundcoverConfig,
) -> ConversionPlan {
    let static_plan = build_static_conversion_plan(loaded_plugins, config);
    let cell_plans = if static_plan.matched_static_ids.is_empty() {
        Vec::new()
    } else {
        scan_cells_parallel(loaded_plugins, &static_plan.matched_static_ids, &|_, _| {})
    };

    static_plan.with_cell_plans(cell_plans)
}

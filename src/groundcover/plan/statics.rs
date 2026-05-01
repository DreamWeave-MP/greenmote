use std::collections::HashSet;

use tes3::esp::Static;

use crate::groundcover::GroundcoverConfig;

use super::{LoadedPlugin, StaticConversionPlan, StaticPlan, ids};

/// Builds the conversion plan portion that only needs `Header | Static` records.
///
/// This phase records winning static definitions only. Mesh validation is deliberately deferred
/// until a static is actually used by a converted exterior reference.
#[must_use]
pub fn build_static_conversion_plan(
    loaded_plugins: &[LoadedPlugin],
    config: &GroundcoverConfig,
) -> StaticConversionPlan {
    let (static_plans, matched_static_ids) = collect_winning_statics(loaded_plugins, config);

    StaticConversionPlan {
        static_plans,
        matched_static_ids,
    }
}

fn collect_winning_statics(
    loaded_plugins: &[LoadedPlugin],
    config: &GroundcoverConfig,
) -> (Vec<StaticPlan>, HashSet<String>) {
    let mut seen_static_ids = HashSet::new();
    let all_static_ids = loaded_plugins
        .iter()
        .flat_map(|loaded| loaded.plugin.objects_of_type::<Static>())
        .map(|static_record| static_record.id.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let mut generated_static_ids = HashSet::new();
    let mut matched_static_ids = HashSet::new();
    let mut static_plans = Vec::new();

    let mut plugins_in_reverse_load_order = loaded_plugins.iter().collect::<Vec<_>>();
    plugins_in_reverse_load_order.sort_by(|left, right| right.load_index.cmp(&left.load_index));

    for loaded in plugins_in_reverse_load_order {
        for static_record in loaded.plugin.objects_of_type::<Static>() {
            let lower_id = static_record.id.to_ascii_lowercase();
            if !seen_static_ids.insert(lower_id.clone()) || !config.matches_static_id(&lower_id) {
                continue;
            }

            matched_static_ids.insert(lower_id);
            let source_master = loaded.source_master();
            let generated_id = ids::allocate_generated_static_id(
                &static_record.id.to_ascii_lowercase(),
                &source_master,
                &all_static_ids,
                &mut generated_static_ids,
            );

            static_plans.push(StaticPlan {
                source_load_index: loaded.load_index,
                source_plugin_name: loaded.plugin_name.clone(),
                source_plugin_path: loaded.plugin_path.clone(),
                source_master,
                source_static: static_record.clone(),
                generated_id,
            });
        }
    }

    (static_plans, matched_static_ids)
}

// SPDX-License-Identifier: GPL-3.0-only

use std::{cmp::Reverse, collections::HashSet};

use tes3::esp::{ObjectFlags, TES3Object};

use crate::groundcover::GroundcoverConfig;

use super::{LoadedPlugin, SourceRecord, StaticConversionPlan, StaticPlan, ids};

/// Builds the conversion plan portion that only needs `Header | Static | Activator` records.
///
/// This phase records winning static definitions and scriptless activators. Mesh validation is
/// deliberately deferred until a source record is actually used by a converted exterior reference.
#[must_use]
pub fn build_static_conversion_plan(
    loaded_plugins: &[LoadedPlugin],
    config: &GroundcoverConfig,
) -> StaticConversionPlan {
    let (static_plans, matched_source_ids) = collect_winning_source_records(loaded_plugins, config);

    StaticConversionPlan {
        static_plans,
        matched_source_ids,
    }
}

fn collect_winning_source_records(
    loaded_plugins: &[LoadedPlugin],
    config: &GroundcoverConfig,
) -> (Vec<StaticPlan>, HashSet<String>) {
    let mut seen_source_ids = HashSet::new();
    let all_source_ids = loaded_plugins
        .iter()
        .flat_map(|loaded| loaded.plugin.objects.iter())
        .filter_map(source_record_id)
        .map(str::to_ascii_lowercase)
        .collect::<HashSet<_>>();
    let mut generated_static_ids = HashSet::new();
    let mut matched_source_ids = HashSet::new();
    let mut static_plans = Vec::new();

    let mut plugins_in_reverse_load_order = loaded_plugins.iter().collect::<Vec<_>>();
    plugins_in_reverse_load_order.sort_by_key(|loaded| Reverse(loaded.load_index));

    for loaded in plugins_in_reverse_load_order {
        for object in &loaded.plugin.objects {
            let Some(source_id) = source_record_id(object) else {
                continue;
            };
            let lower_id = source_id.to_ascii_lowercase();
            if !seen_source_ids.insert(lower_id.clone()) || !config.matches_static_id(source_id) {
                continue;
            }

            let Some(source_record) = convertible_source_record(object) else {
                continue;
            };

            matched_source_ids.insert(lower_id.clone());
            let source_master = loaded.source_master();
            let generated_id = ids::allocate_generated_static_id(
                &lower_id,
                &source_master,
                &all_source_ids,
                &mut generated_static_ids,
            );

            static_plans.push(StaticPlan {
                source_load_index: loaded.load_index,
                source_plugin_name: loaded.plugin_name.clone(),
                source_plugin_path: loaded.plugin_path.clone(),
                source_master,
                source_record,
                generated_id,
            });
        }
    }

    (static_plans, matched_source_ids)
}

fn source_record_id(object: &TES3Object) -> Option<&str> {
    match object {
        TES3Object::Static(record) => Some(record.id.as_str()),
        TES3Object::Activator(record) => Some(record.id.as_str()),
        _ => None,
    }
}

fn convertible_source_record(object: &TES3Object) -> Option<SourceRecord> {
    match object {
        TES3Object::Static(record) if !record.flags.contains(ObjectFlags::DELETED) => {
            Some(SourceRecord::from_static(record))
        }
        TES3Object::Activator(record)
            if !record.flags.contains(ObjectFlags::DELETED)
                && record.script.trim().is_empty()
                && !record.mesh.trim().is_empty() =>
        {
            Some(SourceRecord::from_scriptless_activator(record))
        }
        _ => None,
    }
}

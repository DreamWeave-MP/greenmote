use std::io::{self, Write};

use crate::groundcover::{
    DELETED_PLUGIN_NAME, GROUNDCOVER_PLUGIN_NAME, GroundcoverConfig, mesh,
    plan::{ConversionPlan, PluginCellPlan, StaticPlan},
};

#[derive(Debug)]
pub struct RunSummary {
    pub content_files: usize,
    pub loaded_plugins: usize,
    pub skipped_generated_plugins: Vec<String>,
    pub matched_records: usize,
    pub used_records: usize,
    pub changed_cells: usize,
    pub touched_refs: usize,
    pub meshes_to_copy: usize,
}

pub fn write_summary(
    mut writer: impl Write,
    summary: &RunSummary,
    plan: &ConversionPlan,
    config: &GroundcoverConfig,
) -> io::Result<()> {
    writeln!(writer, "# greenmote convert {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(
        writer,
        "# output directory: {}",
        config.output_directory.display()
    )?;
    writeln!(
        writer,
        "# groundcover output: {}",
        config
            .output_directory
            .join(GROUNDCOVER_PLUGIN_NAME)
            .display()
    )?;
    writeln!(
        writer,
        "# deleted output: {}",
        config.output_directory.join(DELETED_PLUGIN_NAME).display()
    )?;
    writeln!(writer, "# content files: {}", summary.content_files)?;
    writeln!(writer, "# loaded plugins: {}", summary.loaded_plugins)?;
    writeln!(
        writer,
        "# skipped generated plugins: {}",
        summary.skipped_generated_plugins.len()
    )?;
    writeln!(writer, "# matched records: {}", summary.matched_records)?;
    writeln!(writer, "# used records: {}", summary.used_records)?;
    writeln!(writer, "# changed cells: {}", summary.changed_cells)?;
    writeln!(writer, "# touched refs: {}", summary.touched_refs)?;
    writeln!(writer, "# meshes to copy: {}", summary.meshes_to_copy)?;

    for plugin in &summary.skipped_generated_plugins {
        writeln!(writer, "SKIP generated plugin {plugin:?}")?;
    }

    let mut used_static_plans = plan.used_static_plans();
    used_static_plans.sort_by(|left, right| compare_static_report_order(left, right));
    for static_plan in used_static_plans {
        let output_static = static_plan.output_static()?;
        writeln!(
            writer,
            "{} {:?} from {:?}: generated STAT {:?}; mesh {:?} -> {:?}",
            static_plan.source_record.kind_label(),
            static_plan.source_record.id,
            static_plan.source_plugin_name,
            output_static.id,
            static_plan.source_record.mesh,
            output_static.mesh
        )?;
    }

    let mut used_cell_plans = plan
        .cell_plans
        .iter()
        .filter(|cell_plan| cell_plan.is_used())
        .collect::<Vec<_>>();
    used_cell_plans.sort_by(|left, right| compare_cell_report_order(left, right));
    for cell_plan in used_cell_plans {
        writeln!(
            writer,
            "CELL refs from {:?}: {} refs in {} exterior cells",
            cell_plan.plugin_name,
            cell_plan.touched_refs,
            cell_plan.groundcover_cells.len()
        )?;
    }

    for mesh_path in &plan.used_mesh_paths()? {
        writeln!(
            writer,
            "MESH {:?} -> {}",
            mesh_path.source,
            mesh::mesh_output_path(&config.output_directory, &mesh_path.target)?.display()
        )?;
    }

    Ok(())
}

fn compare_static_report_order(left: &StaticPlan, right: &StaticPlan) -> std::cmp::Ordering {
    left.source_load_index
        .cmp(&right.source_load_index)
        .then_with(|| left.source_record.id.cmp(&right.source_record.id))
}

fn compare_cell_report_order(left: &PluginCellPlan, right: &PluginCellPlan) -> std::cmp::Ordering {
    left.load_index
        .cmp(&right.load_index)
        .then_with(|| left.plugin_name.cmp(&right.plugin_name))
}

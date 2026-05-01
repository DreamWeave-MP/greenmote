use std::io::{self, Write};

use crate::groundcover::{GroundcoverConfig, mesh, plan::ConversionPlan};

#[derive(Debug)]
pub struct RunSummary {
    pub content_files: usize,
    pub loaded_plugins: usize,
    pub skipped_generated_plugins: Vec<String>,
    pub matched_statics: usize,
    pub used_statics: usize,
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
            .join(&config.groundcover_output)
            .display()
    )?;
    writeln!(
        writer,
        "# deleted output: {}",
        config
            .output_directory
            .join(&config.deleted_output)
            .display()
    )?;
    writeln!(writer, "# content files: {}", summary.content_files)?;
    writeln!(writer, "# loaded plugins: {}", summary.loaded_plugins)?;
    writeln!(
        writer,
        "# skipped generated plugins: {}",
        summary.skipped_generated_plugins.len()
    )?;
    writeln!(writer, "# matched statics: {}", summary.matched_statics)?;
    writeln!(writer, "# used statics: {}", summary.used_statics)?;
    writeln!(writer, "# changed cells: {}", summary.changed_cells)?;
    writeln!(writer, "# touched refs: {}", summary.touched_refs)?;
    writeln!(writer, "# meshes to copy: {}", summary.meshes_to_copy)?;

    for plugin in &summary.skipped_generated_plugins {
        writeln!(writer, "SKIP generated plugin {plugin:?}")?;
    }

    for static_plan in plan.used_static_plans() {
        let output_static = static_plan.output_static()?;
        writeln!(
            writer,
            "STAT {:?} from {:?}: generated {:?}; mesh {:?} -> {:?}",
            static_plan.source_static.id,
            static_plan.source_plugin_name,
            output_static.id,
            static_plan.source_static.mesh,
            output_static.mesh
        )?;
    }

    for cell_plan in plan.cell_plans.iter().filter(|plan| plan.is_used()) {
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

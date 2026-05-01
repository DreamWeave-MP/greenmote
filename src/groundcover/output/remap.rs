use std::{collections::BTreeMap, io};

use crate::groundcover::plan::{MasterSpec, PluginCellPlan};

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum RefIdMode {
    Source,
    Generated,
}

pub fn remap_cell(
    cell: &tes3::esp::Cell,
    cell_plan: &PluginCellPlan,
    master_indices: &BTreeMap<MasterSpec, u32>,
    ref_id_mode: RefIdMode,
    generated_static_ids: &BTreeMap<String, String>,
) -> io::Result<tes3::esp::Cell> {
    let mut remapped = cell.clone();
    remapped.references.clear();

    for (key, reference) in &cell.references {
        let remapped_key_mast = remap_source_mast_index(key.0, cell_plan, master_indices)?;
        let mut remapped_reference = reference.clone();
        if ref_id_mode == RefIdMode::Generated {
            let source_id = remapped_reference.id.to_ascii_lowercase();
            remapped_reference.id =
                generated_static_ids
                    .get(&source_id)
                    .cloned()
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "generated static id map is missing source static {source_id:?}"
                            ),
                        )
                    })?;
        }
        remapped_reference.mast_index =
            remap_source_mast_index(remapped_reference.mast_index, cell_plan, master_indices)?;
        remapped
            .references
            .insert((remapped_key_mast, key.1), remapped_reference);
    }

    Ok(remapped)
}

fn remap_source_mast_index(
    mast_index: u32,
    cell_plan: &PluginCellPlan,
    master_indices: &BTreeMap<MasterSpec, u32>,
) -> io::Result<u32> {
    let source_master = cell_plan.master_for_source_index(mast_index).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, format!("cell ref in {} uses source master index {mast_index}, but that plugin header does not define it", cell_plan.plugin_name))
    })?;
    master_indices.get(source_master).copied().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "generated master list is missing {} while remapping {}",
                source_master.name, cell_plan.plugin_name
            ),
        )
    })
}

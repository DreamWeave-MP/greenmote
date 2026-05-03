use std::collections::BTreeMap;
use std::io;

use crate::groundcover::plan::{ConversionPlan, MasterSpec, PluginCellPlan};

const MAX_GENERATED_MASTERS: usize = 255;

pub fn groundcover_master_indices(plan: &ConversionPlan) -> BTreeMap<MasterSpec, u32> {
    let mut masters = MasterIndexBuilder::new(plan);

    for cell_plan in plan
        .cell_plans
        .iter()
        .filter(|cell_plan| cell_plan.is_used())
    {
        for cell in &cell_plan.groundcover_cells {
            masters.insert_cell_reference_masters(cell, cell_plan);
        }
    }

    masters.into_map()
}

pub fn deleted_master_indices(plan: &ConversionPlan) -> BTreeMap<MasterSpec, u32> {
    let mut masters = MasterIndexBuilder::new(plan);

    for cell_plan in plan
        .cell_plans
        .iter()
        .filter(|cell_plan| cell_plan.is_used())
    {
        for cell in &cell_plan.groundcover_cells {
            masters.insert_cell_reference_masters(cell, cell_plan);
        }
    }

    masters.into_map()
}

pub fn validate_master_count(
    output_name: &str,
    master_indices: &BTreeMap<MasterSpec, u32>,
) -> io::Result<()> {
    if master_indices.len() > MAX_GENERATED_MASTERS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{output_name} output needs {} masters, but TES3 reference indices only support {MAX_GENERATED_MASTERS}",
                master_indices.len()
            ),
        ));
    }

    Ok(())
}

struct MasterIndexBuilder {
    source_load_indices: BTreeMap<MasterSpec, usize>,
    masters: Vec<(usize, MasterSpec)>,
}

impl MasterIndexBuilder {
    fn new(plan: &ConversionPlan) -> Self {
        let source_load_indices = plan
            .cell_plans
            .iter()
            .map(|cell_plan| (cell_plan.source_master.clone(), cell_plan.load_index))
            .collect();

        Self {
            source_load_indices,
            masters: Vec::new(),
        }
    }

    fn insert(&mut self, master: MasterSpec) {
        if !self.masters.iter().any(|(_, existing)| existing == &master) {
            let load_index = self
                .source_load_indices
                .get(&master)
                .copied()
                .unwrap_or(usize::MAX);
            self.masters.push((load_index, master));
        }
    }

    fn insert_cell_reference_masters(
        &mut self,
        cell: &tes3::esp::Cell,
        cell_plan: &PluginCellPlan,
    ) {
        for (key, reference) in &cell.references {
            self.insert_source_master(key.0, cell_plan);
            if reference.mast_index != key.0 {
                self.insert_source_master(reference.mast_index, cell_plan);
            }
        }
    }

    fn insert_source_master(&mut self, mast_index: u32, cell_plan: &PluginCellPlan) {
        if let Some(master) = cell_plan.master_for_source_index(mast_index) {
            self.insert(master.clone());
        }
    }

    fn into_map(self) -> BTreeMap<MasterSpec, u32> {
        let mut masters = self.masters;
        masters.sort_by(|(left_index, left_master), (right_index, right_master)| {
            left_index
                .cmp(right_index)
                .then_with(|| left_master.name.cmp(&right_master.name))
        });

        masters
            .into_iter()
            .enumerate()
            .map(|(index, (_, master))| (master, u32::try_from(index + 1).unwrap_or(u32::MAX)))
            .collect()
    }
}

pub fn masters_from_index_map(master_indices: &BTreeMap<MasterSpec, u32>) -> Vec<(String, u64)> {
    let mut indexed_masters = master_indices.iter().collect::<Vec<_>>();
    indexed_masters.sort_by_key(|(_, index)| *index);

    indexed_masters
        .into_iter()
        .map(|(master, _)| master.as_header_master())
        .collect()
}

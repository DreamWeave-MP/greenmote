// SPDX-License-Identifier: GPL-3.0-only

use std::{
    collections::{BTreeMap, btree_map::Entry},
    io,
};

use tes3::esp::{Cell, FixedString, Header, ObjectFlags, Plugin, types::FileType};

use crate::groundcover::{
    GENERATED_PLUGIN_AUTHOR, GENERATED_PLUGIN_DESCRIPTION, plan::ConversionPlan,
};

use super::{
    masters,
    remap::{self, RefIdMode},
};

#[derive(Debug)]
pub struct BuiltPlugins {
    pub groundcover_plugin: Plugin,
    pub deleted_plugin: Plugin,
    pub groundcover_header: Header,
    pub deleted_header: Header,
}

pub fn build_plugins(plan: &ConversionPlan) -> io::Result<BuiltPlugins> {
    let mut groundcover_plugin = Plugin::new();
    let mut deleted_plugin = Plugin::new();
    let mut groundcover_header = groundcover_header();
    let mut deleted_header = deleted_header();
    let groundcover_master_indices = masters::groundcover_master_indices(plan);
    let deleted_master_indices = masters::deleted_master_indices(plan);
    let generated_static_ids = plan.generated_static_ids_by_source_id();
    masters::validate_master_count("groundcover", &groundcover_master_indices)?;
    masters::validate_master_count("deleted groundcover", &deleted_master_indices)?;

    for static_plan in plan.used_static_plans() {
        groundcover_plugin
            .objects
            .push(static_plan.output_static()?.into());
    }

    // One CELL record per grid. OpenMW's groundcover loader appends the contexts of every
    // record for a grid in file order and keeps the last copy of each reference number, so
    // writing one record per source plugin would let the oldest plugin's copy win. The cell
    // plans are sorted newest first, so the first copy seen of each reference is the winner in
    // load order, and a newer plugin's deleted copy shadows the older placement.
    let mut groundcover_cells: BTreeMap<(i32, i32), Cell> = BTreeMap::new();
    let mut deleted_cells: BTreeMap<(i32, i32), Cell> = BTreeMap::new();
    for cell_plan in &plan.cell_plans {
        if !cell_plan.is_used() {
            continue;
        }

        for cell in &cell_plan.groundcover_cells {
            let remapped = remap::remap_cell(
                cell,
                cell_plan,
                &groundcover_master_indices,
                RefIdMode::Generated,
                &generated_static_ids,
            )?;
            merge_cell(&mut groundcover_cells, remapped);
        }
        for cell in &cell_plan.groundcover_cells {
            let mut deleted_cell = remap::remap_cell(
                cell,
                cell_plan,
                &deleted_master_indices,
                RefIdMode::Source,
                &generated_static_ids,
            )?;
            for reference in deleted_cell.references.values_mut() {
                reference.deleted = Some(true);
            }
            merge_cell(&mut deleted_cells, deleted_cell);
        }
    }
    groundcover_plugin
        .objects
        .extend(groundcover_cells.into_values().map(Into::into));
    deleted_plugin
        .objects
        .extend(deleted_cells.into_values().map(Into::into));

    groundcover_header.masters = masters::masters_from_index_map(&groundcover_master_indices);
    deleted_header.masters = masters::masters_from_index_map(&deleted_master_indices);

    groundcover_header.num_objects = groundcover_plugin
        .objects
        .len()
        .try_into()
        .unwrap_or(u32::MAX);
    deleted_header.num_objects = deleted_plugin.objects.len().try_into().unwrap_or(u32::MAX);

    Ok(BuiltPlugins {
        groundcover_plugin,
        deleted_plugin,
        groundcover_header,
        deleted_header,
    })
}

/// Folds `cell` into the record for its grid, keeping the first copy of every reference.
fn merge_cell(cells: &mut BTreeMap<(i32, i32), Cell>, cell: Cell) {
    match cells.entry(cell.data.grid) {
        Entry::Vacant(slot) => {
            slot.insert(cell);
        }
        Entry::Occupied(mut slot) => {
            let target = slot.get_mut();
            for (key, reference) in cell.references {
                target.references.entry(key).or_insert(reference);
            }
        }
    }
}

fn groundcover_header() -> Header {
    Header {
        version: 1.3,
        author: FixedString(GENERATED_PLUGIN_AUTHOR.to_owned()),
        description: FixedString(GENERATED_PLUGIN_DESCRIPTION.to_owned()),
        file_type: FileType::Esp,
        flags: ObjectFlags::default(),
        num_objects: 0,
        masters: Vec::new(),
    }
}

fn deleted_header() -> Header {
    Header {
        version: 1.3,
        author: FixedString(GENERATED_PLUGIN_AUTHOR.to_owned()),
        description: FixedString(GENERATED_PLUGIN_DESCRIPTION.to_owned()),
        file_type: FileType::Esp,
        flags: ObjectFlags::default(),
        num_objects: 0,
        masters: Vec::new(),
    }
}

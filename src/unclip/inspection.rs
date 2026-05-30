use std::io;

use tes3::esp::Plugin;

use super::{
    args::UnclipPolicy,
    cells::CellCoord,
    mesh::{MeshCache, MeshContact, StaticMesh, StaticMeshIndex, WorldAabb},
    model::{
        BoundsInspection, MeshContactInspection, OriginInspection, ReferenceInspection,
        StaticBoundsOcclusionInspection, StaticMeshInspection, TerrainInspectionReport,
    },
    occlusion::{
        StaticBoundsAction, StaticOccluder, StaticOccluderIndex, decide_static_bounds_action,
    },
    orientation::orientation_angle_degrees,
    target::TargetRefIndex,
    terrain::TerrainIndex,
    write_plan::WriteStatusIndex,
    write_policy::{RefTransform, find_valid_relocation_transform},
    write_status::{
        MeshResolutionStatus, WriteStatusEvidence, WriteStatusInput, write_plan_evidence,
        write_status_label,
    },
};

pub(super) fn inspect_target_refs(
    plugin: &Plugin,
    target_refs: &TargetRefIndex,
    context: &mut ReferenceInspectionContext<'_, '_>,
    mut reference_sink: impl FnMut(&ReferenceInspection) -> io::Result<()>,
) -> io::Result<TerrainInspectionReport> {
    let mut report = TerrainInspectionReport::default();

    for (cell, key, reference) in target_refs.iter_refs(plugin) {
        inspect_reference(
            &mut report,
            context,
            cell,
            key,
            reference,
            &mut reference_sink,
        )?;
    }

    Ok(report)
}

pub(super) fn count_target_refs(
    plugin: &Plugin,
    target_refs: &TargetRefIndex,
    context: &mut ReferenceInspectionContext<'_, '_>,
) -> TerrainInspectionReport {
    let mut report = TerrainInspectionReport::default();

    for (cell, key, reference) in target_refs.iter_refs(plugin) {
        count_reference(&mut report, context, cell, key, reference);
    }

    report
}

pub(super) struct ReferenceInspectionContext<'a, 'b> {
    pub(super) terrain: &'a TerrainIndex,
    pub(super) static_index: &'a StaticMeshIndex,
    pub(super) mesh_contacts: &'a mut MeshCache<'b>,
    pub(super) static_occluders: &'a StaticOccluderIndex,
    pub(super) policy: &'a UnclipPolicy,
    pub(super) write: Option<&'a WriteStatusIndex>,
}

struct OriginDetails {
    terrain_z: Option<f32>,
    delta: Option<f32>,
    classification: &'static str,
}

fn inspect_reference(
    report: &mut TerrainInspectionReport,
    context: &mut ReferenceInspectionContext<'_, '_>,
    cell: CellCoord,
    key: (u32, u32),
    reference: &tes3::esp::Reference,
    reference_sink: &mut impl FnMut(&ReferenceInspection) -> io::Result<()>,
) -> io::Result<()> {
    report.refs += 1;
    if reference.deleted == Some(true) {
        report.refs_deleted += 1;
        let inspection = deleted_reference_inspection(
            cell,
            key,
            reference,
            if context
                .write
                .is_some_and(|write| write.is_deleted(cell, key))
            {
                "deleted_static_bounds_occluded"
            } else {
                "skipped_deleted_ref"
            },
        );
        return reference_sink(&inspection);
    }

    let static_bounds_context = StaticBoundsContext {
        terrain: context.terrain,
        static_occluders: context.static_occluders,
        policy: context.policy,
        write: context.write,
    };
    let mesh_contact = resolve_ref_mesh_contact(
        report,
        reference,
        context.static_index,
        context.mesh_contacts,
    );
    let contact_details = match &mesh_contact {
        MeshContactResolution::Resolved { contact, .. } => Some(classify_contact(
            report,
            context.terrain,
            reference,
            contact,
            context.policy.contact_epsilon,
        )),
        MeshContactResolution::UnresolvedStatic | MeshContactResolution::MissingContact { .. } => {
            None
        }
    };
    let static_bounds_occlusion = classify_static_bounds_occlusion(
        report,
        StaticBoundsOcclusionInput {
            cell,
            key,
            reference,
            mesh_resolution: &mesh_contact,
            context: static_bounds_context,
        },
    );
    let origin = classify_origin(
        report,
        context.terrain,
        reference,
        context.policy.origin_epsilon,
    );
    let inspection = reference_inspection(&ReferenceInspectionInput {
        cell,
        key,
        reference,
        origin: &origin,
        mesh_resolution: &mesh_contact,
        contact_details: contact_details.as_ref(),
        static_bounds_occlusion: static_bounds_occlusion.as_ref(),
        write: WriteStatusEvidence {
            plan: write_plan_evidence(context.write, cell, key),
            actions: context.policy.write_actions,
        },
        contact_epsilon: context.policy.contact_epsilon,
        orientation_epsilon_degrees: context.policy.orientation_epsilon_degrees,
    });
    reference_sink(&inspection)
}

fn count_reference(
    report: &mut TerrainInspectionReport,
    context: &mut ReferenceInspectionContext<'_, '_>,
    cell: CellCoord,
    key: (u32, u32),
    reference: &tes3::esp::Reference,
) {
    report.refs += 1;
    if reference.deleted == Some(true) {
        report.refs_deleted += 1;
        return;
    }

    let static_bounds_context = StaticBoundsContext {
        terrain: context.terrain,
        static_occluders: context.static_occluders,
        policy: context.policy,
        write: context.write,
    };
    let mesh_contact = resolve_ref_mesh_contact(
        report,
        reference,
        context.static_index,
        context.mesh_contacts,
    );
    if let MeshContactResolution::Resolved { contact, .. } = &mesh_contact {
        let _ = classify_contact(
            report,
            context.terrain,
            reference,
            contact,
            context.policy.contact_epsilon,
        );
    }
    let _ = classify_static_bounds_occlusion(
        report,
        StaticBoundsOcclusionInput {
            cell,
            key,
            reference,
            mesh_resolution: &mesh_contact,
            context: static_bounds_context,
        },
    );
    let _ = classify_origin(
        report,
        context.terrain,
        reference,
        context.policy.origin_epsilon,
    );
}

fn classify_origin(
    report: &mut TerrainInspectionReport,
    terrain: &TerrainIndex,
    reference: &tes3::esp::Reference,
    epsilon: f32,
) -> OriginDetails {
    let [x, y, z] = reference.translation;
    let Some(terrain_z) = terrain.height_at(x, y) else {
        report.refs_missing_terrain += 1;
        return OriginDetails {
            terrain_z: None,
            delta: None,
            classification: "origin_missing_terrain",
        };
    };

    report.refs_with_terrain += 1;
    let delta = z - terrain_z;
    let classification = classify_origin_delta(delta, epsilon);
    match classification {
        OriginTerrainClassification::Above => report.refs_above_terrain += 1,
        OriginTerrainClassification::Below => report.refs_below_terrain += 1,
        OriginTerrainClassification::OnTerrain => {}
    }

    OriginDetails {
        terrain_z: Some(terrain_z),
        delta: Some(delta),
        classification: classification.label(),
    }
}

struct ReferenceInspectionInput<'a, 'b> {
    cell: CellCoord,
    key: (u32, u32),
    reference: &'a tes3::esp::Reference,
    origin: &'a OriginDetails,
    mesh_resolution: &'a MeshContactResolution<'b>,
    contact_details: Option<&'a ContactDetails>,
    static_bounds_occlusion: Option<&'a StaticBoundsOcclusionDetails>,
    write: WriteStatusEvidence,
    contact_epsilon: f32,
    orientation_epsilon_degrees: f32,
}

fn reference_inspection(input: &ReferenceInspectionInput<'_, '_>) -> ReferenceInspection {
    ReferenceInspection {
        cell: [input.cell.0, input.cell.1],
        reference_key: [input.key.0, input.key.1],
        id: input.reference.id.clone(),
        origin: OriginInspection {
            position: input.reference.translation,
            terrain_z: input.origin.terrain_z,
            delta: input.origin.delta,
            classification: input.origin.classification,
        },
        static_resolution: input.mesh_resolution.static_resolution_label(),
        mesh_contact_status: input
            .mesh_resolution
            .mesh_contact_status_label(input.contact_details),
        deleted: input.reference.deleted == Some(true),
        write_status: write_status_label(&write_status_input(input)),
        static_mesh: input
            .mesh_resolution
            .static_mesh()
            .map(static_mesh_inspection),
        mesh_contact_error: input
            .mesh_resolution
            .mesh_contact_error()
            .map(str::to_owned),
        mesh_contact: input.contact_details.map(|contact| MeshContactInspection {
            position: contact.position,
            terrain_z: contact.terrain_z,
            terrain_normal: contact.terrain_normal,
            delta: contact.delta,
            orientation_angle_degrees: contact.orientation_angle_degrees,
            classification: contact.classification.map_or(
                "mesh_contact_missing_terrain",
                ContactTerrainClassification::label,
            ),
        }),
        static_bounds_occlusion: input.static_bounds_occlusion.map(|occlusion| {
            StaticBoundsOcclusionInspection {
                status: occlusion.status,
                ratio: occlusion.ratio,
                occluder_id: occlusion
                    .occluder
                    .as_ref()
                    .map(|occluder| occluder.id.clone()),
                occluder_cell: occlusion.occluder.as_ref().map(|occluder| occluder.cell),
                occluder_reference_key: occlusion
                    .occluder
                    .as_ref()
                    .map(|occluder| occluder.reference_key),
                target_bounds: occlusion.target_bounds.map(bounds_inspection),
                occluder_bounds: occlusion
                    .occluder
                    .as_ref()
                    .map(|occluder| bounds_inspection(occluder.bounds)),
                intersection_volume: occlusion.intersection_volume,
            }
        }),
    }
}

fn write_status_input(input: &ReferenceInspectionInput<'_, '_>) -> WriteStatusInput {
    WriteStatusInput {
        reference_deleted: input.reference.deleted == Some(true),
        mesh_status: input.mesh_resolution.write_status_mesh_status(),
        contact_delta: input.contact_details.and_then(|contact| contact.delta),
        orientation_angle_degrees: input
            .contact_details
            .and_then(|contact| contact.orientation_angle_degrees),
        static_bounds_status: input
            .static_bounds_occlusion
            .map(|occlusion| occlusion.status),
        write: input.write,
        contact_epsilon: input.contact_epsilon,
        orientation_epsilon_degrees: input.orientation_epsilon_degrees,
    }
}

fn deleted_reference_inspection(
    cell: CellCoord,
    key: (u32, u32),
    reference: &tes3::esp::Reference,
    write_status: &'static str,
) -> ReferenceInspection {
    ReferenceInspection {
        cell: [cell.0, cell.1],
        reference_key: [key.0, key.1],
        id: reference.id.clone(),
        static_resolution: "skipped_deleted_ref",
        mesh_contact_status: "skipped_deleted_ref",
        deleted: true,
        write_status,
        origin: OriginInspection {
            position: reference.translation,
            terrain_z: None,
            delta: None,
            classification: "skipped_deleted_ref",
        },
        static_mesh: None,
        mesh_contact_error: None,
        mesh_contact: None,
        static_bounds_occlusion: None,
    }
}

fn static_mesh_inspection(static_mesh: &StaticMesh) -> StaticMeshInspection {
    StaticMeshInspection {
        id: static_mesh.static_id.clone(),
        mesh: static_mesh.mesh_path.clone(),
    }
}

struct ContactDetails {
    position: [f32; 3],
    terrain_z: Option<f32>,
    terrain_normal: Option<[f32; 3]>,
    delta: Option<f32>,
    orientation_angle_degrees: Option<f32>,
    classification: Option<ContactTerrainClassification>,
}

struct StaticBoundsOcclusionDetails {
    status: &'static str,
    ratio: f32,
    occluder: Option<StaticOccluder>,
    target_bounds: Option<WorldAabb>,
    intersection_volume: Option<f32>,
}

#[derive(Clone, Copy)]
struct StaticBoundsOcclusionInput<'a, 'b> {
    cell: CellCoord,
    key: (u32, u32),
    reference: &'a tes3::esp::Reference,
    mesh_resolution: &'a MeshContactResolution<'b>,
    context: StaticBoundsContext<'a>,
}

#[derive(Clone, Copy)]
struct StaticBoundsContext<'a> {
    terrain: &'a TerrainIndex,
    static_occluders: &'a StaticOccluderIndex,
    policy: &'a UnclipPolicy,
    write: Option<&'a WriteStatusIndex>,
}

fn classify_static_bounds_occlusion(
    report: &mut TerrainInspectionReport,
    input: StaticBoundsOcclusionInput<'_, '_>,
) -> Option<StaticBoundsOcclusionDetails> {
    let MeshContactResolution::Resolved {
        contact, bounds, ..
    } = input.mesh_resolution
    else {
        return None;
    };
    if let Some(analysis) = input
        .context
        .write
        .and_then(|write| write.static_bounds_analysis(input.cell, input.key))
    {
        record_static_bounds_report_counts(report, analysis.status);
        return Some(StaticBoundsOcclusionDetails {
            status: analysis.status,
            ratio: analysis.ratio,
            occluder: analysis.occluder_id.as_ref().map(|id| StaticOccluder {
                id: id.clone(),
                cell: analysis.occluder_cell.unwrap_or([0, 0]),
                reference_key: analysis.occluder_reference_key.unwrap_or([0, 0]),
                bounds: analysis.occluder_bounds.unwrap_or(WorldAabb {
                    min: [0.0; 3],
                    max: [0.0; 3],
                }),
            }),
            target_bounds: analysis.target_bounds,
            intersection_volume: analysis.intersection_volume,
        });
    }
    let mut corrected_translation = input.reference.translation;
    let contact_position = contact.world_position(
        input.reference.translation,
        input.reference.rotation,
        input.reference.scale,
    );
    if let Some(terrain_z) = input
        .context
        .terrain
        .height_at(contact_position[0], contact_position[1])
    {
        corrected_translation[2] -= contact_position[2] - terrain_z;
    }
    let corrected_bounds = bounds.world_aabb(
        corrected_translation,
        input.reference.rotation,
        input.reference.scale,
    );
    let action = decide_static_bounds_action(corrected_bounds, input.context.static_occluders);
    let (status, ratio, occluder) = match action {
        StaticBoundsAction::None => ("static_bounds_clear", 0.0, None),
        StaticBoundsAction::Delete { ratio, occluder } => {
            record_static_bounds_report_counts(report, "static_bounds_fully_occluded");
            (
                "static_bounds_fully_occluded",
                ratio,
                Some(occluder.clone()),
            )
        }
        StaticBoundsAction::Move {
            ratio, occluder, ..
        } if find_valid_relocation_transform(
            input.cell,
            RefTransform {
                translation: corrected_translation,
                rotation: input.reference.rotation,
                scale: input.reference.scale,
            },
            contact,
            *bounds,
            input.context.terrain,
            input.context.static_occluders,
            input.context.policy.relocation,
        )
        .is_some() =>
        {
            record_static_bounds_report_counts(report, "static_bounds_relocatable");
            ("static_bounds_relocatable", ratio, Some(occluder.clone()))
        }
        StaticBoundsAction::Move {
            ratio, occluder, ..
        } => {
            record_static_bounds_report_counts(report, "static_bounds_blocked");
            ("static_bounds_blocked", ratio, Some(occluder.clone()))
        }
    };

    let intersection_volume = occluder
        .as_ref()
        .and_then(|occluder| corrected_bounds.intersection(occluder.bounds))
        .map(WorldAabb::volume);
    Some(StaticBoundsOcclusionDetails {
        status,
        ratio,
        occluder,
        target_bounds: Some(corrected_bounds),
        intersection_volume,
    })
}

fn record_static_bounds_report_counts(report: &mut TerrainInspectionReport, status: &str) {
    match status {
        "static_bounds_fully_occluded" => {
            report.refs_static_bounds_occluded += 1;
            report.refs_static_bounds_fully_occluded += 1;
        }
        "static_bounds_relocatable" => {
            report.refs_static_bounds_occluded += 1;
            report.refs_static_bounds_relocatable += 1;
        }
        "static_bounds_blocked" => {
            report.refs_static_bounds_occluded += 1;
            report.refs_static_bounds_blocked += 1;
        }
        "static_bounds_clear" => {}
        _ => unreachable!("unknown static bounds status"),
    }
}

fn bounds_inspection(bounds: WorldAabb) -> BoundsInspection {
    BoundsInspection {
        min: bounds.min,
        max: bounds.max,
    }
}

fn classify_contact(
    report: &mut TerrainInspectionReport,
    terrain: &TerrainIndex,
    reference: &tes3::esp::Reference,
    contact: &MeshContact,
    epsilon: f32,
) -> ContactDetails {
    let position =
        contact.world_position(reference.translation, reference.rotation, reference.scale);
    let terrain_sample = terrain.sample_at(position[0], position[1]);
    let terrain_z = terrain_sample.map(|sample| sample.height);
    let terrain_normal = terrain_sample.map(|sample| sample.normal);
    let delta = terrain_z.map(|terrain_z| position[2] - terrain_z);
    let classification = delta.map(|delta| classify_counted_contact_delta(report, delta, epsilon));
    if terrain_z.is_none() {
        report.refs_contact_missing_terrain += 1;
    }
    let orientation_angle_degrees =
        terrain_normal.and_then(|normal| orientation_angle_degrees(reference.rotation, normal));

    ContactDetails {
        position,
        terrain_z,
        terrain_normal,
        delta,
        orientation_angle_degrees,
        classification,
    }
}

fn classify_counted_contact_delta(
    report: &mut TerrainInspectionReport,
    delta: f32,
    epsilon: f32,
) -> ContactTerrainClassification {
    let classification = classify_contact_delta(delta, epsilon);
    match classification {
        ContactTerrainClassification::Above => {
            report.refs_contact_above_terrain += 1;
        }
        ContactTerrainClassification::Below => {
            report.refs_contact_below_terrain += 1;
        }
        ContactTerrainClassification::OnTerrain => {}
    }
    classification
}

enum MeshContactResolution<'a> {
    Resolved {
        static_mesh: &'a StaticMesh,
        contact: &'a MeshContact,
        bounds: super::mesh::MeshAabb,
    },
    MissingContact {
        static_mesh: &'a StaticMesh,
        error: String,
    },
    UnresolvedStatic,
}

impl<'a> MeshContactResolution<'a> {
    const fn static_resolution_label(&self) -> &'static str {
        match self {
            Self::Resolved { .. } | Self::MissingContact { .. } => "resolved",
            Self::UnresolvedStatic => "unresolved",
        }
    }

    fn mesh_contact_status_label(&self, contact_details: Option<&ContactDetails>) -> &'static str {
        match self {
            Self::Resolved { .. } => contact_details.map_or("missing_terrain", |details| {
                if details.terrain_z.is_some() {
                    "resolved"
                } else {
                    "missing_terrain"
                }
            }),
            Self::MissingContact { .. } => "missing_contact",
            Self::UnresolvedStatic => "unresolved_static",
        }
    }

    const fn static_mesh(&self) -> Option<&'a StaticMesh> {
        match self {
            Self::Resolved { static_mesh, .. } | Self::MissingContact { static_mesh, .. } => {
                Some(*static_mesh)
            }
            Self::UnresolvedStatic => None,
        }
    }

    fn mesh_contact_error(&self) -> Option<&str> {
        match self {
            Self::MissingContact { error, .. } => Some(error),
            Self::Resolved { .. } | Self::UnresolvedStatic => None,
        }
    }

    const fn write_status_mesh_status(&self) -> MeshResolutionStatus {
        match self {
            Self::Resolved { .. } => MeshResolutionStatus::Resolved,
            Self::MissingContact { .. } => MeshResolutionStatus::MissingContact,
            Self::UnresolvedStatic => MeshResolutionStatus::UnresolvedStatic,
        }
    }
}

fn resolve_ref_mesh_contact<'a>(
    report: &mut TerrainInspectionReport,
    reference: &tes3::esp::Reference,
    static_index: &'a StaticMeshIndex,
    mesh_contacts: &'a mut MeshCache<'_>,
) -> MeshContactResolution<'a> {
    let Some(static_mesh) = static_index.get(&reference.id) else {
        report.refs_without_resolved_static += 1;
        return MeshContactResolution::UnresolvedStatic;
    };

    match mesh_contacts.geometry(static_mesh) {
        Ok(geometry) => {
            report.refs_with_mesh_contact += 1;
            MeshContactResolution::Resolved {
                static_mesh,
                contact: &geometry.contact,
                bounds: geometry.bounds,
            }
        }
        Err(error) => {
            report.refs_missing_mesh_contact += 1;
            let error = error.to_string();
            MeshContactResolution::MissingContact { static_mesh, error }
        }
    }
}

#[derive(Clone, Copy)]
enum OriginTerrainClassification {
    Above,
    Below,
    OnTerrain,
}

impl OriginTerrainClassification {
    const fn label(self) -> &'static str {
        match self {
            Self::Above => "origin_above_terrain",
            Self::Below => "origin_below_terrain",
            Self::OnTerrain => "origin_on_terrain",
        }
    }
}

fn classify_origin_delta(delta: f32, epsilon: f32) -> OriginTerrainClassification {
    if delta > epsilon {
        OriginTerrainClassification::Above
    } else if delta < -epsilon {
        OriginTerrainClassification::Below
    } else {
        OriginTerrainClassification::OnTerrain
    }
}

#[derive(Clone, Copy)]
enum ContactTerrainClassification {
    Above,
    Below,
    OnTerrain,
}

impl ContactTerrainClassification {
    const fn label(self) -> &'static str {
        match self {
            Self::Above => "mesh_contact_above_terrain",
            Self::Below => "mesh_contact_below_terrain",
            Self::OnTerrain => "mesh_contact_on_terrain",
        }
    }
}

fn classify_contact_delta(delta: f32, epsilon: f32) -> ContactTerrainClassification {
    if delta > epsilon {
        ContactTerrainClassification::Above
    } else if delta < -epsilon {
        ContactTerrainClassification::Below
    } else {
        ContactTerrainClassification::OnTerrain
    }
}

#[cfg(test)]
mod tests {
    use tes3::esp::Reference;

    use super::{
        MeshContactResolution, StaticBoundsContext, StaticBoundsOcclusionInput,
        classify_static_bounds_occlusion, deleted_reference_inspection,
    };
    use crate::unclip::{
        args::{IdFilter, RelocationPolicy, UnclipPolicy, WriteActions},
        mesh::{MeshAabb, MeshContact, StaticMesh, WorldAabb},
        model::TerrainInspectionReport,
        occlusion::StaticOccluderIndex,
        terrain::TerrainIndex,
        write_plan::{WritePlan, WriteStaticBoundsAnalysis, WriteStatusIndex},
    };

    #[test]
    fn deleted_reference_inspection_skips_actionable_details() {
        let mut reference = reference_at_z(10.0);
        reference.deleted = Some(true);

        let inspection =
            deleted_reference_inspection((1, 2), (3, 4), &reference, "skipped_deleted_ref");

        assert!(inspection.deleted);
        assert_eq!(inspection.static_resolution, "skipped_deleted_ref");
        assert_eq!(inspection.mesh_contact_status, "skipped_deleted_ref");
        assert_eq!(inspection.write_status, "skipped_deleted_ref");
        assert!(inspection.static_mesh.is_none());
        assert!(inspection.mesh_contact.is_none());
    }

    #[test]
    fn static_bounds_classification_uses_relocatable_write_analysis() {
        let mut report = TerrainInspectionReport::default();
        let mesh = static_mesh();
        let contact = MeshContact::new(vec![[0.0; 3]]);
        let resolution = MeshContactResolution::Resolved {
            static_mesh: &mesh,
            contact: &contact,
            bounds: MeshAabb {
                min: [0.0; 3],
                max: [1.0; 3],
            },
        };
        let write = WriteStatusIndex::from_plan(&WritePlan {
            static_bounds_analysis: vec![static_bounds_analysis("static_bounds_relocatable")],
            ..WritePlan::default()
        });

        let terrain = TerrainIndex::from_landscapes(std::iter::empty());
        let static_occluders = StaticOccluderIndex::default();
        let policy = test_policy();
        let reference = reference_at_z(10.0);
        let details = classify_static_bounds_occlusion(
            &mut report,
            StaticBoundsOcclusionInput {
                cell: (1, 2),
                key: (3, 4),
                reference: &reference,
                mesh_resolution: &resolution,
                context: StaticBoundsContext {
                    terrain: &terrain,
                    static_occluders: &static_occluders,
                    policy: &policy,
                    write: Some(&write),
                },
            },
        )
        .expect("static bounds evidence should produce details");

        assert_eq!(details.status, "static_bounds_relocatable");
        assert_eq!(report.refs_static_bounds_occluded, 1);
        assert_eq!(report.refs_static_bounds_relocatable, 1);
        assert_eq!(report.refs_static_bounds_blocked, 0);
    }

    #[test]
    fn static_bounds_classification_uses_blocked_write_analysis() {
        let mut report = TerrainInspectionReport::default();
        let mesh = static_mesh();
        let contact = MeshContact::new(vec![[0.0; 3]]);
        let resolution = MeshContactResolution::Resolved {
            static_mesh: &mesh,
            contact: &contact,
            bounds: MeshAabb {
                min: [0.0; 3],
                max: [1.0; 3],
            },
        };
        let write = WriteStatusIndex::from_plan(&WritePlan {
            static_bounds_analysis: vec![static_bounds_analysis("static_bounds_blocked")],
            ..WritePlan::default()
        });

        let terrain = TerrainIndex::from_landscapes(std::iter::empty());
        let static_occluders = StaticOccluderIndex::default();
        let policy = test_policy();
        let reference = reference_at_z(10.0);
        let details = classify_static_bounds_occlusion(
            &mut report,
            StaticBoundsOcclusionInput {
                cell: (1, 2),
                key: (3, 4),
                reference: &reference,
                mesh_resolution: &resolution,
                context: StaticBoundsContext {
                    terrain: &terrain,
                    static_occluders: &static_occluders,
                    policy: &policy,
                    write: Some(&write),
                },
            },
        )
        .expect("static bounds evidence should produce details");

        assert_eq!(details.status, "static_bounds_blocked");
        assert_eq!(report.refs_static_bounds_occluded, 1);
        assert_eq!(report.refs_static_bounds_relocatable, 0);
        assert_eq!(report.refs_static_bounds_blocked, 1);
    }

    fn reference_at_z(z: f32) -> Reference {
        Reference {
            id: "grass".to_owned(),
            translation: [0.0, 0.0, z],
            ..Reference::default()
        }
    }

    fn static_mesh() -> StaticMesh {
        StaticMesh::new_for_test("grass", "grass.nif")
    }

    fn static_bounds_analysis(status: &'static str) -> WriteStaticBoundsAnalysis {
        WriteStaticBoundsAnalysis {
            cell: [1, 2],
            reference_key: [3, 4],
            status,
            ratio: 0.5,
            occluder_id: Some("rock".to_owned()),
            occluder_cell: Some([1, 2]),
            occluder_reference_key: Some([9, 8]),
            target_bounds: Some(WorldAabb {
                min: [0.0; 3],
                max: [2.0; 3],
            }),
            occluder_bounds: Some(WorldAabb {
                min: [1.0; 3],
                max: [3.0; 3],
            }),
            intersection_volume: Some(1.0),
        }
    }

    fn test_policy() -> UnclipPolicy {
        UnclipPolicy {
            write_actions: WriteActions::all(),
            contact_epsilon: 0.5,
            origin_epsilon: 0.5,
            orientation_epsilon_degrees: 1.0,
            relocation: RelocationPolicy {
                step: 32.0,
                steps: 8,
            },
            target_filter: IdFilter::new(&[], &[]).unwrap(),
            occluder_filter: IdFilter::new(&[], &[]).unwrap(),
        }
    }
}

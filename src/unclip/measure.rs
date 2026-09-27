// SPDX-License-Identifier: GPL-3.0-only

//! Geometric measurements of a groundcover reference against terrain, water, roads, and statics.
//!
//! Everything downstream (verdicts, reports, writes) derives from the values produced here, so the
//! report and the written plugin can never disagree about what a reference looks like in-game.

use glam::Vec3;

use super::{
    args::UnclipPolicy,
    mesh::{MeshGeometry, WorldAabb},
    occlusion::{StaticOccluder, StaticOccluderIndex},
    physics::RapierCollider,
    terrain::{TerrainIndex, TerrainSample, TerrainTextureIndex},
};

/// Exterior water plane height. `OpenMW` renders exterior water at `Z = 0` in every cell.
pub(crate) const EXTERIOR_WATER_LEVEL: f32 = 0.0;

/// Placement of a reference in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RefTransform {
    pub(crate) translation: [f32; 3],
    pub(crate) rotation: [f32; 3],
    pub(crate) scale: Option<f32>,
}

/// How the mesh base relates to the terrain surface under it.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub(crate) struct GroundContact {
    /// Height of the highest base vertex above the terrain directly beneath it.
    /// Positive means a visible gap; negative means that vertex is buried.
    pub(crate) gap: f32,
    /// Height of the lowest base vertex above the terrain beneath it (the most buried point).
    pub(crate) min_gap: f32,
    /// Number of base vertices that had terrain under them.
    pub(crate) base_points: usize,
}

/// Relationship between a reference's visible volume and the static occluders around it.
#[derive(Clone, Copy)]
pub(crate) enum Occlusion<'a> {
    Clear,
    /// The whole visible volume lies inside one occluder.
    Inside(&'a StaticOccluder),
    /// The visible volume overlaps an occluder.
    Intersects(&'a StaticOccluder),
}

/// Read-only view of the world a reference is measured against.
pub(crate) struct Surveyor<'a> {
    pub(crate) terrain: &'a TerrainIndex,
    pub(crate) textures: &'a TerrainTextureIndex,
    pub(crate) occluders: &'a StaticOccluderIndex,
    pub(crate) policy: &'a UnclipPolicy,
}

impl Surveyor<'_> {
    /// Terrain height, normal, and generator tilt under a world XY position.
    #[must_use]
    pub(crate) fn terrain_at(&self, x: f32, y: f32) -> Option<TerrainSample> {
        self.terrain.sample_at(x, y)
    }

    /// Terrain height alone under a world XY position; much cheaper than [`Self::terrain_at`].
    #[must_use]
    pub(crate) fn height_at(&self, x: f32, y: f32) -> Option<f32> {
        self.terrain.height_at(x, y)
    }

    /// Measures the mesh base against the terrain for a transform.
    ///
    /// Returns `None` when no base vertex has terrain under it.
    #[must_use]
    pub(crate) fn ground_contact(
        &self,
        geometry: &MeshGeometry,
        transform: &RefTransform,
    ) -> Option<GroundContact> {
        let offsets = geometry
            .contact
            .base_offsets(transform.rotation, transform.scale);
        let origin = Vec3::from(transform.translation);
        let mut gap = f32::NEG_INFINITY;
        let mut min_gap = f32::INFINITY;
        let mut base_points = 0;
        for offset in offsets.iter() {
            let point = origin + Vec3::from(*offset);
            let Some(height) = self.terrain.height_at(point.x, point.y) else {
                continue;
            };
            let point_gap = point.z - height;
            gap = gap.max(point_gap);
            min_gap = min_gap.min(point_gap);
            base_points += 1;
        }
        (base_points > 0).then_some(GroundContact {
            gap,
            min_gap,
            base_points,
        })
    }

    /// Road texture path under a world XY position when it matches the road filter.
    #[must_use]
    pub(crate) fn road_at(&self, x: f32, y: f32) -> Option<String> {
        self.textures
            .matches_road_at(x, y, &self.policy.road_texture_filter)
            .map(str::to_owned)
    }

    /// Whether the texture under a world XY position is a road, without building a path.
    #[must_use]
    pub(crate) fn is_road_at(&self, x: f32, y: f32) -> bool {
        self.textures
            .is_road_at(x, y, &self.policy.road_texture_filter)
    }

    /// Whether the terrain under a world XY position lies below the exterior water plane.
    #[must_use]
    pub(crate) fn submerged_ground(terrain: &TerrainSample) -> bool {
        terrain.height < EXTERIOR_WATER_LEVEL
    }

    /// Visible-volume collider for a reference transform.
    #[must_use]
    pub(crate) fn collider(geometry: &MeshGeometry, transform: &RefTransform) -> RapierCollider {
        RapierCollider::from_mesh_bounds(
            geometry.bounds,
            transform.translation,
            transform.rotation,
            transform.scale,
        )
    }

    /// Tests a reference's visible volume against nearby static occluders.
    #[must_use]
    pub(crate) fn occlusion(&self, collider: &RapierCollider) -> Occlusion<'_> {
        let candidates = self.occluders.candidates_for(collider.bounds());
        Self::occlusion_among(&candidates, collider)
    }

    /// Occluders whose bounds overlap a world region, for repeated tests inside that region.
    #[must_use]
    pub(crate) fn occluders_within(&self, bounds: WorldAabb) -> Vec<&StaticOccluder> {
        self.occluders.candidates_for(bounds)
    }

    /// Tests a collider against a fixed list of occluders.
    #[must_use]
    pub(crate) fn occlusion_among<'o>(
        candidates: &[&'o StaticOccluder],
        collider: &RapierCollider,
    ) -> Occlusion<'o> {
        // A volume inside a closed mesh also counts as intersecting it (its centre is inside),
        // so the cheap overlap test gates the eight-ray containment test.
        let mut intersecting = None;
        for &occluder in candidates {
            if !occluder.collider.intersects(collider) {
                continue;
            }
            if occluder.collider.contains(collider) {
                return Occlusion::Inside(occluder);
            }
            intersecting.get_or_insert(occluder);
        }
        intersecting.map_or(Occlusion::Clear, Occlusion::Intersects)
    }

    /// Whether any of the occluders overlaps the collider at all. Relocation probes only need
    /// this, never the containment distinction.
    #[must_use]
    pub(crate) fn blocked_by_any(
        candidates: &[&StaticOccluder],
        collider: &RapierCollider,
    ) -> bool {
        candidates
            .iter()
            .any(|occluder| occluder.collider.intersects(collider))
    }
}

#[cfg(test)]
#[allow(clippy::large_stack_arrays)]
mod tests {
    use crate::unclip::{
        args::UnclipPolicy,
        mesh::{MeshAabb, MeshContact, MeshGeometry},
        occlusion::{StaticOccluder, StaticOccluderIndex},
        physics::RapierCollider,
        terrain::{TerrainIndex, TerrainTextureIndex},
    };

    use super::{GroundContact, Occlusion, RefTransform, Surveyor};

    fn flat_terrain(height: f32) -> TerrainIndex {
        TerrainIndex::from_decoded_heights((0, 0), Box::new([[height; 65]; 65]))
    }

    fn grass_geometry() -> MeshGeometry {
        // A 20x20 cross-plane grass mesh 40 units tall with its base at local z = 0.
        let vertices = vec![
            [-10.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, -10.0, 0.0],
            [0.0, 10.0, 0.0],
            [-10.0, 0.0, 40.0],
            [10.0, 0.0, 40.0],
            [0.0, -10.0, 40.0],
            [0.0, 10.0, 40.0],
        ];
        let bounds = MeshAabb {
            min: [-10.0, -10.0, 0.0],
            max: [10.0, 10.0, 40.0],
        };
        MeshGeometry::new_for_test(MeshContact::new(vertices), bounds)
    }

    fn surveyor<'a>(
        terrain: &'a TerrainIndex,
        textures: &'a TerrainTextureIndex,
        occluders: &'a StaticOccluderIndex,
        policy: &'a UnclipPolicy,
    ) -> Surveyor<'a> {
        Surveyor {
            terrain,
            textures,
            occluders,
            policy,
        }
    }

    #[test]
    fn ground_contact_reports_gap_of_highest_base_vertex() {
        let terrain = flat_terrain(100.0);
        let textures = TerrainTextureIndex::from_parts(std::iter::empty(), std::iter::empty());
        let occluders = StaticOccluderIndex::default();
        let policy = UnclipPolicy::for_test();
        let surveyor = surveyor(&terrain, &textures, &occluders, &policy);
        let geometry = grass_geometry();

        let floating = surveyor
            .ground_contact(
                &geometry,
                &RefTransform {
                    translation: [1000.0, 1000.0, 103.0],
                    rotation: [0.0; 3],
                    scale: None,
                },
            )
            .unwrap();
        assert_eq!(
            floating,
            GroundContact {
                gap: 3.0,
                min_gap: 3.0,
                base_points: 4
            }
        );

        let buried = surveyor
            .ground_contact(
                &geometry,
                &RefTransform {
                    translation: [1000.0, 1000.0, 95.0],
                    rotation: [0.0; 3],
                    scale: Some(2.0),
                },
            )
            .unwrap();
        assert!((buried.gap + 5.0).abs() < 1e-4);
    }

    #[test]
    fn ground_contact_is_none_without_terrain() {
        let terrain = flat_terrain(0.0);
        let textures = TerrainTextureIndex::from_parts(std::iter::empty(), std::iter::empty());
        let occluders = StaticOccluderIndex::default();
        let policy = UnclipPolicy::for_test();
        let surveyor = surveyor(&terrain, &textures, &occluders, &policy);

        assert!(
            surveyor
                .ground_contact(
                    &grass_geometry(),
                    &RefTransform {
                        translation: [20000.0, 20000.0, 0.0],
                        rotation: [0.0; 3],
                        scale: None,
                    },
                )
                .is_none()
        );
    }

    #[test]
    fn occlusion_distinguishes_inside_from_intersecting() {
        let terrain = flat_terrain(0.0);
        let textures = TerrainTextureIndex::from_parts(std::iter::empty(), std::iter::empty());
        let policy = UnclipPolicy::for_test();
        let occluders = StaticOccluderIndex::new(vec![
            StaticOccluder::new_for_test(
                "rock_big",
                RapierCollider::from_world_aabb(crate::unclip::mesh::WorldAabb {
                    min: [0.0, 0.0, -10.0],
                    max: [200.0, 200.0, 200.0],
                }),
            ),
            StaticOccluder::new_for_test(
                "rock_edge",
                RapierCollider::from_world_aabb(crate::unclip::mesh::WorldAabb {
                    min: [500.0, 500.0, -10.0],
                    max: [505.0, 505.0, 200.0],
                }),
            ),
        ]);
        let surveyor = surveyor(&terrain, &textures, &occluders, &policy);
        let geometry = grass_geometry();

        let inside = Surveyor::collider(
            &geometry,
            &RefTransform {
                translation: [100.0, 100.0, 0.0],
                rotation: [0.0; 3],
                scale: None,
            },
        );
        assert!(matches!(
            surveyor.occlusion(&inside),
            Occlusion::Inside(occluder) if occluder.id == "rock_big"
        ));

        let touching = Surveyor::collider(
            &geometry,
            &RefTransform {
                translation: [495.0, 495.0, 0.0],
                rotation: [0.0; 3],
                scale: None,
            },
        );
        assert!(matches!(
            surveyor.occlusion(&touching),
            Occlusion::Intersects(occluder) if occluder.id == "rock_edge"
        ));

        let clear = Surveyor::collider(
            &geometry,
            &RefTransform {
                translation: [3000.0, 3000.0, 0.0],
                rotation: [0.0; 3],
                scale: None,
            },
        );
        assert!(matches!(surveyor.occlusion(&clear), Occlusion::Clear));
    }
}

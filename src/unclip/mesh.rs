// SPDX-License-Identifier: GPL-3.0-only

use std::{
    collections::{HashMap, HashSet, VecDeque},
    io,
    sync::{Arc, Mutex},
};

use glam::{Affine3A, Vec3};
use tes3::{
    esp::{ObjectFlags, Static},
    nif::{
        AvoidNode, NiAVObject, NiCollisionSwitch, NiFltAnimationNode, NiGeometryData, NiKey,
        NiLODNode, NiLink, NiNode, NiObjectNET, NiStream, NiStringExtraData, NiSwitchNode,
        NiTriShape, NiTriShapeData, NiTriStrips, NiTriStripsData, NiVisController,
        RootCollisionNode,
    },
};
use vfstool_lib::{VFS, VfsFile};

use super::transform::world_rotation;

#[derive(Clone, Debug)]
pub struct StaticMesh {
    pub mesh_path: String,
    mesh_key: String,
}

#[derive(Clone, Default)]
pub struct StaticMeshIndex {
    statics: HashMap<String, StaticMesh>,
}

impl StaticMeshIndex {
    #[must_use]
    pub fn from_statics<'a>(statics: impl IntoIterator<Item = &'a Static>) -> Self {
        let mut index = Self::default();

        for static_ in statics {
            let key = static_.id.to_lowercase();
            if static_.flags.contains(ObjectFlags::DELETED) || static_.mesh.is_empty() {
                index.statics.remove(&key);
            } else {
                index.statics.insert(
                    key,
                    StaticMesh {
                        mesh_path: static_.mesh.clone(),
                        mesh_key: normalize_mesh_key(&static_.mesh),
                    },
                );
            }
        }

        index
    }

    #[cfg(test)]
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&StaticMesh> {
        self.statics.get(&id.to_lowercase())
    }

    #[must_use]
    pub fn get_normalized_key(&self, key: &str) -> Option<&StaticMesh> {
        self.statics.get(key)
    }
}

#[derive(Debug)]
pub struct MeshContact {
    vertices: Vec<[f32; 3]>,
    transform_cache: Mutex<HashMap<ContactTransformKey, ContactTransform>>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ContactTransformKey {
    rotation: [u32; 3],
    scale: u32,
}

/// Rotated and scaled contact data for one `(rotation, scale)` pair.
#[derive(Clone, Debug)]
struct ContactTransform {
    /// The lowest rotated vertex.
    #[cfg_attr(not(test), allow(dead_code))]
    lowest: [f32; 3],
    /// Every rotated vertex within the base band of the lowest one.
    base: Arc<[[f32; 3]]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshAabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldAabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshGeometry {
    pub contact: MeshContact,
    pub bounds: MeshAabb,
    pub occluder_bounds: MeshAabb,
    pub(crate) occluder_parts: MeshColliderParts,
}

#[cfg(test)]
impl MeshGeometry {
    pub(crate) fn new_for_test(contact: MeshContact, bounds: MeshAabb) -> Self {
        Self {
            contact,
            bounds,
            occluder_bounds: bounds,
            occluder_parts: MeshColliderParts::from_mesh_aabb(bounds),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MeshColliderParts {
    parts: Vec<MeshColliderPart>,
    fallback: Option<ColliderPartsFallback>,
    source: MeshColliderSource,
}

/// One collision shape's triangle vertices in mesh-local space, with the accumulated node
/// transform (including any non-uniform scale) already applied. Exact duplicates are removed.
/// The occluder collider builds one convex hull per part.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MeshColliderPart {
    pub(crate) points: Vec<[f32; 3]>,
}

/// Test-only oriented box description, expanded to its eight corners when it becomes a part.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LocalObb {
    pub(crate) center: [f32; 3],
    pub(crate) half_extents: [f32; 3],
    pub(crate) orientation: glam::Quat,
}

/// Why collider parts were replaced by one aggregate box. Never used for meshes that
/// intentionally have no collision; see [`MeshColliderSource::NoCollision`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ColliderPartsFallback {
    Empty,
    OverBudget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MeshColliderSource {
    /// A non-empty `RootCollisionNode` selected the way `BulletNifLoader::handleRoot` does.
    Collision,
    /// No usable `RootCollisionNode`; `OpenMW` autogenerates collision from rendered geometry.
    VisibleFallback,
    /// `OpenMW` never lets actors collide with this mesh (`NC*` string extra data or an empty
    /// `RootCollisionNode`), so the collider is intentionally empty.
    NoCollision,
}

const MAX_COLLIDER_PARTS: usize = 256;

impl MeshContact {
    #[must_use]
    pub fn new(vertices: Vec<[f32; 3]>) -> Self {
        Self {
            vertices,
            transform_cache: Mutex::default(),
        }
    }

    #[cfg(test)]
    #[must_use]
    pub fn world_position(
        &self,
        translation: [f32; 3],
        rotation: [f32; 3],
        scale: Option<f32>,
    ) -> [f32; 3] {
        let offset = self.local_contact_offset(rotation, scale);
        [
            offset[0] + translation[0],
            offset[1] + translation[1],
            offset[2] + translation[2],
        ]
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn local_contact_offset(&self, rotation: [f32; 3], scale: Option<f32>) -> [f32; 3] {
        self.contact_transform(rotation, scale).lowest
    }

    /// Rotated and scaled local positions of the mesh's base vertices: every vertex whose
    /// rotated z lies within `band` of the lowest rotated z, where
    /// `band = max(1.0, 0.05 * rotated height)`. Cached per (rotation bits, scale bits) like
    /// [`Self::local_contact_offset`].
    #[must_use]
    pub(crate) fn base_offsets(&self, rotation: [f32; 3], scale: Option<f32>) -> Arc<[[f32; 3]]> {
        self.contact_transform(rotation, scale).base
    }

    #[cfg(test)]
    /// Unrotated local z extent of the contact vertices, for reporting.
    #[must_use]
    pub(crate) fn height(&self) -> f32 {
        let (min, max) = self
            .vertices
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), vertex| {
                (min.min(vertex[2]), max.max(vertex[2]))
            });
        if min.is_finite() && max.is_finite() {
            max - min
        } else {
            0.0
        }
    }

    fn contact_transform(&self, rotation: [f32; 3], scale: Option<f32>) -> ContactTransform {
        let scale = scale.unwrap_or(1.0);
        let key = ContactTransformKey {
            rotation: rotation.map(f32::to_bits),
            scale: scale.to_bits(),
        };

        if let Some(transform) = self
            .transform_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
        {
            return transform.clone();
        }

        let transform = self.contact_transform_uncached(rotation, scale);
        self.transform_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(key)
            .or_insert(transform)
            .clone()
    }

    #[cfg(test)]
    fn cached_transform_count(&self) -> usize {
        self.transform_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    fn contact_transform_uncached(&self, rotation: [f32; 3], scale: f32) -> ContactTransform {
        let rotation = world_rotation(rotation);
        let rotated: Vec<Vec3> = self
            .vertices
            .iter()
            .map(|vertex| rotation * (Vec3::from(*vertex) * scale))
            .collect();

        let Some(lowest) = rotated
            .iter()
            .copied()
            .min_by(|left, right| left.z.total_cmp(&right.z))
        else {
            return ContactTransform {
                lowest: [0.0; 3],
                base: Arc::from(Vec::new()),
            };
        };
        let highest_z = rotated
            .iter()
            .map(|position| position.z)
            .fold(f32::NEG_INFINITY, f32::max);
        let band = (0.05 * (highest_z - lowest.z)).max(1.0);
        let base: Vec<[f32; 3]> = rotated
            .iter()
            .filter(|position| position.z <= lowest.z + band)
            .map(Vec3::to_array)
            .collect();

        ContactTransform {
            lowest: lowest.to_array(),
            base: Arc::from(base),
        }
    }
}

impl Clone for MeshContact {
    fn clone(&self) -> Self {
        Self::new(self.vertices.clone())
    }
}

impl PartialEq for MeshContact {
    fn eq(&self, other: &Self) -> bool {
        self.vertices == other.vertices
    }
}

impl MeshAabb {
    #[cfg(test)]
    #[must_use]
    pub fn world_aabb(
        self,
        translation: [f32; 3],
        rotation: [f32; 3],
        scale: Option<f32>,
    ) -> WorldAabb {
        let rotation = world_rotation(rotation);
        let scale = scale.unwrap_or(1.0);
        let min = Vec3::from(self.min);
        let max = Vec3::from(self.max);
        let corners = [
            Vec3::new(min.x, min.y, min.z),
            Vec3::new(min.x, min.y, max.z),
            Vec3::new(min.x, max.y, min.z),
            Vec3::new(min.x, max.y, max.z),
            Vec3::new(max.x, min.y, min.z),
            Vec3::new(max.x, min.y, max.z),
            Vec3::new(max.x, max.y, min.z),
            Vec3::new(max.x, max.y, max.z),
        ];

        let mut world_min = Vec3::splat(f32::INFINITY);
        let mut world_max = Vec3::splat(f32::NEG_INFINITY);
        for corner in corners {
            let world = rotation * (corner * scale) + Vec3::from(translation);
            world_min = world_min.min(world);
            world_max = world_max.max(world);
        }

        WorldAabb {
            min: world_min.to_array(),
            max: world_max.to_array(),
        }
    }
}

impl MeshColliderParts {
    #[must_use]
    pub(crate) fn from_mesh_aabb(bounds: MeshAabb) -> Self {
        Self {
            parts: vec![MeshColliderPart {
                points: aabb_corners(bounds.min, bounds.max).to_vec(),
            }],
            fallback: None,
            source: MeshColliderSource::VisibleFallback,
        }
    }

    /// The intentionally empty collider of a mesh `OpenMW` never collides actors with.
    #[must_use]
    const fn no_collision() -> Self {
        Self {
            parts: Vec::new(),
            fallback: None,
            source: MeshColliderSource::NoCollision,
        }
    }

    #[must_use]
    fn aggregate_fallback(
        bounds: MeshAabb,
        fallback: ColliderPartsFallback,
        source: MeshColliderSource,
    ) -> Self {
        let mut parts = Self::from_mesh_aabb(bounds);
        parts.fallback = Some(fallback);
        parts.source = source;
        parts
    }

    #[cfg(test)]
    pub(crate) fn from_local_obbs(local_obbs: impl IntoIterator<Item = LocalObb>) -> Self {
        Self {
            parts: local_obbs
                .into_iter()
                .map(|obb| MeshColliderPart {
                    points: obb.corners().to_vec(),
                })
                .collect(),
            fallback: None,
            source: MeshColliderSource::VisibleFallback,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_points(parts: impl IntoIterator<Item = Vec<[f32; 3]>>) -> Self {
        Self {
            parts: parts
                .into_iter()
                .map(|points| MeshColliderPart { points })
                .collect(),
            fallback: None,
            source: MeshColliderSource::VisibleFallback,
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &MeshColliderPart> {
        self.parts.iter()
    }

    /// True when the mesh contributes no collider parts at all, which only happens for
    /// [`MeshColliderSource::NoCollision`].
    #[must_use]
    pub(crate) const fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    #[must_use]
    pub(crate) const fn fallback(&self) -> Option<ColliderPartsFallback> {
        self.fallback
    }

    #[must_use]
    pub(crate) const fn source(&self) -> MeshColliderSource {
        self.source
    }
}

#[cfg(test)]
impl MeshColliderPart {
    /// Axis-aligned bounds of the part's points in mesh-local space.
    #[must_use]
    pub(crate) fn local_bounds(&self) -> Option<MeshAabb> {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for point in &self.points {
            min = min.min(Vec3::from(*point));
            max = max.max(Vec3::from(*point));
        }
        (!self.points.is_empty()).then(|| MeshAabb {
            min: min.to_array(),
            max: max.to_array(),
        })
    }
}

/// The eight corners of an axis-aligned box.
#[must_use]
pub(crate) fn aabb_corners(min: [f32; 3], max: [f32; 3]) -> [[f32; 3]; 8] {
    [
        [min[0], min[1], min[2]],
        [min[0], min[1], max[2]],
        [min[0], max[1], min[2]],
        [min[0], max[1], max[2]],
        [max[0], min[1], min[2]],
        [max[0], min[1], max[2]],
        [max[0], max[1], min[2]],
        [max[0], max[1], max[2]],
    ]
}

#[cfg(test)]
impl LocalObb {
    #[must_use]
    pub(crate) fn from_mesh_aabb(bounds: MeshAabb) -> Self {
        let min = Vec3::from(bounds.min);
        let max = Vec3::from(bounds.max);
        let center = (min + max) * 0.5;
        let half = (max - min).abs() * 0.5;
        Self {
            center: center.to_array(),
            half_extents: half.to_array(),
            orientation: glam::Quat::IDENTITY,
        }
    }

    #[must_use]
    pub(crate) fn corners(self) -> [[f32; 3]; 8] {
        let half = Vec3::from(self.half_extents);
        aabb_corners((-half).to_array(), half.to_array()).map(|corner| {
            (self.orientation * Vec3::from(corner) + Vec3::from(self.center)).to_array()
        })
    }
}

impl WorldAabb {
    #[must_use]
    pub fn intersects_xy(self, other: Self) -> bool {
        self.min[0] < other.max[0]
            && self.max[0] > other.min[0]
            && self.min[1] < other.max[1]
            && self.max[1] > other.min[1]
    }

    #[must_use]
    pub fn intersection(self, other: Self) -> Option<Self> {
        let min = [
            self.min[0].max(other.min[0]),
            self.min[1].max(other.min[1]),
            self.min[2].max(other.min[2]),
        ];
        let max = [
            self.max[0].min(other.max[0]),
            self.max[1].min(other.max[1]),
            self.max[2].min(other.max[2]),
        ];

        (min[0] < max[0] && min[1] < max[1] && min[2] < max[2]).then_some(Self { min, max })
    }
}

pub struct MeshCache<'a> {
    vfs: &'a VFS,
    meshes: HashMap<String, CachedMesh>,
}

enum CachedMesh {
    BoundsOnly {
        stream: NiStream,
        bounds: MeshAabb,
        collider_parts: MeshColliderParts,
        geometry_error: Option<CachedMeshError>,
    },
    Loaded(MeshGeometry),
    Failed(CachedMeshError),
}

#[derive(Clone, Debug)]
struct CachedMeshError {
    kind: io::ErrorKind,
    message: String,
}

impl CachedMeshError {
    fn from_io(error: &io::Error) -> Self {
        Self {
            kind: error.kind(),
            message: error.to_string(),
        }
    }

    fn to_io(&self) -> io::Error {
        io::Error::new(self.kind, self.message.clone())
    }
}

impl<'a> MeshCache<'a> {
    #[must_use]
    pub fn new(vfs: &'a VFS) -> Self {
        Self {
            vfs,
            meshes: HashMap::new(),
        }
    }

    pub fn geometry(&mut self, static_mesh: &StaticMesh) -> io::Result<&MeshGeometry> {
        let key = &static_mesh.mesh_key;
        let cached = self.meshes.entry(key.clone()).or_insert_with(|| {
            load_geometry(self.vfs, &static_mesh.mesh_path).map_or_else(
                |error| CachedMesh::Failed(CachedMeshError::from_io(&error)),
                CachedMesh::Loaded,
            )
        });

        if let CachedMesh::BoundsOnly {
            geometry_error: Some(error),
            ..
        } = cached
        {
            return Err(error.to_io());
        }

        if let CachedMesh::BoundsOnly {
            stream,
            bounds,
            collider_parts,
            ..
        } = cached
        {
            let mesh = mesh_visible_geometry(stream, *bounds, collider_parts.clone())
                .ok_or_else(|| no_triangle_vertices_error(&static_mesh.mesh_path));
            match mesh {
                Ok(mesh) => {
                    *cached = CachedMesh::Loaded(mesh);
                }
                Err(error) => {
                    if let CachedMesh::BoundsOnly { geometry_error, .. } = cached {
                        *geometry_error = Some(CachedMeshError::from_io(&error));
                    }
                    return Err(error);
                }
            }
        }

        match cached {
            CachedMesh::BoundsOnly { .. } => unreachable!("bounds-only mesh should be promoted"),
            CachedMesh::Loaded(geometry) => Ok(geometry),
            CachedMesh::Failed(error) => Err(error.to_io()),
        }
    }

    pub fn bounds(&mut self, static_mesh: &StaticMesh) -> io::Result<MeshAabb> {
        let cached = self
            .meshes
            .entry(static_mesh.mesh_key.clone())
            .or_insert_with(|| {
                load_bounds(self.vfs, &static_mesh.mesh_path).map_or_else(
                    |error| CachedMesh::Failed(CachedMeshError::from_io(&error)),
                    |(stream, bounds)| CachedMesh::BoundsOnly {
                        collider_parts: mesh_collider_parts(&stream, bounds),
                        stream,
                        bounds,
                        geometry_error: None,
                    },
                )
            });

        match cached {
            CachedMesh::BoundsOnly { bounds, .. } => Ok(*bounds),
            CachedMesh::Loaded(geometry) => Ok(geometry.occluder_bounds),
            CachedMesh::Failed(error) => Err(error.to_io()),
        }
    }

    pub(crate) fn collider_parts(
        &mut self,
        static_mesh: &StaticMesh,
    ) -> io::Result<&MeshColliderParts> {
        let cached = self
            .meshes
            .entry(static_mesh.mesh_key.clone())
            .or_insert_with(|| {
                load_bounds(self.vfs, &static_mesh.mesh_path).map_or_else(
                    |error| CachedMesh::Failed(CachedMeshError::from_io(&error)),
                    |(stream, bounds)| CachedMesh::BoundsOnly {
                        collider_parts: mesh_collider_parts(&stream, bounds),
                        stream,
                        bounds,
                        geometry_error: None,
                    },
                )
            });

        match cached {
            CachedMesh::BoundsOnly { collider_parts, .. } => Ok(collider_parts),
            CachedMesh::Loaded(geometry) => Ok(&geometry.occluder_parts),
            CachedMesh::Failed(error) => Err(error.to_io()),
        }
    }

    #[cfg(test)]
    fn cached_mesh_count(&self) -> usize {
        self.meshes.len()
    }

    #[cfg(test)]
    fn cached_mesh_state(&self, static_mesh: &StaticMesh) -> Option<&'static str> {
        self.meshes
            .get(&static_mesh.mesh_key)
            .map(|mesh| match mesh {
                CachedMesh::BoundsOnly { .. } => "bounds_only",
                CachedMesh::Loaded(_) => "loaded",
                CachedMesh::Failed(_) => "failed",
            })
    }

    #[cfg(test)]
    fn set_cached_bounds_only_collision_data(
        &mut self,
        static_mesh: &StaticMesh,
        replacement_bounds: MeshAabb,
        replacement: MeshColliderParts,
    ) {
        if let Some(CachedMesh::BoundsOnly {
            bounds,
            collider_parts,
            ..
        }) = self.meshes.get_mut(&static_mesh.mesh_key)
        {
            *bounds = replacement_bounds;
            *collider_parts = replacement;
        }
    }
}

fn load_geometry(vfs: &VFS, mesh_path: &str) -> io::Result<MeshGeometry> {
    let stream = load_stream(vfs, mesh_path)?;

    mesh_geometry(&stream).ok_or_else(|| no_triangle_vertices_error(mesh_path))
}

fn load_bounds(vfs: &VFS, mesh_path: &str) -> io::Result<(NiStream, MeshAabb)> {
    let stream = load_stream(vfs, mesh_path)?;
    let bounds = mesh_bounds(&stream).ok_or_else(|| no_triangle_vertices_error(mesh_path))?;

    Ok((stream, bounds))
}

fn load_stream(vfs: &VFS, mesh_path: &str) -> io::Result<NiStream> {
    let file = resolve_mesh(vfs, mesh_path)?;
    let mut reader = file.open()?;
    let mut bytes = Vec::new();
    io::Read::read_to_end(&mut reader, &mut bytes)?;
    NiStream::from_bytes(&bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to load mesh {mesh_path}: {error}"),
        )
    })
}

fn no_triangle_vertices_error(mesh_path: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("mesh {mesh_path} has no triangle vertices"),
    )
}

pub(super) fn normalize_mesh_key(mesh_path: &str) -> String {
    strip_meshes_prefix(mesh_path)
        .replace('/', "\\")
        .to_lowercase()
}

fn resolve_mesh(vfs: &VFS, mesh_path: &str) -> io::Result<VfsFile> {
    let relative_mesh_path = strip_meshes_prefix(mesh_path);
    let backslash_key = format!("Meshes\\{}", relative_mesh_path.replace('/', "\\"));
    let slash_key = format!("Meshes/{}", relative_mesh_path.replace('\\', "/"));

    vfs.get_file(&backslash_key)
        .or_else(|| vfs.get_file(&slash_key))
        .cloned()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("mesh {mesh_path} was not found in the OpenMW VFS"),
            )
        })
}

fn strip_meshes_prefix(mesh_path: &str) -> &str {
    let Some((prefix, rest)) = mesh_path.split_once(['\\', '/']) else {
        return mesh_path;
    };

    if prefix.eq_ignore_ascii_case("meshes") {
        rest
    } else {
        mesh_path
    }
}

fn mesh_geometry(stream: &NiStream) -> Option<MeshGeometry> {
    let visible = mesh_visible(stream)?;
    let occluder_bounds = mesh_bounds(stream).unwrap_or(visible.bounds);

    Some(MeshGeometry {
        contact: MeshContact::new(visible.vertices),
        bounds: visible.bounds,
        occluder_bounds,
        occluder_parts: mesh_collider_parts(stream, occluder_bounds),
    })
}

fn mesh_visible_geometry(
    stream: &NiStream,
    occluder_bounds: MeshAabb,
    occluder_parts: MeshColliderParts,
) -> Option<MeshGeometry> {
    let visible = mesh_visible(stream)?;

    Some(MeshGeometry {
        contact: MeshContact::new(visible.vertices),
        bounds: visible.bounds,
        occluder_bounds,
        occluder_parts,
    })
}

struct VisibleMeshGeometry {
    vertices: Vec<[f32; 3]>,
    bounds: MeshAabb,
}

fn mesh_visible(stream: &NiStream) -> Option<VisibleMeshGeometry> {
    let (_, accumulated) = collect_mesh(stream, true, MeshSource::Visible);
    if !accumulated.has_vertices {
        return None;
    }
    let mut vertices = accumulated.vertices.unwrap_or_default();
    dedup_vertices_preserving_order(&mut vertices);
    let bounds = MeshAabb {
        min: accumulated.min.to_array(),
        max: accumulated.max.to_array(),
    };

    Some(VisibleMeshGeometry {
        vertices: vertices.iter().map(glam::Vec3::to_array).collect(),
        bounds,
    })
}

fn mesh_bounds(stream: &NiStream) -> Option<MeshAabb> {
    let (source, collision) = collect_mesh(stream, false, MeshSource::Collision);
    // A mesh actors never collide with still needs bounds for reporting and pruning. OpenMW
    // builds its camera-only shape from the rendered geometry in that case, so do the same.
    let accumulated = if source == MeshColliderSource::NoCollision {
        collect_mesh(stream, false, MeshSource::Visible).1
    } else {
        collision
    };
    if !accumulated.has_vertices {
        return None;
    }

    Some(MeshAabb {
        min: accumulated.min.to_array(),
        max: accumulated.max.to_array(),
    })
}

fn mesh_collider_parts(stream: &NiStream, bounds: MeshAabb) -> MeshColliderParts {
    let mut parts = Vec::new();
    let outcome = traverse_shapes(stream, MeshSource::Collision, &mut |shape, transform| {
        include_part(&shape, transform, &mut parts);
        (parts.len() > MAX_COLLIDER_PARTS).then_some(ColliderPartsFallback::OverBudget)
    });

    match (outcome.source, outcome.fallback) {
        (MeshColliderSource::NoCollision, _) => MeshColliderParts::no_collision(),
        (source, Some(fallback)) => MeshColliderParts::aggregate_fallback(bounds, fallback, source),
        (source, None) if parts.is_empty() => {
            MeshColliderParts::aggregate_fallback(bounds, ColliderPartsFallback::Empty, source)
        }
        (source, None) => MeshColliderParts {
            parts,
            fallback: None,
            source,
        },
    }
}

fn dedup_vertices_preserving_order(vertices: &mut Vec<Vec3>) {
    let mut seen = HashSet::new();
    vertices.retain(|vertex| {
        let key = [vertex.x.to_bits(), vertex.y.to_bits(), vertex.z.to_bits()];
        seen.insert(key)
    });
}

struct MeshAccumulator {
    vertices: Option<Vec<Vec3>>,
    min: Vec3,
    max: Vec3,
    has_vertices: bool,
}

impl MeshAccumulator {
    fn new(store_vertices: bool) -> Self {
        Self {
            vertices: store_vertices.then(Vec::new),
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
            has_vertices: false,
        }
    }

    fn include(&mut self, vertex: Vec3) {
        self.has_vertices = true;
        self.min = self.min.min(vertex);
        self.max = self.max.max(vertex);
        if let Some(vertices) = &mut self.vertices {
            vertices.push(vertex);
        }
    }
}

/// Which `OpenMW` loader's view of the scene graph a traversal reproduces.
#[derive(Clone, Copy, Eq, PartialEq)]
enum MeshSource {
    /// Rendered geometry, as `NifOsg::LoaderImpl::handleNode` would create it.
    Visible,
    /// Collision geometry, as `NifBullet::BulletNifLoader::handleNode` would collect it.
    Collision,
}

fn collect_mesh(
    stream: &NiStream,
    store_vertices: bool,
    source: MeshSource,
) -> (MeshColliderSource, MeshAccumulator) {
    let mut accumulated = MeshAccumulator::new(store_vertices);
    let outcome = traverse_shapes(stream, source, &mut |shape, transform| {
        for vertex in shape.iter() {
            accumulated.include(transform.transform_point3(vertex));
        }
        None
    });

    (outcome.source, accumulated)
}

/// Triangle vertex references of one `NiTriShape` or `NiTriStrips`.
struct ShapeVertices<'a> {
    data: &'a NiGeometryData,
    indices: ShapeIndices<'a>,
}

enum ShapeIndices<'a> {
    Triangles(&'a [[u16; 3]]),
    Strips(&'a [u16]),
}

impl ShapeVertices<'_> {
    fn iter(&self) -> impl Iterator<Item = Vec3> + '_ {
        let indices: Box<dyn Iterator<Item = u16> + '_> = match self.indices {
            ShapeIndices::Triangles(triangles) => Box::new(triangles.iter().flatten().copied()),
            ShapeIndices::Strips(strips) => Box::new(strips.iter().copied()),
        };
        indices.filter_map(|index| self.data.vertices.get(usize::from(index)).copied())
    }
}

fn shape_vertices(stream: &NiStream, link: NiLink<NiAVObject>) -> Option<ShapeVertices<'_>> {
    if let Some(shape) = stream.get_as::<_, NiTriShape>(link) {
        let data = stream.get_as::<_, NiTriShapeData>(shape.geometry_data)?;
        Some(ShapeVertices {
            data,
            indices: ShapeIndices::Triangles(&data.triangles),
        })
    } else if let Some(strips) = stream.get_as::<_, NiTriStrips>(link) {
        let data = stream.get_as::<_, NiTriStripsData>(strips.geometry_data)?;
        Some(ShapeVertices {
            data,
            indices: ShapeIndices::Strips(&data.strips),
        })
    } else {
        None
    }
}

/// Shape callback for [`traverse_shapes`]; returning `Some` stops the traversal.
type ShapeVisitor<'a> =
    dyn FnMut(ShapeVertices<'_>, Affine3A) -> Option<ColliderPartsFallback> + 'a;

struct TraversalOutcome {
    source: MeshColliderSource,
    fallback: Option<ColliderPartsFallback>,
}

/// What a NIF root tells `OpenMW` about its subtree. Derived exactly like
/// `NifBullet::BulletNifLoader::handleRoot` (pre-Gamebryo branch) and the root-node handling in
/// `NifOsg::LoaderImpl::handleNode`.
#[derive(Clone, Copy, Debug)]
struct RootPolicy {
    /// The `RootCollisionNode` chosen by `Nif::NiNode::findRootCollisionNode`, if any.
    collision_node: Option<NiKey>,
    /// `HandleNodeArgs::mHasTriMarkers` (bullet) / `HandleNodeArgs::mHasMarkers` (osg): the root
    /// carries a `MRK` string, so `Tri EditorMarker` shapes exist only for the editor.
    has_markers: bool,
    /// `HandleNodeArgs::mGenerateCollision`: no usable `RootCollisionNode`, so collision is
    /// autogenerated from rendered geometry.
    generate_collision: bool,
    /// `BulletShape::mVisualCollisionType != None`: actors never collide with this object.
    no_collision: bool,
}

fn root_policy(root: NiLink<NiAVObject>, stream: &NiStream) -> RootPolicy {
    let mut has_markers = false;
    let mut recursive_rcn = false;
    let mut no_collision = false;

    if let Some(object) = stream.get_as::<_, NiObjectNET>(root) {
        for extra in object.extra_datas_of_type::<NiStringExtraData>(stream) {
            // `BulletNifLoader::handleRoot`: `MRK` and `RCN` are exact matches while `NC` is a
            // case-insensitive prefix. `NCC` makes the shape camera-only and any other `NC*`
            // makes it visual-only (`PhysicsSystem::addObject`); actors collide with neither.
            if extra.value == "MRK" {
                has_markers = true;
            } else if starts_with_ignore_ascii_case(&extra.value, "NC") {
                no_collision = true;
            } else if extra.value == "RCN" {
                recursive_rcn = true;
            }
        }
    }

    let collision_node = stream
        .get_as::<_, NiNode>(root)
        .and_then(|node| find_root_collision_node(node, recursive_rcn, stream));
    // `BulletNifLoader::handleRoot`: an empty RootCollisionNode is treated like NCC. Collision is
    // then autogenerated from rendered geometry but only used for the camera.
    let empty_collision_node = collision_node.is_some_and(|key| {
        stream
            .get_as::<_, NiNode>(NiLink::<NiAVObject>::new(key))
            .is_none_or(|node| node.children.is_empty())
    });

    RootPolicy {
        collision_node,
        has_markers,
        generate_collision: collision_node.is_none() || empty_collision_node,
        no_collision: no_collision || empty_collision_node,
    }
}

/// `Nif::NiNode::findRootCollisionNode`: children are searched in reverse order, depth first,
/// and only descend into child nodes when the root carried the `RCN` string extra data.
fn find_root_collision_node(root: &NiNode, recursive: bool, stream: &NiStream) -> Option<NiKey> {
    let mut visited = HashSet::new();
    let mut stack = vec![root.children.iter().rev()];

    while let Some(children) = stack.last_mut() {
        let Some(child) = children.next() else {
            stack.pop();
            continue;
        };
        if child.is_null() {
            continue;
        }
        if stream.get_as::<_, RootCollisionNode>(*child).is_some() {
            return Some(child.key);
        }
        if recursive
            && let Some(node) = stream.get_as::<_, NiNode>(*child)
            && visited.insert(child.key)
        {
            stack.push(node.children.iter().rev());
        }
    }

    None
}

fn traverse_shapes(
    stream: &NiStream,
    source: MeshSource,
    on_shape: &mut ShapeVisitor<'_>,
) -> TraversalOutcome {
    let mut queue = VecDeque::new();
    let mut collider_source = MeshColliderSource::VisibleFallback;

    for root in &stream.roots {
        let root: NiLink<NiAVObject> = root.cast();
        let policy = root_policy(root, stream);
        if source == MeshSource::Collision {
            if policy.no_collision {
                return TraversalOutcome {
                    source: MeshColliderSource::NoCollision,
                    fallback: None,
                };
            }
            if !policy.generate_collision {
                collider_source = MeshColliderSource::Collision;
            }
        }
        queue.push_back(VisitItem {
            parent: None,
            link: root,
            transform: Affine3A::IDENTITY,
            generate: policy.generate_collision,
            avoid: false,
            policy,
        });
    }

    let mut visited = HashSet::new();
    while let Some(item) = queue.pop_front() {
        let fallback = match source {
            MeshSource::Visible => visit_visible(stream, item, &mut visited, &mut queue, on_shape),
            MeshSource::Collision => {
                visit_collision(stream, item, &mut visited, &mut queue, on_shape)
            }
        };
        if fallback.is_some() {
            return TraversalOutcome {
                source: collider_source,
                fallback,
            };
        }
    }

    TraversalOutcome {
        source: collider_source,
        fallback: None,
    }
}

#[derive(Clone, Copy)]
struct VisitItem {
    parent: Option<NiKey>,
    link: NiLink<NiAVObject>,
    transform: Affine3A,
    /// `HandleNodeArgs::mGenerateCollision` for this subtree.
    generate: bool,
    /// `HandleNodeArgs::mAvoid`: inside an `AvoidNode`.
    avoid: bool,
    policy: RootPolicy,
}

type VisitEdge = (Option<NiKey>, NiKey, bool);

/// `Nif::NiAVObject::Flag_ActiveCollision`, checked by `collisionActive()`.
const FLAG_ACTIVE_COLLISION: u16 = 0x0020;

/// Mirrors `NifOsg::LoaderImpl::handleNode` closely enough to know which geometry `OpenMW` renders.
fn visit_visible(
    stream: &NiStream,
    item: VisitItem,
    visited: &mut HashSet<VisitEdge>,
    queue: &mut VecDeque<VisitItem>,
    on_shape: &mut ShapeVisitor<'_>,
) -> Option<ColliderPartsFallback> {
    let VisitItem {
        parent,
        link,
        transform: parent_transform,
        policy,
        ..
    } = item;
    if link.is_null() || !visited.insert((parent, link.key, false)) {
        return None;
    }
    // `handleNode`: the selected collision node gets the hidden node mask and `mSkipMeshes` for
    // its whole subtree. Any other RootCollisionNode is rendered like a plain NiNode.
    if policy.collision_node == Some(link.key) {
        return None;
    }
    let object = stream.get_as::<_, NiAVObject>(link)?;
    // `handleNode`: hidden nodes skip their meshes unless a NiVisController may show them later.
    if object.app_culled()
        && object
            .controllers_of_type::<NiVisController>(stream)
            .next()
            .is_none()
    {
        return None;
    }

    let transform = parent_transform * object.transform();
    if let Some(shape) = shape_vertices(stream, link) {
        // `handleNode` (VER_MW branch): `Tri EditorMarker` shapes are skipped when the root has
        // `MRK`; `Shadow` and `Tri Shadow` shapes are never rendered.
        let name = &object.name;
        let skip = (policy.has_markers && starts_with_ignore_ascii_case(name, "tri editormarker"))
            || starts_with_ignore_ascii_case(name, "shadow")
            || starts_with_ignore_ascii_case(name, "tri shadow");
        if !skip && let Some(fallback) = on_shape(shape, transform) {
            return Some(fallback);
        }
    }

    for child in visible_children(stream, link) {
        queue.push_back(VisitItem {
            parent: Some(link.key),
            link: *child,
            transform,
            generate: false,
            avoid: false,
            policy,
        });
    }

    None
}

/// Children `OpenMW`'s renderer attaches: `osg::LOD` and `osg::Sequence` keep every child while
/// `osg::Switch` shows only `NiSwitchNode::mInitialIndex` (`handleSwitchNode`).
fn visible_children(stream: &NiStream, link: NiLink<NiAVObject>) -> &[NiLink<NiAVObject>] {
    if let Some(lod) = stream.get_as::<_, NiLODNode>(link) {
        &lod.children
    } else if let Some(sequence) = stream.get_as::<_, NiFltAnimationNode>(link) {
        &sequence.children
    } else if let Some(switch) = stream.get_as::<_, NiSwitchNode>(link) {
        switch
            .children
            .get(switch.active_index..=switch.active_index)
            .unwrap_or(&[])
    } else if let Some(node) = stream.get_as::<_, NiNode>(link) {
        &node.children
    } else {
        &[]
    }
}

/// Mirrors `NifBullet::BulletNifLoader::handleNode` and `handleGeometry`.
fn visit_collision(
    stream: &NiStream,
    item: VisitItem,
    visited: &mut HashSet<VisitEdge>,
    queue: &mut VecDeque<VisitItem>,
    on_shape: &mut ShapeVisitor<'_>,
) -> Option<ColliderPartsFallback> {
    let VisitItem {
        parent,
        link,
        transform: parent_transform,
        mut generate,
        mut avoid,
        policy,
    } = item;
    if link.is_null() || !visited.insert((parent, link.key, generate)) {
        return None;
    }
    // `handleNode`: an inactive NiCollisionSwitch hides its whole subtree from physics.
    if stream
        .get_as::<_, NiCollisionSwitch>(link)
        .is_some_and(|switch| switch.flags & FLAG_ACTIVE_COLLISION == 0)
    {
        return None;
    }
    // `handleNode`: only the RootCollisionNode picked by `handleRoot` turns generation on. An
    // "unexpected" or "extra" RootCollisionNode is treated as visible geometry, i.e. it simply
    // inherits whatever its parent was doing. The empty-RCN case never gets here because
    // `traverse_shapes` already reported `NoCollision`.
    if policy.collision_node == Some(link.key)
        && stream.get_as::<_, RootCollisionNode>(link).is_some()
    {
        generate = true;
    }
    // `handleNode`: AvoidNode geometry goes to the avoid shape, which actors never collide with.
    if stream.get_as::<_, AvoidNode>(link).is_some() {
        avoid = true;
    }
    // Note that the bullet loader never looks at `isHidden()`: app-culled shapes still collide.
    let object = stream.get_as::<_, NiAVObject>(link)?;

    let transform = parent_transform * object.transform();
    if generate
        && !avoid
        && let Some(shape) = shape_vertices(stream, link)
    {
        // `handleGeometry`: `Tri EditorMarker` shapes are skipped when the root has `MRK`. The
        // plain `EditorMarker` rule there depends on Gamebryo BSXFlags, which Morrowind-era NIFs
        // cannot carry.
        if !(policy.has_markers && starts_with_ignore_ascii_case(&object.name, "Tri EditorMarker"))
            && let Some(fallback) = on_shape(shape, transform)
        {
            return Some(fallback);
        }
    }

    for child in collision_children(stream, link) {
        queue.push_back(VisitItem {
            parent: Some(link.key),
            link: *child,
            transform,
            generate,
            avoid,
            policy,
        });
    }

    None
}

/// Children the bullet loader visits: `handleNode` breaks after the first child of a
/// `NiSwitchNode` or `NiFltAnimationNode` but keeps every child of a `NiLODNode`.
fn collision_children(stream: &NiStream, link: NiLink<NiAVObject>) -> &[NiLink<NiAVObject>] {
    if let Some(lod) = stream.get_as::<_, NiLODNode>(link) {
        &lod.children
    } else if let Some(switch) = stream.get_as::<_, NiSwitchNode>(link) {
        switch.children.get(..1).unwrap_or(&[])
    } else if let Some(node) = stream.get_as::<_, NiNode>(link) {
        &node.children
    } else {
        &[]
    }
}

/// `Misc::StringUtils::ciStartsWith`.
fn starts_with_ignore_ascii_case(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// Collects one collision shape's vertices in mesh-local space. Non-uniform node scales and
/// skews are fine: every vertex is transformed individually, so no decomposition is needed.
fn include_part(shape: &ShapeVertices<'_>, transform: Affine3A, parts: &mut Vec<MeshColliderPart>) {
    let mut points: Vec<Vec3> = shape
        .iter()
        .map(|vertex| transform.transform_point3(vertex))
        .collect();
    if points.is_empty() {
        return;
    }
    dedup_vertices_preserving_order(&mut points);
    parts.push(MeshColliderPart {
        points: points.iter().map(Vec3::to_array).collect(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use glam::Mat3;

    use tes3::nif::{
        NiExtraData, NiGeometry, NiObjectNET, NiTriBasedGeom, NiTriBasedGeomData, NiType,
    };

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn deleted_static_removes_previous_mesh_mapping() {
        let first = Static {
            id: "grass_a".to_owned(),
            mesh: "grass/a.nif".to_owned(),
            ..Static::default()
        };
        let deleted = Static {
            flags: ObjectFlags::DELETED,
            id: "GRASS_A".to_owned(),
            ..Static::default()
        };

        let index = StaticMeshIndex::from_statics([&first, &deleted]);

        assert!(index.get("grass_a").is_none());
    }

    #[test]
    fn mesh_contact_world_z_applies_reference_transform() {
        let contact = MeshContact::new(vec![[0.0, 0.0, -10.0]]);

        assert!(
            (contact.world_position([1.0, 2.0, 100.0], [0.0; 3], Some(2.0))[2] - 80.0).abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn mesh_contact_world_position_applies_openmw_axis_order() {
        let contact = MeshContact::new(vec![[0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);

        let position = contact.world_position(
            [10.0, 20.0, 30.0],
            [std::f32::consts::FRAC_PI_2, 0.0, 0.0],
            None,
        );

        assert_position_close(position, [10.0, 20.0, 29.0]);
    }

    #[test]
    fn mesh_contact_world_position_uses_cached_transform_for_repeated_calls() {
        let contact = test_contact();
        let rotation = [0.1, 0.2, 0.3];
        let scale = Some(1.5);

        let first = contact.world_position([10.0, 20.0, 30.0], rotation, scale);
        let second = contact.world_position([10.0, 20.0, 30.0], rotation, scale);

        assert_position_close(
            first,
            reference_world_position(&contact, [10.0, 20.0, 30.0], rotation, scale),
        );
        assert_position_close(second, first);
        assert_eq!(contact.cached_transform_count(), 1);
    }

    #[test]
    fn mesh_contact_world_position_reuses_cached_offset_for_different_translations() {
        let contact = test_contact();
        let rotation = [0.1, 0.2, 0.3];
        let scale = Some(1.5);

        let first = contact.world_position([10.0, 20.0, 30.0], rotation, scale);
        let second = contact.world_position([-5.0, 12.0, 80.0], rotation, scale);

        assert_position_close(
            first,
            reference_world_position(&contact, [10.0, 20.0, 30.0], rotation, scale),
        );
        assert_position_close(
            second,
            reference_world_position(&contact, [-5.0, 12.0, 80.0], rotation, scale),
        );
        assert_eq!(contact.cached_transform_count(), 1);
    }

    #[test]
    fn mesh_contact_world_position_caches_different_rotation_and_scale_separately() {
        let contact = test_contact();

        let first = contact.world_position([10.0, 20.0, 30.0], [0.1, 0.2, 0.3], Some(1.5));
        let second = contact.world_position([10.0, 20.0, 30.0], [0.3, 0.2, 0.1], Some(0.75));

        assert_position_close(
            first,
            reference_world_position(&contact, [10.0, 20.0, 30.0], [0.1, 0.2, 0.3], Some(1.5)),
        );
        assert_position_close(
            second,
            reference_world_position(&contact, [10.0, 20.0, 30.0], [0.3, 0.2, 0.1], Some(0.75)),
        );
        assert_eq!(contact.cached_transform_count(), 2);
    }

    #[test]
    fn mesh_contact_world_position_normalizes_missing_scale_to_one() {
        let contact = test_contact();
        let rotation = [0.1, 0.2, 0.3];

        let implicit = contact.world_position([10.0, 20.0, 30.0], rotation, None);
        let explicit = contact.world_position([10.0, 20.0, 30.0], rotation, Some(1.0));

        assert_position_close(implicit, explicit);
        assert_position_close(
            implicit,
            reference_world_position(&contact, [10.0, 20.0, 30.0], rotation, None),
        );
        assert_eq!(contact.cached_transform_count(), 1);
    }

    #[test]
    fn mesh_contact_vertices_deduplicate_exact_duplicates_without_reordering() {
        let mut vertices = vec![
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 2.0, 3.0),
        ];

        dedup_vertices_preserving_order(&mut vertices);

        assert_eq!(
            vertices,
            vec![Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 0.0, 0.0)]
        );
    }

    #[test]
    fn mesh_cache_key_matches_optional_meshes_prefix() {
        assert_eq!(
            normalize_mesh_key("Meshes/Grass/Foo.nif"),
            normalize_mesh_key("grass\\foo.nif")
        );
    }

    #[test]
    fn mesh_cache_bounds_then_geometry_promotes_cached_stream() {
        let temp_dir = TempDir::new("bounds-then-geometry");
        write_visible_and_collision_nif(&temp_dir.path().join("Meshes/Grass/Foo.nif"));
        let vfs = VFS::from_directories(vec![temp_dir.path().to_path_buf()], None);
        let static_mesh = test_static_mesh("Meshes/Grass/Foo.nif");
        let mut cache = MeshCache::new(&vfs);

        assert_eq!(cache.bounds(&static_mesh).unwrap(), collision_bounds());
        assert_eq!(cache.cached_mesh_state(&static_mesh), Some("bounds_only"));
        let cached_parts =
            MeshColliderParts::from_local_obbs([LocalObb::from_mesh_aabb(far_collision_bounds())]);
        cache.set_cached_bounds_only_collision_data(
            &static_mesh,
            far_collision_bounds(),
            cached_parts.clone(),
        );

        std::fs::remove_file(temp_dir.path().join("Meshes/Grass/Foo.nif")).unwrap();

        {
            let cached_geometry = cache.geometry(&static_mesh).unwrap();
            assert_eq!(cached_geometry.bounds, expected_bounds());
            assert_eq!(cached_geometry.occluder_bounds, far_collision_bounds());
            assert_eq!(cached_geometry.occluder_parts, cached_parts);
            assert_eq!(
                cached_geometry.contact.vertices,
                expected_contact_vertices()
            );
        }

        assert_eq!(cache.cached_mesh_count(), 1);
        assert_eq!(cache.cached_mesh_state(&static_mesh), Some("loaded"));
    }

    #[test]
    fn mesh_cache_bounds_does_not_load_contact_geometry() {
        let temp_dir = TempDir::new("bounds-only");
        write_nif(&temp_dir.path().join("Meshes/Grass/Foo.nif"));
        let vfs = VFS::from_directories(vec![temp_dir.path().to_path_buf()], None);
        let static_mesh = test_static_mesh("Meshes/Grass/Foo.nif");
        let mut cache = MeshCache::new(&vfs);

        assert_eq!(cache.bounds(&static_mesh).unwrap(), expected_bounds());

        assert_eq!(cache.cached_mesh_state(&static_mesh), Some("bounds_only"));
    }

    #[test]
    fn mesh_cache_recreates_cached_errors_consistently() {
        let vfs = empty_vfs();
        let static_mesh = test_static_mesh("Meshes/Grass/Missing.nif");
        let mut cache = MeshCache::new(&vfs);

        let first = cache.bounds(&static_mesh).unwrap_err();
        let second = cache.bounds(&static_mesh).unwrap_err();
        let third = cache.geometry(&static_mesh).unwrap_err();

        assert_eq!(first.kind(), io::ErrorKind::NotFound);
        assert_eq!(second.kind(), first.kind());
        assert_eq!(third.kind(), first.kind());
        assert_eq!(second.to_string(), first.to_string());
        assert_eq!(third.to_string(), first.to_string());
        assert_eq!(cache.cached_mesh_count(), 1);
    }

    #[test]
    fn mesh_cache_keeps_collision_bounds_after_geometry_promotion_failure() {
        let temp_dir = TempDir::new("collision-only");
        write_collision_only_nif(&temp_dir.path().join("Meshes/Grass/Foo.nif"));
        let vfs = VFS::from_directories(vec![temp_dir.path().to_path_buf()], None);
        let static_mesh = test_static_mesh("Meshes/Grass/Foo.nif");
        let mut cache = MeshCache::new(&vfs);

        assert_eq!(cache.bounds(&static_mesh).unwrap(), collision_bounds());
        let error = cache.geometry(&static_mesh).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(cache.bounds(&static_mesh).unwrap(), collision_bounds());
        assert_eq!(cache.cached_mesh_state(&static_mesh), Some("bounds_only"));
    }

    #[test]
    fn visible_shape_named_root_collision_node_is_not_dropped() {
        let mut stream = NiStream::new();
        let shape = insert_shape(
            &mut stream,
            "RootCollisionNode",
            &expected_contact_vertices(),
            0,
        );
        push_root(&mut stream, shape);

        let geometry = mesh_geometry(&stream).unwrap();

        assert_eq!(geometry.bounds, expected_bounds());
        assert_eq!(geometry.contact.vertices, expected_contact_vertices());
    }

    #[test]
    fn occluder_bounds_prefer_actual_root_collision_node_geometry() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let collision_shape = insert_shape(&mut stream, "collision", &collision_vertices(), 0);
        let collision_root = insert_root_collision_node(&mut stream, vec![collision_shape]);
        let root = insert_node(&mut stream, "root", vec![visible, collision_root], None, 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();

        assert_eq!(geometry.bounds, expected_bounds());
        assert_eq!(geometry.occluder_bounds, collision_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), collision_bounds());
        assert_eq!(geometry.contact.vertices, expected_contact_vertices());
    }

    #[test]
    fn collider_parts_prefer_collision_geometry() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let collision_shape = insert_shape(&mut stream, "collision", &collision_vertices(), 0);
        let collision_root = insert_root_collision_node(&mut stream, vec![collision_shape]);
        let root = insert_node(&mut stream, "root", vec![visible, collision_root], None, 0);
        push_root(&mut stream, root);

        let parts = mesh_collider_parts(&stream, mesh_bounds(&stream).unwrap());

        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.source(), MeshColliderSource::Collision);
        assert_eq!(parts.parts.len(), 1);
        assert_part_bounds_close(&parts.parts[0], collision_bounds());
    }

    #[test]
    fn collider_parts_fall_back_to_visible_geometry_without_collision() {
        let mut stream = NiStream::new();
        let shape = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        push_root(&mut stream, shape);

        let parts = mesh_collider_parts(&stream, mesh_bounds(&stream).unwrap());

        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.source(), MeshColliderSource::VisibleFallback);
        assert_eq!(parts.parts.len(), 1);
        assert_part_bounds_close(&parts.parts[0], expected_bounds());
    }

    #[test]
    fn collider_parts_preserve_shape_translation_rotation_and_scale() {
        let mut stream = NiStream::new();
        let shape = insert_shape(&mut stream, "rotated", &box_vertices(), 0);
        set_transform(
            &mut stream,
            shape,
            Vec3::new(10.0, 20.0, 30.0),
            Mat3::from_rotation_z(std::f32::consts::FRAC_PI_2),
            2.0,
        );
        push_root(&mut stream, shape);

        let parts = mesh_collider_parts(&stream, mesh_bounds(&stream).unwrap());

        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.parts.len(), 1);
        // Half extents [2, 4, 6] rotated a quarter turn about z swap x and y.
        assert_part_bounds_close(
            &parts.parts[0],
            MeshAabb {
                min: [6.0, 18.0, 24.0],
                max: [14.0, 22.0, 36.0],
            },
        );
        assert_eq!(parts.parts[0].points.len(), box_vertices().len());
    }

    #[test]
    fn collider_parts_apply_non_uniform_node_scale_per_vertex() {
        let mut stream = NiStream::new();
        let shape = insert_shape(&mut stream, "cube", &unit_cube_vertices(), 0);
        let node = insert_node(&mut stream, "scaled", vec![shape], None, 0);
        set_transform(
            &mut stream,
            node,
            Vec3::ZERO,
            Mat3::from_diagonal(Vec3::new(2.0, 1.0, 1.0)),
            1.0,
        );
        push_root(&mut stream, node);

        let parts = mesh_collider_parts(&stream, mesh_bounds(&stream).unwrap());

        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.parts.len(), 1);
        assert_eq!(parts.parts[0].points.len(), 8);
        assert_part_bounds_close(
            &parts.parts[0],
            MeshAabb {
                min: [-2.0, -1.0, -1.0],
                max: [2.0, 1.0, 1.0],
            },
        );
    }

    #[test]
    fn collider_parts_dedup_exact_duplicate_vertices() {
        let mut stream = NiStream::new();
        let shape = insert_shape(&mut stream, "cube", &unit_cube_vertices(), 0);
        push_root(&mut stream, shape);

        let parts = mesh_collider_parts(&stream, mesh_bounds(&stream).unwrap());

        // Every triangle corner references one of only eight distinct positions.
        assert_eq!(parts.parts[0].points.len(), 8);
    }

    #[test]
    fn collider_parts_over_budget_falls_back_to_aggregate_aabb() {
        let mut stream = NiStream::new();
        let children = (0_u16..=u16::try_from(MAX_COLLIDER_PARTS).unwrap())
            .map(|index| {
                let shape = insert_shape(&mut stream, "part", &box_vertices(), 0);
                set_transform(
                    &mut stream,
                    shape,
                    Vec3::new(f32::from(index) * 10.0, 0.0, 0.0),
                    Mat3::IDENTITY,
                    1.0,
                );
                shape
            })
            .collect::<Vec<_>>();
        let root = insert_node(&mut stream, "root", children, None, 0);
        push_root(&mut stream, root);
        let bounds = mesh_bounds(&stream).unwrap();

        let parts = mesh_collider_parts(&stream, bounds);

        assert_eq!(parts.fallback(), Some(ColliderPartsFallback::OverBudget));
        assert_eq!(parts.parts.len(), 1);
        assert_part_bounds_close(&parts.parts[0], bounds);
    }

    #[test]
    fn root_rcn_extra_without_actual_root_collision_node_uses_visible_fallback() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let collision_shape = insert_shape(&mut stream, "collision", &collision_vertices(), 1);
        let not_collision_root = insert_node(
            &mut stream,
            "whatever-blender-called-it",
            vec![collision_shape],
            None,
            0,
        );
        let root = insert_node(
            &mut stream,
            "root",
            vec![visible, not_collision_root],
            Some("RCN"),
            0,
        );
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        assert_eq!(geometry.bounds, expected_bounds());
        assert_eq!(geometry.occluder_bounds, collision_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), collision_bounds());
        assert_eq!(parts.source(), MeshColliderSource::VisibleFallback);
        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.parts.len(), 2);
    }

    #[test]
    fn non_root_shape_rcn_extra_does_not_mark_shape_as_collision_geometry() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        set_extra_data(&mut stream, visible, "RCN");
        let root = insert_node(&mut stream, "root", vec![visible], None, 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        assert_eq!(geometry.bounds, expected_bounds());
        assert_eq!(geometry.occluder_bounds, expected_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), expected_bounds());
        assert_eq!(parts.source(), MeshColliderSource::VisibleFallback);
        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.parts.len(), 1);
        assert_part_bounds_close(&parts.parts[0], expected_bounds());
    }

    #[test]
    fn root_rcn_extra_enables_nested_actual_root_collision_node_discovery() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let collision_shape = insert_shape(&mut stream, "collision", &collision_vertices(), 0);
        let collision_root = insert_root_collision_node(&mut stream, vec![collision_shape]);
        let wrapper = insert_node(&mut stream, "wrapper", vec![collision_root], None, 0);
        let root = insert_node(&mut stream, "root", vec![visible, wrapper], Some("RCN"), 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        assert_eq!(geometry.bounds, expected_bounds());
        assert_eq!(geometry.occluder_bounds, collision_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), collision_bounds());
        assert_eq!(parts.source(), MeshColliderSource::Collision);
        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.parts.len(), 1);
        assert_part_bounds_close(&parts.parts[0], collision_bounds());
    }

    #[test]
    fn nested_root_collision_node_without_root_rcn_extra_uses_visible_fallback() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let collision_shape = insert_shape(&mut stream, "collision", &collision_vertices(), 0);
        let collision_root = insert_root_collision_node(&mut stream, vec![collision_shape]);
        let wrapper = insert_node(&mut stream, "wrapper", vec![collision_root], None, 0);
        let root = insert_node(&mut stream, "root", vec![visible, wrapper], None, 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        // OpenMW only hides the RootCollisionNode it selected; an unselected one is rendered.
        assert_eq!(geometry.bounds, collision_bounds());
        assert_eq!(geometry.occluder_bounds, collision_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), collision_bounds());
        assert_eq!(parts.source(), MeshColliderSource::VisibleFallback);
        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.parts.len(), 2);
    }

    #[test]
    fn non_root_rcn_extra_does_not_enable_nested_root_collision_node_discovery() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let collision_shape = insert_shape(&mut stream, "collision", &collision_vertices(), 0);
        let collision_root = insert_root_collision_node(&mut stream, vec![collision_shape]);
        let wrapper = insert_node(&mut stream, "wrapper", vec![collision_root], Some("RCN"), 0);
        let root = insert_node(&mut stream, "root", vec![visible, wrapper], None, 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        // OpenMW only hides the RootCollisionNode it selected; an unselected one is rendered.
        assert_eq!(geometry.bounds, collision_bounds());
        assert_eq!(geometry.occluder_bounds, collision_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), collision_bounds());
        assert_eq!(parts.source(), MeshColliderSource::VisibleFallback);
        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.parts.len(), 2);
    }

    #[test]
    fn root_rcn_extra_search_is_reversed_and_depth_first() {
        // `NiNode::findRootCollisionNode` walks children in reverse and, with RCN, descends
        // into a child node before looking at earlier siblings, so the deep RCN wins here.
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let immediate_shape = insert_shape(&mut stream, "immediate", &collision_vertices(), 0);
        let immediate_collision_root =
            insert_root_collision_node(&mut stream, vec![immediate_shape]);
        let deep_shape = insert_shape(&mut stream, "deep", &far_collision_vertices(), 0);
        let deep_collision_root = insert_root_collision_node(&mut stream, vec![deep_shape]);
        let wrapper = insert_node(&mut stream, "wrapper", vec![deep_collision_root], None, 0);
        let root = insert_node(
            &mut stream,
            "root",
            vec![visible, immediate_collision_root, wrapper],
            Some("RCN"),
            0,
        );
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        // The unselected immediate RootCollisionNode is rendered like a plain node.
        assert_eq!(geometry.bounds, collision_bounds());
        assert_eq!(geometry.occluder_bounds, deep_collision_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), deep_collision_bounds());
        assert_eq!(parts.source(), MeshColliderSource::Collision);
        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.parts.len(), 1);
        assert_part_bounds_close(&parts.parts[0], deep_collision_bounds());
    }

    #[test]
    fn root_rcn_extra_deep_search_takes_last_nested_root_collision_node_only() {
        // Reverse child order means the later wrapper is searched (and its RCN found) first.
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let first_shape = insert_shape(&mut stream, "first", &collision_vertices(), 0);
        let first_collision_root = insert_root_collision_node(&mut stream, vec![first_shape]);
        let first_wrapper = insert_node(
            &mut stream,
            "first_wrapper",
            vec![first_collision_root],
            None,
            0,
        );
        let later_shape = insert_shape(&mut stream, "later", &far_collision_vertices(), 0);
        let later_collision_root = insert_root_collision_node(&mut stream, vec![later_shape]);
        let later_wrapper = insert_node(
            &mut stream,
            "later_wrapper",
            vec![later_collision_root],
            None,
            0,
        );
        let root = insert_node(
            &mut stream,
            "root",
            vec![visible, first_wrapper, later_wrapper],
            Some("RCN"),
            0,
        );
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        // The unselected first RootCollisionNode is rendered like a plain node.
        assert_eq!(geometry.bounds, collision_bounds());
        assert_eq!(geometry.occluder_bounds, deep_collision_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), deep_collision_bounds());
        assert_eq!(parts.source(), MeshColliderSource::Collision);
        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.parts.len(), 1);
        assert_part_bounds_close(&parts.parts[0], deep_collision_bounds());
    }

    #[test]
    fn no_valid_rcn_uses_whole_scene_graph_for_collision_fallback() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let second_visible = insert_shape(&mut stream, "second_visible", &collision_vertices(), 0);
        let app_culled = insert_shape(&mut stream, "app_culled", &far_collision_vertices(), 1);
        let root = insert_node(
            &mut stream,
            "root",
            vec![visible, second_visible, app_culled],
            None,
            0,
        );
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        assert_eq!(geometry.bounds, collision_bounds());
        assert_eq!(geometry.occluder_bounds, far_collision_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), far_collision_bounds());
        assert_eq!(parts.source(), MeshColliderSource::VisibleFallback);
        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.parts.len(), 3);
    }

    #[test]
    fn shared_shape_can_be_visible_and_collision_geometry() {
        let mut stream = NiStream::new();
        let shared_shape = insert_shape(&mut stream, "shared", &collision_vertices(), 0);
        let collision_root = insert_root_collision_node(&mut stream, vec![shared_shape]);
        let root = insert_node(
            &mut stream,
            "root",
            vec![shared_shape, collision_root],
            None,
            0,
        );
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();

        assert_eq!(geometry.bounds, collision_bounds());
        assert_eq!(geometry.occluder_bounds, collision_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), collision_bounds());
    }

    #[test]
    fn app_culled_non_root_rcn_extra_subtree_is_included_in_scene_graph_fallback() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let collision_shape = insert_shape(&mut stream, "collision", &collision_vertices(), 0);
        let not_collision_root = insert_node(
            &mut stream,
            "collision",
            vec![collision_shape],
            Some("RCN"),
            1,
        );
        let root = insert_node(
            &mut stream,
            "root",
            vec![visible, not_collision_root],
            None,
            0,
        );
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();

        assert_eq!(geometry.bounds, expected_bounds());
        assert_eq!(geometry.occluder_bounds, collision_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), collision_bounds());
    }

    #[test]
    fn app_culled_root_collision_node_is_included_for_collision_extraction() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let collision_shape = insert_shape(&mut stream, "collision", &collision_vertices(), 0);
        let collision_root =
            insert_root_collision_node_with_flags(&mut stream, vec![collision_shape], 1);
        let root = insert_node(&mut stream, "root", vec![visible, collision_root], None, 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        assert_eq!(geometry.bounds, expected_bounds());
        assert_eq!(geometry.contact.vertices, expected_contact_vertices());
        assert_eq!(geometry.occluder_bounds, collision_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), collision_bounds());
        assert_eq!(parts.source(), MeshColliderSource::Collision);
        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.parts.len(), 1);
        assert_part_bounds_close(&parts.parts[0], collision_bounds());
    }

    #[test]
    fn app_culled_root_collision_node_descendant_shape_is_included_for_collision_extraction() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let collision_shape = insert_shape(&mut stream, "collision", &collision_vertices(), 1);
        let collision_root = insert_root_collision_node(&mut stream, vec![collision_shape]);
        let root = insert_node(&mut stream, "root", vec![visible, collision_root], None, 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        assert_eq!(geometry.bounds, expected_bounds());
        assert_eq!(geometry.contact.vertices, expected_contact_vertices());
        assert_eq!(geometry.occluder_bounds, collision_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), collision_bounds());
        assert_eq!(parts.source(), MeshColliderSource::Collision);
        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.parts.len(), 1);
        assert_part_bounds_close(&parts.parts[0], collision_bounds());
    }

    #[test]
    fn occluder_bounds_fall_back_to_visible_geometry_without_collision() {
        let mut stream = NiStream::new();
        let shape = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        push_root(&mut stream, shape);

        assert_eq!(mesh_bounds(&stream).unwrap(), expected_bounds());
    }

    #[test]
    fn mesh_collection_skips_app_culled_subtrees() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let culled = insert_shape(&mut stream, "culled", &collision_vertices(), 1);
        let root = insert_node(&mut stream, "root", vec![visible, culled], None, 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();

        assert_eq!(geometry.bounds, expected_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), collision_bounds());
    }

    #[test]
    fn app_culled_visible_geometry_remains_excluded_from_visible_extraction() {
        let mut stream = NiStream::new();
        let culled = insert_shape(&mut stream, "culled", &collision_vertices(), 1);
        push_root(&mut stream, culled);

        assert!(mesh_geometry(&stream).is_none());
        assert_eq!(mesh_bounds(&stream).unwrap(), collision_bounds());
    }

    #[test]
    fn mesh_collection_handles_deep_node_hierarchy_iteratively() {
        let mut stream = NiStream::new();
        let mut child = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        for index in 0..2048 {
            child = insert_node(&mut stream, &format!("node_{index}"), vec![child], None, 0);
        }
        push_root(&mut stream, child);

        assert_eq!(mesh_geometry(&stream).unwrap().bounds, expected_bounds());
    }

    #[test]
    fn mesh_collection_terminates_on_cyclic_child_links() {
        let mut stream = NiStream::new();
        let shape = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let root = insert_node(&mut stream, "root", Vec::new(), None, 0);
        if let Some(NiType::NiNode(node)) = stream.objects.get_mut(root.key) {
            node.children.push(shape);
            node.children.push(root);
        }
        push_root(&mut stream, root);

        assert_eq!(mesh_geometry(&stream).unwrap().bounds, expected_bounds());
    }

    // Ground truth for rotation [0.3, 0.4, 1.2] at scale 1.5, computed outside this module as
    // Rx(-0.3) * Ry(-0.4) * Rz(-1.2) applied to column vectors (OpenMW's makeOsgQuat order).
    const MULTI_AXIS_ROTATION: [f32; 3] = [0.3, 0.4, 1.2];
    const MULTI_AXIS_LOWEST_OFFSET: [f32; 3] = [5.841_28, -4.082_88, -13.198_85];
    const MULTI_AXIS_AABB_MIN: [f32; 3] = [4.253_83, 8.191_49, 13.620_72];
    const MULTI_AXIS_AABB_MAX: [f32; 3] = [22.207_52, 27.949_29, 36.795_05];

    #[test]
    fn mesh_contact_world_position_matches_openmw_multi_axis_order() {
        let contact = test_contact();

        let position = contact.world_position([10.0, 20.0, 30.0], MULTI_AXIS_ROTATION, Some(1.5));

        assert_position_within(
            position,
            [
                MULTI_AXIS_LOWEST_OFFSET[0] + 10.0,
                MULTI_AXIS_LOWEST_OFFSET[1] + 20.0,
                MULTI_AXIS_LOWEST_OFFSET[2] + 30.0,
            ],
            0.000_2,
        );
    }

    #[test]
    fn mesh_aabb_world_aabb_matches_openmw_multi_axis_order() {
        let world =
            expected_bounds().world_aabb([10.0, 20.0, 30.0], MULTI_AXIS_ROTATION, Some(1.5));

        assert_position_within(world.min, MULTI_AXIS_AABB_MIN, 0.000_2);
        assert_position_within(world.max, MULTI_AXIS_AABB_MAX, 0.000_2);
    }

    #[test]
    fn mesh_aabb_world_aabb_single_axis_rotation() {
        let bounds = MeshAabb {
            min: [0.0, 0.0, 0.0],
            max: [2.0, 4.0, 6.0],
        };

        // Rx(-90 deg): (x, y, z) -> (x, z, -y).
        let world = bounds.world_aabb([0.0; 3], [std::f32::consts::FRAC_PI_2, 0.0, 0.0], None);

        assert_position_close(world.min, [0.0, 0.0, -4.0]);
        assert_position_close(world.max, [2.0, 6.0, 0.0]);
    }

    #[test]
    fn base_offsets_flat_bottom_cross_plane_grass_returns_all_bottom_vertices() {
        let contact = MeshContact::new(cross_plane_grass_vertices());

        let base = contact.base_offsets([0.0; 3], None);

        assert_eq!(base.len(), 4);
        assert!(base.iter().all(|vertex| vertex[2] == 0.0));
        assert!((contact.height() - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn base_offsets_single_low_root_vertex_returns_only_that_vertex() {
        let mut vertices = cross_plane_grass_vertices();
        vertices.push([0.0, 0.0, -5.0]);
        let contact = MeshContact::new(vertices);

        let base = contact.base_offsets([0.0; 3], Some(1.0));

        assert_eq!(base.as_ref(), &[[0.0, 0.0, -5.0]]);
        assert_position_close(
            contact.local_contact_offset([0.0; 3], None),
            [0.0, 0.0, -5.0],
        );
    }

    #[test]
    fn base_offsets_rotation_changes_membership() {
        let contact = MeshContact::new(cross_plane_grass_vertices());

        // Rx(-90 deg) maps (x, y, z) to (x, z, -y): only the two y = 5 vertices end up lowest.
        let base = contact.base_offsets([std::f32::consts::FRAC_PI_2, 0.0, 0.0], None);

        assert_eq!(base.len(), 2);
        for vertex in base.iter() {
            assert!((vertex[2] + 5.0).abs() < 0.000_01, "{vertex:?}");
        }
    }

    #[test]
    fn base_offsets_are_cached_per_rotation_and_scale() {
        let contact = MeshContact::new(cross_plane_grass_vertices());

        let first = contact.base_offsets([0.1, 0.2, 0.3], Some(1.5));
        let second = contact.base_offsets([0.1, 0.2, 0.3], Some(1.5));
        let lowest = contact.local_contact_offset([0.1, 0.2, 0.3], Some(1.5));

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(contact.cached_transform_count(), 1);
        assert!(first.iter().any(|vertex| {
            vertex
                .iter()
                .zip(lowest)
                .all(|(actual, expected)| (actual - expected).abs() < f32::EPSILON)
        }));
    }

    #[test]
    fn empty_root_collision_node_means_no_collision() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let collision_root = insert_root_collision_node(&mut stream, Vec::new());
        let root = insert_node(&mut stream, "root", vec![visible, collision_root], None, 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        assert_eq!(geometry.bounds, expected_bounds());
        assert_eq!(geometry.occluder_bounds, expected_bounds());
        assert!(parts.is_empty());
        assert_eq!(parts.source(), MeshColliderSource::NoCollision);
        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.iter().count(), 0);
    }

    #[test]
    fn nc_string_extra_data_means_no_collision() {
        for flag in ["NCO", "NCC", "ncc"] {
            let mut stream = NiStream::new();
            let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
            let collision_shape = insert_shape(&mut stream, "collision", &collision_vertices(), 0);
            let collision_root = insert_root_collision_node(&mut stream, vec![collision_shape]);
            let root = insert_node(
                &mut stream,
                "root",
                vec![visible, collision_root],
                Some(flag),
                0,
            );
            push_root(&mut stream, root);

            let geometry = mesh_geometry(&stream).unwrap();
            let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

            assert_eq!(geometry.bounds, expected_bounds(), "{flag}");
            assert_eq!(geometry.occluder_bounds, expected_bounds(), "{flag}");
            assert!(parts.is_empty(), "{flag}");
            assert_eq!(parts.source(), MeshColliderSource::NoCollision, "{flag}");
        }
    }

    #[test]
    fn mrk_root_skips_tri_editor_marker_shapes_for_collision_and_rendering() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let marker = insert_shape(
            &mut stream,
            "Tri EditorMarker01",
            &far_collision_vertices(),
            0,
        );
        let root = insert_node(&mut stream, "root", vec![visible, marker], Some("MRK"), 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        assert_eq!(geometry.bounds, expected_bounds());
        assert_eq!(geometry.occluder_bounds, expected_bounds());
        assert_eq!(parts.source(), MeshColliderSource::VisibleFallback);
        assert_eq!(parts.fallback(), None);
        assert_eq!(parts.parts.len(), 1);
        assert_part_bounds_close(&parts.parts[0], expected_bounds());
    }

    #[test]
    fn tri_editor_marker_shapes_are_kept_without_mrk_root() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let marker = insert_shape(
            &mut stream,
            "Tri EditorMarker01",
            &far_collision_vertices(),
            0,
        );
        let root = insert_node(&mut stream, "root", vec![visible, marker], None, 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        assert_ne!(geometry.bounds, expected_bounds());
        assert_eq!(parts.parts.len(), 2);
    }

    #[test]
    fn shadow_shapes_are_not_rendered_but_still_collide() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let shadow = insert_shape(&mut stream, "Tri Shadow", &far_collision_vertices(), 0);
        let root = insert_node(&mut stream, "root", vec![visible, shadow], None, 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        assert_eq!(geometry.bounds, expected_bounds());
        assert_eq!(parts.parts.len(), 2);
    }

    #[test]
    fn switch_node_uses_first_child_for_collision_and_active_child_for_rendering() {
        let mut stream = NiStream::new();
        let first = insert_shape(&mut stream, "first", &expected_contact_vertices(), 0);
        let second = insert_shape(&mut stream, "second", &collision_vertices(), 0);
        let switch = insert_switch_node(&mut stream, vec![first, second], 1);
        let root = insert_node(&mut stream, "root", vec![switch], None, 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        // NifOsg shows the initially active child; BulletNifLoader only visits the first child.
        assert_eq!(geometry.bounds, collision_bounds());
        assert_eq!(geometry.occluder_bounds, expected_bounds());
        assert_eq!(parts.parts.len(), 1);
        assert_part_bounds_close(&parts.parts[0], expected_bounds());
    }

    #[test]
    fn lod_node_uses_all_children() {
        let mut stream = NiStream::new();
        let near = insert_shape(&mut stream, "near", &expected_contact_vertices(), 0);
        let far = insert_shape(&mut stream, "far", &collision_vertices(), 0);
        let lod = insert_lod_node(&mut stream, vec![near, far]);
        let root = insert_node(&mut stream, "root", vec![lod], None, 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        assert_eq!(geometry.bounds, collision_bounds());
        assert_eq!(geometry.occluder_bounds, collision_bounds());
        assert_eq!(parts.parts.len(), 2);
    }

    #[test]
    fn root_collision_node_as_root_is_autogenerated_visible_geometry() {
        // `findRootCollisionNode` only inspects children, so a RootCollisionNode root is an
        // "unexpected" one: OpenMW renders it and autogenerates collision from it.
        let mut stream = NiStream::new();
        let shape = insert_shape(&mut stream, "collision", &collision_vertices(), 0);
        let collision_root = insert_root_collision_node(&mut stream, vec![shape]);
        push_root(&mut stream, collision_root);

        let geometry = mesh_geometry(&stream).unwrap();
        let parts = mesh_collider_parts(&stream, geometry.occluder_bounds);

        assert_eq!(geometry.bounds, collision_bounds());
        assert_eq!(parts.source(), MeshColliderSource::VisibleFallback);
        assert_eq!(parts.parts.len(), 1);
    }

    fn cross_plane_grass_vertices() -> Vec<[f32; 3]> {
        vec![
            [-5.0, 0.0, 0.0],
            [5.0, 0.0, 0.0],
            [5.0, 0.0, 20.0],
            [-5.0, 0.0, 20.0],
            [0.0, -5.0, 0.0],
            [0.0, 5.0, 0.0],
            [0.0, 5.0, 20.0],
            [0.0, -5.0, 20.0],
        ]
    }

    fn assert_position_within(actual: [f32; 3], expected: [f32; 3], tolerance: f32) {
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() < tolerance,
                "{actual} vs {expected}"
            );
        }
    }

    fn test_contact() -> MeshContact {
        MeshContact::new(vec![[0.0, 0.0, -10.0], [5.0, -2.0, -3.0], [-4.0, 3.0, 2.0]])
    }

    fn expected_bounds() -> MeshAabb {
        MeshAabb {
            min: [-4.0, -2.0, -10.0],
            max: [5.0, 3.0, 2.0],
        }
    }

    fn expected_contact_vertices() -> Vec<[f32; 3]> {
        vec![[0.0, 0.0, -10.0], [5.0, -2.0, -3.0], [-4.0, 3.0, 2.0]]
    }

    fn collision_vertices() -> Vec<[f32; 3]> {
        vec![
            [-100.0, -50.0, -20.0],
            [80.0, -50.0, -20.0],
            [80.0, 40.0, 30.0],
        ]
    }

    fn far_collision_vertices() -> Vec<[f32; 3]> {
        vec![
            [1_000.0, 900.0, 800.0],
            [1_100.0, 900.0, 800.0],
            [1_100.0, 950.0, 850.0],
        ]
    }

    fn box_vertices() -> Vec<[f32; 3]> {
        vec![[-1.0, -2.0, -3.0], [1.0, -2.0, -3.0], [1.0, 2.0, 3.0]]
    }

    fn unit_cube_vertices() -> Vec<[f32; 3]> {
        aabb_corners([-1.0; 3], [1.0; 3]).to_vec()
    }

    fn collision_bounds() -> MeshAabb {
        MeshAabb {
            min: [-100.0, -50.0, -20.0],
            max: [80.0, 40.0, 30.0],
        }
    }

    fn deep_collision_bounds() -> MeshAabb {
        MeshAabb {
            min: [1_000.0, 900.0, 800.0],
            max: [1_100.0, 950.0, 850.0],
        }
    }

    fn far_collision_bounds() -> MeshAabb {
        MeshAabb {
            min: [-100.0, -50.0, -20.0],
            max: [1_100.0, 950.0, 850.0],
        }
    }

    fn test_static_mesh(mesh_path: &str) -> StaticMesh {
        StaticMesh {
            mesh_path: mesh_path.to_owned(),
            mesh_key: normalize_mesh_key(mesh_path),
        }
    }

    fn empty_vfs() -> VFS {
        VFS::from_directories(Vec::<std::path::PathBuf>::new(), None)
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "greenmote-mesh-cache-{name}-{}-{}",
                std::process::id(),
                NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn write_nif(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut stream = NiStream::new();
        let shape = insert_shape(&mut stream, "test", &expected_contact_vertices(), 0);
        push_root(&mut stream, shape);
        std::fs::write(path, stream.save_bytes().unwrap()).unwrap();
    }

    fn write_collision_only_nif(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut stream = NiStream::new();
        let hidden = insert_shape(&mut stream, "hidden", &expected_contact_vertices(), 1);
        let shape = insert_shape(&mut stream, "collision", &collision_vertices(), 0);
        let collision_root = insert_root_collision_node(&mut stream, vec![shape]);
        let root = insert_node(&mut stream, "root", vec![hidden, collision_root], None, 0);
        push_root(&mut stream, root);
        std::fs::write(path, stream.save_bytes().unwrap()).unwrap();
    }

    fn write_visible_and_collision_nif(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let collision_shape = insert_shape(&mut stream, "collision", &collision_vertices(), 0);
        let collision_root = insert_root_collision_node(&mut stream, vec![collision_shape]);
        let root = insert_node(&mut stream, "root", vec![visible, collision_root], None, 0);
        push_root(&mut stream, root);
        std::fs::write(path, stream.save_bytes().unwrap()).unwrap();
    }

    fn insert_shape(
        stream: &mut NiStream,
        name: &str,
        vertices: &[[f32; 3]],
        flags: u16,
    ) -> NiLink<NiAVObject> {
        let geometry_data = NiTriShapeData {
            base: NiTriBasedGeomData {
                base: NiGeometryData {
                    vertices: vertices.iter().copied().map(Vec3::from_array).collect(),
                    ..NiGeometryData::default()
                },
            },
            // The duplicated first triangle exercises vertex dedup; a fan covers any further
            // vertices so every position is referenced by at least one triangle.
            triangles: [[0, 1, 2], [0, 1, 2]]
                .into_iter()
                .chain((3..vertices.len()).map(|index| {
                    let index = u16::try_from(index).unwrap();
                    [0, index - 1, index]
                }))
                .collect(),
            shared_normals: Vec::new(),
        };
        let data_key = stream.objects.insert(NiType::from(geometry_data));
        let shape = NiTriShape {
            base: NiTriBasedGeom {
                base: NiGeometry {
                    base: NiAVObject {
                        flags,
                        base: NiObjectNET {
                            name: name.to_owned(),
                            ..NiObjectNET::default()
                        },
                        ..NiAVObject::default()
                    },
                    geometry_data: NiLink::new(data_key),
                    ..NiGeometry::default()
                },
            },
        };
        let shape_key = stream.objects.insert(NiType::from(shape));
        NiLink::new(shape_key)
    }

    fn insert_node(
        stream: &mut NiStream,
        name: &str,
        children: Vec<NiLink<NiAVObject>>,
        extra: Option<&str>,
        flags: u16,
    ) -> NiLink<NiAVObject> {
        let extra_data =
            extra.map_or_else(NiLink::null, |value| insert_string_extra(stream, value));
        let node = NiNode {
            base: NiAVObject {
                flags,
                base: NiObjectNET {
                    name: name.to_owned(),
                    extra_data,
                    ..NiObjectNET::default()
                },
                ..NiAVObject::default()
            },
            children,
            ..NiNode::default()
        };
        let node_key = stream.objects.insert(NiType::from(node));
        NiLink::new(node_key)
    }

    fn insert_switch_node(
        stream: &mut NiStream,
        children: Vec<NiLink<NiAVObject>>,
        active_index: usize,
    ) -> NiLink<NiAVObject> {
        let node = NiSwitchNode {
            base: NiNode {
                base: NiAVObject {
                    base: NiObjectNET {
                        name: "switch".to_owned(),
                        ..NiObjectNET::default()
                    },
                    ..NiAVObject::default()
                },
                children,
                ..NiNode::default()
            },
            active_index,
        };
        let node_key = stream.objects.insert(NiType::from(node));
        NiLink::new(node_key)
    }

    fn insert_lod_node(
        stream: &mut NiStream,
        children: Vec<NiLink<NiAVObject>>,
    ) -> NiLink<NiAVObject> {
        let node = NiLODNode {
            base: NiSwitchNode {
                base: NiNode {
                    base: NiAVObject {
                        base: NiObjectNET {
                            name: "lod".to_owned(),
                            ..NiObjectNET::default()
                        },
                        ..NiAVObject::default()
                    },
                    children,
                    ..NiNode::default()
                },
                active_index: 0,
            },
            ..NiLODNode::default()
        };
        let node_key = stream.objects.insert(NiType::from(node));
        NiLink::new(node_key)
    }

    fn insert_root_collision_node(
        stream: &mut NiStream,
        children: Vec<NiLink<NiAVObject>>,
    ) -> NiLink<NiAVObject> {
        insert_root_collision_node_with_flags(stream, children, 0)
    }

    fn insert_root_collision_node_with_flags(
        stream: &mut NiStream,
        children: Vec<NiLink<NiAVObject>>,
        flags: u16,
    ) -> NiLink<NiAVObject> {
        let node = RootCollisionNode {
            base: NiNode {
                base: NiAVObject {
                    flags,
                    base: NiObjectNET {
                        name: "not-semantically-important".to_owned(),
                        ..NiObjectNET::default()
                    },
                    ..NiAVObject::default()
                },
                children,
                ..NiNode::default()
            },
        };
        let node_key = stream.objects.insert(NiType::from(node));
        NiLink::new(node_key)
    }

    fn insert_string_extra(stream: &mut NiStream, value: &str) -> NiLink<NiExtraData> {
        let extra = NiStringExtraData {
            base: NiExtraData::default(),
            value: value.to_owned(),
        };
        let extra_key = stream.objects.insert(NiType::from(extra));
        NiLink::new(extra_key)
    }

    fn set_transform(
        stream: &mut NiStream,
        link: NiLink<NiAVObject>,
        translation: Vec3,
        rotation: Mat3,
        scale: f32,
    ) {
        match stream.objects.get_mut(link.key).unwrap() {
            NiType::NiTriShape(shape) => {
                shape.base.base.base.translation = translation;
                shape.base.base.base.rotation = rotation;
                shape.base.base.base.scale = scale;
            }
            NiType::NiNode(node) => {
                node.base.translation = translation;
                node.base.rotation = rotation;
                node.base.scale = scale;
            }
            NiType::RootCollisionNode(node) => {
                node.base.base.translation = translation;
                node.base.base.rotation = rotation;
                node.base.base.scale = scale;
            }
            _ => panic!("unsupported transform target"),
        }
    }

    fn set_extra_data(stream: &mut NiStream, link: NiLink<NiAVObject>, value: &str) {
        let extra_data = insert_string_extra(stream, value);
        match stream.objects.get_mut(link.key).unwrap() {
            NiType::NiTriShape(shape) => shape.base.base.base.base.extra_data = extra_data,
            NiType::NiNode(node) => node.base.base.extra_data = extra_data,
            NiType::RootCollisionNode(node) => node.base.base.base.extra_data = extra_data,
            _ => panic!("unsupported extra data target"),
        }
    }

    fn push_root(stream: &mut NiStream, link: NiLink<NiAVObject>) {
        stream.roots.push(link.cast());
    }

    fn reference_world_position(
        contact: &MeshContact,
        translation: [f32; 3],
        rotation: [f32; 3],
        scale: Option<f32>,
    ) -> [f32; 3] {
        let offset = contact
            .contact_transform_uncached(rotation, scale.unwrap_or(1.0))
            .lowest;
        [
            offset[0] + translation[0],
            offset[1] + translation[1],
            offset[2] + translation[2],
        ]
    }

    fn assert_position_close(actual: [f32; 3], expected: [f32; 3]) {
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 0.000_01);
        }
    }

    fn assert_part_bounds_close(actual: &MeshColliderPart, expected: MeshAabb) {
        let bounds = actual.local_bounds().expect("part has points");
        assert_position_close(bounds.min, expected.min);
        assert_position_close(bounds.max, expected.max);
    }
}

use std::{
    collections::{HashMap, HashSet, VecDeque},
    io,
    sync::Mutex,
};

use glam::{Affine3A, EulerRot, Mat3, Quat, Vec3};
use tes3::{
    esp::{ObjectFlags, Static},
    nif::{
        NiAVObject, NiGeometryData, NiLink, NiNode, NiObjectNET, NiStream, NiStringExtraData,
        NiTriShape, NiTriShapeData, NiTriStrips, NiTriStripsData, RootCollisionNode,
    },
};
use vfstool_lib::{VFS, VfsFile};

type NifAffine3A = rapier3d::glamx::Affine3A;
#[cfg(test)]
type NifMat3 = rapier3d::math::Mat3;
type NifVec3 = rapier3d::math::Vec3;

#[derive(Clone, Debug)]
pub struct StaticMesh {
    pub static_id: String,
    pub mesh_path: String,
    mesh_key: String,
}

#[derive(Default)]
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
                        static_id: static_.id.clone(),
                        mesh_path: static_.mesh.clone(),
                        mesh_key: normalize_mesh_key(&static_.mesh),
                    },
                );
            }
        }

        index
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&StaticMesh> {
        self.statics.get(&id.to_lowercase())
    }

    #[must_use]
    pub fn get_normalized_key(&self, key: &str) -> Option<&StaticMesh> {
        self.statics.get(key)
    }
}

#[cfg(test)]
impl StaticMesh {
    pub(crate) fn new_for_test(static_id: &str, mesh_path: &str) -> Self {
        Self {
            static_id: static_id.to_owned(),
            mesh_path: mesh_path.to_owned(),
            mesh_key: normalize_mesh_key(mesh_path),
        }
    }
}

#[derive(Debug)]
pub struct MeshContact {
    vertices: Vec<[f32; 3]>,
    transform_cache: Mutex<HashMap<ContactTransformKey, [f32; 3]>>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ContactTransformKey {
    rotation: [u32; 3],
    scale: u32,
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

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MeshColliderParts {
    parts: Vec<MeshColliderPart>,
    fallback: Option<ColliderPartsFallback>,
    source: MeshColliderSource,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MeshColliderPart {
    pub(crate) obb: LocalObb,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LocalObb {
    pub(crate) center: [f32; 3],
    pub(crate) half_extents: [f32; 3],
    pub(crate) orientation: Quat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ColliderPartsFallback {
    Empty,
    OverBudget,
    UnsafeTransform,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MeshColliderSource {
    Collision,
    VisibleFallback,
}

const MAX_COLLIDER_PARTS: usize = 32;

impl MeshContact {
    #[must_use]
    pub fn new(vertices: Vec<[f32; 3]>) -> Self {
        Self {
            vertices,
            transform_cache: Mutex::default(),
        }
    }

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

    #[must_use]
    pub(crate) fn local_contact_offset(&self, rotation: [f32; 3], scale: Option<f32>) -> [f32; 3] {
        let scale = scale.unwrap_or(1.0);
        let key = ContactTransformKey {
            rotation: rotation.map(f32::to_bits),
            scale: scale.to_bits(),
        };

        if let Some(offset) = self
            .transform_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            .copied()
        {
            return offset;
        }

        let offset = self.local_contact_offset_uncached(rotation, scale);
        self.transform_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(key)
            .or_insert(offset);
        offset
    }

    #[cfg(test)]
    fn cached_transform_count(&self) -> usize {
        self.transform_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    fn local_contact_offset_uncached(&self, rotation: [f32; 3], scale: f32) -> [f32; 3] {
        // Match OpenMW's ESM-to-scene conversion: Z, then Y, then X, with negated axes.
        let rotation = Mat3::from_euler(EulerRot::ZYX, -rotation[2], -rotation[1], -rotation[0]);
        self.vertices
            .iter()
            .map(|vertex| rotation * (Vec3::from(*vertex) * scale))
            .min_by(|left, right| left.z.total_cmp(&right.z))
            .map_or([0.0; 3], |position| position.to_array())
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
    #[must_use]
    pub fn world_aabb(
        self,
        translation: [f32; 3],
        rotation: [f32; 3],
        scale: Option<f32>,
    ) -> WorldAabb {
        // Match OpenMW's ESM-to-scene conversion: Z, then Y, then X, with negated axes.
        let rotation = Mat3::from_euler(EulerRot::ZYX, -rotation[2], -rotation[1], -rotation[0]);
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
                obb: LocalObb::from_mesh_aabb(bounds),
            }],
            fallback: None,
            source: MeshColliderSource::VisibleFallback,
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
                .map(|obb| MeshColliderPart { obb })
                .collect(),
            fallback: None,
            source: MeshColliderSource::VisibleFallback,
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &MeshColliderPart> {
        self.parts.iter()
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
            orientation: Quat::IDENTITY,
        }
    }
}

impl WorldAabb {
    #[must_use]
    pub fn volume(self) -> f32 {
        let dx = (self.max[0] - self.min[0]).max(0.0);
        let dy = (self.max[1] - self.min[1]).max(0.0);
        let dz = (self.max[2] - self.min[2]).max(0.0);
        dx * dy * dz
    }

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

    #[must_use]
    pub fn contains_point(self, point: [f32; 3]) -> bool {
        self.min[0] <= point[0]
            && self.min[1] <= point[1]
            && self.min[2] <= point[2]
            && self.max[0] >= point[0]
            && self.max[1] >= point[1]
            && self.max[2] >= point[2]
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
        if !self.meshes.contains_key(key) {
            let mesh = load_geometry(self.vfs, &static_mesh.mesh_path).map_or_else(
                |error| CachedMesh::Failed(CachedMeshError::from_io(&error)),
                CachedMesh::Loaded,
            );
            self.meshes.insert(key.clone(), mesh);
        }

        if let Some(CachedMesh::BoundsOnly {
            geometry_error: Some(error),
            ..
        }) = self.meshes.get(key)
        {
            return Err(error.to_io());
        }

        if let Some(CachedMesh::BoundsOnly { stream, .. }) = self.meshes.get(key) {
            let mesh = mesh_geometry(stream)
                .ok_or_else(|| no_triangle_vertices_error(&static_mesh.mesh_path));
            match mesh {
                Ok(mesh) => {
                    self.meshes.insert(key.clone(), CachedMesh::Loaded(mesh));
                }
                Err(error) => {
                    if let Some(CachedMesh::BoundsOnly { geometry_error, .. }) =
                        self.meshes.get_mut(key)
                    {
                        *geometry_error = Some(CachedMeshError::from_io(&error));
                    }
                    return Err(error);
                }
            }
        }

        match &self.meshes[key] {
            CachedMesh::BoundsOnly { .. } => unreachable!("bounds-only mesh should be promoted"),
            CachedMesh::Loaded(geometry) => Ok(geometry),
            CachedMesh::Failed(error) => Err(error.to_io()),
        }
    }

    pub fn bounds(&mut self, static_mesh: &StaticMesh) -> io::Result<MeshAabb> {
        let key = &static_mesh.mesh_key;
        if !self.meshes.contains_key(key) {
            let mesh = load_bounds(self.vfs, &static_mesh.mesh_path).map_or_else(
                |error| CachedMesh::Failed(CachedMeshError::from_io(&error)),
                |(stream, bounds)| CachedMesh::BoundsOnly {
                    collider_parts: mesh_collider_parts(&stream, bounds),
                    stream,
                    bounds,
                    geometry_error: None,
                },
            );
            self.meshes.insert(key.clone(), mesh);
        }

        match &self.meshes[key] {
            CachedMesh::BoundsOnly { bounds, .. } => Ok(*bounds),
            CachedMesh::Loaded(geometry) => Ok(geometry.occluder_bounds),
            CachedMesh::Failed(error) => Err(error.to_io()),
        }
    }

    pub(crate) fn collider_parts(
        &mut self,
        static_mesh: &StaticMesh,
    ) -> io::Result<&MeshColliderParts> {
        let key = &static_mesh.mesh_key;
        if !self.meshes.contains_key(key) {
            let mesh = load_bounds(self.vfs, &static_mesh.mesh_path).map_or_else(
                |error| CachedMesh::Failed(CachedMeshError::from_io(&error)),
                |(stream, bounds)| CachedMesh::BoundsOnly {
                    collider_parts: mesh_collider_parts(&stream, bounds),
                    stream,
                    bounds,
                    geometry_error: None,
                },
            );
            self.meshes.insert(key.clone(), mesh);
        }

        match &self.meshes[key] {
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

fn normalize_mesh_key(mesh_path: &str) -> String {
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
    let accumulated = collect_mesh(stream, true, MeshSource::Visible)?;
    let mut vertices = accumulated.vertices.unwrap_or_default();
    dedup_vertices_preserving_order(&mut vertices);
    let bounds = MeshAabb {
        min: accumulated.min.to_array(),
        max: accumulated.max.to_array(),
    };

    let occluder_bounds = mesh_bounds(stream).unwrap_or(bounds);

    Some(MeshGeometry {
        contact: MeshContact::new(vertices.iter().map(glam::Vec3::to_array).collect()),
        bounds,
        occluder_bounds,
        occluder_parts: mesh_collider_parts(stream, occluder_bounds),
    })
}

fn mesh_bounds(stream: &NiStream) -> Option<MeshAabb> {
    let accumulated = collect_mesh(stream, false, MeshSource::Collision)
        .or_else(|| collect_mesh(stream, false, MeshSource::Visible))?;

    Some(MeshAabb {
        min: accumulated.min.to_array(),
        max: accumulated.max.to_array(),
    })
}

fn mesh_collider_parts(stream: &NiStream, bounds: MeshAabb) -> MeshColliderParts {
    let (source, collected) = collect_mesh_parts(stream, MeshSource::Collision)
        .map(|parts| (MeshColliderSource::Collision, parts))
        .or_else(|| {
            collect_mesh_parts(stream, MeshSource::Visible)
                .map(|parts| (MeshColliderSource::VisibleFallback, parts))
        })
        .unwrap_or((MeshColliderSource::VisibleFallback, Ok(Vec::new())));

    match collected {
        Ok(parts) if parts.is_empty() => {
            MeshColliderParts::aggregate_fallback(bounds, ColliderPartsFallback::Empty, source)
        }
        Ok(parts) if parts.len() > MAX_COLLIDER_PARTS => {
            MeshColliderParts::aggregate_fallback(bounds, ColliderPartsFallback::OverBudget, source)
        }
        Ok(parts) => MeshColliderParts {
            parts,
            fallback: None,
            source,
        },
        Err(fallback) => MeshColliderParts::aggregate_fallback(bounds, fallback, source),
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

#[derive(Clone, Copy, Eq, PartialEq)]
enum MeshSource {
    Visible,
    Collision,
}

impl MeshSource {
    const fn includes(self, collision: bool) -> bool {
        matches!(
            (self, collision),
            (Self::Visible, false) | (Self::Collision, true)
        )
    }
}

fn collect_mesh(
    stream: &NiStream,
    store_vertices: bool,
    source: MeshSource,
) -> Option<MeshAccumulator> {
    let mut accumulated = MeshAccumulator::new(store_vertices);
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();

    for root in &stream.roots {
        queue.push_back(VisitItem {
            parent: None,
            link: root.cast(),
            transform: Affine3A::IDENTITY,
            collision: false,
        });
    }

    while let Some(item) = queue.pop_front() {
        visit_object(
            stream,
            item,
            source,
            &mut visited,
            &mut queue,
            &mut accumulated,
        );
    }

    if !accumulated.has_vertices {
        return None;
    }

    Some(accumulated)
}

fn collect_mesh_parts(
    stream: &NiStream,
    source: MeshSource,
) -> Option<Result<Vec<MeshColliderPart>, ColliderPartsFallback>> {
    let mut parts = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();

    for root in &stream.roots {
        queue.push_back(VisitItem {
            parent: None,
            link: root.cast(),
            transform: Affine3A::IDENTITY,
            collision: false,
        });
    }

    while let Some(item) = queue.pop_front() {
        if let Err(fallback) =
            visit_object_parts(stream, item, source, &mut visited, &mut queue, &mut parts)
        {
            return Some(Err(fallback));
        }
        if parts.len() > MAX_COLLIDER_PARTS {
            return Some(Err(ColliderPartsFallback::OverBudget));
        }
    }

    (!parts.is_empty()).then_some(Ok(parts))
}

#[derive(Clone, Copy)]
struct VisitItem {
    parent: Option<tes3::nif::NiKey>,
    link: NiLink<NiAVObject>,
    transform: Affine3A,
    collision: bool,
}

type VisitEdge = (Option<tes3::nif::NiKey>, tes3::nif::NiKey, bool);

fn visit_object(
    stream: &NiStream,
    item: VisitItem,
    source: MeshSource,
    visited: &mut HashSet<VisitEdge>,
    queue: &mut VecDeque<VisitItem>,
    mesh: &mut MeshAccumulator,
) {
    let VisitItem {
        parent,
        link,
        transform: parent_transform,
        collision: parent_collision,
    } = item;
    if link.is_null() {
        return;
    }
    if !visited.insert((parent, link.key, parent_collision)) {
        return;
    }

    if let Some(root_collision_node) = stream.get_as::<_, RootCollisionNode>(link) {
        // Some shipped collision roots are app-culled while still carrying valid
        // collision geometry. Keep app-cull filtering for visible extraction and
        // for ordinary descendants below; only the RootCollisionNode itself gets
        // this collision-source exception.
        if source == MeshSource::Visible && root_collision_node.base.base.app_culled() {
            return;
        }
        let transform =
            parent_transform * affine3a_from_nif(root_collision_node.base.base.transform());
        for child in &root_collision_node.base.children {
            queue.push_back(VisitItem {
                parent: Some(link.key),
                link: *child,
                transform,
                collision: true,
            });
        }
    } else if let Some(node) = stream.get_as::<_, NiNode>(link) {
        if node.base.app_culled() {
            return;
        }
        let collision = parent_collision || has_rcn_extra(&node.base.base, stream);
        if source == MeshSource::Visible && collision {
            return;
        }
        let transform = parent_transform * affine3a_from_nif(node.base.transform());
        for child in &node.children {
            queue.push_back(VisitItem {
                parent: Some(link.key),
                link: *child,
                transform,
                collision,
            });
        }
    } else if let Some(shape) = stream.get_as::<_, NiTriShape>(link) {
        if shape.base.base.base.app_culled() {
            return;
        }
        let collision = parent_collision || has_rcn_extra(&shape.base.base.base.base, stream);
        if !source.includes(collision) {
            return;
        }
        let transform = parent_transform * affine3a_from_nif(shape.base.base.base.transform());
        if let Some(data) = stream.get_as::<_, NiTriShapeData>(shape.base.base.geometry_data) {
            include_vertices(
                &data.base.base,
                data.triangles.iter().flatten().copied(),
                transform,
                mesh,
            );
        }
    } else if let Some(strips) = stream.get_as::<_, NiTriStrips>(link) {
        if strips.base.base.base.app_culled() {
            return;
        }
        let collision = parent_collision || has_rcn_extra(&strips.base.base.base.base, stream);
        if !source.includes(collision) {
            return;
        }
        let transform = parent_transform * affine3a_from_nif(strips.base.base.base.transform());
        if let Some(data) = stream.get_as::<_, NiTriStripsData>(strips.base.base.geometry_data) {
            include_vertices(
                &data.base.base,
                data.strips.iter().copied(),
                transform,
                mesh,
            );
        }
    }
}

fn visit_object_parts(
    stream: &NiStream,
    item: VisitItem,
    source: MeshSource,
    visited: &mut HashSet<VisitEdge>,
    queue: &mut VecDeque<VisitItem>,
    parts: &mut Vec<MeshColliderPart>,
) -> Result<(), ColliderPartsFallback> {
    let VisitItem {
        parent,
        link,
        transform: parent_transform,
        collision: parent_collision,
    } = item;
    if link.is_null() {
        return Ok(());
    }
    if !visited.insert((parent, link.key, parent_collision)) {
        return Ok(());
    }

    if let Some(root_collision_node) = stream.get_as::<_, RootCollisionNode>(link) {
        // Some shipped collision roots are app-culled while still carrying valid
        // collision geometry. Keep app-cull filtering for visible extraction and
        // for ordinary descendants below; only the RootCollisionNode itself gets
        // this collision-source exception.
        if source == MeshSource::Visible && root_collision_node.base.base.app_culled() {
            return Ok(());
        }
        let transform =
            parent_transform * affine3a_from_nif(root_collision_node.base.base.transform());
        for child in &root_collision_node.base.children {
            queue.push_back(VisitItem {
                parent: Some(link.key),
                link: *child,
                transform,
                collision: true,
            });
        }
    } else if let Some(node) = stream.get_as::<_, NiNode>(link) {
        if node.base.app_culled() {
            return Ok(());
        }
        let collision = parent_collision || has_rcn_extra(&node.base.base, stream);
        if source == MeshSource::Visible && collision {
            return Ok(());
        }
        let transform = parent_transform * affine3a_from_nif(node.base.transform());
        for child in &node.children {
            queue.push_back(VisitItem {
                parent: Some(link.key),
                link: *child,
                transform,
                collision,
            });
        }
    } else if let Some(shape) = stream.get_as::<_, NiTriShape>(link) {
        if shape.base.base.base.app_culled() {
            return Ok(());
        }
        let collision = parent_collision || has_rcn_extra(&shape.base.base.base.base, stream);
        if !source.includes(collision) {
            return Ok(());
        }
        let transform = parent_transform * affine3a_from_nif(shape.base.base.base.transform());
        if let Some(data) = stream.get_as::<_, NiTriShapeData>(shape.base.base.geometry_data) {
            include_part(
                &data.base.base,
                data.triangles.iter().flatten().copied(),
                transform,
                parts,
            )?;
        }
    } else if let Some(strips) = stream.get_as::<_, NiTriStrips>(link) {
        if strips.base.base.base.app_culled() {
            return Ok(());
        }
        let collision = parent_collision || has_rcn_extra(&strips.base.base.base.base, stream);
        if !source.includes(collision) {
            return Ok(());
        }
        let transform = parent_transform * affine3a_from_nif(strips.base.base.base.transform());
        if let Some(data) = stream.get_as::<_, NiTriStripsData>(strips.base.base.geometry_data) {
            include_part(
                &data.base.base,
                data.strips.iter().copied(),
                transform,
                parts,
            )?;
        }
    }

    Ok(())
}

fn has_rcn_extra(object: &NiObjectNET, stream: &NiStream) -> bool {
    object
        .extra_datas_of_type::<NiStringExtraData>(stream)
        .any(|data| data.starts_with_ignore_ascii_case("RCN"))
}

fn include_vertices(
    data: &NiGeometryData,
    indices: impl IntoIterator<Item = u16>,
    transform: Affine3A,
    mesh: &mut MeshAccumulator,
) {
    for index in indices {
        if let Some(vertex) = data.vertices.get(usize::from(index)) {
            mesh.include(transform.transform_point3(vec3_from_nif(*vertex)));
        }
    }
}

fn include_part(
    data: &NiGeometryData,
    indices: impl IntoIterator<Item = u16>,
    transform: Affine3A,
    parts: &mut Vec<MeshColliderPart>,
) -> Result<(), ColliderPartsFallback> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut has_vertices = false;

    for index in indices {
        if let Some(vertex) = data.vertices.get(usize::from(index)) {
            has_vertices = true;
            let vertex = vec3_from_nif(*vertex);
            min = min.min(vertex);
            max = max.max(vertex);
        }
    }

    if !has_vertices {
        return Ok(());
    }

    let local_bounds = MeshAabb {
        min: min.to_array(),
        max: max.to_array(),
    };
    let local = LocalObb::from_mesh_aabb(local_bounds);
    let obb = transform_local_obb(local, transform)?;
    parts.push(MeshColliderPart { obb });

    Ok(())
}

fn transform_local_obb(
    local: LocalObb,
    transform: Affine3A,
) -> Result<LocalObb, ColliderPartsFallback> {
    let (orientation, scale) = decompose_uniform_transform(transform)?;
    let center = transform.transform_point3(Vec3::from(local.center));
    let half_extents = Vec3::from(local.half_extents) * scale.abs();

    Ok(LocalObb {
        center: center.to_array(),
        half_extents: half_extents.to_array(),
        orientation: orientation * local.orientation,
    })
}

fn affine3a_from_nif(value: NifAffine3A) -> Affine3A {
    Affine3A::from_cols_array(&value.to_cols_array())
}

fn vec3_from_nif(value: NifVec3) -> Vec3 {
    Vec3::from_array(value.to_array())
}

#[cfg(test)]
fn nif_vec3_from_array(value: [f32; 3]) -> NifVec3 {
    NifVec3::new(value[0], value[1], value[2])
}

#[cfg(test)]
fn nif_mat3_from_glam(value: Mat3) -> NifMat3 {
    NifMat3::from_cols_array(&value.to_cols_array())
}

fn decompose_uniform_transform(transform: Affine3A) -> Result<(Quat, f32), ColliderPartsFallback> {
    let matrix = Mat3::from(transform.matrix3);
    let x = matrix.x_axis;
    let y = matrix.y_axis;
    let z = matrix.z_axis;
    let scale = (x.length() + y.length() + z.length()) / 3.0;
    if !scale.is_finite() || scale <= f32::EPSILON {
        return Err(ColliderPartsFallback::UnsafeTransform);
    }

    let tolerance = 0.000_1_f32.max(scale * 0.000_1);
    if (x.length() - scale).abs() > tolerance
        || (y.length() - scale).abs() > tolerance
        || (z.length() - scale).abs() > tolerance
    {
        return Err(ColliderPartsFallback::UnsafeTransform);
    }

    let mut rotation = Mat3::from_cols(x / scale, y / scale, z / scale);
    if rotation.determinant() < 0.0 {
        rotation = Mat3::from_cols(-rotation.x_axis, -rotation.y_axis, -rotation.z_axis);
    }
    if !is_orthonormal(rotation) {
        return Err(ColliderPartsFallback::UnsafeTransform);
    }

    Ok((Quat::from_mat3(&rotation), scale))
}

fn is_orthonormal(matrix: Mat3) -> bool {
    const TOLERANCE: f32 = 0.000_1;
    let x = matrix.x_axis;
    let y = matrix.y_axis;
    let z = matrix.z_axis;

    (x.length() - 1.0).abs() <= TOLERANCE
        && (y.length() - 1.0).abs() <= TOLERANCE
        && (z.length() - 1.0).abs() <= TOLERANCE
        && x.dot(y).abs() <= TOLERANCE
        && x.dot(z).abs() <= TOLERANCE
        && y.dot(z).abs() <= TOLERANCE
        && (matrix.determinant() - 1.0).abs() <= TOLERANCE
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

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
        write_nif(&temp_dir.path().join("Meshes/Grass/Foo.nif"));
        let vfs = VFS::from_directories(vec![temp_dir.path().to_path_buf()], None);
        let static_mesh = test_static_mesh("Meshes/Grass/Foo.nif");
        let mut cache = MeshCache::new(&vfs);

        assert_eq!(cache.bounds(&static_mesh).unwrap(), expected_bounds());
        assert_eq!(cache.cached_mesh_state(&static_mesh), Some("bounds_only"));

        std::fs::remove_file(temp_dir.path().join("Meshes/Grass/Foo.nif")).unwrap();

        {
            let cached_geometry = cache.geometry(&static_mesh).unwrap();
            assert_eq!(cached_geometry.bounds, expected_bounds());
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
        assert_obb_close(
            parts.parts[0].obb,
            LocalObb::from_mesh_aabb(collision_bounds()),
        );
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
        assert_obb_close(
            parts.parts[0].obb,
            LocalObb::from_mesh_aabb(expected_bounds()),
        );
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
        let obb = parts.parts[0].obb;
        assert_position_close(obb.center, [10.0, 20.0, 30.0]);
        assert_position_close(obb.half_extents, [2.0, 4.0, 6.0]);
        assert_quat_close(
            obb.orientation,
            Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2),
        );
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
        assert_obb_close(parts.parts[0].obb, LocalObb::from_mesh_aabb(bounds));
    }

    #[test]
    fn rcn_extra_data_marks_subtree_as_collision_geometry() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let collision_shape = insert_shape(&mut stream, "collision", &collision_vertices(), 0);
        let collision_root = insert_node(
            &mut stream,
            "whatever-blender-called-it",
            vec![collision_shape],
            Some("RCN"),
            0,
        );
        let root = insert_node(&mut stream, "root", vec![visible, collision_root], None, 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();

        assert_eq!(geometry.bounds, expected_bounds());
        assert_eq!(geometry.occluder_bounds, collision_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), collision_bounds());
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
    fn culled_collision_subtree_does_not_replace_visible_occluder_bounds() {
        let mut stream = NiStream::new();
        let visible = insert_shape(&mut stream, "visible", &expected_contact_vertices(), 0);
        let collision_shape = insert_shape(&mut stream, "collision", &collision_vertices(), 0);
        let collision_root = insert_node(
            &mut stream,
            "collision",
            vec![collision_shape],
            Some("RCN"),
            1,
        );
        let root = insert_node(&mut stream, "root", vec![visible, collision_root], None, 0);
        push_root(&mut stream, root);

        let geometry = mesh_geometry(&stream).unwrap();

        assert_eq!(geometry.bounds, expected_bounds());
        assert_eq!(geometry.occluder_bounds, expected_bounds());
        assert_eq!(mesh_bounds(&stream).unwrap(), expected_bounds());
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
        assert_eq!(mesh_bounds(&stream).unwrap(), expected_bounds());
    }

    #[test]
    fn app_culled_visible_geometry_remains_excluded_from_visible_extraction() {
        let mut stream = NiStream::new();
        let culled = insert_shape(&mut stream, "culled", &collision_vertices(), 1);
        push_root(&mut stream, culled);

        assert!(mesh_geometry(&stream).is_none());
        assert!(mesh_bounds(&stream).is_none());
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

    fn box_vertices() -> Vec<[f32; 3]> {
        vec![[-1.0, -2.0, -3.0], [1.0, -2.0, -3.0], [1.0, 2.0, 3.0]]
    }

    fn collision_bounds() -> MeshAabb {
        MeshAabb {
            min: [-100.0, -50.0, -20.0],
            max: [80.0, 40.0, 30.0],
        }
    }

    fn test_static_mesh(mesh_path: &str) -> StaticMesh {
        StaticMesh {
            static_id: "test_static".to_owned(),
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
        let shape = insert_shape(&mut stream, "collision", &collision_vertices(), 0);
        let collision_root = insert_root_collision_node(&mut stream, vec![shape]);
        push_root(&mut stream, collision_root);
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
                    vertices: vertices.iter().copied().map(nif_vec3_from_array).collect(),
                    ..NiGeometryData::default()
                },
            },
            triangles: vec![[0, 1, 2], [0, 1, 2]],
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
                shape.base.base.base.translation = nif_vec3_from_array(translation.to_array());
                shape.base.base.base.rotation = nif_mat3_from_glam(rotation);
                shape.base.base.base.scale = scale;
            }
            NiType::NiNode(node) => {
                node.base.translation = nif_vec3_from_array(translation.to_array());
                node.base.rotation = nif_mat3_from_glam(rotation);
                node.base.scale = scale;
            }
            NiType::RootCollisionNode(node) => {
                node.base.base.translation = nif_vec3_from_array(translation.to_array());
                node.base.base.rotation = nif_mat3_from_glam(rotation);
                node.base.base.scale = scale;
            }
            _ => panic!("unsupported transform target"),
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
        let offset = contact.local_contact_offset_uncached(rotation, scale.unwrap_or(1.0));
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

    fn assert_obb_close(actual: LocalObb, expected: LocalObb) {
        assert_position_close(actual.center, expected.center);
        assert_position_close(actual.half_extents, expected.half_extents);
        assert_quat_close(actual.orientation, expected.orientation);
    }

    fn assert_quat_close(actual: Quat, expected: Quat) {
        assert!((actual.dot(expected).abs() - 1.0).abs() < 0.000_01);
    }
}

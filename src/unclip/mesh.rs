use std::{
    collections::{HashMap, HashSet},
    io,
    sync::Mutex,
};

use glam::{Affine3A, EulerRot, Mat3, Vec3};
use tes3::{
    esp::{ObjectFlags, Static},
    nif::{
        NiAVObject, NiGeometryData, NiLink, NiNode, NiStream, NiTriShape, NiTriShapeData,
        NiTriStrips, NiTriStripsData,
    },
};
use vfstool_lib::{VFS, VfsFile};

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
}

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

impl WorldAabb {
    #[must_use]
    pub fn volume(self) -> f32 {
        let dx = (self.max[0] - self.min[0]).max(0.0);
        let dy = (self.max[1] - self.min[1]).max(0.0);
        let dz = (self.max[2] - self.min[2]).max(0.0);
        dx * dy * dz
    }

    #[must_use]
    pub fn contains(self, other: Self) -> bool {
        self.min[0] <= other.min[0]
            && self.min[1] <= other.min[1]
            && self.min[2] <= other.min[2]
            && self.max[0] >= other.max[0]
            && self.max[1] >= other.max[1]
            && self.max[2] >= other.max[2]
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
    BoundsOnly { stream: NiStream, bounds: MeshAabb },
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

        if let Some(CachedMesh::BoundsOnly { stream, .. }) = self.meshes.get(key) {
            let mesh = mesh_geometry(stream).map_or_else(
                || {
                    CachedMesh::Failed(CachedMeshError::from_io(&io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("mesh {} has no triangle vertices", static_mesh.mesh_path),
                    )))
                },
                CachedMesh::Loaded,
            );
            self.meshes.insert(key.clone(), mesh);
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
                |(stream, bounds)| CachedMesh::BoundsOnly { stream, bounds },
            );
            self.meshes.insert(key.clone(), mesh);
        }

        match &self.meshes[key] {
            CachedMesh::BoundsOnly { bounds, .. } => Ok(*bounds),
            CachedMesh::Loaded(geometry) => Ok(geometry.bounds),
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
    let accumulated = collect_mesh(stream, true)?;
    let mut vertices = accumulated.vertices.unwrap_or_default();
    dedup_vertices_preserving_order(&mut vertices);

    Some(MeshGeometry {
        contact: MeshContact::new(vertices.iter().map(glam::Vec3::to_array).collect()),
        bounds: MeshAabb {
            min: accumulated.min.to_array(),
            max: accumulated.max.to_array(),
        },
    })
}

fn mesh_bounds(stream: &NiStream) -> Option<MeshAabb> {
    let accumulated = collect_mesh(stream, false)?;

    Some(MeshAabb {
        min: accumulated.min.to_array(),
        max: accumulated.max.to_array(),
    })
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

fn collect_mesh(stream: &NiStream, store_vertices: bool) -> Option<MeshAccumulator> {
    let mut accumulated = MeshAccumulator::new(store_vertices);

    for root in &stream.roots {
        visit_object(stream, root.cast(), Affine3A::IDENTITY, &mut accumulated);
    }

    if !accumulated.has_vertices {
        return None;
    }

    Some(accumulated)
}

fn visit_object(
    stream: &NiStream,
    link: NiLink<NiAVObject>,
    parent_transform: Affine3A,
    mesh: &mut MeshAccumulator,
) {
    if link.is_null() {
        return;
    }

    if let Some(node) = stream.get_as::<_, NiNode>(link) {
        if node.base.base.name == "RootCollisionNode" || node.base.app_culled() {
            return;
        }
        let transform = parent_transform * node.base.transform();
        for child in &node.children {
            visit_object(stream, *child, transform, mesh);
        }
    } else if let Some(shape) = stream.get_as::<_, NiTriShape>(link) {
        if shape.base.base.base.base.name == "RootCollisionNode"
            || shape.base.base.base.app_culled()
        {
            return;
        }
        let transform = parent_transform * shape.base.base.base.transform();
        if let Some(data) = stream.get_as::<_, NiTriShapeData>(shape.base.base.geometry_data) {
            include_vertices(
                &data.base.base,
                data.triangles.iter().flatten().copied(),
                transform,
                mesh,
            );
        }
    } else if let Some(strips) = stream.get_as::<_, NiTriStrips>(link) {
        if strips.base.base.base.base.name == "RootCollisionNode"
            || strips.base.base.base.app_culled()
        {
            return;
        }
        let transform = parent_transform * strips.base.base.base.transform();
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

fn include_vertices(
    data: &NiGeometryData,
    indices: impl IntoIterator<Item = u16>,
    transform: Affine3A,
    mesh: &mut MeshAccumulator,
) {
    for index in indices {
        if let Some(vertex) = data.vertices.get(usize::from(index)) {
            mesh.include(transform.transform_point3(*vertex));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use tes3::nif::{NiGeometry, NiObjectNET, NiTriBasedGeom, NiTriBasedGeomData, NiType};

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
        let geometry_data = NiTriShapeData {
            base: NiTriBasedGeomData {
                base: NiGeometryData {
                    vertices: expected_contact_vertices()
                        .into_iter()
                        .map(Vec3::from)
                        .collect(),
                    ..NiGeometryData::default()
                },
            },
            triangles: vec![[0, 1, 2], [0, 1, 2]],
            shared_normals: Vec::new(),
        };
        let mut stream = NiStream::new();
        let data_key = stream.objects.insert(NiType::from(geometry_data));
        let shape = NiTriShape {
            base: NiTriBasedGeom {
                base: NiGeometry {
                    base: NiAVObject {
                        base: NiObjectNET {
                            name: "test".to_owned(),
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
        stream.roots.push(NiLink::new(shape_key));
        std::fs::write(path, stream.save_bytes().unwrap()).unwrap();
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
}

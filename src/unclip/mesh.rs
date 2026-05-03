use std::{collections::HashMap, io};

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

#[derive(Clone, Debug, PartialEq)]
pub struct MeshContact {
    pub vertices: Vec<[f32; 3]>,
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
    pub fn world_position(
        &self,
        translation: [f32; 3],
        rotation: [f32; 3],
        scale: Option<f32>,
    ) -> [f32; 3] {
        // Match OpenMW's ESM-to-scene conversion: Z, then Y, then X, with negated axes.
        let rotation = Mat3::from_euler(EulerRot::ZYX, -rotation[2], -rotation[1], -rotation[0]);
        let scale = scale.unwrap_or(1.0);
        self.vertices
            .iter()
            .map(|vertex| rotation * (Vec3::from(*vertex) * scale) + Vec3::from(translation))
            .min_by(|left, right| left.z.total_cmp(&right.z))
            .map_or(translation, |position| position.to_array())
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

pub struct MeshContactCache<'a> {
    vfs: &'a VFS,
    meshes: HashMap<String, io::Result<MeshGeometry>>,
}

pub struct MeshBoundsCache<'a> {
    vfs: &'a VFS,
    meshes: HashMap<String, io::Result<MeshAabb>>,
}

impl<'a> MeshContactCache<'a> {
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
            let geometry = self.load_geometry(&static_mesh.mesh_path);
            self.meshes.insert(key.clone(), geometry);
        }

        self.meshes[key].as_ref().map_or_else(
            |error| Err(io::Error::new(error.kind(), error.to_string())),
            Ok,
        )
    }

    fn load_geometry(&self, mesh_path: &str) -> io::Result<MeshGeometry> {
        let file = resolve_mesh(self.vfs, mesh_path)?;
        let mut reader = file.open()?;
        let mut bytes = Vec::new();
        io::Read::read_to_end(&mut reader, &mut bytes)?;
        let stream = NiStream::from_bytes(&bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to load mesh {mesh_path}: {error}"),
            )
        })?;

        mesh_geometry(&stream).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("mesh {mesh_path} has no triangle vertices"),
            )
        })
    }
}

impl<'a> MeshBoundsCache<'a> {
    #[must_use]
    pub fn new(vfs: &'a VFS) -> Self {
        Self {
            vfs,
            meshes: HashMap::new(),
        }
    }

    pub fn bounds(&mut self, static_mesh: &StaticMesh) -> io::Result<MeshAabb> {
        let key = &static_mesh.mesh_key;
        if !self.meshes.contains_key(key) {
            let bounds = self.load_bounds(&static_mesh.mesh_path);
            self.meshes.insert(key.clone(), bounds);
        }

        self.meshes[key].as_ref().map_or_else(
            |error| Err(io::Error::new(error.kind(), error.to_string())),
            |bounds| Ok(*bounds),
        )
    }

    fn load_bounds(&self, mesh_path: &str) -> io::Result<MeshAabb> {
        let file = resolve_mesh(self.vfs, mesh_path)?;
        let mut reader = file.open()?;
        let mut bytes = Vec::new();
        io::Read::read_to_end(&mut reader, &mut bytes)?;
        let stream = NiStream::from_bytes(&bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to load mesh {mesh_path}: {error}"),
            )
        })?;

        mesh_bounds(&stream).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("mesh {mesh_path} has no triangle vertices"),
            )
        })
    }
}

fn normalize_mesh_key(mesh_path: &str) -> String {
    mesh_path.replace('/', "\\").to_lowercase()
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
    let vertices = accumulated.vertices.unwrap_or_default();

    Some(MeshGeometry {
        contact: MeshContact {
            vertices: vertices.iter().map(glam::Vec3::to_array).collect(),
        },
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
        let contact = MeshContact {
            vertices: vec![[0.0, 0.0, -10.0]],
        };

        assert!(
            (contact.world_position([1.0, 2.0, 100.0], [0.0; 3], Some(2.0))[2] - 80.0).abs()
                < f32::EPSILON
        );
    }
}

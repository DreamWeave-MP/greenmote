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
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshContact {
    pub vertices: Vec<[f32; 3]>,
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

pub struct MeshContactCache<'a> {
    vfs: &'a VFS,
    contacts: HashMap<String, io::Result<MeshContact>>,
}

impl<'a> MeshContactCache<'a> {
    #[must_use]
    pub fn new(vfs: &'a VFS) -> Self {
        Self {
            vfs,
            contacts: HashMap::new(),
        }
    }

    pub fn contact(&mut self, mesh_path: &str) -> io::Result<&MeshContact> {
        let key = mesh_path.replace('/', "\\").to_lowercase();
        if !self.contacts.contains_key(&key) {
            let contact = self.load_contact(mesh_path);
            self.contacts.insert(key.clone(), contact);
        }

        self.contacts[&key].as_ref().map_or_else(
            |error| Err(io::Error::new(error.kind(), error.to_string())),
            Ok,
        )
    }

    fn load_contact(&self, mesh_path: &str) -> io::Result<MeshContact> {
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

        lowest_contact(&stream).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("mesh {mesh_path} has no triangle vertices"),
            )
        })
    }
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

fn lowest_contact(stream: &NiStream) -> Option<MeshContact> {
    let mut lowest = Vec::new();

    for root in &stream.roots {
        visit_object(stream, root.cast(), Affine3A::IDENTITY, &mut lowest);
    }

    (!lowest.is_empty()).then(|| MeshContact {
        vertices: lowest.into_iter().map(|vertex| vertex.to_array()).collect(),
    })
}

fn visit_object(
    stream: &NiStream,
    link: NiLink<NiAVObject>,
    parent_transform: Affine3A,
    lowest: &mut Vec<Vec3>,
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
            visit_object(stream, *child, transform, lowest);
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
                lowest,
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
                lowest,
            );
        }
    }
}

fn include_vertices(
    data: &NiGeometryData,
    indices: impl IntoIterator<Item = u16>,
    transform: Affine3A,
    lowest: &mut Vec<Vec3>,
) {
    for index in indices {
        if let Some(vertex) = data.vertices.get(usize::from(index)) {
            lowest.push(transform.transform_point3(*vertex));
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

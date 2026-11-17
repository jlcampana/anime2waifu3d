//! Modelo en memoria: capas (una por nodo del glTF), materiales y texturas.
//!
//! Todo se lleva a coordenadas del mundo en la pose de reposo. Las mallas con skin se colocan
//! con sus huesos en reposo (matriz del hueso por su inverse bind matrix), de modo que, para
//! moverlas, la matriz de cada hueso es `mundo_en_pose * inversa(mundo_en_reposo)`.

use anyhow::{Context, Result};
use glam::{Mat3, Mat4, Vec3};
use std::collections::HashMap;
use std::path::Path;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

pub struct Primitive {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub material: usize,
    /// Huesos (índices en `Skeleton::bones`) y pesos de cada vértice; vacíos si no hay skin.
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
}

impl Primitive {
    /// Una primitiva nueva con solo los triángulos indicados, con los vértices renumerados.
    pub fn subset(&self, triangles: &[usize]) -> Primitive {
        let mut remap: HashMap<u32, u32> = HashMap::new();
        let (mut vertices, mut joints, mut weights) = (Vec::new(), Vec::new(), Vec::new());
        let mut indices = Vec::with_capacity(triangles.len() * 3);
        let skinned = !self.joints.is_empty();
        for &t in triangles {
            for k in 0..3 {
                let old = self.indices[t * 3 + k];
                let new = *remap.entry(old).or_insert_with(|| {
                    vertices.push(self.vertices[old as usize]);
                    if skinned {
                        joints.push(self.joints[old as usize]);
                        weights.push(self.weights[old as usize]);
                    }
                    (vertices.len() - 1) as u32
                });
                indices.push(new);
            }
        }
        Primitive { vertices, indices, material: self.material, joints, weights }
    }
}

pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    /// Matriz del hueso en el mundo, en la pose de reposo.
    pub rest_world: Mat4,
}

pub struct Skeleton {
    pub bones: Vec<Bone>,
    /// Nombre humanoide VRM (hips, leftUpperArm…) -> hueso.
    pub humanoid: HashMap<String, usize>,
}

impl Skeleton {
    pub fn bone(&self, humanoid: &str) -> Option<usize> {
        self.humanoid.get(humanoid).copied()
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }

    pub fn rest_position(&self, bone: usize) -> Vec3 {
        self.bones[bone].rest_world.w_axis.truncate()
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AlphaMode {
    Opaque,
    Mask(f32),
    Blend,
}

pub struct Material {
    pub name: String,
    pub base_color: [f32; 4],
    pub texture: Option<usize>,
    pub alpha: AlphaMode,
    pub double_sided: bool,
}

pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Una capa de la paleta. Las capas forman un árbol: ocultar una oculta sus hijas.
pub struct Layer {
    pub name: String,
    pub parent: Option<usize>,
    pub depth: usize,
    pub primitives: Vec<Primitive>,
    pub visible: bool,
    pub opacity: f32,
    pub expanded: bool,
}

pub struct Model {
    pub name: String,
    pub layers: Vec<Layer>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    pub skeleton: Option<Skeleton>,
}

impl Model {
    pub fn load(path: &Path) -> Result<Self> {
        let (document, buffers, images) =
            gltf::import(path).with_context(|| format!("no se pudo leer {}", path.display()))?;

        let textures = images.iter().map(convert_image).collect();
        let mut materials: Vec<Material> = document
            .materials()
            .map(|m| {
                let pbr = m.pbr_metallic_roughness();
                Material {
                    name: m.name().unwrap_or("material").to_string(),
                    base_color: pbr.base_color_factor(),
                    texture: pbr.base_color_texture().map(|t| t.texture().source().index()),
                    alpha: match m.alpha_mode() {
                        gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
                        gltf::material::AlphaMode::Mask => AlphaMode::Mask(m.alpha_cutoff().unwrap_or(0.5)),
                        gltf::material::AlphaMode::Blend => AlphaMode::Blend,
                    },
                    double_sided: m.double_sided(),
                }
            })
            .collect();
        // Material por defecto para primitivas sin material.
        let default_material = materials.len();
        materials.push(Material {
            name: "por defecto".into(),
            base_color: [0.8, 0.8, 0.8, 1.0],
            texture: None,
            alpha: AlphaMode::Opaque,
            double_sided: true,
        });

        let scene = document
            .default_scene()
            .or_else(|| document.scenes().next())
            .context("el archivo no tiene escenas")?;

        // Los VRM 0.x miran hacia -Z; glTF y VRM 1.0, hacia +Z. Se giran media vuelta para que
        // todos miren igual, como hace three-vrm.
        let vrm0 = document.extensions_used().any(|e| e == "VRM");
        let root = if vrm0 { Mat4::from_rotation_y(std::f32::consts::PI) } else { Mat4::IDENTITY };
        let mut node_world = vec![Mat4::IDENTITY; document.nodes().len()];
        let mut node_parent = vec![None; document.nodes().len()];
        for node in scene.nodes() {
            walk_nodes(&node, None, Mat4::IDENTITY, &mut node_world, &mut node_parent);
        }
        let (skeleton, bone_of_node) = read_skeleton(&document, path, root, &node_world, &node_parent);
        let context = ReadContext {
            root,
            buffers: &buffers,
            default_material,
            node_world: &node_world,
            skeleton: skeleton.as_ref(),
            bone_of_node: &bone_of_node,
        };
        let mut layers = Vec::new();
        for node in scene.nodes() {
            add_node(&node, None, 0, root, &context, &mut layers);
        }
        anyhow::ensure!(!layers.is_empty(), "el archivo no tiene mallas");

        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Self { name, layers, materials, textures, skeleton })
    }

    /// Visible si la capa y todas sus antecesoras lo son.
    pub fn effective_visible(&self, mut index: usize) -> bool {
        loop {
            let layer = &self.layers[index];
            if !layer.visible {
                return false;
            }
            match layer.parent {
                Some(parent) => index = parent,
                None => return true,
            }
        }
    }

    /// Producto de la opacidad de la capa y la de sus antecesoras, como los grupos de Photoshop.
    pub fn effective_opacity(&self, mut index: usize) -> f32 {
        let mut opacity = 1.0;
        loop {
            let layer = &self.layers[index];
            opacity *= layer.opacity;
            match layer.parent {
                Some(parent) => index = parent,
                None => return opacity,
            }
        }
    }

    pub fn is_ancestor(&self, ancestor: usize, mut index: usize) -> bool {
        while let Some(parent) = self.layers[index].parent {
            if parent == ancestor {
                return true;
            }
            index = parent;
        }
        false
    }

    /// Caja de lo que se ve, o de todo si no se ve nada.
    pub fn bounds(&self, only_visible: bool) -> Option<(Vec3, Vec3)> {
        let mut low = Vec3::splat(f32::MAX);
        let mut high = Vec3::splat(f32::MIN);
        for (index, layer) in self.layers.iter().enumerate() {
            if only_visible && !self.effective_visible(index) {
                continue;
            }
            for primitive in &layer.primitives {
                for v in &primitive.vertices {
                    low = low.min(Vec3::from(v.position));
                    high = high.max(Vec3::from(v.position));
                }
            }
        }
        if low.x <= high.x {
            Some((low, high))
        } else if only_visible {
            self.bounds(false)
        } else {
            None
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.layers.iter().flat_map(|l| &l.primitives).map(|p| p.indices.len() / 3).sum()
    }
}

fn walk_nodes(node: &gltf::Node, parent: Option<usize>, parent_world: Mat4, world: &mut [Mat4], parents: &mut [Option<usize>]) {
    let w = parent_world * Mat4::from_cols_array_2d(&node.transform().matrix());
    world[node.index()] = w;
    parents[node.index()] = parent;
    for child in node.children() {
        walk_nodes(&child, Some(node.index()), w, world, parents);
    }
}

/// El esqueleto: los nodos que son huesos de algún skin, con su padre más cercano que también
/// lo sea, y los nombres humanoides de VRM 0.x o 1.0 si el archivo los tiene.
fn read_skeleton(
    document: &gltf::Document,
    path: &Path,
    root: Mat4,
    node_world: &[Mat4],
    node_parent: &[Option<usize>],
) -> (Option<Skeleton>, HashMap<usize, usize>) {
    let mut bone_of_node: HashMap<usize, usize> = HashMap::new();
    let mut nodes = Vec::new();
    for skin in document.skins() {
        for joint in skin.joints() {
            if !bone_of_node.contains_key(&joint.index()) {
                bone_of_node.insert(joint.index(), nodes.len());
                nodes.push(joint);
            }
        }
    }
    if nodes.is_empty() {
        return (None, bone_of_node);
    }
    let bones = nodes
        .iter()
        .map(|node| {
            let mut parent = node_parent[node.index()];
            while let Some(p) = parent {
                if bone_of_node.contains_key(&p) {
                    break;
                }
                parent = node_parent[p];
            }
            Bone {
                name: node.name().map(str::to_string).unwrap_or_else(|| format!("hueso {}", node.index())),
                parent: parent.map(|p| bone_of_node[&p]),
                rest_world: root * node_world[node.index()],
            }
        })
        .collect();
    let mut humanoid = HashMap::new();
    if let Some(json) = raw_json(path) {
        let ext = &json["extensions"];
        if let Some(list) = ext["VRM"]["humanoid"]["humanBones"].as_array() {
            for b in list {
                if let (Some(name), Some(node)) = (b["bone"].as_str(), b["node"].as_u64()) {
                    if let Some(&bone) = bone_of_node.get(&(node as usize)) {
                        humanoid.insert(name.to_string(), bone);
                    }
                }
            }
        }
        if let Some(map) = ext["VRMC_vrm"]["humanoid"]["humanBones"].as_object() {
            for (name, b) in map {
                if let Some(&bone) = b["node"].as_u64().and_then(|n| bone_of_node.get(&(n as usize))) {
                    humanoid.insert(name.clone(), bone);
                }
            }
        }
    }
    (Some(Skeleton { bones, humanoid }), bone_of_node)
}

/// El JSON del archivo, para leer extensiones que el crate gltf no expone.
pub fn raw_json(path: &Path) -> Option<serde_json::Value> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.starts_with(b"glTF") && bytes.len() > 20 {
        let len = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
        serde_json::from_slice(bytes.get(20..20 + len)?).ok()
    } else {
        serde_json::from_slice(&bytes).ok()
    }
}

struct ReadContext<'a> {
    root: Mat4,
    buffers: &'a [gltf::buffer::Data],
    default_material: usize,
    node_world: &'a [Mat4],
    skeleton: Option<&'a Skeleton>,
    bone_of_node: &'a HashMap<usize, usize>,
}

/// Añade el nodo como capa si él o alguno de sus descendientes tiene malla. Los huesos sueltos
/// no aparecen en la paleta.
fn add_node(node: &gltf::Node, parent: Option<usize>, depth: usize, parent_world: Mat4, context: &ReadContext, layers: &mut Vec<Layer>) {
    if !has_mesh(node) {
        return;
    }
    let world = parent_world * Mat4::from_cols_array_2d(&node.transform().matrix());
    let mut primitives = Vec::new();
    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            if let Some(p) = read_primitive(&primitive, node, world, context) {
                primitives.push(p);
            }
        }
    }
    let name = node
        .name()
        .map(str::to_string)
        .or_else(|| node.mesh().and_then(|m| m.name().map(str::to_string)))
        .unwrap_or_else(|| format!("nodo {}", node.index()));
    let index = layers.len();
    layers.push(Layer { name, parent, depth, primitives, visible: true, opacity: 1.0, expanded: depth < 2 });
    for child in node.children() {
        add_node(&child, Some(index), depth + 1, world, context, layers);
    }
}

fn has_mesh(node: &gltf::Node) -> bool {
    node.mesh().is_some() || node.children().any(|c| has_mesh(&c))
}

fn read_primitive(primitive: &gltf::Primitive, node: &gltf::Node, world: Mat4, context: &ReadContext) -> Option<Primitive> {
    if primitive.mode() != gltf::mesh::Mode::Triangles {
        return None;
    }
    let reader = primitive.reader(|b| Some(&context.buffers[b.index()]));
    let raw_positions: Vec<Vec3> = reader.read_positions()?.map(Vec3::from).collect();
    let indices: Vec<u32> = match reader.read_indices() {
        Some(i) => i.into_u32().collect(),
        None => (0..raw_positions.len() as u32).collect(),
    };
    let raw_normals: Option<Vec<Vec3>> = reader.read_normals().map(|n| n.map(Vec3::from).collect());

    // Huesos y pesos, con los huesos traducidos a índices del esqueleto. Cada vértice se lleva a
    // su posición en reposo con las matrices de sus huesos.
    let skin = node.skin().zip(context.skeleton);
    let (mut joints, mut weights) = (Vec::new(), Vec::new());
    let mut vertex_matrix: Vec<Mat4> = Vec::new();
    if let (Some((skin, skeleton)), Some(j), Some(w)) = (skin, reader.read_joints(0), reader.read_weights(0)) {
        let joint_nodes: Vec<usize> = skin.joints().map(|n| n.index()).collect();
        let _ = skeleton;
        let bone_of = |local: u16| -> u16 {
            let node = joint_nodes[local as usize];
            context.bone_of_node.get(&node).copied().unwrap_or(0) as u16
        };
        let ibm: Vec<Mat4> = match skin.reader(|b| Some(&context.buffers[b.index()])).read_inverse_bind_matrices() {
            Some(m) => m.map(|m| Mat4::from_cols_array_2d(&m)).collect(),
            None => vec![Mat4::IDENTITY; joint_nodes.len()],
        };
        let joint_matrix: Vec<Mat4> = joint_nodes.iter().zip(&ibm).map(|(&n, ibm)| context.root * context.node_world[n] * *ibm).collect();
        for (jv, wv) in j.into_u16().zip(w.into_f32()) {
            let sum: f32 = wv.iter().sum();
            let wv = if sum > 0.0 { wv.map(|x| x / sum) } else { [1.0, 0.0, 0.0, 0.0] };
            let mut m = Mat4::ZERO;
            for k in 0..4 {
                m += joint_matrix[jv[k] as usize] * wv[k];
            }
            vertex_matrix.push(m);
            joints.push(jv.map(bone_of));
            weights.push(wv);
        }
    }
    let skinned = vertex_matrix.len() == raw_positions.len();
    if !skinned {
        joints.clear();
        weights.clear();
    }
    let positions: Vec<Vec3> = raw_positions
        .iter()
        .enumerate()
        .map(|(i, &p)| if skinned { vertex_matrix[i].transform_point3(p) } else { world.transform_point3(p) })
        .collect();
    let normals: Vec<Vec3> = match raw_normals {
        Some(n) => n
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let m = if skinned { vertex_matrix[i] } else { world };
                (Mat3::from_mat4(m).inverse().transpose() * v).normalize_or_zero()
            })
            .collect(),
        None => smooth_normals(&positions, &indices),
    };
    let uvs: Vec<[f32; 2]> = match reader.read_tex_coords(0) {
        Some(t) => t.into_f32().collect(),
        None => vec![[0.0, 0.0]; positions.len()],
    };
    let vertices = positions
        .iter()
        .zip(&normals)
        .zip(&uvs)
        .map(|((p, n), uv)| Vertex { position: p.to_array(), normal: n.to_array(), uv: *uv })
        .collect();
    Some(Primitive {
        vertices,
        indices,
        material: primitive.material().index().unwrap_or(context.default_material),
        joints,
        weights,
    })
}

pub fn smooth_normals(positions: &[Vec3], indices: &[u32]) -> Vec<Vec3> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for tri in indices.chunks_exact(3) {
        let [a, b, c] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        let face = (positions[b] - positions[a]).cross(positions[c] - positions[a]);
        for i in [a, b, c] {
            normals[i] += face;
        }
    }
    normals.iter().map(|n| n.normalize_or(Vec3::Y)).collect()
}

fn convert_image(image: &gltf::image::Data) -> Texture {
    use gltf::image::Format;
    let pixels = (image.width * image.height) as usize;
    let rgba = match image.format {
        Format::R8G8B8A8 => image.pixels.clone(),
        Format::R8G8B8 => image.pixels.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        Format::R8G8 => image.pixels.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        Format::R8 => image.pixels.iter().flat_map(|&p| [p, p, p, 255]).collect(),
        Format::R16G16B16A16 => image.pixels.chunks_exact(2).map(|p| p[1]).collect(),
        other => {
            log::warn!("formato de imagen no soportado: {other:?}");
            vec![255; pixels * 4]
        }
    };
    Texture { width: image.width, height: image.height, rgba }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_normals_of_a_flat_quad_point_up() {
        let positions = [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 0.0, -1.0), Vec3::new(0.0, 0.0, -1.0)];
        let normals = smooth_normals(&positions, &[0, 1, 2, 0, 2, 3]);
        for n in normals {
            assert!(n.distance(Vec3::Y) < 1e-6, "{n}");
        }
    }
}

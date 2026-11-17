//! Modelo en memoria: capas (una por nodo del glTF), materiales y texturas.
//!
//! Por ahora se lee en la pose de reposo: las mallas con skin se dejan tal cual vienen (el
//! estándar glTF ignora la transformación del nodo en ellas) y el resto se lleva a coordenadas
//! del mundo.

use anyhow::{Context, Result};
use glam::{Mat3, Mat4, Vec3};
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
        let mut layers = Vec::new();
        for node in scene.nodes() {
            add_node(&node, None, 0, root, root, &buffers, default_material, &mut layers);
        }
        anyhow::ensure!(!layers.is_empty(), "el archivo no tiene mallas");

        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Self { name, layers, materials, textures })
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

/// Añade el nodo como capa si él o alguno de sus descendientes tiene malla. Los huesos sueltos
/// no aparecen en la paleta.
#[allow(clippy::too_many_arguments)]
fn add_node(
    node: &gltf::Node,
    parent: Option<usize>,
    depth: usize,
    parent_world: Mat4,
    root: Mat4,
    buffers: &[gltf::buffer::Data],
    default_material: usize,
    layers: &mut Vec<Layer>,
) {
    if !has_mesh(node) {
        return;
    }
    let world = parent_world * Mat4::from_cols_array_2d(&node.transform().matrix());
    let mut primitives = Vec::new();
    if let Some(mesh) = node.mesh() {
        let transform = if node.skin().is_some() { root } else { world };
        for primitive in mesh.primitives() {
            if let Some(p) = read_primitive(&primitive, transform, buffers, default_material) {
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
        add_node(&child, Some(index), depth + 1, world, root, buffers, default_material, layers);
    }
}

fn has_mesh(node: &gltf::Node) -> bool {
    node.mesh().is_some() || node.children().any(|c| has_mesh(&c))
}

fn read_primitive(
    primitive: &gltf::Primitive,
    transform: Mat4,
    buffers: &[gltf::buffer::Data],
    default_material: usize,
) -> Option<Primitive> {
    if primitive.mode() != gltf::mesh::Mode::Triangles {
        return None;
    }
    let reader = primitive.reader(|b| Some(&buffers[b.index()]));
    let positions: Vec<Vec3> = reader.read_positions()?.map(|p| transform.transform_point3(p.into())).collect();
    let indices: Vec<u32> = match reader.read_indices() {
        Some(i) => i.into_u32().collect(),
        None => (0..positions.len() as u32).collect(),
    };
    let normal_matrix = Mat3::from_mat4(transform).inverse().transpose();
    let normals: Vec<Vec3> = match reader.read_normals() {
        Some(n) => n.map(|n| (normal_matrix * Vec3::from(n)).normalize_or_zero()).collect(),
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
        material: primitive.material().index().unwrap_or(default_material),
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

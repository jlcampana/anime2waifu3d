//! Escribe un `Model` como GLB: un nodo por capa, con la jerarquía de la paleta, los materiales
//! y las texturas (en PNG). Por ahora sin rig ni animaciones.

use crate::model::{AlphaMode, Model};
use anyhow::Result;
use serde_json::{json, Value};
use std::path::Path;

pub fn write(model: &Model, path: &Path) -> Result<()> {
    let mut bin: Vec<u8> = Vec::new();
    let mut views: Vec<Value> = Vec::new();
    let mut accessors: Vec<Value> = Vec::new();
    let mut push_view = |bin: &mut Vec<u8>, bytes: &[u8], target: Option<u32>| -> usize {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let mut view = json!({ "buffer": 0, "byteOffset": bin.len(), "byteLength": bytes.len() });
        if let Some(t) = target {
            view["target"] = json!(t);
        }
        bin.extend_from_slice(bytes);
        views.push(view);
        views.len() - 1
    };

    // Texturas e imágenes.
    let mut images = Vec::new();
    for texture in &model.textures {
        let mut png = Vec::new();
        image::write_buffer_with_format(
            &mut std::io::Cursor::new(&mut png),
            &texture.rgba,
            texture.width,
            texture.height,
            image::ExtendedColorType::Rgba8,
            image::ImageFormat::Png,
        )?;
        let view = push_view(&mut bin, &png, None);
        images.push(json!({ "bufferView": view, "mimeType": "image/png" }));
    }
    let textures: Vec<Value> = (0..images.len()).map(|i| json!({ "source": i, "sampler": 0 })).collect();
    let materials: Vec<Value> = model
        .materials
        .iter()
        .map(|m| {
            let mut pbr = json!({ "baseColorFactor": m.base_color, "metallicFactor": 0.0, "roughnessFactor": 0.8 });
            if let Some(t) = m.texture {
                pbr["baseColorTexture"] = json!({ "index": t });
            }
            let mut material = json!({ "name": m.name, "pbrMetallicRoughness": pbr, "doubleSided": m.double_sided });
            match m.alpha {
                AlphaMode::Opaque => {}
                AlphaMode::Mask(cutoff) => {
                    material["alphaMode"] = json!("MASK");
                    material["alphaCutoff"] = json!(cutoff);
                }
                AlphaMode::Blend => material["alphaMode"] = json!("BLEND"),
            }
            material
        })
        .collect();

    let mut meshes = Vec::new();
    let mut nodes: Vec<Value> = Vec::new();
    for layer in &model.layers {
        let mut node = json!({ "name": layer.name });
        if !layer.primitives.is_empty() {
            let mut primitives = Vec::new();
            for p in &layer.primitives {
                let positions: Vec<f32> = p.vertices.iter().flat_map(|v| v.position).collect();
                let normals: Vec<f32> = p.vertices.iter().flat_map(|v| v.normal).collect();
                let uvs: Vec<f32> = p.vertices.iter().flat_map(|v| v.uv).collect();
                let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
                for v in &p.vertices {
                    for c in 0..3 {
                        min[c] = min[c].min(v.position[c]);
                        max[c] = max[c].max(v.position[c]);
                    }
                }
                let count = p.vertices.len();
                let pos_view = push_view(&mut bin, bytemuck::cast_slice(&positions), Some(34962));
                accessors.push(json!({ "bufferView": pos_view, "componentType": 5126, "count": count, "type": "VEC3", "min": min, "max": max }));
                let pos = accessors.len() - 1;
                let nor_view = push_view(&mut bin, bytemuck::cast_slice(&normals), Some(34962));
                accessors.push(json!({ "bufferView": nor_view, "componentType": 5126, "count": count, "type": "VEC3" }));
                let nor = accessors.len() - 1;
                let uv_view = push_view(&mut bin, bytemuck::cast_slice(&uvs), Some(34962));
                accessors.push(json!({ "bufferView": uv_view, "componentType": 5126, "count": count, "type": "VEC2" }));
                let uv = accessors.len() - 1;
                let idx_view = push_view(&mut bin, bytemuck::cast_slice(&p.indices), Some(34963));
                accessors.push(json!({ "bufferView": idx_view, "componentType": 5125, "count": p.indices.len(), "type": "SCALAR" }));
                let idx = accessors.len() - 1;
                primitives.push(json!({
                    "attributes": { "POSITION": pos, "NORMAL": nor, "TEXCOORD_0": uv },
                    "indices": idx,
                    "material": p.material,
                }));
            }
            meshes.push(json!({ "name": layer.name, "primitives": primitives }));
            node["mesh"] = json!(meshes.len() - 1);
        }
        nodes.push(node);
    }
    // Jerarquía: los hijos de cada capa, y las capas sin padre como raíces de la escena.
    let mut roots = Vec::new();
    for (i, layer) in model.layers.iter().enumerate() {
        match layer.parent {
            Some(parent) => {
                let children = nodes[parent].get("children").cloned().unwrap_or(json!([]));
                let mut children: Vec<Value> = children.as_array().cloned().unwrap_or_default();
                children.push(json!(i));
                nodes[parent]["children"] = json!(children);
            }
            None => roots.push(i),
        }
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }

    let mut document = json!({
        "asset": { "version": "2.0", "generator": "anime2waifu3d" },
        "scene": 0,
        "scenes": [{ "nodes": roots }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{ "byteLength": bin.len() }],
    });
    if !images.is_empty() {
        document["images"] = json!(images);
        document["textures"] = json!(textures);
        document["samplers"] = json!([{ "magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497 }]);
    }
    let mut text = serde_json::to_vec(&document)?;
    while text.len() % 4 != 0 {
        text.push(b' ');
    }
    let total = 12 + 8 + text.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    std::fs::write(path, out)?;
    Ok(())
}

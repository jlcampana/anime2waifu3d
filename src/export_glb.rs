//! Escribe un `Model` como GLB: un nodo por capa, con la jerarquía de la paleta, los materiales
//! y las texturas (en PNG). Si el modelo tiene esqueleto, se escriben sus huesos, el skin y la
//! extensión VRMC_vrm con los nombres humanoides (VRM 1.0 mira hacia +Z, como el modelo).

use crate::model::{AlphaMode, Model};
use anyhow::Result;
use serde_json::{json, Value};
use std::path::Path;

pub fn write(model: &Model, path: &Path) -> Result<()> {
    write_with(model, path, None)
}

/// Como `write`, con el estado del proyecto en `extras.anime2waifu3d`.
pub fn write_with(model: &Model, path: &Path, project: Option<&Value>) -> Result<()> {
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
    // Los huesos van primero: el nodo de cada hueso tiene su mismo índice.
    let bone_count = model.skeleton.as_ref().map_or(0, |s| s.bones.len());
    let mut skin_index = None;
    if let Some(skeleton) = &model.skeleton {
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); bone_count];
        for (i, bone) in skeleton.bones.iter().enumerate() {
            let local = match bone.parent {
                Some(p) => skeleton.bones[p].rest_world.inverse() * bone.rest_world,
                None => bone.rest_world,
            };
            let (scale, rotation, translation) = local.to_scale_rotation_translation();
            let mut node = json!({ "name": bone.name, "translation": translation.to_array() });
            if !rotation.abs_diff_eq(glam::Quat::IDENTITY, 1e-6) {
                node["rotation"] = json!(rotation.to_array());
            }
            if !scale.abs_diff_eq(glam::Vec3::ONE, 1e-6) {
                node["scale"] = json!(scale.to_array());
            }
            nodes.push(node);
            if let Some(p) = bone.parent {
                children[p].push(i);
            }
        }
        for (i, list) in children.into_iter().enumerate() {
            if !list.is_empty() {
                nodes[i]["children"] = json!(list);
            }
        }
        let ibm: Vec<f32> = skeleton.bones.iter().flat_map(|b| b.rest_world.inverse().to_cols_array()).collect();
        let view = push_view(&mut bin, bytemuck::cast_slice(&ibm), None);
        accessors.push(json!({ "bufferView": view, "componentType": 5126, "count": bone_count, "type": "MAT4" }));
        skin_index = Some(json!({ "inverseBindMatrices": accessors.len() - 1, "joints": (0..bone_count).collect::<Vec<_>>() }));
    }
    // Formas de cada capa, para las expresiones VRM: nombre -> (nodo, índice).
    let mut expression_binds: Vec<(String, usize, usize)> = Vec::new();
    let mut texture_binds: Vec<(String, usize, [f32; 2])> = Vec::new();
    for layer in &model.layers {
        let mut node = json!({ "name": layer.name });
        // Las primitivas de una malla glTF comparten las formas: solo si todas tienen las mismas.
        let morph_names: Vec<String> = layer.primitives.first().map_or(Vec::new(), |p| p.morphs.iter().map(|m| m.name.clone()).collect());
        let with_morphs = !morph_names.is_empty()
            && layer.primitives.iter().all(|p| p.morphs.iter().map(|m| &m.name).eq(morph_names.iter()));
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
                let mut attributes = json!({ "POSITION": pos, "NORMAL": nor, "TEXCOORD_0": uv });
                if !p.joints.is_empty() {
                    let view = push_view(&mut bin, bytemuck::cast_slice(&p.joints), Some(34962));
                    accessors.push(json!({ "bufferView": view, "componentType": 5123, "count": count, "type": "VEC4" }));
                    attributes["JOINTS_0"] = json!(accessors.len() - 1);
                    let view = push_view(&mut bin, bytemuck::cast_slice(&p.weights), Some(34962));
                    accessors.push(json!({ "bufferView": view, "componentType": 5126, "count": count, "type": "VEC4" }));
                    attributes["WEIGHTS_0"] = json!(accessors.len() - 1);
                }
                let mut primitive = json!({ "attributes": attributes, "indices": idx, "material": p.material });
                if with_morphs {
                    let targets: Vec<Value> = p
                        .morphs
                        .iter()
                        .map(|m| {
                            let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
                            for d in &m.deltas {
                                for c in 0..3 {
                                    min[c] = min[c].min(d[c]);
                                    max[c] = max[c].max(d[c]);
                                }
                            }
                            let view = push_view(&mut bin, bytemuck::cast_slice(&m.deltas), Some(34962));
                            accessors.push(json!({ "bufferView": view, "componentType": 5126, "count": count, "type": "VEC3", "min": min, "max": max }));
                            let mut target = json!({ "POSITION": accessors.len() - 1 });
                            if !m.uv_deltas.is_empty() {
                                let view = push_view(&mut bin, bytemuck::cast_slice(&m.uv_deltas), Some(34962));
                                accessors.push(json!({ "bufferView": view, "componentType": 5126, "count": count, "type": "VEC2" }));
                                target["TEXCOORD_0"] = json!(accessors.len() - 1);
                            }
                            target
                        })
                        .collect();
                    primitive["targets"] = json!(targets);
                }
                primitives.push(primitive);
            }
            let mut mesh = json!({ "name": layer.name, "primitives": primitives });
            if with_morphs {
                mesh["weights"] = json!(vec![0.0; morph_names.len()]);
                mesh["extras"] = json!({ "targetNames": morph_names });
                for (i, name) in morph_names.iter().enumerate() {
                    // Las formas que solo mueven UV (cambiar de dibujo en un atlas) van en VRM
                    // como transformación de textura de su material, que es lo que entienden
                    // los visores VRM.
                    let uv_only = layer.primitives.iter().all(|p| {
                        let m = &p.morphs[i];
                        !m.uv_deltas.is_empty() && m.deltas.iter().all(|d| d.iter().all(|c| c.abs() < 1e-7))
                    });
                    if uv_only {
                        for p in &layer.primitives {
                            if let Some(offset) = p.morphs[i].uv_deltas.first() {
                                texture_binds.push((name.clone(), p.material, *offset));
                            }
                        }
                    } else {
                        expression_binds.push((name.clone(), nodes.len(), i));
                    }
                }
            }
            meshes.push(mesh);
            node["mesh"] = json!(meshes.len() - 1);
            if skin_index.is_some() && layer.primitives.iter().any(|p| !p.joints.is_empty()) {
                node["skin"] = json!(0);
            }
        }
        nodes.push(node);
    }
    // Jerarquía: los hijos de cada capa, y las capas sin padre (y los huesos raíz) como raíces.
    let mut roots: Vec<usize> = model
        .skeleton
        .iter()
        .flat_map(|s| s.bones.iter().enumerate().filter(|(_, b)| b.parent.is_none()).map(|(i, _)| i))
        .collect();
    for (i, layer) in model.layers.iter().enumerate() {
        let node = bone_count + i;
        match layer.parent {
            Some(parent) => {
                let parent = bone_count + parent;
                let children = nodes[parent].get("children").cloned().unwrap_or(json!([]));
                let mut children: Vec<Value> = children.as_array().cloned().unwrap_or_default();
                children.push(json!(node));
                nodes[parent]["children"] = json!(children);
            }
            None => roots.push(node),
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
    if let (Some(skin), Some(skeleton)) = (skin_index, &model.skeleton) {
        document["skins"] = json!([skin]);
        let mut human = serde_json::Map::new();
        for (name, &bone) in &skeleton.humanoid {
            human.insert(name.clone(), json!({ "node": bone }));
        }
        document["extensionsUsed"] = json!(["VRMC_vrm"]);
        document["extensions"] = json!({ "VRMC_vrm": {
            "specVersion": "1.0",
            "meta": { "name": model.name, "authors": ["anime2waifu3d"], "licenseUrl": "https://vrm.dev/licenses/1.0/" },
            "humanoid": { "humanBones": human },
        }});
        // Expresiones VRM: las formas con nombre de una expresión predefinida.
        // Las de nombre predefinido van en `preset`; las demás (sonrisa, gatito…), en `custom`.
        const PRESETS: [&str; 13] = ["blink", "blinkLeft", "blinkRight", "happy", "angry", "sad", "relaxed", "surprised", "aa", "ih", "ou", "ee", "oh"];
        let mut names: Vec<&String> = expression_binds.iter().map(|(n, _, _)| n).chain(texture_binds.iter().map(|(n, _, _)| n)).collect();
        names.sort();
        names.dedup();
        let (mut presets, mut custom) = (serde_json::Map::new(), serde_json::Map::new());
        for name in names {
            let binds: Vec<Value> = expression_binds
                .iter()
                .filter(|(n, _, _)| n == name)
                .map(|(_, node, index)| json!({ "node": node, "index": index, "weight": 1.0 }))
                .collect();
            let textures: Vec<Value> = texture_binds
                .iter()
                .filter(|(n, _, _)| n == name)
                .map(|(_, material, offset)| json!({ "material": material, "scale": [1.0, 1.0], "offset": offset }))
                .collect();
            let mut expression = json!({ "isBinary": !textures.is_empty() });
            if !binds.is_empty() {
                expression["morphTargetBinds"] = json!(binds);
            }
            if !textures.is_empty() {
                expression["textureTransformBinds"] = json!(textures);
            }
            if PRESETS.contains(&name.as_str()) {
                presets.insert(name.clone(), expression);
            } else {
                custom.insert(name.clone(), expression);
            }
        }
        if !presets.is_empty() || !custom.is_empty() {
            document["extensions"]["VRMC_vrm"]["expressions"] = json!({ "preset": presets, "custom": custom });
        }
        if let Some(hair) = crate::hair::HairSim::new(model) {
            document["extensionsUsed"] = json!(["VRMC_vrm", "VRMC_springBone"]);
            document["extensions"]["VRMC_springBone"] = hair.spring_bone_extension(skeleton);
        }
    }
    if let Some(project) = project {
        document["extras"] = json!({ "anime2waifu3d": project });
    }
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

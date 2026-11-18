//! Formato de proyecto y exportaciones.
//!
//! Un proyecto es un GLB normal (se abre en Blender y en cualquier visor glTF) con el estado de
//! la app en `extras.anime2waifu3d`: capas (visibilidad, opacidad, plegado, física propia),
//! pose, animación, expresión, física, luz, cámara y modo de vista. Al abrirlo, la app lo
//! restaura. Las capas se guardan por nombre e índice: si el archivo se edita fuera y cambian,
//! se aplica lo que sigue cuadrando.
//!
//! Exportaciones (lo que pide el LEEME: "todo" o "solo modelo y texturas"):
//! - GLB completo: esqueleto, pesos, formas, física (`VRMC_springBone`) y expresiones VRM.
//! - VRM 1.0: lo mismo con extensión `.vrm`.
//! - GLB solo modelo y texturas: mallas estáticas en reposo, sin esqueleto.
//! - OBJ + MTL + PNG: solo modelo y texturas.
//! - FBX: el GLB completo convertido con Blender, si está instalado (en local, sin servicios).
//!
//! Todas pueden llevar solo las capas visibles; la opacidad de las capas pasa a los materiales.

use crate::model::Model;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

pub const VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct Project {
    pub version: u32,
    pub view_mode: String,
    pub wire_overlay: bool,
    pub show_grid: bool,
    pub light: usize,
    pub pose: String,
    pub animation: String,
    pub bounce: f32,
    pub spring_preset: usize,
    pub hair_physics: bool,
    pub hair_preset: usize,
    pub cloth_preset: usize,
    pub wind: f32,
    pub expression: String,
    pub auto_blink: bool,
    pub camera: Option<CameraState>,
    pub layers: Vec<LayerState>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct CameraState {
    pub target: [f32; 3],
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct LayerState {
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub expanded: bool,
    /// Juego de valores propio de la física de la capa (nombre de `hair::PRESETS`).
    pub physics: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Format {
    Glb,
    Vrm,
    GlbStatic,
    Obj,
    Fbx,
}

impl Format {
    pub const ALL: [Format; 5] = [Format::Glb, Format::Vrm, Format::GlbStatic, Format::Obj, Format::Fbx];

    pub fn name(self) -> &'static str {
        match self {
            Format::Glb => "GLB completo (esqueleto, física, expresiones)",
            Format::Vrm => "VRM 1.0",
            Format::GlbStatic => "GLB solo modelo y texturas",
            Format::Obj => "OBJ solo modelo y texturas",
            Format::Fbx => "FBX (con Blender)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Glb | Format::GlbStatic => "glb",
            Format::Vrm => "vrm",
            Format::Obj => "obj",
            Format::Fbx => "fbx",
        }
    }

    /// El formato por la extensión del archivo; `static_only` elige el GLB sin esqueleto.
    pub fn from_path(path: &Path, static_only: bool) -> Option<Format> {
        match path.extension()?.to_str()?.to_lowercase().as_str() {
            "glb" if static_only => Some(Format::GlbStatic),
            "glb" => Some(Format::Glb),
            "vrm" => Some(Format::Vrm),
            "obj" => Some(Format::Obj),
            "fbx" => Some(Format::Fbx),
            _ => None,
        }
    }
}

/// Guarda el proyecto: el modelo entero (también lo oculto) y el estado.
pub fn save(model: &Model, project: &Project, path: &Path) -> Result<()> {
    let state = serde_json::to_value(project)?;
    crate::export_glb::write_with(model, path, Some(&state))
}

/// El estado guardado en un modelo abierto, si es un proyecto.
pub fn read(model: &Model) -> Option<Project> {
    serde_json::from_value(model.project.clone()?).ok()
}

pub fn export(model: &Model, path: &Path, format: Format, only_visible: bool) -> Result<()> {
    match format {
        Format::Glb | Format::Vrm => crate::export_glb::write(&model.export_copy(only_visible, true), path),
        Format::GlbStatic => crate::export_glb::write(&model.export_copy(only_visible, false), path),
        Format::Obj => write_obj(&model.export_copy(only_visible, false), path),
        Format::Fbx => {
            let temporary = path.with_extension("tmp.glb");
            crate::export_glb::write(&model.export_copy(only_visible, true), &temporary)?;
            let result = blender_to_fbx(&temporary, path);
            let _ = std::fs::remove_file(&temporary);
            result
        }
    }
}

/// OBJ con su MTL y las texturas en PNG al lado. Una `o` por capa con mallas.
fn write_obj(model: &Model, path: &Path) -> Result<()> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("modelo").to_string();
    let dir = path.parent().unwrap_or(Path::new("."));
    let mtl_name = format!("{stem}.mtl");
    let mut obj = std::io::BufWriter::new(std::fs::File::create(path).with_context(|| format!("no se pudo crear {}", path.display()))?);
    writeln!(obj, "# anime2waifu3d\nmtllib {mtl_name}")?;
    let mut base = 1usize;
    for layer in &model.layers {
        if layer.primitives.is_empty() {
            continue;
        }
        writeln!(obj, "o {}", layer.name.replace(' ', "_"))?;
        for p in &layer.primitives {
            for v in &p.vertices {
                writeln!(obj, "v {} {} {}", v.position[0], v.position[1], v.position[2])?;
            }
            for v in &p.vertices {
                // glTF tiene el origen de la textura arriba; OBJ, abajo.
                writeln!(obj, "vt {} {}", v.uv[0], 1.0 - v.uv[1])?;
            }
            for v in &p.vertices {
                writeln!(obj, "vn {} {} {}", v.normal[0], v.normal[1], v.normal[2])?;
            }
            writeln!(obj, "usemtl material_{}", p.material)?;
            for tri in p.indices.chunks_exact(3) {
                let [a, b, c] = [tri[0] as usize + base, tri[1] as usize + base, tri[2] as usize + base];
                writeln!(obj, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}")?;
            }
            base += p.vertices.len();
        }
    }
    obj.flush()?;

    let mut mtl = std::io::BufWriter::new(std::fs::File::create(dir.join(&mtl_name))?);
    let mut saved = vec![false; model.textures.len()];
    for (i, m) in model.materials.iter().enumerate() {
        let [r, g, b, a] = m.base_color;
        writeln!(mtl, "newmtl material_{i}\nKd {r} {g} {b}\nd {a}")?;
        if let Some(t) = m.texture.filter(|t| *t < model.textures.len()) {
            let file = format!("{stem}_textura_{t}.png");
            if !saved[t] {
                let texture = &model.textures[t];
                image::save_buffer(dir.join(&file), &texture.rgba, texture.width, texture.height, image::ExtendedColorType::Rgba8)?;
                saved[t] = true;
            }
            writeln!(mtl, "map_Kd {file}")?;
        }
        writeln!(mtl)?;
    }
    mtl.flush()?;
    Ok(())
}

/// Convierte un GLB a FBX con Blender en segundo plano, con su configuración de fábrica.
fn blender_to_fbx(glb: &Path, fbx: &Path) -> Result<()> {
    let script = "import bpy, sys\n\
        args = sys.argv[sys.argv.index('--') + 1:]\n\
        bpy.ops.wm.read_factory_settings(use_empty=True)\n\
        bpy.ops.import_scene.gltf(filepath=args[0])\n\
        bpy.ops.export_scene.fbx(filepath=args[1], add_leaf_bones=False, path_mode='COPY', embed_textures=True)\n";
    let output = std::process::Command::new("blender")
        .args(["-b", "--factory-startup", "--python-expr", script, "--"])
        .arg(glb)
        .arg(fbx)
        .output()
        .context("no se encuentra Blender: el FBX se hace con Blender instalado")?;
    anyhow::ensure!(fbx.exists(), "Blender no escribió el FBX: {}", String::from_utf8_lossy(&output.stderr).lines().last().unwrap_or(""));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_state_survives_json() {
        let project = Project {
            version: VERSION,
            pose: "saludo".into(),
            expression: "feliz".into(),
            wind: 0.4,
            camera: Some(CameraState { target: [0.0, 1.0, 0.0], distance: 3.0, yaw: 0.5, pitch: 0.1 }),
            layers: vec![LayerState { name: "pelo".into(), visible: false, opacity: 0.5, expanded: true, physics: Some("suelto".into()) }],
            ..Default::default()
        };
        let back: Project = serde_json::from_value(serde_json::to_value(&project).unwrap()).unwrap();
        assert_eq!(back, project);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let back: Project = serde_json::from_str(r#"{"version": 1, "pose": "jarras"}"#).unwrap();
        assert_eq!(back.pose, "jarras");
        assert!(back.layers.is_empty());
    }
}

//! Las dos rutas completas, para la interfaz y para la línea de comandos. Informan de su avance
//! con `progress` (la interfaz lo enseña en la barra de estado; la línea de comandos lo imprime).
//!
//! - `from_image`: una ilustración → personaje 3D (recorte, análisis, ajuste de Base_Female,
//!   pintura, cara, pelo y falda).
//! - `from_ai_mesh`: una malla de IA de una pieza (como las de SupaVoxel) → personaje por capas y
//!   riggeado (reducción con Blender si es muy densa, segmentación, rig, pelo, falda y cara).

use crate::model::Model;
use anyhow::{Context, Result};
use image::RgbaImage;
use std::path::{Path, PathBuf};

/// Un archivo de `assets/` (o de `tools/`): relativo a la carpeta actual o, si no está ahí, a la
/// del proyecto (para lanzar la app desde cualquier sitio).
pub fn resource(relative: &str) -> PathBuf {
    let here = PathBuf::from(relative);
    if here.exists() {
        return here;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

pub fn default_base() -> PathBuf {
    resource("assets/candidatas/vroid-cc0/base_female/Base_Female.vrm")
}

pub struct ImageResult {
    pub model: Model,
    /// Por imagen: su papel ("frente", "espalda", "no usada"…), la imagen sin fondo y la de
    /// comprobación del análisis (imagen y partes, con líneas).
    pub images: Vec<(String, RgbaImage, RgbaImage)>,
}

/// Una imagen ya recortada y analizada.
struct Candidate {
    name: String,
    cut: RgbaImage,
    analysis: crate::analyze::Analysis,
}

impl Candidate {
    /// Se ve la cara (la imagen es de frente).
    fn front(&self) -> bool {
        self.analysis.keypoints.face_width > 0.0
    }

    /// De cuerpo entero con proporciones creíbles.
    fn full_body(&self) -> bool {
        (4.8..=9.5).contains(&self.analysis.keypoints.heads_tall)
    }

    /// Para elegir la principal: de frente, de cuerpo entero, con piel distinguible y cerca de
    /// las 6,5 cabezas habituales.
    fn score(&self) -> f32 {
        let k = &self.analysis.keypoints;
        (self.front() as u8 * 3 + self.full_body() as u8 * 2 + self.analysis.skin_by_color as u8) as f32 - (k.heads_tall - 6.5).abs() * 0.2
    }
}

/// Puntos clave de la principal con las proporciones medias de todas las de frente y de cuerpo
/// entero (alturas y anchos relativos al alto de cada imagen).
fn averaged_keypoints(main: &Candidate, all: &[Candidate]) -> crate::analyze::Keypoints {
    let k = &main.analysis.keypoints;
    let usable: Vec<&Candidate> = all.iter().filter(|c| c.front() && c.full_body() && c.analysis.skin_by_color).collect();
    if usable.len() < 2 {
        return k.clone();
    }
    let tall = (k.feet - k.top) as f32;
    let mean = |f: &dyn Fn(&crate::analyze::Keypoints) -> f32| -> f32 { usable.iter().map(|c| f(&c.analysis.keypoints)).sum::<f32>() / usable.len() as f32 };
    let row = |f: fn(&crate::analyze::Keypoints) -> u32| -> u32 {
        let n = mean(&|q: &crate::analyze::Keypoints| (f(q).saturating_sub(q.top)) as f32 / (q.feet - q.top).max(1) as f32);
        k.top + (n * tall).round() as u32
    };
    let width = |f: fn(&crate::analyze::Keypoints) -> f32| mean(&|q: &crate::analyze::Keypoints| f(q) / (q.feet - q.top).max(1) as f32) * tall;
    let mut out = k.clone();
    out.chin = row(|q| q.chin);
    out.shoulders = row(|q| q.shoulders);
    out.waist = row(|q| q.waist);
    out.hips = row(|q| q.hips);
    out.crotch = row(|q| q.crotch);
    out.knees = row(|q| q.knees);
    out.face_width = width(|q| q.face_width);
    out.shoulder_width = width(|q| q.shoulder_width);
    out.waist_width = width(|q| q.waist_width);
    out.hip_width = width(|q| q.hip_width);
    out.heads_tall = tall / (out.chin - out.top).max(1) as f32;
    out
}

/// Una ilustración → personaje (ver `from_images`).
pub fn from_image(image_path: &Path, base: &Path, progress: &dyn Fn(String)) -> Result<ImageResult> {
    from_images(&[image_path.to_path_buf()], base, progress)
}

/// Varias ilustraciones del mismo personaje → personaje. La mejor de frente y de cuerpo entero
/// es la principal (figura y frente); las proporciones son la media de todas las de frente y de
/// cuerpo entero; una de espaldas (sin cara), si la hay, pinta la espalda.
pub fn from_images(paths: &[PathBuf], base: &Path, progress: &dyn Fn(String)) -> Result<ImageResult> {
    anyhow::ensure!(!paths.is_empty(), "no hay imágenes");
    let mut candidates = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("imagen").to_string();
        progress(format!("1/6 quitando el fondo de {name} ({}/{}, unos 20 s)…", i + 1, paths.len()));
        let image = match image::open(path) {
            Ok(image) => image.to_rgba8(),
            Err(error) => {
                progress(format!("    no se pudo abrir {name}: {error}"));
                continue;
            }
        };
        let cut = crate::cutout::cut(&image, &crate::cutout::default_model())?;
        progress(format!("2/6 analizando {name}…"));
        match crate::analyze::analyze(&cut) {
            Ok(analysis) => {
                let k = &analysis.keypoints;
                progress(format!(
                    "    {:.1} cabezas, {}{}",
                    k.heads_tall,
                    if k.face_width > 0.0 { "de frente" } else { "sin cara visible" },
                    if analysis.skin_by_color { "" } else { ", piel no distinguible por color" }
                ));
                candidates.push(Candidate { name, cut, analysis });
            }
            Err(error) => progress(format!("    {name}: {error:#}")),
        }
    }
    anyhow::ensure!(!candidates.is_empty(), "ninguna imagen tiene un personaje reconocible");
    let main_index = (0..candidates.len()).max_by(|a, b| candidates[*a].score().total_cmp(&candidates[*b].score())).unwrap();
    // De espaldas: sin cara, de cuerpo entero y con piel visible (si no, puede ser cualquier cosa).
    let mut back_index = (0..candidates.len()).find(|&i| i != main_index && !candidates[i].front() && candidates[i].full_body() && candidates[i].analysis.skin_by_color);
    // Depuración: A2W_ESPALDA=n usa la imagen n (desde 1) como espalda aunque tenga cara.
    if let Some(n) = std::env::var("A2W_ESPALDA").ok().and_then(|v| v.parse::<usize>().ok()) {
        back_index = n.checked_sub(1).filter(|i| *i < candidates.len());
    }
    let main = &candidates[main_index];
    progress(format!("    principal: {}{}", main.name, back_index.map_or(String::new(), |b| format!("; espalda: {}", candidates[b].name))));
    let analysis = crate::analyze::Analysis { keypoints: averaged_keypoints(main, &candidates), ..main.analysis.clone() };
    let cut = &main.cut;
    let image_path = &paths[0];
    progress("3/6 ajustando el cuerpo base…".into());
    let mut model = Model::load(base)?;
    let _ = image_path;
    model.name = std::path::Path::new(&main.name).file_stem().and_then(|s| s.to_str()).unwrap_or("personaje").to_string();
    let fit = crate::fit::fit(&mut model, &analysis.keypoints)?;
    progress(format!("    cabeza x{:.2}, hombros x{:.2}, cintura x{:.2}", fit.head, fit.shoulders, fit.waist));
    progress("4/6 pintando la imagen…".into());
    let projection = crate::fit::project(&mut model, cut, &analysis, &fit)?;
    let back = match back_index {
        Some(b) => Some(crate::fit::project_back(&mut model, &candidates[b].cut, &candidates[b].analysis, &projection)?),
        None => None,
    };
    // La cara de Base_Female, como piel para buscar ojos y boca.
    for layer in &mut model.layers {
        if layer.name.eq_ignore_ascii_case("face") {
            layer.name = "piel cara".into();
        }
    }
    progress("5/6 cara: párpados y boca…".into());
    face(&mut model, progress);
    progress("6/6 pelo y falda…".into());
    match crate::dress::add_hair(&mut model, cut, &analysis, &projection, back.as_ref()) {
        Ok((cap, sheets)) => progress(format!("    pelo: {cap} triángulos de casquete, {sheets} de melena")),
        Err(error) => progress(format!("    sin pelo: {error:#}")),
    }
    strands(&crate::hair::rig_hair(&mut model)?, progress);
    match crate::dress::add_skirt(&mut model, &analysis, &projection)? {
        0 => progress("    sin falda".into()),
        rows => progress(format!("    falda de {rows} filas de la imagen")),
    }
    strands(&crate::hair::rig_skirt(&mut model)?, progress);
    let images = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let role = if i == main_index {
                "frente (principal)"
            } else if Some(i) == back_index {
                "espalda"
            } else if c.front() && c.full_body() {
                "proporciones"
            } else {
                "no usada"
            };
            (format!("{}: {role}", c.name), c.cut.clone(), crate::analyze::debug_image(&c.cut, &c.analysis))
        })
        .collect();
    Ok(ImageResult { model, images })
}

/// Una malla de IA de una pieza → personaje por capas y riggeado. `props`: objetos marcados a
/// mano, rígidos con un hueso (ver `rig::Prop`).
pub fn from_ai_mesh(mesh: &Path, base: &Path, props: &[crate::rig::Prop], progress: &dyn Fn(String)) -> Result<Model> {
    let mut model = Model::load(mesh)?;
    let triangles = model.triangle_count();
    // Las mallas de IA traen a veces más de un millón de triángulos: se reducen con Blender.
    const TARGET: usize = 80_000;
    if triangles > 2 * TARGET {
        progress(format!("0/5 reduciendo {triangles} triángulos a {TARGET} con Blender…"));
        let script = resource("tools/blender/decimate.py");
        let reduced = std::env::temp_dir().join(format!("anime2waifu3d-reducida-{}.glb", std::process::id()));
        let status = std::process::Command::new("blender")
            .args(["-b", "--factory-startup", "--python"])
            .arg(&script)
            .arg("--")
            .arg(mesh)
            .arg(&reduced)
            .arg(TARGET.to_string())
            .output();
        match status {
            Ok(out) if reduced.exists() => {
                let _ = out;
                let name = model.name.clone();
                model = Model::load(&reduced)?;
                model.name = name;
                let _ = std::fs::remove_file(&reduced);
            }
            _ => progress("    no se pudo reducir (¿está Blender instalado?): sigo con la malla entera, será lento".into()),
        }
    }
    progress("1/5 separando en capas por color…".into());
    let segments = crate::segment::segment(&model, &crate::segment::Options::default());
    progress(format!("    {} capas", segments.len()));
    let mut model = crate::segment::into_model(model, &segments);
    progress("2/5 poniendo el esqueleto (más de un minuto)…".into());
    let base_model = Model::load(base)?;
    let report = crate::rig::rig(&mut model, &base_model, props)?;
    progress(format!("    escala {:.3}", report.scale));
    progress("3/5 pelo…".into());
    strands(&crate::hair::rig_hair(&mut model)?, progress);
    progress("4/5 falda…".into());
    strands(&crate::hair::rig_skirt(&mut model)?, progress);
    progress("5/5 cara…".into());
    face(&mut model, progress);
    Ok(model)
}

fn strands(report: &crate::hair::Report, progress: &dyn Fn(String)) {
    for s in &report.strands {
        progress(format!("    {:<22} {} huesos, {} vértices", s.name, s.bones, s.vertices));
    }
}

/// Párpados y boca; si no se encuentran los ojos, se avisa y se sigue.
fn face(model: &mut Model, progress: &dyn Fn(String)) {
    match crate::face::rig_face(model) {
        Ok(r) => {
            for eye in &r.eyes {
                progress(format!("    ojo en ({:.3}, {:.3}), {:.3} x {:.3}", eye.centre.x, eye.centre.y, eye.radius.x * 2.0, eye.radius.y * 2.0));
            }
            let (centre, width, found) = r.mouth;
            progress(format!("    boca en ({:.3}, {:.3}), {:.3} de ancho{}", centre.x, centre.y, width, if found { "" } else { " (supuesta)" }));
        }
        Err(error) => progress(format!("    sin párpados: {error:#}")),
    }
}

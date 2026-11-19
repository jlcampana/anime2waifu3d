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
    /// La imagen sin fondo y la de comprobación del análisis (imagen y partes, con líneas).
    pub cutout: RgbaImage,
    pub analysis: RgbaImage,
}

pub fn from_image(image_path: &Path, base: &Path, progress: &dyn Fn(String)) -> Result<ImageResult> {
    let image = image::open(image_path).with_context(|| format!("no se pudo abrir {}", image_path.display()))?.to_rgba8();
    progress("1/6 quitando el fondo (unos 20 s)…".into());
    let cut = crate::cutout::cut(&image, &crate::cutout::default_model())?;
    progress("2/6 analizando partes y proporciones…".into());
    let analysis = crate::analyze::analyze(&cut)?;
    let analysis_image = crate::analyze::debug_image(&cut, &analysis);
    let k = &analysis.keypoints;
    progress(format!(
        "    {:.1} cabezas de alto{}",
        k.heads_tall,
        if analysis.skin_by_color { "" } else { " (piel no distinguible por color: alturas supuestas)" }
    ));
    progress("3/6 ajustando el cuerpo base…".into());
    let mut model = Model::load(base)?;
    model.name = image_path.file_stem().and_then(|s| s.to_str()).unwrap_or("personaje").to_string();
    let fit = crate::fit::fit(&mut model, k)?;
    progress(format!("    cabeza x{:.2}, hombros x{:.2}, cintura x{:.2}", fit.head, fit.shoulders, fit.waist));
    progress("4/6 pintando la imagen…".into());
    let projection = crate::fit::project(&mut model, &cut, &analysis, &fit)?;
    // La cara de Base_Female, como piel para buscar ojos y boca.
    for layer in &mut model.layers {
        if layer.name.eq_ignore_ascii_case("face") {
            layer.name = "piel cara".into();
        }
    }
    progress("5/6 cara: párpados y boca…".into());
    face(&mut model, progress);
    progress("6/6 pelo y falda…".into());
    match crate::dress::add_hair(&mut model, &cut, &analysis, &projection) {
        Ok((cap, sheets)) => progress(format!("    pelo: {cap} triángulos de casquete, {sheets} de melena")),
        Err(error) => progress(format!("    sin pelo: {error:#}")),
    }
    strands(&crate::hair::rig_hair(&mut model)?, progress);
    match crate::dress::add_skirt(&mut model, &analysis, &projection)? {
        0 => progress("    sin falda".into()),
        rows => progress(format!("    falda de {rows} filas de la imagen")),
    }
    strands(&crate::hair::rig_skirt(&mut model)?, progress);
    Ok(ImageResult { model, cutout: cut, analysis: analysis_image })
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

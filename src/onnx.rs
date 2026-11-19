//! Modelos ONNX en local: ONNX Runtime del sistema (paquete `onnxruntime` de Fedora), cargado
//! al usarlo con el crate `ort`. Lo comparten el recorte (BiRefNet) y la profundidad (Depth
//! Anything V2).

use anyhow::{Context, Result};
use image::{imageops::FilterType, RgbaImage};
use std::path::{Path, PathBuf};

/// Media y desviación de ImageNet, con las que se normalizan las entradas de los dos modelos.
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// La biblioteca de ONNX Runtime: `ORT_DYLIB_PATH` o la del paquete del sistema.
fn runtime_library() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("ORT_DYLIB_PATH") {
        return Ok(PathBuf::from(path));
    }
    // La del paquete `onnxruntime` de Fedora.
    ["/usr/lib64/libonnxruntime.so", "/usr/lib64/libonnxruntime.so.1"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .context("no se encuentra ONNX Runtime: ejecuta scripts/instalar.sh (o `sudo dnf install onnxruntime`), o indica la biblioteca en ORT_DYLIB_PATH")
}

/// La imagen como entrada 1×3×alto×ancho, sobre blanco donde es transparente, normalizada con
/// ImageNet.
pub fn input(image: &RgbaImage, width: u32, height: u32) -> ndarray::Array4<f32> {
    let resized = image::imageops::resize(image, width, height, FilterType::Triangle);
    let mut input = ndarray::Array4::<f32>::zeros((1, 3, height as usize, width as usize));
    for (x, y, p) in resized.enumerate_pixels() {
        let a = p[3] as f32 / 255.0;
        for c in 0..3 {
            let v = (p[c] as f32 / 255.0) * a + (1.0 - a);
            input[[0, c, y as usize, x as usize]] = (v - MEAN[c]) / STD[c];
        }
    }
    input
}

/// El entorno de ONNX Runtime: solo puede haber uno por proceso, así que se crea la primera vez
/// y se reutiliza (varias imágenes seguidas, o crear otro personaje sin cerrar la app).
fn environment() -> Result<ort::environment::Environment> {
    static ENVIRONMENT: std::sync::Mutex<Option<ort::environment::Environment>> = std::sync::Mutex::new(None);
    let mut slot = ENVIRONMENT.lock().map_err(|_| anyhow::anyhow!("ONNX Runtime: entorno bloqueado"))?;
    if let Some(environment) = slot.as_ref() {
        return Ok(environment.clone());
    }
    let library = runtime_library()?;
    let environment = ort::init_from(&library)
        .map_err(|e| anyhow::anyhow!("no se pudo cargar {}: {e}", library.display()))?
        .build()
        .map_err(|e| anyhow::anyhow!("ONNX Runtime: {e}"))?;
    *slot = Some(environment.clone());
    Ok(environment)
}

/// Ejecuta un modelo de una entrada y devuelve su última salida: forma y datos.
pub fn run(model: &Path, input: ndarray::Array4<f32>) -> Result<(Vec<usize>, Vec<f32>)> {
    anyhow::ensure!(model.exists(), "no está el modelo {} (ver assets/modelos/)", model.display());
    let error = |e: &dyn std::fmt::Display| anyhow::anyhow!("ONNX Runtime: {e}");
    let environment = environment()?;
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut session = ort::session::Session::builder(&environment)
        .map_err(|e| error(&e))?
        .with_intra_threads(threads)
        .map_err(|e| error(&e))?
        .commit_from_file(model)
        .map_err(|e| anyhow::anyhow!("no se pudo abrir el modelo {}: {e}", model.display()))?;
    let tensor = ort::value::Tensor::from_array(input).map_err(|e| error(&e))?;
    let outputs = session.run(ort::inputs![tensor]).map_err(|e| error(&e))?;
    let (shape, data) = outputs[outputs.len() - 1].try_extract_tensor::<f32>().map_err(|e| error(&e))?;
    Ok((shape.iter().map(|d| *d as usize).collect(), data.to_vec()))
}

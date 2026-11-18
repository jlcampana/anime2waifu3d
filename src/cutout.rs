//! Ruta desde imagen, paso 1: quitar el fondo.
//!
//! BiRefNet ToonOut (MIT, BiRefNet afinado para anime) en ONNX, ejecutado en la CPU con ONNX
//! Runtime del sistema (paquete `onnxruntime` de Fedora), que se carga al usarlo: si no está, el
//! resto de la app funciona igual. Todo en local.
//!
//! Entrada del modelo: 1×3×1024×1024, RGB normalizado con la media y la desviación de ImageNet.
//! Salida: la máscara del personaje (logits; se pasa por una sigmoide), que se escala al tamaño
//! de la imagen y se usa como canal alfa. El resultado se recorta al personaje con un margen.

use anyhow::{Context, Result};
use image::{imageops::FilterType, RgbaImage};
use std::path::{Path, PathBuf};

const SIZE: u32 = 1024;
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// Dónde está el modelo por defecto (se descarga aparte; `assets/` no va en git).
pub fn default_model() -> PathBuf {
    PathBuf::from("assets/modelos/birefnet-toonout.onnx")
}

/// La biblioteca de ONNX Runtime: `ORT_DYLIB_PATH` o la del paquete del sistema.
fn runtime_library() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("ORT_DYLIB_PATH") {
        return Ok(PathBuf::from(path));
    }
    ["/usr/lib64/libonnxruntime.so", "/usr/lib64/libonnxruntime.so.1", "/usr/lib/x86_64-linux-gnu/libonnxruntime.so", "/usr/lib/libonnxruntime.so"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .context("no se encuentra ONNX Runtime: instálalo con `sudo dnf install onnxruntime` (Fedora) o indica la biblioteca en ORT_DYLIB_PATH")
}

/// Máscara del personaje (0..1) del tamaño de la imagen.
pub fn mask(image: &RgbaImage, model: &Path) -> Result<image::ImageBuffer<image::Luma<f32>, Vec<f32>>> {
    anyhow::ensure!(model.exists(), "no está el modelo {}: descárgalo de huggingface.co/sprited/birefnet-toonout-onnx", model.display());
    let library = runtime_library()?;
    let environment = ort::init_from(&library)
        .map_err(|e| anyhow::anyhow!("no se pudo cargar {}: {e}", library.display()))?
        .build()
        .map_err(|e| anyhow::anyhow!("ONNX Runtime: {e}"))?;

    // Entrada: la imagen a 1024×1024 sobre blanco (lo transparente, si lo hay, como fondo).
    let resized = image::imageops::resize(image, SIZE, SIZE, FilterType::Triangle);
    let mut input = ndarray::Array4::<f32>::zeros((1, 3, SIZE as usize, SIZE as usize));
    for (x, y, p) in resized.enumerate_pixels() {
        let a = p[3] as f32 / 255.0;
        for c in 0..3 {
            let v = (p[c] as f32 / 255.0) * a + (1.0 - a);
            input[[0, c, y as usize, x as usize]] = (v - MEAN[c]) / STD[c];
        }
    }
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let error = |e: &dyn std::fmt::Display| anyhow::anyhow!("ONNX Runtime: {e}");
    let mut session = ort::session::Session::builder(&environment)
        .map_err(|e| error(&e))?
        .with_intra_threads(threads)
        .map_err(|e| error(&e))?
        .commit_from_file(model)
        .map_err(|e| anyhow::anyhow!("no se pudo abrir el modelo {}: {e}", model.display()))?;
    let tensor = ort::value::Tensor::from_array(input).map_err(|e| error(&e))?;
    let outputs = session.run(ort::inputs![tensor]).map_err(|e| error(&e))?;
    // La última salida es la de más resolución.
    let last = outputs.len() - 1;
    let (shape, data) = outputs[last].try_extract_tensor::<f32>().map_err(|e| error(&e))?;
    let (h, w) = (shape[shape.len() - 2] as u32, shape[shape.len() - 1] as u32);
    // Logits o probabilidades: si hay valores fuera de 0..1, son logits.
    let logits = data.iter().any(|v| *v < -0.01 || *v > 1.01);
    if std::env::var("A2W_DEBUG_CUTOUT").is_ok() {
        let (lo, hi) = data.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
        let mean = data.iter().sum::<f32>() / data.len() as f32;
        eprintln!("salidas {} forma {:?} min {lo} max {hi} media {mean} logits {logits}", outputs.len(), shape);
    }
    let small = image::ImageBuffer::<image::Luma<f32>, Vec<f32>>::from_fn(w, h, |x, y| {
        let v = data[(y * w + x) as usize];
        image::Luma([if logits { 1.0 / (1.0 + (-v).exp()) } else { v }])
    });
    Ok(image::imageops::resize(&small, image.width(), image.height(), FilterType::Triangle))
}

/// La imagen sin fondo, recortada al personaje con un margen del 3 %.
pub fn cut(image: &RgbaImage, model: &Path) -> Result<RgbaImage> {
    let mask = mask(image, model)?;
    let mut out = image.clone();
    let keep = main_pieces(&mask);
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in out.enumerate_pixels_mut() {
        let m = if keep[(y * mask.width() + x) as usize] { mask.get_pixel(x, y)[0].clamp(0.0, 1.0) } else { 0.0 };
        p[3] = ((p[3] as f32) * m).round() as u8;
        if p[3] > 16 {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    anyhow::ensure!(x1 > x0 && y1 > y0, "no se encuentra ningún personaje en la imagen");
    let margin = ((x1 - x0).max(y1 - y0) as f32 * 0.03) as u32;
    let (x0, y0) = (x0.saturating_sub(margin), y0.saturating_sub(margin));
    let (x1, y1) = ((x1 + margin).min(out.width() - 1), (y1 + margin).min(out.height() - 1));
    Ok(image::imageops::crop_imm(&out, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image())
}

/// Qué píxeles quedan: la pieza más grande de la máscara (el personaje) y las sueltas de al
/// menos el 10 % de su tamaño (un mechón o una mano separados). Las pequeñas (las letras de una
/// portada, motas) se van. Las piezas incluyen el borde suave que las rodea.
fn main_pieces(mask: &image::ImageBuffer<image::Luma<f32>, Vec<f32>>) -> Vec<bool> {
    let (w, h) = (mask.width() as usize, mask.height() as usize);
    let solid: Vec<bool> = mask.pixels().map(|p| p[0] > 0.5).collect();
    let soft: Vec<bool> = mask.pixels().map(|p| p[0] > 0.02).collect();
    // Piezas sobre la máscara suave: así el borde semitransparente va con su pieza.
    let mut label = vec![usize::MAX; w * h];
    let mut sizes: Vec<usize> = Vec::new();
    for start in 0..w * h {
        if !soft[start] || label[start] != usize::MAX {
            continue;
        }
        let id = sizes.len();
        let mut stack = vec![start];
        label[start] = id;
        let mut size = 0;
        while let Some(i) = stack.pop() {
            size += solid[i] as usize;
            let (x, y) = (i % w, i / w);
            let mut visit = |n: usize| {
                if soft[n] && label[n] == usize::MAX {
                    label[n] = id;
                    stack.push(n);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        sizes.push(size);
    }
    let largest = sizes.iter().copied().max().unwrap_or(0);
    label.iter().map(|&l| l != usize::MAX && sizes[l] * 10 >= largest).collect()
}

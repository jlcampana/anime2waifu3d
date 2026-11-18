//! Ruta desde imagen, paso 2a: profundidad.
//!
//! Depth Anything V2 small (Apache-2.0) en ONNX, en la CPU. Da profundidad relativa inversa
//! (más es más cerca) de la imagen recortada; se normaliza a 0..1 dentro del personaje (1, lo
//! más cercano) y fuera vale 0. Sirve para dar volumen al pelo y a la ropa y para saber qué
//! parte va delante de cuál.

use anyhow::Result;
use image::{imageops::FilterType, ImageBuffer, Luma, RgbaImage};
use std::path::{Path, PathBuf};

/// Lado largo de la entrada (el del modelo), en múltiplos de 14.
const SIZE: u32 = 518;

pub fn default_model() -> PathBuf {
    PathBuf::from("assets/modelos/depth-anything-v2-small.onnx")
}

pub type DepthMap = ImageBuffer<Luma<f32>, Vec<f32>>;

pub fn depth(image: &RgbaImage, model: &Path) -> Result<DepthMap> {
    // Proporción de la imagen, con los dos lados en múltiplos de 14.
    let scale = SIZE as f32 / image.width().max(image.height()) as f32;
    let side = |v: u32| (((v as f32 * scale) / 14.0).round().max(1.0) * 14.0) as u32;
    let (w, h) = (side(image.width()), side(image.height()));
    let (shape, data) = crate::onnx::run(model, crate::onnx::input(image, w, h))?;
    let (oh, ow) = (shape[shape.len() - 2] as u32, shape[shape.len() - 1] as u32);
    // A 0..1 antes de escalar: `resize` recorta los f32 a ese rango.
    let (lo, hi) = data.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    let span = (hi - lo).max(1e-6);
    let raw = DepthMap::from_fn(ow, oh, |x, y| Luma([(data[(y * ow + x) as usize] - lo) / span]));
    let full = image::imageops::resize(&raw, image.width(), image.height(), FilterType::Triangle);
    // Normalizada dentro del personaje (percentiles 1 y 99, para que un píxel raro no mande).
    let mut inside: Vec<f32> = full.pixels().zip(image.pixels()).filter(|(_, p)| p[3] > 128).map(|(d, _)| d[0]).collect();
    anyhow::ensure!(!inside.is_empty(), "la imagen no tiene personaje (¿está recortada?)");
    inside.sort_by(f32::total_cmp);
    let low = inside[inside.len() / 100];
    let high = inside[inside.len() * 99 / 100];
    let range = (high - low).max(1e-6);
    Ok(DepthMap::from_fn(image.width(), image.height(), |x, y| {
        let alpha = image.get_pixel(x, y)[3] as f32 / 255.0;
        Luma([((full.get_pixel(x, y)[0] - low) / range).clamp(0.0, 1.0) * alpha])
    }))
}

/// Imagen de la profundidad en grises de 16 bits, para mirarla o usarla en otros programas.
pub fn save(depth: &DepthMap, path: &Path) -> Result<()> {
    let image = ImageBuffer::<Luma<u16>, Vec<u16>>::from_fn(depth.width(), depth.height(), |x, y| {
        Luma([(depth.get_pixel(x, y)[0] * 65535.0).round() as u16])
    });
    image.save(path)?;
    Ok(())
}

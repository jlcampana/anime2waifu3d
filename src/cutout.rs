//! Ruta desde imagen, paso 1: quitar el fondo.
//!
//! BiRefNet ToonOut (MIT, BiRefNet afinado para anime) en ONNX, ejecutado en la CPU con ONNX
//! Runtime del sistema (paquete `onnxruntime` de Fedora), que se carga al usarlo: si no está, el
//! resto de la app funciona igual. Todo en local.
//!
//! Entrada del modelo: 1×3×1024×1024, RGB normalizado con la media y la desviación de ImageNet.
//! Salida: la máscara del personaje (logits; se pasa por una sigmoide), que se escala al tamaño
//! de la imagen y se usa como canal alfa. El resultado se recorta al personaje con un margen.

use anyhow::Result;
use image::{imageops::FilterType, RgbaImage};
use std::path::{Path, PathBuf};

const SIZE: u32 = 1024;

/// Dónde está el modelo por defecto (se descarga aparte; `assets/` no va en git).
pub fn default_model() -> PathBuf {
    crate::pipeline::resource("assets/modelos/birefnet-toonout.onnx")
}

/// Máscara del personaje (0..1) del tamaño de la imagen.
pub fn mask(image: &RgbaImage, model: &Path) -> Result<image::ImageBuffer<image::Luma<f32>, Vec<f32>>> {
    let (shape, data) = crate::onnx::run(model, crate::onnx::input(image, SIZE, SIZE))?;
    let (h, w) = (shape[shape.len() - 2] as u32, shape[shape.len() - 1] as u32);
    // Logits o probabilidades: si hay valores fuera de 0..1, son logits.
    let logits = data.iter().any(|v| *v < -0.01 || *v > 1.01);
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

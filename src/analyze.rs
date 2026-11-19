//! Ruta desde imagen, paso 2b: partes y puntos clave de la imagen recortada.
//!
//! **Partes.** Los colores del personaje se agrupan en Lab (k-means) y cada grupo se clasifica:
//! - piel: tonos de piel anime (claros o morenos: luminosidad media o alta, rojizo-anaranjado y
//!   con algo de color);
//! - pelo: el grupo que domina la parte de arriba de la silueta y no es piel, conectado a ella;
//! - calzado: lo que no es piel en el último 6 % de la altura;
//! - ropa: el resto.
//!
//! En imágenes en blanco y negro la piel no se distingue por color: se avisa y todo lo que no
//! es pelo queda como ropa.
//!
//! **Puntos clave**, de la silueta sin el pelo (el pelo largo tapa los hombros y la cintura):
//! coronilla, barbilla (por la cara), hombros, cintura (lo más estrecho del tronco), cadera, la
//! entrepierna (donde se separan las piernas), rodillas y pies. Con ellos, las proporciones:
//! cuántas cabezas mide y los anchos relativos, para ajustar Base_Female en el paso 3.

use crate::face::lab;
use anyhow::Result;
use glam::Vec3;
use image::{Rgba, RgbaImage};
use serde::Serialize;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
pub enum Part {
    Fondo,
    Pelo,
    Piel,
    Ropa,
    Calzado,
}

impl Part {
    fn color(self) -> [u8; 3] {
        match self {
            Part::Fondo => [40, 40, 60],
            Part::Pelo => [230, 60, 160],
            Part::Piel => [250, 200, 120],
            Part::Ropa => [70, 140, 230],
            Part::Calzado => [90, 200, 90],
        }
    }
}

/// Puntos clave, en píxeles de la imagen (y hacia abajo). Los anchos son de la silueta sin pelo.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Keypoints {
    pub top: u32,
    pub chin: u32,
    pub shoulders: u32,
    pub waist: u32,
    pub hips: u32,
    pub crotch: u32,
    pub knees: u32,
    pub feet: u32,
    pub centre_x: f32,
    pub face_width: f32,
    pub shoulder_width: f32,
    pub waist_width: f32,
    pub hip_width: f32,
    /// Alto total entre alto de la cabeza (coronilla a barbilla).
    pub heads_tall: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub width: u32,
    pub height: u32,
    /// Fracción del personaje de cada parte.
    pub parts: Vec<(Part, f32)>,
    pub keypoints: Keypoints,
    /// Si la piel se pudo distinguir por color.
    pub skin_by_color: bool,
    #[serde(skip)]
    pub labels: Vec<Part>,
}

/// k-means en Lab sobre una muestra; devuelve los centros.
fn kmeans(samples: &[Vec3], k: usize) -> Vec<Vec3> {
    // Arranque: el primero y luego siempre el más lejano a los elegidos (determinista).
    let mut centres = vec![samples[0]];
    while centres.len() < k.min(samples.len()) {
        let far = samples
            .iter()
            .max_by(|a, b| {
                let da = centres.iter().map(|c| c.distance_squared(**a)).fold(f32::MAX, f32::min);
                let db = centres.iter().map(|c| c.distance_squared(**b)).fold(f32::MAX, f32::min);
                da.total_cmp(&db)
            })
            .copied()
            .unwrap();
        centres.push(far);
    }
    for _ in 0..15 {
        let mut sum = vec![Vec3::ZERO; centres.len()];
        let mut count = vec![0usize; centres.len()];
        for s in samples {
            let c = nearest(&centres, *s);
            sum[c] += *s;
            count[c] += 1;
        }
        for c in 0..centres.len() {
            if count[c] > 0 {
                centres[c] = sum[c] / count[c] as f32;
            }
        }
    }
    centres
}

fn nearest(centres: &[Vec3], p: Vec3) -> usize {
    (0..centres.len()).min_by(|a, b| centres[*a].distance_squared(p).total_cmp(&centres[*b].distance_squared(p))).unwrap()
}

/// Tono de piel anime: luminosidad media o alta, rojizo-anaranjado, con algo de color.
fn skin_like(c: Vec3) -> bool {
    let chroma = (c.y * c.y + c.z * c.z).sqrt();
    let hue = c.z.atan2(c.y).to_degrees();
    (35.0..=97.0).contains(&c.x) && (5.0..=55.0).contains(&chroma) && (15.0..=90.0).contains(&hue)
}

pub fn analyze(image: &RgbaImage) -> Result<Analysis> {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let inside: Vec<bool> = image.pixels().map(|p| p[3] > 128).collect();
    let colors: Vec<Vec3> = image.pixels().map(|p| lab([p[0], p[1], p[2]].map(|v| v as f32 / 255.0))).collect();
    let (mut top, mut bottom) = (usize::MAX, 0);
    for y in 0..h {
        if (0..w).any(|x| inside[y * w + x]) {
            top = top.min(y);
            bottom = bottom.max(y);
        }
    }
    anyhow::ensure!(top < bottom, "la imagen no tiene personaje (¿está recortada?)");
    let tall = (bottom - top) as f32;

    // Grupos de color.
    let step = ((w * h) as f32 / 40000.0).sqrt().max(1.0) as usize;
    let samples: Vec<Vec3> = (0..h).step_by(step).flat_map(|y| (0..w).step_by(step).map(move |x| (x, y))).filter(|(x, y)| inside[y * w + x]).map(|(x, y)| colors[y * w + x]).collect();
    let centres = kmeans(&samples, 8);
    let cluster: Vec<usize> = colors.iter().map(|c| nearest(&centres, *c)).collect();

    // Piel: grupos con tono de piel. Pelo: el grupo más frecuente en el 8 % de arriba que no es
    // piel, y luego lo de ese grupo conectado a esa zona.
    let skin_cluster: Vec<bool> = centres.iter().map(|c| skin_like(*c)).collect();
    let skin_by_color = skin_cluster.iter().any(|s| *s);
    let mut top_count = vec![0usize; centres.len()];
    let band = top + (tall * 0.08) as usize;
    for y in top..=band {
        for x in 0..w {
            if inside[y * w + x] && !skin_cluster[cluster[y * w + x]] {
                top_count[cluster[y * w + x]] += 1;
            }
        }
    }
    let hair_cluster = (0..centres.len()).max_by_key(|c| top_count[*c]).unwrap();
    // El pelo puede tener brillos de otro grupo parecido: valen los grupos cercanos al del pelo.
    let hair_like: Vec<bool> = centres.iter().map(|c| c.distance(centres[hair_cluster]) < 22.0 && !skin_like(*c)).collect();
    let mut hair = vec![false; w * h];
    let mut stack: Vec<usize> = (top..=band).flat_map(|y| (0..w).map(move |x| y * w + x)).filter(|&i| inside[i] && hair_like[cluster[i]]).collect();
    for &i in &stack {
        hair[i] = true;
    }
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                let n = ny as usize * w + nx as usize;
                if !hair[n] && inside[n] && hair_like[cluster[n]] {
                    hair[n] = true;
                    stack.push(n);
                }
            }
        }
    }
    let shoes_from = bottom - (tall * 0.06) as usize;
    let labels: Vec<Part> = (0..w * h)
        .map(|i| {
            if !inside[i] {
                Part::Fondo
            } else if hair[i] {
                Part::Pelo
            } else if skin_cluster[cluster[i]] {
                Part::Piel
            } else if i / w >= shoes_from {
                Part::Calzado
            } else {
                Part::Ropa
            }
        })
        .collect();

    // Puntos clave sobre la silueta sin pelo: el tramo continuo que contiene el centro.
    let body = |x: usize, y: usize| inside[y * w + x] && labels[y * w + x] != Part::Pelo;
    let centre_x = {
        let (sum, n) = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|(x, y)| body(*x, *y)).fold((0.0, 0usize), |(s, n), (x, _)| (s + x as f32, n + 1));
        sum / n.max(1) as f32
    };
    let cx = centre_x.round() as usize;
    let run = |y: usize| -> Option<(usize, usize)> {
        if !body(cx, y) {
            return None;
        }
        let (mut l, mut r) = (cx, cx);
        while l > 0 && body(l - 1, y) {
            l -= 1;
        }
        while r + 1 < w && body(r + 1, y) {
            r += 1;
        }
        Some((l, r))
    };
    let width_at = |y: usize| run(y).map_or(0.0, |(l, r)| (r - l + 1) as f32);
    // La cara: la mayor zona de piel en el 30 % de arriba; la barbilla, su parte más baja.
    let face_limit = top + (tall * 0.3) as usize;
    let mut chin = top + (tall / 7.0) as usize;
    let mut face_width = 0.0;
    if skin_by_color {
        let mut seen = vec![false; w * h];
        let mut best: Vec<usize> = Vec::new();
        for start in top * w..face_limit * w {
            if seen[start] || labels[start] != Part::Piel {
                continue;
            }
            let mut stack = vec![start];
            seen[start] = true;
            let mut blob = Vec::new();
            while let Some(i) = stack.pop() {
                blob.push(i);
                let (x, y) = (i % w, i / w);
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < face_limit {
                        let n = ny as usize * w + nx as usize;
                        if !seen[n] && labels[n] == Part::Piel {
                            seen[n] = true;
                            stack.push(n);
                        }
                    }
                }
            }
            if blob.len() > best.len() {
                best = blob;
            }
        }
        if best.len() > 50 {
            // Ancho de la mancha por filas: la barbilla es donde, bajando desde lo más ancho de la
            // cara, se estrecha hacia el cuello (la piel del cuello y el escote va pegada).
            let mut rows: std::collections::BTreeMap<usize, (usize, usize)> = std::collections::BTreeMap::new();
            for i in &best {
                let (x, y) = (i % w, i / w);
                let e = rows.entry(y).or_insert((x, x));
                e.0 = e.0.min(x);
                e.1 = e.1.max(x);
            }
            let widths: Vec<(usize, f32)> = rows.iter().map(|(y, (l, r))| (*y, (r - l + 1) as f32)).collect();
            let (widest_at, widest) = widths.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
            face_width = widest;
            chin = widths.iter().find(|(y, wd)| *y > widest_at && *wd < widest * 0.6).map_or(widths.last().unwrap().0, |(y, _)| *y);
        }
    }
    let head = (chin - top).max(1) as f32;
    // Hombros: donde el tronco se ensancha bajo la barbilla (lo más ancho en media cabeza).
    let shoulders_end = (chin + (head * 0.7) as usize).min(bottom);
    let shoulders = (chin..=shoulders_end).max_by(|a, b| width_at(*a).total_cmp(&width_at(*b))).unwrap_or(chin);
    let shoulder_width = width_at(shoulders);
    // Entrepierna: la primera fila, ya bien por debajo de los hombros, en la que el centro deja de
    // ser cuerpo (se separan las piernas); si la falda lo tapa, el final de la falda.
    let from = shoulders + (head * 1.5) as usize;
    let crotch = (from..bottom).find(|y| !body(cx, *y) && (0..w).any(|x| body(x, *y))).unwrap_or(top + (tall * 0.5) as usize);
    // Cintura: lo más estrecho entre los hombros y la entrepierna (sin la cabeza de más abajo).
    let waist_from = shoulders + (head * 0.8) as usize;
    let waist_to = crotch.saturating_sub((head * 0.3) as usize).max(waist_from + 1);
    let waist = (waist_from..waist_to).filter(|y| width_at(*y) > 0.0).min_by(|a, b| width_at(*a).total_cmp(&width_at(*b))).unwrap_or(waist_from);
    // Cadera: lo más ancho entre la cintura y la entrepierna.
    let hips = (waist..crotch.max(waist + 1)).max_by(|a, b| width_at(*a).total_cmp(&width_at(*b))).unwrap_or(waist);
    let knees = crotch + (bottom - crotch) / 2;

    let mut parts: Vec<(Part, f32)> = [Part::Pelo, Part::Piel, Part::Ropa, Part::Calzado]
        .iter()
        .map(|p| (*p, labels.iter().filter(|l| *l == p).count() as f32))
        .collect();
    let total: f32 = parts.iter().map(|(_, n)| n).sum();
    parts.iter_mut().for_each(|(_, n)| *n /= total.max(1.0));

    Ok(Analysis {
        width: w as u32,
        height: h as u32,
        parts,
        skin_by_color,
        keypoints: Keypoints {
            top: top as u32,
            chin: chin as u32,
            shoulders: shoulders as u32,
            waist: waist as u32,
            hips: hips as u32,
            crotch: crotch as u32,
            knees: knees as u32,
            feet: bottom as u32,
            centre_x,
            face_width,
            shoulder_width,
            waist_width: width_at(waist),
            hip_width: width_at(hips),
            heads_tall: tall / head,
        },
        labels,
    })
}

/// Imagen de comprobación: la imagen a la izquierda y las partes a la derecha, con una línea
/// por punto clave (coronilla y pies blancos, barbilla amarilla, hombros rojos, cintura verde,
/// cadera azul, entrepierna magenta, rodillas cian).
pub fn debug_image(image: &RgbaImage, analysis: &Analysis) -> RgbaImage {
    let (w, h) = (image.width(), image.height());
    let mut out = RgbaImage::from_pixel(w * 2, h, Rgba([40, 40, 60, 255]));
    for y in 0..h {
        for x in 0..w {
            let p = image.get_pixel(x, y);
            let a = p[3] as f32 / 255.0;
            let mix = |c: u8, bg: u8| (c as f32 * a + bg as f32 * (1.0 - a)) as u8;
            out.put_pixel(x, y, Rgba([mix(p[0], 40), mix(p[1], 40), mix(p[2], 60), 255]));
            let c = analysis.labels[(y * w + x) as usize].color();
            out.put_pixel(w + x, y, Rgba([c[0], c[1], c[2], 255]));
        }
    }
    let k = &analysis.keypoints;
    for (y, c) in [
        (k.top, [255, 255, 255]),
        (k.chin, [255, 230, 0]),
        (k.shoulders, [255, 0, 0]),
        (k.waist, [0, 255, 0]),
        (k.hips, [0, 80, 255]),
        (k.crotch, [255, 0, 255]),
        (k.knees, [0, 255, 255]),
        (k.feet, [255, 255, 255]),
    ] {
        for x in 0..w * 2 {
            for t in 0..3 {
                if y + t < h {
                    out.put_pixel(x, y + t, Rgba([c[0], c[1], c[2], 255]));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skin_tones_are_recognised() {
        let skin = |rgb: [u8; 3]| skin_like(lab(rgb.map(|v| v as f32 / 255.0)));
        assert!(skin([250, 220, 200]), "piel clara");
        assert!(skin([200, 140, 100]), "piel morena");
        assert!(!skin([255, 255, 255]), "camisa blanca");
        assert!(!skin([30, 40, 80]), "falda azul marino");
        assert!(!skin([20, 20, 20]), "pelo negro");
    }

    #[test]
    fn kmeans_separates_clear_groups() {
        let mut samples = vec![Vec3::new(10.0, 0.0, 0.0); 50];
        samples.extend(vec![Vec3::new(90.0, 0.0, 0.0); 50]);
        let centres = kmeans(&samples, 2);
        let mut l: Vec<f32> = centres.iter().map(|c| c.x).collect();
        l.sort_by(f32::total_cmp);
        assert!((l[0] - 10.0).abs() < 1e-3 && (l[1] - 90.0).abs() < 1e-3, "{l:?}");
    }
}

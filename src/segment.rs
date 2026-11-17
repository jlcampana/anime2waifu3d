//! Paso 2 de la ruta "importar malla de IA": separa una malla de una sola pieza en capas
//! (pelo, piel, ropa, calzado, accesorios) por el color de su textura y por vecindad.
//!
//! 1. Color de cada triángulo, leyendo la textura en varios puntos de la cara, en Lab.
//! 2. k-means ponderado por área sobre esos colores.
//! 3. Limpieza: filtro de mayoría entre caras vecinas (soldando los vértices duplicados por las
//!    costuras de UV) y absorción de trozos pequeños en el vecino con más borde compartido.
//! 4. Regiones: cada trozo conexo es una capa, y los trozos vecinos se unen si en su frontera el
//!    color cambia poco (degradados).
//! 5. Nombres por pistas: la piel por su tono, el pelo por la altura, el calzado por estar abajo,
//!    y la zona del cuerpo por la posición.

use crate::model::{Model, Vertex};
use glam::{Vec2, Vec3};
use std::collections::HashMap;

pub struct Options {
    /// Número de grupos de color.
    pub clusters: usize,
    /// Pasadas del filtro de mayoría.
    pub smooth_passes: usize,
    /// Los trozos con menos área que esta fracción del total se absorben.
    pub min_component: f32,
    /// Diferencia de color media (ΔE en Lab) en la frontera por debajo de la cual se unen dos
    /// trozos vecinos.
    pub merge_delta: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self { clusters: 8, smooth_passes: 3, min_component: 0.002, merge_delta: 12.0 }
    }
}

/// Una cara de la malla, con todo lo que necesita la segmentación.
struct Face {
    /// Primitiva de origen (capa, índice dentro de la capa) e índice del triángulo.
    source: (usize, usize, usize),
    lab: [f32; 3],
    rgb: [f32; 3],
    area: f32,
    center: Vec3,
}

pub struct Segment {
    /// Pelo, Piel, Ropa, Calzado o Accesorios: el grupo de la paleta.
    pub class: String,
    pub name: String,
    /// Color medio en sRGB.
    pub color: [u8; 3],
    pub area_fraction: f32,
    /// Caras de cada primitiva de origen: (capa, primitiva) -> triángulos.
    pub faces: Vec<((usize, usize), Vec<usize>)>,
}

pub fn segment(model: &Model, options: &Options) -> Vec<Segment> {
    let faces = collect_faces(model);
    if faces.is_empty() {
        return Vec::new();
    }
    let neighbors = face_neighbors(model, &faces);
    let mut labels = kmeans(&faces, options.clusters);
    for _ in 0..options.smooth_passes {
        labels = majority_filter(&labels, &neighbors);
    }
    absorb_small_components(&faces, &neighbors, &mut labels, options.min_component);
    let regions = merge_regions(&faces, &neighbors, &labels, options.merge_delta);
    build_segments(&faces, &regions)
}

fn collect_faces(model: &Model) -> Vec<Face> {
    let mut faces = Vec::new();
    for (li, layer) in model.layers.iter().enumerate() {
        for (pi, primitive) in layer.primitives.iter().enumerate() {
            let material = &model.materials[primitive.material];
            let texture = material.texture.and_then(|t| model.textures.get(t));
            for (ti, tri) in primitive.indices.chunks_exact(3).enumerate() {
                let v: [&Vertex; 3] = [
                    &primitive.vertices[tri[0] as usize],
                    &primitive.vertices[tri[1] as usize],
                    &primitive.vertices[tri[2] as usize],
                ];
                let p = v.map(|v| Vec3::from(v.position));
                let uv = v.map(|v| Vec2::from(v.uv));
                let area = (p[1] - p[0]).cross(p[2] - p[0]).length() * 0.5;
                // Cuatro muestras: el centro y un punto hacia cada esquina.
                let mut linear = Vec3::ZERO;
                for w in [[1.0 / 3.0; 3], [2.0 / 3.0, 1.0 / 6.0, 1.0 / 6.0], [1.0 / 6.0, 2.0 / 3.0, 1.0 / 6.0], [1.0 / 6.0, 1.0 / 6.0, 2.0 / 3.0]] {
                    let at = uv[0] * w[0] + uv[1] * w[1] + uv[2] * w[2];
                    let c = match texture {
                        Some(t) => sample(t, at),
                        None => [material.base_color[0], material.base_color[1], material.base_color[2]].map(linear_to_srgb),
                    };
                    linear += Vec3::from(c.map(srgb_to_linear));
                }
                let rgb = (linear / 4.0).to_array().map(linear_to_srgb);
                faces.push(Face {
                    source: (li, pi, ti),
                    lab: srgb_to_lab(rgb),
                    rgb,
                    area,
                    center: (p[0] + p[1] + p[2]) / 3.0,
                });
            }
        }
    }
    faces
}

/// Color sRGB (0..1) de la textura en `uv`, con repetición y el píxel más cercano.
fn sample(texture: &crate::model::Texture, uv: Vec2) -> [f32; 3] {
    let x = ((uv.x.rem_euclid(1.0)) * texture.width as f32) as u32 % texture.width;
    let y = ((uv.y.rem_euclid(1.0)) * texture.height as f32) as u32 % texture.height;
    let i = ((y * texture.width + x) * 4) as usize;
    [texture.rgba[i], texture.rgba[i + 1], texture.rgba[i + 2]].map(|c| c as f32 / 255.0)
}

/// Vecinos de cada cara por aristas compartidas, soldando los vértices que están en el mismo
/// sitio (las mallas generadas duplican los vértices en las costuras de UV).
fn face_neighbors(model: &Model, faces: &[Face]) -> Vec<Vec<u32>> {
    let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for f in faces {
        low = low.min(f.center);
        high = high.max(f.center);
    }
    let cell = (high - low).max_element().max(1e-6) * 1e-6;
    let key = |p: [f32; 3]| p.map(|c| (c / cell).round() as i64);
    let mut welded: HashMap<[i64; 3], u32> = HashMap::new();
    let mut edges: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
    for (fi, face) in faces.iter().enumerate() {
        let (li, pi, ti) = face.source;
        let primitive = &model.layers[li].primitives[pi];
        let ids = [0, 1, 2].map(|k| {
            let v = &primitive.vertices[primitive.indices[ti * 3 + k] as usize];
            let next = welded.len() as u32;
            *welded.entry(key(v.position)).or_insert(next)
        });
        for (a, b) in [(ids[0], ids[1]), (ids[1], ids[2]), (ids[2], ids[0])] {
            if a != b {
                edges.entry((a.min(b), a.max(b))).or_default().push(fi as u32);
            }
        }
    }
    let mut neighbors = vec![Vec::new(); faces.len()];
    for list in edges.values() {
        for &a in list {
            for &b in list {
                if a != b {
                    neighbors[a as usize].push(b);
                }
            }
        }
    }
    neighbors
}

/// k-means en Lab, ponderado por área, con arranque k-means++ determinista.
fn kmeans(faces: &[Face], k: usize) -> Vec<usize> {
    let k = k.clamp(1, faces.len());
    let dist = |a: &[f32; 3], b: &[f32; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
    // k-means++ con un generador fijo, para que el resultado se repita.
    let mut rng = 0x2545F4914F6CDD1Du64;
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 11) as f64 / (1u64 << 53) as f64
    };
    let heaviest = faces.iter().enumerate().max_by(|a, b| a.1.area.total_cmp(&b.1.area)).map(|(i, _)| i).unwrap_or(0);
    let mut centers = vec![faces[heaviest].lab];
    let mut nearest: Vec<f32> = faces.iter().map(|f| dist(&f.lab, &centers[0])).collect();
    while centers.len() < k {
        let total: f64 = faces.iter().zip(&nearest).map(|(f, d)| (*d * f.area) as f64).sum();
        let mut pick = next() * total;
        let mut chosen = faces.len() - 1;
        for (i, (f, d)) in faces.iter().zip(&nearest).enumerate() {
            pick -= (*d * f.area) as f64;
            if pick <= 0.0 {
                chosen = i;
                break;
            }
        }
        centers.push(faces[chosen].lab);
        for (i, f) in faces.iter().enumerate() {
            nearest[i] = nearest[i].min(dist(&f.lab, centers.last().unwrap()));
        }
    }
    let mut labels = vec![0; faces.len()];
    for _ in 0..30 {
        let mut changed = false;
        for (i, f) in faces.iter().enumerate() {
            let best = (0..k).min_by(|&a, &b| dist(&f.lab, &centers[a]).total_cmp(&dist(&f.lab, &centers[b]))).unwrap();
            changed |= labels[i] != best;
            labels[i] = best;
        }
        let mut sums = vec![([0.0f64; 3], 0.0f64); k];
        for (f, &l) in faces.iter().zip(&labels) {
            for c in 0..3 {
                sums[l].0[c] += (f.lab[c] * f.area) as f64;
            }
            sums[l].1 += f.area as f64;
        }
        for (c, (sum, weight)) in centers.iter_mut().zip(sums) {
            if weight > 0.0 {
                *c = sum.map(|s| (s / weight) as f32);
            }
        }
        if !changed {
            break;
        }
    }
    labels
}

fn majority_filter(labels: &[usize], neighbors: &[Vec<u32>]) -> Vec<usize> {
    labels
        .iter()
        .enumerate()
        .map(|(i, &own)| {
            let mut counts: HashMap<usize, usize> = HashMap::new();
            *counts.entry(own).or_default() += 1;
            for &n in &neighbors[i] {
                *counts.entry(labels[n as usize]).or_default() += 1;
            }
            let best = counts.iter().max_by_key(|(_, c)| **c).map(|(l, c)| (*l, *c)).unwrap();
            // Solo cambia si la mayoría es clara, para no comerse bordes finos.
            if best.0 != own && best.1 * 2 > neighbors[i].len() + 1 { best.0 } else { own }
        })
        .collect()
}

/// Trozos conexos de la misma etiqueta. Los pequeños toman la etiqueta del vecino con el que
/// comparten más aristas, hasta que no queda ninguno pequeño.
fn absorb_small_components(faces: &[Face], neighbors: &[Vec<u32>], labels: &mut [usize], min_fraction: f32) {
    let total: f32 = faces.iter().map(|f| f.area).sum();
    for _ in 0..20 {
        let (component, count) = components(labels, neighbors);
        let mut area = vec![0.0f32; count];
        for (f, &c) in faces.iter().zip(&component) {
            area[c] += f.area;
        }
        let mut border: Vec<HashMap<usize, usize>> = vec![HashMap::new(); count];
        for (i, list) in neighbors.iter().enumerate() {
            for &n in list {
                let (a, b) = (component[i], component[n as usize]);
                if a != b {
                    *border[a].entry(b).or_default() += 1;
                }
            }
        }
        let mut target_label: Vec<Option<usize>> = vec![None; count];
        let mut any = false;
        for c in 0..count {
            if area[c] < total * min_fraction {
                if let Some((&other, _)) = border[c].iter().max_by_key(|(_, n)| **n) {
                    let face = component.iter().position(|&x| x == other).unwrap();
                    target_label[c] = Some(labels[face]);
                    any = true;
                }
            }
        }
        if !any {
            break;
        }
        for (i, label) in labels.iter_mut().enumerate() {
            if let Some(t) = target_label[component[i]] {
                *label = t;
            }
        }
    }
}

fn components(labels: &[usize], neighbors: &[Vec<u32>]) -> (Vec<usize>, usize) {
    let mut component = vec![usize::MAX; labels.len()];
    let mut count = 0;
    let mut stack = Vec::new();
    for start in 0..labels.len() {
        if component[start] != usize::MAX {
            continue;
        }
        component[start] = count;
        stack.push(start);
        while let Some(f) = stack.pop() {
            for &n in &neighbors[f] {
                let n = n as usize;
                if component[n] == usize::MAX && labels[n] == labels[f] {
                    component[n] = count;
                    stack.push(n);
                }
            }
        }
        count += 1;
    }
    (component, count)
}

/// Une trozos vecinos cuando en su frontera el color cambia poco (un degradado, como el pelo
/// que oscurece hacia las puntas) y los deja separados si cambia de golpe (pelo contra piel).
/// Devuelve la región final de cada cara.
fn merge_regions(faces: &[Face], neighbors: &[Vec<u32>], labels: &[usize], max_delta: f32) -> Vec<usize> {
    let (component, count) = components(labels, neighbors);
    let mut boundary: HashMap<(usize, usize), (f32, u32)> = HashMap::new();
    for (i, list) in neighbors.iter().enumerate() {
        for &n in list {
            let (a, b) = (component[i], component[n as usize]);
            if a < b {
                let d = delta_e(&faces[i].lab, &faces[n as usize].lab);
                let entry = boundary.entry((a, b)).or_default();
                entry.0 += d;
                entry.1 += 1;
            }
        }
    }
    // Color medio de cada trozo.
    let mut mean = vec![([0.0f32; 3], 0.0f32); count];
    for (f, &c) in faces.iter().zip(&component) {
        for k in 0..3 {
            mean[c].0[k] += f.lab[k] * f.area;
        }
        mean[c].1 += f.area;
    }
    let mean: Vec<[f32; 3]> = mean.iter().map(|(sum, area)| sum.map(|v| v / area.max(1e-12))).collect();
    // Se unen solo los degradados: frontera suave, y mucho más suave que la diferencia entre los
    // dos trozos. Dos materiales oscuros distintos (pelo negro y abrigo azul marino) tienen una
    // frontera tan diferente como sus medias, y quedan separados.
    let mut pairs: Vec<(f32, usize, usize)> = boundary
        .into_iter()
        .filter(|(_, (_, n))| *n >= 4)
        .map(|((a, b), (sum, n))| (sum / n as f32, a, b))
        .filter(|(edge, a, b)| *edge < max_delta && *edge < 0.5 * delta_e(&mean[*a], &mean[*b]))
        .collect();
    pairs.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut parent: Vec<usize> = (0..count).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for (_, a, b) in pairs {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra != rb {
            parent[rb] = ra;
        }
    }
    component.iter().map(|&c| find(&mut parent, c)).collect()
}

fn delta_e(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

struct Region {
    id: usize,
    area: f32,
    rgb: Vec3,
    lab: [f32; 3],
    centroid: Vec3,
    /// Altura del punto más alto, como fracción de la altura del modelo.
    top: f32,
}

fn build_segments(faces: &[Face], regions: &[usize]) -> Vec<Segment> {
    let total: f32 = faces.iter().map(|f| f.area).sum();
    let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for f in faces {
        low = low.min(f.center);
        high = high.max(f.center);
    }
    let size = (high - low).max(Vec3::splat(1e-6));
    let rel = |p: Vec3| (p - low) / size;
    let mut by_id: HashMap<usize, Region> = HashMap::new();
    for (f, &r) in faces.iter().zip(regions) {
        let region = by_id.entry(r).or_insert(Region { id: r, area: 0.0, rgb: Vec3::ZERO, lab: [0.0; 3], centroid: Vec3::ZERO, top: 0.0 });
        region.area += f.area;
        region.rgb += Vec3::from(f.rgb) * f.area;
        for c in 0..3 {
            region.lab[c] += f.lab[c] * f.area;
        }
        region.centroid += f.center * f.area;
        region.top = region.top.max(rel(f.center).y);
    }
    let mut list: Vec<Region> = by_id.into_values().collect();
    for r in &mut list {
        r.rgb /= r.area;
        r.lab = r.lab.map(|v| v / r.area);
        r.centroid /= r.area;
    }
    // Las piezas diminutas aisladas (sin vecino al que unirse) van a la región grande más cercana.
    let tiny = total * 0.001;
    let big: Vec<(usize, Vec3)> = list.iter().filter(|r| r.area >= tiny).map(|r| (r.id, r.centroid)).collect();
    let mut moved: HashMap<usize, usize> = HashMap::new();
    if !big.is_empty() {
        for r in list.iter().filter(|r| r.area < tiny) {
            let nearest = big.iter().min_by(|a, b| a.1.distance(r.centroid).total_cmp(&b.1.distance(r.centroid))).unwrap().0;
            moved.insert(r.id, nearest);
        }
    }
    if !moved.is_empty() {
        let regions: Vec<usize> = regions.iter().map(|r| *moved.get(r).unwrap_or(r)).collect();
        return build_segments(faces, &regions);
    }
    list.sort_by(|a, b| b.area.total_cmp(&a.area));

    // Piel: claro, cálido y poco saturado.
    let is_skin = |r: &Region| {
        let [l, a, b] = r.lab;
        let chroma = (a * a + b * b).sqrt();
        a > 2.0 && b > 5.0 && l > 50.0 && chroma < 40.0 && b < a * 3.5
    };
    // Pelo: la región que no es piel y llega más arriba, con área apreciable, y las regiones de
    // color parecido que están en la mitad de arriba (mechones que la cara separa).
    let hair = list
        .iter()
        .filter(|r| !is_skin(r) && r.area > total * 0.03)
        .max_by(|a, b| (a.top + rel(a.centroid).y * 0.5).total_cmp(&(b.top + rel(b.centroid).y * 0.5)))
        .map(|r| r.lab);
    let class_of = |r: &Region| -> &'static str {
        let h = rel(r.centroid).y;
        if is_skin(r) {
            "Piel"
        } else if hair.is_some_and(|lab| r.lab == lab || (delta_e(&lab, &r.lab) < 12.0 && h > 0.72)) {
            "Pelo"
        } else if h < 0.15 {
            "Calzado"
        } else if r.area < total * 0.015 {
            "Accesorios"
        } else {
            "Ropa"
        }
    };
    let zone = |r: &Region| -> &'static str {
        let p = rel(r.centroid);
        // El personaje mira hacia +Z, así que su izquierda está en +X.
        let side = if p.x > 0.58 { "izq" } else if p.x < 0.42 { "der" } else { "" };
        match (p.y, side) {
            (y, _) if y > 0.82 => "cabeza",
            (y, "") if y > 0.5 => "torso",
            (y, "izq") if y > 0.5 => "brazo izq",
            (y, "der") if y > 0.5 => "brazo der",
            (y, "") if y > 0.4 => "cintura",
            (y, s) if y > 0.12 => if s == "izq" { "pierna izq" } else if s == "der" { "pierna der" } else { "piernas" },
            (_, "izq") => "pie izq",
            (_, "der") => "pie der",
            _ => "pies",
        }
    };

    let mut segments = Vec::new();
    let mut counters: HashMap<String, usize> = HashMap::new();
    for r in &list {
        let class = class_of(r);
        let z = zone(r);
        let rgb = r.rgb.to_array().map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        let base = format!("{} {}", class.to_lowercase(), z);
        let n = counters.entry(base.clone()).or_default();
        *n += 1;
        let name = if *n == 1 { base } else { format!("{base} {n}") };
        let mut by_primitive: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for (f, _) in faces.iter().zip(regions).filter(|(_, &x)| x == r.id) {
            by_primitive.entry((f.source.0, f.source.1)).or_default().push(f.source.2);
        }
        let mut faces_by_primitive: Vec<_> = by_primitive.into_iter().collect();
        faces_by_primitive.sort_by_key(|(k, _)| *k);
        segments.push(Segment {
            class: class.to_string(),
            name,
            color: rgb,
            area_fraction: r.area / total,
            faces: faces_by_primitive,
        });
    }
    segments
}

/// Construye un modelo nuevo: una capa raíz con el nombre del modelo, un grupo por clase (Piel,
/// Pelo, Ropa, Calzado, Accesorios) y dentro una capa por región. Cada capa conserva el material
/// y la textura de origen.
pub fn into_model(model: Model, segments: &[Segment]) -> Model {
    use crate::model::{Layer, Primitive};
    let layer = |name: &str, parent: Option<usize>, depth: usize, primitives: Vec<Primitive>| Layer {
        name: name.to_string(),
        parent,
        depth,
        primitives,
        visible: true,
        opacity: 1.0,
        expanded: depth < 1,
    };
    let mut layers = vec![layer(&model.name, None, 0, Vec::new())];
    for class in ["Pelo", "Piel", "Ropa", "Calzado", "Accesorios"] {
        let members: Vec<&Segment> = segments.iter().filter(|s| s.class == class).collect();
        if members.is_empty() {
            continue;
        }
        let group = layers.len();
        layers.push(layer(class, Some(0), 1, Vec::new()));
        for segment in members {
            let mut primitives = Vec::new();
            for ((li, pi), triangles) in &segment.faces {
                let source = &model.layers[*li].primitives[*pi];
                let mut remap: HashMap<u32, u32> = HashMap::new();
                let mut vertices = Vec::new();
                let mut indices = Vec::with_capacity(triangles.len() * 3);
                for &t in triangles {
                    for k in 0..3 {
                        let old = source.indices[t * 3 + k];
                        let new = *remap.entry(old).or_insert_with(|| {
                            vertices.push(source.vertices[old as usize]);
                            (vertices.len() - 1) as u32
                        });
                        indices.push(new);
                    }
                }
                primitives.push(Primitive { vertices, indices, material: source.material });
            }
            layers.push(layer(&segment.name, Some(group), 2, primitives));
        }
    }
    Model { name: model.name, layers, materials: model.materials, textures: model.textures }
}

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn srgb_to_lab(rgb: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(srgb_to_linear);
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_and_black_have_the_expected_lightness() {
        assert!((srgb_to_lab([1.0, 1.0, 1.0])[0] - 100.0).abs() < 0.1);
        assert!(srgb_to_lab([0.0, 0.0, 0.0])[0].abs() < 0.1);
    }

    #[test]
    fn majority_filter_removes_a_lone_face() {
        // Una cadena de 5 caras con una distinta en medio.
        let labels = vec![0, 0, 1, 0, 0];
        let neighbors = vec![vec![1], vec![0, 2], vec![1, 3], vec![2, 4], vec![3]];
        assert_eq!(majority_filter(&labels, &neighbors), vec![0, 0, 0, 0, 0]);
    }
}

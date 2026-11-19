//! Cara: busca los ojos pintados en la textura y les pone párpados de geometría que se cierran
//! con formas (morph targets): `blink`, `blinkLeft`, `blinkRight` y `happy` (ojos en arco, ^^).
//!
//! Las mallas de IA (SupaVoxel) traen la cara pintada en la textura, sin párpados. La cara se
//! proyecta de frente sobre una rejilla (el personaje mira hacia +Z) quedándose, en cada celda,
//! con la superficie de cara más al fondo: así el flequillo que tapa los ojos no cuenta. En esa
//! rejilla, los ojos son las manchas de color lejano a la piel alrededor de los huesos de ojo
//! del esqueleto (los de Base_Female, ajustados con el rig).
//!
//! Cada ojo recibe un párpado superior y uno inferior: rejillas pegadas a la superficie de la cara
//! (un poco por delante), de color piel, con una fila de pestañas oscura en el borde. Abiertos,
//! están plegados en una línea (sin área, no se ven); cada forma los despliega.

use crate::model::{AlphaMode, Layer, Material, Model, Morph, Primitive, Vertex};
use anyhow::{Context, Result};
use glam::{Vec2, Vec3};

/// Lado de la rejilla de la cara, en celdas.
const GRID: usize = 384;

/// Una celda de la cara: profundidad, normal y color de la textura.
#[derive(Clone, Copy)]
struct Cell {
    z: f32,
    color: [f32; 3],
    /// Si viene de una capa de piel de la segmentación.
    skin: bool,
}

pub struct Eye {
    /// Centro y semiejes en el plano de la cara (x, y del mundo).
    pub centre: Vec2,
    pub radius: Vec2,
}

pub struct Report {
    pub eyes: Vec<Eye>,
    /// Centro y ancho de la boca, si se encontró pintada (si no, se pone donde suele estar).
    pub mouth: (Vec2, f32, bool),
}

/// La cara de frente: celdas de `low` a `high` en x e y.
struct Raster {
    low: Vec2,
    high: Vec2,
    cells: Vec<Option<Cell>>,
}

impl Raster {
    fn cell_of(&self, p: Vec2) -> Option<(usize, usize)> {
        let t = (p - self.low) / (self.high - self.low);
        if t.x < 0.0 || t.y < 0.0 || t.x >= 1.0 || t.y >= 1.0 {
            return None;
        }
        Some(((t.x * GRID as f32) as usize, (t.y * GRID as f32) as usize))
    }

    /// El punto llevado dentro de la rejilla.
    fn clamp(&self, p: Vec2) -> Vec2 {
        p.clamp(self.low, self.high - (self.high - self.low) * 1e-4)
    }

    fn point_of(&self, x: usize, y: usize) -> Vec2 {
        self.low + (self.high - self.low) * Vec2::new((x as f32 + 0.5) / GRID as f32, (y as f32 + 0.5) / GRID as f32)
    }

    fn get(&self, x: usize, y: usize) -> Option<Cell> {
        self.cells[y * GRID + x]
    }

    /// Profundidad de la cara en un punto; si la celda está vacía, la más cercana con cara.
    fn depth_at(&self, p: Vec2) -> Option<f32> {
        let (cx, cy) = self.cell_of(p.clamp(self.low, self.high - (self.high - self.low) * 1e-4))?;
        for r in 0..12i32 {
            let mut best: Option<(f32, f32)> = None;
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let (x, y) = (cx as i32 + dx, cy as i32 + dy);
                    if x < 0 || y < 0 || x >= GRID as i32 || y >= GRID as i32 {
                        continue;
                    }
                    if let Some(c) = self.get(x as usize, y as usize) {
                        let d = (dx * dx + dy * dy) as f32;
                        if best.map_or(true, |b| d < b.0) {
                            best = Some((d, c.z));
                        }
                    }
                }
            }
            if let Some((_, z)) = best {
                return Some(z);
            }
        }
        None
    }
}

fn sample(model: &Model, material: usize, uv: [f32; 2]) -> [f32; 3] {
    let m = &model.materials[material.min(model.materials.len().saturating_sub(1))];
    let base = [m.base_color[0], m.base_color[1], m.base_color[2]];
    let Some(t) = m.texture.and_then(|t| model.textures.get(t)) else { return base };
    let u = uv[0].rem_euclid(1.0);
    let v = uv[1].rem_euclid(1.0);
    let x = ((u * t.width as f32) as u32).min(t.width - 1);
    let y = ((v * t.height as f32) as u32).min(t.height - 1);
    let i = ((y * t.width + x) * 4) as usize;
    [0, 1, 2].map(|k| t.rgba[i + k] as f32 / 255.0 * base[k])
}

/// sRGB (0..1) a Lab.
pub(crate) fn lab(c: [f32; 3]) -> Vec3 {
    let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    let [r, g, b] = c.map(lin);
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    Vec3::new(116.0 * f(y) - 16.0, 500.0 * (f(x) - f(y)), 200.0 * (f(y) - f(z)))
}

/// Proyecta de frente los triángulos que van con la cabeza y miran hacia delante.
fn rasterize(model: &Model, head: usize, low: Vec2, high: Vec2, size: f32) -> Raster {
    let scale = Vec2::splat(GRID as f32) / (high - low);
    let hair = crate::hair::hair_layers(model);
    // Recorre los píxeles de los triángulos que van con la cabeza y miran hacia delante:
    // `visit(capa, celda, z, material, uv)`.
    let each = |visit: &mut dyn FnMut(usize, usize, f32, usize, [f32; 2])| {
        for (l, layer) in model.layers.iter().enumerate() {
            for p in &layer.primitives {
                if p.joints.is_empty() {
                    continue;
                }
                let goes_with_head = |i: usize| {
                    let (j, w) = (p.joints[i], p.weights[i]);
                    (0..4).any(|k| j[k] as usize == head && w[k] > 0.5)
                };
                for tri in p.indices.chunks_exact(3) {
                    let ids = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
                    if !ids.iter().all(|&i| goes_with_head(i)) {
                        continue;
                    }
                    let v = ids.map(|i| p.vertices[i]);
                    let pos = v.map(|v| Vec3::from(v.position));
                    let normal = (pos[1] - pos[0]).cross(pos[2] - pos[0]);
                    if normal.z <= 0.2 * normal.length() {
                        continue;
                    }
                    let s = pos.map(|q| (Vec2::new(q.x, q.y) - low) * scale);
                    let min = s[0].min(s[1]).min(s[2]).floor().max(Vec2::ZERO);
                    let max = s[0].max(s[1]).max(s[2]).ceil().min(Vec2::splat(GRID as f32 - 1.0));
                    let area = (s[1] - s[0]).perp_dot(s[2] - s[0]);
                    if area.abs() < 1e-9 || min.x > max.x || min.y > max.y {
                        continue;
                    }
                    for y in min.y as usize..=max.y as usize {
                        for x in min.x as usize..=max.x as usize {
                            let q = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                            let w0 = (s[1] - q).perp_dot(s[2] - q) / area;
                            let w1 = (s[2] - q).perp_dot(s[0] - q) / area;
                            let w2 = 1.0 - w0 - w1;
                            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                                continue;
                            }
                            let z = pos[0].z * w0 + pos[1].z * w1 + pos[2].z * w2;
                            let uv = [0, 1].map(|k| v[0].uv[k] * w0 + v[1].uv[k] * w1 + v[2].uv[k] * w2);
                            visit(l, y * GRID + x, z, p.material, uv);
                        }
                    }
                }
            }
        }
    };
    // 1. Profundidad de la piel (lo más delantero de las capas de piel), con los huecos
    //    rellenos con la celda de piel más cercana.
    let is_skin: Vec<bool> = model.layers.iter().map(|l| l.name.to_lowercase().starts_with("piel")).collect();
    let mut skin_z: Vec<Option<f32>> = vec![None; GRID * GRID];
    each(&mut |l, cell, z, _, _| {
        if is_skin[l] && skin_z[cell].map_or(true, |s| z > s) {
            skin_z[cell] = Some(z);
        }
    });
    for _ in 0..24 {
        let previous = skin_z.clone();
        for y in 0..GRID {
            for x in 0..GRID {
                if previous[y * GRID + x].is_some() {
                    continue;
                }
                let mut sum = 0.0;
                let mut n = 0;
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx >= 0 && ny >= 0 && nx < GRID as i32 && ny < GRID as i32 {
                        if let Some(z) = previous[ny as usize * GRID + nx as usize] {
                            sum += z;
                            n += 1;
                        }
                    }
                }
                if n > 0 {
                    skin_z[y * GRID + x] = Some(sum / n as f32);
                }
            }
        }
    }
    // 2. Lo más delantero que está pegado a la piel: los ojos pintados sí; la melena de detrás
    //    y el flequillo que flota delante, no.
    let tolerance = 0.04 * size;
    let mut cells: Vec<Option<Cell>> = vec![None; GRID * GRID];
    each(&mut |l, cell, z, material, uv| {
        let Some(surface) = skin_z[cell] else { return };
        // El pelo solo si está prácticamente sobre la piel: pintado en ella (a veces la
        // segmentación mete ahí el iris, si es del color del pelo), no un mechón por delante.
        let limit = if hair[l] { tolerance * 0.3 } else { tolerance };
        if (z - surface).abs() > limit {
            return;
        }
        if cells[cell].map_or(true, |c| z > c.z) {
            cells[cell] = Some(Cell { z, color: sample(model, material, uv), skin: is_skin[l] });
        }
    });
    Raster { low, high, cells }
}

/// La cara de frente: del hueso de la cabeza a la coronilla. Devuelve también el alto de la
/// cabeza y la posición de su hueso.
fn face_raster(model: &Model) -> Result<(Raster, f32, Vec3)> {
    let skeleton = model.skeleton.as_ref().context("el modelo no tiene esqueleto")?;
    let head = skeleton.bone("head").context("el esqueleto no tiene cabeza")?;
    let head_pos = skeleton.rest_position(head);
    // Alto de la cabeza: del hueso a lo más alto que va con ella.
    let mut top = head_pos.y;
    for layer in &model.layers {
        for p in &layer.primitives {
            for (v, (j, w)) in p.vertices.iter().zip(p.joints.iter().zip(&p.weights)) {
                if (0..4).any(|k| j[k] as usize == head && w[k] > 0.5) {
                    top = top.max(v.position[1]);
                }
            }
        }
    }
    let size = (top - head_pos.y).max(0.05);
    let low = Vec2::new(head_pos.x - 0.9 * size, head_pos.y - 0.3 * size);
    let high = Vec2::new(head_pos.x + 0.9 * size, head_pos.y + 1.1 * size);
    Ok((rasterize(model, head, low, high, size), size, head_pos))
}

/// Busca los ojos. Devuelve también la rejilla, el tamaño de la cabeza y el color de la piel.
/// Primero con criterios estrictos; si no, sin contar lo del color del pelo (pestañas pegadas al
/// flequillo) y admitiendo cabezas inclinadas.
fn find_eyes(model: &Model) -> Result<(Vec<Eye>, Raster, f32, [f32; 3])> {
    find_eyes_with(model, false).or_else(|_| find_eyes_with(model, true))
}

fn find_eyes_with(model: &Model, relaxed: bool) -> Result<(Vec<Eye>, Raster, f32, [f32; 3])> {
    let (raster, size, head_pos) = face_raster(model)?;

    // Piel: la mediana de lo que viene de capas de piel.
    let skin: Vec<Vec3> = raster.cells.iter().flatten().filter(|c| c.skin).map(|c| lab(c.color)).collect();
    anyhow::ensure!(skin.len() >= 50, "no se ve la piel de la cara de frente");
    let median = |k: usize| {
        let mut v: Vec<f32> = skin.iter().map(|c| c[k]).collect();
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    };
    let skin_lab = Vec3::new(median(0), median(1), median(2));
    let skin_rgb = {
        // El color de la piel en sRGB, para los párpados: el de la celda más parecida.
        let mut best = ([0.9, 0.75, 0.65], f32::MAX);
        for c in raster.cells.iter().flatten() {
            let d = lab(c.color).distance(skin_lab);
            if d < best.1 {
                best = (c.color, d);
            }
        }
        best.0
    };
    let is_skin = |x: usize, y: usize| raster.get(x, y).is_some_and(|c| lab(c.color).distance(skin_lab) < 18.0);
    // Color del pelo: la mediana de lo que no es piel en la frente y lo de arriba. Lo que se le
    // parece no cuenta como marca: así las pestañas no unen los ojos al flequillo pintado.
    let mut top_marks: Vec<Vec3> = raster
        .cells
        .iter()
        .enumerate()
        .filter(|(i, c)| c.is_some_and(|c| lab(c.color).distance(skin_lab) > 30.0) && raster.point_of(i % GRID, i / GRID).y > head_pos.y + 0.75 * size)
        .map(|(_, c)| lab(c.unwrap().color))
        .collect();
    let hair_lab = (relaxed && top_marks.len() >= 50).then(|| {
        let mid = top_marks.len() / 2;
        let mut channel = |k: usize| {
            top_marks.sort_by(|a, b| a[k].total_cmp(&b[k]));
            top_marks[mid][k]
        };
        Vec3::new(channel(0), channel(1), channel(2))
    });
    let is_mark = |x: usize, y: usize| {
        raster.get(x, y).is_some_and(|c| {
            let l = lab(c.color);
            l.distance(skin_lab) > 30.0 && hair_lab.map_or(true, |h| l.distance(h) > 20.0)
        })
    };

    // Manchas que no son piel, con lo rodeadas de piel que están.
    struct Blob {
        cells: Vec<(usize, usize)>,
        centre: Vec2,
        enclosed: f32,
        /// Alto entre ancho, y croma media (el iris tiene color).
        aspect: f32,
        chroma: f32,
    }
    // Las manchas se separan donde solo las une una línea fina (pestañas que tocan la ceja o un
    // mechón): se erosiona la máscara, se etiqueta y se vuelve a crecer dentro de la máscara.
    let mask: Vec<bool> = (0..GRID * GRID).map(|i| is_mark(i % GRID, i / GRID)).collect();
    let neighbours = |i: usize| {
        let (x, y) = ((i % GRID) as i32, (i / GRID) as i32);
        [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)].into_iter().filter_map(move |(dx, dy)| {
            let (nx, ny) = (x + dx, y + dy);
            (nx >= 0 && ny >= 0 && nx < GRID as i32 && ny < GRID as i32).then(|| ny as usize * GRID + nx as usize)
        })
    };
    let mut core = mask.clone();
    for _ in 0..2 {
        core = (0..GRID * GRID).map(|i| core[i] && neighbours(i).all(|n| core[n])).collect();
    }
    let mut label = vec![usize::MAX; GRID * GRID];
    let mut labels = 0;
    for start in 0..GRID * GRID {
        if !core[start] || label[start] != usize::MAX {
            continue;
        }
        let mut stack = vec![start];
        label[start] = labels;
        while let Some(i) = stack.pop() {
            for n in neighbours(i) {
                if core[n] && label[n] == usize::MAX {
                    label[n] = labels;
                    stack.push(n);
                }
            }
        }
        labels += 1;
    }
    for _ in 0..3 {
        let previous = label.clone();
        for i in 0..GRID * GRID {
            if mask[i] && previous[i] == usize::MAX {
                if let Some(l) = neighbours(i).map(|n| previous[n]).find(|l| *l != usize::MAX) {
                    label[i] = l;
                }
            }
        }
    }
    let mut groups: Vec<Vec<usize>> = vec![Vec::new(); labels];
    for i in 0..GRID * GRID {
        if label[i] != usize::MAX {
            groups[label[i]].push(i);
        }
    }
    let mut blobs: Vec<Blob> = Vec::new();
    for group in groups {
        let (mut border, mut border_skin) = (0usize, 0usize);
        for &i in &group {
            for n in neighbours(i) {
                if label[n] != label[i] {
                    border += 1;
                    if is_skin(n % GRID, n / GRID) {
                        border_skin += 1;
                    }
                }
            }
        }
        let cells: Vec<(usize, usize)> = group.iter().map(|i| (i % GRID, i / GRID)).collect();
        if cells.len() < 15 {
            continue;
        }
        let centre = cells.iter().fold(Vec2::ZERO, |a, (x, y)| a + raster.point_of(*x, *y)) / cells.len() as f32;
        let (min_x, max_x) = cells.iter().fold((usize::MAX, 0), |(a, b), (x, _)| (a.min(*x), b.max(*x)));
        let (min_y, max_y) = cells.iter().fold((usize::MAX, 0), |(a, b), (_, y)| (a.min(*y), b.max(*y)));
        let aspect = (max_y - min_y + 1) as f32 / (max_x - min_x + 1) as f32;
        let chroma = cells
            .iter()
            .filter_map(|(x, y)| raster.get(*x, *y))
            .map(|c| {
                let l = lab(c.color);
                (l.y * l.y + l.z * l.z).sqrt()
            })
            .sum::<f32>()
            / cells.len() as f32;
        blobs.push(Blob { enclosed: border_skin as f32 / border.max(1) as f32, cells, centre, aspect, chroma });
    }
    // Centro de la cara: la mediana en x de la piel a la altura de los ojos.
    let mut skin_x: Vec<f32> = (0..GRID * GRID)
        .filter(|i| raster.cells[*i].is_some_and(|c| c.skin))
        .map(|i| raster.point_of(i % GRID, i / GRID))
        .filter(|p| p.y > head_pos.y && p.y < head_pos.y + 0.6 * size)
        .map(|p| p.x)
        .collect();
    skin_x.sort_by(f32::total_cmp);
    let face_x = skin_x.get(skin_x.len() / 2).copied().unwrap_or(head_pos.x);
    // La mejor pareja: una mancha a cada lado del centro, a alturas parecidas, simétricas y en la
    // franja de los ojos; cuenta su tamaño y lo rodeadas de piel que están.
    // Un ojo: ni raya vertical (sombras del cuello) ni ceja plana, bastante rodeado de piel.
    let tallest = if relaxed { 1.8 } else { 1.6 };
    let eye_like = |b: &Blob| (0.3..=tallest).contains(&b.aspect) && b.enclosed >= 0.12;
    let score = |b: &Blob| b.cells.len() as f32 * b.enclosed.powi(2) * (1.0 + b.chroma / 30.0);
    let mut best: Option<(f32, usize, usize)> = None;
    for (i, a) in blobs.iter().enumerate() {
        for (j, b) in blobs.iter().enumerate() {
            let (da, db) = (a.centre.x - face_x, b.centre.x - face_x);
            if da <= 0.0 || db >= 0.0 {
                continue;
            }
            let band = |c: Vec2| c.y > head_pos.y && c.y < head_pos.y + 0.65 * size;
            if !band(a.centre) || !band(b.centre) || !eye_like(a) || !eye_like(b) {
                continue;
            }
            let apart = (a.centre.x - b.centre.x) / size;
            if !(0.15..=0.7).contains(&apart) {
                continue;
            }
            let level = (a.centre.y - b.centre.y).abs() / size;
            let mirror = (da + db).abs() / size;
            let ratio = a.cells.len().min(b.cells.len()) as f32 / a.cells.len().max(b.cells.len()) as f32;
            if level > if relaxed { 0.2 } else { 0.12 } || mirror > 0.2 || ratio < 0.3 {
                continue;
            }
            let s = (score(a) + score(b)) * (1.0 - level * 4.0) * (1.0 - mirror * 3.0);
            if best.map_or(true, |(bs, _, _)| s > bs) {
                best = Some((s, i, j));
            }
        }
    }
    if std::env::var("A2W_DEBUG_FACE").is_ok() {
        for b in &blobs {
            let rel = (b.centre - Vec2::new(head_pos.x, head_pos.y)) / size;
            eprintln!("mancha {:5} celdas en ({:+.2}, {:.2}) rodeada {:.2} alto/ancho {:.2} croma {:.0}", b.cells.len(), rel.x, rel.y, b.enclosed, b.aspect, b.chroma);
        }
    }
    let (_, i, j) = best.context("no se encuentran dos ojos pintados en la cara")?;
    let found = [i, j]
        .iter()
        .map(|&k| {
            let cells = &blobs[k].cells;
            let mut xs: Vec<f32> = cells.iter().map(|(x, y)| raster.point_of(*x, *y).x).collect();
            let mut ys: Vec<f32> = cells.iter().map(|(x, y)| raster.point_of(*x, *y).y).collect();
            xs.sort_by(f32::total_cmp);
            ys.sort_by(f32::total_cmp);
            let q = |v: &[f32], f: f32| v[((v.len() - 1) as f32 * f) as usize];
            let (lx, hx, ly, hy) = (q(&xs, 0.02), q(&xs, 0.98), q(&ys, 0.02), q(&ys, 0.98));
            Eye { centre: Vec2::new((lx + hx) * 0.5, (ly + hy) * 0.5), radius: Vec2::new((hx - lx) * 0.5, (hy - ly) * 0.5) }
        })
        .collect::<Vec<_>>();
    // Los ojos son simétricos: los dos con el tamaño del menor (el otro suele llevar pegado algo
    // de ceja o de pelo), y a la altura media.
    let radius = found[0].radius.min(found[1].radius);
    // Ojos de menos de una décima de su separación son manchas, no ojos.
    let spacing = (found[0].centre.x - found[1].centre.x).abs();
    anyhow::ensure!(radius.x * 2.0 >= spacing * 0.15, "los ojos encontrados son demasiado pequeños");
    let mut found = found;
    for eye in &mut found {
        eye.radius = radius;
    }
    Ok((found, raster, size, skin_rgb))
}

/// Boca: centro, ancho y si se encontró pintada. Es la línea de marcas rojizas (labios) u
/// oscuras más poblada debajo de los ojos, centrada entre ellos; si no hay, donde suele estar.
fn find_mouth(raster: &Raster, eyes: &[Eye], skin: [f32; 3]) -> (Vec2, f32, bool) {
    let spacing = (eyes[0].centre.x - eyes[1].centre.x).abs();
    let eye_y = (eyes[0].centre.y + eyes[1].centre.y) * 0.5;
    let mid_x = (eyes[0].centre.x + eyes[1].centre.x) * 0.5;
    let skin_lab = lab(skin);
    let (low, high) = mouth_window(eyes);
    let cell = (raster.high.y - raster.low.y) / GRID as f32;
    // Filas de labios rojizos y filas de rayas oscuras: mandan los labios si los hay.
    let mut rows: std::collections::HashMap<i32, Vec<f32>> = std::collections::HashMap::new();
    let mut dark_rows: std::collections::HashMap<i32, Vec<f32>> = std::collections::HashMap::new();
    if let (Some((x0, y0)), Some((x1, y1))) = (raster.cell_of(raster.clamp(low)), raster.cell_of(raster.clamp(high))) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                let Some(c) = raster.get(x, y) else { continue };
                let l = lab(c.color);
                let reddish = l.y - skin_lab.y > 10.0 && l.y > l.z.abs() * 0.5;
                // Oscuro y de poco color (no un collar azul).
                let dark = l.x < skin_lab.x - 25.0 && (l.y * l.y + l.z * l.z).sqrt() < 35.0;
                let p = raster.point_of(x, y);
                let row = ((p.y - low.y) / cell) as i32;
                if reddish {
                    rows.entry(row).or_default().push(p.x);
                } else if dark {
                    dark_rows.entry(row).or_default().push(p.x);
                }
            }
        }
    }
    if std::env::var("A2W_DEBUG_FACE").is_ok() {
        // Los colores más alejados de la piel en la ventana, para ajustar los criterios.
        let mut seen: Vec<(f32, Vec3)> = Vec::new();
        if let (Some((x0, y0)), Some((x1, y1))) = (raster.cell_of(raster.clamp(low)), raster.cell_of(raster.clamp(high))) {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if let Some(c) = raster.get(x, y) {
                        let l = lab(c.color);
                        seen.push((l.distance(skin_lab), l));
                    }
                }
            }
        }
        seen.sort_by(|a, b| b.0.total_cmp(&a.0));
        eprintln!("piel {skin_lab:?}");
        for (d, l) in seen.iter().step_by(4).take(12) {
            eprintln!("  ΔE {d:.0} Lab ({:.0}, {:.0}, {:.0})", l.x, l.y, l.z);
        }
    }
    if !rows.values().any(|xs| xs.len() >= 3) {
        rows = dark_rows;
    }
    match rows.iter().filter(|(_, xs)| xs.len() >= 3).max_by_key(|(_, xs)| xs.len()) {
        Some((row, xs)) => {
            let left = xs.iter().copied().fold(f32::MAX, f32::min);
            let right = xs.iter().copied().fold(f32::MIN, f32::max);
            let y = low.y + (*row as f32 + 0.5) * cell;
            (Vec2::new((left + right) * 0.5, y), (right - left).clamp(0.2 * spacing, 0.6 * spacing), true)
        }
        None => (Vec2::new(mid_x, eye_y - 0.7 * spacing), 0.3 * spacing, false),
    }
}

/// Dónde se busca la boca: debajo de los ojos, entre ellos.
fn mouth_window(eyes: &[Eye]) -> (Vec2, Vec2) {
    let spacing = (eyes[0].centre.x - eyes[1].centre.x).abs();
    let eye_y = (eyes[0].centre.y + eyes[1].centre.y) * 0.5;
    let mid_x = (eyes[0].centre.x + eyes[1].centre.x) * 0.5;
    (Vec2::new(mid_x - 0.3 * spacing, eye_y - 1.05 * spacing), Vec2::new(mid_x + 0.3 * spacing, eye_y - 0.4 * spacing))
}

/// Dibujos de boca del atlas, en orden: el primero es la boca original.
pub const MOUTH_DRAWINGS: [&str; 4] = ["original", "sonrisa", "gatito", "puchero"];

/// Boca por textura: una calcomanía pegada a la cara sobre la boca pintada, con un atlas de
/// dibujos. El primero es la boca original copiada tal cual (no se nota la calcomanía); en los
/// demás, la boca se borra rellenando con la piel de alrededor y se dibuja otra encima:
/// sonrisa cerrada (‿), boca de gatito (ω) y puchero (︵). Cada forma `sonrisa`, `gatito` y
/// `puchero` solo mueve las UV a su dibujo.
#[allow(clippy::too_many_arguments)]
fn add_mouth_drawings(model: &mut Model, face: &Raster, head: usize, size: f32, lift: f32, skin: [f32; 3], centre: Vec2, width: f32) -> Result<()> {
    const W: usize = 192;
    const H: usize = 96;
    let half = Vec2::new(0.75 * width, 0.375 * width);
    let low = centre - Vec2::new(half.x, half.y * 1.1);
    let high = centre + Vec2::new(half.x, half.y * 0.9);
    // Proyección fina de la zona de la boca (las capas de párpados y boca están plegadas: no
    // tienen área y no salen).
    let fine = rasterize(model, head, low, high, size);
    let skin_lab = lab(skin);
    let pixel_point = |x: usize, y: usize| {
        Vec2::new(low.x + (x as f32 + 0.5) / W as f32 * (high.x - low.x), high.y - (y as f32 + 0.5) / H as f32 * (high.y - low.y))
    };
    let mut original = vec![skin; W * H];
    for y in 0..H {
        for x in 0..W {
            if let Some(c) = fine.cell_of(pixel_point(x, y)).and_then(|(cx, cy)| fine.get(cx, cy)) {
                original[y * W + x] = c.color;
            }
        }
    }
    // Lo que es boca: lejos del color de piel, dentro de una elipse alrededor del centro.
    let (cx, cy) = (W as f32 * 0.5, H as f32 * 0.55);
    let inside = |x: usize, y: usize| {
        let dx = (x as f32 - cx) / (W as f32 * 0.4);
        let dy = (y as f32 - cy) / (H as f32 * 0.42);
        dx * dx + dy * dy < 1.0
    };
    let mut mark = vec![false; W * H];
    let mut line = [0.35f32, 0.12, 0.12];
    let mut darkest = f32::MAX;
    for y in 0..H {
        for x in 0..W {
            let c = original[y * W + x];
            let l = lab(c);
            if inside(x, y) && l.distance(skin_lab) > 18.0 {
                mark[y * W + x] = true;
                if l.x < darkest {
                    darkest = l.x;
                    line = c;
                }
            }
        }
    }
    // La línea nueva, un poco menos oscura que lo más oscuro de la boca pintada.
    line = line.map(|v| v * 0.85 + 0.08);
    // Sin boca: la piel de alrededor se extiende hacia dentro.
    let mut clean = original.clone();
    let mut known: Vec<bool> = mark.iter().map(|m| !m).collect();
    for _ in 0..W {
        let mut changed = false;
        let previous = clean.clone();
        let was_known = known.clone();
        for y in 0..H {
            for x in 0..W {
                if was_known[y * W + x] {
                    continue;
                }
                let mut sum = [0.0f32; 3];
                let mut n = 0.0;
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < W && (ny as usize) < H && was_known[ny as usize * W + nx as usize] {
                        let c = previous[ny as usize * W + nx as usize];
                        for k in 0..3 {
                            sum[k] += c[k];
                        }
                        n += 1.0;
                    }
                }
                if n > 0.0 {
                    clean[y * W + x] = sum.map(|v| v / n);
                    known[y * W + x] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    // Trazos: curvas por puntos, con borde suavizado.
    let thickness = (W as f32 * 0.018).max(1.2);
    let draw = |image: &mut Vec<[f32; 3]>, curve: &dyn Fn(f32) -> (f32, f32)| {
        for k in 0..=400 {
            let t = k as f32 / 400.0 * 2.0 - 1.0;
            let (px, py) = curve(t);
            let r = thickness;
            for y in (py - r - 1.0).max(0.0) as usize..=((py + r + 1.0) as usize).min(H - 1) {
                for x in (px - r - 1.0).max(0.0) as usize..=((px + r + 1.0) as usize).min(W - 1) {
                    let d = ((x as f32 + 0.5 - px).powi(2) + (y as f32 + 0.5 - py).powi(2)).sqrt();
                    let alpha = (r + 0.5 - d).clamp(0.0, 1.0);
                    if alpha > 0.0 {
                        let c = &mut image[y * W + x];
                        for i in 0..3 {
                            c[i] = c[i] * (1.0 - alpha) + line[i] * alpha;
                        }
                    }
                }
            }
        }
    };
    // Medidas en píxeles: media boca, la mitad del ancho detectado.
    let hw = W as f32 / 3.0;
    let mut smile = clean.clone();
    draw(&mut smile, &|t| (cx + t * 0.75 * hw, cy - 0.12 * hw + 0.22 * hw * (1.0 - t * t)));
    let mut cat = clean.clone();
    for side in [-1.0f32, 1.0] {
        draw(&mut cat, &|t| (cx + side * 0.26 * hw + t * 0.26 * hw, cy - 0.06 * hw + 0.2 * hw * (1.0 - t * t)));
    }
    let mut pout = clean.clone();
    draw(&mut pout, &|t| (cx + t * 0.32 * hw, cy + 0.08 * hw - 0.14 * hw * (1.0 - t * t)));

    // Atlas: los cuatro dibujos en fila.
    let sprites = [&original, &smile, &cat, &pout];
    let mut rgba = vec![0u8; sprites.len() * W * H * 4];
    for (s, sprite) in sprites.iter().enumerate() {
        for y in 0..H {
            for x in 0..W {
                let c = sprite[y * W + x];
                let i = (y * W * sprites.len() + s * W + x) * 4;
                for k in 0..3 {
                    rgba[i + k] = (c[k].clamp(0.0, 1.0) * 255.0).round() as u8;
                }
                // Borde fundido: la calcomanía no deja marco aunque la luz cambie un poco.
                let edge = |p: usize, n: usize| (p.min(n - 1 - p) as f32 / (n as f32 * 0.15)).min(1.0);
                rgba[i + 3] = (edge(x, W).min(edge(y, H)) * 255.0).round() as u8;
            }
        }
    }
    let texture = model.textures.len();
    model.textures.push(crate::model::Texture { width: (sprites.len() * W) as u32, height: H as u32, rgba });
    let material = model.materials.len();
    model.materials.push(Material { name: "boca dibujada".into(), base_color: [1.0; 4], texture: Some(texture), alpha: AlphaMode::Blend, double_sided: true });

    // La calcomanía: una rejilla pegada a la cara, por delante de la cara y por detrás del
    // interior de la boca abierta.
    const COLUMNS: usize = 16;
    const ROWS: usize = 8;
    let decal_lift = lift * 1.2;
    let mut primitive = Primitive { vertices: Vec::new(), indices: Vec::new(), material, joints: Vec::new(), weights: Vec::new(), morphs: Vec::new() };
    let step = (high.x - low.x) / COLUMNS as f32;
    for row in 0..=ROWS {
        for col in 0..=COLUMNS {
            let (u, v) = (col as f32 / COLUMNS as f32, row as f32 / ROWS as f32);
            let p = Vec2::new(low.x + u * (high.x - low.x), high.y - v * (high.y - low.y));
            let z = face.depth_at(p).unwrap_or(0.0) + decal_lift;
            let dz = |d: Vec2| face.depth_at(p + d).unwrap_or(z) - face.depth_at(p - d).unwrap_or(z);
            let normal = Vec3::new(-dz(Vec2::new(step, 0.0)) / (2.0 * step), -dz(Vec2::new(0.0, step)) / (2.0 * step), 1.0).normalize();
            primitive.vertices.push(Vertex { position: [p.x, p.y, z], normal: normal.to_array(), uv: [u / sprites.len() as f32, v] });
            primitive.joints.push([head as u16, 0, 0, 0]);
            primitive.weights.push([1.0, 0.0, 0.0, 0.0]);
        }
    }
    let stride = (COLUMNS + 1) as u32;
    for row in 0..ROWS as u32 {
        for col in 0..COLUMNS as u32 {
            let a = row * stride + col;
            let (b, c) = (a + 1, a + stride);
            primitive.indices.extend_from_slice(&[a, c, b, b, c, c + 1]);
        }
    }
    let count = primitive.vertices.len();
    primitive.morphs = MOUTH_DRAWINGS[1..]
        .iter()
        .enumerate()
        .map(|(k, name)| Morph {
            name: name.to_string(),
            deltas: vec![[0.0; 3]; count],
            uv_deltas: vec![[(k + 1) as f32 / sprites.len() as f32, 0.0]; count],
        })
        .collect();
    model.layers.push(Layer {
        name: "boca dibujada".into(),
        parent: None,
        depth: 0,
        primitives: vec![primitive],
        visible: true,
        opacity: 1.0,
        expanded: false,
    });
    Ok(())
}

/// Imagen de la cara proyectada con los ojos encontrados, para comprobar a ojo.
pub fn debug_image(model: &Model, path: &std::path::Path) -> Result<()> {
    let (raster, _, _) = face_raster(model)?;
    let (eyes, skin) = match find_eyes(model) {
        Ok((eyes, _, _, skin)) => (eyes, skin),
        Err(error) => {
            eprintln!("{error:#}");
            (Vec::new(), [0.9, 0.75, 0.65])
        }
    };
    let mut image = image::RgbImage::new(GRID as u32, GRID as u32);
    for y in 0..GRID {
        for x in 0..GRID {
            let c = raster.get(x, y).map_or([40, 40, 60], |c| c.color.map(|v| (v * 255.0) as u8));
            image.put_pixel(x as u32, (GRID - 1 - y) as u32, image::Rgb(c));
        }
    }
    if eyes.len() == 2 {
        // La ventana de la boca en azul y la boca encontrada en rojo.
        let (low, high) = mouth_window(&eyes);
        let (centre, width, _) = find_mouth(&raster, &eyes, skin);
        let mut plot = |p: Vec2, c: [u8; 3]| {
            if let Some((x, y)) = raster.cell_of(p) {
                image.put_pixel(x as u32, (GRID - 1 - y) as u32, image::Rgb(c));
            }
        };
        for k in 0..100 {
            let t = k as f32 / 100.0;
            plot(Vec2::new(low.x + (high.x - low.x) * t, low.y), [0, 0, 255]);
            plot(Vec2::new(low.x + (high.x - low.x) * t, high.y), [0, 0, 255]);
            plot(Vec2::new(low.x, low.y + (high.y - low.y) * t), [0, 0, 255]);
            plot(Vec2::new(high.x, low.y + (high.y - low.y) * t), [0, 0, 255]);
            plot(Vec2::new(centre.x - width * 0.5 + width * t, centre.y), [255, 0, 0]);
        }
    }
    for e in &eyes {
        for k in 0..200 {
            let a = k as f32 / 200.0 * std::f32::consts::TAU;
            let p = e.centre + e.radius * Vec2::new(a.cos(), a.sin());
            if let Some((x, y)) = raster.cell_of(p) {
                image.put_pixel(x as u32, (GRID - 1 - y) as u32, image::Rgb([0, 255, 0]));
            }
        }
    }
    image.save(path)?;
    Ok(())
}

/// Columnas y filas de cada párpado.
const COLUMNS: usize = 14;
const ROWS: usize = 7;

pub fn rig_face(model: &mut Model) -> Result<Report> {
    let (eyes, raster, size, skin) = find_eyes(model)?;
    let skeleton = model.skeleton.as_ref().context("el modelo no tiene esqueleto")?;
    let head = skeleton.bone("head").context("el esqueleto no tiene cabeza")? as u16;
    if model.layers.iter().any(|l| l.name == "párpados") {
        anyhow::bail!("el modelo ya tiene párpados");
    }
    // Un poco por delante de la cara, para que no se hundan en ella.
    let lift = size * 0.012;

    // Materiales: piel y pestañas (el color más oscuro alrededor de los ojos).
    let mut lash = [0.08, 0.06, 0.06];
    let mut darkest = f32::MAX;
    for e in &eyes {
        for k in 0..64 {
            let a = k as f32 / 64.0 * std::f32::consts::PI;
            let p = e.centre + Vec2::new(e.radius.x * a.cos(), e.radius.y * a.sin());
            if let Some(c) = raster.cell_of(p).and_then(|(x, y)| raster.get(x, y)) {
                let l = lab(c.color).x;
                if l < darkest {
                    darkest = l;
                    lash = c.color;
                }
            }
        }
    }
    let material = |name: &str, c: [f32; 3]| Material {
        name: name.into(),
        // El color base de glTF es lineal; el de la textura, sRGB.
        base_color: {
            let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
            [lin(c[0]), lin(c[1]), lin(c[2]), 1.0]
        },
        texture: None,
        alpha: AlphaMode::Opaque,
        double_sided: true,
    };
    let skin_material = model.materials.len();
    model.materials.push(material("párpado", skin));
    let lash_material = model.materials.len();
    model.materials.push(material("pestañas", lash));

    // Rejillas: en cada ojo, el párpado superior baja desde arriba y el inferior sube desde
    // abajo. Las posiciones son la forma cerrada; la abierta las pliega en su borde de origen.
    let morph_names = ["blink", "blinkLeft", "blinkRight", "happy"];
    let mut skin_prim = Primitive { vertices: Vec::new(), indices: Vec::new(), material: skin_material, joints: Vec::new(), weights: Vec::new(), morphs: Vec::new() };
    let mut lash_prim = Primitive { vertices: Vec::new(), indices: Vec::new(), material: lash_material, joints: Vec::new(), weights: Vec::new(), morphs: Vec::new() };
    let mut skin_morphs: Vec<Vec<[f32; 3]>> = vec![Vec::new(); morph_names.len()];
    let mut lash_morphs: Vec<Vec<[f32; 3]>> = vec![Vec::new(); morph_names.len()];
    let surface = |p: Vec2| -> Vec3 {
        let z = raster.depth_at(p).unwrap_or(0.0);
        Vec3::new(p.x, p.y, z + lift)
    };
    // El personaje mira hacia +Z: su ojo izquierdo está en +X.
    let left_index = if eyes[0].centre.x > eyes[1].centre.x { 0 } else { 1 };
    for (e, eye) in eyes.iter().enumerate() {
        let is_left = e == left_index;
        let margin = Vec2::new(eye.radius.x * 0.32, eye.radius.y * 0.3);
        let (x0, x1) = (eye.centre.x - eye.radius.x - margin.x, eye.centre.x + eye.radius.x + margin.x);
        let reach = eye.radius.y + margin.y;
        // Normal de la cara en un punto, por la pendiente de su profundidad.
        let step = eye.radius.x * 0.3;
        let normal_at = |p: Vec2| -> [f32; 3] {
            let dz = |d: Vec2| raster.depth_at(p + d).unwrap_or(0.0) - raster.depth_at(p - d).unwrap_or(0.0);
            Vec3::new(-dz(Vec2::new(step, 0.0)) / (2.0 * step), -dz(Vec2::new(0.0, step)) / (2.0 * step), 1.0).normalize().to_array()
        };
        // Línea del arco de los ojos felices (^^): más alta en el centro.
        let arch = |u: f32| eye.centre.y - eye.radius.y * 0.15 + eye.radius.y * 0.55 * (1.0 - u * u);
        for upper in [true, false] {
            // Fila 0: el borde fijo (arriba o abajo); fila ROWS: el borde que se mueve.
            for row in 0..=ROWS {
                let f = row as f32 / ROWS as f32;
                for col in 0..=COLUMNS {
                    let u = col as f32 / COLUMNS as f32 * 2.0 - 1.0;
                    // Bordes redondeados, siguiendo el contorno del ojo.
                    let round = 0.6 + 0.4 * (1.0 - u * u).max(0.0).sqrt();
                    let top = eye.centre.y + reach * round;
                    let bottom = eye.centre.y - reach * round;
                    let edge = if upper { top } else { bottom };
                    // Los extremos se estrechan hacia el rabillo del ojo.
                    let x = eye.centre.x + u * (x1 - x0) * 0.5;
                    let taper = 1.0 - 0.35 * u * u;
                    let closed_y = top + (bottom - top) * f * taper;
                    let happy_y = if upper { top + (arch(u) - top) * f } else { bottom + (arch(u) - bottom) * f };
                    let open = surface(Vec2::new(x, edge));
                    let closed = surface(Vec2::new(x, closed_y));
                    let happy = surface(Vec2::new(x, happy_y));
                    let blink = if upper { closed - open } else { Vec3::ZERO };
                    let deltas = [
                        blink,
                        if is_left { blink } else { Vec3::ZERO },
                        if is_left { Vec3::ZERO } else { blink },
                        happy - open,
                    ];
                    let normal = normal_at(Vec2::new(x, if upper { closed_y } else { happy_y }));
                    // La última fila de quads del párpado superior son las pestañas.
                    let push = |prim: &mut Primitive, morphs: &mut Vec<Vec<[f32; 3]>>| {
                        prim.vertices.push(Vertex { position: open.to_array(), normal, uv: [0.0, 0.0] });
                        prim.joints.push([head, 0, 0, 0]);
                        prim.weights.push([1.0, 0.0, 0.0, 0.0]);
                        for (m, d) in morphs.iter_mut().zip(deltas) {
                            m.push(d.to_array());
                        }
                    };
                    push(&mut skin_prim, &mut skin_morphs);
                    if upper && row >= ROWS - 1 {
                        push(&mut lash_prim, &mut lash_morphs);
                    }
                }
            }
            // Triángulos: la rejilla de piel, menos la última fila del párpado superior, que va
            // en pestañas.
            let stride = COLUMNS + 1;
            let skin_base = skin_prim.vertices.len() - (ROWS + 1) * stride;
            let skin_rows = if upper { ROWS - 1 } else { ROWS };
            for row in 0..skin_rows {
                for col in 0..COLUMNS {
                    let a = (skin_base + row * stride + col) as u32;
                    let b = a + 1;
                    let c = a + stride as u32;
                    let d = c + 1;
                    // Las filas del párpado inferior suben: el otro sentido, para que mire afuera.
                    if upper {
                        skin_prim.indices.extend_from_slice(&[a, c, b, b, c, d]);
                    } else {
                        skin_prim.indices.extend_from_slice(&[a, b, c, b, d, c]);
                    }
                }
            }
            if upper {
                let lash_base = (lash_prim.vertices.len() - 2 * stride) as u32;
                for col in 0..COLUMNS as u32 {
                    let a = lash_base + col;
                    let b = a + 1;
                    let c = a + stride as u32;
                    let d = c + 1;
                    lash_prim.indices.extend_from_slice(&[a, c, b, b, c, d]);
                }
            }
        }
    }
    for (prim, morphs) in [(&mut skin_prim, skin_morphs), (&mut lash_prim, lash_morphs)] {
        prim.morphs = morph_names.iter().zip(morphs).map(|(n, deltas)| Morph { name: n.to_string(), deltas, uv_deltas: Vec::new() }).collect();
    }
    model.layers.push(Layer {
        name: "párpados".into(),
        parent: None,
        depth: 0,
        primitives: vec![skin_prim, lash_prim],
        visible: true,
        opacity: 1.0,
        expanded: false,
    });

    let (centre, width, found) = find_mouth(&raster, &eyes, skin);
    let mouth_material = model.materials.len();
    model.materials.push(material("boca", [0.42, 0.12, 0.14]));
    // Interior de la boca: un abanico elíptico pegado a la cara, plegado en la línea de los
    // labios cuando está cerrada; cada vocal lo abre con otra forma (ancho, alto).
    const RING: usize = 20;
    let shapes: [(&str, f32, f32); 5] = [("aa", 0.5, 0.55), ("ih", 0.5, 0.22), ("ou", 0.25, 0.32), ("ee", 0.55, 0.16), ("oh", 0.35, 0.52)];
    let mouth_lift = lift * 1.5;
    let surface = |p: Vec2| -> Vec3 { Vec3::new(p.x, p.y, raster.depth_at(p).unwrap_or(0.0) + mouth_lift) };
    let mut mouth = Primitive { vertices: Vec::new(), indices: Vec::new(), material: mouth_material, joints: Vec::new(), weights: Vec::new(), morphs: Vec::new() };
    let mut deltas: Vec<Vec<[f32; 3]>> = vec![Vec::new(); shapes.len()];
    for k in 0..=RING {
        // El 0 es el centro; del 1 al RING, el contorno.
        let angle = (k as f32 - 1.0) / RING as f32 * std::f32::consts::TAU;
        let ring = Vec2::new(angle.cos(), angle.sin());
        let closed = if k == 0 { centre } else { centre + Vec2::new(ring.x * width * 0.5, 0.0) };
        let open = surface(closed);
        mouth.vertices.push(Vertex { position: open.to_array(), normal: [0.0, 0.0, 1.0], uv: [0.0, 0.0] });
        mouth.joints.push([head, 0, 0, 0]);
        mouth.weights.push([1.0, 0.0, 0.0, 0.0]);
        for (d, (_, w, h)) in deltas.iter_mut().zip(shapes) {
            // La boca se abre sobre todo hacia abajo, como la mandíbula.
            let shape = if k == 0 { centre - Vec2::new(0.0, h * width * 0.3) } else {
                centre + Vec2::new(ring.x * w * width, (ring.y - 0.6) * h * width * 0.5)
            };
            d.push((surface(shape) - open).to_array());
        }
    }
    for k in 1..=RING as u32 {
        let next = if k == RING as u32 { 1 } else { k + 1 };
        mouth.indices.extend_from_slice(&[0, k, next]);
    }
    mouth.morphs = shapes.iter().zip(deltas).map(|((n, _, _), d)| Morph { name: n.to_string(), deltas: d, uv_deltas: Vec::new() }).collect();
    model.layers.push(Layer {
        name: "boca".into(),
        parent: None,
        depth: 0,
        primitives: vec![mouth],
        visible: true,
        opacity: 1.0,
        expanded: false,
    });
    add_mouth_drawings(model, &raster, head as usize, size, lift, skin, centre, width)?;

    // Los huesos de los ojos, a los ojos encontrados (un poco por detrás de la superficie, en
    // el centro del globo): así funciona la mirada (`lookAt`) de VRM.
    if let Some(skeleton) = model.skeleton.as_mut() {
        let left = if eyes[0].centre.x > eyes[1].centre.x { 0 } else { 1 };
        for (name, e) in [("leftEye", left), ("rightEye", 1 - left)] {
            if let Some(b) = skeleton.bone(name) {
                let eye = &eyes[e];
                let z = raster.depth_at(eye.centre).unwrap_or(0.0) - eye.radius.x * 1.5;
                skeleton.bones[b].rest_world.w_axis = glam::Vec4::new(eye.centre.x, eye.centre.y, z, 1.0);
            }
        }
    }
    Ok(Report { eyes, mouth: (centre, width, found) })
}

//! Ruta desde imagen, paso 4: pelo y falda con volumen a partir de las partes de la imagen.
//!
//! **Pelo** (todo va con la cabeza; luego `hair::rig_hair` le pone mechones, cadenas y física):
//! - casquete: los triángulos del cráneo del cuerpo ajustado que en la imagen son pelo, más la
//!   parte de atrás y la nuca, duplicados un poco hacia fuera;
//! - melena: una rejilla sobre la máscara de pelo por debajo de los ojos, con dos láminas y
//!   paredes en el borde para que tenga grosor. Donde el cuerpo está detrás (el pelo se ve
//!   encima), va justo delante del cuerpo; donde no (cae por los lados), por detrás del plano de
//!   la cabeza y más gruesa;
//! - cortina por la espalda: en las alturas con pelo a los dos lados del cuerpo, una lámina por
//!   detrás de la espalda.
//!
//! La cara de delante lleva la imagen; lo que no se ve en ella (detrás, paredes), el color
//! mediano del pelo.
//!
//! **Falda**: si la ropa sigue desde la cintura bastante hacia abajo, un cono de anillos con el
//! ancho de la silueta en cada altura (sin meterse en las piernas), con la imagen. Se llama
//! "ropa cintura falda" para que `hair::rig_skirt` le ponga cadenas.

use crate::analyze::{Analysis, Part};
use crate::fit::Projection;
use crate::model::{AlphaMode, Layer, Material, Model, Primitive, Vertex};
use anyhow::{Context, Result};
use glam::Vec3;
use image::RgbaImage;

fn linear(c: u8) -> f32 {
    let v = c as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn dominant(j: &[u16; 4], w: &[f32; 4]) -> usize {
    j[(0..4).max_by(|a, b| w[*a].total_cmp(&w[*b])).unwrap_or(0)] as usize
}

/// Profundidad del cuerpo en la pose de la proyección, por celdas de la imagen: lo más
/// delantero y lo más trasero.
struct BodyDepth {
    columns: usize,
    rows: usize,
    front: Vec<f32>,
    back: Vec<f32>,
}

impl BodyDepth {
    fn new(model: &Model, projection: &Projection, cell: f32) -> Self {
        let columns = (projection.width as f32 / cell).ceil() as usize + 1;
        let rows = (projection.height as f32 / cell).ceil() as usize + 1;
        let mut depth = BodyDepth { columns, rows, front: vec![f32::MIN; columns * rows], back: vec![f32::MAX; columns * rows] };
        let posed = crate::pose::skin_model(model, &projection.local, &[]);
        for (layer, posed_layer) in model.layers.iter().zip(&posed) {
            for (p, q) in layer.primitives.iter().zip(posed_layer) {
                for tri in p.indices.chunks_exact(3) {
                    let pts = [0, 1, 2].map(|k| Vec3::from(q[tri[k] as usize].position));
                    let px = pts.map(|v| {
                        let (c, r) = projection.pixel(v);
                        glam::Vec2::new(c / cell, r / cell)
                    });
                    let lo = px[0].min(px[1]).min(px[2]).floor().max(glam::Vec2::ZERO);
                    let hi = px[0].max(px[1]).max(px[2]).ceil();
                    let area = (px[1] - px[0]).perp_dot(px[2] - px[0]);
                    if area.abs() < 1e-9 {
                        continue;
                    }
                    for r in lo.y as usize..=(hi.y as usize).min(rows - 1) {
                        for c in lo.x as usize..=(hi.x as usize).min(columns - 1) {
                            let s = glam::Vec2::new(c as f32 + 0.5, r as f32 + 0.5);
                            let w0 = (px[1] - s).perp_dot(px[2] - s) / area;
                            let w1 = (px[2] - s).perp_dot(px[0] - s) / area;
                            let w2 = 1.0 - w0 - w1;
                            if w0 < -0.05 || w1 < -0.05 || w2 < -0.05 {
                                continue;
                            }
                            let z = pts[0].z * w0 + pts[1].z * w1 + pts[2].z * w2;
                            let i = r * columns + c;
                            depth.front[i] = depth.front[i].max(z);
                            depth.back[i] = depth.back[i].min(z);
                        }
                    }
                }
            }
        }
        depth
    }

    fn at(&self, column: usize, row: usize) -> Option<(f32, f32)> {
        let i = row.min(self.rows - 1) * self.columns + column.min(self.columns - 1);
        (self.front[i] > f32::MIN).then(|| (self.front[i], self.back[i]))
    }
}

/// Añade el pelo con volumen. Devuelve cuántos triángulos de casquete y de melena.
pub fn add_hair(model: &mut Model, image: &RgbaImage, analysis: &Analysis, projection: &Projection) -> Result<(usize, usize)> {
    let skeleton = model.skeleton.as_ref().context("el modelo no tiene esqueleto")?;
    let head = skeleton.bone("head").context("sin cabeza")?;
    let head_z = skeleton.rest_position(head).z;
    let k = &analysis.keypoints;
    let (w, h) = (image.width() as usize, image.height() as usize);
    let head_size = (k.chin - k.top) as f32 * projection.scale;
    let label = |c: f32, r: f32| -> Part {
        if c < 0.0 || r < 0.0 || c as usize >= w || r as usize >= h {
            return Part::Fondo;
        }
        analysis.labels[r as usize * w + c as usize]
    };

    // Color mediano del pelo, para lo que no se ve en la imagen.
    let mut hair_pixels: Vec<[u8; 3]> = image.pixels().zip(&analysis.labels).filter(|(_, l)| **l == Part::Pelo).map(|(p, _)| [p[0], p[1], p[2]]).collect();
    anyhow::ensure!(hair_pixels.len() > 50, "la imagen no tiene pelo reconocible");
    hair_pixels.sort_by_key(|c| c[0] as u32 + c[1] as u32 + c[2] as u32);
    let median = hair_pixels[hair_pixels.len() / 2];
    let image_material = 1; // "pelo", con la imagen (ver `fit::project`)
    let solid_material = model.materials.len();
    model.materials.push(Material {
        name: "pelo (detrás)".into(),
        base_color: [linear(median[0]), linear(median[1]), linear(median[2]), 1.0],
        texture: None,
        alpha: AlphaMode::Opaque,
        double_sided: true,
    });
    let make = |material: usize| Primitive { vertices: Vec::new(), indices: Vec::new(), material, joints: Vec::new(), weights: Vec::new(), morphs: Vec::new() };
    let push = |p: &mut Primitive, position: Vec3, normal: Vec3| -> u32 {
        p.vertices.push(Vertex { position: position.to_array(), normal: normal.to_array(), uv: projection.uv(position) });
        p.joints.push([head as u16, 0, 0, 0]);
        p.weights.push([1.0, 0.0, 0.0, 0.0]);
        (p.vertices.len() - 1) as u32
    };

    // 1. Casquete.
    let chin_y = projection.point(0.0, k.chin as f32).1;
    let mut cap_front = make(image_material);
    let mut cap_back = make(solid_material);
    let lift = head_size * 0.05;
    for layer in &model.layers {
        for p in &layer.primitives {
            let chosen = |i: usize| -> bool {
                if dominant(&p.joints[i], &p.weights[i]) != head {
                    return false;
                }
                let v = &p.vertices[i];
                let (q, n) = (Vec3::from(v.position), Vec3::from(v.normal));
                let (c, r) = projection.pixel(q);
                let up = q.y - chin_y;
                (up > head_size * 0.45 && label(c, r) == Part::Pelo) || (n.z < -0.15 && up > head_size * 0.1)
            };
            for tri in p.indices.chunks_exact(3) {
                let ids = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
                if !ids.iter().all(|&i| chosen(i)) {
                    continue;
                }
                let facing = ids.iter().map(|&i| p.vertices[i].normal[2]).sum::<f32>() / 3.0;
                let target = if facing > 0.0 { &mut cap_front } else { &mut cap_back };
                let new: Vec<u32> = ids
                    .iter()
                    .map(|&i| {
                        let v = &p.vertices[i];
                        let n = Vec3::from(v.normal);
                        push(target, Vec3::from(v.position) + n * lift, n)
                    })
                    .collect();
                target.indices.extend_from_slice(&new);
            }
        }
    }
    let cap_triangles = (cap_front.indices.len() + cap_back.indices.len()) / 3;

    // 2. Melena: celdas de la máscara de pelo por debajo de los ojos.
    let cell = (h as f32 / 160.0).max(3.0);
    let depth = BodyDepth::new(model, projection, cell);
    let (columns, rows) = (depth.columns, depth.rows);
    let eyes_row = k.top as f32 + 0.55 * (k.chin - k.top) as f32;
    let mut hair_cell = vec![false; columns * rows];
    for r in 0..rows {
        for c in 0..columns {
            if (r as f32 + 0.5) * cell < eyes_row {
                continue;
            }
            let mut hits = 0;
            let mut total = 0;
            for dy in 0..cell as usize {
                for dx in 0..cell as usize {
                    let (x, y) = (c as f32 * cell + dx as f32, r as f32 * cell + dy as f32);
                    total += 1;
                    if label(x, y) == Part::Pelo {
                        hits += 1;
                    }
                }
            }
            hair_cell[r * columns + c] = hits * 2 > total;
        }
    }
    // Esquinas de la rejilla: profundidad (delante del cuerpo o por detrás de la cabeza) y grosor.
    let corners = (columns + 1) * (rows + 1);
    let mut used = vec![false; corners];
    for r in 0..rows {
        for c in 0..columns {
            if hair_cell[r * columns + c] {
                for (dc, dr) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    used[(r + dr) * (columns + 1) + c + dc] = true;
                }
            }
        }
    }
    let mut z = vec![0.0f32; corners];
    let mut thickness = vec![0.0f32; corners];
    for i in 0..corners {
        if !used[i] {
            continue;
        }
        let (c, r) = (i % (columns + 1), i / (columns + 1));
        let body = depth.at(c.saturating_sub(1), r.saturating_sub(1)).or_else(|| depth.at(c, r));
        (z[i], thickness[i]) = match body {
            Some((front, _)) => (front + head_size * 0.03, head_size * 0.04),
            None => (head_z - head_size * 0.15, head_size * 0.25),
        };
    }
    // Suavizado para que no haya escalones entre delante y detrás.
    for _ in 0..6 {
        let previous = (z.clone(), thickness.clone());
        for i in 0..corners {
            if !used[i] {
                continue;
            }
            let (c, r) = (i % (columns + 1), i / (columns + 1));
            let mut sum = (previous.0[i], previous.1[i]);
            let mut n = 1.0;
            for (dc, dr) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let (nc, nr) = (c as i32 + dc, r as i32 + dr);
                if nc >= 0 && nr >= 0 && (nc as usize) <= columns && (nr as usize) <= rows {
                    let j = nr as usize * (columns + 1) + nc as usize;
                    if used[j] {
                        sum.0 += previous.0[j];
                        sum.1 += previous.1[j];
                        n += 1.0;
                    }
                }
            }
            z[i] = sum.0 / n;
            thickness[i] = sum.1 / n;
        }
    }
    let mut sheet_front = make(image_material);
    let mut sheet_back = make(solid_material);
    let mut front_index = vec![u32::MAX; corners];
    let mut back_index = vec![u32::MAX; corners];
    for i in 0..corners {
        if !used[i] {
            continue;
        }
        let (c, r) = (i % (columns + 1), i / (columns + 1));
        let (x, y) = projection.point(c as f32 * cell, r as f32 * cell);
        front_index[i] = push(&mut sheet_front, Vec3::new(x, y, z[i]), Vec3::Z);
        back_index[i] = push(&mut sheet_back, Vec3::new(x, y, z[i] - thickness[i]), -Vec3::Z);
    }
    let corner = |c: usize, r: usize| r * (columns + 1) + c;
    for r in 0..rows {
        for c in 0..columns {
            if !hair_cell[r * columns + c] {
                continue;
            }
            let (a, b, d, e) = (corner(c, r), corner(c + 1, r), corner(c, r + 1), corner(c + 1, r + 1));
            // Delante mirando hacia +Z (filas hacia abajo, columnas hacia +X), detrás al revés.
            sheet_front.indices.extend_from_slice(&[front_index[a], front_index[d], front_index[b], front_index[b], front_index[d], front_index[e]]);
            sheet_back.indices.extend_from_slice(&[back_index[a], back_index[b], back_index[d], back_index[b], back_index[e], back_index[d]]);
            // Paredes en los bordes de la melena.
            for (edge, neighbour) in [((a, b), (c as i32, r as i32 - 1)), ((d, e), (c as i32, r as i32 + 1)), ((a, d), (c as i32 - 1, r as i32)), ((b, e), (c as i32 + 1, r as i32))] {
                let (nc, nr) = neighbour;
                let outside = nc < 0 || nr < 0 || nc as usize >= columns || nr as usize >= rows || !hair_cell[nr as usize * columns + nc as usize];
                if outside {
                    let (p0, p1) = edge;
                    let f0 = Vec3::from(sheet_front.vertices[front_index[p0] as usize].position);
                    let f1 = Vec3::from(sheet_front.vertices[front_index[p1] as usize].position);
                    let b0 = Vec3::from(sheet_back.vertices[back_index[p0] as usize].position);
                    let b1 = Vec3::from(sheet_back.vertices[back_index[p1] as usize].position);
                    let n = (f1 - f0).cross(b0 - f0).normalize_or_zero();
                    let q = [push(&mut sheet_back, f0, n), push(&mut sheet_back, f1, n), push(&mut sheet_back, b0, n), push(&mut sheet_back, b1, n)];
                    sheet_back.indices.extend_from_slice(&[q[0], q[1], q[2], q[1], q[3], q[2]]);
                }
            }
        }
    }
    // 3. Cortina por la espalda: desde la barbilla hasta la fila más baja con pelo, si la melena
    //    pasa de los hombros (aunque en la imagen solo asome por un lado: por detrás cubre la
    //    espalda), de un lado al otro de lo que haya en cada fila.
    let mut curtain = make(solid_material);
    // La fila más baja con pelo de verdad (al menos tres celdas: no un contorno suelto), y nunca
    // por debajo de la entrepierna.
    let crotch_row = (k.crotch as f32 / cell) as usize;
    let lowest_hair = (0..rows).rev().find(|r| (0..columns).filter(|c| hair_cell[r * columns + c]).count() >= 2).map(|r| r.min(crotch_row));
    let shoulders_row = (k.shoulders as f32 / cell) as usize;
    let chin_row = (k.chin as f32 / cell) as usize;
    // Si la melena llega a los hombros, por detrás se supone que baja al menos una cabeza más
    // (lo de detrás no se ve en la imagen).
    let head_rows = ((k.chin - k.top) as f32 / cell) as usize;
    let curtain_end = lowest_hair.filter(|l| *l >= shoulders_row).map_or(0, |l| l.max(shoulders_row + head_rows).min(crotch_row));
    for r in chin_row..=curtain_end {
        let span: Vec<usize> = (0..columns).filter(|c| hair_cell[r * columns + c] || depth.at(*c, r).is_some()).collect();
        let (Some(&hl), Some(&hr)) = (span.first(), span.last()) else { continue };
        // No más ancho que los hombros más el pelo de los lados.
        for c in hl..=hr {
            let back = |cc: usize, rr: usize| depth.at(cc, rr).map_or(head_z - head_size * 0.15, |(_, b)| b.min(head_z - head_size * 0.15)) - head_size * 0.04;
            let corners = [(c, r), (c + 1, r), (c, r + 1), (c + 1, r + 1)].map(|(cc, rr)| {
                let (x, y) = projection.point(cc as f32 * cell, rr as f32 * cell);
                push(&mut curtain, Vec3::new(x, y, back(cc, rr)), -Vec3::Z)
            });
            curtain.indices.extend_from_slice(&[corners[0], corners[1], corners[2], corners[1], corners[3], corners[2]]);
        }
    }
    let sheet_triangles = (sheet_front.indices.len() + sheet_back.indices.len() + curtain.indices.len()) / 3;

    let layer = |name: &str, primitives: Vec<Primitive>| Layer {
        name: name.into(),
        parent: None,
        depth: 0,
        primitives: primitives.into_iter().filter(|p| !p.indices.is_empty()).collect(),
        visible: true,
        opacity: 1.0,
        expanded: false,
    };
    model.layers.push(layer("pelo cabeza", vec![cap_front, cap_back]));
    model.layers.push(layer("pelo melena", vec![sheet_front, sheet_back, curtain]));
    model.layers.retain(|l| !l.primitives.is_empty() || !l.name.starts_with("pelo"));
    Ok((cap_triangles, sheet_triangles))
}

/// Añade la falda si la hay. Devuelve el alto de la falda en filas de la imagen (0 si no hay).
pub fn add_skirt(model: &mut Model, analysis: &Analysis, projection: &Projection) -> Result<u32> {
    let skeleton = model.skeleton.as_ref().context("el modelo no tiene esqueleto")?;
    let hips = skeleton.bone("hips").context("sin cadera")?;
    let k = &analysis.keypoints;
    let (w, h) = (analysis.width as usize, analysis.height as usize);
    let centre = k.centre_x.round() as usize;
    // Tramo continuo de personaje que pasa por el centro, y cuánto de él es ropa.
    let run = |row: usize| -> Option<(usize, usize, f32)> {
        let inside = |x: usize| analysis.labels[row * w + x] != Part::Fondo;
        if row >= h || !inside(centre) {
            return None;
        }
        let (mut l, mut r) = (centre, centre);
        while l > 0 && inside(l - 1) {
            l -= 1;
        }
        while r + 1 < w && inside(r + 1) {
            r += 1;
        }
        let clothes = (l..=r).filter(|x| analysis.labels[row * w + x] == Part::Ropa).count();
        Some((l, r, clothes as f32 / (r - l + 1) as f32))
    };
    // La falda empieza donde la silueta se ensancha bajo la cintura (más arriba aún es la camisa
    // o la cintura del cuerpo).
    let width = |row: usize| run(row).map_or(0, |(l, r, _)| r - l + 1);
    let waist = (k.waist as usize..k.hips.max(k.waist + 1) as usize)
        .find(|r| width(*r) as f32 > width(k.waist as usize) as f32 * 1.1)
        .unwrap_or(k.waist as usize);
    let mut hem = waist;
    while let Some((_, _, share)) = run(hem + 1) {
        if share < 0.85 {
            break;
        }
        hem += 1;
    }
    let head_rows = (k.chin - k.top) as usize;
    if hem < waist + head_rows / 2 || hem <= k.hips as usize {
        return Ok(0);
    }
    // Contorno del cuerpo (sin brazos) en una altura, del modelo en reposo.
    let regions_arm = |b: usize| {
        let mut current = Some(b);
        while let Some(c) = current {
            if ["leftUpperArm", "rightUpperArm"].iter().any(|n| skeleton.bone(n) == Some(c)) {
                return true;
            }
            current = skeleton.bones[c].parent;
        }
        false
    };
    let arm_bone: Vec<bool> = (0..skeleton.bones.len()).map(regions_arm).collect();
    let mut body: Vec<Vec3> = Vec::new();
    for layer in &model.layers {
        for p in &layer.primitives {
            for (v, (j, wt)) in p.vertices.iter().zip(p.joints.iter().zip(&p.weights)) {
                if !p.joints.is_empty() && !arm_bone[dominant(j, wt)] {
                    body.push(Vec3::from(v.position));
                }
            }
        }
    }
    const RING: usize = 32;
    const LEVELS: usize = 10;
    let material = model.materials.len();
    model.materials.push(Material { name: "falda".into(), base_color: [1.0; 4], texture: Some(0), alpha: AlphaMode::Opaque, double_sided: true });
    let mut skirt = Primitive { vertices: Vec::new(), indices: Vec::new(), material, joints: Vec::new(), weights: Vec::new(), morphs: Vec::new() };
    for level in 0..=LEVELS {
        let row = waist + (hem - waist) * level / LEVELS;
        let (l, r, _) = run(row).unwrap_or((centre, centre, 0.0));
        let (xl, y) = projection.point(l as f32, row as f32);
        let (xr, _) = projection.point(r as f32 + 1.0, row as f32);
        let cx = (xl + xr) * 0.5;
        let slice: Vec<&Vec3> = body.iter().filter(|q| (q.y - y).abs() < 0.015).collect();
        let body_x = slice.iter().map(|q| (q.x - cx).abs()).fold(0.0, f32::max);
        let (zmin, zmax) = slice.iter().fold((f32::MAX, f32::MIN), |(a, b), q| (a.min(q.z), b.max(q.z)));
        let (zc, body_z) = if slice.is_empty() { (0.0, 0.08) } else { ((zmin + zmax) * 0.5, (zmax - zmin) * 0.5) };
        let a = ((xr - xl) * 0.5).max(body_x + 0.01);
        let b = (a * 0.8).max(body_z + 0.015);
        for i in 0..RING {
            let t = i as f32 / RING as f32 * std::f32::consts::TAU;
            let q = Vec3::new(cx + a * t.sin(), y, zc + b * t.cos());
            let n = Vec3::new(t.sin() / a, 0.0, t.cos() / b).normalize();
            // La textura, del ancho de la falda dibujada en esa fila (el cono puede ser más ancho
            // para no meterse en las piernas, y su borde cogería piernas o fondo).
            let column = l as f32 + (0.5 + 0.5 * t.sin()) * (r - l + 1) as f32;
            let uv = [column / projection.width as f32, row as f32 / projection.height as f32];
            skirt.vertices.push(Vertex { position: q.to_array(), normal: n.to_array(), uv });
            skirt.joints.push([hips as u16, 0, 0, 0]);
            skirt.weights.push([1.0, 0.0, 0.0, 0.0]);
        }
    }
    for level in 0..LEVELS as u32 {
        for i in 0..RING as u32 {
            let a = level * RING as u32 + i;
            let b = level * RING as u32 + (i + 1) % RING as u32;
            let (c, d) = (a + RING as u32, b + RING as u32);
            skirt.indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    model.layers.push(Layer { name: "ropa cintura falda".into(), parent: None, depth: 0, primitives: vec![skirt], visible: true, opacity: 1.0, expanded: false });
    Ok((hem - waist) as u32)
}

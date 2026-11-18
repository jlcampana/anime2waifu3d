//! Pelo por mechones: cadenas de huesos para el pelo que cuelga y su física.
//!
//! `rig_hair` trabaja sobre un modelo ya riggeado (el pelo va entero con la cabeza). Reparte el
//! pelo que cuelga por debajo de la raíz (a la altura de las sienes) en sectores alrededor del
//! eje de la cabeza, uno por mechón. A cada mechón le pone una cadena de huesos
//! (`J_Sec_Hair{n}_{k}`, como en VRoid) que sigue el centro del mechón hacia abajo. Los pesos
//! se mezclan a lo largo de la cadena y entre mechones vecinos, así que la malla no se rompe.
//! Además separa cada capa de pelo en subcapas, una por mechón (más la coronilla y el
//! flequillo), para la paleta de capas.
//!
//! `HairSim` es la física en el visor: cada cadena es una lista de puntas (Verlet, a 60 pasos
//! por segundo) que vuelven a la forma de reposo con una rigidez, se quedan atrás con la inercia
//! y chocan con cápsulas del cuerpo. Sigue la idea de las cadenas de DOA3 de doa.player (un
//! segmento de longitud fija por partícula, empujado hacia una dirección de reposo, con fuerza
//! limitada y colisiones), con una dirección de reposo por segmento sacada de la malla.

use crate::model::{Layer, Model, Skeleton};
use anyhow::{Context, Result};
use glam::{Mat3, Mat4, Quat, Vec3};
use std::collections::HashMap;

pub const BONE_PREFIX: &str = "J_Sec_Hair";
/// Sectores alrededor de la cabeza, de 30°.
const SECTORS: usize = 12;
/// Segmentos como máximo por cadena.
const MAX_SEGMENTS: usize = 8;

pub struct Strand {
    pub name: String,
    pub bones: usize,
    pub vertices: usize,
}

pub struct Report {
    pub strands: Vec<Strand>,
}

/// Capas de pelo: las que están en el grupo "Pelo" de la segmentación o se llaman "pelo …".
pub fn hair_layers(model: &Model) -> Vec<bool> {
    let n = model.layers.len();
    let mut hair = vec![false; n];
    for i in 0..n {
        let mut j = Some(i);
        while let Some(k) = j {
            let name = model.layers[k].name.to_lowercase();
            if name == "pelo" || name.starts_with("pelo ") {
                hair[i] = true;
                break;
            }
            j = model.layers[k].parent;
        }
    }
    hair
}

/// Ángulo alrededor del eje vertical de la cabeza: 0 delante (+Z), positivo hacia la izquierda
/// del personaje (+X).
fn azimuth(p: Vec3, axis: Vec3) -> f32 {
    (p.x - axis.x).atan2(p.z - axis.z)
}

fn angle_diff(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(std::f32::consts::TAU);
    if d > std::f32::consts::PI { d - std::f32::consts::TAU } else { d }
}

/// Clave para soldar vértices en la misma posición (las costuras de UV los duplican).
fn weld_key(p: Vec3) -> [i32; 3] {
    (p * 1e4).round().as_ivec3().to_array()
}

/// Nombre de un mechón por su ángulo.
fn direction_name(theta: f32) -> &'static str {
    let deg = theta.to_degrees();
    match deg {
        d if d.abs() < 22.5 => "delante",
        d if (22.5..67.5).contains(&d) => "delante izq",
        d if (67.5..112.5).contains(&d) => "izq",
        d if (112.5..157.5).contains(&d) => "detrás izq",
        d if (-67.5..-22.5).contains(&d) => "delante der",
        d if (-112.5..-67.5).contains(&d) => "der",
        d if (-157.5..-112.5).contains(&d) => "detrás der",
        _ => "detrás",
    }
}

struct Chain {
    theta: f32,
    length: f32,
    joints: Vec<Vec3>,
    first_bone: usize,
}

/// En los pesos de una cadena, la parte que se queda con los pesos que ya tenía el vértice.
const KEEP: usize = usize::MAX;

impl Chain {
    /// Huesos y pesos de un punto de esta cadena por su distancia a la raíz por la malla: en la
    /// raíz, los pesos que ya tenía (`KEEP`), y de ahí hacia la punta pasa de un segmento al
    /// siguiente.
    fn weights(&self, distance: f32) -> [(usize, f32); 2] {
        let segments = self.joints.len() - 1;
        let j = (distance / self.length - 1.0).clamp(-1.0, segments as f32 - 1.0);
        let low = j.floor();
        let frac = j - low;
        let bone = |i: f32| if i < 0.0 { KEEP } else { self.first_bone + i as usize };
        [(bone(low), 1.0 - frac), (bone((low + 1.0).min(segments as f32 - 1.0)), frac)]
    }
}

/// Qué cadenas poner y dónde: el pelo cuelga de la cabeza, la falda de la cadera.
struct Spec<'a> {
    prefix: &'static str,
    /// Hueso del que cuelgan y punto de su eje vertical.
    anchor: usize,
    centre: Vec3,
    /// Capas que entran y si solo cuentan los vértices que van enteros con el ancla.
    layers: Vec<bool>,
    anchor_only: bool,
    /// Altura de la raíz según el ángulo.
    root: Box<dyn Fn(f32) -> f32 + 'a>,
    segment_length: f32,
    /// Cuánto se queda cada punto con los pesos que ya tenía (1, todo): lo pegado a la piel.
    keep: Box<dyn Fn(Vec3) -> f32 + 'a>,
    /// Nombre de la cadena por su ángulo.
    name: Box<dyn Fn(f32) -> String + 'a>,
    /// Segmentos de las piernas. Si hay, lo ceñido solo se queda con la pierna si la rodea (como
    /// unas medias): una zona ceñida que no da la vuelta a su hueso es un trozo de falda o de
    /// abrigo que roza el muslo, y cuelga.
    legs: Vec<(Vec3, Vec3)>,
}

/// Resultado: nombres de las cadenas, vértices de cada una y, por capa, primitiva y vértice,
/// su cadena (`>= 0`), `-1` si está en la raíz y `-2` si no entra.
struct Built {
    names: Vec<String>,
    bones: Vec<usize>,
    counts: Vec<usize>,
    owner: Vec<Vec<Vec<i32>>>,
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let f = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    f * f * (3.0 - 2.0 * f)
}

/// Pone cadenas que cuelgan del ancla a los vértices de `spec` y les cambia los pesos.
fn build_chains(model: &mut Model, spec: &Spec) -> Result<Built> {
    let skeleton = model.skeleton.as_mut().context("el modelo no tiene esqueleto")?;
    if skeleton.bones.iter().any(|b| b.name.starts_with(spec.prefix)) {
        anyhow::bail!("el modelo ya tiene cadenas {}", spec.prefix);
    }
    let anchor = spec.anchor;
    let eligible = |j: &[u16; 4], w: &[f32; 4]| !spec.anchor_only || (j[0] as usize == anchor && w[0] > 0.999);
    let empty = || Built { names: Vec::new(), bones: Vec::new(), counts: Vec::new(), owner: Vec::new() };

    // Vértices soldados por posición y las aristas entre ellos.
    let mut points: Vec<Vec3> = Vec::new();
    let mut node_of: HashMap<[i32; 3], usize> = HashMap::new();
    let mut edges: Vec<Vec<usize>> = Vec::new();
    let mut node_layer: Vec<usize> = Vec::new();
    for (l, layer) in model.layers.iter().enumerate() {
        if !spec.layers[l] {
            continue;
        }
        for p in &layer.primitives {
            if p.joints.is_empty() {
                continue;
            }
            let nodes: Vec<Option<usize>> = p
                .vertices
                .iter()
                .zip(p.joints.iter().zip(&p.weights))
                .map(|(v, (j, w))| {
                    eligible(j, w).then(|| {
                        let position = Vec3::from(v.position);
                        *node_of.entry(weld_key(position)).or_insert_with(|| {
                            points.push(position);
                            edges.push(Vec::new());
                            node_layer.push(l);
                            points.len() - 1
                        })
                    })
                })
                .collect();
            for tri in p.indices.chunks_exact(3) {
                for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                    if let (Some(a), Some(b)) = (nodes[a as usize], nodes[b as usize]) {
                        if a != b && !edges[a].contains(&b) {
                            edges[a].push(b);
                            edges[b].push(a);
                        }
                    }
                }
            }
        }
    }
    if points.is_empty() {
        return Ok(empty());
    }
    let centre = spec.centre;
    let segment_length = spec.segment_length;

    // Distancia de cada punto a la raíz por la malla (Dijkstra desde todo lo que está por encima
    // de la raíz): sigue a las coletas y a lo que sale de lado o se curva. Las piezas sueltas
    // que no llegan a la raíz usan la altura bajo ella.
    let mut distance: Vec<f32> = vec![f32::INFINITY; points.len()];
    let mut queue = std::collections::BinaryHeap::new();
    for (i, p) in points.iter().enumerate() {
        if p.y >= (spec.root)(azimuth(*p, centre)) {
            distance[i] = 0.0;
            queue.push(std::cmp::Reverse((0u32, i)));
        }
    }
    while let Some(std::cmp::Reverse((d, i))) = queue.pop() {
        let d = f32::from_bits(d);
        if d > distance[i] {
            continue;
        }
        for &n in &edges[i] {
            let nd = d + points[i].distance(points[n]);
            if nd < distance[n] {
                distance[n] = nd;
                // Los f32 positivos se ordenan igual que sus bits.
                queue.push(std::cmp::Reverse((nd.to_bits(), n)));
            }
        }
    }
    for (i, p) in points.iter().enumerate() {
        if !distance[i].is_finite() {
            distance[i] = ((spec.root)(azimuth(*p, centre)) - p.y).max(0.0);
        }
    }
    let mut keep: Vec<f32> = points.iter().map(|p| (spec.keep)(*p)).collect();
    if !spec.legs.is_empty() {
        // Ceñido o suelto, con un filtro de mayoría sobre la malla: fronteras limpias para el
        // corte, sin vértices sueltos alternos que dejarían tiras.
        let mut tight: Vec<bool> = keep.iter().map(|k| *k >= 0.5).collect();
        for _ in 0..4 {
            tight = (0..points.len())
                .map(|i| {
                    let yes = edges[i].iter().filter(|&&n| tight[n]).count() + tight[i] as usize;
                    yes * 2 > edges[i].len() + 1
                })
                .collect();
        }
        for (k, t) in keep.iter_mut().zip(&tight) {
            *k = if *t { 1.0 } else { 0.0 };
        }
        let mut piece = vec![usize::MAX; points.len()];
        for start in 0..points.len() {
            if piece[start] != usize::MAX || keep[start] < 0.5 {
                continue;
            }
            // Zona ceñida conectada.
            let mut stack = vec![start];
            piece[start] = start;
            let mut members = Vec::new();
            while let Some(i) = stack.pop() {
                members.push(i);
                for &n in &edges[i] {
                    if piece[n] == usize::MAX && keep[n] >= 0.5 {
                        piece[n] = start;
                        stack.push(n);
                    }
                }
            }
            // Ángulos que cubre alrededor de su propio centro (sectores de 30°), por tramos de
            // cada hueso de la pierna: unas medias dan la vuelta entera, un trozo de abrigo no.
            let mut groups: HashMap<(usize, usize), Vec<Vec3>> = HashMap::new();
            for &i in &members {
                let p = points[i];
                let (s, t) = spec
                    .legs
                    .iter()
                    .enumerate()
                    .map(|(s, (a, b))| {
                        let ab = *b - *a;
                        let t = ((p - *a).dot(ab) / ab.length_squared().max(1e-10)).clamp(0.0, 1.0);
                        (s, t, p.distance(*a + ab * t))
                    })
                    .min_by(|x, y| x.2.total_cmp(&y.2))
                    .map(|(s, t, _)| (s, t))
                    .unwrap();
                groups.entry((s, (t * 8.0) as usize)).or_default().push(p);
            }
            let covered: Vec<u16> = groups
                .values()
                .filter(|g| g.len() >= 12)
                .map(|g| {
                    let centre = g.iter().copied().sum::<Vec3>() / g.len() as f32;
                    g.iter().fold(0u16, |bits, p| {
                        let t = azimuth(*p, centre).rem_euclid(std::f32::consts::TAU);
                        bits | 1 << ((t / std::f32::consts::TAU * 12.0) as u32 % 12)
                    })
                })
                .collect();
            let wraps = covered.iter().any(|c| c.count_ones() >= 8);
            if !wraps {
                for i in members {
                    keep[i] = 0.0;
                }
            }
        }
        // Voto por capa: una capa que es casi toda ceñida (medias, botas) o casi toda suelta (un
        // abrigo) va entera de ese lado; así el corte cae en la frontera entre capas.
        let mut votes: HashMap<usize, (usize, usize)> = HashMap::new();
        for i in 0..points.len() {
            if distance[i] > 0.0 {
                let e = votes.entry(node_layer[i]).or_default();
                if keep[i] >= 0.5 { e.0 += 1 } else { e.1 += 1 }
            }
        }
        for i in 0..points.len() {
            if let Some(&(tight, loose)) = votes.get(&node_layer[i]) {
                if tight * 5 >= (tight + loose) * 3 {
                    keep[i] = 1.0;
                } else if loose * 5 >= (tight + loose) * 3 {
                    keep[i] = 0.0;
                }
            }
        }
    }
    // Sectores con bastante de lo que cuelga suelto, y su ángulo medio.
    let hanging: Vec<usize> = (0..points.len()).filter(|&i| distance[i] > 0.5 * segment_length && keep[i] < 0.5).collect();
    let mut bins: Vec<Vec<usize>> = vec![Vec::new(); SECTORS];
    for &i in &hanging {
        let t = azimuth(points[i], centre).rem_euclid(std::f32::consts::TAU);
        bins[(t / std::f32::consts::TAU * SECTORS as f32) as usize % SECTORS].push(i);
    }
    let mut chains: Vec<Chain> = Vec::new();
    for bin in &bins {
        if bin.len() < 30 || bin.len() * 80 < hanging.len() {
            continue;
        }
        let (s, c) = bin.iter().fold((0.0, 0.0), |(s, c), &i| {
            let t = azimuth(points[i], centre);
            (s + t.sin(), c + t.cos())
        });
        chains.push(Chain { theta: s.atan2(c), length: segment_length, joints: Vec::new(), first_bone: 0 });
    }
    if chains.is_empty() {
        return Ok(empty());
    }
    let thetas: Vec<f32> = chains.iter().map(|c| c.theta).collect();
    let nearest_two = |theta: f32| -> [(usize, f32); 2] {
        let mut order: Vec<(usize, f32)> = thetas.iter().enumerate().map(|(i, t)| (i, angle_diff(theta, *t).abs())).collect();
        order.sort_by(|a, b| a.1.total_cmp(&b.1));
        match order.as_slice() {
            [(a, da), (b, db), ..] => {
                let total = da + db;
                let wa = if total > 1e-6 { db / total } else { 1.0 };
                [(*a, wa), (*b, 1.0 - wa)]
            }
            [(a, _)] => [(*a, 1.0), (*a, 0.0)],
            [] => unreachable!(),
        }
    };

    // Articulaciones: el centro de lo suelto de cada cadena en cada nivel de distancia.
    let owner_of: Vec<usize> = points.iter().map(|p| nearest_two(azimuth(*p, centre))[0].0).collect();
    for (c, chain) in chains.iter_mut().enumerate() {
        let mine: Vec<usize> = (0..points.len()).filter(|&i| owner_of[i] == c && keep[i] < 0.5).collect();
        let extent = mine.iter().map(|&i| distance[i]).fold(0.0, f32::max);
        let segments = ((extent / segment_length).ceil() as usize).clamp(2, MAX_SEGMENTS);
        chain.length = extent.max(segment_length) / segments as f32;
        let half = chain.length * 0.5;
        for k in 0..=segments {
            let level = k as f32 * chain.length;
            let band: Vec<Vec3> = mine
                .iter()
                .filter(|&&i| if k == segments { distance[i] > level - half } else { (distance[i] - level).abs() < half })
                .map(|&i| points[i])
                .collect();
            let joint = if band.is_empty() {
                match chain.joints.as_slice() {
                    [.., a, b] => *b + (*b - *a),
                    [.., b] => *b - Vec3::Y * chain.length,
                    [] => Vec3::new(centre.x + chain.theta.sin() * segment_length, (spec.root)(chain.theta), centre.z + chain.theta.cos() * segment_length),
                }
            } else {
                band.iter().copied().sum::<Vec3>() / band.len() as f32
            };
            chain.joints.push(joint);
        }
    }

    // Huesos: cada articulación, hija de la anterior; la primera, del ancla.
    let mut names: HashMap<String, usize> = HashMap::new();
    let mut chain_names = Vec::new();
    for (c, chain) in chains.iter_mut().enumerate() {
        chain.first_bone = skeleton.bones.len();
        for (k, joint) in chain.joints.iter().enumerate() {
            let parent = if k == 0 { anchor } else { skeleton.bones.len() - 1 };
            skeleton.bones.push(crate::model::Bone {
                name: format!("{}{}_{:02}", spec.prefix, c + 1, k + 1),
                parent: Some(parent),
                rest_world: Mat4::from_translation(*joint),
            });
        }
        let label = (spec.name)(chain.theta);
        let count = names.entry(label.clone()).or_insert(0);
        *count += 1;
        chain_names.push(if *count > 1 { format!("{label} {count}") } else { label });
    }

    // Pesos: las dos cadenas más cercanas por ángulo y, en cada una, los dos segmentos por
    // distancia; la parte de la raíz y la de lo pegado a la piel se quedan con los pesos de antes.
    let compute = |p: Vec3, node: usize, pin: f32, old_j: [u16; 4], old_w: [f32; 4]| -> ([u16; 4], [f32; 4], usize) {
        let near = nearest_two(azimuth(p, centre));
        let mut acc: Vec<(usize, f32)> = Vec::new();
        let mut add = |bone: usize, w: f32| match acc.iter_mut().find(|(b, _)| *b == bone) {
            Some(e) => e.1 += w,
            None => acc.push((bone, w)),
        };
        let mut kept = pin;
        for (c, wc) in near {
            if wc <= 0.0 {
                continue;
            }
            for (bone, w) in chains[c].weights(distance[node]) {
                if bone == KEEP {
                    kept += w * wc * (1.0 - pin);
                } else {
                    add(bone, w * wc * (1.0 - pin));
                }
            }
        }
        for k in 0..4 {
            if old_w[k] > 0.0 {
                add(old_j[k] as usize, old_w[k] * kept);
            }
        }
        acc.retain(|(_, w)| *w > 1e-4);
        acc.sort_by(|a, b| b.1.total_cmp(&a.1));
        acc.truncate(4);
        let sum: f32 = acc.iter().map(|(_, w)| w).sum();
        let mut joints = [0u16; 4];
        let mut weights = [0f32; 4];
        for (k, (b, w)) in acc.iter().enumerate() {
            joints[k] = *b as u16;
            weights[k] = w / sum;
        }
        (joints, weights, near[0].0)
    };
    let mut counts = vec![0usize; chains.len()];
    let mut owner: Vec<Vec<Vec<i32>>> = Vec::new();
    for (l, layer) in model.layers.iter_mut().enumerate() {
        let mut per_layer = Vec::new();
        for primitive in &mut layer.primitives {
            let mut per_primitive = vec![-2i32; primitive.vertices.len()];
            if spec.layers[l] && !primitive.joints.is_empty() {
                let old: Vec<([u16; 4], [f32; 4])> = primitive.joints.iter().copied().zip(primitive.weights.iter().copied()).collect();
                // 1: ceñido por debajo de la raíz (va con el cuerpo); 2: suelto (va con su cadena).
                let mut side = vec![0u8; primitive.vertices.len()];
                for (i, v) in primitive.vertices.iter().enumerate() {
                    let (old_j, old_w) = old[i];
                    if !eligible(&old_j, &old_w) {
                        continue;
                    }
                    let p = Vec3::from(v.position);
                    let Some(&node) = node_of.get(&weld_key(p)) else { continue };
                    let pin = keep[node];
                    let (j, w, near) = compute(p, node, pin, old_j, old_w);
                    primitive.joints[i] = j;
                    primitive.weights[i] = w;
                    per_primitive[i] = if distance[node] > 0.0 && pin < 0.5 {
                        counts[near] += 1;
                        side[i] = 2;
                        near as i32
                    } else {
                        if distance[node] > 0.0 && pin >= 0.5 {
                            side[i] = 1;
                        }
                        -1
                    };
                }
                // Cortes: un triángulo entre lo ceñido y lo suelto se estiraría al separarse (la
                // malla es una sola piel); se queda del lado de la mayoría, con copias de los
                // vértices del otro lado.
                let mut copies: HashMap<(u32, u8), u32> = HashMap::new();
                // Solo en la ropa: en el pelo, lo pegado a la cara pasa a lo suelto poco a poco.
                let cuts = if spec.legs.is_empty() { 0 } else { primitive.indices.len() / 3 };
                for t in 0..cuts {
                    let tri = [0, 1, 2].map(|k| primitive.indices[t * 3 + k]);
                    let sides = tri.map(|v| side[v as usize]);
                    if !(sides.contains(&1) && sides.contains(&2)) {
                        continue;
                    }
                    let tight = sides.iter().filter(|s| **s == 1).count();
                    let majority = if tight >= 2 { 1u8 } else { 2u8 };
                    for k in 0..3 {
                        let v = tri[k];
                        if sides[k] == 0 || sides[k] == majority {
                            continue;
                        }
                        let copy = *copies.entry((v, majority)).or_insert_with(|| {
                            let vertex = primitive.vertices[v as usize];
                            let p = Vec3::from(vertex.position);
                            let node = node_of[&weld_key(p)];
                            let (old_j, old_w) = old[v as usize];
                            let (j, w, near) = compute(p, node, if majority == 1 { 1.0 } else { 0.0 }, old_j, old_w);
                            primitive.vertices.push(vertex);
                            primitive.joints.push(j);
                            primitive.weights.push(w);
                            per_primitive.push(if majority == 2 { near as i32 } else { -1 });
                            (primitive.vertices.len() - 1) as u32
                        });
                        primitive.indices[t * 3 + k] = copy;
                    }
                }
            }
            per_layer.push(per_primitive);
        }
        owner.push(per_layer);
    }
    let bones = chains.iter().map(|c| c.joints.len()).collect();
    Ok(Built { names: chain_names, bones, counts, owner })
}

/// Cadenas de pelo y subcapas por mechón.
pub fn rig_hair(model: &mut Model) -> Result<Report> {
    let hair = hair_layers(model);
    let skeleton = model.skeleton.as_ref().context("el modelo no tiene esqueleto")?;
    let head = skeleton.bone("head").context("el esqueleto no tiene cabeza")?;
    let head_pos = skeleton.rest_position(head);
    let is_head_only = |j: &[u16; 4], w: &[f32; 4]| j[0] as usize == head && w[0] > 0.999;

    // Altura de la cabeza: de su hueso a lo más alto del pelo.
    let mut top = f32::MIN;
    let mut face: Vec<Vec3> = Vec::new();
    for (l, layer) in model.layers.iter().enumerate() {
        for p in &layer.primitives {
            for (v, (j, w)) in p.vertices.iter().zip(p.joints.iter().zip(&p.weights)) {
                if hair[l] && is_head_only(j, w) {
                    top = top.max(v.position[1]);
                } else if !hair[l] && j[0] as usize == head && w[0] > 0.5 {
                    // La piel de la cabeza (lo que no es pelo y va con ella).
                    face.push(Vec3::from(v.position));
                }
            }
        }
    }
    if top == f32::MIN {
        return Ok(Report { strands: Vec::new() });
    }
    let head_height = (top - head_pos.y).max(0.05);
    // La raíz de los mechones, a la altura de las sienes. Por delante sube hasta la frente, para
    // que el flequillo tenga sus propias cadenas.
    let side_root = head_pos.y + 0.45 * head_height;
    let front_root = head_pos.y + 0.8 * head_height;
    // El pelo pegado a la piel de la cabeza, como trozos de cara que la segmentación dejó en el
    // pelo o mechones pintados sobre la frente, se queda con la cabeza.
    let (glued, free) = (0.02 * head_height, 0.08 * head_height);
    let face = crate::rig::Grid::new(face, free);
    let spec = Spec {
        prefix: BONE_PREFIX,
        anchor: head,
        centre: head_pos,
        layers: hair.clone(),
        anchor_only: true,
        root: Box::new(|theta: f32| side_root + (front_root - side_root) * smoothstep(0.3, 0.8, theta.cos())),
        segment_length: 0.3 * head_height,
        keep: Box::new(|p: Vec3| 1.0 - smoothstep(glued, free, face.nearest_within(p, free))),
        name: Box::new(|theta: f32| {
            // Los de delante, con la raíz en la frente, son el flequillo.
            let kind = if theta.cos() > 0.5 { "flequillo" } else { "mechón" };
            format!("{kind} {}", direction_name(theta))
        }),
        legs: Vec::new(),
    };
    let built = build_chains(model, &spec)?;
    drop(spec);
    if built.names.is_empty() {
        return Ok(Report { strands: Vec::new() });
    }

    // Nuevas capas: cada capa de pelo pasa a ser un grupo con un hijo por mechón, más la
    // coronilla (lo que no cuelga) y el resto (objetos marcados).
    let mut new_layers: Vec<Layer> = Vec::new();
    let mut remap: Vec<usize> = Vec::new();
    let old_layers = std::mem::take(&mut model.layers);
    for (l, mut layer) in old_layers.into_iter().enumerate() {
        remap.push(new_layers.len());
        layer.parent = layer.parent.map(|p| remap[p]);
        if !hair[l] || layer.primitives.is_empty() {
            new_layers.push(layer);
            continue;
        }
        // Grupos: 0 coronilla, 1 resto, 2.. mechones.
        let groups = 2 + built.names.len();
        let mut parts: Vec<Vec<crate::model::Primitive>> = (0..groups).map(|_| Vec::new()).collect();
        for (pi, primitive) in std::mem::take(&mut layer.primitives).into_iter().enumerate() {
            let vertex_group: Vec<usize> = built.owner[l][pi].iter().map(|&o| match o {
                -2 => 1,
                -1 => 0,
                c => 2 + c as usize,
            }).collect();
            let mut faces: Vec<Vec<usize>> = vec![Vec::new(); groups];
            for t in 0..primitive.indices.len() / 3 {
                let tri = [0, 1, 2].map(|k| vertex_group[primitive.indices[t * 3 + k] as usize]);
                // Un mechón si algún vértice cuelga (el de más votos); si no, coronilla o resto.
                let votes = |g: usize| tri.iter().filter(|x| **x == g).count();
                let mut best = tri[0];
                for &g in &tri {
                    if (g >= 2 && (best < 2 || votes(g) > votes(best))) || (best == 1 && g == 0) {
                        best = g;
                    }
                }
                faces[best].push(t);
            }
            for (g, f) in faces.iter().enumerate() {
                if !f.is_empty() {
                    parts[g].push(primitive.subset(f));
                }
            }
        }
        let used: Vec<usize> = (0..groups).filter(|g| !parts[*g].is_empty()).collect();
        if used.len() <= 1 {
            layer.primitives = used.first().map(|g| std::mem::take(&mut parts[*g])).unwrap_or_default();
            new_layers.push(layer);
            continue;
        }
        let group_index = new_layers.len();
        let depth = layer.depth;
        new_layers.push(Layer { primitives: Vec::new(), expanded: false, ..layer });
        for g in used {
            let name = match g {
                0 => "coronilla".to_string(),
                1 => "resto".to_string(),
                s => built.names[s - 2].clone(),
            };
            new_layers.push(Layer {
                name,
                parent: Some(group_index),
                depth: depth + 1,
                primitives: std::mem::take(&mut parts[g]),
                visible: true,
                opacity: 1.0,
                expanded: false,
            });
        }
    }
    model.layers = new_layers;
    Ok(report_of(&built))
}

fn report_of(built: &Built) -> Report {
    let strands = (0..built.names.len())
        .map(|c| Strand { name: built.names[c].clone(), bones: built.bones[c], vertices: built.counts[c] })
        .collect();
    Report { strands }
}

pub const SKIRT_PREFIX: &str = "J_Sec_Skirt";

/// Cadenas de falda: la ropa de cintura para abajo que cuelga suelta alrededor de las piernas
/// (faldas, sobrefaldas, faldones de abrigo) cuelga de la cadera, y choca con las piernas en el
/// visor. Lo ceñido a la pierna (medias, pantalones) se queda con los pesos que tenía: se mide
/// con la distancia al eje de cada hueso de la pierna, comparada con el radio de la piel de esa
/// pierna.
pub fn rig_skirt(model: &mut Model) -> Result<Report> {
    let skeleton = model.skeleton.as_ref().context("el modelo no tiene esqueleto")?;
    let hips = skeleton.bone("hips").context("el esqueleto no tiene cadera")?;
    let hips_pos = skeleton.rest_position(hips);
    let names = ["leftUpperLeg", "leftLowerLeg", "leftFoot", "rightUpperLeg", "rightLowerLeg", "rightFoot"];
    let bones: Vec<Option<usize>> = names.iter().map(|n| skeleton.bone(n)).collect();
    let segments: Vec<(usize, Vec3, Vec3)> = [(0, 1), (1, 2), (3, 4), (4, 5)]
        .iter()
        .filter_map(|&(a, b)| Some((bones[a]?, skeleton.rest_position(bones[a]?), skeleton.rest_position(bones[b]?))))
        .collect();
    let Some(foot_y) = bones[2].map(|b| skeleton.rest_position(b).y) else { return Ok(Report { strands: Vec::new() }) };

    let garments: Vec<bool> = crate::rig::lower_garment_layers(model)
        .into_iter()
        .zip(&model.layers)
        .map(|(g, l)| g && !l.name.to_lowercase().contains("pies"))
        .collect();
    // La superficie más interior alrededor de cada hueso de la pierna (piel, medias, botas: todo
    // lo que no es pelo), por tramos a lo largo del hueso y sectores de 30° alrededor. Lo que
    // está pegado a ella es ceñido; lo que queda bastante por fuera, suelto.
    const BANDS: usize = 8;
    let nearest_leg = |p: Vec3| -> (usize, f32, f32, Vec3) {
        segments
            .iter()
            .enumerate()
            .map(|(s, (_, a, b))| {
                let ab = *b - *a;
                let t = ((p - *a).dot(ab) / ab.length_squared().max(1e-10)).clamp(0.0, 1.0);
                let closest = *a + ab * t;
                (s, t, p.distance(closest), closest)
            })
            .min_by(|x, y| x.2.total_cmp(&y.2))
            .unwrap()
    };
    let cell = |p: Vec3| -> (usize, usize, usize, f32) {
        let (s, t, d, closest) = nearest_leg(p);
        let band = ((t * BANDS as f32) as usize).min(BANDS - 1);
        let sector = (azimuth(p, closest).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU * 12.0) as usize % 12;
        (s, band, sector, d)
    };
    let hair = hair_layers(model);
    let mut inner = vec![f32::MAX; segments.len() * BANDS * 12];
    for (l, layer) in model.layers.iter().enumerate() {
        if hair[l] {
            continue;
        }
        for p in &layer.primitives {
            for v in &p.vertices {
                let point = Vec3::from(v.position);
                if point.y > hips_pos.y {
                    continue;
                }
                let (s, b, k, d) = cell(point);
                let slot = &mut inner[(s * BANDS + b) * 12 + k];
                *slot = slot.min(d);
            }
        }
    }
    let leg = hips_pos.y - foot_y;
    let ankle = foot_y + 0.04 * leg;
    let spec = Spec {
        prefix: SKIRT_PREFIX,
        anchor: hips,
        centre: hips_pos,
        layers: garments,
        anchor_only: false,
        root: Box::new(|_| hips_pos.y),
        segment_length: 0.12 * leg,
        keep: Box::new(|p: Vec3| {
            // Del tobillo para abajo, calzado: siempre con el pie.
            if p.y < ankle {
                return 1.0;
            }
            let (s, b, k, d) = cell(p);
            let surface = inner[(s * BANDS + b) * 12 + k];
            // El hueso no tiene por qué ir centrado en la pierna: se mide lo que sobresale de la
            // superficie más interior en esa dirección, en proporción a la pierna (la ropa
            // ceñida, 1 o 2 cm; un abrigo, 10 o más).
            let excess = d - surface.min(d);
            1.0 - smoothstep(0.035 * leg, 0.07 * leg, excess)
        }),
        name: Box::new(|theta: f32| format!("falda {}", direction_name(theta))),
        legs: segments.iter().map(|(_, a, b)| (*a, *b)).collect(),
    };
    let built = build_chains(model, &spec)?;
    Ok(report_of(&built))
}

/// Valores de la física del pelo, por paso de 1/60 s.
#[derive(Clone, Copy, Debug)]
pub struct HairParams {
    /// Cuánto vuelve cada segmento a su forma de reposo en cada paso.
    pub stiffness: f32,
    /// Velocidad que conserva cada paso.
    pub damping: f32,
    /// Cuánto sigue colgando hacia su dirección de reposo en el mundo cuando la cabeza gira
    /// (0, nada: va con la cabeza; 1, todo).
    pub gravity: f32,
    /// Ángulo máximo con la forma de reposo, en radianes.
    pub max_angle: f32,
}

pub const PRESETS: &[(&str, HairParams)] = &[
    ("normal", HairParams { stiffness: 0.08, damping: 0.88, gravity: 0.5, max_angle: 1.2 }),
    ("suelto", HairParams { stiffness: 0.035, damping: 0.93, gravity: 0.7, max_angle: 1.2 }),
    ("rígido", HairParams { stiffness: 0.25, damping: 0.75, gravity: 0.2, max_angle: 0.6 }),
    // Sin física: la forma de reposo, con la cabeza.
    ("fijo", HairParams { stiffness: 1.0, damping: 0.0, gravity: 0.0, max_angle: 0.0 }),
];

/// Cápsula de colisión entre dos huesos (o una esfera, si `b` es `None`), con su radio.
struct Collider {
    a: usize,
    b: Option<usize>,
    /// Centro en el espacio de `a` (para la esfera de la cabeza).
    offset: Vec3,
    radius: f32,
}

impl Collider {
    fn segment(&self, skin: &[Mat4], skeleton: &Skeleton, offset: Vec3) -> (Vec3, Vec3) {
        let a = skin[self.a].transform_point3(skeleton.rest_position(self.a) + self.offset) + offset;
        let b = self.b.map_or(a, |b| skin[b].transform_point3(skeleton.rest_position(b)) + offset);
        (a, b)
    }
}

fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let ab = b - a;
    let t = if ab.length_squared() > 1e-10 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    a + ab * t
}

struct SimChain {
    /// Huesos de la cadena, del primero a la punta.
    bones: Vec<usize>,
    /// Segmentos en reposo, en el mundo.
    rest: Vec<Vec3>,
    /// Radio de cada colisionador para esta cadena (sus puntas en reposo quedan fuera).
    radii: Vec<f32>,
    tips: Vec<Vec3>,
    previous: Vec<Vec3>,
    /// Raíz en el último paso.
    root: Vec3,
    /// Hueso del que cuelga y si es ropa (y no pelo).
    anchor: usize,
    cloth: bool,
    /// Capa (mechón) a la que mueve, y su juego de valores si no es el general.
    pub layer: Option<usize>,
    pub preset: Option<usize>,
}

/// Dos puntas de mechones distintos que en reposo están cerca: no se acercan más de `min`.
struct Contact {
    a: (usize, usize),
    b: (usize, usize),
    min: f32,
}

pub struct HairSim {
    chains: Vec<SimChain>,
    colliders: Vec<Collider>,
    contacts: Vec<Contact>,
    ready: bool,
    offset: Vec3,
    /// Lo que se movió la punta más rápida en el último paso.
    pub motion: f32,
}

impl HairSim {
    /// Las cadenas `J_Sec_Hair*` del esqueleto y las cápsulas del cuerpo, medidas en la malla.
    pub fn new(model: &Model) -> Option<Self> {
        let skeleton = model.skeleton.as_ref()?;
        // Cadenas de pelo y de falda, por (tipo, número).
        let mut by_chain: HashMap<(usize, usize), Vec<(usize, usize)>> = HashMap::new();
        for (i, bone) in skeleton.bones.iter().enumerate() {
            for (kind, prefix) in [BONE_PREFIX, SKIRT_PREFIX].iter().enumerate() {
                if let Some((chain, k)) = bone.name.strip_prefix(prefix).and_then(|rest| rest.split_once('_')) {
                    if let (Ok(chain), Ok(k)) = (chain.parse::<usize>(), k.parse::<usize>()) {
                        by_chain.entry((kind, chain)).or_default().push((k, i));
                    }
                }
            }
        }
        if by_chain.is_empty() {
            return None;
        }
        let mut keys: Vec<(usize, usize)> = by_chain.keys().copied().collect();
        keys.sort();

        // Cápsulas: tronco, cuello, brazos, y una esfera para la cabeza. El radio es un
        // percentil bajo de la distancia de los vértices del cuerpo (no pelo) que van con ese
        // hueso: el ancho más estrecho, para no empujar el pelo que ya está bien colocado.
        let pairs: [(&str, Option<&str>); 17] = [
            ("hips", Some("spine")),
            ("spine", Some("chest")),
            ("chest", Some("upperChest")),
            ("upperChest", Some("neck")),
            ("neck", Some("head")),
            ("head", None),
            ("leftShoulder", Some("leftUpperArm")),
            ("rightShoulder", Some("rightUpperArm")),
            ("leftUpperArm", Some("leftLowerArm")),
            ("rightUpperArm", Some("rightLowerArm")),
            ("leftLowerArm", Some("leftHand")),
            ("rightLowerArm", Some("rightHand")),
            ("chest", Some("neck")),
            ("leftUpperLeg", Some("leftLowerLeg")),
            ("rightUpperLeg", Some("rightLowerLeg")),
            ("leftLowerLeg", Some("leftFoot")),
            ("rightLowerLeg", Some("rightFoot")),
        ];
        let hair = hair_layers(model);
        let mut by_bone: HashMap<usize, Vec<Vec3>> = HashMap::new();
        for (l, layer) in model.layers.iter().enumerate() {
            if hair[l] {
                continue;
            }
            for p in &layer.primitives {
                for (v, (j, w)) in p.vertices.iter().zip(p.joints.iter().zip(&p.weights)) {
                    let k = (0..4).max_by(|a, b| w[*a].total_cmp(&w[*b])).unwrap_or(0);
                    by_bone.entry(j[k] as usize).or_default().push(Vec3::from(v.position));
                }
            }
        }
        let percentile = |mut d: Vec<f32>, q: f32| -> Option<f32> {
            if d.len() < 20 {
                return None;
            }
            d.sort_by(f32::total_cmp);
            Some(d[((d.len() - 1) as f32 * q) as usize])
        };
        let mut colliders = Vec::new();
        for (a, b) in pairs {
            let (Some(a), b) = (skeleton.bone(a), b.and_then(|b| skeleton.bone(b))) else { continue };
            let Some(points) = by_bone.get(&a) else { continue };
            let pa = skeleton.rest_position(a);
            let collider = match b {
                Some(b) => {
                    let pb = skeleton.rest_position(b);
                    let d = points.iter().map(|p| p.distance(closest_on_segment(*p, pa, pb))).collect();
                    percentile(d, 0.2).map(|radius| Collider { a, b: Some(b), offset: Vec3::ZERO, radius })
                }
                None => {
                    // La cabeza: centrada en sus vértices.
                    let centre = points.iter().copied().sum::<Vec3>() / points.len() as f32;
                    let d = points.iter().map(|p| p.distance(centre)).collect();
                    percentile(d, 0.3).map(|radius| Collider { a, b: None, offset: centre - pa, radius })
                }
            };
            colliders.extend(collider);
        }

        let identity = vec![Mat4::IDENTITY; skeleton.bones.len()];
        let chains = keys
            .iter()
            .map(|key| {
                let mut list = by_chain[key].clone();
                list.sort();
                let bones: Vec<usize> = list.iter().map(|(_, b)| *b).collect();
                let joints: Vec<Vec3> = bones.iter().map(|b| skeleton.rest_position(*b)).collect();
                let rest: Vec<Vec3> = joints.windows(2).map(|w| w[1] - w[0]).collect();
                let radii = colliders
                    .iter()
                    .map(|c| {
                        let (a, b) = c.segment(&identity, skeleton, Vec3::ZERO);
                        let closest = joints[1..].iter().map(|j| j.distance(closest_on_segment(*j, a, b))).fold(f32::MAX, f32::min);
                        c.radius.min(closest * 0.9)
                    })
                    .collect();
                // La capa con más vértices que mueve esta cadena.
                let mut votes: HashMap<usize, usize> = HashMap::new();
                for (l, layer) in model.layers.iter().enumerate() {
                    for p in &layer.primitives {
                        for (j, w) in p.joints.iter().zip(&p.weights) {
                            let k = (0..4).max_by(|a, b| w[*a].total_cmp(&w[*b])).unwrap_or(0);
                            if bones.contains(&(j[k] as usize)) {
                                *votes.entry(l).or_default() += 1;
                            }
                        }
                    }
                }
                let layer = votes.into_iter().max_by_key(|(l, n)| (*n, usize::MAX - l)).map(|(l, _)| l);
                let anchor = skeleton.bones[bones[0]].parent.unwrap_or(0);
                SimChain { bones, tips: joints[1..].to_vec(), previous: joints[1..].to_vec(), root: joints[0], rest, radii, layer, preset: None, anchor, cloth: key.0 == 1 }
            })
            .collect::<Vec<_>>();
        // Contactos entre puntas de mechones distintos que en reposo están a menos de un
        // segmento: el mechón de fuera no atraviesa al de dentro al moverse.
        let mut contacts = Vec::new();
        for a in 0..chains.len() {
            for b in a + 1..chains.len() {
                for (i, ta) in chains[a].tips.iter().enumerate() {
                    for (j, tb) in chains[b].tips.iter().enumerate() {
                        let d = ta.distance(*tb);
                        if d < chains[a].rest[i].length().min(chains[b].rest[j].length()) {
                            contacts.push(Contact { a: (a, i), b: (b, j), min: d * 0.7 });
                        }
                    }
                }
            }
        }
        Some(Self { chains, colliders, contacts, ready: false, offset: Vec3::ZERO, motion: 0.0 })
    }

    /// Un paso de 1/60 s. `skin` son las matrices de skinning de la pose (el pelo no influye
    /// en ellas) y `offset` un desplazamiento del cuerpo entero. `wind` es una fuerza por paso.
    pub fn step(&mut self, skeleton: &Skeleton, skin: &[Mat4], offset: Vec3, general: &HairParams, cloth: &HairParams, wind: Vec3) {
        self.offset = offset;
        let colliders: Vec<(Vec3, Vec3)> = self.colliders.iter().map(|c| c.segment(skin, skeleton, offset)).collect();
        let first_step = !self.ready;
        self.ready = true;
        let mut motion: f32 = 0.0;
        for chain in &mut self.chains {
            let default = if chain.cloth { cloth } else { general };
            let params = chain.preset.and_then(|i| PRESETS.get(i)).map_or(default, |p| &p.1);
            let wind = if params.stiffness >= 1.0 { Vec3::ZERO } else { wind };
            let head = skin[chain.anchor];
            let turn = Quat::from_mat3(&Mat3::from_mat4(head)).normalize();
            let mut base = head.transform_point3(skeleton.rest_position(chain.bones[0])) + offset;
            chain.root = base;
            for k in 0..chain.rest.len() {
                let length = chain.rest[k].length();
                let posed = turn * chain.rest[k];
                let target = posed.lerp(chain.rest[k], params.gravity).normalize_or_zero() * length;
                if first_step {
                    chain.tips[k] = base + target;
                    chain.previous[k] = chain.tips[k];
                }
                let tip = chain.tips[k];
                let velocity = (tip - chain.previous[k]) * params.damping;
                let moved = tip + velocity + (base + target - tip) * params.stiffness + wind;
                let mut segment = (moved - base).normalize_or(target / length.max(1e-6)) * length;
                // Ángulo máximo con el reposo.
                let angle = segment.angle_between(target);
                if angle > params.max_angle {
                    let axis = target.cross(segment).normalize_or_zero();
                    if axis != Vec3::ZERO {
                        segment = Quat::from_axis_angle(axis, params.max_angle) * target.normalize() * length;
                    }
                }
                // Colisiones: la punta sale de cada cápsula.
                for ((a, b), radius) in colliders.iter().zip(&chain.radii) {
                    let end = base + segment;
                    let closest = closest_on_segment(end, *a, *b);
                    let away = end - closest;
                    let distance = away.length();
                    if distance < *radius && distance > 1e-6 {
                        let pushed = closest + away * (*radius / distance);
                        segment = (pushed - base).normalize_or(segment / length) * length;
                    }
                }
                let new_tip = base + segment;
                motion = motion.max(new_tip.distance(tip));
                chain.previous[k] = tip;
                chain.tips[k] = new_tip;
                base = new_tip;
            }
        }
        // Los mechones se separan donde se acercan más de la cuenta, y cada cadena recupera
        // después la longitud de sus segmentos desde la raíz.
        if !self.contacts.is_empty() {
            for contact in &self.contacts {
                let (pa, pb) = (self.chains[contact.a.0].tips[contact.a.1], self.chains[contact.b.0].tips[contact.b.1]);
                let d = pa.distance(pb);
                if d < contact.min && d > 1e-6 {
                    let push = (pb - pa) * ((contact.min - d) / d * 0.5);
                    self.chains[contact.a.0].tips[contact.a.1] -= push;
                    self.chains[contact.b.0].tips[contact.b.1] += push;
                }
            }
            for chain in &mut self.chains {
                let mut base = chain.root;
                for k in 0..chain.rest.len() {
                    let length = chain.rest[k].length();
                    let segment = (chain.tips[k] - base).normalize_or(chain.rest[k] / length.max(1e-6)) * length;
                    chain.tips[k] = base + segment;
                    base = chain.tips[k];
                }
            }
        }
        self.motion = motion;
    }

    /// Gira los huesos de cada cadena hacia sus puntas simuladas. `offset` es el mismo que en
    /// `step`.
    pub fn apply(&self, skeleton: &Skeleton, local: &mut [Quat]) {
        if !self.ready {
            return;
        }
        for chain in &self.chains {
            for k in 0..chain.rest.len() {
                let base = if k == 0 {
                    // La raíz en la pose (más el desplazamiento del cuerpo, como las puntas).
                    crate::pose::world_matrices(skeleton, local)[chain.bones[0]].w_axis.truncate() + self.offset
                } else {
                    chain.tips[k - 1]
                };
                let direction = chain.tips[k] - base;
                crate::pose::aim(skeleton, local, chain.bones[k], chain.bones[k + 1], direction);
            }
        }
    }

    /// La física como extensión `VRMC_springBone` de VRM 1.0, para Blender (VRM Add-on) y los
    /// visores VRM. Los nodos son los índices de los huesos (`export_glb` los escribe primero).
    /// Cada colisionador lleva el radio más pequeño que le dan las cadenas, para que ninguna
    /// punta en reposo quede dentro. Rigidez, arrastre y gravedad son valores habituales de
    /// VRoid para pelo largo, no una conversión de los de `HairParams`, que siguen otra fórmula.
    pub fn spring_bone_extension(&self, skeleton: &Skeleton) -> serde_json::Value {
        use serde_json::json;
        let local = |bone: usize, world: Vec3| skeleton.bones[bone].rest_world.inverse().transform_point3(world).to_array();
        let colliders: Vec<serde_json::Value> = self
            .colliders
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let radius = c.radius;
                let _ = i;
                let centre = skeleton.rest_position(c.a) + c.offset;
                let shape = match c.b {
                    Some(b) => json!({ "capsule": { "offset": local(c.a, centre), "radius": radius, "tail": local(c.a, skeleton.rest_position(b)) } }),
                    None => json!({ "sphere": { "offset": local(c.a, centre), "radius": radius } }),
                };
                json!({ "node": c.a, "shape": shape })
            })
            .collect();
        let springs: Vec<serde_json::Value> = self
            .chains
            .iter()
            .map(|chain| {
                let joints: Vec<serde_json::Value> = chain
                    .bones
                    .iter()
                    .map(|b| json!({ "node": b, "hitRadius": 0.02, "stiffness": 1.0, "gravityPower": 0.1, "gravityDir": [0.0, -1.0, 0.0], "dragForce": 0.4 }))
                    .collect();
                let name = skeleton.bones[chain.bones[0]].name.trim_end_matches("_01").to_string();
                // Solo los colisionadores que no tocan la cadena en reposo.
                let groups: Vec<usize> = (0..self.colliders.len()).filter(|i| chain.radii[*i] >= 0.95 * self.colliders[*i].radius).collect();
                json!({ "name": name, "joints": joints, "colliderGroups": groups })
            })
            .collect();
        json!({
            "specVersion": "1.0",
            "colliders": colliders,
            "colliderGroups": (0..self.colliders.len()).map(|i| json!({ "name": skeleton.bones[self.colliders[i].a].name, "colliders": [i] })).collect::<Vec<_>>(),
            "springs": springs,
        })
    }

    /// Juego de valores propio de una capa (y de sus hijas, si es un grupo), o `None` para el
    /// general.
    pub fn set_layer_preset(&mut self, layers: &[Layer], layer: usize, preset: Option<usize>) {
        for chain in &mut self.chains {
            let mut l = chain.layer;
            while let Some(i) = l {
                if i == layer {
                    chain.preset = preset;
                    break;
                }
                l = layers.get(i).and_then(|x| x.parent);
            }
        }
    }

    /// El juego de valores de una capa si todas sus cadenas tienen el mismo, y si tiene cadenas.
    pub fn layer_preset(&self, layers: &[Layer], layer: usize) -> Option<Option<usize>> {
        let mut found: Option<Option<usize>> = None;
        for chain in &self.chains {
            let mut l = chain.layer;
            while let Some(i) = l {
                if i == layer {
                    match found {
                        None => found = Some(chain.preset),
                        Some(p) if p != chain.preset => return Some(None),
                        _ => {}
                    }
                    break;
                }
                l = layers.get(i).and_then(|x| x.parent);
            }
        }
        found
    }

    pub fn reset(&mut self) {
        self.ready = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strand_weights_go_from_head_to_the_tip() {
        let chain = Chain { theta: 0.0, length: 0.1, joints: vec![Vec3::ZERO; 5], first_bone: 10 };
        // En la raíz, la cabeza entera.
        assert_eq!(chain.weights(0.0)[0], (KEEP, 1.0));
        // A dos segmentos y medio, mitad del segundo hueso y mitad del tercero.
        let w = chain.weights(0.15 + 0.1);
        assert_eq!((w[0].0, w[1].0), (11, 12));
        assert!((w[0].1 - 0.5).abs() < 1e-4 && (w[1].1 - 0.5).abs() < 1e-4);
        // Muy abajo, el último segmento.
        assert_eq!(chain.weights(5.0)[0].0, 13);
    }

    #[test]
    fn strands_are_named_by_their_side() {
        assert_eq!(direction_name(0.0), "delante");
        assert_eq!(direction_name(std::f32::consts::FRAC_PI_2), "izq");
        assert_eq!(direction_name(-std::f32::consts::FRAC_PI_2), "der");
        assert_eq!(direction_name(std::f32::consts::PI), "detrás");
    }
}

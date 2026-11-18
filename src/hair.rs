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
    root_y: f32,
    length: f32,
    joints: Vec<Vec3>,
    first_bone: usize,
}

impl Chain {
    /// Huesos y pesos de un punto de este mechón por su altura: la cabeza por encima de la raíz,
    /// y de ahí hacia abajo pasa de un segmento al siguiente.
    fn weights(&self, y: f32, root_y: f32, head: usize) -> [(usize, f32); 2] {
        let segments = self.joints.len() - 1;
        let j = ((root_y - y) / self.length - 1.0).clamp(-1.0, segments as f32 - 1.0);
        let low = j.floor();
        let frac = j - low;
        let bone = |i: f32| if i < 0.0 { head } else { self.first_bone + i as usize };
        [(bone(low), 1.0 - frac), (bone((low + 1.0).min(segments as f32 - 1.0)), frac)]
    }
}

pub fn rig_hair(model: &mut Model) -> Result<Report> {
    let hair = hair_layers(model);
    let skeleton = model.skeleton.as_mut().context("el modelo no tiene esqueleto")?;
    if skeleton.bones.iter().any(|b| b.name.starts_with(BONE_PREFIX)) {
        anyhow::bail!("el modelo ya tiene cadenas de pelo");
    }
    let head = skeleton.bone("head").context("el esqueleto no tiene cabeza")?;
    let head_pos = skeleton.rest_position(head);

    // Vértices de pelo que van enteros con la cabeza (los de objetos marcados no).
    let is_head_only = |j: &[u16; 4], w: &[f32; 4]| j[0] as usize == head && w[0] > 0.999;
    let mut points: Vec<Vec3> = Vec::new();
    for (l, layer) in model.layers.iter().enumerate() {
        if !hair[l] {
            continue;
        }
        for p in &layer.primitives {
            for (v, (j, w)) in p.vertices.iter().zip(p.joints.iter().zip(&p.weights)) {
                if is_head_only(j, w) {
                    points.push(Vec3::from(v.position));
                }
            }
        }
    }
    if points.is_empty() {
        return Ok(Report { strands: Vec::new() });
    }
    let top = points.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    let head_height = (top - head_pos.y).max(0.05);
    // La raíz de los mechones, a la altura de las sienes.
    // Por delante sube hasta la frente, para que el flequillo tenga sus propias cadenas.
    let side_root = head_pos.y + 0.45 * head_height;
    let front_root = head_pos.y + 0.8 * head_height;
    let root_at = |theta: f32| {
        let f = ((theta.cos() - 0.3) / 0.5).clamp(0.0, 1.0);
        side_root + (front_root - side_root) * f * f * (3.0 - 2.0 * f)
    };
    let segment_length = 0.3 * head_height;

    // Sectores con bastante pelo colgando, y su ángulo medio.
    let hanging: Vec<Vec3> = points.iter().copied().filter(|p| p.y < root_at(azimuth(*p, head_pos)) - 0.5 * segment_length).collect();
    let mut bins: Vec<Vec<Vec3>> = vec![Vec::new(); SECTORS];
    for p in &hanging {
        let t = azimuth(*p, head_pos).rem_euclid(std::f32::consts::TAU);
        bins[(t / std::f32::consts::TAU * SECTORS as f32) as usize % SECTORS].push(*p);
    }
    let mut chains: Vec<Chain> = Vec::new();
    for bin in &bins {
        if bin.len() < 30 || bin.len() * 80 < hanging.len() {
            continue;
        }
        let (s, c) = bin.iter().fold((0.0, 0.0), |(s, c), p| {
            let t = azimuth(*p, head_pos);
            (s + t.sin(), c + t.cos())
        });
        let theta = s.atan2(c);
        let root_y = root_at(theta);
        let bottom = bin.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        let extent = root_y - bottom;
        let segments = ((extent / segment_length).ceil() as usize).clamp(2, MAX_SEGMENTS);
        let length = extent / segments as f32;
        chains.push(Chain { theta, root_y, length, joints: Vec::new(), first_bone: 0 });
    }
    if chains.is_empty() {
        return Ok(Report { strands: Vec::new() });
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

    // Articulaciones: el centro del mechón en cada nivel. Los puntos de cada mechón son los que
    // tienen ese mechón como el más cercano.
    let owner: Vec<usize> = points.iter().map(|p| nearest_two(azimuth(*p, head_pos))[0].0).collect();
    for (c, chain) in chains.iter_mut().enumerate() {
        let mine: Vec<Vec3> = points.iter().zip(&owner).filter(|(_, o)| **o == c).map(|(p, _)| *p).collect();
        let root_y = chain.root_y;
        let segments = ((root_y - mine.iter().map(|p| p.y).fold(f32::MAX, f32::min)) / chain.length).round() as usize;
        let segments = segments.clamp(2, MAX_SEGMENTS);
        for k in 0..=segments {
            let level = root_y - k as f32 * chain.length;
            let half = chain.length * 0.5;
            let band: Vec<Vec3> = mine
                .iter()
                .copied()
                .filter(|p| if k == segments { p.y < level + half } else { (p.y - level).abs() < half })
                .collect();
            let joint = if band.is_empty() {
                match chain.joints.as_slice() {
                    [.., a, b] => *b + (*b - *a),
                    [.., b] => *b - Vec3::Y * chain.length,
                    [] => head_pos + Vec3::new(chain.theta.sin(), 0.0, chain.theta.cos()) * 0.5 * head_height,
                }
            } else {
                let mut centre = band.iter().copied().sum::<Vec3>() / band.len() as f32;
                centre.y = if k == segments { band.iter().map(|p| p.y).fold(f32::MAX, f32::min) } else { level };
                centre
            };
            chain.joints.push(joint);
        }
    }

    // Huesos: cada articulación, hija de la anterior; la primera, de la cabeza.
    let mut report = Report { strands: Vec::new() };
    let mut names: HashMap<String, usize> = HashMap::new();
    let mut chain_names = Vec::new();
    for (c, chain) in chains.iter_mut().enumerate() {
        chain.first_bone = skeleton.bones.len();
        for (k, joint) in chain.joints.iter().enumerate() {
            let parent = if k == 0 { head } else { skeleton.bones.len() - 1 };
            skeleton.bones.push(crate::model::Bone {
                name: format!("{BONE_PREFIX}{}_{:02}", c + 1, k + 1),
                parent: Some(parent),
                rest_world: Mat4::from_translation(*joint),
            });
        }
        // Los de delante, con la raíz en la frente, son el flequillo.
        let kind = if chain.theta.cos() > 0.5 { "flequillo" } else { "mechón" };
        let base = direction_name(chain.theta);
        let label = format!("{kind} {base}");
        let count = names.entry(label.clone()).or_insert(0);
        *count += 1;
        chain_names.push(if *count > 1 { format!("{label} {count}") } else { label });
    }

    // Pesos: los dos mechones más cercanos por ángulo y, en cada uno, los dos segmentos por
    // altura.
    // La piel de la cabeza (lo que no es pelo y va con la cabeza): el pelo pegado a ella, como
    // trozos de cara que la segmentación dejó en el pelo o mechones pintados sobre la frente,
    // se queda con la cabeza.
    let mut face: Vec<Vec3> = Vec::new();
    for (l, layer) in model.layers.iter().enumerate() {
        if hair[l] {
            continue;
        }
        for p in &layer.primitives {
            for (v, (j, w)) in p.vertices.iter().zip(p.joints.iter().zip(&p.weights)) {
                if j[0] as usize == head && w[0] > 0.5 {
                    face.push(Vec3::from(v.position));
                }
            }
        }
    }
    let (glued, free) = (0.02 * head_height, 0.08 * head_height);
    let face = crate::rig::Grid::new(face, free);
    let weights_at = |p: Vec3| -> ([u16; 4], [f32; 4], usize) {
        let mut acc: Vec<(usize, f32)> = Vec::new();
        let near = nearest_two(azimuth(p, head_pos));
        for (c, wc) in near {
            if wc <= 0.0 {
                continue;
            }
            for (bone, w) in chains[c].weights(p.y, chains[c].root_y, head) {
                match acc.iter_mut().find(|(b, _)| *b == bone) {
                    Some(e) => e.1 += w * wc,
                    None => acc.push((bone, w * wc)),
                }
            }
        }
        let d = face.nearest_within(p, free);
        let f = ((d - glued) / (free - glued)).clamp(0.0, 1.0);
        let pin = 1.0 - f * f * (3.0 - 2.0 * f);
        if pin > 0.0 {
            acc.iter_mut().for_each(|e| e.1 *= 1.0 - pin);
            match acc.iter_mut().find(|(b, _)| *b == head) {
                Some(e) => e.1 += pin,
                None => acc.push((head, pin)),
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

    // Nuevas capas: cada capa de pelo pasa a ser un grupo con un hijo por mechón, más la
    // coronilla y el flequillo (lo que no cuelga) y el resto (objetos marcados).
    let mut counts = vec![0usize; chains.len()];
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
        // Grupos: 0 coronilla, 1 flequillo, 2 resto, 3.. mechones.
        let groups = 3 + chains.len();
        let mut parts: Vec<Vec<crate::model::Primitive>> = (0..groups).map(|_| Vec::new()).collect();
        for mut primitive in std::mem::take(&mut layer.primitives) {
            let mut vertex_group = vec![2usize; primitive.vertices.len()];
            for (i, v) in primitive.vertices.iter().enumerate() {
                let (j, w) = (primitive.joints[i], primitive.weights[i]);
                if !is_head_only(&j, &w) {
                    continue;
                }
                let p = Vec3::from(v.position);
                let (nj, nw, owner) = weights_at(p);
                primitive.joints[i] = nj;
                primitive.weights[i] = nw;
                vertex_group[i] = if p.y < chains[owner].root_y {
                    counts[owner] += 1;
                    3 + owner
                } else {
                    0
                };
            }
            let mut faces: Vec<Vec<usize>> = vec![Vec::new(); groups];
            for t in 0..primitive.indices.len() / 3 {
                let tri = [0, 1, 2].map(|k| vertex_group[primitive.indices[t * 3 + k] as usize]);
                // Un mechón si algún vértice cuelga (el de más votos), si no, el del primero.
                let mut best = tri[0];
                let strand_votes = |g: usize| tri.iter().filter(|x| **x == g).count();
                for &g in &tri {
                    if (g >= 3 && (best < 3 || strand_votes(g) > strand_votes(best))) || (best == 2 && g < 2) {
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
                1 => "flequillo".to_string(),
                2 => "resto".to_string(),
                s => chain_names[s - 3].clone(),
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

    for (c, chain) in chains.iter().enumerate() {
        report.strands.push(Strand { name: chain_names[c].clone(), bones: chain.joints.len(), vertices: counts[c] });
    }
    Ok(report)
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
    ("suelto", HairParams { stiffness: 0.035, damping: 0.93, gravity: 0.7, max_angle: 1.5 }),
    ("rígido", HairParams { stiffness: 0.25, damping: 0.75, gravity: 0.2, max_angle: 0.6 }),
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
}

pub struct HairSim {
    head: usize,
    chains: Vec<SimChain>,
    colliders: Vec<Collider>,
    ready: bool,
    offset: Vec3,
    /// Lo que se movió la punta más rápida en el último paso.
    pub motion: f32,
}

impl HairSim {
    /// Las cadenas `J_Sec_Hair*` del esqueleto y las cápsulas del cuerpo, medidas en la malla.
    pub fn new(model: &Model) -> Option<Self> {
        let skeleton = model.skeleton.as_ref()?;
        let head = skeleton.bone("head")?;
        let mut by_chain: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
        for (i, bone) in skeleton.bones.iter().enumerate() {
            if let Some(rest) = bone.name.strip_prefix(BONE_PREFIX) {
                if let Some((chain, k)) = rest.split_once('_') {
                    by_chain.entry(chain.to_string()).or_default().push((k.parse().ok()?, i));
                }
            }
        }
        if by_chain.is_empty() {
            return None;
        }
        let mut keys: Vec<String> = by_chain.keys().cloned().collect();
        keys.sort_by_key(|k| k.parse::<usize>().unwrap_or(usize::MAX));

        // Cápsulas: tronco, cuello, brazos, y una esfera para la cabeza. El radio es un
        // percentil bajo de la distancia de los vértices del cuerpo (no pelo) que van con ese
        // hueso: el ancho más estrecho, para no empujar el pelo que ya está bien colocado.
        let pairs: [(&str, Option<&str>); 13] = [
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
                SimChain { bones, tips: joints[1..].to_vec(), previous: joints[1..].to_vec(), rest, radii }
            })
            .collect();
        Some(Self { head, chains, colliders, ready: false, offset: Vec3::ZERO, motion: 0.0 })
    }

    /// Un paso de 1/60 s. `skin` son las matrices de skinning de la pose (el pelo no influye
    /// en ellas) y `offset` un desplazamiento del cuerpo entero. `wind` es una fuerza por paso.
    pub fn step(&mut self, skeleton: &Skeleton, skin: &[Mat4], offset: Vec3, params: &HairParams, wind: Vec3) {
        let head = skin[self.head];
        let turn = Quat::from_mat3(&Mat3::from_mat4(head)).normalize();
        self.offset = offset;
        let colliders: Vec<(Vec3, Vec3)> = self.colliders.iter().map(|c| c.segment(skin, skeleton, offset)).collect();
        let first_step = !self.ready;
        self.ready = true;
        let mut motion: f32 = 0.0;
        for chain in &mut self.chains {
            let mut base = head.transform_point3(skeleton.rest_position(chain.bones[0])) + offset;
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

    pub fn reset(&mut self) {
        self.ready = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strand_weights_go_from_head_to_the_tip() {
        let chain = Chain { theta: 0.0, root_y: 1.0, length: 0.1, joints: vec![Vec3::ZERO; 5], first_bone: 10 };
        // Por encima de la raíz, la cabeza entera.
        assert_eq!(chain.weights(1.05, 1.0, 3)[0], (3, 1.0));
        // A un segmento y medio, mitad del primer hueso y mitad del segundo.
        let w = chain.weights(0.85, 1.0, 3);
        assert_eq!((w[0].0, w[1].0), (10, 11));
        assert!((w[0].1 - 0.5).abs() < 1e-4 && (w[1].1 - 0.5).abs() < 1e-4);
        // Muy abajo, el último segmento.
        assert_eq!(chain.weights(-5.0, 1.0, 3)[0].0, 13);
    }

    #[test]
    fn strands_are_named_by_their_side() {
        assert_eq!(direction_name(0.0), "delante");
        assert_eq!(direction_name(std::f32::consts::FRAC_PI_2), "izq");
        assert_eq!(direction_name(-std::f32::consts::FRAC_PI_2), "der");
        assert_eq!(direction_name(std::f32::consts::PI), "detrás");
    }
}

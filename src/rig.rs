//! Paso 3 de la ruta "importar malla de IA": pone el esqueleto de un modelo base (Base_Female,
//! VRoid CC0) a una malla sin huesos.
//!
//! 1. Escala y centra el modelo base sobre el objetivo (altura sin contar el pelo, pies al suelo).
//! 2. Busca el giro de cada brazo y pierna, hueso a hueso, que mejor encaja el modelo base con el
//!    objetivo: distancia de chamfer en los dos sentidos alrededor de esa extremidad.
//! 3. Transfiere los pesos de cada vértice del objetivo desde los vértices más cercanos del
//!    modelo base en esa pose. El pelo va entero a la cabeza (su física llegará después).
//! 4. Quita la pose: lleva el objetivo a la pose de reposo del modelo base (en T), de modo que su
//!    esqueleto es el del modelo base escalado y le sirven las animaciones VRM tal cual.

use crate::model::{Bone, Model, Skeleton, Vertex};
use crate::pose;
use anyhow::{Context, Result};
use glam::{Mat3, Mat4, Quat, Vec3};
use std::collections::HashMap;

/// Rejilla espacial para buscar vecinos cercanos.
pub struct Grid {
    cell: f32,
    cells: HashMap<[i32; 3], Vec<u32>>,
    pub points: Vec<Vec3>,
}

impl Grid {
    pub fn new(points: Vec<Vec3>, cell: f32) -> Self {
        let mut cells: HashMap<[i32; 3], Vec<u32>> = HashMap::new();
        for (i, p) in points.iter().enumerate() {
            cells.entry(Self::key(*p, cell)).or_default().push(i as u32);
        }
        Self { cell, cells, points }
    }

    fn key(p: Vec3, cell: f32) -> [i32; 3] {
        (p / cell).floor().as_ivec3().to_array()
    }

    /// Los `k` puntos más cercanos a `p`, como (índice, distancia), del más cercano al más lejano.
    pub fn knn(&self, p: Vec3, k: usize) -> Vec<(u32, f32)> {
        self.knn_within(p, k, f32::MAX)
    }

    /// Como `knn`, pero sin buscar más allá de `limit`: lo que esté más lejos no cuenta.
    pub fn knn_within(&self, p: Vec3, k: usize, limit: f32) -> Vec<(u32, f32)> {
        let center = Self::key(p, self.cell);
        let mut best: Vec<(u32, f32)> = Vec::with_capacity(k + 1);
        let max_ring = ((limit / self.cell).ceil().min(63.0) as i32) + 1;
        for ring in 0..=max_ring {
            for dx in -ring..=ring {
                for dy in -ring..=ring {
                    for dz in -ring..=ring {
                        if dx.abs().max(dy.abs()).max(dz.abs()) != ring {
                            continue;
                        }
                        if let Some(list) = self.cells.get(&[center[0] + dx, center[1] + dy, center[2] + dz]) {
                            for &i in list {
                                let d = self.points[i as usize].distance(p);
                                if best.len() < k || d < best[best.len() - 1].1 {
                                    let at = best.partition_point(|x| x.1 < d);
                                    best.insert(at, (i, d));
                                    best.truncate(k);
                                }
                            }
                        }
                    }
                }
            }
            // Todo lo que queda fuera de este anillo está al menos a `ring * cell`.
            if best.len() == k && best[k - 1].1 <= ring as f32 * self.cell {
                break;
            }
        }
        best
    }

    pub fn nearest_distance(&self, p: Vec3) -> f32 {
        self.knn(p, 1).first().map(|x| x.1).unwrap_or(f32::MAX)
    }

    /// Distancia al punto más cercano, o `limit` si no hay ninguno a menos de eso.
    pub fn nearest_within(&self, p: Vec3, limit: f32) -> f32 {
        self.knn_within(p, 1, limit).first().map(|x| x.1.min(limit)).unwrap_or(limit)
    }
}

/// Las cadenas de huesos que se ajustan, en orden: (hueso, hijo que marca su dirección).
const LIMBS: &[(&str, &str)] = &[
    ("leftUpperArm", "leftLowerArm"),
    ("leftLowerArm", "leftHand"),
    ("rightUpperArm", "rightLowerArm"),
    ("rightLowerArm", "rightHand"),
    ("leftUpperLeg", "leftLowerLeg"),
    ("leftLowerLeg", "leftFoot"),
    ("rightUpperLeg", "rightLowerLeg"),
    ("rightLowerLeg", "rightFoot"),
];

pub struct Report {
    pub scale: f32,
    pub limbs: Vec<(String, f32, f32)>,
}

/// Vértices del objetivo que cuentan para el ajuste: todo menos el pelo (el modelo base no lo
/// tiene). Las capas de pelo son las del grupo "Pelo" de la segmentación.
/// Capas de ropa de cintura para abajo (faldas, abrigos largos, pantalones), por el nombre que
/// les da la segmentación: nunca van con los huesos de los brazos, aunque cuelguen junto a las
/// manos.
pub(crate) fn lower_garment_layers(model: &Model) -> Vec<bool> {
    model
        .layers
        .iter()
        .map(|l| {
            let name = l.name.to_lowercase();
            name.starts_with("ropa") && ["cintura", "pierna", "pies"].iter().any(|z| name.contains(z))
        })
        .collect()
}

/// Un objeto marcado a mano: capas (por nombre exacto) que van rígidas con un hueso humanoide.
pub struct Prop {
    pub layers: Vec<String>,
    pub bone: String,
}

pub fn rig(target: &mut Model, base: &Model, props: &[Prop]) -> Result<Report> {
    let base_skeleton = base.skeleton.as_ref().context("el modelo base no tiene esqueleto")?;
    let hair = crate::hair::hair_layers(target);
    let lower_garment = lower_garment_layers(target);

    // 1. Escala y centrado.
    let target_vertices: Vec<(Vec3, Vec3)> = target
        .layers
        .iter()
        .enumerate()
        .filter(|(i, _)| !hair[*i])
        .flat_map(|(_, l)| l.primitives.iter().flat_map(|p| p.vertices.iter().map(|v| (Vec3::from(v.position), Vec3::from(v.normal)))))
        .collect();
    let target_points: Vec<Vec3> = target_vertices.iter().map(|v| v.0).collect();
    let target_normals: Vec<Vec3> = target_vertices.iter().map(|v| v.1).collect();
    anyhow::ensure!(!target_points.is_empty(), "el objetivo no tiene vértices fuera del pelo");
    let bounds = |pts: &[Vec3]| pts.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let (tlo, thi) = bounds(&target_points);
    let base_points: Vec<Vec3> = base.layers.iter().flat_map(|l| l.primitives.iter().flat_map(|p| p.vertices.iter().map(|v| Vec3::from(v.position)))).collect();
    let (blo, bhi) = bounds(&base_points);
    let scale = (thi.y - tlo.y) / (bhi.y - blo.y);
    let base_center = Vec3::new((blo.x + bhi.x) * 0.5, blo.y, (blo.z + bhi.z) * 0.5);
    let target_center = Vec3::new((tlo.x + thi.x) * 0.5, tlo.y, (tlo.z + thi.z) * 0.5);
    let fit = Mat4::from_translation(target_center) * Mat4::from_scale(Vec3::splat(scale)) * Mat4::from_translation(-base_center);

    // Esqueleto base ajustado (solo traslaciones, como en VRoid) y sus vértices.
    let skeleton = Skeleton {
        bones: base_skeleton
            .bones
            .iter()
            .map(|b| Bone { name: b.name.clone(), parent: b.parent, rest_world: Mat4::from_translation(fit.transform_point3(b.rest_world.w_axis.truncate())) })
            .collect(),
        humanoid: base_skeleton.humanoid.clone(),
    };
    struct BaseVertex {
        rest: Vec3,
        normal: Vec3,
        joints: [u16; 4],
        weights: [f32; 4],
    }
    let base_vertices: Vec<BaseVertex> = base
        .layers
        .iter()
        .flat_map(|l| &l.primitives)
        .filter(|p| !p.joints.is_empty())
        .flat_map(|p| p.vertices.iter().zip(p.joints.iter().zip(&p.weights)).map(|(v, (j, w))| BaseVertex { rest: fit.transform_point3(Vec3::from(v.position)), normal: Vec3::from(v.normal), joints: *j, weights: *w }))
        .collect();
    // VRoid repite el mismo búfer de vértices en cada primitiva: se quitan los repetidos. También
    // la falda del modelo base (huesos de falda): ocupa el sitio donde cuelgan las manos del
    // objetivo y les daría pesos de pierna.
    let skirt: Vec<bool> = skeleton.bones.iter().map(|b| b.name.to_lowercase().contains("skirt")).collect();
    let base_vertices: Vec<BaseVertex> = {
        let mut seen = std::collections::HashSet::new();
        base_vertices
            .into_iter()
            .filter(|v| (0..4).all(|k| v.weights[k] < 0.05 || !skirt[v.joints[k] as usize]))
            .filter(|v| seen.insert(v.rest.to_array().map(f32::to_bits)))
            .collect()
    };
    let posed_base = |local: &[Quat]| -> Vec<Vec3> {
        let skin = pose::skin_matrices(&skeleton, local);
        base_vertices
            .iter()
            .map(|v| {
                let mut m = Mat4::ZERO;
                for k in 0..4 {
                    m += skin[v.joints[k] as usize] * v.weights[k];
                }
                m.transform_point3(v.rest)
            })
            .collect()
    };

    // 2. Ajuste de extremidades.
    let cell = (thi.y - tlo.y) * 0.02;
    // Distancia a partir de la cual un punto "no encaja" igual de mal: acota las búsquedas.
    let far = (thi.y - tlo.y) * 0.15;
    let target_grid = Grid::new(target_points.clone(), cell);
    // Muestra del objetivo para el sentido objetivo -> base.
    let step = (target_points.len() / 4000).max(1);
    let target_sample: Vec<Vec3> = target_points.iter().step_by(step).copied().collect();
    let mut local = vec![Quat::IDENTITY; skeleton.bones.len()];
    // Las extremidades empiezan en una A-pose suave, más cerca de la mayoría de modelos.
    for (bone, child, dir) in [
        ("leftUpperArm", "leftLowerArm", Vec3::new(0.5, -0.85, 0.0)),
        ("rightUpperArm", "rightLowerArm", Vec3::new(-0.5, -0.85, 0.0)),
    ] {
        if let (Some(b), Some(c)) = (skeleton.bone(bone), skeleton.bone(child)) {
            pose::aim(&skeleton, &mut local, b, c, dir);
        }
    }
    let descendants = |bone: usize| -> Vec<bool> {
        let mut inside = vec![false; skeleton.bones.len()];
        for b in 0..skeleton.bones.len() {
            let mut j = Some(b);
            while let Some(k) = j {
                if k == bone {
                    inside[b] = true;
                    break;
                }
                j = skeleton.bones[k].parent;
            }
        }
        inside
    };
    let mut limbs = Vec::new();
    eprintln!("ajuste: escala {scale:.3}, {} vértices base, {} puntos objetivo", base_vertices.len(), target_points.len());
    for &(bone_name, child_name) in LIMBS {
        let (Some(bone), Some(child)) = (skeleton.bone(bone_name), skeleton.bone(child_name)) else { continue };
        let inside = descendants(bone);
        // Vértices de la extremidad: los que dependen sobre todo de este hueso o sus hijos.
        let limb: Vec<usize> = base_vertices
            .iter()
            .enumerate()
            .filter(|(_, v)| (0..4).map(|k| if inside[v.joints[k] as usize] { v.weights[k] } else { 0.0 }).sum::<f32>() > 0.5)
            .map(|(i, _)| i)
            .collect();
        // Con unos 1500 vértices basta para medir el encaje de la extremidad.
        let limb: Vec<usize> = limb.iter().step_by((limb.len() / 1500).max(1)).copied().collect();
        if limb.is_empty() {
            continue;
        }
        let mut in_limb = vec![false; base_vertices.len()];
        for &i in &limb {
            in_limb[i] = true;
        }
        let t0 = std::time::Instant::now();
        let posed = posed_base(&local);
        eprintln!("{bone_name}: pose base {:?}", t0.elapsed());
        let rest_grid = Grid::new(posed.iter().enumerate().filter(|(i, _)| !in_limb[*i]).map(|(_, p)| *p).collect(), cell);
        // Distancia de cada punto de la muestra al resto del modelo base, que no cambia.
        let static_dist: Vec<f32> = target_sample.iter().map(|p| rest_grid.nearest_within(*p, far)).collect();
        eprintln!("{bone_name}: distancias estáticas {:?} ({} puntos de muestra, celda {cell:.4}, lejos {far:.4})", t0.elapsed(), target_sample.len());
        // Solo cuentan los puntos que el resto del cuerpo no explica ya: los de cerca no pueden
        // mejorar mucho, y así la búsqueda es mucho más corta.
        let open: Vec<(Vec3, f32)> = target_sample.iter().zip(&static_dist).filter(|(_, s)| **s > cell).map(|(p, s)| (*p, *s)).collect();
        let explained: f32 = static_dist.iter().filter(|s| **s <= cell).sum();
        let world = pose::world_matrices(&skeleton, &local);
        let current = (world[child].w_axis.truncate() - world[bone].w_axis.truncate()).normalize();
        let score = |dir: Vec3| -> f32 {
            let mut trial = local.clone();
            pose::aim(&skeleton, &mut trial, bone, child, dir);
            let posed = posed_base(&trial);
            let moved: Vec<Vec3> = limb.iter().map(|&i| posed[i]).collect();
            // Distancia de la extremidad a la superficie del objetivo; si queda por dentro (al otro
            // lado de la normal), cuenta triple: así el brazo no se esconde dentro del torso.
            let a: f32 = moved
                .iter()
                .map(|p| match target_grid.knn_within(*p, 1, far).first() {
                    Some(&(i, d)) => if (*p - target_points[i as usize]).dot(target_normals[i as usize]) < 0.0 { d * 3.0 } else { d },
                    None => far,
                })
                .sum::<f32>()
                / moved.len() as f32;
            let limb_grid = Grid::new(moved, cell * 2.0);
            let b: f32 = (explained + open.iter().map(|(p, s)| limb_grid.nearest_within(*p, *s)).sum::<f32>()) / target_sample.len() as f32;
            a + b
        };
        let t = std::time::Instant::now();
        let before = score(current);
        eprintln!("{bone_name}: {} vértices, {} puntos abiertos, una evaluación {:?}", limb.len(), open.len(), t.elapsed());
        let mut best = (before, current);
        // Primero toda la esfera (brazo y muslo) o un cono amplio (antebrazo y pierna), para
        // encontrar también un brazo levantado; después se afina. Los candidatos se evalúan en
        // paralelo.
        let wide = if bone_name.contains("Upper") { (180f32, 400usize) } else { (150.0, 240) };
        for (cap, count) in [wide, (15.0, 60), (4.0, 30)] {
            let candidates = cap_directions(best.1, cap.to_radians(), count);
            let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
            let chunk = candidates.len().div_ceil(threads);
            let found = std::thread::scope(|scope| {
                let handles: Vec<_> = candidates
                    .chunks(chunk)
                    .map(|part| {
                        let score = &score;
                        scope.spawn(move || part.iter().map(|d| (score(*d), *d)).min_by(|a, b| a.0.total_cmp(&b.0)))
                    })
                    .collect();
                handles.into_iter().filter_map(|h| h.join().ok().flatten()).min_by(|a, b| a.0.total_cmp(&b.0))
            });
            if let Some(f) = found {
                if f.0 < best.0 {
                    best = f;
                }
            }
        }
        pose::aim(&skeleton, &mut local, bone, child, best.1);
        eprintln!("{bone_name}: {before:.4} -> {:.4}", best.0);
        limbs.push((bone_name.to_string(), before, best.0));
    }

    // 3. Pesos por cercanía en la pose ajustada.
    let posed = posed_base(&local);
    let posed_normals: Vec<Vec3> = {
        let skin = pose::skin_matrices(&skeleton, &local);
        base_vertices
            .iter()
            .map(|v| {
                let mut m = Mat4::ZERO;
                for k in 0..4 {
                    m += skin[v.joints[k] as usize] * v.weights[k];
                }
                (Mat3::from_mat4(m) * v.normal).normalize_or_zero()
            })
            .collect()
    };
    // Depuración: con A2W_DEBUG_FIT=archivo.glb se escribe el objetivo con el modelo base ajustado
    // encima, como una capa más.
    if let Ok(path) = std::env::var("A2W_DEBUG_FIT") {
        use crate::model::{Layer, Primitive};
        let vertices: Vec<Vertex> = posed.iter().map(|p| Vertex { position: p.to_array(), normal: [0.0, 1.0, 0.0], uv: [0.0, 0.0] }).collect();
        // Puntos sueltos como triángulos diminutos, para verlos en el visor.
        let mut tri_vertices = Vec::new();
        let mut indices = Vec::new();
        let r = cell * 0.15;
        for v in &vertices {
            let p = Vec3::from(v.position);
            for o in [Vec3::new(r, 0.0, 0.0), Vec3::new(0.0, r, 0.0), Vec3::new(0.0, 0.0, r)] {
                tri_vertices.push(Vertex { position: (p + o).to_array(), ..*v });
            }
            let n = tri_vertices.len() as u32;
            indices.extend([n - 3, n - 2, n - 1]);
        }
        let mut debug = Model { name: "ajuste".into(), layers: Vec::new(), materials: Vec::new(), textures: Vec::new(), skeleton: None };
        std::mem::swap(&mut debug.materials, &mut target.materials);
        std::mem::swap(&mut debug.textures, &mut target.textures);
        std::mem::swap(&mut debug.layers, &mut target.layers);
        let material = debug.materials.len().saturating_sub(1);
        debug.layers.push(Layer { name: "BASE AJUSTADA".into(), parent: None, depth: 0, primitives: vec![Primitive { vertices: tri_vertices, indices, material, joints: Vec::new(), weights: Vec::new() }], visible: true, opacity: 1.0, expanded: true });
        let _ = crate::export_glb::write(&debug, std::path::Path::new(&path));
        debug.layers.pop();
        std::mem::swap(&mut debug.materials, &mut target.materials);
        std::mem::swap(&mut debug.textures, &mut target.textures);
        std::mem::swap(&mut debug.layers, &mut target.layers);
    }
    let base_grid = Grid::new(posed, cell);
    let head = skeleton.bone("head").context("el modelo base no tiene hueso head")?;
    let skin = pose::skin_matrices(&skeleton, &local);

    // Cadena de cada hueso: 0 torso (y cabeza), 1 brazo izquierdo, 2 brazo derecho, 3 pierna
    // izquierda, 4 pierna derecha.
    let roots = ["leftUpperArm", "rightUpperArm", "leftUpperLeg", "rightUpperLeg"].map(|n| skeleton.bone(n));
    let chain_of_bone: Vec<u8> = (0..skeleton.bones.len())
        .map(|b| {
            let mut j = Some(b);
            while let Some(k) = j {
                if let Some(c) = roots.iter().position(|r| *r == Some(k)) {
                    return c as u8 + 1;
                }
                j = skeleton.bones[k].parent;
            }
            0
        })
        .collect();

    // Vértices del objetivo soldados por posición (las capas parten la malla en trozos que
    // comparten bordes) y su vecindad, para razonar sobre la superficie conectada.
    let weld_cell = cell * 1e-3;
    let mut weld: HashMap<[i64; 3], u32> = HashMap::new();
    let mut global: Vec<Vec<Vec<u32>>> = Vec::new();
    let mut points: Vec<(Vec3, Vec3, bool)> = Vec::new();
    let mut lower: Vec<bool> = Vec::new();
    let mut layer_of: Vec<usize> = Vec::new();
    for (li, layer) in target.layers.iter().enumerate() {
        let mut per_layer = Vec::new();
        for primitive in &layer.primitives {
            per_layer.push(
                primitive
                    .vertices
                    .iter()
                    .map(|v| {
                        let key = v.position.map(|c| (c / weld_cell).round() as i64);
                        *weld.entry(key).or_insert_with(|| {
                            points.push((Vec3::from(v.position), Vec3::from(v.normal), hair[li]));
                            lower.push(lower_garment[li]);
                            layer_of.push(li);
                            (points.len() - 1) as u32
                        })
                    })
                    .collect::<Vec<u32>>(),
            );
        }
        global.push(per_layer);
    }
    let mut adjacency: Vec<Vec<u32>> = vec![Vec::new(); points.len()];
    for (li, layer) in target.layers.iter().enumerate() {
        for (pi, primitive) in layer.primitives.iter().enumerate() {
            for tri in primitive.indices.chunks_exact(3) {
                let g = [0, 1, 2].map(|k| global[li][pi][tri[k] as usize]);
                for (a, b) in [(g[0], g[1]), (g[1], g[2]), (g[2], g[0])] {
                    if a != b {
                        adjacency[a as usize].push(b);
                        adjacency[b as usize].push(a);
                    }
                }
            }
        }
    }

    // Donantes de cada vértice: vecinos del modelo base cuya normal mira hacia el mismo lado
    // (la cara interior del brazo no hereda de la cadera que tiene enfrente), y solo los casi
    // tan cercanos como el primero.
    let donors: Vec<Vec<(u32, f32)>> = points
        .iter()
        .map(|(p, n, _)| {
            let candidates = base_grid.knn(*p, 24);
            let facing: Vec<(u32, f32)> = candidates.iter().filter(|(i, _)| posed_normals[*i as usize].dot(*n) > 0.2).take(8).copied().collect();
            let near = if facing.is_empty() { candidates.into_iter().take(8).collect::<Vec<_>>() } else { facing };
            let first = near.first().map(|x| x.1).unwrap_or(0.0);
            near.into_iter().filter(|x| x.1 <= first * 1.5 + cell * 0.25).collect()
        })
        .collect();
    // Cadena de cada vértice por votos de sus donantes. La ropa de cintura para abajo no vota
    // por los brazos.
    let votes: Vec<[f32; 5]> = (0..points.len())
        .map(|g| {
            let mut votes = [0f32; 5];
            if points[g].2 {
                votes[0] = 1.0;
                return votes;
            }
            for &(i, d) in &donors[g] {
                let v = &base_vertices[i as usize];
                for k in 0..4 {
                    votes[chain_of_bone[v.joints[k] as usize] as usize] += v.weights[k] / (d + 1e-4);
                }
            }
            if lower[g] {
                votes[1] = 0.0;
                votes[2] = 0.0;
            }
            votes
        })
        .collect();
    let best_chain = |votes: &[f32; 5], options: &[u8]| -> u8 { *options.iter().max_by(|a, b| votes[**a as usize].total_cmp(&votes[**b as usize])).unwrap() };
    let mut chain: Vec<u8> = votes.iter().map(|v| best_chain(v, &[0, 1, 2, 3, 4])).collect();
    // Filtro de mayoría sobre la malla: quita rayas finas de una cadena dentro de otra.
    for _ in 0..6 {
        chain = (0..points.len())
            .map(|v| {
                let mut count = [0usize; 5];
                count[chain[v] as usize] += 1;
                for &n in &adjacency[v] {
                    count[chain[n as usize] as usize] += 1;
                }
                let best = (0..5).max_by_key(|c| count[*c]).unwrap();
                if count[best] * 2 > adjacency[v].len() + 1 { best as u8 } else { chain[v] }
            })
            .collect();
    }
    // Islas: piezas de la malla sin conexión con el resto (muñequeras, pendientes…). Las que no
    // son la pieza principal toman, entera, la cadena que más votos tenga en ella.
    let island: Vec<u32> = {
        let mut island = vec![u32::MAX; points.len()];
        let mut next = 0;
        for start in 0..points.len() {
            if island[start] != u32::MAX {
                continue;
            }
            let mut stack = vec![start];
            island[start] = next;
            while let Some(v) = stack.pop() {
                for &n in &adjacency[v] {
                    if island[n as usize] == u32::MAX {
                        island[n as usize] = next;
                        stack.push(n as usize);
                    }
                }
            }
            next += 1;
        }
        island
    };
    let main_island = {
        let mut count: HashMap<u32, usize> = HashMap::new();
        for &i in &island {
            *count.entry(i).or_default() += 1;
        }
        count.into_iter().max_by_key(|x| x.1).map(|x| x.0).unwrap_or(0)
    };
    {
        let mut votes: HashMap<u32, [usize; 5]> = HashMap::new();
        for v in 0..points.len() {
            if island[v] != main_island && !points[v].2 {
                votes.entry(island[v]).or_default()[chain[v] as usize] += 1;
            }
        }
        for v in 0..points.len() {
            if let Some(count) = votes.get(&island[v]) {
                chain[v] = (0..5).max_by_key(|c| count[*c]).unwrap() as u8;
            }
        }
    }
    // De cada extremidad solo vale el trozo conexo más grande: los trozos pequeños (un parche
    // del muslo junto a la mano) son confusiones y toman la cadena de sus vecinos.
    {
        const UNKNOWN: u8 = u8::MAX;
        let mut component = vec![u32::MAX; points.len()];
        let mut sizes: Vec<(u8, usize)> = Vec::new();
        for start in 0..points.len() {
            if component[start] != u32::MAX {
                continue;
            }
            let id = sizes.len() as u32;
            let mut stack = vec![start];
            component[start] = id;
            let mut size = 0;
            while let Some(v) = stack.pop() {
                size += 1;
                for &n in &adjacency[v] {
                    if component[n as usize] == u32::MAX && chain[n as usize] == chain[start] {
                        component[n as usize] = id;
                        stack.push(n as usize);
                    }
                }
            }
            sizes.push((chain[start], size));
        }
        let mut biggest = [None::<(u32, usize)>; 5];
        let mut island_of_component = vec![0u32; sizes.len()];
        for v in 0..points.len() {
            island_of_component[component[v] as usize] = island[v];
        }
        for (id, &(c, size)) in sizes.iter().enumerate() {
            if c > 0 && island_of_component[id] == main_island && biggest[c as usize].is_none_or(|(_, s)| size > s) {
                biggest[c as usize] = Some((id as u32, size));
            }
        }
        for v in 0..points.len() {
            let c = chain[v];
            if c > 0 && island[v] == main_island && biggest[c as usize].map(|(id, _)| id) != Some(component[v]) {
                chain[v] = UNKNOWN;
            }
        }
        // Relleno desde los vecinos conocidos, en anchura.
        let mut queue: std::collections::VecDeque<usize> = (0..points.len()).filter(|&v| chain[v] != UNKNOWN).collect();
        while let Some(v) = queue.pop_front() {
            for &n in &adjacency[v] {
                if chain[n as usize] == UNKNOWN {
                    chain[n as usize] = chain[v];
                    queue.push_back(n as usize);
                }
            }
        }
        for c in &mut chain {
            if *c == UNKNOWN {
                *c = 0;
            }
        }
    }

    // Depuración: con A2W_DEBUG_CHAIN=archivo.glb se escribe el objetivo partido por cadenas.
    if let Ok(path) = std::env::var("A2W_DEBUG_CHAIN") {
        use crate::model::Layer;
        let names = ["torso", "brazo izq", "brazo der", "pierna izq", "pierna der"];
        let mut layers: Vec<Layer> = names.iter().map(|n| Layer { name: n.to_string(), parent: None, depth: 0, primitives: Vec::new(), visible: true, opacity: 1.0, expanded: true }).collect();
        for (li, layer) in target.layers.iter().enumerate() {
            for (pi, primitive) in layer.primitives.iter().enumerate() {
                let mut by_chain: Vec<Vec<usize>> = vec![Vec::new(); 5];
                for (t, tri) in primitive.indices.chunks_exact(3).enumerate() {
                    by_chain[chain[global[li][pi][tri[0] as usize] as usize] as usize].push(t);
                }
                for (c, tris) in by_chain.iter().enumerate() {
                    if !tris.is_empty() {
                        layers[c].primitives.push(primitive.subset(tris));
                    }
                }
            }
        }
        let debug = Model { name: "cadenas".into(), layers, materials: std::mem::take(&mut target.materials), textures: std::mem::take(&mut target.textures), skeleton: None };
        let _ = crate::export_glb::write(&debug, std::path::Path::new(&path));
        target.materials = debug.materials;
        target.textures = debug.textures;
        let mut crossing = [[0usize; 5]; 5];
        for (v, list) in adjacency.iter().enumerate() {
            for &n in list {
                crossing[chain[v] as usize][chain[n as usize] as usize] += 1;
            }
        }
        eprintln!("aristas entre cadenas (torso, bi, bd, pi, pd): {crossing:?}");
    }

    // Geometría extra: vértices lejos de cualquier superficie del cuerpo base (armas, objetos en
    // la mano, piezas grandes). No se les buscan pesos propios: van rígidos con su ancla, el
    // vértice del cuerpo más cercano a través de la malla (o en el espacio, si la pieza está
    // suelta).
    let extra_limit = (thi.y - tlo.y) * 0.07;
    let is_extra: Vec<bool> = (0..points.len()).map(|g| !points[g].2 && donors[g].first().is_none_or(|x| x.1 > extra_limit)).collect();
    let mut is_extra = is_extra;
    let mut anchor: Vec<u32> = {
        let mut anchor: Vec<u32> = (0..points.len() as u32).collect();
        let mut seen: Vec<bool> = is_extra.iter().map(|e| !e).collect();
        let mut queue: std::collections::VecDeque<usize> = (0..points.len()).filter(|&g| !is_extra[g]).collect();
        while let Some(v) = queue.pop_front() {
            for &n in &adjacency[v] {
                let n = n as usize;
                if !seen[n] {
                    seen[n] = true;
                    anchor[n] = anchor[v];
                    queue.push_back(n);
                }
            }
        }
        let body: Vec<usize> = (0..points.len()).filter(|&g| !is_extra[g]).collect();
        if !body.is_empty() {
            let grid = Grid::new(body.iter().map(|&g| points[g].0).collect(), cell);
            for g in 0..points.len() {
                if !seen[g] {
                    if let Some(&(i, _)) = grid.knn(points[g].0, 1).first() {
                        anchor[g] = body[i as usize] as u32;
                    }
                }
            }
        }
        anchor
    };
    // Objetos: regiones de la segmentación con buena parte lejos del cuerpo (una motosierra, una
    // espada). Van enteros y rígidos con un solo ancla, la parte del cuerpo que más los toca;
    // si no, cada trozo seguiría a un hueso distinto y el objeto se estiraría.
    {
        let layers = target.layers.len();
        let mut total = vec![0usize; layers];
        let mut far = vec![0usize; layers];
        for g in 0..points.len() {
            total[layer_of[g]] += 1;
            if is_extra[g] {
                far[layer_of[g]] += 1;
            }
        }
        // Nunca piel ni ropa de cintura para abajo (una media lejos del cuerpo base sigue siendo
        // una media), y solo si más de la mitad está lejos del cuerpo.
        let prop_layer: Vec<bool> = (0..layers)
            .map(|l| {
                !hair[l] && !lower_garment[l] && !target.layers[l].name.to_lowercase().starts_with("piel") && total[l] >= 30 && far[l] * 2 > total[l]
            })
            .collect();
        // Regiones objeto que se tocan forman un mismo objeto.
        let mut parent: Vec<usize> = (0..layers).collect();
        fn find(parent: &mut [usize], mut x: usize) -> usize {
            while parent[x] != x {
                parent[x] = parent[parent[x]];
                x = parent[x];
            }
            x
        }
        for g in 0..points.len() {
            let a = layer_of[g];
            for &n in &adjacency[g] {
                let b = layer_of[n as usize];
                if a != b && prop_layer[a] && prop_layer[b] {
                    let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                    parent[rb] = ra;
                }
            }
        }
        let group_of: Vec<Option<usize>> = (0..layers).map(|l| prop_layer[l].then(|| find(&mut parent, l))).collect();
        let is_prop = |g: usize| group_of[layer_of[g]].is_some();
        // Contactos de cada objeto con el cuerpo (ni objeto ni pelo), por cadena.
        let mut contacts: HashMap<usize, Vec<usize>> = HashMap::new();
        for g in 0..points.len() {
            if let Some(group) = group_of[layer_of[g]] {
                for &n in &adjacency[g] {
                    let n = n as usize;
                    if !is_prop(n) && !points[n].2 && !is_extra[n] {
                        contacts.entry(group).or_default().push(n);
                    }
                }
            }
        }
        let body: Vec<usize> = (0..points.len()).filter(|&g| !is_prop(g) && !points[g].2 && !is_extra[g]).collect();
        let body_grid = (!body.is_empty()).then(|| Grid::new(body.iter().map(|&g| points[g].0).collect(), cell));
        let groups: std::collections::HashSet<usize> = group_of.iter().flatten().copied().collect();
        for group in groups {
            let members: Vec<usize> = (0..points.len()).filter(|&g| group_of[layer_of[g]] == Some(group)).collect();
            let mut touching = contacts.remove(&group).unwrap_or_default();
            if touching.is_empty() {
                // Pieza suelta: los puntos del cuerpo más cercanos a ella.
                if let Some(grid) = &body_grid {
                    for &g in members.iter().step_by((members.len() / 200).max(1)) {
                        if let Some(&(i, d)) = grid.knn(points[g].0, 1).first() {
                            if d < extra_limit * 2.0 {
                                touching.push(body[i as usize]);
                            }
                        }
                    }
                }
            }
            if touching.is_empty() {
                continue;
            }
            // La cadena con más contactos (la mano que lo agarra), y de ella el contacto más
            // repetido como ancla.
            let mut by_chain = [0usize; 5];
            for &t in &touching {
                by_chain[chain[t] as usize] += 1;
            }
            let best = (0..5).max_by_key(|c| by_chain[*c]).unwrap() as u8;
            let mut count: HashMap<usize, usize> = HashMap::new();
            for &t in touching.iter().filter(|t| chain[**t] == best) {
                *count.entry(t).or_default() += 1;
            }
            let Some((&a, _)) = count.iter().max_by_key(|x| x.1) else { continue };
            for &g in &members {
                is_extra[g] = true;
                anchor[g] = a as u32;
            }
            let name = &target.layers[layer_of[members[0]]].name;
            eprintln!("objeto ({name}…): {} vértices, rígido con la cadena {best}", members.len());
        }
    }
    for g in 0..points.len() {
        if is_extra[g] {
            chain[g] = chain[anchor[g] as usize];
        }
    }
    eprintln!("geometría extra: {} vértices", is_extra.iter().filter(|e| **e).count());
    // Objetos marcados a mano: rígidos con su hueso.
    let mut prop_bone: Vec<Option<u16>> = vec![None; points.len()];
    for prop in props {
        let bone = skeleton.bone(&prop.bone).with_context(|| format!("el modelo base no tiene el hueso humanoide {}", prop.bone))?;
        let wanted: Vec<bool> = target.layers.iter().map(|l| prop.layers.iter().any(|n| *n == l.name)).collect();
        let mut count = 0;
        for g in 0..points.len() {
            if wanted[layer_of[g]] {
                prop_bone[g] = Some(bone as u16);
                chain[g] = chain_of_bone[bone];
                count += 1;
            }
        }
        anyhow::ensure!(count > 0, "ninguna capa se llama así: {:?}", prop.layers);
        eprintln!("objeto marcado: {count} vértices rígidos con {}", prop.bone);
    }

    // La limpieza puede haber pasado algo de ropa larga a un brazo desde sus vecinos: vuelve a
    // su mejor cadena entre torso y piernas.
    for g in 0..points.len() {
        if lower[g] && (chain[g] == 1 || chain[g] == 2) {
            chain[g] = best_chain(&votes[g], &[0, 3, 4]);
        }
    }

    // Los vértices del torso a pocas aristas de una extremidad (hombro, ingle) también pueden
    // usar los huesos de esa extremidad; los que solo están cerca en el espacio, no.
    let mut near_limb: Vec<u8> = vec![0; points.len()]; // bit c-1 si está cerca de la cadena c
    {
        const HOPS: u32 = 10;
        let world = pose::world_matrices(&skeleton, &local);
        let root_radius = (thi.y - tlo.y) * 0.12;
        for c in 1..5u8 {
            // Solo desde la raíz de la extremidad (hombro, ingle): la mano que toca la cadera no
            // presta sus huesos a la cadera.
            let Some(root) = roots[c as usize - 1] else { continue };
            let root_at = world[root].w_axis.truncate();
            let mut depth = vec![u32::MAX; points.len()];
            let mut queue: std::collections::VecDeque<usize> =
                (0..points.len()).filter(|&v| chain[v] == c && points[v].0.distance(root_at) < root_radius).collect();
            for &v in &queue {
                depth[v] = 0;
            }
            while let Some(v) = queue.pop_front() {
                if depth[v] >= HOPS {
                    continue;
                }
                for &n in &adjacency[v] {
                    let n = n as usize;
                    if depth[n] == u32::MAX && chain[n] == 0 {
                        depth[n] = depth[v] + 1;
                        near_limb[n] |= 1 << (c - 1);
                        queue.push_back(n);
                    }
                }
            }
        }
    }

    // Raíces de las extremidades en la pose ajustada: cerca de ellas (hombro, ingle) la piel de
    // la extremidad y la del torso se mezclan; lejos, cada una va con sus propios huesos.
    let world_fit = pose::world_matrices(&skeleton, &local);
    let root_radius = (thi.y - tlo.y) * 0.12;
    let root_at: Vec<Option<Vec3>> = roots.iter().map(|r| r.map(|b| world_fit[b].w_axis.truncate())).collect();
    let near_root = |g: usize, c: u8| c > 0 && root_at[c as usize - 1].is_some_and(|r| points[g].0.distance(r) < root_radius * 1.3);

    let lbs = |j: &[u16; 4], w: &[f32; 4]| -> Mat4 {
        let mut m = Mat4::ZERO;
        for k in 0..4 {
            m += skin[j[k] as usize] * w[k];
        }
        m
    };
    // Pesos de un donante con solo los huesos de las cadenas permitidas (bit 0 torso, bit c la
    // cadena c), renormalizados.
    let filtered = |i: u32, mask: u8| -> Option<([u16; 4], [f32; 4])> {
        let v = &base_vertices[i as usize];
        let mut w = v.weights;
        for k in 0..4 {
            if mask & (1 << chain_of_bone[v.joints[k] as usize]) == 0 {
                w[k] = 0.0;
            }
        }
        let sum: f32 = w.iter().sum();
        (sum > 1e-4).then(|| (v.joints, w.map(|x| x / sum)))
    };
    let hips = skeleton.bone("hips").unwrap_or(0) as u16;
    // Pesos del vértice `g` como si perteneciera a la cadena `c`.
    let weights_for = |g: usize, c: u8| -> ([u16; 4], [f32; 4]) {
        let (p, _, is_hair) = points[g];
        if is_hair {
            return ([head as u16, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
        }
        let mask: u8 = if c > 0 {
            (1 << c) | if near_root(g, c) { 1 } else { 0 }
        } else {
            1 | (near_limb[g] << 1)
        };
        let usable: Vec<([u16; 4], [f32; 4], f32)> = donors[g].iter().filter_map(|&(i, d)| filtered(i, mask).map(|(j, w)| (j, w, d))).collect();
        let blended = if usable.is_empty() {
            // Ningún donante cercano vale: el más cercano que sí.
            base_grid.knn(p, 96).into_iter().find_map(|(i, _)| filtered(i, mask))
        } else {
            Some(blend_weights(usable.iter().map(|(j, w, d)| (j, w, *d))))
        };
        let fallback = if c > 0 { roots[c as usize - 1].map_or(hips, |b| b as u16) } else { hips };
        let (mut j, mut w) = blended.unwrap_or(([fallback, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]));
        // Si la mezcla deja una matriz casi singular, los pesos del mejor donante solo.
        if Mat3::from_mat4(lbs(&j, &w)).determinant().abs() < 0.3 {
            if let Some((j1, w1, _)) = usable.first() {
                j = *j1;
                w = *w1;
            }
        }
        (j, w)
    };
    let mut cache: HashMap<(u32, u8), ([u16; 4], [f32; 4])> = HashMap::new();

    // Pesos propios de cada vértice (su cadena; la geometría extra, los de su ancla; los objetos
    // marcados, su hueso), suavizados sobre la malla con los vecinos de la misma cadena: quita
    // saltos bruscos que dejan escamas en axilas e ingles al mover los brazos.
    let mut own_weights: Vec<([u16; 4], [f32; 4])> = (0..points.len())
        .map(|g| match prop_bone[g] {
            Some(bone) => ([bone, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
            None => {
                let source = if is_extra[g] { anchor[g] as usize } else { g };
                weights_for(source, chain[g])
            }
        })
        .collect();
    let fixed: Vec<bool> = (0..points.len()).map(|g| prop_bone[g].is_some() || is_extra[g] || points[g].2).collect();
    for _ in 0..4 {
        own_weights = (0..points.len())
            .map(|g| {
                if fixed[g] {
                    return own_weights[g];
                }
                let mut acc: HashMap<u16, f32> = HashMap::new();
                let mut add = |(j, w): &([u16; 4], [f32; 4]), f: f32| {
                    for k in 0..4 {
                        if w[k] > 0.0 {
                            *acc.entry(j[k]).or_default() += w[k] * f;
                        }
                    }
                };
                add(&own_weights[g], 2.0);
                for &n in &adjacency[g] {
                    let n = n as usize;
                    if chain[n] == chain[g] && !fixed[n] {
                        add(&own_weights[n], 1.0);
                    }
                }
                let mut list: Vec<(u16, f32)> = acc.into_iter().collect();
                list.sort_by(|a, b| b.1.total_cmp(&a.1));
                list.truncate(4);
                let total: f32 = list.iter().map(|x| x.1).sum::<f32>().max(1e-12);
                let mut j = [0u16; 4];
                let mut w = [0f32; 4];
                for (k, (b, x)) in list.into_iter().enumerate() {
                    j[k] = b;
                    w[k] = x / total;
                }
                (j, w)
            })
            .collect();
    }

    // Pecho: el de Base_Female es pequeño y casi toda la piel del objetivo hereda los pesos del
    // torso. Como en DOA, la zona alrededor de cada hueso de pecho recibe peso de ese hueso, con
    // una caída suave, solo por delante del pecho.
    let bust_radius = (thi.y - tlo.y) * 0.085;
    let busts: Vec<(u16, Vec3, Vec3)> = ["J_Sec_L_Bust1", "J_Sec_R_Bust1"]
        .iter()
        .filter_map(|name| {
            let b = skeleton.find(name)?;
            let tip = skeleton.bones.iter().position(|x| x.parent == Some(b)).map(|t| world_fit[t].w_axis.truncate())?;
            Some((b as u16, world_fit[b].w_axis.truncate(), tip))
        })
        .collect();
    let mut bust_vertices = 0;
    for g in 0..points.len() {
        if fixed[g] || chain[g] != 0 {
            continue;
        }
        let p = points[g].0;
        let best = busts
            .iter()
            .map(|(bone, root, tip)| {
                let forward = (*tip - *root).normalize_or_zero();
                // Distancia a la punta, y solo si el punto está por delante de la raíz.
                let ahead = (p - *root).dot(forward) > -bust_radius * 0.2;
                let t = (1.0 - p.distance(*tip) / bust_radius).clamp(0.0, 1.0);
                (*bone, if ahead { t * t * (3.0 - 2.0 * t) } else { 0.0 })
            })
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let Some((bone, w)) = best.filter(|x| x.1 > 0.01) else { continue };
        let (j, wt) = own_weights[g];
        let mut acc: HashMap<u16, f32> = HashMap::new();
        for k in 0..4 {
            if wt[k] > 0.0 {
                *acc.entry(j[k]).or_default() += wt[k] * (1.0 - w);
            }
        }
        *acc.entry(bone).or_default() += w;
        let mut list: Vec<(u16, f32)> = acc.into_iter().collect();
        list.sort_by(|a, b| b.1.total_cmp(&a.1));
        list.truncate(4);
        let total: f32 = list.iter().map(|x| x.1).sum::<f32>().max(1e-12);
        let mut nj = [0u16; 4];
        let mut nw = [0f32; 4];
        for (k, (bb, x)) in list.into_iter().enumerate() {
            nj[k] = bb;
            nw[k] = x / total;
        }
        own_weights[g] = (nj, nw);
        bust_vertices += 1;
    }
    eprintln!("pecho: {bust_vertices} vértices con peso de los huesos de pecho");

    // 4. Quitar la pose: cada vértice vuelve a la pose de reposo del modelo base. Donde la malla
    // une una extremidad con otra parte lejos de su raíz (la mano fusionada con la cadera), se
    // corta: el vértice se duplica, una copia por cadena, para que al moverse no arrastre la piel.
    let mut cuts = 0usize;
    for (li, layer) in target.layers.iter_mut().enumerate() {
        for (pi, primitive) in layer.primitives.iter_mut().enumerate() {
            let ids = &global[li][pi];
            let mut vertices = Vec::with_capacity(primitive.vertices.len());
            let (mut joints, mut weights) = (Vec::new(), Vec::new());
            let mut remap: HashMap<(u32, u8), u32> = HashMap::new();
            let mut indices = Vec::with_capacity(primitive.indices.len());
            for tri in primitive.indices.chunks_exact(3) {
                let c = [0, 1, 2].map(|k| chain[ids[tri[k] as usize] as usize]);
                let tri_chain = if c[1] == c[2] { c[1] } else { c[0] };
                for k in 0..3 {
                    let old = tri[k];
                    let g = ids[old as usize] as usize;
                    let own = chain[g];
                    // Dos extremidades distintas que se tocan se separan siempre. Una extremidad y el
                    // torso, solo lejos de la raíz de la extremidad: el hombro sigue unido, la mano
                    // apoyada en la cadera no.
                    let cut = own != tri_chain
                        && match (own.min(tri_chain), own.max(tri_chain)) {
                            (0, limb) => !near_root(g, limb),
                            _ => true,
                        };
                    let use_chain = if cut { tri_chain } else { own };
                    let new = *remap.entry((old, use_chain)).or_insert_with(|| {
                        if cut {
                            cuts += 1;
                        }
                        // La geometría extra lleva los pesos de su ancla.
                        let source = if is_extra[g] { anchor[g] as usize } else { g };
                        let (j, w) = match prop_bone[g] {
                            Some(bone) => ([bone, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
                            None if !cut => own_weights[g],
                            None => *cache.entry((source as u32, use_chain)).or_insert_with(|| weights_for(source, use_chain)),
                        };
                        let v = &primitive.vertices[old as usize];
                        let inv = lbs(&j, &w).inverse();
                        let rest = inv.transform_point3(Vec3::from(v.position));
                        let normal = (Mat3::from_mat4(inv) * Vec3::from(v.normal)).normalize_or_zero();
                        vertices.push(Vertex { position: rest.to_array(), normal: normal.to_array(), uv: v.uv });
                        joints.push(j);
                        weights.push(w);
                        (vertices.len() - 1) as u32
                    });
                    indices.push(new);
                }
            }
            primitive.vertices = vertices;
            primitive.indices = indices;
            primitive.joints = joints;
            primitive.weights = weights;
        }
    }
    eprintln!("cortes: {cuts} vértices duplicados");
    target.skeleton = Some(skeleton);
    Ok(Report { scale, limbs })
}

/// Mezcla los pesos de varios vecinos, por el inverso de la distancia, y se queda con los cuatro
/// huesos más pesados.
fn blend_weights<'a>(neighbors: impl Iterator<Item = (&'a [u16; 4], &'a [f32; 4], f32)>) -> ([u16; 4], [f32; 4]) {
    let mut acc: HashMap<u16, f32> = HashMap::new();
    for (j, w, d) in neighbors {
        let f = 1.0 / (d + 1e-4);
        for k in 0..4 {
            if w[k] > 0.0 {
                *acc.entry(j[k]).or_default() += w[k] * f;
            }
        }
    }
    let mut list: Vec<(u16, f32)> = acc.into_iter().collect();
    list.sort_by(|a, b| b.1.total_cmp(&a.1));
    list.truncate(4);
    let total: f32 = list.iter().map(|x| x.1).sum::<f32>().max(1e-12);
    let mut joints = [0u16; 4];
    let mut weights = [0f32; 4];
    for (k, (j, w)) in list.into_iter().enumerate() {
        joints[k] = j;
        weights[k] = w / total;
    }
    (joints, weights)
}

/// `count` direcciones repartidas (espiral de Fibonacci) dentro de un cono de semiángulo `cap`
/// alrededor de `center`, incluido el propio centro.
fn cap_directions(center: Vec3, cap: f32, count: usize) -> Vec<Vec3> {
    let rotation = Quat::from_rotation_arc(Vec3::Z, center.normalize());
    let golden = std::f32::consts::PI * (3.0 - 5f32.sqrt());
    let min_cos = cap.cos();
    let mut dirs = vec![center.normalize()];
    for i in 0..count {
        let z = 1.0 - (1.0 - min_cos) * (i as f32 + 0.5) / count as f32;
        let r = (1.0 - z * z).max(0.0).sqrt();
        let a = golden * i as f32;
        dirs.push(rotation * Vec3::new(r * a.cos(), r * a.sin(), z));
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knn_finds_the_closest_points_in_order() {
        let points = (0..100).map(|i| Vec3::new(i as f32 * 0.1, 0.0, 0.0)).collect();
        let grid = Grid::new(points, 0.25);
        let found = grid.knn(Vec3::new(2.04, 0.0, 0.0), 3);
        let ids: Vec<u32> = found.iter().map(|x| x.0).collect();
        assert_eq!(ids, vec![20, 21, 19]);
    }

    #[test]
    fn cap_directions_stay_inside_the_cone() {
        let center = Vec3::new(1.0, -1.0, 0.0).normalize();
        for d in cap_directions(center, 0.3, 50) {
            assert!(d.dot(center) >= 0.3f32.cos() - 1e-4);
        }
    }

    #[test]
    fn blended_weights_add_up_to_one() {
        let (j1, w1) = ([1u16, 2, 0, 0], [0.5f32, 0.5, 0.0, 0.0]);
        let (j2, w2) = ([3u16, 0, 0, 0], [1.0f32, 0.0, 0.0, 0.0]);
        let (_, w) = blend_weights([(&j1, &w1, 0.1), (&j2, &w2, 0.2)].into_iter());
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-5);
    }
}

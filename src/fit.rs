//! Ruta desde imagen, paso 3: Base_Female con la figura de la imagen, y la imagen como textura.
//!
//! **Ajuste.** Con los puntos clave del análisis (paso 2b):
//! - la escala sale de la altura: metros por píxel, con la altura de Base_Female;
//! - en vertical, por tramos (pies → cintura → hombros → barbilla), cada altura de Base_Female
//!   va a la de la imagen: piernas, tronco y cuello se alargan o se acortan;
//! - la cabeza (todo lo que va con ella) se escala entera desde la barbilla, a la altura de la
//!   cabeza de la imagen;
//! - el tronco se ensancha o se estrecha con los anchos de hombros y cintura (interpolados por
//!   alturas); brazos y piernas se desplazan con su articulación, y los brazos se alargan en la
//!   proporción del tronco.
//!
//! **Textura.** Primera aproximación: con el cuerpo en una pose de brazos caídos, cada vértice
//! toma la UV del píxel de la imagen que tiene delante (proyección frontal). La espalda recibe
//! el mismo dibujo. Lo transparente de la imagen se rellena con el color más cercano del
//! personaje, para que los bordes no salgan del color del fondo.

use crate::analyze::Keypoints;
use crate::model::{AlphaMode, Material, Model, Skeleton, Texture};
use anyhow::{Context, Result};
use glam::{Mat4, Quat, Vec3};
use image::RgbaImage;

/// Medidas de Base_Female, en metros, en su pose de reposo (T).
#[derive(Debug)]
struct BaseKeys {
    top: f32,
    chin: f32,
    shoulders: f32,
    waist: f32,
    feet: f32,
    shoulder_width: f32,
    waist_width: f32,
    hip: f32,
}

/// A qué parte del cuerpo va cada hueso, por su antecesor humanoide.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Region {
    Torso,
    Neck,
    Head,
    Arm(f32),
    Leg(f32),
}

fn regions(skeleton: &Skeleton) -> Vec<Region> {
    let named = |n: &str| skeleton.bone(n);
    let roots: Vec<(Option<usize>, Region)> = vec![
        (named("head"), Region::Head),
        (named("neck"), Region::Neck),
        (named("leftUpperArm"), Region::Arm(1.0)),
        (named("rightUpperArm"), Region::Arm(-1.0)),
        (named("leftUpperLeg"), Region::Leg(1.0)),
        (named("rightUpperLeg"), Region::Leg(-1.0)),
    ];
    (0..skeleton.bones.len())
        .map(|b| {
            let mut current = Some(b);
            while let Some(c) = current {
                if let Some((_, region)) = roots.iter().find(|(r, _)| *r == Some(c)) {
                    return *region;
                }
                current = skeleton.bones[c].parent;
            }
            Region::Torso
        })
        .collect()
}

/// Interpolación lineal por tramos; fuera de los extremos, sigue la pendiente del último tramo.
fn piecewise(points: &[(f32, f32)], x: f32) -> f32 {
    let n = points.len();
    let segment = if x <= points[0].0 {
        0
    } else if x >= points[n - 1].0 {
        n - 2
    } else {
        (0..n - 1).find(|&i| x <= points[i + 1].0).unwrap_or(n - 2)
    };
    let ((x0, y0), (x1, y1)) = (points[segment], points[segment + 1]);
    if (x1 - x0).abs() < 1e-9 {
        return y0;
    }
    y0 + (x - x0) * (y1 - y0) / (x1 - x0)
}

/// Igual, pero sin salirse de los extremos.
fn clamped(points: &[(f32, f32)], x: f32) -> f32 {
    piecewise(points, x.clamp(points[0].0, points[points.len() - 1].0))
}

fn base_keys(model: &Model, regions: &[Region]) -> Result<BaseKeys> {
    let skeleton = model.skeleton.as_ref().context("el modelo base no tiene esqueleto")?;
    let at = |n: &str| skeleton.bone(n).map(|b| skeleton.rest_position(b)).with_context(|| format!("el modelo base no tiene {n}"));
    let head = at("head")?;
    let (left_arm, right_arm) = (at("leftUpperArm")?, at("rightUpperArm")?);
    let hips = at("hips")?;
    let left_leg = at("leftUpperLeg")?;
    let chest = at("chest").or_else(|_| at("spine"))?;
    let mut top = f32::MIN;
    let mut feet = f32::MAX;
    let mut chin = f32::MAX;
    let mut torso: Vec<Vec3> = Vec::new();
    let mut arm_radius: Vec<f32> = Vec::new();
    for layer in &model.layers {
        for p in &layer.primitives {
            for (v, (j, w)) in p.vertices.iter().zip(p.joints.iter().zip(&p.weights)) {
                let q = Vec3::from(v.position);
                top = top.max(q.y);
                feet = feet.min(q.y);
                let k = (0..4).max_by(|a, b| w[*a].total_cmp(&w[*b])).unwrap_or(0);
                match regions[j[k] as usize] {
                    // La barbilla: lo más bajo de la cabeza por delante y en el centro.
                    Region::Head if q.x.abs() < 0.02 && q.z > head.z => chin = chin.min(q.y),
                    Region::Torso => torso.push(q),
                    Region::Arm(_) if (q.x.abs() - left_arm.x.abs()).abs() < 0.08 => {
                        arm_radius.push(((q.y - left_arm.y).powi(2) + (q.z - left_arm.z).powi(2)).sqrt())
                    }
                    _ => {}
                }
            }
        }
    }
    anyhow::ensure!(chin < f32::MAX, "no se encuentra la barbilla del modelo base");
    // Ancho del tronco en una altura: entre lo más a la izquierda y lo más a la derecha.
    let width_at = |y: f32| {
        let row: Vec<f32> = torso.iter().filter(|q| (q.y - y).abs() < 0.01).map(|q| q.x).collect();
        row.iter().copied().fold(f32::MIN, f32::max) - row.iter().copied().fold(f32::MAX, f32::min)
    };
    // Cintura: lo más estrecho entre la cadera y el pecho.
    let waist = (0..40)
        .map(|i| hips.y + (chest.y - hips.y) * i as f32 / 40.0)
        .filter(|y| width_at(*y) > 0.0)
        .min_by(|a, b| width_at(*a).total_cmp(&width_at(*b)))
        .unwrap_or((hips.y + chest.y) * 0.5);
    arm_radius.sort_by(f32::total_cmp);
    let radius = arm_radius.get(arm_radius.len() / 2).copied().unwrap_or(0.04);
    Ok(BaseKeys {
        top,
        chin,
        shoulders: (left_arm.y + right_arm.y) * 0.5,
        waist,
        feet,
        // Como en la imagen: de un hombro (con el grosor del brazo) al otro.
        shoulder_width: (left_arm.x - right_arm.x).abs() + 2.0 * radius,
        waist_width: width_at(waist),
        hip: left_leg.y,
    })
}

pub struct Fit {
    /// Metros por píxel de la imagen.
    pub scale: f32,
    pub head: f32,
    pub shoulders: f32,
    pub waist: f32,
}

/// Ajusta el modelo base (con esqueleto humanoide) a los puntos clave de la imagen.
pub fn fit(model: &mut Model, k: &Keypoints) -> Result<Fit> {
    let skeleton = model.skeleton.as_ref().context("el modelo base no tiene esqueleto")?;
    let regions = regions(skeleton);
    let base = base_keys(model, &regions)?;
    let scale = (base.top - base.feet) / (k.feet - k.top).max(1) as f32;
    let target = |row: u32| base.feet + (k.feet as f32 - row as f32) * scale;
    let (t_chin, t_top) = (target(k.chin), target(k.top));
    // Cuello y tronco por tramos; la cabeza aparte.
    let vertical = [(base.feet, base.feet), (base.waist, target(k.waist)), (base.shoulders, target(k.shoulders)), (base.chin, t_chin)];
    let head = ((t_top - t_chin) / (base.top - base.chin)).clamp(0.7, 1.8);
    let ratio = |image: f32, base: f32| if image > 0.0 { (image * scale / base).clamp(0.7, 1.5) } else { 1.0 };
    let s_shoulders = ratio(k.shoulder_width, base.shoulder_width);
    let s_waist = ratio(k.waist_width, base.waist_width);
    let s_hips = (s_waist + 1.0) * 0.5;
    let widths = [(base.hip, s_hips), (base.waist, s_waist), (base.shoulders, s_shoulders), (base.chin, 1.0)];
    // Los brazos crecen con el tronco (de los pies a los hombros).
    let arm_length = (target(k.shoulders) - base.feet) / (base.shoulders - base.feet);
    let shoulder_of = |side: f32| {
        let name = if side > 0.0 { "leftUpperArm" } else { "rightUpperArm" };
        skeleton.bone(name).map(|b| skeleton.rest_position(b)).unwrap_or(Vec3::ZERO)
    };
    let leg_x = skeleton.bone("leftUpperLeg").map(|b| skeleton.rest_position(b).x.abs()).unwrap_or(0.08);
    let head_z = skeleton.bone("head").map(|b| skeleton.rest_position(b).z).unwrap_or(0.0);
    let (left_shoulder, right_shoulder) = (shoulder_of(1.0), shoulder_of(-1.0));
    let moved_shoulder = |s: Vec3| Vec3::new(s.x + s.x.signum() * (s_shoulders - 1.0) * s.x.abs(), piecewise(&vertical, s.y), s.z);

    let place = |q: Vec3, region: Region| -> Vec3 {
        match region {
            Region::Head => Vec3::new(q.x * head, t_chin + (q.y - base.chin) * head, head_z + (q.z - head_z) * head),
            Region::Neck => Vec3::new(q.x, piecewise(&vertical, q.y), q.z),
            Region::Torso => Vec3::new(q.x * clamped(&widths, q.y), piecewise(&vertical, q.y), q.z),
            Region::Arm(side) => {
                let joint = if side > 0.0 { left_shoulder } else { right_shoulder };
                moved_shoulder(joint) + (q - joint) * arm_length
            }
            Region::Leg(side) => Vec3::new(q.x + side * (s_hips - 1.0) * leg_x, piecewise(&vertical, q.y), q.z),
        }
    };

    // Vértices: por la región de su hueso con más peso.
    for layer in &mut model.layers {
        for p in &mut layer.primitives {
            for i in 0..p.vertices.len() {
                let (j, w) = (p.joints[i], p.weights[i]);
                let k = (0..4).max_by(|a, b| w[*a].total_cmp(&w[*b])).unwrap_or(0);
                let q = Vec3::from(p.vertices[i].position);
                p.vertices[i].position = place(q, regions[j[k] as usize]).to_array();
                for m in &mut p.morphs {
                    // Las formas de la cara se escalan con la cabeza.
                    if regions[j[k] as usize] == Region::Head {
                        m.deltas[i] = (Vec3::from(m.deltas[i]) * head).to_array();
                    }
                }
            }
        }
    }
    // Huesos: la misma regla para su posición.
    let skeleton = model.skeleton.as_mut().unwrap();
    for (b, bone) in skeleton.bones.iter_mut().enumerate() {
        let (s, r, t) = bone.rest_world.to_scale_rotation_translation();
        bone.rest_world = Mat4::from_scale_rotation_translation(s, r, place(t, regions[b]));
    }
    Ok(Fit { scale, head, shoulders: s_shoulders, waist: s_waist })
}

/// La imagen sin huecos: lo que no es `keep` toma el color conservado más cercano (por capas,
/// desde el borde) y todo queda opaco.
fn fill(image: &RgbaImage, keep: &[bool]) -> RgbaImage {
    let (w, h) = (image.width() as i32, image.height() as i32);
    let mut out = image.clone();
    // Sin el borde de lo conservado: en las ilustraciones es la línea del dibujo, y extenderla
    // pinta de negro lo que el cuerpo 3D tenga fuera de la silueta.
    let mut known = keep.to_vec();
    let margin = ((w.max(h) as f32) * 0.004).ceil().max(2.0) as usize;
    for _ in 0..margin {
        let previous = known.clone();
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if previous[i] {
                    let edge = [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|(dx, dy)| {
                        let (nx, ny) = (x + dx, y + dy);
                        nx < 0 || ny < 0 || nx >= w || ny >= h || !previous[(ny * w + nx) as usize]
                    });
                    if edge {
                        known[i] = false;
                    }
                }
            }
        }
    }
    // Lo de dentro de la silueta que se ha quitado con el borde vuelve a ser el de la imagen.
    let interior = known.clone();
    for _ in 0..(w.max(h)) {
        let previous = out.clone();
        let was = known.clone();
        let mut changed = false;
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if was[i] {
                    continue;
                }
                let mut sum = [0u32; 3];
                let mut n = 0;
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < w && ny < h && was[(ny * w + nx) as usize] {
                        let p = previous.get_pixel(nx as u32, ny as u32);
                        for c in 0..3 {
                            sum[c] += p[c] as u32;
                        }
                        n += 1;
                    }
                }
                if n > 0 {
                    out.put_pixel(x as u32, y as u32, image::Rgba([(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8, 255]));
                    known[i] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    // La silueta conserva su dibujo (también su línea); solo lo de fuera usa el relleno.
    for (i, p) in out.pixels_mut().enumerate() {
        if keep[i] && !interior[i] {
            let original = image.get_pixel((i % w as usize) as u32, (i / w as usize) as u32);
            p[0] = original[0];
            p[1] = original[1];
            p[2] = original[2];
        }
        p[3] = 255;
    }
    out
}

/// Cómo se pasa del modelo a la imagen (y al revés) en la proyección frontal, y la pose con la
/// que se proyectó (brazos como en la imagen).
pub struct Projection {
    pub scale: f32,
    pub centre_x: f32,
    pub feet_row: f32,
    pub feet_y: f32,
    pub width: u32,
    pub height: u32,
    pub local: Vec<Quat>,
}

impl Projection {
    /// Píxel (columna, fila) de un punto del modelo.
    pub fn pixel(&self, p: Vec3) -> (f32, f32) {
        (self.centre_x + p.x / self.scale, self.feet_row - (p.y - self.feet_y) / self.scale)
    }

    /// Punto del modelo (x, y) de un píxel.
    pub fn point(&self, column: f32, row: f32) -> (f32, f32) {
        ((column - self.centre_x) * self.scale, self.feet_y + (self.feet_row - row) * self.scale)
    }

    pub fn uv(&self, p: Vec3) -> [f32; 2] {
        let (c, r) = self.pixel(p);
        [c / self.width as f32, r / self.height as f32]
    }
}

/// Pinta la imagen sobre el modelo ajustado por proyección frontal. Antes elige, para cada brazo,
/// el ángulo con el que sus vértices caen sobre más piel o ropa de la imagen (no pelo ni fondo).
/// El cuerpo se pinta con el pelo de debajo de la barbilla borrado (lo pone el paso del pelo).
pub fn project(model: &mut Model, image: &RgbaImage, analysis: &crate::analyze::Analysis, fit: &Fit) -> Result<Projection> {
    use crate::analyze::Part;
    let k = &analysis.keypoints;
    let skeleton = model.skeleton.as_ref().context("el modelo no tiene esqueleto")?;
    let regions = regions(skeleton);
    let feet_y = model.layers.iter().flat_map(|l| l.primitives.iter()).flat_map(|p| p.vertices.iter()).map(|v| v.position[1]).fold(f32::MAX, f32::min);
    let mut projection = Projection {
        scale: fit.scale,
        centre_x: k.centre_x,
        feet_row: k.feet as f32,
        feet_y,
        width: image.width(),
        height: image.height(),
        local: vec![Quat::IDENTITY; skeleton.bones.len()],
    };
    let label_at = |c: f32, r: f32| -> Part {
        if c < 0.0 || r < 0.0 || c >= image.width() as f32 || r >= image.height() as f32 {
            return Part::Fondo;
        }
        analysis.labels[r as usize * image.width() as usize + c as usize]
    };
    // Brazos: ángulos bajo la horizontal, de casi colgando a casi en cruz.
    for side in [1.0f32, -1.0] {
        let (upper, lower, hand) = if side > 0.0 { ("leftUpperArm", "leftLowerArm", "leftHand") } else { ("rightUpperArm", "rightLowerArm", "rightHand") };
        let (Some(u), Some(l), Some(hd)) = (skeleton.bone(upper), skeleton.bone(lower), skeleton.bone(hand)) else { continue };
        let mut best = (f32::MIN, projection.local.clone());
        for degrees in [80.0f32, 65.0, 50.0, 35.0, 20.0, 5.0] {
            let angle = degrees.to_radians();
            let direction = Vec3::new(angle.cos() * side, -angle.sin(), 0.0);
            let mut local = projection.local.clone();
            crate::pose::aim(skeleton, &mut local, u, l, direction);
            crate::pose::aim(skeleton, &mut local, l, hd, direction);
            let posed = crate::pose::skin_model(model, &local, &[]);
            let mut score = 0.0;
            let mut count: f32 = 0.0;
            for (layer, posed_layer) in model.layers.iter().zip(&posed) {
                for (p, posed_p) in layer.primitives.iter().zip(posed_layer) {
                    for (i, q) in posed_p.iter().enumerate() {
                        let (j, w) = (p.joints[i], p.weights[i]);
                        let d = (0..4).max_by(|a, b| w[*a].total_cmp(&w[*b])).unwrap_or(0);
                        if regions[j[d] as usize] != Region::Arm(side) {
                            continue;
                        }
                        let (c, r) = projection.pixel(Vec3::from(q.position));
                        score += match label_at(c, r) {
                            Part::Piel | Part::Ropa => 1.0,
                            Part::Pelo | Part::Calzado => 0.0,
                            Part::Fondo => -1.0,
                        };
                        count += 1.0;
                    }
                }
            }
            let score = score / count.max(1.0);
            if score > best.0 {
                best = (score, local);
            }
        }
        projection.local = best.1;
    }
    // Piernas: abiertas de 0° a 15°, como en la imagen.
    for side in [1.0f32, -1.0] {
        let (upper, lower, foot) = if side > 0.0 { ("leftUpperLeg", "leftLowerLeg", "leftFoot") } else { ("rightUpperLeg", "rightLowerLeg", "rightFoot") };
        let (Some(u), Some(l), Some(f)) = (skeleton.bone(upper), skeleton.bone(lower), skeleton.bone(foot)) else { continue };
        let mut best = (f32::MIN, projection.local.clone());
        for degrees in [0.0f32, 3.0, 6.0, 9.0, 12.0, 15.0] {
            let angle = degrees.to_radians();
            let direction = Vec3::new(angle.sin() * side, -angle.cos(), 0.0);
            let mut local = projection.local.clone();
            crate::pose::aim(skeleton, &mut local, u, l, direction);
            crate::pose::aim(skeleton, &mut local, l, f, direction);
            let posed = crate::pose::skin_model(model, &local, &[]);
            let (mut score, mut count) = (0.0f32, 0.0f32);
            for (layer, posed_layer) in model.layers.iter().zip(&posed) {
                for (p, posed_p) in layer.primitives.iter().zip(posed_layer) {
                    for (i, q) in posed_p.iter().enumerate() {
                        let (j, w) = (p.joints[i], p.weights[i]);
                        let d = (0..4).max_by(|a, b| w[*a].total_cmp(&w[*b])).unwrap_or(0);
                        if regions[j[d] as usize] != Region::Leg(side) {
                            continue;
                        }
                        let (c, r) = projection.pixel(Vec3::from(q.position));
                        score += match label_at(c, r) {
                            Part::Piel | Part::Ropa | Part::Calzado => 1.0,
                            Part::Pelo => 0.0,
                            Part::Fondo => -1.0,
                        };
                        count += 1.0;
                    }
                }
            }
            let score = score / count.max(1.0);
            if score > best.0 {
                best = (score, local);
            }
        }
        projection.local = best.1;
    }
    let posed = crate::pose::skin_model(model, &projection.local, &[]);
    for (layer, posed_layer) in model.layers.iter_mut().zip(&posed) {
        for (p, posed_p) in layer.primitives.iter_mut().zip(posed_layer) {
            for (v, q) in p.vertices.iter_mut().zip(posed_p) {
                v.uv = projection.uv(Vec3::from(q.position));
            }
            p.material = 0;
            p.morphs.iter_mut().for_each(|m| m.uv_deltas.clear());
        }
    }
    // Textura del cuerpo: sin el pelo de debajo de la barbilla; la del pelo, entera.
    let w = image.width() as usize;
    let body_keep: Vec<bool> = image
        .pixels()
        .enumerate()
        .map(|(i, p)| p[3] > 128 && !(analysis.labels[i] == Part::Pelo && (i / w) as u32 > k.chin))
        .collect();
    let all_keep: Vec<bool> = image.pixels().map(|p| p[3] > 128).collect();
    let body = fill(image, &body_keep);
    let hair = fill(image, &all_keep);
    model.textures = vec![
        Texture { width: body.width(), height: body.height(), rgba: body.into_raw() },
        Texture { width: hair.width(), height: hair.height(), rgba: hair.into_raw() },
    ];
    model.materials = vec![
        Material { name: "cuerpo".into(), base_color: [1.0; 4], texture: Some(0), alpha: AlphaMode::Opaque, double_sided: false },
        Material { name: "pelo".into(), base_color: [1.0; 4], texture: Some(1), alpha: AlphaMode::Opaque, double_sided: true },
    ];
    Ok(projection)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piecewise_interpolates_and_extends() {
        let points = [(0.0, 0.0), (1.0, 2.0), (2.0, 3.0)];
        assert!((piecewise(&points, 0.5) - 1.0).abs() < 1e-6);
        assert!((piecewise(&points, 1.5) - 2.5).abs() < 1e-6);
        // Fuera, con la pendiente del último tramo; `clamped`, sin salirse.
        assert!((piecewise(&points, 3.0) - 4.0).abs() < 1e-6);
        assert!((clamped(&points, 3.0) - 3.0).abs() < 1e-6);
    }

    #[test]
    fn fill_keeps_the_drawing_and_paints_outside_from_inside() {
        // Un cuadrado rojo con borde negro sobre fondo transparente.
        let mut image = RgbaImage::new(40, 40);
        for y in 10..30 {
            for x in 10..30 {
                let edge = x == 10 || x == 29 || y == 10 || y == 29;
                image.put_pixel(x, y, image::Rgba(if edge { [0, 0, 0, 255] } else { [200, 0, 0, 255] }));
            }
        }
        let keep: Vec<bool> = image.pixels().map(|p| p[3] > 128).collect();
        let out = fill(&image, &keep);
        // El dibujo sigue igual (también su borde negro)…
        assert_eq!(out.get_pixel(10, 15)[0], 0);
        assert_eq!(out.get_pixel(15, 15)[0], 200);
        // …y fuera no se extiende el negro del borde, sino el rojo de dentro.
        assert!(out.get_pixel(5, 15)[0] > 150, "{:?}", out.get_pixel(5, 15));
    }
}

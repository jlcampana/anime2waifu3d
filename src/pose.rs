//! Poses: rotaciones locales de los huesos, cinemática directa y deformación de las mallas
//! (skinning lineal en CPU).
//!
//! Las direcciones de las poses predefinidas son del mundo, con el personaje mirando hacia +Z:
//! su izquierda está en +X.

use crate::model::{Model, Primitive, Skeleton, Vertex};
use glam::{Mat3, Mat4, Quat, Vec3};

/// Matrices de cada hueso en el mundo con las rotaciones locales `local` (una por hueso).
pub fn world_matrices(skeleton: &Skeleton, local: &[Quat]) -> Vec<Mat4> {
    let n = skeleton.bones.len();
    let mut world: Vec<Option<Mat4>> = vec![None; n];
    fn solve(skeleton: &Skeleton, local: &[Quat], world: &mut [Option<Mat4>], b: usize) -> Mat4 {
        if let Some(m) = world[b] {
            return m;
        }
        let bone = &skeleton.bones[b];
        let m = match bone.parent {
            Some(p) => {
                let parent_rest = skeleton.bones[p].rest_world;
                let rest_local = parent_rest.inverse() * bone.rest_world;
                solve(skeleton, local, world, p) * rest_local * Mat4::from_quat(local[b])
            }
            None => bone.rest_world * Mat4::from_quat(local[b]),
        };
        world[b] = Some(m);
        m
    }
    (0..n).map(|b| solve(skeleton, local, &mut world, b)).collect()
}

/// Matrices de skinning: llevan un vértice en reposo (en el mundo) a su sitio en la pose.
pub fn skin_matrices(skeleton: &Skeleton, local: &[Quat]) -> Vec<Mat4> {
    world_matrices(skeleton, local)
        .iter()
        .zip(&skeleton.bones)
        .map(|(posed, bone)| *posed * bone.rest_world.inverse())
        .collect()
}

pub fn skin_primitive(primitive: &Primitive, skin: &[Mat4]) -> Vec<Vertex> {
    if primitive.joints.is_empty() {
        return primitive.vertices.clone();
    }
    primitive
        .vertices
        .iter()
        .zip(primitive.joints.iter().zip(&primitive.weights))
        .map(|(v, (j, w))| {
            let mut m = Mat4::ZERO;
            for k in 0..4 {
                if w[k] > 0.0 {
                    m += skin[j[k] as usize] * w[k];
                }
            }
            let p = m.transform_point3(Vec3::from(v.position));
            let n = (Mat3::from_mat4(m) * Vec3::from(v.normal)).normalize_or_zero();
            Vertex { position: p.to_array(), normal: n.to_array(), uv: v.uv }
        })
        .collect()
}

/// Vértices de cada primitiva de cada capa en la pose.
pub fn skin_model(model: &Model, local: &[Quat]) -> Vec<Vec<Vec<Vertex>>> {
    let Some(skeleton) = &model.skeleton else {
        return model.layers.iter().map(|l| l.primitives.iter().map(|p| p.vertices.clone()).collect()).collect();
    };
    let skin = skin_matrices(skeleton, local);
    model.layers.iter().map(|l| l.primitives.iter().map(|p| skin_primitive(p, &skin)).collect()).collect()
}

/// Gira el hueso `bone` para que el segmento hacia su hijo `child` apunte en la dirección `dir`
/// del mundo, con las rotaciones de sus antecesores ya puestas en `local`.
pub fn aim(skeleton: &Skeleton, local: &mut [Quat], bone: usize, child: usize, dir: Vec3) {
    let world = world_matrices(skeleton, local);
    let b = &skeleton.bones[bone];
    // Rotación del hueso antes de su propia rotación local.
    let before = match b.parent {
        Some(p) => world[p] * (skeleton.bones[p].rest_world.inverse() * b.rest_world),
        None => b.rest_world,
    };
    let before_rot = Quat::from_mat3(&Mat3::from_mat4(before)).normalize();
    // Segmento hacia el hijo, en el espacio local del hueso.
    let segment = (b.rest_world.inverse() * skeleton.bones[child].rest_world).w_axis.truncate();
    let target = before_rot.inverse() * dir.normalize();
    local[bone] = Quat::from_rotation_arc(segment.normalize(), target.normalize());
}

/// Poses predefinidas por nombre, sobre los huesos humanoides.
pub const PRESETS: &[&str] = &["reposo", "a-pose", "brazos-arriba", "saludo", "patada", "jarras"];

pub fn preset(skeleton: &Skeleton, name: &str) -> Option<Vec<Quat>> {
    let mut local = vec![Quat::IDENTITY; skeleton.bones.len()];
    let mut aim_h = |local: &mut Vec<Quat>, bone: &str, child: &str, dir: Vec3| {
        if let (Some(b), Some(c)) = (skeleton.bone(bone), skeleton.bone(child)) {
            aim(skeleton, local, b, c, dir);
        }
    };
    match name {
        "reposo" => {}
        "a-pose" => {
            aim_h(&mut local, "leftUpperArm", "leftLowerArm", Vec3::new(0.7, -0.7, 0.0));
            aim_h(&mut local, "leftLowerArm", "leftHand", Vec3::new(0.7, -0.7, 0.0));
            aim_h(&mut local, "rightUpperArm", "rightLowerArm", Vec3::new(-0.7, -0.7, 0.0));
            aim_h(&mut local, "rightLowerArm", "rightHand", Vec3::new(-0.7, -0.7, 0.0));
        }
        "brazos-arriba" => {
            aim_h(&mut local, "leftUpperArm", "leftLowerArm", Vec3::new(0.3, 1.0, 0.0));
            aim_h(&mut local, "leftLowerArm", "leftHand", Vec3::new(0.15, 1.0, 0.0));
            aim_h(&mut local, "rightUpperArm", "rightLowerArm", Vec3::new(-0.3, 1.0, 0.0));
            aim_h(&mut local, "rightLowerArm", "rightHand", Vec3::new(-0.15, 1.0, 0.0));
        }
        "saludo" => {
            aim_h(&mut local, "rightUpperArm", "rightLowerArm", Vec3::new(-1.0, 0.35, 0.1));
            aim_h(&mut local, "rightLowerArm", "rightHand", Vec3::new(-0.15, 1.0, 0.15));
            aim_h(&mut local, "leftUpperArm", "leftLowerArm", Vec3::new(0.25, -1.0, 0.0));
            aim_h(&mut local, "leftLowerArm", "leftHand", Vec3::new(0.2, -1.0, 0.1));
        }
        "patada" => {
            aim_h(&mut local, "leftUpperLeg", "leftLowerLeg", Vec3::new(0.1, -0.15, 1.0));
            aim_h(&mut local, "leftLowerLeg", "leftFoot", Vec3::new(0.1, 0.05, 1.0));
            aim_h(&mut local, "leftUpperArm", "leftLowerArm", Vec3::new(0.8, -0.3, -0.4));
            aim_h(&mut local, "rightUpperArm", "rightLowerArm", Vec3::new(-0.8, -0.3, 0.4));
        }
        "jarras" => {
            aim_h(&mut local, "leftUpperArm", "leftLowerArm", Vec3::new(0.6, -0.8, -0.15));
            aim_h(&mut local, "leftLowerArm", "leftHand", Vec3::new(-0.6, -0.5, 0.2));
            aim_h(&mut local, "rightUpperArm", "rightLowerArm", Vec3::new(-0.6, -0.8, -0.15));
            aim_h(&mut local, "rightLowerArm", "rightHand", Vec3::new(0.6, -0.5, 0.2));
        }
        _ => return None,
    }
    Some(local)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Bone;
    use std::collections::HashMap;

    fn arm() -> Skeleton {
        let at = |x: f32| Mat4::from_translation(Vec3::new(x, 1.0, 0.0));
        Skeleton {
            bones: vec![
                Bone { name: "shoulder".into(), parent: None, rest_world: at(0.0) },
                Bone { name: "elbow".into(), parent: Some(0), rest_world: at(0.3) },
                Bone { name: "hand".into(), parent: Some(1), rest_world: at(0.55) },
            ],
            humanoid: HashMap::new(),
        }
    }

    #[test]
    fn the_rest_pose_leaves_vertices_in_place() {
        let s = arm();
        for m in skin_matrices(&s, &[Quat::IDENTITY; 3]) {
            assert!(m.abs_diff_eq(Mat4::IDENTITY, 1e-6));
        }
    }

    #[test]
    fn aiming_points_the_segment_at_the_direction() {
        let s = arm();
        let mut local = vec![Quat::IDENTITY; 3];
        aim(&s, &mut local, 0, 1, Vec3::new(0.0, -1.0, 0.0));
        aim(&s, &mut local, 1, 2, Vec3::new(0.0, 0.0, 1.0));
        let w = world_matrices(&s, &local);
        let elbow = w[1].w_axis.truncate();
        let hand = w[2].w_axis.truncate();
        assert!(elbow.distance(Vec3::new(0.0, 0.7, 0.0)) < 1e-5, "{elbow}");
        assert!(hand.distance(Vec3::new(0.0, 0.7, 0.25)) < 1e-5, "{hand}");
    }
}

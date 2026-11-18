//! Juegos de iluminación del visor. Las direcciones apuntan hacia la luz, en el espacio del mundo
//! (+Y arriba, el personaje mira hacia +Z). Los colores son lineales.

use glam::Vec3;

#[derive(Clone, Copy)]
pub struct LightRig {
    pub name: &'static str,
    pub key_dir: Vec3,
    pub key_color: Vec3,
    pub fill_dir: Vec3,
    pub fill_color: Vec3,
    /// Luz de contorno: ilumina los bordes vistos de refilón.
    pub rim_color: Vec3,
    /// Luz ambiente de cielo (arriba) y de rebote del suelo (abajo).
    pub sky: Vec3,
    pub ground: Vec3,
    /// Color de fondo, en sRGB.
    pub background: [f64; 3],
    /// Sombreado en dos tonos, como en el anime.
    pub toon: bool,
}

pub const PRESETS: &[LightRig] = &[
    LightRig {
        name: "Luz natural (día)",
        key_dir: Vec3::new(-0.45, 0.75, 0.5),
        key_color: Vec3::new(1.0, 0.95, 0.86),
        fill_dir: Vec3::new(0.6, 0.2, 0.4),
        fill_color: Vec3::new(0.12, 0.14, 0.18),
        rim_color: Vec3::new(0.15, 0.17, 0.2),
        sky: Vec3::new(0.32, 0.37, 0.45),
        ground: Vec3::new(0.2, 0.18, 0.15),
        background: [0.72, 0.8, 0.88],
        toon: false,
    },
    LightRig {
        name: "Nublado",
        key_dir: Vec3::new(0.0, 1.0, 0.3),
        key_color: Vec3::new(0.45, 0.47, 0.5),
        fill_dir: Vec3::new(0.0, 0.3, 1.0),
        fill_color: Vec3::new(0.2, 0.21, 0.22),
        rim_color: Vec3::new(0.05, 0.05, 0.05),
        sky: Vec3::new(0.55, 0.57, 0.6),
        ground: Vec3::new(0.3, 0.3, 0.3),
        background: [0.78, 0.8, 0.82],
        toon: false,
    },
    LightRig {
        name: "Estudio (3 puntos)",
        key_dir: Vec3::new(-0.6, 0.5, 0.65),
        key_color: Vec3::new(0.7, 0.7, 0.7),
        fill_dir: Vec3::new(0.7, 0.1, 0.7),
        fill_color: Vec3::new(0.18, 0.18, 0.2),
        rim_color: Vec3::new(0.35, 0.35, 0.4),
        sky: Vec3::new(0.12, 0.12, 0.13),
        ground: Vec3::new(0.08, 0.08, 0.08),
        background: [0.22, 0.22, 0.24],
        toon: false,
    },
    LightRig {
        name: "Atardecer",
        key_dir: Vec3::new(-0.85, 0.2, 0.45),
        key_color: Vec3::new(1.0, 0.6, 0.32),
        fill_dir: Vec3::new(0.7, 0.4, 0.3),
        fill_color: Vec3::new(0.12, 0.1, 0.22),
        rim_color: Vec3::new(0.5, 0.3, 0.2),
        sky: Vec3::new(0.25, 0.2, 0.3),
        ground: Vec3::new(0.2, 0.12, 0.08),
        background: [0.95, 0.62, 0.45],
        toon: false,
    },
    LightRig {
        name: "Noche",
        key_dir: Vec3::new(0.3, 0.8, 0.4),
        key_color: Vec3::new(0.25, 0.32, 0.55),
        fill_dir: Vec3::new(-0.5, 0.0, 0.8),
        fill_color: Vec3::new(0.05, 0.05, 0.1),
        rim_color: Vec3::new(0.3, 0.4, 0.7),
        sky: Vec3::new(0.06, 0.07, 0.12),
        ground: Vec3::new(0.02, 0.02, 0.03),
        background: [0.05, 0.06, 0.12],
        toon: false,
    },
    LightRig {
        name: "Anime (toon)",
        key_dir: Vec3::new(-0.4, 0.7, 0.6),
        key_color: Vec3::new(0.45, 0.43, 0.42),
        fill_dir: Vec3::new(0.6, 0.2, 0.4),
        fill_color: Vec3::new(0.0, 0.0, 0.0),
        rim_color: Vec3::new(0.2, 0.2, 0.22),
        sky: Vec3::new(0.62, 0.6, 0.64),
        ground: Vec3::new(0.55, 0.52, 0.56),
        background: [0.94, 0.94, 0.96],
        toon: true,
    },
];

//! Física secundaria. Por ahora, el muelle de pecho de DOA3, medido en doa.player
//! (`docs/doa3-internals.md`, "Chest spring"), a 60 pasos por segundo:
//!
//! ```text
//! target = r/2 - r/2 * chest.up.y
//! v = damping * v - stiffness * (x - target) - drive_gain * drive
//! x = clamp(x + v, -1, 1)
//! ```
//!
//! con los valores por defecto de DOA3 (rigidez 0.128, amortiguación 0.85, impulso 20 por 0.5 de
//! pie, reposo 0.25). `drive` es lo que sube en un paso un punto fijo al pecho, en metros.

#[derive(Clone, Copy, Debug)]
pub struct SpringParams {
    pub stiffness: f32,
    pub damping: f32,
    pub drive_gain: f32,
    pub rest: f32,
}

/// Juegos de valores: DOA3/DOAX ("normal") y DOA2U ("suave", la mitad de su impulso de 30).
pub const PRESETS: &[(&str, SpringParams)] = &[
    ("normal (DOA3)", SpringParams { stiffness: 0.128, damping: 0.85, drive_gain: 10.0, rest: 0.25 }),
    ("suave (DOA2U)", SpringParams { stiffness: 0.075, damping: 0.9, drive_gain: 15.0, rest: 0.3 }),
    ("exagerado", SpringParams { stiffness: 0.09, damping: 0.9, drive_gain: 22.0, rest: 0.25 }),
];

#[derive(Default, Clone, Copy, Debug)]
pub struct BustSpring {
    pub x: f32,
    v: f32,
    previous_y: Option<f32>,
}

impl BustSpring {
    /// Un paso de 1/60 s. `up_y` es la componente vertical del eje "arriba" del pecho y
    /// `point_y` la altura de un punto fijo al pecho.
    pub fn step(&mut self, params: &SpringParams, up_y: f32, point_y: f32) {
        let drive = self.previous_y.map_or(0.0, |p| point_y - p);
        self.previous_y = Some(point_y);
        let half = params.rest * 0.5;
        let target = half - half * up_y;
        self.v = params.damping * self.v - params.stiffness * (self.x - target) - params.drive_gain * drive;
        self.x = (self.x + self.v).clamp(-1.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_still_stays_at_rest() {
        let mut s = BustSpring::default();
        for _ in 0..100 {
            s.step(&PRESETS[0].1, 1.0, 1.2);
        }
        assert!(s.x.abs() < 1e-6, "{}", s.x);
    }

    #[test]
    fn a_jump_makes_it_lag_and_settle() {
        let mut s = BustSpring::default();
        // Sube 1 cm por paso durante 10 pasos: el pecho se queda atrás (x negativo)…
        for i in 0..10 {
            s.step(&PRESETS[0].1, 1.0, 1.2 + i as f32 * 0.01);
        }
        assert!(s.x < -0.05, "{}", s.x);
        // …y al parar vuelve al reposo.
        for _ in 0..400 {
            s.step(&PRESETS[0].1, 1.0, 1.29);
        }
        assert!(s.x.abs() < 1e-3, "{}", s.x);
    }
}

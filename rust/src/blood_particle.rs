//! Blood-cell particles: red blood cells (RBCs), white blood cells (WBCs),
//! and platelets. Each type has a low-poly shape, characteristic colour,
//! radial distribution, and size, while all advect along the vessel using the
//! local velocity from the `FlowField`.

use godot::builtin::{Basis, EulerOrder, Vector3};
use rand::Rng;
use std::f32::consts::TAU;

/// The three major visible blood constituents represented as particles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellType {
    /// Erythrocytes: the most numerous cells, flattened discs that tend to
    /// migrate toward the faster centre of the vessel.
    Rbc,
    /// Leukocytes: much rarer, roughly spherical cells that marginate near
    /// the vessel wall.
    Wbc,
    /// Thrombocytes: small cell fragments that also marginate and are far
    /// smaller than RBCs.
    Platelet,
}

impl CellType {
    /// Fractional count mix in the particle system. The absolute numbers are
    /// scaled so that WBCs and platelets remain visually identifiable while
    /// still reflecting blood's strong RBC dominance.
    pub fn target_counts() -> [(CellType, usize); 3] {
        [
            (CellType::Rbc, 270),
            (CellType::Wbc, 12),
            (CellType::Platelet, 38),
        ]
    }
}

/// One simulated blood-cell particle.
#[derive(Debug, Clone, Copy)]
pub struct BloodParticle {
    pub cell_type: CellType,
    /// Normalized position along the vessel axis, in `[0, 1]`.
    pub t: f32,
    /// Radial offset as a fraction (`[0, 1)`) of the *local* lumen radius.
    /// Type-specific spawn distributions create the classic centre-biased
    /// RBC profile and the wall-biased WBC/platelet profile.
    pub radius_frac: f32,
    /// Angle around the vessel axis, in radians.
    pub angle: f32,
    /// Random 3D orientation so the low-poly cells do not all align.
    pub orientation: Basis,
}

impl BloodParticle {
    pub fn random(cell_type: CellType, rng: &mut impl Rng) -> Self {
        Self {
            cell_type,
            t: rng.gen_range(0.0..1.0),
            radius_frac: Self::random_radius_frac(cell_type, rng),
            angle: rng.gen_range(0.0..TAU),
            orientation: Self::random_orientation(rng),
        }
    }

    /// Respawns this particle at the inlet with a fresh radial/angular
    /// placement, preserving its cell type.
    pub fn respawn_at_inlet(&mut self, rng: &mut impl Rng) {
        self.t = 0.0;
        self.radius_frac = Self::random_radius_frac(self.cell_type, rng);
        self.angle = rng.gen_range(0.0..TAU);
        self.orientation = Self::random_orientation(rng);
    }

    fn random_radius_frac(cell_type: CellType, rng: &mut impl Rng) -> f32 {
        match cell_type {
            // Centre-biased distribution (Fahraeus-Lindqvist / axial
            // migration). The transform `1 - sqrt(u)` pushes more RBCs toward
            // the low-radius_frac core without ever reaching the exact wall.
            CellType::Rbc => {
                let u: f32 = rng.gen();
                0.05 + 0.70 * (1.0 - u.sqrt())
            }
            // Wall-biased (margination) distribution for WBCs and platelets.
            CellType::Wbc | CellType::Platelet => {
                let u: f32 = rng.gen();
                0.45 + 0.45 * u
            }
        }
    }

    fn random_orientation(rng: &mut impl Rng) -> Basis {
        Basis::from_euler(
            EulerOrder::XYZ,
            Vector3::new(
                rng.gen_range(0.0..TAU),
                rng.gen_range(0.0..TAU),
                rng.gen_range(0.0..TAU),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::thread_rng;

    #[test]
    fn target_counts_sum_and_reflect_blood_composition() {
        let counts = CellType::target_counts();
        let total: usize = counts.iter().map(|(_, c)| c).sum();
        assert_eq!(total, 270 + 12 + 38);

        let rbc = counts.iter().find(|(t, _)| *t == CellType::Rbc).unwrap().1;
        let wbc = counts.iter().find(|(t, _)| *t == CellType::Wbc).unwrap().1;
        let plt = counts
            .iter()
            .find(|(t, _)| *t == CellType::Platelet)
            .unwrap()
            .1;

        // RBCs dominate, platelets outnumber WBCs.
        assert!(rbc > plt && plt > wbc);
    }

    #[test]
    fn rbc_spawn_is_centre_biased() {
        let mut rng = thread_rng();
        let mut sum = 0.0f32;
        let n = 2000;
        for _ in 0..n {
            let p = BloodParticle::random(CellType::Rbc, &mut rng);
            assert!(p.radius_frac >= 0.05 && p.radius_frac < 0.75);
            sum += p.radius_frac;
        }
        let mean = sum / n as f32;
        // Centre-biased distribution should sit clearly below the midpoint of
        // the 0.05..0.75 interval.
        assert!(mean < 0.35, "RBC mean radius_frac was {}", mean);
    }

    #[test]
    fn wbc_and_platelet_spawn_are_wall_biased() {
        let mut rng = thread_rng();
        for cell_type in [CellType::Wbc, CellType::Platelet] {
            let mut sum = 0.0f32;
            let n = 2000;
            for _ in 0..n {
                let p = BloodParticle::random(cell_type, &mut rng);
                assert!(p.radius_frac >= 0.45 && p.radius_frac < 0.90);
                sum += p.radius_frac;
            }
            let mean = sum / n as f32;
            // Wall-biased distribution should sit clearly above the midpoint of
            // the 0.45..0.90 interval.
            assert!(mean > 0.60, "{:?} mean radius_frac was {}", cell_type, mean);
        }
    }
}

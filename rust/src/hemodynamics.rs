//! Simplified hemodynamic model for the artery / plaque visualization.
//!
//! # Physics summary
//!
//! 1. **Continuity** (mass conservation of an incompressible fluid): the
//!    volumetric flow rate `Q` is constant along the vessel, so the local
//!    mean velocity is `v(x) = Q / A(x)`, where `A(x) = pi * r(x)^2` is the
//!    local lumen (open channel) cross-sectional area. As plaque narrows the
//!    middle of the artery, `A` drops there and `v` must rise to match - this
//!    is why the blood particles visibly speed up over the stenosis.
//!
//! 2. **Bernoulli's principle** (conservation of energy along a streamline)
//!    ties local pressure to local velocity. For a horizontal vessel
//!    (no elevation change) with a viscous-loss term `loss(a -> b)` added to
//!    account for real (non-ideal) blood:
//!
//!    ```text
//!    P(a) + 1/2 * rho * v(a)^2 = P(b) + 1/2 * rho * v(b)^2 + loss(a -> b)
//!    ```
//!
//!    Solved for the pressure at any point `t` relative to the *outlet*
//!    (see below for why the outlet is the reference), this gives a pressure
//!    profile that dips at the narrow throat exactly where the velocity
//!    peaks - the classic Venturi effect seen in real stenosed arteries
//!    (it's also the reason stenoses can produce audible turbulence/bruits).
//!
//! 3. **Hagen-Poiseuille resistance** supplies the `loss` term above. Blood
//!    is viscous, so pushing a constant flow `Q` through a narrowed segment
//!    costs extra driving pressure. The resistance of an infinitesimal slice
//!    of the vessel is `dR = 8 * eta / (pi * r(x)^4) * dx`; integrating this
//!    along the vessel gives the total resistance. This term is *not*
//!    Bernoulli, but it is essential for scientific consistency: a
//!    frictionless (inviscid) fluid flowing through a vessel that returns to
//!    its original radius would show *zero* net pressure change, yet real
//!    blood does - which is precisely the mechanism behind the clinically
//!    observed rise in blood pressure caused by arterial plaque.
//!
//! # Boundary condition
//!
//! We fix the **outlet** pressure to the normal mean arterial pressure
//! (downstream tissue/arterioles autoregulate to require this perfusion
//! pressure) and assume the heart maintains a constant flow `Q` (autoregulated
//! cardiac output). Solving backwards from the outlet then tells us how much
//! *extra* pressure the heart must generate at the inlet to overcome the
//! added resistance of the plaque - this is the rising "blood pressure"
//! readout. At the same time, the Bernoulli term produces a local pressure
//! *dip* right at the throat, independent of (and superimposed on) that
//! overall rise.

use std::f64::consts::PI;

/// Blood density (kg/m^3).
pub const BLOOD_DENSITY_KG_M3: f64 = 1060.0;
/// Blood dynamic viscosity (Pa*s), typical whole blood at high shear rate.
pub const BLOOD_VISCOSITY_PA_S: f64 = 0.0035;
/// Normal mean arterial / perfusion pressure (Pa), ~100 mmHg.
pub const MEAN_ARTERIAL_PRESSURE_PA: f64 = 13_332.2;
/// Conversion factor from Pascal to mmHg.
pub const PA_PER_MMHG: f64 = 133.322;

/// Healthy (unobstructed) lumen radius (m) of a proximal epicardial coronary
/// artery - the vessels that supply the heart muscle itself and are the
/// classic site of atherosclerotic plaque buildup (coronary artery disease).
/// Angiographic studies put proximal LAD/RCA/circumflex diameters at roughly
/// 3.5-4.5 mm, i.e. ~1.75-2.25 mm radius; 2 mm (4 mm diameter) is a
/// representative average, close to the proximal LAD ("widow maker" vessel).
pub const HEALTHY_LUMEN_RADIUS_M: f64 = 0.002;
/// Length of the modeled vessel segment (m) - comparable to a focal coronary
/// lesion plus healthy shoulders on either side.
pub const VESSEL_SEGMENT_LENGTH_M: f64 = 0.04;
/// Baseline mean blood velocity (m/s) in the healthy vessel. Resting
/// intracoronary Doppler studies report average peak velocities of roughly
/// 15-20 cm/s in normal proximal coronary arteries.
pub const BASELINE_MEAN_VELOCITY_M_S: f64 = 0.18;

/// Maximum fractional radius reduction at 100% plaque (a residual lumen
/// always remains). A 75% radius reduction is a 16x area reduction, pushing
/// peak throat velocity to roughly 16x baseline - in the same ballpark as
/// clinical Doppler-ultrasound velocity ratios used to grade critical
/// (>=70-80% area reduction) real-world coronary stenoses.
pub const MAX_RADIUS_REDUCTION_FRACTION: f64 = 0.75;
/// Normalized (0..1) start/end of the constricted zone; plaque only builds up
/// in this *middle* segment, leaving both ends at full healthy radius.
pub const STENOSIS_START_T: f64 = 0.30;
pub const STENOSIS_END_T: f64 = 0.70;

/// Fraction of the stenosis zone width that forms a flat-bottomed "neck" of
/// (near-)constant minimum radius, rather than a single infinitesimal point.
/// Real stenoses have a throat of finite length, which matters a lot for the
/// integrated (Poiseuille) resistance - and thus for the pressure gradient -
/// even though it barely changes the peak velocity.
const STENOSIS_PLATEAU_FRACTION: f64 = 0.6;

/// Smooth (C1-continuous) flat-bottomed bump function: 0 outside the stenosis
/// zone, rising via a half-cosine ramp to a plateau of 1 across the "neck",
/// then descending symmetrically back to 0.
fn stenosis_bump(t: f64) -> f64 {
    if t <= STENOSIS_START_T || t >= STENOSIS_END_T {
        return 0.0;
    }
    let span = STENOSIS_END_T - STENOSIS_START_T;
    let center = (STENOSIS_START_T + STENOSIS_END_T) * 0.5;
    let plateau_half = 0.5 * STENOSIS_PLATEAU_FRACTION * span;
    let plateau_lo = center - plateau_half;
    let plateau_hi = center + plateau_half;

    if t >= plateau_lo && t <= plateau_hi {
        1.0
    } else if t < plateau_lo {
        let local = (t - STENOSIS_START_T) / (plateau_lo - STENOSIS_START_T);
        0.5 * (1.0 - (PI * local).cos())
    } else {
        let local = (t - plateau_hi) / (STENOSIS_END_T - plateau_hi);
        0.5 * (1.0 + (PI * local).cos())
    }
}

/// Lumen (open channel) radius at normalized position `t` in `[0, 1]` along
/// the vessel, for a given plaque buildup fraction in `[0, 1]`.
pub fn lumen_radius_m(t: f64, plaque_fraction: f64) -> f64 {
    let bump = stenosis_bump(t.clamp(0.0, 1.0));
    let reduction = plaque_fraction.clamp(0.0, 1.0) * MAX_RADIUS_REDUCTION_FRACTION * bump;
    HEALTHY_LUMEN_RADIUS_M * (1.0 - reduction)
}

/// A single sampled cross-section of the vessel.
#[derive(Debug, Clone, Copy)]
pub struct FlowSample {
    pub radius_m: f64,
    pub velocity_m_s: f64,
    pub pressure_pa: f64,
}

/// The full flow/pressure field along the vessel for a given plaque state.
#[derive(Debug, Clone)]
pub struct FlowField {
    pub flow_rate_m3_s: f64,
    samples: Vec<FlowSample>,
}

impl FlowField {
    /// Computes the flow field by sampling `sample_count` cross-sections and
    /// numerically integrating the Hagen-Poiseuille viscous loss term.
    pub fn compute(plaque_fraction: f64, sample_count: usize) -> Self {
        let plaque_fraction = plaque_fraction.clamp(0.0, 1.0);
        let n = sample_count.max(2);

        let area0 = PI * HEALTHY_LUMEN_RADIUS_M.powi(2);
        // Autoregulation assumption: the heart keeps flow rate constant even
        // as resistance rises, at the cost of higher driving pressure.
        let flow_rate = area0 * BASELINE_MEAN_VELOCITY_M_S;

        let dt = 1.0 / (n - 1) as f64;
        let mut radii = Vec::with_capacity(n);
        let mut velocities = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f64 * dt;
            let r = lumen_radius_m(t, plaque_fraction);
            let area = PI * r * r;
            radii.push(r);
            velocities.push(flow_rate / area);
        }

        // Cumulative Hagen-Poiseuille resistance loss from the inlet to each
        // sample point (trapezoidal integration of 8*eta / (pi*r(x)^4)).
        let dx = VESSEL_SEGMENT_LENGTH_M * dt;
        let resistance_density = |r: f64| (8.0 * BLOOD_VISCOSITY_PA_S) / (PI * r.powi(4));
        let mut cumulative_loss = vec![0.0_f64; n];
        for i in 1..n {
            let seg_resistance =
                0.5 * (resistance_density(radii[i - 1]) + resistance_density(radii[i])) * dx;
            cumulative_loss[i] = cumulative_loss[i - 1] + flow_rate * seg_resistance;
        }
        let total_loss = cumulative_loss[n - 1];
        let v_end = velocities[n - 1];

        // Reference pressure at the OUTLET (downstream tissue requires the
        // normal perfusion pressure); solve backwards from there.
        let mut samples = Vec::with_capacity(n);
        for i in 0..n {
            let pressure = MEAN_ARTERIAL_PRESSURE_PA
                + 0.5 * BLOOD_DENSITY_KG_M3 * (v_end * v_end - velocities[i] * velocities[i])
                + (total_loss - cumulative_loss[i]);
            samples.push(FlowSample {
                radius_m: radii[i],
                velocity_m_s: velocities[i],
                pressure_pa: pressure,
            });
        }

        Self {
            flow_rate_m3_s: flow_rate,
            samples,
        }
    }

    fn interpolate(&self, t: f64, extract: impl Fn(&FlowSample) -> f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        let n = self.samples.len();
        let scaled = t * (n - 1) as f64;
        let i0 = scaled.floor() as usize;
        let i1 = (i0 + 1).min(n - 1);
        let frac = scaled - i0 as f64;
        extract(&self.samples[i0]) * (1.0 - frac) + extract(&self.samples[i1]) * frac
    }

    /// Local mean blood velocity (m/s) at normalized position `t`.
    pub fn velocity_at(&self, t: f64) -> f64 {
        self.interpolate(t, |s| s.velocity_m_s)
    }

    /// Local lumen radius (m) at normalized position `t`.
    pub fn radius_at(&self, t: f64) -> f64 {
        self.interpolate(t, |s| s.radius_m)
    }

    /// Local static pressure (Pa) at normalized position `t`. Not currently
    /// sampled by the UI (which only needs the extremes), but kept as the
    /// natural counterpart to [`Self::velocity_at`] / [`Self::radius_at`] for
    /// anyone wanting a full pressure-along-the-vessel profile (e.g. a graph).
    #[allow(dead_code)]
    pub fn pressure_at(&self, t: f64) -> f64 {
        self.interpolate(t, |s| s.pressure_pa)
    }

    pub fn inlet_pressure_pa(&self) -> f64 {
        self.samples
            .first()
            .map_or(MEAN_ARTERIAL_PRESSURE_PA, |s| s.pressure_pa)
    }

    pub fn outlet_pressure_pa(&self) -> f64 {
        self.samples
            .last()
            .map_or(MEAN_ARTERIAL_PRESSURE_PA, |s| s.pressure_pa)
    }

    pub fn min_pressure_pa(&self) -> f64 {
        self.samples
            .iter()
            .map(|s| s.pressure_pa)
            .fold(f64::INFINITY, f64::min)
    }

    pub fn max_velocity_m_s(&self) -> f64 {
        self.samples
            .iter()
            .map(|s| s.velocity_m_s)
            .fold(0.0, f64::max)
    }
}

pub fn pa_to_mmhg(pa: f64) -> f64 {
    pa / PA_PER_MMHG
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_vessel_has_uniform_velocity_and_pressure() {
        let field = FlowField::compute(0.0, 200);
        let v0 = BASELINE_MEAN_VELOCITY_M_S;
        assert!((field.max_velocity_m_s() - v0).abs() < 1e-6);
        assert!(
            (pa_to_mmhg(field.inlet_pressure_pa()) - pa_to_mmhg(field.outlet_pressure_pa())).abs()
                < 0.5
        );
    }

    #[test]
    fn stenosis_raises_inlet_pressure_and_peak_velocity() {
        let healthy = FlowField::compute(0.0, 200);
        let mild = FlowField::compute(0.5, 200);
        let severe = FlowField::compute(1.0, 200);

        assert!(mild.max_velocity_m_s() > healthy.max_velocity_m_s());
        assert!(severe.max_velocity_m_s() > mild.max_velocity_m_s());

        assert!(mild.inlet_pressure_pa() > healthy.inlet_pressure_pa());
        assert!(severe.inlet_pressure_pa() > mild.inlet_pressure_pa());

        // Venturi effect: local pressure at the throat should dip below the
        // normal mean arterial pressure once the stenosis is significant.
        assert!(severe.min_pressure_pa() < MEAN_ARTERIAL_PRESSURE_PA);

        eprintln!(
            "healthy: inlet={:.1} mmHg, min={:.1} mmHg, peak_v={:.3} m/s",
            pa_to_mmhg(healthy.inlet_pressure_pa()),
            pa_to_mmhg(healthy.min_pressure_pa()),
            healthy.max_velocity_m_s()
        );
        eprintln!(
            "mild (50%): inlet={:.1} mmHg, min={:.1} mmHg, peak_v={:.3} m/s",
            pa_to_mmhg(mild.inlet_pressure_pa()),
            pa_to_mmhg(mild.min_pressure_pa()),
            mild.max_velocity_m_s()
        );
        eprintln!(
            "severe (100%): inlet={:.1} mmHg, min={:.1} mmHg, peak_v={:.3} m/s",
            pa_to_mmhg(severe.inlet_pressure_pa()),
            pa_to_mmhg(severe.min_pressure_pa()),
            severe.max_velocity_m_s()
        );
    }

    #[test]
    fn mild_stenosis_has_small_model_pressure_rise() {
        // At this model setting, the additional inlet pressure required to
        // maintain constant flow should remain below this acceptance limit.
        // This is a model regression criterion, not a clinical diagnostic
        // threshold for hemodynamic significance.
        let mild = FlowField::compute(0.4, 200);
        let rise_mmhg =
            pa_to_mmhg(mild.inlet_pressure_pa()) - pa_to_mmhg(MEAN_ARTERIAL_PRESSURE_PA);
        assert!(rise_mmhg < 1.0, "rise was {rise_mmhg} mmHg");
    }
}

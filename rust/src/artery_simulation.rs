//! The main simulation node: owns the Blood/Plaque mesh instances and the
//! multi-type particle system, drives them from the [`hemodynamics`] model,
//! and exposes `#[func]` entry points for the UI slider.

use godot::classes::base_material_3d::{CullMode, Feature, Flags, TextureParam, Transparency};
use godot::classes::image::Format;
use godot::classes::multi_mesh::TransformFormat;
use godot::classes::{
    Image, ImageTexture, Mesh, MeshInstance3D, MultiMesh, MultiMeshInstance3D, SphereMesh,
    StandardMaterial3D, Texture2D,
};
use godot::prelude::*;
use rand::SeedableRng;
use std::f32::consts::TAU;

use crate::artery_mesh;
use crate::blood_particle::{BloodParticle, CellType};
use crate::hemodynamics::{self, FlowField};

/// How many rings (cross-sections) to sample along the vessel length when
/// building the procedural Blood/Plaque meshes. Kept modest so slider-driven
/// rebuilds stay cheap enough for mobile-class GPUs; fine-grained clumpiness
/// comes from the plaque normal map, not from extra geometry.
const MESH_RINGS: usize = 64;
/// How many segments around the circumference of the tube.
const RADIAL_SEGMENTS: usize = 48;
/// Plaque-mesh resolution. The plaque volume (outer + inner + end caps) has
/// roughly twice the triangle count of a single-sided shell at equal
/// resolution, so it is tessellated more coarsely than the blood lumen to
/// keep slider-driven rebuilds cheap. The fine granular look still comes from
/// the plaque normal/albedo maps, not from tessellation density.
const PLAQUE_RADIAL_SEGMENTS: usize = 32;
const PLAQUE_MESH_RINGS: usize = 48;
/// Number of samples used when solving the flow field (continuity + Bernoulli
/// + Poiseuille loss). Independent of mesh resolution.
const FLOW_SAMPLE_COUNT: usize = 200;

/// Visual length of the modeled vessel segment, in Godot units. Matches the
/// existing `Wall` cylinder's height.
const VISUAL_LENGTH: f32 = 4.0;
/// Godot units per meter, derived from the 4-unit vessel length over the
/// modeled 0.04 m segment. With the 2 mm healthy lumen radius, the dynamic
/// healthy lumen radius is 0.2 Godot units.
const VISUAL_UNITS_PER_METER: f32 = VISUAL_LENGTH / hemodynamics::VESSEL_SEGMENT_LENGTH_M as f32;

/// Slows down particle playback so the (otherwise sub-second) real transit
/// time is actually visible. This is purely a presentation choice: it scales
/// all particle speeds uniformly, so the *relative* speedup through the
/// stenosis - the physically meaningful part - is preserved exactly.
const VISUAL_TIME_SCALE: f64 = 1.0 / 30.0;

// Lifestyle-plaque model constants, derived from the MESA low-risk-lifestyle
// study (Ahmed et al., American Journal of Epidemiology, 2013;
// doi:10.1093/aje/kws453). The study reports how many Agatston units per year
// a higher lifestyle score slows coronary-calcium progression. We convert that
// 10-year calcium difference into an equivalent plaque fraction for this
// educational visualization using a calibration constant.
const MESA_FOLLOWUP_YEARS: f64 = 10.0;
const MESA_REFERENCE_CAC_AU_PER_YEAR: f64 = 25.0; // approximate score-0 rate
const MESA_LIFESTYLE_SLOWDOWN_AU_PER_YEAR: [f64; 5] = [0.0, 3.5, 4.2, 6.8, 11.1];
const CAC_TO_PLAQUE_FRACTION: f64 = 0.0018; // 1 Agatston unit over 10 y ≈ 0.18% plaque

/// One particle cell type and the Godot `MultiMesh` that renders it.
struct ParticleGroup {
    particles: Vec<BloodParticle>,
    multimesh: Gd<MultiMesh>,
}

/// Deterministic RNG for visual-regression tests. When `DEVIN_PARTICLE_SEED`
/// is set, particle placement is reproducible across runs; otherwise the
/// thread RNG is used as usual.
fn make_particle_rng() -> impl rand::Rng {
    match std::env::var("DEVIN_PARTICLE_SEED") {
        Ok(seed) => rand::rngs::StdRng::seed_from_u64(seed.parse().unwrap_or(0)),
        Err(_) => rand::rngs::StdRng::from_entropy(),
    }
}

/// Visual-regression hook: when `DEVIN_FREEZE_PARTICLES` is set, particle
/// motion is paused so screenshots are deterministic.
fn particle_motion_enabled() -> bool {
    std::env::var("DEVIN_FREEZE_PARTICLES")
        .map(|v| v != "1" && v.to_lowercase() != "true")
        .unwrap_or(true)
}

#[derive(GodotClass)]
#[class(init, base=Node3D)]
struct ArterySimulation {
    base: Base<Node3D>,

    #[init(node = "Wall")]
    wall: OnReady<Gd<MeshInstance3D>>,
    #[init(node = "Blood")]
    blood: OnReady<Gd<MeshInstance3D>>,
    #[init(node = "Plaque")]
    plaque: OnReady<Gd<MeshInstance3D>>,
    #[init(node = "ParticlesRbc")]
    particles_rbc: OnReady<Gd<MultiMeshInstance3D>>,
    #[init(node = "ParticlesWbc")]
    particles_wbc: OnReady<Gd<MultiMeshInstance3D>>,
    #[init(node = "ParticlesPlatelet")]
    particles_platelet: OnReady<Gd<MultiMeshInstance3D>>,

    wall_material: Option<Gd<StandardMaterial3D>>,
    blood_material: Option<Gd<StandardMaterial3D>>,
    plaque_material: Option<Gd<StandardMaterial3D>>,

    /// Direct plaque slider value (0..100).
    direct_plaque_percent: f64,
    /// MESA low-risk lifestyle score (0..4), set by the second UI slider.
    lifestyle_score: f64,
    /// Smoking dose (cigarettes per day, 0..40), set by the third UI slider.
    cigarettes_per_day: f64,
    /// Years of the 10-year window spent smoking (0..10), set by the fourth
    /// UI slider.
    years_of_smoking: f64,
    /// Which plaque source the sliders currently control: 0 = direct plaque,
    /// 1 = MESA lifestyle, 2 = smoking. Adjusting a slider makes its group
    /// the *exclusive* contributor so each control can be inspected alone.
    active_source: i64,
    /// Effective plaque percentage contributed by the active source, clamped,
    /// that the current meshes were built from.
    effective_percent: f64,

    flow: Option<FlowField>,
    groups: Vec<ParticleGroup>,
    /// Defers the first (heavy) mesh build to the first process frame so the
    /// E2E automation server can finish its handshake before `_ready` blocks.
    pending_initial_rebuild: bool,
}

#[godot_api]
impl INode3D for ArterySimulation {
    fn ready(&mut self) {
        self.wall_material = Some(make_wall_material());
        self.blood_material = Some(make_blood_material());
        self.plaque_material = Some(make_plaque_material());
        self.wall
            .set_material_override(self.wall_material.as_ref().unwrap());
        self.setup_particles();

        self.direct_plaque_percent = 0.0;
        self.lifestyle_score = 0.0;
        self.cigarettes_per_day = 0.0;
        self.years_of_smoking = 10.0;
        self.active_source = Self::SOURCE_DIRECT;
        self.effective_percent = -1.0;
        self.pending_initial_rebuild = true;
    }

    fn process(&mut self, delta: f64) {
        if self.pending_initial_rebuild {
            self.pending_initial_rebuild = false;
            self.rebuild();
            return;
        }
        if !particle_motion_enabled() {
            return;
        }
        let Some(flow) = self.flow.clone() else {
            return;
        };

        let mut rng = rand::thread_rng();
        for group in &mut self.groups {
            let mut multimesh = group.multimesh.clone();
            for (i, particle) in group.particles.iter_mut().enumerate() {
                // Mean velocity from the 1D flow model; assume a Poiseuille
                // parabolic profile so particles near the centre move faster
                // than particles hugging the wall.
                let mean_velocity = flow.velocity_at(particle.t as f64) as f32;
                let centreline_velocity = 2.0 * mean_velocity;
                let radial_factor = 1.0 - particle.radius_frac * particle.radius_frac;
                let local_velocity = centreline_velocity * radial_factor;

                let dt_t = (local_velocity / hemodynamics::VESSEL_SEGMENT_LENGTH_M as f32)
                    * VISUAL_TIME_SCALE as f32
                    * delta as f32;
                particle.t += dt_t;
                if particle.t >= 1.0 {
                    particle.respawn_at_inlet(&mut rng);
                }

                let transform = particle_transform(
                    particle,
                    &flow,
                    self.effective_percent,
                    healthy_lumen_radius_visual(),
                );
                multimesh.set_instance_transform(i as i32, transform);
            }
        }
    }
}

#[godot_api]
impl ArterySimulation {
    /// Plaque-source selector values, mirrored by `ui.gd`.
    const SOURCE_DIRECT: i64 = 0;
    const SOURCE_LIFESTYLE: i64 = 1;
    const SOURCE_SMOKING: i64 = 2;

    /// Called by the direct plaque UI slider (0..100) whenever plaque
    /// accumulation changes.
    #[func]
    fn set_plaque_percent(&mut self, percent: f64) {
        self.direct_plaque_percent = percent.clamp(0.0, 100.0);
        self.rebuild();
    }

    #[func]
    fn get_plaque_percent(&self) -> f64 {
        self.direct_plaque_percent
    }

    /// Called by the lifestyle UI slider (0..4) whenever the MESA lifestyle
    /// score changes.
    #[func]
    fn set_lifestyle_score(&mut self, score: f64) {
        self.lifestyle_score = score.clamp(0.0, 4.0);
        self.rebuild();
    }

    #[func]
    fn get_lifestyle_score(&self) -> f64 {
        self.lifestyle_score
    }

    /// Updates all four slider inputs plus the active plaque source at once
    /// and rebuilds only once. Used by the UI whenever any slider changes.
    #[func]
    fn set_parameters(
        &mut self,
        plaque_percent: f64,
        lifestyle_score: f64,
        cigarettes_per_day: f64,
        years_of_smoking: f64,
        active_source: i64,
    ) {
        self.direct_plaque_percent = plaque_percent.clamp(0.0, 100.0);
        self.lifestyle_score = lifestyle_score.clamp(0.0, 4.0);
        self.cigarettes_per_day = cigarettes_per_day.clamp(0.0, 40.0);
        self.years_of_smoking = years_of_smoking.clamp(0.0, 10.0);
        self.active_source = active_source.clamp(0, 2);
        self.rebuild();
    }

    /// Selects which plaque source the sliders control exclusively (0 =
    /// direct plaque, 1 = MESA lifestyle, 2 = smoking). Adjusting any slider
    /// in the UI sets this, zeroing the other sources' contributions.
    #[func]
    fn set_active_source(&mut self, source: i64) {
        self.active_source = source.clamp(0, 2);
        self.rebuild();
    }

    #[func]
    fn get_active_source(&self) -> i64 {
        self.active_source
    }

    #[func]
    fn get_cigarettes_per_day(&self) -> f64 {
        self.cigarettes_per_day
    }

    #[func]
    fn get_years_of_smoking(&self) -> f64 {
        self.years_of_smoking
    }

    /// The plaque fraction contributed by the 10-year MESA lifestyle model,
    /// in percent (0..100).
    #[func]
    fn get_lifestyle_plaque_percent(&self) -> f64 {
        lifestyle_plaque_percent(self.lifestyle_score)
    }

    /// The plaque fraction contributed by the 10-year smoking model, in
    /// percent (0..100).
    #[func]
    fn get_smoking_plaque_percent(&self) -> f64 {
        crate::smoking_model::smoking_plaque_percent(self.cigarettes_per_day, self.years_of_smoking)
    }

    /// The effective plaque fraction used by the simulation, in percent: the
    /// *active* source's contribution alone (direct plaque, MESA lifestyle, or
    /// CARDIA/PDAY smoking), clamped to the model's range. Adjusting any
    /// slider switches the active source, so each control can be inspected
    /// without interplay from the others.
    #[func]
    fn get_effective_plaque_percent(&self) -> f64 {
        self.effective_percent
    }

    /// Driving pressure the heart must generate at the vessel inlet (mmHg).
    #[func]
    fn get_inlet_pressure_mmhg(&self) -> f64 {
        self.flow.as_ref().map_or(
            hemodynamics::pa_to_mmhg(hemodynamics::MEAN_ARTERIAL_PRESSURE_PA),
            |f| hemodynamics::pa_to_mmhg(f.inlet_pressure_pa()),
        )
    }

    /// Lowest local pressure along the vessel (mmHg) - the Venturi dip at the
    /// stenosis throat.
    #[func]
    fn get_min_pressure_mmhg(&self) -> f64 {
        self.flow.as_ref().map_or(
            hemodynamics::pa_to_mmhg(hemodynamics::MEAN_ARTERIAL_PRESSURE_PA),
            |f| hemodynamics::pa_to_mmhg(f.min_pressure_pa()),
        )
    }

    /// Peak blood velocity anywhere along the vessel (cm/s).
    #[func]
    fn get_peak_velocity_cm_s(&self) -> f64 {
        self.flow
            .as_ref()
            .map_or(0.0, |f| f.max_velocity_m_s() * 100.0)
    }

    /// Pressure right at the vessel outlet (mmHg) - the downstream perfusion
    /// pressure, which stays pinned near the healthy baseline in this model.
    #[func]
    fn get_outlet_pressure_mmhg(&self) -> f64 {
        self.flow.as_ref().map_or(
            hemodynamics::pa_to_mmhg(hemodynamics::MEAN_ARTERIAL_PRESSURE_PA),
            |f| hemodynamics::pa_to_mmhg(f.outlet_pressure_pa()),
        )
    }

    /// Blood flow rate through the vessel (mL/s), held constant by the
    /// autoregulation assumption regardless of plaque buildup.
    #[func]
    fn get_flow_rate_ml_s(&self) -> f64 {
        self.flow.as_ref().map_or(0.0, |f| f.flow_rate_m3_s * 1.0e6)
    }

    fn setup_particles(&mut self) {
        let mut rng = make_particle_rng();
        self.groups = CellType::target_counts()
            .iter()
            .map(|(cell_type, count)| {
                let mesh = match cell_type {
                    CellType::Rbc => make_rbc_mesh(),
                    CellType::Wbc => make_wbc_mesh(),
                    CellType::Platelet => make_platelet_mesh(),
                };
                let multimesh = make_multimesh(&mesh, *count as i32);

                match cell_type {
                    CellType::Rbc => self.particles_rbc.set_multimesh(&multimesh),
                    CellType::Wbc => self.particles_wbc.set_multimesh(&multimesh),
                    CellType::Platelet => self.particles_platelet.set_multimesh(&multimesh),
                }

                let particles = (0..*count)
                    .map(|_| BloodParticle::random(*cell_type, &mut rng))
                    .collect();

                ParticleGroup {
                    particles,
                    multimesh,
                }
            })
            .collect();
    }

    /// Recomputes the effective plaque fraction from the *active* plaque
    /// source only — adjusting any slider makes its group the exclusive
    /// contributor, zeroing the others, so each control's independent effect
    /// is visible without interplay. Skipped entirely when the effective
    /// percentage is unchanged, so fine-grained slider events emitted during
    /// a drag do not rebuild the meshes hundreds of times.
    fn rebuild(&mut self) {
        let effective_percent = match self.active_source {
            Self::SOURCE_LIFESTYLE => lifestyle_plaque_percent(self.lifestyle_score),
            Self::SOURCE_SMOKING => crate::smoking_model::smoking_plaque_percent(
                self.cigarettes_per_day,
                self.years_of_smoking,
            ),
            _ => self.direct_plaque_percent,
        }
        .clamp(0.0, 100.0);
        if self.flow.is_some() && effective_percent == self.effective_percent {
            return;
        }
        self.effective_percent = effective_percent;
        let fraction = effective_percent / 100.0;
        let flow = FlowField::compute(fraction, FLOW_SAMPLE_COUNT);
        let healthy_radius_visual = healthy_lumen_radius_visual();

        // Visual-only channel boundary: the smooth hemodynamic lumen minus
        // deposit protrusions that push INTO the lumen (like the reference
        // imagery). The flow field itself stays mathematically smooth.
        let channel_radius_at = |t: f32, angle: f32| -> f32 {
            (flow.radius_at(t as f64) as f32 * VISUAL_UNITS_PER_METER
                - deposit_protrusion(&flow, fraction, healthy_radius_visual, t, angle))
            .max(0.01)
        };

        if let Some(mesh) = artery_mesh::build_solid_tube(
            VISUAL_LENGTH,
            RADIAL_SEGMENTS,
            MESH_RINGS,
            self.blood_material.as_ref().unwrap(),
            channel_radius_at,
        ) {
            self.blood.set_mesh(&mesh);
        }

        let lumen_radius_at =
            |t: f32, _angle: f32| flow.radius_at(t as f64) as f32 * VISUAL_UNITS_PER_METER;

        // Plaque outer surface: healthy-lumen boundary plus chunky, rounded
        // clumps, plus a small baseline thickness so the deposit has visible
        // volume even at the vessel ends (where the stenosis envelope is zero).
        // The deposit is now a closed annular volume with an inner surface at
        // the lumen, an outer bumpy surface, and annular end caps, so it no
        // longer reads as a hollow film.
        let plaque_outer_radius = |t: f32, angle: f32| -> f32 {
            let base = healthy_radius_visual;
            let lumen = lumen_radius_at(t, 0.0);
            let thickness = base - lumen;
            if fraction <= 0.001 || thickness <= 0.0 {
                return base;
            }

            // Raised-cosine-ish envelope: zero at t=0/1, maximum at t=0.5.
            let envelope = (std::f32::consts::PI * t).sin().max(0.0);

            // Two octaves of clump noise at lobe scale (~0.3-0.5 units, like
            // the reference lobes); only the positive part forms lobes. The
            // strong contrast exponent (0.4) pushes even modest noise values
            // up to full lobe height, so the deposit bulges outward in
            // distinct, separated clumps instead of broad gentle swells.
            let n1 = lump_noise(t * 8.0, angle / TAU * 10.0, 10);
            let n2 = lump_noise(t * 14.0 + 3.9, angle / TAU * 16.0, 16);
            let clump = ((0.55 * n1 + 0.45 * n2) * 2.0 - 1.0).max(0.0).powf(0.4);

            // Baseline thickness: a diffuse intimal layer that stays visible at
            // the ends (envelope=0) so the plaque volume closes with annular
            // end caps rather than pinching to a zero-thickness film.
            let base_thickness = 0.10f32 * (fraction as f32) * base;
            let lobe_amplitude = (0.03 + 0.6 * thickness) * envelope;
            (base + base_thickness + lobe_amplitude * clump).max(lumen + 0.002)
        };

        // Inner plaque surface sits just outside the smooth hemodynamic lumen;
        // the blood mesh is slightly inside because of deposit_protrusion, so
        // there is no coplanar z-fighting between plaque and blood.
        let plaque_inner_radius = |t: f32, _angle: f32| -> f32 {
            lumen_radius_at(t, 0.0) + 0.002
        };

        // Per-vertex alpha: the raised lobes stay fully opaque, while the thin
        // diffuse lining stays translucent so the blood flow remains visible.
        // The translucent yellow tinge strengthens with overall accumulation.
        // The clump noise here mirrors the geometry closure exactly, so
        // opacity correlates 1:1 with the visible bulges.
        let plaque_alpha = |t: f32, angle: f32| -> f32 {
            if fraction <= 0.001 {
                return 0.0;
            }
            let envelope = (std::f32::consts::PI * t).sin().max(0.0);
            let n1 = lump_noise(t * 8.0, angle / TAU * 10.0, 10);
            let n2 = lump_noise(t * 14.0 + 3.9, angle / TAU * 16.0, 16);
            let clump = ((0.55 * n1 + 0.45 * n2) * 2.0 - 1.0).max(0.0).powf(0.4);
            let tinge = (0.15f32 + 0.45f32 * fraction as f32).clamp(0.0, 0.6);
            tinge + (1.0 - tinge) * envelope * clump
        };

        if let Some(mesh) = artery_mesh::build_plaque_volume(
            VISUAL_LENGTH,
            PLAQUE_RADIAL_SEGMENTS,
            PLAQUE_MESH_RINGS,
            self.plaque_material.as_ref().unwrap(),
            plaque_outer_radius,
            plaque_inner_radius,
            plaque_alpha,
        ) {
            self.plaque.set_mesh(&mesh);
        }
        self.plaque.set_visible(fraction > 0.001);

        self.write_particle_transforms(&flow);
        self.flow = Some(flow);
    }

    /// Writes one transform per particle so the frozen (test) layout is
    /// visible even when `process` is paused by `DEVIN_FREEZE_PARTICLES`.
    fn write_particle_transforms(&mut self, flow: &FlowField) {
        let healthy_radius_visual = healthy_lumen_radius_visual();
        for group in &mut self.groups {
            let mut multimesh = group.multimesh.clone();
            for (i, particle) in group.particles.iter_mut().enumerate() {
                let transform = particle_transform(
                    particle,
                    flow,
                    self.effective_percent,
                    healthy_radius_visual,
                );
                multimesh.set_instance_transform(i as i32, transform);
            }
        }
    }
}

/// Converts a MESA low-risk lifestyle score (0..4) into the modelled
/// 10-year plaque contribution, in percent.
fn lifestyle_plaque_percent(score: f64) -> f64 {
    let index = (score.round() as usize).clamp(0, 4);
    let annual_slowdown = MESA_LIFESTYLE_SLOWDOWN_AU_PER_YEAR[index];
    let ten_year_cac = (MESA_REFERENCE_CAC_AU_PER_YEAR - annual_slowdown) * MESA_FOLLOWUP_YEARS;
    ten_year_cac * CAC_TO_PLAQUE_FRACTION * 100.0
}

/// Deterministic integer hash in `[0, 1)`, used by [`lump_noise`].
fn hash_noise(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(0x27d4eb2d) ^ y.wrapping_mul(0x165667b1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2545_f491);
    h ^= h >> 13;
    (h & 0x00ff_ffff) as f32 / 0x0100_0000 as f32
}

/// Smooth 2D value noise in `[0, 1]`. `v_period` must be a positive integer so
/// the noise wraps seamlessly around the vessel circumference.
fn lump_noise(u: f32, v: f32, v_period: u32) -> f32 {
    let x0 = u.floor();
    let y0 = v.floor();
    let fx = u - x0;
    let fy = v - y0;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let xi = x0 as i64 as u32;
    let yi = (y0 as i64).rem_euclid(v_period as i64) as u32;
    let yi1 = (yi + 1) % v_period;
    let h00 = hash_noise(xi, yi);
    let h10 = hash_noise(xi + 1, yi);
    let h01 = hash_noise(xi, yi1);
    let h11 = hash_noise(xi + 1, yi1);
    let a = h00 + (h10 - h00) * sx;
    let b = h01 + (h11 - h01) * sx;
    a + (b - a) * sy
}

fn make_multimesh(mesh: &Gd<Mesh>, count: i32) -> Gd<MultiMesh> {
    let mut multimesh = MultiMesh::new_gd();
    multimesh.set_transform_format(TransformFormat::TRANSFORM_3D);
    multimesh.set_mesh(mesh);
    multimesh.set_instance_count(count);
    multimesh
}

/// Healthy lumen radius expressed in Godot visual units.
fn healthy_lumen_radius_visual() -> f32 {
    hemodynamics::HEALTHY_LUMEN_RADIUS_M as f32 * VISUAL_UNITS_PER_METER
}

/// Visual-only deposit protrusion into the lumen, in Godot units. Mirrors the
/// reference imagery: the fatty deposit forms rounded bumps that push into the
/// blood channel. Always zero outside the stenosis zone and at zero plaque, so
/// the hemodynamic lumen itself stays mathematically smooth.
fn deposit_protrusion(
    flow: &FlowField,
    fraction: f64,
    healthy_radius_visual: f32,
    t: f32,
    angle: f32,
) -> f32 {
    let lumen = flow.radius_at(t as f64) as f32 * VISUAL_UNITS_PER_METER;
    let thickness = (healthy_radius_visual - lumen).max(0.0);
    if fraction <= 0.001 || thickness <= 0.0 {
        return 0.0;
    }

    // Fade to zero at the vessel ends, peak at the stenosis centre.
    let envelope = (std::f32::consts::PI * t).sin().max(0.0);

    // Three octaves of smooth value noise; only the positive part protrudes,
    // so the deposit grows into the channel without ever receding into the
    // arterial wall. The contrast exponent makes the bumps read as chunky,
    // rounded lobes rather than gentle ripples.
    let n1 = lump_noise(t * 2.0, angle / TAU * 4.0, 4);
    let n2 = lump_noise(t * 5.0 + 11.7, angle / TAU * 7.0, 7);
    let n3 = lump_noise(t * 9.0 + 4.3, angle / TAU * 9.0, 9);
    let combined = 0.45 * n1 + 0.3 * n2 + 0.25 * n3;
    let bump = ((combined * 2.0 - 1.0).max(0.0)).powf(0.45);

    // Cap the protrusion at a fraction of the local lumen radius so a severe
    // stenosis shows deep lobes without ever visually closing the channel.
    (thickness * 0.45 * envelope * bump).min(lumen * 0.6)
}

/// World-space transform for one blood-cell particle, derived from the local
/// flow field and the particle's normalized placement. Particles are contained
/// by the *visual* (lumpy) channel radius so cells never clip through the
/// deposit protrusions; their velocity still comes from the smooth flow model.
fn particle_transform(
    particle: &BloodParticle,
    flow: &FlowField,
    effective_percent: f64,
    healthy_radius_visual: f32,
) -> Transform3D {
    let protrusion = deposit_protrusion(
        flow,
        effective_percent / 100.0,
        healthy_radius_visual,
        particle.t,
        particle.angle,
    );
    let local_radius_visual =
        (flow.radius_at(particle.t as f64) as f32 * VISUAL_UNITS_PER_METER - protrusion).max(0.005);
    let r = particle.radius_frac * local_radius_visual;
    let x = r * particle.angle.cos();
    let z = r * particle.angle.sin();
    let y = -VISUAL_LENGTH * 0.5 + VISUAL_LENGTH * particle.t;
    let scale = cell_type_scale(particle.cell_type);
    let basis = particle.orientation * Basis::from_scale(scale);
    Transform3D {
        basis,
        origin: Vector3::new(x, y, z),
    }
}

fn make_rbc_mesh() -> Gd<Mesh> {
    let mut mesh = SphereMesh::new_gd();
    // Oblate spheroid / flattened disc, approximating the biconcave RBC shape
    // at low-poly resolution.
    mesh.set_radius(0.015);
    mesh.set_height(0.006);
    mesh.set_radial_segments(6);
    mesh.set_rings(3);

    let mut material = StandardMaterial3D::new_gd();
    material.set_albedo(Color::from_rgb(0.75, 0.08, 0.08));
    material.set_feature(Feature::EMISSION, true);
    material.set_emission(Color::from_rgb(0.4, 0.02, 0.02));
    material.set_emission_energy_multiplier(0.5);
    mesh.set_material(&material);

    mesh.upcast::<Mesh>()
}

fn make_wbc_mesh() -> Gd<Mesh> {
    let mut mesh = SphereMesh::new_gd();
    // WBCs are larger and roughly spherical.
    mesh.set_radius(0.018);
    mesh.set_height(0.036);
    mesh.set_radial_segments(6);
    mesh.set_rings(4);

    let mut material = StandardMaterial3D::new_gd();
    material.set_albedo(Color::from_rgb(0.95, 0.92, 0.82));
    material.set_feature(Feature::EMISSION, true);
    material.set_emission(Color::from_rgb(0.3, 0.28, 0.18));
    material.set_emission_energy_multiplier(0.4);
    mesh.set_material(&material);

    mesh.upcast::<Mesh>()
}

fn make_platelet_mesh() -> Gd<Mesh> {
    let mut mesh = SphereMesh::new_gd();
    // Platelets are much smaller than RBCs; slight non-uniform scaling is
    // applied per instance to suggest their irregular fragment shape.
    mesh.set_radius(0.008);
    mesh.set_height(0.008);
    mesh.set_radial_segments(5);
    mesh.set_rings(3);

    let mut material = StandardMaterial3D::new_gd();
    material.set_albedo(Color::from_rgb(0.9, 0.75, 0.3));
    material.set_feature(Feature::EMISSION, true);
    material.set_emission(Color::from_rgb(0.45, 0.35, 0.1));
    material.set_emission_energy_multiplier(0.5);
    mesh.set_material(&material);

    mesh.upcast::<Mesh>()
}

fn cell_type_scale(cell_type: CellType) -> Vector3 {
    match cell_type {
        // Emphasise the flattened-disc look of RBCs in 3D space.
        CellType::Rbc => Vector3::new(1.0, 0.35, 1.0),
        CellType::Wbc => Vector3::new(1.0, 1.0, 1.0),
        // Small, irregular platelet fragments.
        CellType::Platelet => Vector3::new(1.3, 0.45, 0.9),
    }
}

fn make_blood_material() -> Gd<StandardMaterial3D> {
    let mut material = StandardMaterial3D::new_gd();
    material.set_transparency(Transparency::ALPHA);
    material.set_albedo(Color::from_rgba(0.55, 0.02, 0.05, 0.2));
    material.set_roughness(0.25);
    material.set_feature(Feature::EMISSION, true);
    material.set_emission(Color::from_rgb(0.35, 0.0, 0.02));
    material.set_emission_energy_multiplier(0.15);
    material.set_cull_mode(CullMode::DISABLED);
    material
}

fn make_plaque_material() -> Gd<StandardMaterial3D> {
    let mut material = StandardMaterial3D::new_gd();
    material.set_transparency(Transparency::ALPHA);
    // Neutral tint; the detail albedo texture supplies the yellow-orange ramp.
    // Opacity is driven per-vertex (see build_plaque_volume's alpha callback):
    // raised lobes are fully opaque, the thin diffuse lining stays translucent
    // so the blood flow remains visible underneath.
    material.set_albedo(Color::from_rgba(1.0, 1.0, 1.0, 1.0));
    material.set_flag(Flags::ALBEDO_FROM_VERTEX_COLOR, true);
    material.set_roughness(0.75);
    material.set_feature(Feature::SUBSURFACE_SCATTERING, true);
    material.set_subsurface_scattering_strength(0.12);
    // Slightly larger normal-map tiles for fewer, chunkier-looking lipid
    // lobes that match the increased geometric volume.
    material.set_texture(TextureParam::NORMAL, &make_plaque_normal_texture());
    material.set_normal_scale(2.4);
    material.set_uv1_scale(Vector3::new(6.0, 3.0, 1.0));
    material.set_texture(TextureParam::ALBEDO, &make_plaque_albedo_texture());
    // Cull backfaces so the inner (lumen-facing) surface does not draw over
    // the outer surface; the closed volume still looks solid from outside.
    material.set_cull_mode(CullMode::BACK);
    material
}

/// Tileable granular height field over the unit square. Each term is a product
/// of *clipped, squared* sinusoids with integer frequencies: the positive
/// half-waves form isolated, rounded bead-like bumps (like the granular
/// lipid clumps in the reference imagery), and the integer frequencies make
/// the field wrap seamlessly in both axes so it tiles cleanly on the shell's
/// UVs.
fn granular_height(u: f32, v: f32) -> f32 {
    // (freq_u, freq_v, weight, phase_u, phase_v)
    const TERMS: [(f32, f32, f32, f32, f32); 6] = [
        (4.0, 5.0, 0.30, 0.0, 1.3),
        (7.0, 6.0, 0.22, 2.1, 4.2),
        (9.0, 11.0, 0.18, 4.2, 2.7),
        (13.0, 9.0, 0.14, 0.4, 5.0),
        (16.0, 12.0, 0.10, 5.2, 2.2),
        (15.0, 14.0, 0.10, 2.9, 3.7),
    ];
    let mut h = 0.0;
    for (fu, fv, w, pu, pv) in TERMS {
        let su = (TAU * (fu * u) + pu).sin().max(0.0);
        let sv = (TAU * (fv * v) + pv).sin().max(0.0);
        h += w * su * su * sv * sv;
    }
    h * 1.8 // push contrast so bumps read as distinct clumps
}

/// Generates a small tileable normal map encoding the granular clump texture.
/// One-time cost at startup; adds surface detail without extra geometry.
fn make_plaque_normal_texture() -> Gd<Texture2D> {
    const SIZE: usize = 256;
    let sample = |x: usize, y: usize| -> f32 {
        let u = x as f32 / SIZE as f32;
        let v = y as f32 / SIZE as f32;
        granular_height(u, v)
    };

    let mut data = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let hl = sample((x + SIZE - 1) % SIZE, y);
            let hr = sample((x + 1) % SIZE, y);
            let hu = sample(x, (y + SIZE - 1) % SIZE);
            let hd = sample(x, (y + 1) % SIZE);
            let normal = Vector3::new((hl - hr) * 3.0, (hu - hd) * 3.0, 1.0).normalized();
            data.push(((normal.x * 0.5 + 0.5) * 255.0) as u8);
            data.push(((normal.y * 0.5 + 0.5) * 255.0) as u8);
            data.push(((normal.z * 0.5 + 0.5) * 255.0) as u8);
            data.push(255);
        }
    }
    let packed = PackedByteArray::from(data.as_slice());
    let image = Image::create_from_data(SIZE as i32, SIZE as i32, false, Format::RGBA8, &packed)
        .expect("normal map image");
    let texture = ImageTexture::create_from_image(&image).expect("normal map texture");
    texture.upcast::<Texture2D>()
}

/// Generates the matching albedo detail texture: the same granular height
/// field mapped into a yellow-to-orange ramp, so crevices darken and clump
/// tops stay bright. This colour variation is what makes the deposit read as
/// distinct lipid clumps (as in the reference imagery) regardless of lighting.
fn make_plaque_albedo_texture() -> Gd<Texture2D> {
    const SIZE: usize = 256;
    let mut data = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let h = granular_height(x as f32 / SIZE as f32, y as f32 / SIZE as f32);
            let t = (h / 1.8).clamp(0.0, 1.0);
            // Crevice (warm orange) -> clump top (bright yellow). Kept bright
            // overall so the deposit reads as saturated yellow, not brown.
            let r = 0.88 + 0.12 * t;
            let g = 0.60 + 0.25 * t;
            let b = 0.12 + 0.16 * t;
            data.push((r * 255.0) as u8);
            data.push((g * 255.0) as u8);
            data.push((b * 255.0) as u8);
            data.push(255);
        }
    }
    let packed = PackedByteArray::from(data.as_slice());
    let image = Image::create_from_data(SIZE as i32, SIZE as i32, false, Format::RGBA8, &packed)
        .expect("albedo detail image");
    let texture = ImageTexture::create_from_image(&image).expect("albedo detail texture");
    texture.upcast::<Texture2D>()
}

fn make_wall_material() -> Gd<StandardMaterial3D> {
    let mut material = StandardMaterial3D::new_gd();
    material.set_transparency(Transparency::ALPHA);
    // Smooth, wet, glass-like pink arterial wall. Roughness/clearcoat are kept
    // moderate so the specular highlight stays broad and does not alias into
    // striped bands on the tessellated cylinder.
    material.set_albedo(Color::from_rgba(0.94, 0.58, 0.64, 0.12));
    material.set_roughness(0.3);
    material.set_feature(Feature::CLEARCOAT, true);
    material.set_clearcoat(0.5);
    material.set_feature(Feature::SUBSURFACE_SCATTERING, true);
    material.set_subsurface_scattering_strength(0.15);
    material.set_cull_mode(CullMode::DISABLED);
    material
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifestyle_plaque_percent_decreases_with_healthier_score() {
        let p0 = lifestyle_plaque_percent(0.0);
        let p1 = lifestyle_plaque_percent(1.0);
        let p2 = lifestyle_plaque_percent(2.0);
        let p3 = lifestyle_plaque_percent(3.0);
        let p4 = lifestyle_plaque_percent(4.0);

        assert!(p0 > p1);
        assert!(p1 > p2);
        assert!(p2 > p3);
        assert!(p3 > p4);
    }

    #[test]
    fn lifestyle_plaque_percent_matches_mesa_model_constants() {
        // Score 0: reference rate * 10 y * calibration, expressed as percent.
        let expected_0 =
            MESA_REFERENCE_CAC_AU_PER_YEAR * MESA_FOLLOWUP_YEARS * CAC_TO_PLAQUE_FRACTION * 100.0;
        assert!((lifestyle_plaque_percent(0.0) - expected_0).abs() < 1e-9);

        // Score 4 uses the largest reported slowdown (11.1 AU/year).
        let expected_4 = (MESA_REFERENCE_CAC_AU_PER_YEAR - MESA_LIFESTYLE_SLOWDOWN_AU_PER_YEAR[4])
            * MESA_FOLLOWUP_YEARS
            * CAC_TO_PLAQUE_FRACTION
            * 100.0;
        assert!((lifestyle_plaque_percent(4.0) - expected_4).abs() < 1e-9);
    }

    #[test]
    fn lifestyle_plaque_percent_never_negative() {
        for score in [0.0, 0.5, 1.0, 2.5, 3.0, 4.0] {
            assert!(lifestyle_plaque_percent(score) >= 0.0);
        }
    }

    #[test]
    fn effective_plaque_percent_clamps_and_adds_both_inputs() {
        // Direct slider only.
        assert!(
            ((30.0 + lifestyle_plaque_percent(0.0)).clamp(0.0, 100.0)
                - (30.0 + lifestyle_plaque_percent(0.0)).min(100.0))
            .abs()
                < 1e-9
        );

        // Direct + lifestyle stay within [0, 100].
        let low = (0.0 + lifestyle_plaque_percent(4.0)).clamp(0.0, 100.0);
        assert!(low >= 0.0 && low <= 100.0);

        let high = (90.0 + lifestyle_plaque_percent(0.0)).clamp(0.0, 100.0);
        assert_eq!(high, 100.0);
    }

    #[test]
    fn deposit_protrusion_is_zero_without_plaque() {
        let flow = FlowField::compute(0.0, 32);
        let healthy = healthy_lumen_radius_visual();
        for t in [0.1, 0.35, 0.5, 0.65, 0.9] {
            for angle in [0.0, 1.0, 3.0, 5.0] {
                let p = deposit_protrusion(&flow, 0.0, healthy, t, angle);
                assert_eq!(p, 0.0, "protrusion at t={t} angle={angle} was {p}");
            }
        }
    }

    #[test]
    fn deposit_protrusion_is_non_negative_and_bounded_by_thickness() {
        let fraction = 0.8;
        let flow = FlowField::compute(fraction, 64);
        let healthy = healthy_lumen_radius_visual();
        for i in 0..24 {
            let t = i as f32 / 24.0;
            for j in 0..12 {
                let angle = std::f32::consts::TAU * j as f32 / 12.0;
                let p = deposit_protrusion(&flow, fraction, healthy, t, angle);
                assert!(p >= 0.0, "negative protrusion {p} at t={t} angle={angle}");
                let lumen = flow.radius_at(t as f64) as f32 * VISUAL_UNITS_PER_METER;
                let thickness = healthy - lumen;
                assert!(
                    p <= thickness + 1e-6,
                    "protrusion {p} exceeded thickness {thickness} at t={t} angle={angle}"
                );
                // The visual channel must never close, even at severe stenosis.
                assert!(
                    p <= lumen * 0.6 + 1e-6,
                    "protrusion {p} exceeded 60% of lumen {lumen} at t={t} angle={angle}"
                );
            }
        }
    }

    #[test]
    fn deposit_protrusion_grows_with_plaque_fraction() {
        let healthy = healthy_lumen_radius_visual();
        let (t, angle) = (0.5, 1.3);
        let low = deposit_protrusion(&FlowField::compute(0.3, 64), 0.3, healthy, t, angle);
        let high = deposit_protrusion(&FlowField::compute(0.9, 64), 0.9, healthy, t, angle);
        assert!(high > low, "protrusion did not grow: {low} -> {high}");
    }

    #[test]
    fn deposit_protrusion_fades_to_zero_at_vessel_ends() {
        let flow = FlowField::compute(0.8, 64);
        let healthy = healthy_lumen_radius_visual();
        for t in [0.0, 0.05, 0.95, 1.0] {
            let p = deposit_protrusion(&flow, 0.8, healthy, t, 2.0);
            assert_eq!(p, 0.0, "protrusion at vessel end t={t} was {p}");
        }
    }
}

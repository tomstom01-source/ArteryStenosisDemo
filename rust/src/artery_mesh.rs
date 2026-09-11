//! Procedural "surface of revolution" mesh building for the blood lumen and
//! plaque coating. Both need a radius that varies along the vessel's length
//! (narrowing in the middle), which Godot's built-in `CylinderMesh` cannot
//! express, so we build the geometry by hand.
//!
//! Meshes are assembled as plain Rust vectors and committed with a *single*
//! `ArrayMesh::add_surface_from_arrays` call. The previous implementation used
//! `SurfaceTool`, whose per-vertex FFI calls (`set_uv`/`add_vertex`) plus
//! `index()`/`generate_normals()` dominated the slider-driven rebuild cost.

use godot::classes::mesh::PrimitiveType;
use godot::classes::{ArrayMesh, Material};
use godot::meta::AsObjectArg;
use godot::obj::NewGd;
use godot::prelude::*;
use std::f32::consts::TAU;

fn ring_point(radius: f32, y: f32, angle: f32) -> Vector3 {
    Vector3::new(radius * angle.cos(), y, radius * angle.sin())
}

/// CPU-side mesh under construction. Vertices are welded per grid slot (the
/// UV seam duplicates a position but shares its normal accumulator), and
/// smooth normals are accumulated from face normals - the same result the old
/// `SurfaceTool::index()` + `generate_normals()` pipeline produced, without
/// the per-vertex FFI cost.
struct SurfaceBuilder {
    positions: Vec<Vector3>,
    uvs: Vec<Vector2>,
    indices: Vec<i32>,
    /// Face-normal accumulators, indexed in parallel with `positions`.
    normal_acc: Vec<Vector3>,
    /// Maps each vertex to the accumulator it contributes to (itself, or the
    /// seam-shared original vertex).
    acc_of: Vec<usize>,
    /// Optional per-vertex colors. When filled to the same length as
    /// `positions`, a COLOR array is committed with the surface; callers whose
    /// material does not use vertex colors simply leave it empty.
    colors: Vec<Color>,
}

impl SurfaceBuilder {
    fn new() -> Self {
        Self {
            positions: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
            normal_acc: Vec::new(),
            acc_of: Vec::new(),
            colors: Vec::new(),
        }
    }

    /// Adds a vertex. `shares_acc_with` optionally points at an earlier vertex
    /// whose normal accumulator this vertex shares (the UV seam duplicate).
    fn add_vertex(&mut self, pos: Vector3, uv: Vector2, shares_acc_with: Option<u32>) -> u32 {
        let idx = self.positions.len() as u32;
        self.positions.push(pos);
        self.uvs.push(uv);
        self.normal_acc.push(Vector3::ZERO);
        self.acc_of.push(match shares_acc_with {
            Some(other) => other as usize,
            None => idx as usize,
        });
        idx
    }

    fn add_triangle(&mut self, a: u32, b: u32, c: u32) {
        let pa = self.positions[a as usize];
        let pb = self.positions[b as usize];
        let pc = self.positions[c as usize];
        let face = (pb - pa).cross(pc - pa);
        self.normal_acc[self.acc_of[a as usize]] += face;
        self.normal_acc[self.acc_of[b as usize]] += face;
        self.normal_acc[self.acc_of[c as usize]] += face;
        self.indices
            .extend_from_slice(&[a as i32, b as i32, c as i32]);
    }

    /// Adds two triangles for the quad `p00-p01-p11-p10`. `flip` reverses the
    /// winding order, which is how an inward-facing surface can be emitted
    /// without hand-deriving the winding twice.
    fn add_quad(&mut self, v: [u32; 4], flip: bool) {
        let [p00, p01, p10, p11] = v;
        if !flip {
            self.add_triangle(p00, p10, p11);
            self.add_triangle(p00, p11, p01);
        } else {
            self.add_triangle(p00, p11, p10);
            self.add_triangle(p00, p01, p11);
        }
    }

    /// Normalizes the accumulated normals and commits everything to Godot as
    /// one indexed triangle surface.
    fn into_mesh(self, material: impl AsObjectArg<Material>) -> Option<Gd<ArrayMesh>> {
        let normals: Vec<Vector3> = self
            .normal_acc
            .iter()
            .map(|n| {
                if n.length_squared() > 1e-12 {
                    n.normalized()
                } else {
                    Vector3::UP
                }
            })
            .collect();

        // Slot ordinals of Godot's Mesh.ArrayType (stable engine constants):
        // VERTEX = 0, NORMAL = 1, COLOR = 3, TEX_UV = 4, INDEX = 12.
        const SLOT_VERTEX: usize = 0;
        const SLOT_NORMAL: usize = 1;
        const SLOT_COLOR: usize = 3;
        const SLOT_TEX_UV: usize = 4;
        const SLOT_INDEX: usize = 12;

        let mut arrays = Array::<Variant>::new();
        arrays.resize(13, &Variant::nil());
        arrays.set(
            SLOT_VERTEX,
            &PackedVector3Array::from(self.positions).to_variant(),
        );
        arrays.set(SLOT_NORMAL, &PackedVector3Array::from(normals).to_variant());
        if !self.colors.is_empty() {
            arrays.set(
                SLOT_COLOR,
                &PackedColorArray::from(self.colors.as_slice()).to_variant(),
            );
        }
        arrays.set(
            SLOT_TEX_UV,
            &PackedVector2Array::from(self.uvs).to_variant(),
        );
        arrays.set(
            SLOT_INDEX,
            &PackedInt32Array::from(self.indices).to_variant(),
        );

        let mut mesh = ArrayMesh::new_gd();
        mesh.add_surface_from_arrays(PrimitiveType::TRIANGLES, &arrays);
        mesh.surface_set_material(0, material);
        Some(mesh)
    }
}

/// Adds one lateral (side-wall) surface of revolution, from `y = -length/2` to
/// `y = +length/2`. `radius_at(t, angle)` gives the radius at normalized axial
/// position `t` in `[0, 1]` and local polar angle `angle` in radians. UVs map
/// `u` to axial position and `v` around the circumference; the seam column
/// duplicates the first column's positions (shared normal accumulator) with
/// `v = 1`.
fn add_lateral_surface(
    b: &mut SurfaceBuilder,
    length: f32,
    radial_segments: usize,
    rings: usize,
    flip: bool,
    radius_at: &dyn Fn(f32, f32) -> f32,
) {
    let cols = radial_segments + 1;
    let vert = |i: usize, j: usize| (i * cols + j) as u32;

    for i in 0..=rings {
        let t = i as f32 / rings as f32;
        let y = -length * 0.5 + length * t;
        for j in 0..=radial_segments {
            let angle = TAU * (j % radial_segments) as f32 / radial_segments as f32;
            let r = radius_at(t, angle);
            let pos = ring_point(r, y, angle);
            let uv = Vector2::new(t, j as f32 / radial_segments as f32);
            // The seam column (j == radial_segments) duplicates column 0's
            // position; share its normal accumulator so shading stays smooth.
            let shares = (j == radial_segments).then(|| vert(i, 0));
            b.add_vertex(pos, uv, shares);
        }
    }

    for i in 0..rings {
        for j in 0..radial_segments {
            let v00 = vert(i, j);
            let v01 = vert(i, j + 1);
            let v10 = vert(i + 1, j);
            let v11 = vert(i + 1, j + 1);
            b.add_quad([v00, v01, v10, v11], flip);
        }
    }
}

/// Adds a filled disk cap (triangle fan) at height `y`, reusing the lateral
/// surface's end-ring vertices so the rim shades smoothly into the wall.
/// `ring_start` is the first vertex index of the end ring (row `0` or `rings`).
fn add_disk_cap(
    b: &mut SurfaceBuilder,
    y: f32,
    radial_segments: usize,
    ring_start: u32,
    flip: bool,
) {
    let center = b.add_vertex(Vector3::new(0.0, y, 0.0), Vector2::new(0.5, 0.5), None);

    for j in 0..radial_segments {
        let rim0 = ring_start + j as u32;
        let rim1 = ring_start + ((j + 1) % radial_segments) as u32;
        if !flip {
            b.add_triangle(center, rim1, rim0);
        } else {
            b.add_triangle(center, rim0, rim1);
        }
    }
}

/// Builds a solid tube (lateral surface + two end caps), used for the blood
/// lumen: a solid volume whose radius narrows through the stenosis.
pub fn build_solid_tube(
    length: f32,
    radial_segments: usize,
    rings: usize,
    material: impl AsObjectArg<Material>,
    radius_at: impl Fn(f32, f32) -> f32,
) -> Option<Gd<ArrayMesh>> {
    let mut b = SurfaceBuilder::new();
    add_lateral_surface(&mut b, length, radial_segments, rings, false, &radius_at);
    let cols = radial_segments + 1;
    let bottom_ring = 0u32;
    let top_ring = (rings * cols) as u32;
    add_disk_cap(&mut b, -length * 0.5, radial_segments, bottom_ring, true);
    add_disk_cap(&mut b, length * 0.5, radial_segments, top_ring, false);
    b.into_mesh(material)
}

/// Builds a closed annular plaque volume: outer lateral surface, inner lateral
/// surface, and annular end caps. This gives the deposit visible thickness and
/// removes the "hollow film" look of the old single-sided shell. The inner
/// surface is intentionally back-face culled when viewed from outside the
/// lumen, and the end caps close the ring at the vessel ends.
///
/// `alpha_at(t, angle)` supplies a per-vertex alpha emitted as a white vertex
/// color (the material must enable `vertex_color_use_as_albedo`). This lets
/// the deposit be opaque on its raised lobes and translucent elsewhere, so the
/// blood flow stays visible through the thin lining.
pub fn build_plaque_volume(
    length: f32,
    radial_segments: usize,
    rings: usize,
    material: impl AsObjectArg<Material>,
    outer_radius_at: impl Fn(f32, f32) -> f32,
    inner_radius_at: impl Fn(f32, f32) -> f32,
    alpha_at: impl Fn(f32, f32) -> f32,
) -> Option<Gd<ArrayMesh>> {
    let mut b = SurfaceBuilder::new();
    let cols = radial_segments + 1;
    let n_rows = rings + 1;

    // Pre-compute radii for both surfaces; the closures are typically
    // expensive (value noise), so avoid calling them twice while building.
    let mut outer_r: Vec<f32> = Vec::with_capacity(n_rows * cols);
    let mut inner_r: Vec<f32> = Vec::with_capacity(n_rows * cols);
    let mut alphas: Vec<f32> = Vec::with_capacity(n_rows * cols);
    for i in 0..n_rows {
        let t = i as f32 / rings as f32;
        for j in 0..cols {
            let angle = TAU * (j % radial_segments) as f32 / radial_segments as f32;
            let out = outer_radius_at(t, angle);
            let inn = inner_radius_at(t, angle).min(out - 0.001).max(0.0);
            outer_r.push(out);
            inner_r.push(inn);
            alphas.push(alpha_at(t, angle).clamp(0.0, 1.0));
        }
    }

    // Outer lateral surface.
    let outer_base = 0u32;
    for i in 0..n_rows {
        let t = i as f32 / rings as f32;
        let y = -length * 0.5 + length * t;
        for j in 0..cols {
            let angle = TAU * (j % radial_segments) as f32 / radial_segments as f32;
            let r = outer_r[i * cols + j];
            let pos = ring_point(r, y, angle);
            let uv = Vector2::new(t, j as f32 / radial_segments as f32);
            let shares = (j == radial_segments).then(|| outer_base + (i * cols) as u32);
            let a = alphas[i * cols + j];
            b.colors.push(Color::from_rgba(1.0, 1.0, 1.0, a));
            b.add_vertex(pos, uv, shares);
        }
    }
    for i in 0..rings {
        for j in 0..radial_segments {
            let v00 = outer_base + (i * cols + j) as u32;
            let v01 = outer_base + (i * cols + j + 1) as u32;
            let v10 = outer_base + ((i + 1) * cols + j) as u32;
            let v11 = outer_base + ((i + 1) * cols + j + 1) as u32;
            b.add_quad([v00, v01, v10, v11], false);
        }
    }

    // Inner lateral surface (flip so normals point inward, i.e. toward the
    // lumen); it will be back-face culled when viewed from outside the vessel.
    let inner_base = b.positions.len() as u32;
    for i in 0..n_rows {
        let t = i as f32 / rings as f32;
        let y = -length * 0.5 + length * t;
        for j in 0..cols {
            let angle = TAU * (j % radial_segments) as f32 / radial_segments as f32;
            let r = inner_r[i * cols + j];
            let pos = ring_point(r, y, angle);
            let uv = Vector2::new(t, j as f32 / radial_segments as f32);
            let shares = (j == radial_segments).then(|| inner_base + (i * cols) as u32);
            let a = alphas[i * cols + j];
            b.colors.push(Color::from_rgba(1.0, 1.0, 1.0, a));
            b.add_vertex(pos, uv, shares);
        }
    }
    for i in 0..rings {
        for j in 0..radial_segments {
            let v00 = inner_base + (i * cols + j) as u32;
            let v01 = inner_base + (i * cols + j + 1) as u32;
            let v10 = inner_base + ((i + 1) * cols + j) as u32;
            let v11 = inner_base + ((i + 1) * cols + j + 1) as u32;
            b.add_quad([v00, v01, v10, v11], true);
        }
    }

    // Annular end caps. Connect the outer and inner rings at each end.
    // Bottom cap: normal points down (-Y). Top cap: normal up (+Y).
    for j in 0..radial_segments {
        let o0 = outer_base + j as u32;
        let o1 = outer_base + (j + 1) as u32;
        let i0 = inner_base + j as u32;
        let i1 = inner_base + (j + 1) as u32;
        if outer_r[j] > inner_r[j] + 1e-5 && outer_r[j + 1] > inner_r[j + 1] + 1e-5 {
            b.add_quad([o0, o1, i0, i1], true);
        }
    }
    let last = rings * cols;
    for j in 0..radial_segments {
        let o0 = outer_base + (last + j) as u32;
        let o1 = outer_base + (last + j + 1) as u32;
        let i0 = inner_base + (last + j) as u32;
        let i1 = inner_base + (last + j + 1) as u32;
        if outer_r[last + j] > inner_r[last + j] + 1e-5
            && outer_r[last + j + 1] > inner_r[last + j + 1] + 1e-5
        {
            b.add_quad([o0, o1, i0, i1], false);
        }
    }

    b.into_mesh(material)
}

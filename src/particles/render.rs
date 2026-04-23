use bevy::prelude::*;

use super::definitions::ParticleShape;

/// Shared mesh handles for CPU-rendered emissive particles.
/// Only used for ParticleDefs that have lights (hanabi handles non-emissive).
#[derive(Resource)]
pub struct ParticleMeshes {
    /// A 1x1 unit quad centered at origin.
    pub quad: Handle<Mesh>,
    pub circle: Handle<Mesh>,
    pub triangle: Handle<Mesh>,
    pub diamond: Handle<Mesh>,
    pub hexagon: Handle<Mesh>,
    pub star: Handle<Mesh>,
}

impl Default for ParticleMeshes {
    fn default() -> Self {
        Self {
            quad: Handle::default(),
            circle: Handle::default(),
            triangle: Handle::default(),
            diamond: Handle::default(),
            hexagon: Handle::default(),
            star: Handle::default(),
        }
    }
}

impl ParticleMeshes {
    /// Get the mesh handle for a given particle shape.
    pub fn for_shape(&self, shape: ParticleShape) -> Handle<Mesh> {
        match shape {
            ParticleShape::Quad => self.quad.clone(),
            ParticleShape::Circle => self.circle.clone(),
            ParticleShape::Triangle => self.triangle.clone(),
            ParticleShape::Diamond => self.diamond.clone(),
            ParticleShape::Hexagon => self.hexagon.clone(),
            ParticleShape::Star => self.star.clone(),
        }
    }
}

/// Global budget for CPU-side particle lights.
#[derive(Resource)]
pub struct ParticleLightBudget {
    pub max: u32,
    pub current: u32,
}

impl Default for ParticleLightBudget {
    fn default() -> Self {
        Self {
            max: 256,
            current: 0,
        }
    }
}

impl ParticleLightBudget {
    pub fn try_allocate(&mut self) -> bool {
        if self.current < self.max {
            self.current += 1;
            true
        } else {
            false
        }
    }

    pub fn release(&mut self) {
        self.current = self.current.saturating_sub(1);
    }
}

/// Creates the shared particle meshes at startup.
pub fn setup_particle_meshes(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    let quad = meshes.add(Rectangle::new(1.0, 1.0));
    let circle = meshes.add(Circle::new(0.5));
    let triangle = meshes.add(Triangle2d::new(
        Vec2::new(0.0, 0.5),
        Vec2::new(-0.433, -0.25),
        Vec2::new(0.433, -0.25),
    ));
    let diamond = meshes.add(RegularPolygon::new(0.5, 4));
    let hexagon = meshes.add(RegularPolygon::new(0.5, 6));
    let star = meshes.add(build_star_mesh(5, 0.5, 0.2));

    commands.insert_resource(ParticleMeshes {
        quad,
        circle,
        triangle,
        diamond,
        hexagon,
        star,
    });
}

/// Build a 2D star mesh with `points` tips, outer radius `r_out`, inner radius `r_in`.
fn build_star_mesh(points: u32, r_out: f32, r_in: f32) -> Mesh {
    use bevy::mesh::{Indices, PrimitiveTopology};
    use bevy::asset::RenderAssetUsages;
    use std::f32::consts::TAU;

    let n = points * 2;
    let mut positions = vec![[0.0_f32, 0.0, 0.0]]; // center
    let mut indices = Vec::new();

    for i in 0..n {
        let angle = (i as f32 / n as f32) * TAU - TAU / 4.0;
        let r = if i % 2 == 0 { r_out } else { r_in };
        positions.push([angle.cos() * r, angle.sin() * r, 0.0]);
    }

    for i in 1..=n {
        let next = if i == n { 1 } else { i + 1 };
        indices.extend_from_slice(&[0, i, next]);
    }

    let normals = vec![[0.0, 0.0, 1.0]; positions.len()];
    let uvs: Vec<[f32; 2]> = positions.iter().map(|p| [p[0] + 0.5, 0.5 - p[1]]).collect();

    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

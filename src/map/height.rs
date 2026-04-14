/// Height layers for z-ordering. Transform.z = layer base + y-sort offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeightLayer {
    Ground = 0,
    Objects = 10,
    Bridge = 20,
    Overhead = 30,
}

impl HeightLayer {
    pub fn base_z(self) -> f32 {
        self as i32 as f32
    }
}

/// Compute z value from height layer and y position.
/// Lower y on screen (higher world y) → higher z so it renders behind.
pub fn z_from_y(layer: HeightLayer, y: f32, max_y: f32) -> f32 {
    layer.base_z() + (max_y - y) / (max_y * 2.0)
}

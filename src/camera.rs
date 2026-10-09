//! Orbit camera with Maya-style navigation. See Painter-docs/reference/keyboard-shortcuts.md.

use glam::{Mat4, Vec2, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Camera {
    pub target: Vec3,
    /// Radians around +Y.
    pub yaw: f32,
    /// Radians above the horizon.
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            yaw: 0.6,
            pitch: 0.35,
            distance: 5.0,
            fov_y: 40f32.to_radians(),
        }
    }
}

impl Camera {
    pub fn eye(&self) -> Vec3 {
        self.target + self.forward() * -self.distance
    }

    /// Direction the camera looks in.
    pub fn forward(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        -Vec3::new(sy * cp, sp, cy * cp)
    }

    pub fn right(&self) -> Vec3 {
        self.forward().cross(Vec3::Y).normalize_or(Vec3::X)
    }

    pub fn up(&self) -> Vec3 {
        self.right().cross(self.forward())
    }

    pub fn view(&self) -> Mat4 {
        look_at_rh(self.eye(), self.target, Vec3::Y)
    }

    pub fn near_far(&self, scene_radius: f32) -> (f32, f32) {
        let far = self.distance + scene_radius * 4.0 + 1.0;
        let near = (self.distance - scene_radius * 2.0).max(far * 1e-4).max(1e-4);
        (near, far)
    }

    pub fn proj(&self, aspect: f32, scene_radius: f32) -> Mat4 {
        let (near, far) = self.near_far(scene_radius);
        perspective_rh(self.fov_y, aspect.max(1e-3), near, far)
    }

    pub fn orbit(&mut self, delta_px: Vec2) {
        self.yaw -= delta_px.x * 0.008;
        self.pitch = (self.pitch + delta_px.y * 0.008).clamp(-1.55, 1.55);
    }

    /// `viewport_h` in pixels, so panning keeps the point under the cursor fixed.
    pub fn pan(&mut self, delta_px: Vec2, viewport_h: f32) {
        let world_per_px = 2.0 * self.distance * (self.fov_y * 0.5).tan() / viewport_h.max(1.0);
        self.target += (-self.right() * delta_px.x + self.up() * delta_px.y) * world_per_px;
    }

    pub fn dolly(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(1e-3, 1e6);
    }

    /// Frames a sphere so it fills the view.
    pub fn frame(&mut self, center: Vec3, radius: f32) {
        self.target = center;
        self.distance = radius / (self.fov_y * 0.5).sin() * 1.1;
    }
}

/// Right-handed view matrix (camera looks down -Z).
pub fn look_at_rh(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
    let f = (target - eye).normalize();
    let s = f.cross(up).normalize_or(Vec3::X);
    let u = s.cross(f);
    Mat4::from_cols(
        glam::Vec4::new(s.x, u.x, -f.x, 0.0),
        glam::Vec4::new(s.y, u.y, -f.y, 0.0),
        glam::Vec4::new(s.z, u.z, -f.z, 0.0),
        glam::Vec4::new(-s.dot(eye), -u.dot(eye), f.dot(eye), 1.0),
    )
}

/// Right-handed perspective projection with depth in [0, 1].
pub fn perspective_rh(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let h = 1.0 / (fov_y * 0.5).tan();
    let r = far / (near - far);
    Mat4::from_cols(
        glam::Vec4::new(h / aspect, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, h, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, r, -1.0),
        glam::Vec4::new(0.0, 0.0, r * near, 0.0),
    )
}

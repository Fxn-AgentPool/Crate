//! Orbit camera with Maya-style navigation. See docs/reference/keyboard-shortcuts.md.

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

/// The UV view's pan and zoom. See docs/reference/uv-view.md#navigation.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UvView {
    /// UDIM U and V at the view's center.
    pub center: Vec2,
    /// Height of one tile in view heights.
    pub zoom: f32,
    /// Show the final texture between islands (docs/reference/2d-paint.md#display).
    #[serde(default = "yes")]
    pub gaps: bool,
}

fn yes() -> bool {
    true
}

impl Default for UvView {
    fn default() -> Self {
        Self { center: Vec2::splat(0.5), zoom: 0.9, gaps: true }
    }
}

impl UvView {
    pub const MIN_ZOOM: f32 = 0.05;
    pub const MAX_ZOOM: f32 = 64.0;

    /// Screen pixels per UV unit in a view `size` pixels large.
    pub fn scale(&self, size: Vec2) -> f32 {
        self.zoom * size.y.max(1.0)
    }

    /// Screen position (pixels, y down) of a UDIM coordinate (V up).
    pub fn to_screen(self, uv: Vec2, size: Vec2) -> Vec2 {
        let d = (uv - self.center) * self.scale(size);
        Vec2::new(size.x * 0.5 + d.x, size.y * 0.5 - d.y)
    }

    pub fn to_uv(self, px: Vec2, size: Vec2) -> Vec2 {
        let d = Vec2::new(px.x - size.x * 0.5, size.y * 0.5 - px.y) / self.scale(size);
        self.center + d
    }

    /// Maps (u, v, 0) to clip space, for drawing the mesh at its UVs.
    pub fn view_proj(&self, size: Vec2) -> Mat4 {
        let s = self.scale(size);
        let (sx, sy) = (2.0 * s / size.x.max(1.0), 2.0 * s / size.y.max(1.0));
        Mat4::from_cols_array(&[sx, 0.0, 0.0, 0.0, 0.0, sy, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -self.center.x * sx, -self.center.y * sy, 0.5, 1.0])
    }

    /// Moves the view by a pointer drag.
    pub fn pan(&mut self, delta_px: Vec2, size: Vec2) {
        self.center -= Vec2::new(delta_px.x, -delta_px.y) / self.scale(size);
    }

    /// Zooms by `factor`, keeping the point under `px` fixed.
    pub fn zoom_at(&mut self, factor: f32, px: Vec2, size: Vec2) {
        let before = self.to_uv(px, size);
        self.zoom = (self.zoom * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        let after = self.to_uv(px, size);
        self.center += before - after;
    }

    /// Fits the used UDIM tiles, with a 5 % margin.
    pub fn frame(&mut self, tiles: &[u32], size: Vec2) {
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for t in tiles {
            let k = t.saturating_sub(1001);
            let o = Vec2::new((k % 10) as f32, (k / 10) as f32);
            lo = lo.min(o);
            hi = hi.max(o + Vec2::ONE);
        }
        if tiles.is_empty() {
            (lo, hi) = (Vec2::ZERO, Vec2::ONE);
        }
        self.center = (lo + hi) * 0.5;
        let ext = (hi - lo) * 1.05;
        let aspect = size.x.max(1.0) / size.y.max(1.0);
        self.zoom = (1.0 / ext.y).min(aspect / ext.x).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
    }
}

/// The viewport's layout. See docs/reference/uv-view.md#layouts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Layout {
    #[default]
    #[serde(rename = "3d")]
    ThreeD,
    #[serde(rename = "split")]
    Split,
    #[serde(rename = "uv")]
    Uv,
}

/// What a viewport pane shows: the mesh through the 3D camera, or laid out at its UVs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum View {
    Persp(Camera),
    Uv(UvView),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uv_view_maps_both_ways() {
        let size = Vec2::new(800.0, 400.0);
        let mut v = UvView { center: Vec2::new(1.5, 0.5), zoom: 1.0, gaps: true };
        // The center is in the middle; one tile is the view's height; V points up.
        assert_eq!(v.to_screen(Vec2::new(1.5, 0.5), size), Vec2::new(400.0, 200.0));
        assert_eq!(v.to_screen(Vec2::new(1.0, 1.0), size), Vec2::new(200.0, 0.0));
        let p = Vec2::new(123.0, 321.0);
        assert!((v.to_screen(v.to_uv(p, size), size) - p).length() < 1e-3);
        // Clip space matches the screen mapping.
        let c = v.view_proj(size) * glam::Vec4::new(1.0, 1.0, 0.0, 1.0);
        assert!((c.x - (-0.5)).abs() < 1e-6 && (c.y - 1.0).abs() < 1e-6);
        // Zooming about a point keeps it under the pointer.
        let uv = v.to_uv(p, size);
        v.zoom_at(3.0, p, size);
        assert!((v.to_uv(p, size) - uv).length() < 1e-5);
        // Two tiles side by side, framed in a wide view: limited by the height.
        v.frame(&[1001, 1002], size);
        assert_eq!(v.center, Vec2::new(1.0, 0.5));
        assert!((v.zoom - 1.0 / 1.05).abs() < 1e-6);
    }
}

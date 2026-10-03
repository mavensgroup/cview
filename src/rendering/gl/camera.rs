// src/rendering/gl/camera.rs
//
// Orthographic trackball camera. `rotation` uses the main view's convention
// (`ViewState::rotation`: screen y down, larger z farther), so a window can
// start from the main view's orientation and dragging feels the same.

use nalgebra::{Matrix4, Orthographic3, UnitQuaternion, Vector3};

#[derive(Clone, Debug)]
pub struct Camera {
    pub rotation: UnitQuaternion<f64>,
    /// Point the view rotates about, Å.
    pub center: Vector3<f64>,
    /// Radius of a sphere holding everything to show, Å. Sets the default
    /// framing and the depth range.
    pub radius: f64,
    pub zoom: f64,
}

impl Camera {
    pub fn new(center: Vector3<f64>, radius: f64, rotation: UnitQuaternion<f64>) -> Self {
        Self {
            rotation,
            center,
            radius: radius.max(1.0),
            zoom: 1.0,
        }
    }

    /// Screen-space trackball increment, identical to
    /// `ViewState::apply_screen_rotation_deg`.
    pub fn rotate_screen_deg(&mut self, yaw_deg: f64, pitch_deg: f64) {
        let yaw = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), yaw_deg.to_radians());
        let pitch = UnitQuaternion::from_axis_angle(&Vector3::x_axis(), pitch_deg.to_radians());
        self.rotation = yaw * pitch * self.rotation;
    }

    pub fn zoom_by(&mut self, factor: f64) {
        self.zoom = (self.zoom * factor).clamp(0.05, 50.0);
    }

    /// Set the zoom so `points` (padded by `pad`) fill about 80% of a canvas
    /// with this aspect ratio, as the main view frames a structure.
    pub fn fit(&mut self, points: &[Vector3<f64>], pad: f64, aspect: f64) {
        let rot = self.rotation.to_rotation_matrix();
        let (mut hx, mut hy) = (1e-3_f64, 1e-3_f64);
        for p in points {
            let v = rot * (p - self.center);
            hx = hx.max(v.x.abs() + pad);
            hy = hy.max(v.y.abs() + pad);
        }
        let half = self.radius * 1.08;
        // Visible half-extents at zoom 1, matching `projection`.
        let (vw, vh) = if aspect >= 1.0 { (half * aspect, half) } else { (half, half / aspect) };
        self.zoom = (0.8 * (vw / hx).min(vh / hy)).clamp(0.05, 50.0);
    }

    /// World → view space. The main view looks along +z with y down; GL
    /// looks along −z with y up, which is the same camera turned 180° about x.
    pub fn view(&self) -> Matrix4<f32> {
        let flip = UnitQuaternion::from_axis_angle(&Vector3::x_axis(), std::f64::consts::PI);
        let rot = (flip * self.rotation).to_homogeneous();
        let back = Matrix4::new_translation(&Vector3::new(0.0, 0.0, -3.0 * self.radius));
        let to_origin = Matrix4::new_translation(&-self.center);
        (back * rot * to_origin).cast()
    }

    /// Half-width and half-height (Å, view space) visible on a
    /// `width`×`height` canvas.
    pub fn half_extents(&self, width: f64, height: f64) -> (f64, f64) {
        let aspect = (width / height.max(1.0)).max(1e-3);
        let half = self.radius * 1.08 / self.zoom;
        if aspect >= 1.0 {
            (half * aspect, half)
        } else {
            (half, half / aspect)
        }
    }

    pub fn projection(&self, width: f64, height: f64) -> Matrix4<f32> {
        let (hw, hh) = self.half_extents(width, height);
        self.projection_window(-hw, hw, -hh, hh)
    }

    /// Orthographic projection of an arbitrary window of view space (Å);
    /// exports use it for sub-regions and tiles.
    pub fn projection_window(&self, left: f64, right: f64, bottom: f64, top: f64) -> Matrix4<f32> {
        Orthographic3::new(left, right, bottom, top, 0.01 * self.radius, 6.0 * self.radius)
            .to_homogeneous()
            .cast()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centre_projects_to_screen_centre_inside_depth_range() {
        let c = Camera::new(Vector3::new(1.0, 2.0, 3.0), 5.0, UnitQuaternion::identity());
        let p = c.projection(800.0, 600.0) * c.view() * nalgebra::Vector4::new(1.0f32, 2.0, 3.0, 1.0);
        assert!(p.x.abs() < 1e-5 && p.y.abs() < 1e-5);
        assert!(p.z > -1.0 && p.z < 1.0);
    }

    #[test]
    fn fit_fills_the_canvas() {
        let mut c = Camera::new(Vector3::zeros(), 10.0, UnitQuaternion::identity());
        c.fit(&[Vector3::new(2.0, 1.0, 0.0), Vector3::new(-2.0, -1.0, 0.0)], 0.0, 1.0);
        // Widest point (x = 2) lands at 80% of the half-width.
        let p = c.projection(100.0, 100.0) * c.view() * nalgebra::Vector4::new(2.0f32, 0.0, 0.0, 1.0);
        assert!((p.x - 0.8).abs() < 1e-4, "{}", p.x);
    }

    #[test]
    fn screen_y_points_up_like_the_main_view_points_down() {
        // Main view: +y world appears lower on screen (y down) with identity
        // rotation. In GL clip space that is negative y.
        let c = Camera::new(Vector3::zeros(), 5.0, UnitQuaternion::identity());
        let p = c.projection(100.0, 100.0) * c.view() * nalgebra::Vector4::new(0.0f32, 1.0, 0.0, 1.0);
        assert!(p.y < 0.0);
    }
}

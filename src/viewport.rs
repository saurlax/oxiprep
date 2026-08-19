use crate::document::Document;
use crate::gpu::GpuRenderer;
use cadrum::DVec3;
use eframe::egui::{self, Color32, PointerButton, Pos2, Rect, Response, Sense, Stroke, Ui};
use eframe::egui_wgpu::RenderState;

pub const FOV_Y: f64 = 40.0_f64.to_radians();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipAxis {
    X,
    Y,
    Z,
}

/// What the viewport draws. Independent toggles; clip is a view filter, not a split.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayOptions {
    pub faces: bool,
    pub edges: bool,
    pub mesh: bool,
    pub vertices: bool,
    pub clip: bool,
    pub clip_axis: ClipAxis,
    pub clip_t: f32,
    pub clip_flip: bool,
}

impl Default for DisplayOptions {
    fn default() -> Self {
        Self {
            faces: true,
            edges: true,
            mesh: false,
            vertices: false,
            clip: false,
            clip_axis: ClipAxis::Z,
            clip_t: 0.5,
            clip_flip: false,
        }
    }
}

pub struct Viewport {
    pub camera: Camera,
    pub display: DisplayOptions,
    gpu: Option<GpuRenderer>,
}

pub struct Camera {
    pub target: DVec3,
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
}

impl Viewport {
    pub fn new(render_state: Option<RenderState>) -> Self {
        Self {
            camera: Camera::isometric(),
            display: DisplayOptions::default(),
            gpu: render_state.map(GpuRenderer::new),
        }
    }

    pub fn fit(&mut self, bbox: [DVec3; 2]) {
        self.camera.fit(bbox);
    }

    pub fn look_along(&mut self, dir: DVec3) {
        self.camera.look_along(dir);
    }

    pub fn look_isometric(&mut self) {
        self.camera.look_isometric();
    }

    pub fn show(&mut self, ui: &mut Ui, document: &Document) {
        ui.vertical(|ui| {
            self.display_bar(ui);
            let available = ui.available_size();
            if available.x < 1.0 || available.y < 1.0 {
                return;
            }
            let bg = ui.visuals().extreme_bg_color;
            let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
            self.handle_input(&response, ui, document);
            let clip = clip_plane(document, &self.display);
            if let Some(gpu) = self.gpu.as_mut() {
                gpu.show(
                    ui,
                    response.rect,
                    &self.camera,
                    document,
                    &self.display,
                    clip,
                    bg,
                );
            } else {
                painter.rect_filled(response.rect, 0.0, bg);
            }
            paint_gnomon(&painter, response.rect, &self.camera);
        });
    }

    fn display_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.toggle_value(&mut self.display.faces, "Faces");
            ui.toggle_value(&mut self.display.edges, "Edges");
            ui.toggle_value(&mut self.display.mesh, "Mesh");
            ui.toggle_value(&mut self.display.vertices, "Vertices");
            ui.separator();
            ui.toggle_value(&mut self.display.clip, "Clip");
        });
        if self.display.clip {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.display.clip_axis, ClipAxis::X, "X");
                ui.selectable_value(&mut self.display.clip_axis, ClipAxis::Y, "Y");
                ui.selectable_value(&mut self.display.clip_axis, ClipAxis::Z, "Z");
                ui.add(egui::Slider::new(&mut self.display.clip_t, 0.0..=1.0).show_value(false));
                ui.toggle_value(&mut self.display.clip_flip, "Flip");
            });
        }
    }

    fn handle_input(&mut self, response: &Response, ui: &Ui, document: &Document) {
        if response.double_clicked() {
            if let Some(bbox) = document.selection_bbox().or_else(|| document.bbox()) {
                self.camera.fit(bbox);
            }
        }

        if response.dragged_by(PointerButton::Primary) && !ui.input(|i| i.modifiers.shift) {
            let delta = response.drag_delta();
            self.camera.orbit(delta.x as f64, delta.y as f64);
        } else if response.dragged_by(PointerButton::Middle)
            || response.dragged_by(PointerButton::Secondary)
            || (response.dragged_by(PointerButton::Primary) && ui.input(|i| i.modifiers.shift))
        {
            let delta = response.drag_delta();
            self.camera.pan(
                delta.x as f64,
                delta.y as f64,
                response.rect.height() as f64,
            );
        }

        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.camera.zoom(scroll as f64);
            }
        }
    }
}

fn paint_gnomon(painter: &egui::Painter, rect: Rect, camera: &Camera) {
    let dir = camera.view_dir();
    let Some(v) = (DVec3::Z - dir * DVec3::Z.dot(dir)).try_normalize() else {
        return;
    };
    let u = v.cross(dir);
    let origin = Pos2::new(rect.left() + 36.0, rect.bottom() - 36.0);
    let len = 22.0_f32;
    let axes = [
        (DVec3::X, Color32::from_rgb(0xC0, 0x39, 0x2B), "X"),
        (DVec3::Y, Color32::from_rgb(0x27, 0xAE, 0x60), "Y"),
        (DVec3::Z, Color32::from_rgb(0x29, 0x80, 0xB9), "Z"),
    ];
    for (axis, color, label) in axes {
        let px = axis.dot(u) as f32;
        let py = axis.dot(v) as f32;
        let tip = Pos2::new(origin.x + px * len, origin.y - py * len);
        painter.line_segment([origin, tip], Stroke::new(1.5, color));
        painter.text(
            tip + egui::vec2(px.signum() * 6.0, -py.signum() * 6.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(11.0),
            color,
        );
    }
}

impl Camera {
    pub fn isometric() -> Self {
        Self {
            target: DVec3::ZERO,
            yaw: std::f64::consts::FRAC_PI_4,
            pitch: 0.615_479_708_670_387_3,
            distance: 20.0,
        }
    }

    pub fn look_isometric(&mut self) {
        self.yaw = std::f64::consts::FRAC_PI_4;
        self.pitch = 0.615_479_708_670_387_3;
    }

    pub fn look_along(&mut self, dir: DVec3) {
        let Some(dir) = dir.try_normalize() else {
            return;
        };
        let limit = std::f64::consts::FRAC_PI_2 - 0.05;
        self.pitch = dir.z.asin().clamp(-limit, limit);
        let cp = self.pitch.cos();
        if cp.abs() > 1e-6 {
            self.yaw = dir.y.atan2(dir.x);
        }
    }

    pub fn view_dir(&self) -> DVec3 {
        let cp = self.pitch.cos();
        DVec3::new(self.yaw.cos() * cp, self.yaw.sin() * cp, self.pitch.sin())
    }

    pub fn orbit(&mut self, dx: f64, dy: f64) {
        self.yaw -= dx * 0.008;
        self.pitch += dy * 0.008;
        let limit = std::f64::consts::FRAC_PI_2 - 0.05;
        self.pitch = self.pitch.clamp(-limit, limit);
    }

    pub fn pan(&mut self, dx: f64, dy: f64, height_px: f64) {
        if height_px <= 1.0 {
            return;
        }
        let dir = self.view_dir();
        let Some(v) = (DVec3::Z - dir * DVec3::Z.dot(dir)).try_normalize() else {
            return;
        };
        let u = v.cross(dir);
        let world_h = 2.0 * self.distance * (FOV_Y * 0.5).tan();
        let scale = world_h / height_px;
        self.target -= u * dx * scale;
        self.target += v * dy * scale;
    }

    pub fn zoom(&mut self, scroll: f64) {
        let factor = (1.0 - scroll * 0.001).clamp(0.5, 1.5);
        self.distance = (self.distance * factor).clamp(1e-4, 1e8);
    }

    pub fn fit(&mut self, bbox: [DVec3; 2]) {
        let center = (bbox[0] + bbox[1]) * 0.5;
        let radius = (bbox[1] - bbox[0]).length().max(1e-6) * 0.5;
        self.target = center;
        let tan_half = (FOV_Y * 0.5).tan();
        self.distance = (radius / tan_half * 1.2).max(radius * 1.05).max(1e-3);
    }
}

/// Plane `n·p + d = 0`. Fragments with `n·p + d < 0` are discarded.
pub fn clip_plane(document: &Document, display: &DisplayOptions) -> Option<[f32; 4]> {
    if !display.clip {
        return None;
    }
    let bbox = document.bbox()?;
    let t = display.clip_t.clamp(0.0, 1.0) as f64;
    let (mut n, origin) = match display.clip_axis {
        ClipAxis::X => (DVec3::X, bbox[0].x * (1.0 - t) + bbox[1].x * t),
        ClipAxis::Y => (DVec3::Y, bbox[0].y * (1.0 - t) + bbox[1].y * t),
        ClipAxis::Z => (DVec3::Z, bbox[0].z * (1.0 - t) + bbox[1].z * t),
    };
    let mut d = -origin;
    if display.clip_flip {
        n = -n;
        d = -d;
    }
    Some([n.x as f32, n.y as f32, n.z as f32, d as f32])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn look_along_plus_x() {
        let mut camera = Camera::isometric();
        camera.look_along(DVec3::X);
        let dir = camera.view_dir();
        assert!((dir - DVec3::X).length() < 1e-6);
    }

    #[test]
    fn clip_disabled_without_models() {
        let document = Document::new();
        let display = DisplayOptions {
            clip: true,
            ..DisplayOptions::default()
        };
        assert!(clip_plane(&document, &display).is_none());
    }
}

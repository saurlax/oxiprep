use crate::document::Document;
use crate::gpu::GpuRenderer;
use crate::pick;
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PickMode {
    Off,
    #[default]
    Body,
    Face,
    Edge,
    Vertex,
    Cell,
    Node,
}

impl PickMode {
    fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Body => "Body",
            Self::Face => "Face",
            Self::Edge => "Edge",
            Self::Vertex => "Vertex",
            Self::Cell => "Cell",
            Self::Node => "Node",
        }
    }
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
    pub pick: PickMode,
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
            pick: PickMode::Body,
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

    pub fn show(&mut self, ui: &mut Ui, document: &mut Document) {
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
        ui.horizontal(|ui| {
            ui.label("Select");
            for mode in [
                PickMode::Off,
                PickMode::Body,
                PickMode::Face,
                PickMode::Edge,
                PickMode::Vertex,
                PickMode::Cell,
                PickMode::Node,
            ] {
                ui.selectable_value(&mut self.pick, mode, mode.label());
            }
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

    fn handle_input(&mut self, response: &Response, ui: &Ui, document: &mut Document) {
        if response.double_clicked() {
            if let Some(bbox) = document.selection_bbox().or_else(|| document.bbox()) {
                self.camera.fit(bbox);
            }
        }

        if response.clicked() && self.pick != PickMode::Off {
            if let Some(pos) = response.interact_pointer_pos() {
                let clip = clip_plane(document, &self.display);
                let hit = pick::pick(document, &self.camera, response.rect, pos, self.pick, clip);
                let (add, toggle) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
                pick::apply_click(document, hit, add, toggle);
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

    pub fn eye(&self) -> DVec3 {
        self.target + self.view_dir() * self.distance
    }

    pub fn view_proj(&self, aspect: f32) -> ([[f32; 4]; 4], [f32; 3]) {
        let dir = self.view_dir();
        let eye = self.eye();
        let up = {
            let raw = DVec3::Z;
            (raw - dir * raw.dot(dir)).normalize_or_zero()
        };
        let view = look_at_rh(dvec3(eye), dvec3(self.target), dvec3(up));
        let znear = (self.distance * 0.01).max(1e-4) as f32;
        let zfar = (self.distance * 100.0).max(f64::from(znear) * 10.0) as f32;
        let proj = perspective_rh(FOV_Y as f32, aspect.max(1e-6), znear, zfar);
        (mat4_mul(proj, view), dvec3(dir))
    }

    pub fn ray(&self, pos: Pos2, rect: Rect) -> Option<(DVec3, DVec3)> {
        if rect.width() < 1.0 || rect.height() < 1.0 {
            return None;
        }
        let aspect = rect.width() / rect.height();
        let (vp, _) = self.view_proj(aspect);
        let inv = mat4_inverse(vp)?;
        let ndc_x = (pos.x - rect.left()) / rect.width() * 2.0 - 1.0;
        let ndc_y = 1.0 - (pos.y - rect.top()) / rect.height() * 2.0;
        let near = transform_point(inv, [ndc_x, ndc_y, -1.0])?;
        let far = transform_point(inv, [ndc_x, ndc_y, 1.0])?;
        let origin = DVec3::new(near[0] as f64, near[1] as f64, near[2] as f64);
        let dest = DVec3::new(far[0] as f64, far[1] as f64, far[2] as f64);
        let dir = (dest - origin).try_normalize()?;
        Some((origin, dir))
    }

    pub fn project(&self, world: DVec3, rect: Rect) -> Option<Pos2> {
        if rect.width() < 1.0 || rect.height() < 1.0 {
            return None;
        }
        let aspect = rect.width() / rect.height();
        let (vp, _) = self.view_proj(aspect);
        let clip = mat4_mul_vec4(vp, [world.x as f32, world.y as f32, world.z as f32, 1.0]);
        if clip[3].abs() < 1e-8 {
            return None;
        }
        let ndc_x = clip[0] / clip[3];
        let ndc_y = clip[1] / clip[3];
        let ndc_z = clip[2] / clip[3];
        if !(-1.2..=1.2).contains(&ndc_x) || !(-1.2..=1.2).contains(&ndc_y) || ndc_z > 1.0 {
            return None;
        }
        Some(Pos2::new(
            rect.left() + (ndc_x + 1.0) * 0.5 * rect.width(),
            rect.top() + (1.0 - ndc_y) * 0.5 * rect.height(),
        ))
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

    #[test]
    fn project_and_ray_agree_at_target() {
        let mut camera = Camera::isometric();
        camera.target = DVec3::splat(0.5);
        camera.look_along(DVec3::X);
        camera.distance = 4.0;
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 400.0));
        let p = camera.project(camera.target, rect).unwrap();
        assert!((p.x - 200.0).abs() < 2.0);
        assert!((p.y - 200.0).abs() < 2.0);
        let (origin, dir) = camera.ray(Pos2::new(200.0, 200.0), rect).unwrap();
        let t = (camera.target - origin).dot(dir);
        let closest = origin + dir * t;
        assert!((closest - camera.target).length() < 0.05);
    }
}

fn dvec3(v: DVec3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}

fn look_at_rh(eye: [f32; 3], center: [f32; 3], up: [f32; 3]) -> [[f32; 4]; 4] {
    let f = normalize(sub(center, eye));
    let s = normalize(cross(f, up));
    let u = cross(s, f);
    [
        [s[0], u[0], -f[0], 0.0],
        [s[1], u[1], -f[1], 0.0],
        [s[2], u[2], -f[2], 0.0],
        [-dot(s, eye), -dot(u, eye), dot(f, eye), 1.0],
    ]
}

fn perspective_rh(fov_y: f32, aspect: f32, znear: f32, zfar: f32) -> [[f32; 4]; 4] {
    let f = 1.0 / (fov_y * 0.5).tan();
    let r = zfar / (znear - zfar);
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, r, -1.0],
        [0.0, 0.0, r * znear, 0.0],
    ]
}

fn mat4_mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0f32; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            out[col][row] = a[0][row] * b[col][0]
                + a[1][row] * b[col][1]
                + a[2][row] * b[col][2]
                + a[3][row] * b[col][3];
        }
    }
    out
}

fn mat4_mul_vec4(m: [[f32; 4]; 4], v: [f32; 4]) -> [f32; 4] {
    [
        m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2] + m[3][0] * v[3],
        m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2] + m[3][1] * v[3],
        m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2] + m[3][2] * v[3],
        m[0][3] * v[0] + m[1][3] * v[1] + m[2][3] * v[2] + m[3][3] * v[3],
    ]
}

fn transform_point(m: [[f32; 4]; 4], p: [f32; 3]) -> Option<[f32; 3]> {
    let c = mat4_mul_vec4(m, [p[0], p[1], p[2], 1.0]);
    if c[3].abs() < 1e-8 {
        return None;
    }
    Some([c[0] / c[3], c[1] / c[3], c[2] / c[3]])
}

fn mat4_inverse(m: [[f32; 4]; 4]) -> Option<[[f32; 4]; 4]> {
    let mut a = [[0.0f32; 4]; 4];
    let mut inv = [[0.0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            a[i][j] = m[j][i];
        }
        inv[i][i] = 1.0;
    }
    for i in 0..4 {
        let mut pivot = i;
        for r in i + 1..4 {
            if a[r][i].abs() > a[pivot][i].abs() {
                pivot = r;
            }
        }
        if a[pivot][i].abs() < 1e-8 {
            return None;
        }
        a.swap(i, pivot);
        inv.swap(i, pivot);
        let diag = a[i][i];
        for c in 0..4 {
            a[i][c] /= diag;
            inv[i][c] /= diag;
        }
        for r in 0..4 {
            if r == i {
                continue;
            }
            let f = a[r][i];
            for c in 0..4 {
                a[r][c] -= f * a[i][c];
                inv[r][c] -= f * inv[i][c];
            }
        }
    }
    let mut out = [[0.0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            out[j][i] = inv[i][j];
        }
    }
    Some(out)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt().max(1e-12);
    [v[0] / len, v[1] / len, v[2] / len]
}

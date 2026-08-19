use cadrum::DVec3;
use eframe::egui::{self, Id, KeyboardShortcut, Modifiers, Ui, WidgetText};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use std::path::Path;

use crate::document::{Body, BodyStats, Document, Model, Selection};
use crate::viewport::Viewport;
use eframe::CreationContext;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Tab {
    Outliner,
    Viewport,
    Properties,
    Console,
}

impl Tab {
    fn title(self) -> &'static str {
        match self {
            Self::Outliner => "Outliner",
            Self::Viewport => "Viewport",
            Self::Properties => "Properties",
            Self::Console => "Console",
        }
    }
}

pub struct OxiprepApp {
    dock_state: DockState<Tab>,
    document: Document,
    viewport: Viewport,
    console: Vec<String>,
}

impl OxiprepApp {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        let mut dock_state = DockState::new(vec![Tab::Viewport]);
        let surface = dock_state.main_surface_mut();
        let [center, _] = surface.split_left(NodeIndex::root(), 0.22, vec![Tab::Outliner]);
        let [center, _] = surface.split_right(center, 0.74, vec![Tab::Properties]);
        let _ = surface.split_below(center, 0.78, vec![Tab::Console]);
        Self {
            dock_state,
            document: Document::new(),
            viewport: Viewport::new(cc.wgpu_render_state.clone()),
            console: Vec::new(),
        }
    }

    fn log(&mut self, message: impl Into<String>) {
        self.console.push(message.into());
    }

    fn open_dialog(&mut self) {
        let files = rfd::FileDialog::new()
            .add_filter("CAD", &["step", "stp", "brep", "stl"])
            .add_filter("STEP", &["step", "stp"])
            .add_filter("BRep", &["brep"])
            .add_filter("STL", &["stl"])
            .pick_files();
        if let Some(files) = files {
            self.import_paths(&files);
        }
    }

    fn import_paths(&mut self, paths: &[impl AsRef<Path>]) {
        let mut last_index = None;
        for path in paths {
            let path = path.as_ref();
            match self.document.import_path(path) {
                Ok(index) => {
                    let model = &self.document.models[index];
                    let n = model.bodies.len();
                    self.log(format!(
                        "Opened {} ({}, {n} {}).",
                        model.name,
                        model.kind.label(),
                        if n == 1 { "body" } else { "bodies" }
                    ));
                    last_index = Some(index);
                }
                Err(err) => {
                    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
                    self.log(format!("{name}: {}", err.message()));
                }
            }
        }
        if let Some(index) = last_index {
            if let Some(bbox) = crate::document::bbox_of_model(&self.document.models[index]) {
                self.viewport.fit(bbox);
            }
        }
    }
}

impl eframe::App for OxiprepApp {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let open_shortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::O);
        if ui.input_mut(|i| i.consume_shortcut(&open_shortcut)) {
            self.open_dialog();
        }

        let dropped: Vec<_> = ui.ctx().input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.import_paths(&dropped);
        }

        let mut open = false;
        let mut close = false;
        let mut quit = false;
        let mut fit_all = false;
        let mut fit_sel = false;
        let mut look: Option<DVec3> = None;
        let mut look_iso = false;
        let has_selection = !self.document.selection.is_empty();
        let has_models = !self.document.is_empty();

        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui
                        .add(
                            egui::Button::new("Open...")
                                .shortcut_text(ui.ctx().format_shortcut(&open_shortcut)),
                        )
                        .clicked()
                    {
                        open = true;
                        ui.close();
                    }
                    if ui
                        .add_enabled(has_selection, egui::Button::new("Close"))
                        .clicked()
                    {
                        close = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        quit = true;
                        ui.close();
                    }
                });
                ui.menu_button("Edit", |_| {});
                ui.menu_button("View", |ui| {
                    if ui
                        .add_enabled(has_models, egui::Button::new("Fit All"))
                        .clicked()
                    {
                        fit_all = true;
                        ui.close();
                    }
                    if ui
                        .add_enabled(has_selection, egui::Button::new("Fit Selection"))
                        .clicked()
                    {
                        fit_sel = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("+X").clicked() {
                        look = Some(DVec3::X);
                        ui.close();
                    }
                    if ui.button("-X").clicked() {
                        look = Some(-DVec3::X);
                        ui.close();
                    }
                    if ui.button("+Y").clicked() {
                        look = Some(DVec3::Y);
                        ui.close();
                    }
                    if ui.button("-Y").clicked() {
                        look = Some(-DVec3::Y);
                        ui.close();
                    }
                    if ui.button("+Z").clicked() {
                        look = Some(DVec3::Z);
                        ui.close();
                    }
                    if ui.button("-Z").clicked() {
                        look = Some(-DVec3::Z);
                        ui.close();
                    }
                    if ui.button("Isometric").clicked() {
                        look_iso = true;
                        ui.close();
                    }
                    ui.separator();
                    ui.toggle_value(&mut self.viewport.display.faces, "Faces");
                    ui.toggle_value(&mut self.viewport.display.edges, "Edges");
                    ui.toggle_value(&mut self.viewport.display.mesh, "Mesh");
                    ui.toggle_value(&mut self.viewport.display.vertices, "Vertices");
                    ui.separator();
                    ui.toggle_value(&mut self.viewport.display.clip, "Clip");
                });
            });
        });

        if open {
            self.open_dialog();
        }
        if close {
            if let Some((_, model)) = self.document.selected_model() {
                self.log(format!("Closed {}.", model.name));
            }
            self.document.close_selected();
        }
        if quit {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if fit_all {
            if let Some(bbox) = self.document.bbox() {
                self.viewport.fit(bbox);
            }
        }
        if fit_sel {
            if let Some(bbox) = self.document.selection_bbox() {
                self.viewport.fit(bbox);
            }
        }
        if let Some(dir) = look {
            self.viewport.look_along(dir);
        }
        if look_iso {
            self.viewport.look_isometric();
        }

        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("oxiprep {}", env!("CARGO_PKG_VERSION")));
                if !self.document.is_empty() {
                    let n = self.document.models.len();
                    ui.separator();
                    ui.label(if n == 1 {
                        "1 model".to_string()
                    } else {
                        format!("{n} models")
                    });
                }
            });
        });

        let Self {
            dock_state,
            document,
            viewport,
            console,
        } = self;

        egui::CentralPanel::no_frame()
            .frame(egui::Frame::NONE.fill(ui.visuals().panel_fill))
            .show(ui, |ui| {
                DockArea::new(dock_state)
                    .style(Style::from_egui(ui.style().as_ref()))
                    .show_inside(
                        ui,
                        &mut OxiprepTabs {
                            document,
                            viewport,
                            console,
                        },
                    );
            });
    }
}

struct OxiprepTabs<'a> {
    document: &'a mut Document,
    viewport: &'a mut Viewport,
    console: &'a mut Vec<String>,
}

impl TabViewer for OxiprepTabs<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Self::Tab) -> Id {
        Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::Outliner => outliner_ui(ui, self.document, self.console),
            Tab::Viewport => self.viewport.show(ui, self.document),
            Tab::Properties => properties_ui(ui, self.document),
            Tab::Console => console_ui(ui, self.console),
        }
    }
}

fn outliner_ui(ui: &mut Ui, document: &mut Document, console: &mut Vec<String>) {
    if document.is_empty() {
        ui.label("No models loaded.");
        return;
    }
    let mut close = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        for mi in 0..document.models.len() {
            let name = document.models[mi].name.clone();
            let n_bodies = document.models[mi].bodies.len();
            let model_selected = document.selection.iter().any(|s| s.model() == mi);
            let header = egui::collapsing_header::CollapsingHeader::new(if model_selected {
                egui::RichText::new(&name).strong()
            } else {
                egui::RichText::new(&name)
            })
            .id_salt(("model", mi))
            .default_open(true)
            .show(ui, |ui| {
                for bi in 0..n_bodies {
                    let body_name = document.models[mi].bodies[bi].name.clone();
                    let selected = document.is_body_selected(mi, bi);
                    let response = ui.selectable_label(selected, body_name);
                    if response.clicked() || response.secondary_clicked() {
                        document.selection = vec![Selection::Body {
                            model: mi,
                            body: bi,
                        }];
                    }
                    response.context_menu(|ui| {
                        if ui.button("Close").clicked() {
                            close = Some(mi);
                            ui.close();
                        }
                    });
                }
            });
            if header.header_response.clicked() || header.header_response.secondary_clicked() {
                document.selection = vec![Selection::Model(mi)];
            }
            header.header_response.context_menu(|ui| {
                if ui.button("Close").clicked() {
                    close = Some(mi);
                    ui.close();
                }
            });
        }
    });
    if let Some(mi) = close {
        if let Some(model) = document.models.get(mi) {
            console.push(format!("Closed {}.", model.name));
        }
        document.close_model(mi);
    }
}

fn properties_ui(ui: &mut Ui, document: &Document) {
    let Some(item) = document.selection.last().copied() else {
        ui.label("No selection.");
        return;
    };
    if document.selection.len() > 1 {
        ui.label(format!("{} selected", document.selection.len()));
        ui.separator();
    }
    match item {
        Selection::Model(mi) => {
            if let Some(model) = document.models.get(mi) {
                model_properties(ui, model);
            }
        }
        Selection::Body { model, body } => {
            if let Some((m, b)) = document
                .models
                .get(model)
                .and_then(|m| m.bodies.get(body).map(|b| (m, b)))
            {
                body_properties(ui, m, b);
            }
        }
        Selection::Face { model, body, id } => {
            entity_properties(ui, document, item, model, body, "Face", |ui| {
                ui.label("Id");
                ui.label(id.to_string());
                ui.end_row();
            });
        }
        Selection::Edge { model, body, id } => {
            entity_properties(ui, document, item, model, body, "Edge", |ui| {
                ui.label("Id");
                ui.label(id.to_string());
                ui.end_row();
            });
        }
        Selection::Vertex { model, body, index } => {
            entity_properties(ui, document, item, model, body, "Vertex", |ui| {
                ui.label("Index");
                ui.label(index.to_string());
                ui.end_row();
                if let Some(p) = document
                    .models
                    .get(model)
                    .and_then(|m| m.bodies.get(body))
                    .and_then(|b| b.display.cad_vertices.get(index as usize))
                {
                    ui.label("Position");
                    ui.label(fmt_point(*p));
                    ui.end_row();
                }
            });
        }
        Selection::Node { model, body, index } => {
            entity_properties(ui, document, item, model, body, "Node", |ui| {
                ui.label("Index");
                ui.label(index.to_string());
                ui.end_row();
                if let Some(p) = document
                    .models
                    .get(model)
                    .and_then(|m| m.bodies.get(body))
                    .and_then(|b| b.display.positions.get(index as usize))
                {
                    ui.label("Position");
                    ui.label(fmt_point(*p));
                    ui.end_row();
                }
            });
        }
        Selection::Cell { model, body, index } => {
            entity_properties(ui, document, item, model, body, "Cell", |ui| {
                ui.label("Index");
                ui.label(index.to_string());
                ui.end_row();
            });
        }
        Selection::MeshEdge { model, body, a, b } => {
            entity_properties(ui, document, item, model, body, "Edge", |ui| {
                ui.label("Nodes");
                ui.label(format!("{a}, {b}"));
                ui.end_row();
            });
        }
    }
}

fn entity_properties(
    ui: &mut Ui,
    document: &Document,
    item: Selection,
    model: usize,
    body: usize,
    kind: &str,
    extra: impl FnOnce(&mut Ui),
) {
    let Some(m) = document.models.get(model) else {
        return;
    };
    let Some(b) = m.bodies.get(body) else {
        return;
    };
    egui::Grid::new("entity_props")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("Type");
            ui.label(kind);
            ui.end_row();
            extra(ui);
            ui.label("Model");
            ui.label(&m.name);
            ui.end_row();
            ui.label("Body");
            ui.label(&b.name);
            ui.end_row();
            if let Some(bbox) = document.item_bbox(item) {
                bbox_rows(ui, bbox);
            }
        });
}

fn model_properties(ui: &mut Ui, model: &Model) {
    egui::Grid::new("model_props")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("Name");
            ui.label(&model.name);
            ui.end_row();
            ui.label("Format");
            ui.label(model.kind.label());
            ui.end_row();
            ui.label("Path");
            ui.label(model.path.display().to_string());
            ui.end_row();
            ui.label("Bodies");
            ui.label(model.bodies.len().to_string());
            ui.end_row();
            if let Some(bbox) = crate::document::bbox_of_model(model) {
                bbox_rows(ui, bbox);
            }
        });
}

fn body_properties(ui: &mut Ui, model: &Model, body: &Body) {
    egui::Grid::new("body_props")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("Name");
            ui.label(&body.name);
            ui.end_row();
            ui.label("Model");
            ui.label(&model.name);
            ui.end_row();
            match &body.stats {
                BodyStats::Solid {
                    volume,
                    area,
                    center,
                    face_count,
                    edge_count,
                } => {
                    ui.label("Volume");
                    ui.label(fmt_num(*volume));
                    ui.end_row();
                    ui.label("Area");
                    ui.label(fmt_num(*area));
                    ui.end_row();
                    ui.label("Center");
                    ui.label(fmt_vec(*center));
                    ui.end_row();
                    ui.label("Faces");
                    ui.label(face_count.to_string());
                    ui.end_row();
                    ui.label("Edges");
                    ui.label(edge_count.to_string());
                    ui.end_row();
                }
                BodyStats::Mesh { triangle_count } => {
                    ui.label("Triangles");
                    ui.label(triangle_count.to_string());
                    ui.end_row();
                }
            }
            bbox_rows(ui, body.display.bbox);
        });
}

fn bbox_rows(ui: &mut Ui, bbox: [DVec3; 2]) {
    ui.label("Min");
    ui.label(fmt_vec(bbox[0]));
    ui.end_row();
    ui.label("Max");
    ui.label(fmt_vec(bbox[1]));
    ui.end_row();
}

fn fmt_num(v: f64) -> String {
    if v.abs() >= 1e6 || (v != 0.0 && v.abs() < 1e-3) {
        format!("{v:.6e}")
    } else {
        format!("{v:.6}")
    }
}

fn fmt_vec(v: DVec3) -> String {
    format!("{}, {}, {}", fmt_num(v.x), fmt_num(v.y), fmt_num(v.z))
}

fn fmt_point(p: [f32; 3]) -> String {
    fmt_vec(DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64))
}

fn console_ui(ui: &mut Ui, lines: &[String]) {
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .auto_shrink(false)
        .show(ui, |ui| {
            for line in lines {
                ui.label(line);
            }
        });
}

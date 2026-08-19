use cadrum::DVec3;
use eframe::egui::{self, Id, KeyboardShortcut, Modifiers, Ui, WidgetText};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use std::path::Path;

use crate::command::CommandError;
use crate::document::{Body, BodyStats, Document, Model, Selection};
use crate::geometry::{Axis, CreateKind, CreateTool, Plane};
use crate::session::Session;
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
    session: Session,
    viewport: Viewport,
    console: Vec<String>,
    create: Option<CreateTool>,
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
            session: Session::new(),
            viewport: Viewport::new(cc.wgpu_render_state.clone()),
            console: Vec::new(),
            create: None,
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
            match self.session.import_path(path) {
                Ok(message) => {
                    self.log(message);
                    last_index = Some(self.session.document.models.len() - 1);
                }
                Err(err) => {
                    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
                    self.log(format!("{name}: {}", err.message()));
                }
            }
        }
        if let Some(index) = last_index {
            if let Some(bbox) = crate::document::bbox_of_model(&self.session.document.models[index])
            {
                self.viewport.fit(bbox);
            }
        }
    }

    fn apply_undo(&mut self) {
        log_history(&mut self.console, self.session.undo());
    }

    fn apply_redo(&mut self) {
        log_history(&mut self.console, self.session.redo());
    }
}

impl eframe::App for OxiprepApp {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let open_shortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::O);
        let undo_shortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::Z);
        let redo_shortcut =
            KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, egui::Key::Z);
        let redo_y_shortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::Y);
        if ui.input_mut(|i| i.consume_shortcut(&open_shortcut)) {
            self.open_dialog();
        }
        if ui.input_mut(|i| {
            i.consume_shortcut(&redo_shortcut) || i.consume_shortcut(&redo_y_shortcut)
        }) {
            self.apply_redo();
        } else if ui.input_mut(|i| i.consume_shortcut(&undo_shortcut)) {
            self.apply_undo();
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

        let mut undo = false;
        let mut redo = false;
        let mut open = false;
        let mut close = false;
        let mut quit = false;
        let mut fit_all = false;
        let mut fit_sel = false;
        let mut look: Option<DVec3> = None;
        let mut look_iso = false;
        let has_selection = !self.session.document.selection.is_empty();
        let has_models = !self.session.document.is_empty();
        let can_undo = self.session.can_undo();
        let can_redo = self.session.can_redo();
        let undo_text = match self.session.undo_label() {
            Some(label) => format!("Undo {label}"),
            None => "Undo".to_string(),
        };
        let redo_text = match self.session.redo_label() {
            Some(label) => format!("Redo {label}"),
            None => "Redo".to_string(),
        };

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
                ui.menu_button("Edit", |ui| {
                    if ui
                        .add_enabled(
                            can_undo,
                            egui::Button::new(undo_text.clone())
                                .shortcut_text(ui.ctx().format_shortcut(&undo_shortcut)),
                        )
                        .clicked()
                    {
                        undo = true;
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            can_redo,
                            egui::Button::new(redo_text.clone())
                                .shortcut_text(ui.ctx().format_shortcut(&redo_shortcut)),
                        )
                        .clicked()
                    {
                        redo = true;
                        ui.close();
                    }
                });
                ui.menu_button("Geometry", |ui| {
                    if ui.button("Point").clicked() {
                        self.create = Some(CreateTool::new(CreateKind::point()));
                        ui.close();
                    }
                    if ui.button("Line").clicked() {
                        self.create = Some(CreateTool::line_from_document(&self.session.document));
                        ui.close();
                    }
                    if ui.button("Rectangle").clicked() {
                        self.create = Some(CreateTool::new(CreateKind::rectangle()));
                        ui.close();
                    }
                    if ui.button("Disk").clicked() {
                        self.create = Some(CreateTool::new(CreateKind::disk()));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Box").clicked() {
                        self.create = Some(CreateTool::new(CreateKind::r#box()));
                        ui.close();
                    }
                    if ui.button("Cylinder").clicked() {
                        self.create = Some(CreateTool::new(CreateKind::cylinder()));
                        ui.close();
                    }
                    if ui.button("Cone").clicked() {
                        self.create = Some(CreateTool::new(CreateKind::cone()));
                        ui.close();
                    }
                    if ui.button("Sphere").clicked() {
                        self.create = Some(CreateTool::new(CreateKind::sphere()));
                        ui.close();
                    }
                });
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
            match self.session.close_selected() {
                Ok(message) => self.log(message),
                Err(err) => self.log(err.message()),
            }
        }
        if undo {
            self.apply_undo();
        }
        if redo {
            self.apply_redo();
        }
        if quit {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if fit_all {
            if let Some(bbox) = self.session.document.bbox() {
                self.viewport.fit(bbox);
            }
        }
        if fit_sel {
            if let Some(bbox) = self.session.document.selection_bbox() {
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
                if !self.session.document.is_empty() {
                    let n = self.session.document.models.len();
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
            session,
            viewport,
            console,
            create,
        } = self;

        egui::CentralPanel::no_frame()
            .frame(egui::Frame::NONE.fill(ui.visuals().panel_fill))
            .show(ui, |ui| {
                DockArea::new(dock_state)
                    .style(Style::from_egui(ui.style().as_ref()))
                    .show_inside(
                        ui,
                        &mut OxiprepTabs {
                            session,
                            viewport,
                            console,
                            create,
                        },
                    );
            });
    }
}

struct OxiprepTabs<'a> {
    session: &'a mut Session,
    viewport: &'a mut Viewport,
    console: &'a mut Vec<String>,
    create: &'a mut Option<CreateTool>,
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
            Tab::Outliner => outliner_ui(ui, self.session, self.console),
            Tab::Viewport => self.viewport.show(ui, &mut self.session.document),
            Tab::Properties => {
                properties_ui(ui, self.session, self.viewport, self.console, self.create)
            }
            Tab::Console => console_ui(ui, self.console),
        }
    }
}

fn outliner_ui(ui: &mut Ui, session: &mut Session, console: &mut Vec<String>) {
    if session.document.is_empty() {
        ui.label("No models loaded.");
        return;
    }
    let mut close = None;
    {
        let document = &mut session.document;
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
    }
    if let Some(mi) = close {
        match session.close_model(mi) {
            Ok(message) => console.push(message),
            Err(err) => console.push(err.message().to_string()),
        }
    }
}

fn properties_ui(
    ui: &mut Ui,
    session: &mut Session,
    viewport: &mut Viewport,
    console: &mut Vec<String>,
    create: &mut Option<CreateTool>,
) {
    if create.is_some() {
        create_properties_ui(ui, session, viewport, console, create);
        return;
    }
    let document = &session.document;
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
                BodyStats::Wire { edge_count } => {
                    ui.label("Edges");
                    ui.label(edge_count.to_string());
                    ui.end_row();
                }
                BodyStats::Vertex => {}
            }
            bbox_rows(ui, body.display.bbox);
        });
}

fn create_properties_ui(
    ui: &mut Ui,
    session: &mut Session,
    viewport: &mut Viewport,
    console: &mut Vec<String>,
    create: &mut Option<CreateTool>,
) {
    let mut cancel = false;
    if let Some(tool) = create.as_mut() {
        ui.label(tool.kind.title());
        ui.separator();
        egui::Grid::new("create_props")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| match &mut tool.kind {
                CreateKind::Point { p } => {
                    ui.label("Position");
                    vec3_edit(ui, p);
                    ui.end_row();
                }
                CreateKind::Line { a, b } => {
                    ui.label("Start");
                    vec3_edit(ui, a);
                    ui.end_row();
                    ui.label("End");
                    vec3_edit(ui, b);
                    ui.end_row();
                }
                CreateKind::Rectangle {
                    plane,
                    origin,
                    width,
                    height,
                } => {
                    ui.label("Plane");
                    plane_edit(ui, plane);
                    ui.end_row();
                    ui.label("Origin");
                    vec3_edit(ui, origin);
                    ui.end_row();
                    ui.label("Width");
                    ui.add(egui::DragValue::new(width).speed(0.1));
                    ui.end_row();
                    ui.label("Height");
                    ui.add(egui::DragValue::new(height).speed(0.1));
                    ui.end_row();
                }
                CreateKind::Disk {
                    plane,
                    center,
                    radius,
                } => {
                    ui.label("Plane");
                    plane_edit(ui, plane);
                    ui.end_row();
                    ui.label("Center");
                    vec3_edit(ui, center);
                    ui.end_row();
                    ui.label("Radius");
                    ui.add(
                        egui::DragValue::new(radius)
                            .speed(0.1)
                            .range(0.0..=f64::MAX),
                    );
                    ui.end_row();
                }
                CreateKind::Box { origin, size } => {
                    ui.label("Origin");
                    vec3_edit(ui, origin);
                    ui.end_row();
                    ui.label("Size");
                    vec3_edit(ui, size);
                    ui.end_row();
                }
                CreateKind::Cylinder {
                    center,
                    axis,
                    radius,
                    height,
                } => {
                    ui.label("Center");
                    vec3_edit(ui, center);
                    ui.end_row();
                    ui.label("Axis");
                    axis_edit(ui, axis);
                    ui.end_row();
                    ui.label("Radius");
                    ui.add(
                        egui::DragValue::new(radius)
                            .speed(0.1)
                            .range(0.0..=f64::MAX),
                    );
                    ui.end_row();
                    ui.label("Height");
                    ui.add(
                        egui::DragValue::new(height)
                            .speed(0.1)
                            .range(0.0..=f64::MAX),
                    );
                    ui.end_row();
                }
                CreateKind::Cone {
                    center,
                    axis,
                    r1,
                    r2,
                    height,
                } => {
                    ui.label("Center");
                    vec3_edit(ui, center);
                    ui.end_row();
                    ui.label("Axis");
                    axis_edit(ui, axis);
                    ui.end_row();
                    ui.label("Radius 1");
                    ui.add(egui::DragValue::new(r1).speed(0.1).range(0.0..=f64::MAX));
                    ui.end_row();
                    ui.label("Radius 2");
                    ui.add(egui::DragValue::new(r2).speed(0.1).range(0.0..=f64::MAX));
                    ui.end_row();
                    ui.label("Height");
                    ui.add(
                        egui::DragValue::new(height)
                            .speed(0.1)
                            .range(0.0..=f64::MAX),
                    );
                    ui.end_row();
                }
                CreateKind::Sphere { center, radius } => {
                    ui.label("Center");
                    vec3_edit(ui, center);
                    ui.end_row();
                    ui.label("Radius");
                    ui.add(
                        egui::DragValue::new(radius)
                            .speed(0.1)
                            .range(0.0..=f64::MAX),
                    );
                    ui.end_row();
                }
            });
        let has_model = session.document.selected_model_index().is_some();
        if !has_model {
            tool.add_to_current = false;
        }
        ui.add_enabled(
            has_model,
            egui::Checkbox::new(&mut tool.add_to_current, "Add to current model"),
        );
        ui.horizontal(|ui| {
            let can_create = tool.kind.valid();
            if ui
                .add_enabled(can_create, egui::Button::new("Create"))
                .clicked()
            {
                apply_create(session, viewport, console, tool);
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    }
    if cancel {
        *create = None;
    }
}

fn apply_create(
    session: &mut Session,
    viewport: &mut Viewport,
    console: &mut Vec<String>,
    tool: &CreateTool,
) {
    let add_to = tool
        .add_to_current
        .then(|| session.document.selected_model_index())
        .flatten();
    let result = if let Some(model) = add_to {
        match tool.kind.into_body(&session.document, model) {
            Ok(body) => session.add_body(model, body),
            Err(err) => Err(err),
        }
    } else {
        match tool.kind.into_model(&session.document) {
            Ok(model) => session.create_model(model),
            Err(err) => Err(err),
        }
    };
    match result {
        Ok(message) => {
            console.push(message);
            if let Some(bbox) = session.document.selection_bbox() {
                viewport.fit(bbox);
            }
        }
        Err(err) => console.push(err.message().to_string()),
    }
}

fn vec3_edit(ui: &mut Ui, v: &mut [f64; 3]) {
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut v[0]).speed(0.1));
        ui.add(egui::DragValue::new(&mut v[1]).speed(0.1));
        ui.add(egui::DragValue::new(&mut v[2]).speed(0.1));
    });
}

fn axis_edit(ui: &mut Ui, axis: &mut Axis) {
    ui.horizontal(|ui| {
        ui.selectable_value(axis, Axis::X, Axis::X.label());
        ui.selectable_value(axis, Axis::Y, Axis::Y.label());
        ui.selectable_value(axis, Axis::Z, Axis::Z.label());
    });
}

fn plane_edit(ui: &mut Ui, plane: &mut Plane) {
    ui.horizontal(|ui| {
        ui.selectable_value(plane, Plane::XY, Plane::XY.label());
        ui.selectable_value(plane, Plane::YZ, Plane::YZ.label());
        ui.selectable_value(plane, Plane::XZ, Plane::XZ.label());
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

fn log_history(console: &mut Vec<String>, result: Result<Option<String>, CommandError>) {
    match result {
        Ok(Some(message)) => console.push(message),
        Ok(None) => {}
        Err(err) => console.push(err.message().to_string()),
    }
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

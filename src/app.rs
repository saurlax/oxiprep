use eframe::egui::{self, Id, Ui, WidgetText};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};

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
}

impl OxiprepApp {
    pub fn new() -> Self {
        let mut dock_state = DockState::new(vec![Tab::Viewport]);
        let surface = dock_state.main_surface_mut();
        let [center, _] = surface.split_left(NodeIndex::root(), 0.22, vec![Tab::Outliner]);
        let [center, _] = surface.split_right(center, 0.74, vec![Tab::Properties]);
        let _ = surface.split_below(center, 0.78, vec![Tab::Console]);
        Self { dock_state }
    }
}

impl eframe::App for OxiprepApp {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Edit", |_| {});
                ui.menu_button("View", |_| {});
            });
        });

        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("oxiprep {}", env!("CARGO_PKG_VERSION")));
            });
        });

        egui::CentralPanel::no_frame()
            .frame(egui::Frame::NONE.fill(ui.visuals().panel_fill))
            .show(ui, |ui| {
                DockArea::new(&mut self.dock_state)
                    .style(Style::from_egui(ui.style().as_ref()))
                    .show_inside(ui, &mut OxiprepTabs);
            });
    }
}

struct OxiprepTabs;

impl TabViewer for OxiprepTabs {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Self::Tab) -> Id {
        Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Self::Tab) -> WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::Outliner => {
                ui.label("No models loaded.");
            }
            Tab::Viewport => {}
            Tab::Properties => {
                ui.label("No selection.");
            }
            Tab::Console => {}
        }
    }
}

//! Docked AI workspace widgets.

use crate::ai::controller::{AiController, ConnectionState};
use crate::ai::profile::{
    AgentProfile, CODEX_INSTALL_GUIDANCE, CODEX_NPX_GUIDANCE, CODEX_PROFILE_ID,
    WorkingDirectoryPolicy, is_secret_name,
};
use crate::ai::transcript::{TranscriptKind, safe_markdown};
use crate::session::Session;
use crate::viewport::Viewport;
use eframe::egui::{self, Ui};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

const UPDATE_DETAIL_MAX_HEIGHT: f32 = 180.0;
const CONNECTION_DETAIL_MAX_HEIGHT: f32 = 140.0;
const COMPOSER_RESERVED_HEIGHT: f32 = 96.0;

#[derive(Default)]
pub struct AiPanel {
    markdown: CommonMarkCache,
    editing: Option<AgentProfile>,
    args_text: String,
    env_name: String,
    env_value: String,
    message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ConnectionControls {
    can_select_profile: bool,
    can_connect: bool,
    show_disconnect: bool,
    can_start_conversation: bool,
    show_authentication: bool,
    can_send: bool,
    can_cancel: bool,
}

fn connection_controls(state: &ConnectionState, has_profiles: bool) -> ConnectionControls {
    let can_select_profile = !state.connected() && !matches!(state, ConnectionState::Starting);
    ConnectionControls {
        can_select_profile,
        can_connect: can_select_profile && has_profiles,
        show_disconnect: state.connected() || matches!(state, ConnectionState::Starting),
        can_start_conversation: matches!(state, ConnectionState::Ready),
        show_authentication: matches!(state, ConnectionState::AuthenticationRequired),
        can_send: matches!(state, ConnectionState::Ready),
        can_cancel: matches!(state, ConnectionState::PromptActive),
    }
}

impl AiPanel {
    pub fn show(
        &mut self,
        ui: &mut Ui,
        controller: &mut AiController,
        session: &mut Session,
        viewport: &mut Viewport,
        console: &mut Vec<String>,
    ) {
        self.connection_bar(ui, controller, session);
        if let Some(warning) = controller.profile_warning() {
            ui.colored_label(ui.visuals().warn_fg_color, warning);
        }
        if let Some(message) = &self.message {
            ui.colored_label(ui.visuals().warn_fg_color, message);
        }
        ui.separator();

        if self.editing.is_some() {
            self.profile_editor(ui, controller);
            return;
        }

        if let Some(permission) = controller.pending_permission() {
            let title = permission
                .title
                .clone()
                .unwrap_or_else(|| "Agent permission".to_owned());
            let tool_call_id = permission.tool_call_id.clone();
            let options = permission.options.clone();
            ui.group(|ui| {
                ui.label(egui::RichText::new(title).strong());
                ui.label(format!("Tool call: {tool_call_id}"));
                for option in options {
                    if ui
                        .button(&option.name)
                        .on_hover_text(&option.kind)
                        .clicked()
                    {
                        controller.resolve_permission(Some(option.id));
                    }
                }
                if ui.button("Cancel").clicked() {
                    controller.resolve_permission(None);
                }
            });
            ui.separator();
        }

        if let Some(confirmation) = controller.pending_confirmation() {
            let operation = confirmation.operation.clone();
            let detail = confirmation.detail.clone();
            ui.group(|ui| {
                ui.label(egui::RichText::new("Oxiprep confirmation").strong());
                ui.label(operation);
                ui.label(detail);
                ui.horizontal(|ui| {
                    if ui.button("Approve").clicked() {
                        controller.resolve_confirmation(true, session, viewport, console);
                    }
                    if ui.button("Reject").clicked() {
                        controller.resolve_confirmation(false, session, viewport, console);
                    }
                });
            });
            ui.separator();
        }

        ui.with_layout(panel_body_layout(), |ui| {
            self.composer(ui, controller, session, viewport);
            ui.separator();
            self.transcript(ui, controller);
        });
    }

    fn transcript(&mut self, ui: &mut Ui, controller: &AiController) {
        transcript_area(ui, |ui| {
            if controller.transcript().items().is_empty() {
                if matches!(
                    controller.state(),
                    ConnectionState::Disconnected | ConnectionState::Failed(_)
                ) {
                    ui.label("Connect a local agent to start a conversation.");
                }
            } else {
                for item in controller.transcript().items() {
                    ui.push_id(&item.key, |ui| {
                        match item.kind {
                            TranscriptKind::User => {
                                ui.label(egui::RichText::new("You").strong());
                            }
                            TranscriptKind::Agent => {
                                ui.label(egui::RichText::new("Agent").strong());
                            }
                            TranscriptKind::Warning => {
                                ui.colored_label(
                                    ui.visuals().warn_fg_color,
                                    item.title.as_deref().unwrap_or("Warning"),
                                );
                            }
                            TranscriptKind::Reasoning
                            | TranscriptKind::Plan
                            | TranscriptKind::Tool
                            | TranscriptKind::Extension => {
                                let title = item.title.as_deref().unwrap_or(match item.kind {
                                    TranscriptKind::Reasoning => "Reasoning",
                                    TranscriptKind::Plan => "Plan",
                                    TranscriptKind::Tool => "Tool",
                                    _ => "Agent update",
                                });
                                egui::CollapsingHeader::new(if let Some(status) = &item.status {
                                    format!("{title} · {status}")
                                } else {
                                    title.to_owned()
                                })
                                .default_open(item.kind != TranscriptKind::Reasoning)
                                .show(ui, |ui| {
                                    bounded_detail(
                                        ui,
                                        (&item.key, "detail"),
                                        UPDATE_DETAIL_MAX_HEIGHT,
                                        |ui| self.markdown(ui, &item.text),
                                    );
                                });
                                ui.separator();
                                return;
                            }
                        }
                        self.markdown(ui, &item.text);
                        ui.separator();
                    });
                }
            }
        });
    }

    fn composer(
        &mut self,
        ui: &mut Ui,
        controller: &mut AiController,
        session: &Session,
        viewport: &Viewport,
    ) {
        let controls = connection_controls(controller.state(), !controller.profiles().is_empty());
        // The parent lays widgets out bottom-up to keep the composer visible. Add the action row
        // first so it appears below the editor while the composer still remains bottom-anchored.
        ui.horizontal(|ui| {
            if ui
                .add_enabled(controls.can_send, egui::Button::new("Send"))
                .clicked()
                && let Err(error) = controller.send_prompt(session, viewport)
            {
                self.message = Some(error);
            }
            if ui
                .add_enabled(controls.can_cancel, egui::Button::new("Cancel"))
                .clicked()
                && let Err(error) = controller.cancel_prompt()
            {
                self.message = Some(error);
            }
        });
        ui.add_enabled(
            controls.can_send,
            egui::TextEdit::multiline(controller.prompt_mut())
                .desired_rows(3)
                .hint_text("Ask the agent…"),
        );
    }

    fn connection_bar(&mut self, ui: &mut Ui, controller: &mut AiController, session: &Session) {
        let profiles = controller.profiles();
        let selected = controller.selected_profile_id().map(str::to_owned);
        let selected_name = profiles
            .iter()
            .find(|profile| Some(profile.id.as_str()) == selected.as_deref())
            .map(|profile| profile.name.as_str())
            .unwrap_or("No profiles");
        let controls = connection_controls(controller.state(), !profiles.is_empty());
        ui.horizontal(|ui| {
            ui.add_enabled_ui(controls.can_select_profile, |ui| {
                egui::ComboBox::from_id_salt("ai_profile")
                    .selected_text(selected_name)
                    .show_ui(ui, |ui| {
                        for profile in &profiles {
                            if ui
                                .selectable_label(
                                    Some(profile.id.as_str()) == selected.as_deref(),
                                    &profile.name,
                                )
                                .clicked()
                            {
                                controller.select_profile(profile.id.clone());
                            }
                        }
                    });
            });
            if controls.show_disconnect {
                if ui.button("Disconnect").clicked() {
                    controller.disconnect();
                }
            } else if ui
                .add_enabled(controls.can_connect, egui::Button::new("Connect"))
                .clicked()
                && let Err(error) = controller.connect(ui.ctx(), session.document.path.as_deref())
            {
                self.message = Some(error);
            }
            if ui
                .add_enabled(controls.can_select_profile, egui::Button::new("Edit…"))
                .clicked()
                && let Some(profile) = profiles
                    .iter()
                    .find(|profile| Some(profile.id.as_str()) == selected.as_deref())
            {
                self.start_edit(profile.clone());
            }
            if ui
                .add_enabled(controls.can_select_profile, egui::Button::new("+"))
                .clicked()
            {
                self.start_edit(AgentProfile::new("Local agent", "agent-command"));
            }
        });
        ui.horizontal(|ui| {
            ui.label(controller.state().label());
            if let Some(name) = controller.agent_name() {
                ui.separator();
                ui.label(if let Some(version) = controller.agent_version() {
                    format!("{name} {version}")
                } else {
                    name.to_owned()
                });
            }
            if controls.can_start_conversation
                && ui.button("New conversation").clicked()
                && let Err(error) = controller.new_conversation()
            {
                self.message = Some(error);
            }
        });
        if let Some(cwd) = controller.connected_cwd() {
            ui.small(format!("Working directory: {}", cwd.display()));
        }
        if let Some(endpoint) = controller.mcp_endpoint() {
            let state = if controller.mcp_ready() {
                "ready"
            } else {
                "waiting for agent"
            };
            ui.small(format!("Oxiprep MCP: {state} ({endpoint})"));
        }
        if let Some(capabilities) = controller.agent_capabilities() {
            egui::CollapsingHeader::new("Agent capabilities").show(ui, |ui| {
                let maximum = connection_detail_height(ui);
                bounded_detail(ui, "agent_capabilities", maximum, |ui| {
                    ui.monospace(
                        serde_json::to_string_pretty(capabilities)
                            .unwrap_or_else(|_| "Unavailable".to_owned()),
                    );
                });
            });
        }
        if controls.show_authentication {
            let methods = controller.authentication().to_vec();
            for method in methods {
                if ui
                    .button(format!("Authenticate: {}", method.name))
                    .on_hover_text(method.description.unwrap_or_default())
                    .clicked()
                    && let Err(error) = controller.authenticate(&method.id)
                {
                    self.message = Some(error);
                }
            }
        }
        if let ConnectionState::Failed(error) = controller.state() {
            ui.colored_label(ui.visuals().error_fg_color, error);
            if selected.as_deref() == Some(CODEX_PROFILE_ID) {
                ui.label(CODEX_INSTALL_GUIDANCE);
                ui.label(CODEX_NPX_GUIDANCE);
            }
        }
        if !controller.stderr().is_empty() {
            egui::CollapsingHeader::new("Agent diagnostics").show(ui, |ui| {
                let maximum = connection_detail_height(ui);
                bounded_detail(ui, "agent_diagnostics", maximum, |ui| {
                    for line in controller.stderr() {
                        ui.monospace(line);
                    }
                });
            });
        }
    }

    fn start_edit(&mut self, profile: AgentProfile) {
        self.args_text = profile.args.join("\n");
        self.editing = Some(profile);
        self.env_name.clear();
        self.env_value.clear();
    }

    fn edited_profile(&self) -> Option<AgentProfile> {
        let mut profile = self.editing.clone()?;
        profile.args = self
            .args_text
            .lines()
            .map(str::to_owned)
            .filter(|arg| !arg.is_empty())
            .collect();
        Some(profile)
    }

    fn profile_editor(&mut self, ui: &mut Ui, controller: &mut AiController) {
        let Some(profile) = self.editing.as_mut() else {
            return;
        };
        ui.label(egui::RichText::new("Agent profile").strong());
        egui::Grid::new("agent_profile_editor")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut profile.name);
                ui.end_row();
                ui.label("Command");
                ui.text_edit_singleline(&mut profile.command);
                ui.end_row();
                ui.label("Arguments");
                ui.add(
                    egui::TextEdit::multiline(&mut self.args_text)
                        .desired_rows(3)
                        .code_editor(),
                );
                ui.end_row();
            });
        ui.label("Working directory");
        ui.horizontal(|ui| {
            if ui
                .selectable_label(
                    matches!(
                        profile.working_directory,
                        WorkingDirectoryPolicy::SavedProject
                    ),
                    "Saved project",
                )
                .clicked()
            {
                profile.working_directory = WorkingDirectoryPolicy::SavedProject;
            }
            if ui
                .selectable_label(
                    matches!(
                        profile.working_directory,
                        WorkingDirectoryPolicy::Application
                    ),
                    "Application",
                )
                .clicked()
            {
                profile.working_directory = WorkingDirectoryPolicy::Application;
            }
            if ui
                .selectable_label(
                    matches!(
                        profile.working_directory,
                        WorkingDirectoryPolicy::Fixed { .. }
                    ),
                    "Fixed",
                )
                .clicked()
            {
                profile.working_directory = WorkingDirectoryPolicy::Fixed {
                    path: std::path::PathBuf::new(),
                };
            }
        });
        if let WorkingDirectoryPolicy::Fixed { path } = &mut profile.working_directory {
            let mut value = path.to_string_lossy().into_owned();
            if ui.text_edit_singleline(&mut value).changed() {
                *path = value.into();
            }
        }
        ui.separator();
        ui.label("Environment overrides");
        let mut remove = None;
        for (name, value) in profile.environment.iter_mut() {
            ui.horizontal(|ui| {
                ui.monospace(name);
                ui.add(egui::TextEdit::singleline(value).password(is_secret_name(name)));
                if ui.small_button("Remove").clicked() {
                    remove = Some(name.clone());
                }
            });
        }
        if let Some(name) = remove {
            profile.environment.remove(&name);
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.env_name).hint_text("NAME"));
            ui.add(
                egui::TextEdit::singleline(&mut self.env_value)
                    .hint_text("value")
                    .password(is_secret_name(&self.env_name)),
            );
            if ui.button("Add").clicked() && !self.env_name.is_empty() {
                profile.environment.insert(
                    std::mem::take(&mut self.env_name),
                    std::mem::take(&mut self.env_value),
                );
            }
        });
        if profile.id == CODEX_PROFILE_ID {
            ui.label(CODEX_INSTALL_GUIDANCE);
            ui.label(CODEX_NPX_GUIDANCE);
        }
        let id = profile.id.clone();
        let mut save = false;
        let mut reset = false;
        let mut delete = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                save = true;
            }
            if profile.id == CODEX_PROFILE_ID && ui.button("Reset").clicked() {
                reset = true;
            }
            if ui.button("Delete").clicked() {
                delete = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
        let saved = self.edited_profile().expect("profile editor is active");
        if save {
            match controller.save_profile(saved) {
                Ok(()) => self.editing = None,
                Err(error) => self.message = Some(error),
            }
        } else if reset {
            match controller.reset_codex() {
                Ok(()) => self.editing = None,
                Err(error) => self.message = Some(error),
            }
        } else if delete {
            match controller.delete_profile(&id) {
                Ok(()) => self.editing = None,
                Err(error) => self.message = Some(error),
            }
        } else if cancel {
            self.editing = None;
        }
    }

    fn markdown(&mut self, ui: &mut Ui, text: &str) {
        let text = safe_markdown(text);
        CommonMarkViewer::new()
            .explicit_image_uri_scheme(true)
            .show(ui, &mut self.markdown, &text);
    }
}

fn panel_body_layout() -> egui::Layout {
    egui::Layout::bottom_up(egui::Align::Min)
}

fn content_layout() -> egui::Layout {
    egui::Layout::top_down(egui::Align::Min)
}

fn constrained_scroll_content<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let width = ui.available_width();
    ui.set_width(width);
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
    ui.with_layout(content_layout(), add_contents).inner
}

fn transcript_area<R>(
    ui: &mut Ui,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .id_salt("ai_transcript")
        .show(ui, |ui| constrained_scroll_content(ui, add_contents))
}

fn connection_detail_height(ui: &Ui) -> f32 {
    (ui.available_height() - COMPOSER_RESERVED_HEIGHT).clamp(0.0, CONNECTION_DETAIL_MAX_HEIGHT)
}

fn bounded_detail<R>(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    maximum_height: f32,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    egui::ScrollArea::vertical()
        .id_salt(id_salt)
        .max_height(maximum_height)
        .auto_shrink([false, true])
        .show(ui, |ui| constrained_scroll_content(ui, add_contents))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn raw_input(width: f32, height: f32) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            ..Default::default()
        }
    }

    #[test]
    fn controls_cover_every_connection_state() {
        let cases = [
            (
                ConnectionState::Disconnected,
                ConnectionControls {
                    can_select_profile: true,
                    can_connect: true,
                    show_disconnect: false,
                    can_start_conversation: false,
                    show_authentication: false,
                    can_send: false,
                    can_cancel: false,
                },
            ),
            (
                ConnectionState::Starting,
                ConnectionControls {
                    can_select_profile: false,
                    can_connect: false,
                    show_disconnect: true,
                    can_start_conversation: false,
                    show_authentication: false,
                    can_send: false,
                    can_cancel: false,
                },
            ),
            (
                ConnectionState::AuthenticationRequired,
                ConnectionControls {
                    can_select_profile: false,
                    can_connect: false,
                    show_disconnect: true,
                    can_start_conversation: false,
                    show_authentication: true,
                    can_send: false,
                    can_cancel: false,
                },
            ),
            (
                ConnectionState::Ready,
                ConnectionControls {
                    can_select_profile: false,
                    can_connect: false,
                    show_disconnect: true,
                    can_start_conversation: true,
                    show_authentication: false,
                    can_send: true,
                    can_cancel: false,
                },
            ),
            (
                ConnectionState::PromptActive,
                ConnectionControls {
                    can_select_profile: false,
                    can_connect: false,
                    show_disconnect: true,
                    can_start_conversation: false,
                    show_authentication: false,
                    can_send: false,
                    can_cancel: true,
                },
            ),
            (
                ConnectionState::Failed("launch failed".to_owned()),
                ConnectionControls {
                    can_select_profile: true,
                    can_connect: true,
                    show_disconnect: false,
                    can_start_conversation: false,
                    show_authentication: false,
                    can_send: false,
                    can_cancel: false,
                },
            ),
        ];
        for (state, expected) in cases {
            assert_eq!(connection_controls(&state, true), expected);
        }
        assert!(!connection_controls(&ConnectionState::Disconnected, false).can_connect);
    }

    #[test]
    fn profile_editor_round_trips_custom_launch_fields() {
        let mut profile = AgentProfile::new("Custom", "custom-agent");
        profile.args = vec!["--mode".to_owned(), "two words".to_owned()];
        profile.environment = BTreeMap::from([("MODEL".to_owned(), "local".to_owned())]);
        profile.working_directory = WorkingDirectoryPolicy::Fixed {
            path: PathBuf::from("/tmp"),
        };
        let mut panel = AiPanel::default();
        panel.start_edit(profile.clone());
        assert_eq!(panel.edited_profile(), Some(profile));

        let codex = AgentProfile::codex();
        panel.start_edit(codex.clone());
        assert_eq!(panel.edited_profile(), Some(codex));
        assert!(CODEX_INSTALL_GUIDANCE.contains("npm install -g"));
        assert!(CODEX_NPX_GUIDANCE.contains("npx"));
    }

    #[test]
    fn markdown_links_do_not_open_without_an_explicit_click() {
        let context = egui::Context::default();
        let mut panel = AiPanel::default();
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            panel.markdown(
                ui,
                "# Result\n\n- **safe**\n\n```text\ncode\n```\n\n[Open](https://example.com)",
            );
        });
        let opened = output
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command, egui::OutputCommand::OpenUrl(_)));
        assert!(!opened);
        output.drop_without_applying_deltas();
    }

    #[test]
    fn verbose_detail_uses_a_bounded_internal_scroll_region() {
        let context = egui::Context::default();
        let mut metrics = None;
        let output = context.run_ui(raw_input(400.0, 600.0), |ui| {
            let detail = bounded_detail(ui, "bounded_detail_test", 80.0, |ui| {
                let mut first = None;
                let mut last = None;
                for index in 0..100 {
                    let rect = ui.label(format!("Capability line {index}")).rect;
                    first.get_or_insert(rect);
                    last = Some(rect);
                }
                (first.unwrap(), last.unwrap())
            });
            metrics = Some((
                detail.inner_rect.height(),
                detail.content_size.y,
                detail.inner.0,
                detail.inner.1,
            ));
        });
        let (viewport_height, content_height, first, last) = metrics.unwrap();
        assert!(viewport_height <= 80.0 + 0.5);
        assert!(content_height > viewport_height);
        assert!(first.top() < last.top());
        output.drop_without_applying_deltas();
    }

    #[test]
    fn vertical_scroll_content_stays_within_a_narrow_panel() {
        let context = egui::Context::default();
        let mut metrics = None;
        let output = context.run_ui(raw_input(140.0, 180.0), |ui| {
            let detail = bounded_detail(ui, "narrow_detail_test", 80.0, |ui| {
                ui.monospace("x".repeat(200));
            });
            metrics = Some((detail.inner_rect.width(), detail.content_size.x));
        });
        let (viewport_width, content_width) = metrics.unwrap();
        assert!(content_width <= viewport_width + 0.5);
        output.drop_without_applying_deltas();
    }

    #[test]
    fn bottom_layout_reserves_composer_while_long_transcript_scrolls() {
        let context = egui::Context::default();
        let mut metrics = None;
        let output = context.run_ui(raw_input(320.0, 220.0), |ui| {
            metrics = Some(
                ui.with_layout(panel_body_layout(), |ui| {
                    let composer = ui
                        .allocate_response(
                            egui::vec2(ui.available_width(), 72.0),
                            egui::Sense::hover(),
                        )
                        .rect;
                    ui.separator();
                    let transcript = transcript_area(ui, |ui| {
                        let mut first = None;
                        let mut last = None;
                        for index in 0..100 {
                            let rect = ui.label(format!("Transcript line {index}")).rect;
                            first.get_or_insert(rect);
                            last = Some(rect);
                        }
                        (first.unwrap(), last.unwrap())
                    });
                    (
                        composer,
                        transcript.inner_rect,
                        transcript.content_size.y,
                        transcript.inner.0,
                        transcript.inner.1,
                    )
                })
                .inner,
            );
        });
        let (composer, transcript, transcript_content_height, first, last) = metrics.unwrap();
        assert!(composer.bottom() <= 220.0 + 0.5);
        assert!(transcript.bottom() <= composer.top() + 0.5);
        assert!(transcript_content_height > transcript.height());
        assert!(first.top() < last.top());
        output.drop_without_applying_deltas();
    }
}

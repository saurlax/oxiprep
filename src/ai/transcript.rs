//! Ordered ACP/MCP transcript state.

use agent_client_protocol::schema::v1::{ContentBlock, SessionUpdate, ToolCallStatus};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranscriptKind {
    User,
    Agent,
    Reasoning,
    Plan,
    Tool,
    Warning,
    Extension,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptItem {
    pub key: String,
    pub kind: TranscriptKind,
    pub title: Option<String>,
    pub text: String,
    pub status: Option<String>,
    pub detail: Value,
}

#[derive(Default)]
pub struct Transcript {
    items: Vec<TranscriptItem>,
    keyed: HashMap<String, usize>,
    next_id: u64,
}

impl Transcript {
    pub fn items(&self) -> &[TranscriptItem] {
        &self.items
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.keyed.clear();
        self.next_id = 0;
    }

    pub fn push_user(&mut self, text: impl Into<String>) {
        let key = self.unique_key("user");
        self.insert(TranscriptItem {
            key,
            kind: TranscriptKind::User,
            title: None,
            text: text.into(),
            status: None,
            detail: Value::Null,
        });
    }

    pub fn push_warning(&mut self, text: impl Into<String>) {
        let key = self.unique_key("warning");
        self.insert(TranscriptItem {
            key,
            kind: TranscriptKind::Warning,
            title: Some("Warning".to_owned()),
            text: text.into(),
            status: None,
            detail: Value::Null,
        });
    }

    pub fn push_status(&mut self, title: impl Into<String>, text: impl Into<String>) {
        let key = self.unique_key("status");
        self.insert(TranscriptItem {
            key,
            kind: TranscriptKind::Extension,
            title: Some(title.into()),
            text: text.into(),
            status: None,
            detail: Value::Null,
        });
    }

    pub fn apply_update(&mut self, update: SessionUpdate) {
        match update {
            SessionUpdate::UserMessageChunk(chunk) => self.apply_chunk(
                TranscriptKind::User,
                "user",
                chunk.message_id.map(|id| id.0.to_string()),
                chunk.content,
            ),
            SessionUpdate::AgentMessageChunk(chunk) => self.apply_chunk(
                TranscriptKind::Agent,
                "agent",
                chunk.message_id.map(|id| id.0.to_string()),
                chunk.content,
            ),
            SessionUpdate::AgentThoughtChunk(chunk) => self.apply_chunk(
                TranscriptKind::Reasoning,
                "reasoning",
                chunk.message_id.map(|id| id.0.to_string()),
                chunk.content,
            ),
            SessionUpdate::ToolCall(call) => {
                let key = format!("tool:{}", call.tool_call_id.0);
                let item = TranscriptItem {
                    key: key.clone(),
                    kind: TranscriptKind::Tool,
                    title: Some(call.title.clone()),
                    text: tool_content_text(&call.content),
                    status: Some(tool_status(call.status)),
                    detail: serde_json::to_value(&call).unwrap_or(Value::Null),
                };
                self.upsert(key, item);
            }
            SessionUpdate::ToolCallUpdate(update) => {
                let key = format!("tool:{}", update.tool_call_id.0);
                if let Some(index) = self.keyed.get(&key).copied() {
                    let item = &mut self.items[index];
                    if let Some(title) = update.fields.title.clone() {
                        item.title = Some(title);
                    }
                    if let Some(status) = update.fields.status {
                        item.status = Some(tool_status(status));
                    }
                    if let Some(content) = update.fields.content.as_ref() {
                        item.text = tool_content_text(content);
                    }
                    item.detail = serde_json::to_value(&update).unwrap_or(Value::Null);
                } else {
                    let item = TranscriptItem {
                        key: key.clone(),
                        kind: TranscriptKind::Tool,
                        title: update.fields.title.clone().or(Some("Tool call".to_owned())),
                        text: update
                            .fields
                            .content
                            .as_ref()
                            .map(|content| tool_content_text(content))
                            .unwrap_or_default(),
                        status: update.fields.status.map(tool_status),
                        detail: serde_json::to_value(&update).unwrap_or(Value::Null),
                    };
                    self.upsert(key, item);
                }
            }
            SessionUpdate::Plan(plan) => {
                let text = plan
                    .entries
                    .iter()
                    .map(|entry| format!("- [{:?}] {}", entry.status, entry.content))
                    .collect::<Vec<_>>()
                    .join("\n");
                let key = "plan".to_owned();
                self.upsert(
                    key.clone(),
                    TranscriptItem {
                        key,
                        kind: TranscriptKind::Plan,
                        title: Some("Plan".to_owned()),
                        text,
                        status: None,
                        detail: serde_json::to_value(plan).unwrap_or(Value::Null),
                    },
                );
            }
            other => {
                let detail = serde_json::to_value(&other).unwrap_or(Value::Null);
                let key = self.unique_key("extension");
                self.insert(TranscriptItem {
                    key,
                    kind: TranscriptKind::Extension,
                    title: Some("Agent update".to_owned()),
                    text: compact_json(&detail),
                    status: None,
                    detail,
                });
            }
        }
    }

    pub fn apply_mcp_result(
        &mut self,
        tool_call_id: Option<&str>,
        title: &str,
        result: Value,
        is_error: bool,
    ) {
        let key = tool_call_id
            .map(|id| format!("tool:{id}"))
            .unwrap_or_else(|| self.unique_key("mcp"));
        let status = if is_error { "failed" } else { "completed" }.to_owned();
        let text = compact_json(&result);
        if let Some(index) = self.keyed.get(&key).copied() {
            let item = &mut self.items[index];
            item.status = Some(status);
            item.text = text;
            item.detail = result;
        } else {
            self.upsert(
                key.clone(),
                TranscriptItem {
                    key,
                    kind: TranscriptKind::Tool,
                    title: Some(title.to_owned()),
                    text,
                    status: Some(status),
                    detail: result,
                },
            );
        }
    }

    fn apply_chunk(
        &mut self,
        kind: TranscriptKind,
        prefix: &str,
        message_id: Option<String>,
        content: ContentBlock,
    ) {
        let text = content_text(&content);
        let key = message_id
            .map(|id| format!("message:{id}"))
            .unwrap_or_else(|| {
                self.items
                    .last()
                    .filter(|item| item.kind == kind)
                    .map(|item| item.key.clone())
                    .unwrap_or_else(|| self.unique_key(prefix))
            });
        if let Some(index) = self.keyed.get(&key).copied() {
            self.items[index].text.push_str(&text);
        } else {
            let detail = serde_json::to_value(content).unwrap_or(Value::Null);
            self.upsert(
                key.clone(),
                TranscriptItem {
                    key,
                    kind,
                    title: None,
                    text,
                    status: None,
                    detail,
                },
            );
        }
    }

    fn unique_key(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}:{}", self.next_id)
    }

    fn insert(&mut self, item: TranscriptItem) {
        self.keyed.insert(item.key.clone(), self.items.len());
        self.items.push(item);
    }

    fn upsert(&mut self, key: String, item: TranscriptItem) {
        if let Some(index) = self.keyed.get(&key).copied() {
            self.items[index] = item;
        } else {
            self.insert(item);
        }
    }
}

fn content_text(content: &ContentBlock) -> String {
    match content {
        ContentBlock::Text(text) => text.text.clone(),
        ContentBlock::ResourceLink(link) => format!("[{}]({})", link.name, link.uri),
        other => compact_json(&serde_json::to_value(other).unwrap_or(Value::Null)),
    }
}

fn tool_content_text(content: &[agent_client_protocol::schema::v1::ToolCallContent]) -> String {
    content
        .iter()
        .map(|item| compact_json(&serde_json::to_value(item).unwrap_or(Value::Null)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn tool_status(status: ToolCallStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "Unsupported agent update".to_owned())
}

/// Raw HTML is rendered as inert text. Markdown links remain native egui
/// hyperlinks and therefore open only after a user click.
pub fn safe_markdown(source: &str) -> String {
    source
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        AvailableCommandsUpdate, ContentChunk, MessageId, Plan, PlanEntry, PlanEntryPriority,
        PlanEntryStatus, ResourceLink, TextContent, ToolCall, ToolCallUpdate, ToolCallUpdateFields,
    };
    use serde_json::json;

    #[test]
    fn chunks_append_in_order_by_message_id() {
        let mut transcript = Transcript::default();
        for text in ["hello ", "world"] {
            transcript.apply_update(SessionUpdate::AgentMessageChunk(
                ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
                    .message_id(MessageId::new("a")),
            ));
        }
        assert_eq!(transcript.items().len(), 1);
        assert_eq!(transcript.items()[0].text, "hello world");
    }

    #[test]
    fn tool_updates_and_mcp_result_patch_in_place_without_duplicates() {
        let mut transcript = Transcript::default();
        transcript.apply_update(SessionUpdate::ToolCall(ToolCall::new(
            "call-1",
            "Create box",
        )));
        transcript.apply_update(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            "call-1",
            ToolCallUpdateFields::new().title("Creating box"),
        )));
        transcript.apply_mcp_result(
            Some("call-1"),
            "geometry.create",
            json!({"status": "ok"}),
            false,
        );
        assert_eq!(transcript.items().len(), 1);
        assert_eq!(transcript.items()[0].title.as_deref(), Some("Creating box"));
        assert_eq!(transcript.items()[0].status.as_deref(), Some("completed"));
    }

    #[test]
    fn reasoning_plans_links_warnings_and_fallback_updates_remain_ordered() {
        let mut transcript = Transcript::default();
        transcript.apply_update(SessionUpdate::AgentThoughtChunk(
            ContentChunk::new(ContentBlock::Text(TextContent::new("checking geometry")))
                .message_id(MessageId::new("thought-1")),
        ));
        transcript.apply_update(SessionUpdate::AgentMessageChunk(
            ContentChunk::new(ContentBlock::ResourceLink(ResourceLink::new(
                "mesh report",
                "https://example.com/mesh",
            )))
            .message_id(MessageId::new("link-1")),
        ));
        transcript.apply_update(SessionUpdate::Plan(Plan::new(vec![PlanEntry::new(
            "Create and mesh the body",
            PlanEntryPriority::High,
            PlanEntryStatus::InProgress,
        )])));
        transcript.push_warning("The agent reported a recoverable warning.");
        transcript.apply_update(SessionUpdate::AvailableCommandsUpdate(
            AvailableCommandsUpdate::new(Vec::new()),
        ));

        let items = transcript.items();
        assert_eq!(items.len(), 5);
        assert_eq!(items[0].kind, TranscriptKind::Reasoning);
        assert_eq!(items[0].text, "checking geometry");
        assert_eq!(items[1].kind, TranscriptKind::Agent);
        assert_eq!(items[1].text, "[mesh report](https://example.com/mesh)");
        assert_eq!(items[2].kind, TranscriptKind::Plan);
        assert!(items[2].text.contains("Create and mesh the body"));
        assert_eq!(items[3].kind, TranscriptKind::Warning);
        assert_eq!(items[4].kind, TranscriptKind::Extension);
        assert!(items[4].text.contains("available_commands_update"));
    }

    #[test]
    fn html_and_scripts_are_inert_but_markdown_links_remain() {
        let source = "# Heading\n\n- one\n- two\n\n**bold** and *emphasis*\n\n```rust\nlet value = 1;\n```\n\n<script>alert(1)</script> [Open](https://example.com)";
        let rendered = safe_markdown(source);
        assert!(rendered.contains("&lt;script&gt;"));
        assert!(!rendered.contains("<script>"));
        assert!(rendered.contains("# Heading"));
        assert!(rendered.contains("- one\n- two"));
        assert!(rendered.contains("**bold** and *emphasis*"));
        assert!(rendered.contains("```rust\nlet value = 1;\n```"));
        assert!(rendered.contains("[Open](https://example.com)"));
    }
}

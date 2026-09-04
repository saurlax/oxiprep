//! Typed application operations shared by GUI actions and MCP tools.

use crate::document::{BodyShape, Document, Selection};
use crate::geometry::{Axis, CreateKind, Plane};
use crate::mesh::MeshKind;
use crate::session::Session;
use crate::viewport::{ClipAxis, Viewport};
use cadrum::DVec3;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectClass {
    Query,
    UndoableMutation,
    ProjectMutation,
    View,
    Termination,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationClass {
    None,
    DirtyDocumentReplacement,
    NewExternalWriteTarget,
    Termination,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AppOperationSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub parameter_schema: Value,
    pub result_kind: &'static str,
    pub effect: EffectClass,
    pub agent_callable: bool,
    pub confirmation: ConfirmationClass,
}

impl AppOperationSpec {
    fn new(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        parameter_schema: Value,
        effect: EffectClass,
    ) -> Self {
        Self {
            id,
            title,
            description,
            parameter_schema,
            result_kind: "app_operation_result",
            effect,
            agent_callable: true,
            confirmation: ConfirmationClass::None,
        }
    }

    fn confirmation(mut self, confirmation: ConfirmationClass) -> Self {
        self.confirmation = confirmation;
        self
    }

    fn host_only(mut self) -> Self {
        self.agent_callable = false;
        self
    }
}

fn empty_schema() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

fn path_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"path": {"type": "string", "description": "Absolute local path"}},
        "required": ["path"],
        "additionalProperties": false
    })
}

fn targeted_schema(properties: Value, required: &[&str]) -> Value {
    let mut map = properties.as_object().cloned().unwrap_or_default();
    map.insert(
        "targets".to_owned(),
        json!({"type": "array", "items": entity_schema()}),
    );
    let mut required = required.to_vec();
    required.push("targets");
    revisioned_schema(Value::Object(map), &required)
}

fn revisioned_schema(properties: Value, required: &[&str]) -> Value {
    let mut required = required
        .iter()
        .map(|s| Value::String((*s).to_owned()))
        .collect::<Vec<_>>();
    required.push(Value::String("revision".to_owned()));
    let mut map = properties.as_object().cloned().unwrap_or_default();
    map.insert(
        "revision".to_owned(),
        json!({"type": "integer", "minimum": 0}),
    );
    json!({"type": "object", "properties": map, "required": required, "additionalProperties": false})
}

fn entity_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kind": {"enum": ["model", "body", "face", "edge", "vertex", "node", "cell", "mesh_edge"]},
            "model": {"type": "integer", "minimum": 0},
            "body": {"type": "integer", "minimum": 0},
            "id": {"type": "integer", "minimum": 0},
            "index": {"type": "integer", "minimum": 0},
            "a": {"type": "integer", "minimum": 0},
            "b": {"type": "integer", "minimum": 0}
        },
        "required": ["kind", "model"],
        "additionalProperties": false
    })
}

pub fn operation_specs() -> Vec<AppOperationSpec> {
    let create_schema = json!({
        "type": "object",
        "properties": {
            "kind": {"enum": ["point", "line", "rectangle", "disk", "box", "cylinder", "cone", "sphere"]},
            "p": {"$ref": "#/$defs/vector"}, "a": {"$ref": "#/$defs/vector"}, "b": {"$ref": "#/$defs/vector"},
            "origin": {"$ref": "#/$defs/vector"}, "center": {"$ref": "#/$defs/vector"}, "size": {"$ref": "#/$defs/vector"},
            "plane": {"enum": ["xy", "yz", "xz"]}, "axis": {"enum": ["x", "y", "z"]},
            "width": {"type": "number", "exclusiveMinimum": 0}, "height": {"type": "number", "exclusiveMinimum": 0},
            "radius": {"type": "number", "exclusiveMinimum": 0}, "r1": {"type": "number", "minimum": 0}, "r2": {"type": "number", "minimum": 0},
            "add_to_model": {"type": "integer", "minimum": 0}
        },
        "required": ["kind"], "additionalProperties": false,
        "$defs": {"vector": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}
    });
    vec![
        AppOperationSpec::new(
            "context.get",
            "Get context",
            "Get the live project, document, selection, history, revision, and view state.",
            empty_schema(),
            EffectClass::Query,
        ),
        AppOperationSpec::new(
            "operations.list",
            "List operations",
            "List the application operations available to the agent.",
            empty_schema(),
            EffectClass::Query,
        ),
        AppOperationSpec::new(
            "project.new",
            "New project",
            "Replace the current document with a new empty project.",
            empty_schema(),
            EffectClass::ProjectMutation,
        )
        .confirmation(ConfirmationClass::DirtyDocumentReplacement),
        AppOperationSpec::new(
            "project.open",
            "Open project",
            "Open an Oxiprep project from an absolute path.",
            path_schema(),
            EffectClass::ProjectMutation,
        )
        .confirmation(ConfirmationClass::DirtyDocumentReplacement),
        AppOperationSpec::new(
            "project.save",
            "Save project",
            "Save to the current project path.",
            empty_schema(),
            EffectClass::ProjectMutation,
        ),
        AppOperationSpec::new(
            "project.save_as",
            "Save project as",
            "Save to an explicit absolute project path.",
            path_schema(),
            EffectClass::ProjectMutation,
        )
        .confirmation(ConfirmationClass::NewExternalWriteTarget),
        AppOperationSpec::new(
            "document.import",
            "Import model",
            "Import STEP, BRep, or STL from an absolute path.",
            path_schema(),
            EffectClass::UndoableMutation,
        ),
        AppOperationSpec::new(
            "geometry.create",
            "Create geometry",
            "Create one of Oxiprep's shipped geometry primitives.",
            create_schema,
            EffectClass::UndoableMutation,
        ),
        AppOperationSpec::new(
            "mesh.generate",
            "Generate mesh",
            "Generate a surface or volume mesh for validated CAD-solid targets. Omit size to use Oxiprep's normal target bounding-box diagonal / 8 default.",
            targeted_schema(
                json!({"kind": {"enum": ["surface", "volume"]}, "size": {"type": "number", "exclusiveMinimum": 0}}),
                &["kind"],
            ),
            EffectClass::UndoableMutation,
        ),
        AppOperationSpec::new(
            "document.close",
            "Close model",
            "Close a model by index.",
            revisioned_schema(
                json!({"model": {"type": "integer", "minimum": 0}}),
                &["model"],
            ),
            EffectClass::UndoableMutation,
        ),
        AppOperationSpec::new(
            "document.delete",
            "Delete selection",
            "Delete validated target references.",
            targeted_schema(json!({}), &[]),
            EffectClass::UndoableMutation,
        ),
        AppOperationSpec::new(
            "history.undo",
            "Undo",
            "Undo the last document command.",
            empty_schema(),
            EffectClass::UndoableMutation,
        ),
        AppOperationSpec::new(
            "history.redo",
            "Redo",
            "Redo the next document command.",
            empty_schema(),
            EffectClass::UndoableMutation,
        ),
        AppOperationSpec::new(
            "view.fit_all",
            "Fit all",
            "Fit all document geometry in the viewport.",
            empty_schema(),
            EffectClass::View,
        ),
        AppOperationSpec::new(
            "view.fit_selection",
            "Fit selection",
            "Fit the current selection in the viewport.",
            empty_schema(),
            EffectClass::View,
        ),
        AppOperationSpec::new(
            "view.standard",
            "Standard view",
            "Look from a standard axis or isometric direction.",
            json!({"type": "object", "properties": {"direction": {"enum": ["+x", "-x", "+y", "-y", "+z", "-z", "isometric"]}}, "required": ["direction"], "additionalProperties": false}),
            EffectClass::View,
        ),
        AppOperationSpec::new(
            "view.display",
            "Display options",
            "Set one or more viewport visibility toggles.",
            json!({"type": "object", "properties": {"faces": {"type": "boolean"}, "edges": {"type": "boolean"}, "mesh": {"type": "boolean"}, "vertices": {"type": "boolean"}}, "minProperties": 1, "additionalProperties": false}),
            EffectClass::View,
        ),
        AppOperationSpec::new(
            "view.clip",
            "Clip plane",
            "Set axis clip visibility, axis, position, and direction.",
            json!({"type": "object", "properties": {"enabled": {"type": "boolean"}, "axis": {"enum": ["x", "y", "z"]}, "position": {"type": "number", "minimum": 0, "maximum": 1}, "flip": {"type": "boolean"}}, "additionalProperties": false}),
            EffectClass::View,
        ),
        AppOperationSpec::new(
            "app.quit",
            "Quit",
            "Close Oxiprep.",
            empty_schema(),
            EffectClass::Termination,
        )
        .confirmation(ConfirmationClass::Termination)
        .host_only(),
    ]
}

pub fn agent_operation_specs() -> Vec<AppOperationSpec> {
    operation_specs()
        .into_iter()
        .filter(|spec| spec.agent_callable)
        .collect()
}

pub fn selected_entities(document: &Document) -> Vec<EntityRef> {
    document
        .selection
        .iter()
        .copied()
        .map(EntityRef::from)
        .collect()
}

pub fn create_arguments(kind: CreateKind, add_to_model: Option<usize>) -> Value {
    let mut value = match kind {
        CreateKind::Point { p } => json!({"kind": "point", "p": p}),
        CreateKind::Line { a, b } => json!({"kind": "line", "a": a, "b": b}),
        CreateKind::Rectangle {
            plane,
            origin,
            width,
            height,
        } => {
            json!({"kind": "rectangle", "plane": plane_name(plane), "origin": origin, "width": width, "height": height})
        }
        CreateKind::Disk {
            plane,
            center,
            radius,
        } => {
            json!({"kind": "disk", "plane": plane_name(plane), "center": center, "radius": radius})
        }
        CreateKind::Box { origin, size } => json!({"kind": "box", "origin": origin, "size": size}),
        CreateKind::Cylinder {
            center,
            axis,
            radius,
            height,
        } => {
            json!({"kind": "cylinder", "center": center, "axis": axis_name(axis), "radius": radius, "height": height})
        }
        CreateKind::Cone {
            center,
            axis,
            r1,
            r2,
            height,
        } => {
            json!({"kind": "cone", "center": center, "axis": axis_name(axis), "r1": r1, "r2": r2, "height": height})
        }
        CreateKind::Sphere { center, radius } => {
            json!({"kind": "sphere", "center": center, "radius": radius})
        }
    };
    if let Some(model) = add_to_model {
        value
            .as_object_mut()
            .unwrap()
            .insert("add_to_model".to_owned(), json!(model));
    }
    value
}

fn plane_name(plane: Plane) -> &'static str {
    match plane {
        Plane::XY => "xy",
        Plane::YZ => "yz",
        Plane::XZ => "xz",
    }
}

fn axis_name(axis: Axis) -> &'static str {
    match axis {
        Axis::X => "x",
        Axis::Y => "y",
        Axis::Z => "z",
    }
}

pub fn validate_registry(specs: &[AppOperationSpec]) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    for spec in specs {
        if !ids.insert(spec.id) {
            return Err(format!("Duplicate operation ID: {}", spec.id));
        }
        if spec.id.is_empty() || spec.title.is_empty() || spec.description.is_empty() {
            return Err("Operation metadata must not be empty.".to_owned());
        }
        if spec.parameter_schema.get("type") != Some(&Value::String("object".to_owned())) {
            return Err(format!(
                "{} does not have an object parameter schema",
                spec.id
            ));
        }
        if spec.result_kind != "app_operation_result" {
            return Err(format!("{} has an unknown result classification", spec.id));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppOperationRequest {
    pub id: String,
    #[serde(default)]
    pub arguments: Value,
}

impl AppOperationRequest {
    pub fn new(id: impl Into<String>, arguments: Value) -> Self {
        Self {
            id: id.into(),
            arguments,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntityRef {
    Model {
        model: usize,
    },
    Body {
        model: usize,
        body: usize,
    },
    Face {
        model: usize,
        body: usize,
        id: u64,
    },
    Edge {
        model: usize,
        body: usize,
        id: u64,
    },
    Vertex {
        model: usize,
        body: usize,
        index: u32,
    },
    Node {
        model: usize,
        body: usize,
        index: u32,
    },
    Cell {
        model: usize,
        body: usize,
        index: u32,
    },
    MeshEdge {
        model: usize,
        body: usize,
        a: u32,
        b: u32,
    },
}

impl From<Selection> for EntityRef {
    fn from(value: Selection) -> Self {
        match value {
            Selection::Model(model) => Self::Model { model },
            Selection::Body { model, body } => Self::Body { model, body },
            Selection::Face { model, body, id } => Self::Face { model, body, id },
            Selection::Edge { model, body, id } => Self::Edge { model, body, id },
            Selection::Vertex { model, body, index } => Self::Vertex { model, body, index },
            Selection::Node { model, body, index } => Self::Node { model, body, index },
            Selection::Cell { model, body, index } => Self::Cell { model, body, index },
            Selection::MeshEdge { model, body, a, b } => Self::MeshEdge { model, body, a, b },
        }
    }
}

impl EntityRef {
    fn resolve(&self, document: &Document) -> Result<Selection, OperationError> {
        let selection = match *self {
            Self::Model { model } => Selection::Model(model),
            Self::Body { model, body } => Selection::Body { model, body },
            Self::Face { model, body, id } => Selection::Face { model, body, id },
            Self::Edge { model, body, id } => Selection::Edge { model, body, id },
            Self::Vertex { model, body, index } => Selection::Vertex { model, body, index },
            Self::Node { model, body, index } => Selection::Node { model, body, index },
            Self::Cell { model, body, index } => Selection::Cell { model, body, index },
            Self::MeshEdge { model, body, a, b } => Selection::MeshEdge { model, body, a, b },
        };
        validate_selection(document, selection)?;
        Ok(selection)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppOperationResult {
    pub status: OperationStatus,
    pub revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<EntityRef>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Ok,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OperationError {
    UnknownOperation(String),
    InvalidArguments(String),
    StaleRevision { expected: u64, actual: u64 },
    MissingTarget(String),
    ConfirmationRequired { operation: String, detail: String },
    Rejected,
    Failed(String),
}

impl std::fmt::Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownOperation(id) => write!(f, "Unsupported operation: {id}"),
            Self::InvalidArguments(message) => write!(f, "Invalid arguments: {message}"),
            Self::StaleRevision { expected, actual } => write!(
                f,
                "Stale document revision {expected}; current revision is {actual}."
            ),
            Self::MissingTarget(message) => f.write_str(message),
            Self::ConfirmationRequired { operation, detail } => {
                write!(f, "{operation} requires host confirmation: {detail}")
            }
            Self::Rejected => f.write_str("The host rejected the operation."),
            Self::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for OperationError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostApproval {
    NotRequired,
    Approved,
    Rejected,
}

pub fn confirmation_for(
    request: &AppOperationRequest,
    session: &Session,
) -> Result<Option<String>, OperationError> {
    match request.id.as_str() {
        "project.new" if session.document.dirty => {
            Ok(Some("Discard the unsaved current document".to_owned()))
        }
        "project.open" if session.document.dirty => Ok(Some(format!(
            "Discard the unsaved current document and open {}",
            absolute_path(&request.arguments)?.display()
        ))),
        "project.save_as" => {
            let path = absolute_path(&request.arguments)?;
            if session.document.path.as_deref() == Some(path.as_path()) {
                Ok(None)
            } else {
                Ok(Some(format!(
                    "Write a new external project target: {}",
                    path.display()
                )))
            }
        }
        "app.quit" => Ok(Some("Terminate Oxiprep".to_owned())),
        _ => Ok(None),
    }
}

pub fn dispatch(
    request: &AppOperationRequest,
    session: &mut Session,
    viewport: &mut Viewport,
    approval: HostApproval,
) -> Result<AppOperationResult, OperationError> {
    let specs = operation_specs();
    let spec = specs
        .iter()
        .find(|spec| spec.id == request.id)
        .ok_or_else(|| OperationError::UnknownOperation(request.id.clone()))?;
    validate_value(
        &request.arguments,
        &spec.parameter_schema,
        &spec.parameter_schema,
        "arguments",
    )?;
    if let Some(detail) = confirmation_for(request, session)? {
        match approval {
            HostApproval::Approved => {}
            HostApproval::Rejected => return Err(OperationError::Rejected),
            HostApproval::NotRequired => {
                return Err(OperationError::ConfirmationRequired {
                    operation: request.id.clone(),
                    detail,
                });
            }
        }
    }

    let mut message = None;
    let mut data = Value::Null;
    match request.id.as_str() {
        "context.get" => data = live_context(session, viewport),
        "operations.list" => {
            data = serde_json::to_value(agent_operation_specs()).map_err(failed)?
        }
        "project.new" => {
            ensure_empty(&request.arguments)?;
            session.new_project();
            message = Some("New project.".to_owned());
        }
        "project.open" => {
            let path = absolute_path(&request.arguments)?;
            message = Some(
                session
                    .open_project(&path)
                    .map_err(|e| OperationError::Failed(e.message().to_owned()))?,
            );
            if let Some(bbox) = session.document.bbox() {
                viewport.fit(bbox);
            }
        }
        "project.save" => {
            ensure_empty(&request.arguments)?;
            message = Some(
                session
                    .save()
                    .map_err(|e| OperationError::Failed(e.message().to_owned()))?,
            );
        }
        "project.save_as" => {
            let path = absolute_path(&request.arguments)?;
            message = Some(
                session
                    .save_to(&path)
                    .map_err(|e| OperationError::Failed(e.message().to_owned()))?,
            );
        }
        "document.import" => {
            let path = absolute_path(&request.arguments)?;
            message = Some(session.import_path(&path).map_err(command_failed)?);
            if let Some(bbox) = session.document.selection_bbox() {
                viewport.fit(bbox);
            }
        }
        "geometry.create" => {
            let (kind, add_to_model) = parse_create(&request.arguments)?;
            if !kind.valid() {
                return Err(OperationError::InvalidArguments(
                    "Geometry dimensions are degenerate.".to_owned(),
                ));
            }
            message = Some(if let Some(model) = add_to_model {
                if session.document.models.get(model).is_none() {
                    return Err(OperationError::MissingTarget(format!(
                        "Model {model} does not exist."
                    )));
                }
                let body = kind
                    .into_body(&session.document, model)
                    .map_err(command_failed)?;
                session.add_body(model, body).map_err(command_failed)?
            } else {
                let model = kind.into_model(&session.document).map_err(command_failed)?;
                session.create_model(model).map_err(command_failed)?
            });
            if let Some(bbox) = session.document.selection_bbox() {
                viewport.fit(bbox);
            }
        }
        "mesh.generate" => {
            let revision = required_u64(&request.arguments, "revision")?;
            require_revision(session, revision)?;
            let targets = resolve_targets(&request.arguments, &session.document)?;
            let (kind, kind_name) = match required_str(&request.arguments, "kind")? {
                "surface" => (MeshKind::Surface, "surface"),
                "volume" => (MeshKind::Volume, "volume"),
                other => {
                    return Err(OperationError::InvalidArguments(format!(
                        "Unknown mesh kind: {other}"
                    )));
                }
            };
            let requested_size = request
                .arguments
                .get("size")
                .map(|_| required_f64(&request.arguments, "size"))
                .transpose()?;
            if requested_size.is_some_and(|size| size <= 0.0) {
                return Err(OperationError::InvalidArguments(
                    "size must be greater than zero".to_owned(),
                ));
            }
            let (mesh_message, actual_size) = with_targets(session, targets, |session| {
                let size =
                    requested_size.unwrap_or_else(|| crate::mesh::default_size(&session.document));
                session
                    .mesh_selected(kind, size)
                    .map(|message| (message, size))
            })?;
            message = Some(mesh_message);
            data = json!({"kind": kind_name, "size": actual_size});
            viewport.display.mesh = true;
        }
        "document.close" => {
            require_revision(session, required_u64(&request.arguments, "revision")?)?;
            let model = required_u64(&request.arguments, "model")? as usize;
            if session.document.models.get(model).is_none() {
                return Err(OperationError::MissingTarget(format!(
                    "Model {model} does not exist."
                )));
            }
            message = Some(session.close_model(model).map_err(command_failed)?);
        }
        "document.delete" => {
            require_revision(session, required_u64(&request.arguments, "revision")?)?;
            let targets = resolve_targets(&request.arguments, &session.document)?;
            if targets.is_empty() {
                return Err(OperationError::MissingTarget(
                    "At least one target is required.".to_owned(),
                ));
            }
            message = Some(with_targets(session, targets, Session::delete_selected)?);
        }
        "history.undo" => {
            ensure_empty(&request.arguments)?;
            message = session
                .undo()
                .map_err(command_failed)?
                .or(Some("Nothing to undo.".to_owned()));
        }
        "history.redo" => {
            ensure_empty(&request.arguments)?;
            message = session
                .redo()
                .map_err(command_failed)?
                .or(Some("Nothing to redo.".to_owned()));
        }
        "view.fit_all" => {
            ensure_empty(&request.arguments)?;
            let bbox = session.document.bbox().ok_or_else(|| {
                OperationError::MissingTarget("There is no geometry to fit.".to_owned())
            })?;
            viewport.fit(bbox);
            message = Some("Fit all.".to_owned());
        }
        "view.fit_selection" => {
            ensure_empty(&request.arguments)?;
            let bbox = session.document.selection_bbox().ok_or_else(|| {
                OperationError::MissingTarget("There is no selection to fit.".to_owned())
            })?;
            viewport.fit(bbox);
            message = Some("Fit selection.".to_owned());
        }
        "view.standard" => {
            apply_standard_view(viewport, required_str(&request.arguments, "direction")?)?;
            message = Some("Changed view.".to_owned());
        }
        "view.display" => {
            apply_display(viewport, &request.arguments)?;
            message = Some("Changed display options.".to_owned());
        }
        "view.clip" => {
            apply_clip(viewport, &request.arguments)?;
            message = Some("Changed clip plane.".to_owned());
        }
        "app.quit" => {
            return Err(OperationError::UnknownOperation(
                "app.quit is host-only".to_owned(),
            ));
        }
        _ => return Err(OperationError::UnknownOperation(request.id.clone())),
    }
    Ok(AppOperationResult {
        status: OperationStatus::Ok,
        revision: session.revision(),
        message,
        entities: session
            .document
            .selection
            .iter()
            .copied()
            .map(EntityRef::from)
            .collect(),
        data,
    })
}

fn live_context(session: &Session, viewport: &Viewport) -> Value {
    let models = session.document.models.iter().enumerate().map(|(model_index, model)| json!({
        "index": model_index, "name": model.name, "kind": model.kind.label(), "path": model.path,
        "bodies": model.bodies.iter().enumerate().map(|(body_index, body)| json!({
            "index": body_index,
            "name": body.name,
            "shape": match &body.shape {
                BodyShape::Solid(_) => "solid",
                BodyShape::Wire(_) => "wire",
                BodyShape::Vertex(_) => "vertex",
                BodyShape::Mesh => "mesh",
            },
            "meshable": matches!(&body.shape, BodyShape::Solid(_)),
            "has_mesh": body.has_discrete_mesh()
        })).collect::<Vec<_>>()
    })).collect::<Vec<_>>();
    json!({
        "project_path": session.document.path,
        "dirty": session.document.dirty,
        "revision": session.revision(),
        "suggested_mesh_size": crate::mesh::default_size(&session.document),
        "models": models,
        "selection": session.document.selection.iter().copied().map(EntityRef::from).collect::<Vec<_>>(),
        "history": {"can_undo": session.can_undo(), "can_redo": session.can_redo(), "undo_label": session.undo_label(), "redo_label": session.redo_label()},
        "view": {"faces": viewport.display.faces, "edges": viewport.display.edges, "mesh": viewport.display.mesh, "vertices": viewport.display.vertices, "clip": viewport.display.clip}
    })
}

/// A bounded routing hint for ACP prompts. Tool calls must still use
/// `context.get`, which returns the complete live state at execution time.
pub fn agent_prompt_snapshot(session: &Session, viewport: &Viewport) -> Value {
    const MAX_MODELS: usize = 8;
    const MAX_BODIES_PER_MODEL: usize = 16;
    const MAX_SELECTIONS: usize = 32;

    let models = session
        .document
        .models
        .iter()
        .enumerate()
        .take(MAX_MODELS)
        .map(|(model_index, model)| {
            let bodies = model
                .bodies
                .iter()
                .enumerate()
                .take(MAX_BODIES_PER_MODEL)
                .map(|(body_index, body)| {
                    json!({
                        "index": body_index,
                        "name": body.name,
                        "shape": match &body.shape {
                            BodyShape::Solid(_) => "solid",
                            BodyShape::Wire(_) => "wire",
                            BodyShape::Vertex(_) => "vertex",
                            BodyShape::Mesh => "mesh",
                        },
                        "meshable": matches!(&body.shape, BodyShape::Solid(_)),
                        "has_mesh": body.has_discrete_mesh()
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "index": model_index,
                "name": model.name,
                "body_count": model.bodies.len(),
                "bodies": bodies,
                "bodies_truncated": model.bodies.len() > MAX_BODIES_PER_MODEL
            })
        })
        .collect::<Vec<_>>();
    let selection = session
        .document
        .selection
        .iter()
        .copied()
        .take(MAX_SELECTIONS)
        .map(EntityRef::from)
        .collect::<Vec<_>>();

    json!({
        "revision": session.revision(),
        "project_path": session.document.path,
        "dirty": session.document.dirty,
        "suggested_mesh_size": crate::mesh::default_size(&session.document),
        "model_count": session.document.models.len(),
        "models": models,
        "models_truncated": session.document.models.len() > MAX_MODELS,
        "selection": selection,
        "selection_truncated": session.document.selection.len() > MAX_SELECTIONS,
        "view": {
            "faces": viewport.display.faces,
            "edges": viewport.display.edges,
            "mesh": viewport.display.mesh,
            "vertices": viewport.display.vertices
        }
    })
}

fn validate_selection(document: &Document, selection: Selection) -> Result<(), OperationError> {
    let model = document.models.get(selection.model()).ok_or_else(|| {
        OperationError::MissingTarget(format!("Model {} does not exist.", selection.model()))
    })?;
    let Some(body_index) = selection.body() else {
        return Ok(());
    };
    let body = model.bodies.get(body_index).ok_or_else(|| {
        OperationError::MissingTarget(format!(
            "Body {body_index} does not exist in model {}.",
            selection.model()
        ))
    })?;
    let valid = match selection {
        Selection::Face { id, .. } => body.display.triangle_face_ids.contains(&id),
        Selection::Edge { id, .. } => body.display.cad_edges.iter().any(|edge| edge.id == id),
        Selection::Vertex { index, .. } => (index as usize) < body.display.cad_vertices.len(),
        Selection::Node { index, .. } => (index as usize) < body.display.positions.len(),
        Selection::Cell { index, .. } => body.mesh.as_ref().is_some_and(|mesh| {
            (index as usize)
                < if mesh.tets.is_empty() {
                    mesh.triangles.len()
                } else {
                    mesh.tets.len()
                }
        }),
        Selection::MeshEdge { a, b, .. } => {
            (a as usize) < body.display.positions.len()
                && (b as usize) < body.display.positions.len()
        }
        Selection::Model(_) | Selection::Body { .. } => true,
    };
    if valid {
        Ok(())
    } else {
        Err(OperationError::MissingTarget(
            "The referenced entity no longer exists.".to_owned(),
        ))
    }
}

fn resolve_targets(
    arguments: &Value,
    document: &Document,
) -> Result<Vec<Selection>, OperationError> {
    let values = arguments
        .get("targets")
        .and_then(Value::as_array)
        .ok_or_else(|| OperationError::InvalidArguments("targets must be an array".to_owned()))?;
    values
        .iter()
        .map(|value| {
            serde_json::from_value::<EntityRef>(value.clone())
                .map_err(|e| OperationError::InvalidArguments(e.to_string()))?
                .resolve(document)
        })
        .collect()
}

fn with_targets<T>(
    session: &mut Session,
    targets: Vec<Selection>,
    operation: impl FnOnce(&mut Session) -> Result<T, crate::command::CommandError>,
) -> Result<T, OperationError> {
    let previous = std::mem::replace(&mut session.document.selection, targets);
    match operation(session) {
        Ok(message) => Ok(message),
        Err(error) => {
            session.document.selection = previous;
            Err(command_failed(error))
        }
    }
}

fn require_revision(session: &Session, expected: u64) -> Result<(), OperationError> {
    let actual = session.revision();
    if expected == actual {
        Ok(())
    } else {
        Err(OperationError::StaleRevision { expected, actual })
    }
}

fn absolute_path(arguments: &Value) -> Result<PathBuf, OperationError> {
    let path = PathBuf::from(required_str(arguments, "path")?);
    if !path.is_absolute() {
        return Err(OperationError::InvalidArguments(
            "path must be absolute".to_owned(),
        ));
    }
    Ok(path)
}

fn ensure_empty(arguments: &Value) -> Result<(), OperationError> {
    if arguments.is_null() || arguments.as_object().is_some_and(serde_json::Map::is_empty) {
        Ok(())
    } else {
        Err(OperationError::InvalidArguments(
            "this operation accepts no arguments".to_owned(),
        ))
    }
}

fn required_str<'a>(arguments: &'a Value, name: &str) -> Result<&'a str, OperationError> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| OperationError::InvalidArguments(format!("{name} must be a string")))
}

fn required_u64(arguments: &Value, name: &str) -> Result<u64, OperationError> {
    arguments.get(name).and_then(Value::as_u64).ok_or_else(|| {
        OperationError::InvalidArguments(format!("{name} must be a non-negative integer"))
    })
}

fn required_f64(arguments: &Value, name: &str) -> Result<f64, OperationError> {
    arguments
        .get(name)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| OperationError::InvalidArguments(format!("{name} must be a finite number")))
}

fn validate_value(
    value: &Value,
    schema: &Value,
    root: &Value,
    path: &str,
) -> Result<(), OperationError> {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let resolved = reference
            .strip_prefix("#/")
            .and_then(|pointer| root.pointer(&format!("/{pointer}")))
            .ok_or_else(|| {
                OperationError::InvalidArguments(format!(
                    "{path} uses an unresolved schema reference"
                ))
            })?;
        return validate_value(value, resolved, root, path);
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array)
        && !values.contains(value)
    {
        return Err(OperationError::InvalidArguments(format!(
            "{path} has an unsupported value"
        )));
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("object") => {
            let object = value.as_object().ok_or_else(|| {
                OperationError::InvalidArguments(format!("{path} must be an object"))
            })?;
            if let Some(minimum) = schema.get("minProperties").and_then(Value::as_u64)
                && object.len() < minimum as usize
            {
                return Err(OperationError::InvalidArguments(format!(
                    "{path} must contain at least {minimum} properties"
                )));
            }
            let properties = schema
                .get("properties")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            for required in schema
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !object.contains_key(required) {
                    return Err(OperationError::InvalidArguments(format!(
                        "{path}.{required} is required"
                    )));
                }
            }
            if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                for key in object.keys() {
                    if !properties.contains_key(key) {
                        return Err(OperationError::InvalidArguments(format!(
                            "{path}.{key} is unsupported"
                        )));
                    }
                }
            }
            for (name, child) in object {
                if let Some(child_schema) = properties.get(name) {
                    validate_value(child, child_schema, root, &format!("{path}.{name}"))?;
                }
            }
        }
        Some("array") => {
            let array = value.as_array().ok_or_else(|| {
                OperationError::InvalidArguments(format!("{path} must be an array"))
            })?;
            if let Some(minimum) = schema.get("minItems").and_then(Value::as_u64)
                && array.len() < minimum as usize
            {
                return Err(OperationError::InvalidArguments(format!(
                    "{path} must contain at least {minimum} items"
                )));
            }
            if let Some(maximum) = schema.get("maxItems").and_then(Value::as_u64)
                && array.len() > maximum as usize
            {
                return Err(OperationError::InvalidArguments(format!(
                    "{path} must contain at most {maximum} items"
                )));
            }
            if let Some(item_schema) = schema.get("items") {
                for (index, item) in array.iter().enumerate() {
                    validate_value(item, item_schema, root, &format!("{path}[{index}]"))?;
                }
            }
        }
        Some("string") if !value.is_string() => {
            return Err(OperationError::InvalidArguments(format!(
                "{path} must be a string"
            )));
        }
        Some("boolean") if !value.is_boolean() => {
            return Err(OperationError::InvalidArguments(format!(
                "{path} must be boolean"
            )));
        }
        Some("integer") if value.as_u64().is_none() => {
            return Err(OperationError::InvalidArguments(format!(
                "{path} must be a non-negative integer"
            )));
        }
        Some("number") => {
            let number = value
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| {
                    OperationError::InvalidArguments(format!("{path} must be a finite number"))
                })?;
            if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64)
                && number < minimum
            {
                return Err(OperationError::InvalidArguments(format!(
                    "{path} must be at least {minimum}"
                )));
            }
            if let Some(minimum) = schema.get("exclusiveMinimum").and_then(Value::as_f64)
                && number <= minimum
            {
                return Err(OperationError::InvalidArguments(format!(
                    "{path} must be greater than {minimum}"
                )));
            }
            if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64)
                && number > maximum
            {
                return Err(OperationError::InvalidArguments(format!(
                    "{path} must be at most {maximum}"
                )));
            }
        }
        Some("string" | "boolean" | "integer") | None => {}
        Some(other) => {
            return Err(OperationError::InvalidArguments(format!(
                "{path} uses unsupported schema type {other}"
            )));
        }
    }
    Ok(())
}

fn vector(arguments: &Value, name: &str, default: [f64; 3]) -> Result<[f64; 3], OperationError> {
    let Some(value) = arguments.get(name) else {
        return Ok(default);
    };
    let array = value
        .as_array()
        .filter(|array| array.len() == 3)
        .ok_or_else(|| {
            OperationError::InvalidArguments(format!("{name} must contain three numbers"))
        })?;
    let mut out = [0.0; 3];
    for (index, value) in array.iter().enumerate() {
        out[index] = value.as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
            OperationError::InvalidArguments(format!("{name} must contain finite numbers"))
        })?;
    }
    Ok(out)
}

fn optional_f64(arguments: &Value, name: &str, default: f64) -> Result<f64, OperationError> {
    match arguments.get(name) {
        Some(_) => required_f64(arguments, name),
        None => Ok(default),
    }
}

fn parse_create(arguments: &Value) -> Result<(CreateKind, Option<usize>), OperationError> {
    let add = arguments
        .get("add_to_model")
        .map(|_| required_u64(arguments, "add_to_model").map(|v| v as usize))
        .transpose()?;
    let kind = match required_str(arguments, "kind")? {
        "point" => CreateKind::Point {
            p: vector(arguments, "p", [0.0; 3])?,
        },
        "line" => CreateKind::Line {
            a: vector(arguments, "a", [0.0; 3])?,
            b: vector(arguments, "b", [1.0, 0.0, 0.0])?,
        },
        "rectangle" => CreateKind::Rectangle {
            plane: plane(arguments)?,
            origin: vector(arguments, "origin", [0.0; 3])?,
            width: optional_f64(arguments, "width", 1.0)?,
            height: optional_f64(arguments, "height", 1.0)?,
        },
        "disk" => CreateKind::Disk {
            plane: plane(arguments)?,
            center: vector(arguments, "center", [0.0; 3])?,
            radius: optional_f64(arguments, "radius", 1.0)?,
        },
        "box" => CreateKind::Box {
            origin: vector(arguments, "origin", [0.0; 3])?,
            size: vector(arguments, "size", [1.0; 3])?,
        },
        "cylinder" => CreateKind::Cylinder {
            center: vector(arguments, "center", [0.0; 3])?,
            axis: axis(arguments)?,
            radius: optional_f64(arguments, "radius", 0.5)?,
            height: optional_f64(arguments, "height", 1.0)?,
        },
        "cone" => CreateKind::Cone {
            center: vector(arguments, "center", [0.0; 3])?,
            axis: axis(arguments)?,
            r1: optional_f64(arguments, "r1", 0.5)?,
            r2: optional_f64(arguments, "r2", 0.0)?,
            height: optional_f64(arguments, "height", 1.0)?,
        },
        "sphere" => CreateKind::Sphere {
            center: vector(arguments, "center", [0.0; 3])?,
            radius: optional_f64(arguments, "radius", 1.0)?,
        },
        other => {
            return Err(OperationError::InvalidArguments(format!(
                "Unknown geometry kind: {other}"
            )));
        }
    };
    Ok((kind, add))
}

fn plane(arguments: &Value) -> Result<Plane, OperationError> {
    match arguments
        .get("plane")
        .and_then(Value::as_str)
        .unwrap_or("xy")
    {
        "xy" => Ok(Plane::XY),
        "yz" => Ok(Plane::YZ),
        "xz" => Ok(Plane::XZ),
        other => Err(OperationError::InvalidArguments(format!(
            "Unknown plane: {other}"
        ))),
    }
}

fn axis(arguments: &Value) -> Result<Axis, OperationError> {
    match arguments.get("axis").and_then(Value::as_str).unwrap_or("z") {
        "x" => Ok(Axis::X),
        "y" => Ok(Axis::Y),
        "z" => Ok(Axis::Z),
        other => Err(OperationError::InvalidArguments(format!(
            "Unknown axis: {other}"
        ))),
    }
}

fn apply_standard_view(viewport: &mut Viewport, direction: &str) -> Result<(), OperationError> {
    match direction {
        "+x" => viewport.look_along(DVec3::X),
        "-x" => viewport.look_along(-DVec3::X),
        "+y" => viewport.look_along(DVec3::Y),
        "-y" => viewport.look_along(-DVec3::Y),
        "+z" => viewport.look_along(DVec3::Z),
        "-z" => viewport.look_along(-DVec3::Z),
        "isometric" => viewport.look_isometric(),
        other => {
            return Err(OperationError::InvalidArguments(format!(
                "Unknown direction: {other}"
            )));
        }
    }
    Ok(())
}

fn apply_display(viewport: &mut Viewport, arguments: &Value) -> Result<(), OperationError> {
    let object = arguments.as_object().ok_or_else(|| {
        OperationError::InvalidArguments("arguments must be an object".to_owned())
    })?;
    if object.is_empty() {
        return Err(OperationError::InvalidArguments(
            "at least one display option is required".to_owned(),
        ));
    }
    for (name, value) in object {
        let value = value
            .as_bool()
            .ok_or_else(|| OperationError::InvalidArguments(format!("{name} must be boolean")))?;
        match name.as_str() {
            "faces" => viewport.display.faces = value,
            "edges" => viewport.display.edges = value,
            "mesh" => viewport.display.mesh = value,
            "vertices" => viewport.display.vertices = value,
            _ => {
                return Err(OperationError::InvalidArguments(format!(
                    "Unknown display option: {name}"
                )));
            }
        }
    }
    Ok(())
}

fn apply_clip(viewport: &mut Viewport, arguments: &Value) -> Result<(), OperationError> {
    let object = arguments.as_object().ok_or_else(|| {
        OperationError::InvalidArguments("arguments must be an object".to_owned())
    })?;
    for (name, value) in object {
        match name.as_str() {
            "enabled" => {
                viewport.display.clip = value.as_bool().ok_or_else(|| {
                    OperationError::InvalidArguments("enabled must be boolean".to_owned())
                })?
            }
            "axis" => {
                viewport.display.clip_axis = match value.as_str() {
                    Some("x") => ClipAxis::X,
                    Some("y") => ClipAxis::Y,
                    Some("z") => ClipAxis::Z,
                    _ => {
                        return Err(OperationError::InvalidArguments(
                            "axis must be x, y, or z".to_owned(),
                        ));
                    }
                }
            }
            "position" => {
                let position = value
                    .as_f64()
                    .filter(|v| (0.0..=1.0).contains(v))
                    .ok_or_else(|| {
                        OperationError::InvalidArguments(
                            "position must be between 0 and 1".to_owned(),
                        )
                    })?;
                viewport.display.clip_t = position as f32;
            }
            "flip" => {
                viewport.display.clip_flip = value.as_bool().ok_or_else(|| {
                    OperationError::InvalidArguments("flip must be boolean".to_owned())
                })?
            }
            _ => {
                return Err(OperationError::InvalidArguments(format!(
                    "Unknown clip option: {name}"
                )));
            }
        }
    }
    Ok(())
}

fn command_failed(error: crate::command::CommandError) -> OperationError {
    OperationError::Failed(error.message().to_owned())
}
fn failed(error: serde_json::Error) -> OperationError {
    OperationError::Failed(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport() -> Viewport {
        Viewport::new(None)
    }

    #[test]
    fn agent_prompt_snapshot_identifies_the_live_gui_without_replacing_context_get() {
        let mut session = Session::new();
        let mut view = viewport();
        dispatch(
            &AppOperationRequest::new("geometry.create", json!({"kind": "box"})),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap();

        let snapshot = agent_prompt_snapshot(&session, &view);
        assert_eq!(snapshot["revision"], 1);
        assert_eq!(snapshot["model_count"], 1);
        assert_eq!(snapshot["models"][0]["bodies"][0]["shape"], "solid");
        assert_eq!(snapshot["models"][0]["bodies"][0]["meshable"], true);
        assert!(snapshot["suggested_mesh_size"].as_f64().unwrap() > 0.0);
        assert!(snapshot.get("history").is_none());
    }

    #[test]
    fn registry_has_unique_valid_specs_and_complete_shipped_surface() {
        let specs = operation_specs();
        validate_registry(&specs).unwrap();
        let ids = specs.iter().map(|spec| spec.id).collect::<BTreeSet<_>>();
        for expected in [
            "context.get",
            "project.new",
            "project.open",
            "project.save",
            "project.save_as",
            "document.import",
            "geometry.create",
            "mesh.generate",
            "document.close",
            "document.delete",
            "history.undo",
            "history.redo",
            "view.fit_all",
            "view.fit_selection",
            "view.standard",
            "view.display",
            "view.clip",
            "app.quit",
        ] {
            assert!(ids.contains(expected), "missing {expected}");
        }
        assert!(
            !specs
                .iter()
                .find(|spec| spec.id == "app.quit")
                .unwrap()
                .agent_callable
        );
        assert!(
            specs
                .iter()
                .filter(|spec| spec.effect == EffectClass::View)
                .all(|spec| spec.confirmation == ConfirmationClass::None)
        );
        for id in ["mesh.generate", "document.delete"] {
            let required = specs
                .iter()
                .find(|spec| spec.id == id)
                .unwrap()
                .parameter_schema["required"]
                .as_array()
                .unwrap();
            assert!(
                required.contains(&json!("targets")),
                "{id} targets must be required"
            );
        }
        let mesh_required = specs
            .iter()
            .find(|spec| spec.id == "mesh.generate")
            .unwrap()
            .parameter_schema["required"]
            .as_array()
            .unwrap();
        assert!(!mesh_required.contains(&json!("size")));
    }

    #[test]
    fn duplicate_registry_id_is_rejected() {
        let mut specs = operation_specs();
        specs.push(specs[0].clone());
        assert!(validate_registry(&specs).unwrap_err().contains("Duplicate"));
    }

    #[test]
    fn revision_changes_only_after_document_changes() {
        let mut session = Session::new();
        let mut view = viewport();
        assert_eq!(session.revision(), 0);
        dispatch(
            &AppOperationRequest::new("context.get", json!({})),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap();
        assert_eq!(session.revision(), 0);
        dispatch(
            &AppOperationRequest::new("geometry.create", json!({"kind": "box"})),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap();
        assert_eq!(session.revision(), 1);
        dispatch(
            &AppOperationRequest::new("view.standard", json!({"direction": "+x"})),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap();
        assert_eq!(session.revision(), 1);
        dispatch(
            &AppOperationRequest::new("history.undo", json!({})),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap();
        assert_eq!(session.revision(), 2);
        dispatch(
            &AppOperationRequest::new("history.redo", json!({})),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap();
        assert_eq!(session.revision(), 3);
        dispatch(
            &AppOperationRequest::new("project.new", json!({})),
            &mut session,
            &mut view,
            HostApproval::Approved,
        )
        .unwrap();
        assert_eq!(session.revision(), 4);
    }

    #[test]
    fn stale_or_missing_target_causes_no_partial_mutation() {
        let mut session = Session::new();
        let mut view = viewport();
        dispatch(
            &AppOperationRequest::new("geometry.create", json!({"kind": "box"})),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap();
        let before = session.document.models.len();
        let error = dispatch(
            &AppOperationRequest::new(
                "document.delete",
                json!({"revision": 0, "targets": [{"kind": "model", "model": 0}]}),
            ),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap_err();
        assert!(matches!(error, OperationError::StaleRevision { .. }));
        assert_eq!(session.document.models.len(), before);
        let error = dispatch(
            &AppOperationRequest::new(
                "document.delete",
                json!({"revision": 1, "targets": [{"kind": "model", "model": 9}]}),
            ),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap_err();
        assert!(matches!(error, OperationError::MissingTarget(_)));
        assert_eq!(session.document.models.len(), before);
    }

    #[test]
    fn invalid_missing_out_of_range_and_unsupported_arguments_do_not_mutate() {
        let mut session = Session::new();
        let mut view = viewport();
        let initial_display = view.display;
        let invalid = [
            AppOperationRequest::new("geometry.create", json!({})),
            AppOperationRequest::new(
                "geometry.create",
                json!({"kind": "box", "unexpected": true}),
            ),
            AppOperationRequest::new(
                "mesh.generate",
                json!({"revision": 0, "kind": "surface", "size": 0.0}),
            ),
            AppOperationRequest::new("view.standard", json!({"direction": "diagonal"})),
            AppOperationRequest::new("view.display", json!({"shadows": true})),
            AppOperationRequest::new("view.clip", json!({"position": 1.5})),
            AppOperationRequest::new("project.open", json!({"path": "relative.oxiprep"})),
        ];
        for request in invalid {
            assert!(matches!(
                dispatch(&request, &mut session, &mut view, HostApproval::NotRequired),
                Err(OperationError::InvalidArguments(_))
            ));
            assert_eq!(session.revision(), 0);
            assert!(session.document.models.is_empty());
            assert_eq!(view.display, initial_display);
        }
        assert!(matches!(
            dispatch(
                &AppOperationRequest::new("not.supported", json!({})),
                &mut session,
                &mut view,
                HostApproval::NotRequired,
            ),
            Err(OperationError::UnknownOperation(_))
        ));
    }

    #[test]
    fn structured_create_delete_undo_and_view_results() {
        let mut session = Session::new();
        let mut view = viewport();
        let created = dispatch(
            &AppOperationRequest::new("geometry.create", json!({"kind": "sphere", "radius": 2.0})),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap();
        assert_eq!(created.revision, 1);
        assert_eq!(created.message.as_deref(), Some("Created Sphere."));
        assert_eq!(created.entities, [EntityRef::Model { model: 0 }]);
        let viewed = dispatch(
            &AppOperationRequest::new("view.display", json!({"edges": false})),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap();
        assert_eq!(viewed.revision, 1);
        assert!(!view.display.edges);
        let deleted = dispatch(
            &AppOperationRequest::new(
                "document.delete",
                json!({"revision": 1, "targets": [{"kind": "model", "model": 0}]}),
            ),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap();
        assert_eq!(deleted.revision, 2);
        let undone = dispatch(
            &AppOperationRequest::new("history.undo", json!({})),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap();
        assert_eq!(undone.revision, 3);
        assert_eq!(session.document.models.len(), 1);
    }

    #[test]
    fn live_context_describes_meshability_and_mesh_uses_default_size() {
        let mut session = Session::new();
        let mut view = viewport();
        dispatch(
            &AppOperationRequest::new("geometry.create", json!({"kind": "box"})),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap();
        let context = dispatch(
            &AppOperationRequest::new("context.get", json!({})),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap()
        .data;
        assert_eq!(context["models"][0]["bodies"][0]["shape"], "solid");
        assert_eq!(context["models"][0]["bodies"][0]["meshable"], true);
        let suggested = context["suggested_mesh_size"].as_f64().unwrap();
        assert!(suggested > 0.0);

        let meshed = dispatch(
            &AppOperationRequest::new(
                "mesh.generate",
                json!({
                    "revision": 1,
                    "targets": [{"kind": "body", "model": 0, "body": 0}],
                    "kind": "surface"
                }),
            ),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap();
        assert_eq!(meshed.data["kind"], "surface");
        assert_eq!(meshed.data["size"].as_f64().unwrap(), suggested);
        assert!(session.document.models[0].bodies[0].mesh.is_some());
    }

    #[test]
    fn confirmation_is_dynamic_and_rejection_does_not_execute() {
        let mut session = Session::new();
        let mut view = viewport();
        dispatch(
            &AppOperationRequest::new("project.new", json!({})),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap();
        dispatch(
            &AppOperationRequest::new("geometry.create", json!({"kind": "point"})),
            &mut session,
            &mut view,
            HostApproval::NotRequired,
        )
        .unwrap();
        let request = AppOperationRequest::new("project.new", json!({}));
        assert!(matches!(
            dispatch(&request, &mut session, &mut view, HostApproval::NotRequired),
            Err(OperationError::ConfirmationRequired { .. })
        ));
        assert!(matches!(
            dispatch(&request, &mut session, &mut view, HostApproval::Rejected),
            Err(OperationError::Rejected)
        ));
        assert_eq!(session.document.models.len(), 1);
    }
}

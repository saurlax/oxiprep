use std::path::{Path, PathBuf};

use crate::document::{AnalysisMesh, Body, DisplayMesh, Document, Model, Selection, file_stem};
use crate::import::ImportError;

pub trait Command {
    fn label(&self) -> &str;
    fn message(&self) -> &str;
    fn execute(&mut self, document: &mut Document) -> Result<(), CommandError>;
    fn undo(&mut self, document: &mut Document) -> Result<(), CommandError>;
}

#[derive(Debug)]
pub enum CommandError {
    Import(ImportError),
    NoModel,
    NothingToDelete,
    Failed(String),
}

impl CommandError {
    pub fn message(&self) -> &str {
        match self {
            Self::Import(err) => err.message(),
            Self::NoModel => "Nothing to close.",
            Self::NothingToDelete => "Nothing to delete.",
            Self::Failed(text) => text,
        }
    }
}

impl From<ImportError> for CommandError {
    fn from(value: ImportError) -> Self {
        Self::Import(value)
    }
}

#[derive(Default)]
pub struct History {
    undo: Vec<Box<dyn Command>>,
    redo: Vec<Box<dyn Command>>,
}

impl History {
    pub fn push(&mut self, cmd: Box<dyn Command>) {
        self.undo.push(cmd);
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|c| c.label())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|c| c.label())
    }

    pub fn undo(&mut self, document: &mut Document) -> Result<Option<String>, CommandError> {
        let Some(mut cmd) = self.undo.pop() else {
            return Ok(None);
        };
        if let Err(err) = cmd.undo(document) {
            self.undo.push(cmd);
            return Err(err);
        }
        let message = format!("Undo {}.", cmd.label());
        self.redo.push(cmd);
        Ok(Some(message))
    }

    pub fn redo(&mut self, document: &mut Document) -> Result<Option<String>, CommandError> {
        let Some(mut cmd) = self.redo.pop() else {
            return Ok(None);
        };
        if let Err(err) = cmd.execute(document) {
            self.redo.push(cmd);
            return Err(err);
        }
        let message = format!("Redo {}.", cmd.label());
        self.undo.push(cmd);
        Ok(Some(message))
    }
}

pub struct Import {
    path: PathBuf,
    label: String,
    message: String,
    index: usize,
    held: Option<Model>,
    prev_selection: Vec<Selection>,
}

impl Import {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let name = file_stem(&path);
        Self {
            label: format!("Open {name}"),
            path,
            message: String::new(),
            index: 0,
            held: None,
            prev_selection: Vec::new(),
        }
    }
}

impl Command for Import {
    fn label(&self) -> &str {
        &self.label
    }

    fn message(&self) -> &str {
        &self.message
    }

    fn execute(&mut self, document: &mut Document) -> Result<(), CommandError> {
        let model = if let Some(model) = self.held.take() {
            model
        } else {
            self.prev_selection = document.selection.clone();
            self.index = document.models.len();
            Document::load_model(&self.path)?
        };
        document.insert_model(self.index, model);
        document.selection = vec![Selection::Model(self.index)];
        self.message = opened_message(&document.models[self.index]);
        Ok(())
    }

    fn undo(&mut self, document: &mut Document) -> Result<(), CommandError> {
        self.held = Some(
            document
                .take_model(self.index)
                .ok_or(CommandError::NoModel)?,
        );
        document.selection = self.prev_selection.clone();
        Ok(())
    }
}

pub struct Close {
    index: usize,
    label: String,
    message: String,
    held: Option<Model>,
    prev_selection: Vec<Selection>,
}

impl Close {
    pub fn new(document: &Document, index: usize) -> Option<Self> {
        let name = document.models.get(index)?.name.clone();
        Some(Self {
            index,
            label: format!("Close {name}"),
            message: format!("Closed {name}."),
            held: None,
            prev_selection: Vec::new(),
        })
    }
}

impl Command for Close {
    fn label(&self) -> &str {
        &self.label
    }

    fn message(&self) -> &str {
        &self.message
    }

    fn execute(&mut self, document: &mut Document) -> Result<(), CommandError> {
        self.prev_selection = document.selection.clone();
        self.held = Some(
            document
                .take_model(self.index)
                .ok_or(CommandError::NoModel)?,
        );
        Ok(())
    }

    fn undo(&mut self, document: &mut Document) -> Result<(), CommandError> {
        let model = self.held.take().ok_or(CommandError::NoModel)?;
        document.insert_model(self.index, model);
        document.selection = self.prev_selection.clone();
        Ok(())
    }
}

fn opened_message(model: &Model) -> String {
    let n = model.bodies.len();
    format!(
        "Opened {} ({}, {n} {}).",
        model.name,
        model.kind.label(),
        if n == 1 { "body" } else { "bodies" }
    )
}

pub struct Create {
    label: String,
    message: String,
    index: Option<usize>,
    held: Option<Model>,
    prev_selection: Vec<Selection>,
}

impl Create {
    pub fn new(model: Model) -> Self {
        let name = model.name.clone();
        Self {
            label: format!("Create {name}"),
            message: format!("Created {name}."),
            index: None,
            held: Some(model),
            prev_selection: Vec::new(),
        }
    }
}

impl Command for Create {
    fn label(&self) -> &str {
        &self.label
    }

    fn message(&self) -> &str {
        &self.message
    }

    fn execute(&mut self, document: &mut Document) -> Result<(), CommandError> {
        let model = self.held.take().ok_or(CommandError::NoModel)?;
        self.prev_selection = document.selection.clone();
        let index = *self.index.get_or_insert(document.models.len());
        document.insert_model(index, model);
        document.selection = vec![Selection::Model(index)];
        Ok(())
    }

    fn undo(&mut self, document: &mut Document) -> Result<(), CommandError> {
        let index = self.index.ok_or(CommandError::NoModel)?;
        self.held = Some(document.take_model(index).ok_or(CommandError::NoModel)?);
        document.selection = self.prev_selection.clone();
        Ok(())
    }
}

pub struct AddBody {
    label: String,
    message: String,
    model: usize,
    body_index: Option<usize>,
    held: Option<Body>,
    prev_selection: Vec<Selection>,
}

impl AddBody {
    pub fn new(model: usize, body: Body) -> Self {
        let name = body.name.clone();
        Self {
            label: format!("Create {name}"),
            message: format!("Created {name}."),
            model,
            body_index: None,
            held: Some(body),
            prev_selection: Vec::new(),
        }
    }
}

impl Command for AddBody {
    fn label(&self) -> &str {
        &self.label
    }

    fn message(&self) -> &str {
        &self.message
    }

    fn execute(&mut self, document: &mut Document) -> Result<(), CommandError> {
        let body = self.held.take().ok_or(CommandError::NoModel)?;
        self.prev_selection = document.selection.clone();
        let model = document
            .models
            .get_mut(self.model)
            .ok_or(CommandError::NoModel)?;
        let index = *self.body_index.get_or_insert(model.bodies.len());
        let index = index.min(model.bodies.len());
        model.bodies.insert(index, body);
        self.body_index = Some(index);
        document.selection = vec![Selection::Body {
            model: self.model,
            body: index,
        }];
        Ok(())
    }

    fn undo(&mut self, document: &mut Document) -> Result<(), CommandError> {
        let index = self.body_index.ok_or(CommandError::NoModel)?;
        let model = document
            .models
            .get_mut(self.model)
            .ok_or(CommandError::NoModel)?;
        if index >= model.bodies.len() {
            return Err(CommandError::NoModel);
        }
        self.held = Some(model.bodies.remove(index));
        document.selection = self.prev_selection.clone();
        Ok(())
    }
}

enum Removal {
    Model {
        index: usize,
        held: Option<Model>,
    },
    Body {
        model: usize,
        index: usize,
        held: Option<Body>,
    },
}

pub struct Delete {
    label: String,
    message: String,
    plan: Vec<Removal>,
    prev_selection: Vec<Selection>,
}

impl Delete {
    pub fn new(document: &Document) -> Option<Self> {
        let plan = plan_deletions(document);
        if plan.is_empty() {
            return None;
        }
        let (label, message) = delete_copy(document, &plan);
        Some(Self {
            label,
            message,
            plan,
            prev_selection: Vec::new(),
        })
    }

    pub fn can_run(document: &Document) -> bool {
        !plan_deletions(document).is_empty()
    }
}

impl Command for Delete {
    fn label(&self) -> &str {
        &self.label
    }

    fn message(&self) -> &str {
        &self.message
    }

    fn execute(&mut self, document: &mut Document) -> Result<(), CommandError> {
        self.prev_selection = document.selection.clone();
        for item in &mut self.plan {
            match item {
                Removal::Body { model, index, held } => {
                    *held = Some(
                        document
                            .take_body(*model, *index)
                            .ok_or(CommandError::NothingToDelete)?,
                    );
                }
                Removal::Model { index, held } => {
                    *held = Some(
                        document
                            .take_model(*index)
                            .ok_or(CommandError::NothingToDelete)?,
                    );
                }
            }
        }
        Ok(())
    }

    fn undo(&mut self, document: &mut Document) -> Result<(), CommandError> {
        for item in self.plan.iter_mut().rev() {
            match item {
                Removal::Model { index, held } => {
                    let model = held.take().ok_or(CommandError::NothingToDelete)?;
                    document.insert_model(*index, model);
                }
                Removal::Body { model, index, held } => {
                    let body = held.take().ok_or(CommandError::NothingToDelete)?;
                    if !document.insert_body(*model, *index, body) {
                        return Err(CommandError::NothingToDelete);
                    }
                }
            }
        }
        document.selection = self.prev_selection.clone();
        Ok(())
    }
}

fn plan_deletions(document: &Document) -> Vec<Removal> {
    use std::collections::BTreeSet;

    let mut models = BTreeSet::new();
    let mut bodies = BTreeSet::new();
    for s in &document.selection {
        let m = s.model();
        if document.models.get(m).is_none() {
            continue;
        }
        match *s {
            Selection::Model(_) => {
                models.insert(m);
            }
            _ => {
                if let Some(b) = s.body() {
                    if document.models[m].bodies.get(b).is_some() {
                        bodies.insert((m, b));
                    }
                }
            }
        }
    }
    for m in 0..document.models.len() {
        if models.contains(&m) {
            continue;
        }
        let n = document.models[m].bodies.len();
        if n > 0 && (0..n).all(|b| bodies.contains(&(m, b))) {
            models.insert(m);
        }
    }
    bodies.retain(|(m, _)| !models.contains(m));

    let mut plan = Vec::new();
    for &(model, index) in bodies.iter().rev() {
        plan.push(Removal::Body {
            model,
            index,
            held: None,
        });
    }
    for &index in models.iter().rev() {
        plan.push(Removal::Model { index, held: None });
    }
    plan
}

fn delete_copy(document: &Document, plan: &[Removal]) -> (String, String) {
    if let [item] = plan {
        let name = match *item {
            Removal::Model { index, .. } => document.models.get(index).map(|m| m.name.clone()),
            Removal::Body { model, index, .. } => document
                .models
                .get(model)
                .and_then(|m| m.bodies.get(index))
                .map(|b| b.name.clone()),
        };
        if let Some(name) = name {
            return (format!("Delete {name}"), format!("Deleted {name}."));
        }
    }
    let n = plan.len();
    (
        "Delete".to_string(),
        format!("Deleted {n} {}.", if n == 1 { "item" } else { "items" }),
    )
}

struct MeshBackup {
    model: usize,
    body: usize,
    display: DisplayMesh,
    mesh: Option<AnalysisMesh>,
}

pub struct MeshBodies {
    label: String,
    message: String,
    kind: crate::mesh::MeshKind,
    size: f64,
    targets: Vec<(usize, usize)>,
    prev: Vec<MeshBackup>,
    next: Vec<AnalysisMesh>,
}

impl MeshBodies {
    pub fn new(
        document: &Document,
        kind: crate::mesh::MeshKind,
        size: f64,
    ) -> Result<Self, CommandError> {
        let targets = crate::mesh::mesh_targets(document);
        if targets.is_empty() {
            return Err(CommandError::Failed("Select a solid.".to_string()));
        }
        let noun = if kind == crate::mesh::MeshKind::Volume {
            "volume"
        } else {
            "surface"
        };
        let n = targets.len();
        let what = if n == 1 { "body" } else { "bodies" };
        Ok(Self {
            label: format!("Mesh {noun}"),
            message: format!("Meshed {n} {what}."),
            kind,
            size,
            targets,
            prev: Vec::new(),
            next: Vec::new(),
        })
    }
}

impl Command for MeshBodies {
    fn label(&self) -> &str {
        &self.label
    }

    fn message(&self) -> &str {
        &self.message
    }

    fn execute(&mut self, document: &mut Document) -> Result<(), CommandError> {
        if self.next.is_empty() {
            let mut generated = Vec::with_capacity(self.targets.len());
            for &(mi, bi) in &self.targets {
                let solid = match document
                    .models
                    .get(mi)
                    .and_then(|m| m.bodies.get(bi))
                    .map(|b| &b.shape)
                {
                    Some(crate::document::BodyShape::Solid(solid)) => solid,
                    _ => return Err(CommandError::Failed("Select a solid.".to_string())),
                };
                generated.push(crate::mesh::generate(solid, self.kind, self.size)?);
            }
            for &(mi, bi) in &self.targets {
                let body = &document.models[mi].bodies[bi];
                self.prev.push(MeshBackup {
                    model: mi,
                    body: bi,
                    display: body.display.clone(),
                    mesh: body.mesh.clone(),
                });
            }
            self.next = generated;
        }
        for ((mi, bi), mesh) in self.targets.iter().zip(self.next.iter()) {
            document.models[*mi].bodies[*bi].set_analysis_mesh(mesh.clone());
        }
        Ok(())
    }

    fn undo(&mut self, document: &mut Document) -> Result<(), CommandError> {
        for prev in &self.prev {
            let body = document
                .models
                .get_mut(prev.model)
                .and_then(|m| m.bodies.get_mut(prev.body))
                .ok_or_else(|| CommandError::Failed("Could not mesh the solid.".to_string()))?;
            body.display = prev.display.clone();
            body.mesh = prev.mesh.clone();
        }
        Ok(())
    }
}

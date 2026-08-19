use std::path::{Path, PathBuf};

use crate::document::{Body, Document, Model, Selection, file_stem};
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
    Failed(String),
}

impl CommandError {
    pub fn message(&self) -> &str {
        match self {
            Self::Import(err) => err.message(),
            Self::NoModel => "Nothing to close.",
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

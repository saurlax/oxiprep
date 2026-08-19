use std::path::Path;

use crate::command::{AddBody, Close, Command, CommandError, Create, Delete, History, Import};
use crate::document::Document;

pub struct Session {
    pub document: Document,
    history: History,
}

impl Session {
    pub fn new() -> Self {
        Self {
            document: Document::new(),
            history: History::default(),
        }
    }

    pub fn run(&mut self, mut cmd: Box<dyn Command>) -> Result<String, CommandError> {
        cmd.execute(&mut self.document)?;
        self.document.dirty = true;
        let message = cmd.message().to_string();
        self.history.push(cmd);
        Ok(message)
    }

    pub fn import_path(&mut self, path: &Path) -> Result<String, CommandError> {
        self.run(Box::new(Import::new(path)))
    }

    pub fn create_model(&mut self, model: crate::document::Model) -> Result<String, CommandError> {
        self.run(Box::new(Create::new(model)))
    }

    pub fn add_body(
        &mut self,
        model: usize,
        body: crate::document::Body,
    ) -> Result<String, CommandError> {
        self.run(Box::new(AddBody::new(model, body)))
    }

    pub fn delete_selected(&mut self) -> Result<String, CommandError> {
        let cmd = Delete::new(&self.document).ok_or(CommandError::NothingToDelete)?;
        self.run(Box::new(cmd))
    }

    pub fn can_delete(&self) -> bool {
        Delete::can_run(&self.document)
    }

    pub fn close_model(&mut self, index: usize) -> Result<String, CommandError> {
        let cmd = Close::new(&self.document, index).ok_or(CommandError::NoModel)?;
        self.run(Box::new(cmd))
    }

    pub fn close_selected(&mut self) -> Result<String, CommandError> {
        let index = self
            .document
            .selection
            .first()
            .map(|s| s.model())
            .ok_or(CommandError::NoModel)?;
        self.close_model(index)
    }

    pub fn undo(&mut self) -> Result<Option<String>, CommandError> {
        let message = self.history.undo(&mut self.document)?;
        if message.is_some() {
            self.document.dirty = true;
        }
        Ok(message)
    }

    pub fn redo(&mut self) -> Result<Option<String>, CommandError> {
        let message = self.history.redo(&mut self.document)?;
        if message.is_some() {
            self.document.dirty = true;
        }
        Ok(message)
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.history.undo_label()
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.history.redo_label()
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Selection;
    use cadrum::{DVec3, Solid};
    use std::io::Write;
    use std::path::PathBuf;

    fn write_cube(name: &str) -> PathBuf {
        let solid = Solid::cube(DVec3::ZERO, DVec3::ONE);
        let path = std::env::temp_dir().join(name);
        let mut file = std::fs::File::create(&path).unwrap();
        Solid::write_step(std::iter::once(&solid), &mut file).unwrap();
        file.flush().unwrap();
        path
    }

    #[test]
    fn import_undo_redo() {
        let path = write_cube("oxiprep_session_import.step");
        let mut session = Session::new();
        let message = session.import_path(&path).unwrap();
        assert!(message.starts_with("Opened "));
        assert_eq!(session.document.models.len(), 1);
        assert_eq!(session.document.selection, vec![Selection::Model(0)]);
        assert!(session.document.dirty);
        assert_eq!(session.undo_label(), Some("Open oxiprep_session_import"));

        let undo = session.undo().unwrap().unwrap();
        assert_eq!(undo, "Undo Open oxiprep_session_import.");
        assert!(session.document.models.is_empty());
        assert!(session.document.selection.is_empty());

        let redo = session.redo().unwrap().unwrap();
        assert_eq!(redo, "Redo Open oxiprep_session_import.");
        assert_eq!(session.document.models.len(), 1);
        assert_eq!(session.document.selection, vec![Selection::Model(0)]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn close_undo_restores_selection() {
        let path = write_cube("oxiprep_session_close.step");
        let mut session = Session::new();
        session.import_path(&path).unwrap();
        session.document.selection = vec![Selection::Body { model: 0, body: 0 }];
        let message = session.close_selected().unwrap();
        assert_eq!(message, "Closed oxiprep_session_close.");
        assert!(session.document.models.is_empty());
        assert!(session.document.selection.is_empty());

        session.undo().unwrap();
        assert_eq!(session.document.models.len(), 1);
        assert_eq!(
            session.document.selection,
            vec![Selection::Body { model: 0, body: 0 }]
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn second_import_undo_keeps_first() {
        let a = write_cube("oxiprep_session_a.step");
        let b = write_cube("oxiprep_session_b.step");
        let mut session = Session::new();
        session.import_path(&a).unwrap();
        session.import_path(&b).unwrap();
        assert_eq!(session.document.models.len(), 2);
        session.undo().unwrap();
        assert_eq!(session.document.models.len(), 1);
        assert_eq!(session.document.models[0].name, "oxiprep_session_a");
        assert_eq!(session.document.selection, vec![Selection::Model(0)]);
        let _ = std::fs::remove_file(a);
        let _ = std::fs::remove_file(b);
    }

    #[test]
    fn new_command_clears_redo() {
        let path = write_cube("oxiprep_session_clear_redo.step");
        let mut session = Session::new();
        session.import_path(&path).unwrap();
        session.undo().unwrap();
        assert!(session.can_redo());
        session.import_path(&path).unwrap();
        assert!(!session.can_redo());
        assert!(session.can_undo());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn create_box_undo_redo() {
        let mut session = Session::new();
        let model = crate::geometry::CreateKind::r#box()
            .into_model(&session.document)
            .unwrap();
        session.create_model(model).unwrap();
        assert_eq!(session.document.models.len(), 1);
        assert_eq!(session.document.models[0].name, "Box");
        assert!(
            !session.document.models[0].bodies[0]
                .display
                .triangles
                .is_empty()
        );
        session.undo().unwrap();
        assert!(session.document.models.is_empty());
        session.redo().unwrap();
        assert_eq!(session.document.models[0].name, "Box");
    }

    #[test]
    fn add_body_to_current_model() {
        let mut session = Session::new();
        let model = crate::geometry::CreateKind::r#box()
            .into_model(&session.document)
            .unwrap();
        session.create_model(model).unwrap();
        let body = crate::geometry::CreateKind::sphere()
            .into_body(&session.document, 0)
            .unwrap();
        session.add_body(0, body).unwrap();
        assert_eq!(session.document.models.len(), 1);
        assert_eq!(session.document.models[0].bodies.len(), 2);
        assert_eq!(session.document.models[0].bodies[1].name, "Sphere");
        session.undo().unwrap();
        assert_eq!(session.document.models[0].bodies.len(), 1);
    }

    #[test]
    fn delete_model_undo_redo() {
        let mut session = Session::new();
        let model = crate::geometry::CreateKind::r#box()
            .into_model(&session.document)
            .unwrap();
        session.create_model(model).unwrap();
        let message = session.delete_selected().unwrap();
        assert_eq!(message, "Deleted Box.");
        assert!(session.document.models.is_empty());
        session.undo().unwrap();
        assert_eq!(session.document.models[0].name, "Box");
        session.redo().unwrap();
        assert!(session.document.models.is_empty());
    }

    #[test]
    fn delete_one_body_keeps_the_other() {
        let mut session = Session::new();
        let model = crate::geometry::CreateKind::r#box()
            .into_model(&session.document)
            .unwrap();
        session.create_model(model).unwrap();
        let body = crate::geometry::CreateKind::sphere()
            .into_body(&session.document, 0)
            .unwrap();
        session.add_body(0, body).unwrap();
        session.document.selection = vec![Selection::Body { model: 0, body: 1 }];
        session.delete_selected().unwrap();
        assert_eq!(session.document.models.len(), 1);
        assert_eq!(session.document.models[0].bodies.len(), 1);
        assert_eq!(session.document.models[0].bodies[0].name, "Box");
        session.undo().unwrap();
        assert_eq!(session.document.models[0].bodies.len(), 2);
        assert_eq!(session.document.models[0].bodies[1].name, "Sphere");
    }

    #[test]
    fn delete_last_body_removes_model() {
        let mut session = Session::new();
        let model = crate::geometry::CreateKind::r#box()
            .into_model(&session.document)
            .unwrap();
        session.create_model(model).unwrap();
        session.document.selection = vec![Selection::Body { model: 0, body: 0 }];
        session.delete_selected().unwrap();
        assert!(session.document.models.is_empty());
        session.undo().unwrap();
        assert_eq!(session.document.models[0].bodies[0].name, "Box");
        assert_eq!(
            session.document.selection,
            vec![Selection::Body { model: 0, body: 0 }]
        );
    }

    #[test]
    fn delete_face_removes_owning_body() {
        let mut session = Session::new();
        let model = crate::geometry::CreateKind::r#box()
            .into_model(&session.document)
            .unwrap();
        session.create_model(model).unwrap();
        let body = crate::geometry::CreateKind::sphere()
            .into_body(&session.document, 0)
            .unwrap();
        session.add_body(0, body).unwrap();
        session.document.selection = vec![Selection::Face {
            model: 0,
            body: 1,
            id: 1,
        }];
        session.delete_selected().unwrap();
        assert_eq!(session.document.models.len(), 1);
        assert_eq!(session.document.models[0].bodies.len(), 1);
        assert_eq!(session.document.models[0].bodies[0].name, "Box");
        session.undo().unwrap();
        assert_eq!(session.document.models[0].bodies.len(), 2);
        assert_eq!(
            session.document.selection,
            vec![Selection::Face {
                model: 0,
                body: 1,
                id: 1,
            }]
        );
    }

    #[test]
    fn delete_does_nothing_without_selection() {
        let mut session = Session::new();
        assert!(!session.can_delete());
        assert!(session.delete_selected().is_err());
        let model = crate::geometry::CreateKind::r#box()
            .into_model(&session.document)
            .unwrap();
        session.create_model(model).unwrap();
        session.document.selection.clear();
        assert!(!session.can_delete());
    }
}

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use cadrum::{DVec3, Edge, Solid, Tessellation};
use serde::{Deserialize, Serialize};
use zip::CompressionMethod;
use zip::ZipArchive;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::document::{
    Body, BodyShape, Document, Model, body_from_edges, body_from_mesh, body_from_point,
    body_from_solid,
};
use crate::import::ModelKind;

const FORMAT: u32 = 1;
const MESH_MAGIC: &[u8; 4] = b"OXMS";

#[derive(Debug)]
pub enum ProjectError {
    Read,
    Write,
    Invalid,
    Unsupported,
    Geometry,
}

impl ProjectError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Read => "Could not read the project.",
            Self::Write => "Could not write the project.",
            Self::Invalid => "This file is not a valid project.",
            Self::Unsupported => "This project cannot be opened.",
            Self::Geometry => "Could not restore the geometry.",
        }
    }
}

pub fn is_project_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("oxiprep"))
}

pub fn save(document: &Document, path: &Path) -> Result<(), ProjectError> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let tmp = match parent {
        Some(dir) => dir.join(format!(
            ".{}.tmp",
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("project.oxiprep")
        )),
        None => PathBuf::from(format!(
            ".{}.tmp",
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("project.oxiprep")
        )),
    };
    match write_archive(document, &tmp) {
        Ok(()) => {
            if path.exists() {
                std::fs::remove_file(path).map_err(|_| ProjectError::Write)?;
            }
            std::fs::rename(&tmp, path).map_err(|_| {
                let _ = std::fs::remove_file(&tmp);
                ProjectError::Write
            })
        }
        Err(err) => {
            let _ = std::fs::remove_file(&tmp);
            Err(err)
        }
    }
}

pub fn load(path: &Path) -> Result<Document, ProjectError> {
    let file = std::fs::File::open(path).map_err(|_| ProjectError::Read)?;
    let mut zip = ZipArchive::new(file).map_err(|_| ProjectError::Invalid)?;
    let manifest: Manifest = {
        let mut buf = Vec::new();
        zip.by_name("manifest.json")
            .map_err(|_| ProjectError::Invalid)?
            .read_to_end(&mut buf)
            .map_err(|_| ProjectError::Read)?;
        serde_json::from_slice(&buf).map_err(|_| ProjectError::Invalid)?
    };
    if manifest.format != FORMAT {
        return Err(ProjectError::Unsupported);
    }

    let mut models = Vec::with_capacity(manifest.models.len());
    for model in &manifest.models {
        let kind = parse_kind(&model.kind).ok_or(ProjectError::Invalid)?;
        let mut bodies = Vec::with_capacity(model.bodies.len());
        for body in &model.bodies {
            let bytes = zip_bytes(&mut zip, &body.file)?;
            bodies.push(load_body(&body.name, &body.shape, &bytes)?);
        }
        if bodies.is_empty() {
            return Err(ProjectError::Invalid);
        }
        models.push(Model {
            name: model.name.clone(),
            path: if model.source.is_empty() {
                PathBuf::new()
            } else {
                PathBuf::from(&model.source)
            },
            kind,
            bodies,
        });
    }

    Ok(Document {
        models,
        selection: Vec::new(),
        dirty: false,
        path: None,
    })
}

fn write_archive(document: &Document, path: &Path) -> Result<(), ProjectError> {
    let file = std::fs::File::create(path).map_err(|_| ProjectError::Write)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let mut manifest = Manifest {
        format: FORMAT,
        models: Vec::with_capacity(document.models.len()),
        groups: Vec::new(),
    };

    for (mi, model) in document.models.iter().enumerate() {
        let mut bodies = Vec::with_capacity(model.bodies.len());
        for (bi, body) in model.bodies.iter().enumerate() {
            let (shape, file_name, bytes) = save_body(mi, bi, body)?;
            zip.start_file(&file_name, options)
                .map_err(|_| ProjectError::Write)?;
            zip.write_all(&bytes).map_err(|_| ProjectError::Write)?;
            bodies.push(ManifestBody {
                name: body.name.clone(),
                shape: shape.to_string(),
                file: file_name,
            });
        }
        manifest.models.push(ManifestModel {
            name: model.name.clone(),
            kind: kind_name(model.kind).to_string(),
            source: model.path.to_string_lossy().into_owned(),
            bodies,
        });
    }

    let json = serde_json::to_vec_pretty(&manifest).map_err(|_| ProjectError::Write)?;
    zip.start_file("manifest.json", options)
        .map_err(|_| ProjectError::Write)?;
    zip.write_all(&json).map_err(|_| ProjectError::Write)?;
    zip.finish().map_err(|_| ProjectError::Write)?;
    Ok(())
}

fn save_body(
    mi: usize,
    bi: usize,
    body: &Body,
) -> Result<(&'static str, String, Vec<u8>), ProjectError> {
    match &body.shape {
        BodyShape::Solid(solid) => {
            let mut bytes = Vec::new();
            Solid::write_brep(std::iter::once(solid), &mut bytes)
                .map_err(|_| ProjectError::Write)?;
            Ok(("solid", format!("geometry/{mi}/{bi}.brep"), bytes))
        }
        BodyShape::Wire(edges) => {
            let saved: Result<Vec<_>, _> = edges.iter().map(save_edge).collect();
            let json = serde_json::to_vec_pretty(&WireFile { edges: saved? })
                .map_err(|_| ProjectError::Write)?;
            Ok(("wire", format!("geometry/{mi}/{bi}.json"), json))
        }
        BodyShape::Vertex(point) => {
            let json = serde_json::to_vec_pretty(&VertexFile {
                point: [point.x, point.y, point.z],
            })
            .map_err(|_| ProjectError::Write)?;
            Ok(("vertex", format!("geometry/{mi}/{bi}.json"), json))
        }
        BodyShape::Mesh => {
            let bytes = write_mesh_bin(&body.display.positions, &body.display.triangles);
            Ok(("mesh", format!("meshes/{mi}/{bi}.bin"), bytes))
        }
    }
}

fn load_body(name: &str, shape: &str, bytes: &[u8]) -> Result<Body, ProjectError> {
    match shape {
        "solid" => {
            let solids =
                Solid::read_brep(&mut Cursor::new(bytes)).map_err(|_| ProjectError::Geometry)?;
            let solid = solids.into_iter().next().ok_or(ProjectError::Geometry)?;
            body_from_solid(name, solid).map_err(|_| ProjectError::Geometry)
        }
        "wire" => {
            let file: WireFile =
                serde_json::from_slice(bytes).map_err(|_| ProjectError::Invalid)?;
            let mut edges = Vec::with_capacity(file.edges.len());
            for edge in file.edges {
                edges.push(load_edge(edge)?);
            }
            if edges.is_empty() {
                return Err(ProjectError::Invalid);
            }
            Ok(body_from_edges(name, edges))
        }
        "vertex" => {
            let file: VertexFile =
                serde_json::from_slice(bytes).map_err(|_| ProjectError::Invalid)?;
            Ok(body_from_point(
                name,
                DVec3::new(file.point[0], file.point[1], file.point[2]),
            ))
        }
        "mesh" => {
            let (positions, triangles) = read_mesh_bin(bytes)?;
            Ok(body_from_mesh(name, positions, triangles))
        }
        _ => Err(ProjectError::Invalid),
    }
}

fn save_edge(edge: &Edge) -> Result<SavedEdge, ProjectError> {
    if edge.is_closed() {
        let pts = edge.approximation_segments(Tessellation::default());
        if pts.len() >= 3 {
            let i = pts.len() / 3;
            let j = (2 * pts.len()) / 3;
            if let Some((center, radius, axis)) = circumcircle(pts[0], pts[i], pts[j])
                && radius > 1e-12
                && axis.length_squared() > 1e-24
            {
                return Ok(SavedEdge::Circle {
                    center: [center.x, center.y, center.z],
                    radius,
                    axis: [axis.x, axis.y, axis.z],
                });
            }
        }
        return Err(ProjectError::Geometry);
    }
    let a = edge.start_point();
    let b = edge.end_point();
    Ok(SavedEdge::Line {
        a: [a.x, a.y, a.z],
        b: [b.x, b.y, b.z],
    })
}

fn load_edge(edge: SavedEdge) -> Result<Edge, ProjectError> {
    match edge {
        SavedEdge::Line { a, b } => {
            Edge::line(dvec(a), dvec(b)).map_err(|_| ProjectError::Geometry)
        }
        SavedEdge::Circle {
            center,
            radius,
            axis,
        } => Edge::circle(radius, dvec(axis))
            .map_err(|_| ProjectError::Geometry)
            .map(|e| e.translate(dvec(center))),
    }
}

fn circumcircle(a: DVec3, b: DVec3, c: DVec3) -> Option<(DVec3, f64, DVec3)> {
    let ab = b - a;
    let ac = c - a;
    let n = ab.cross(ac);
    let n2 = n.length_squared();
    if n2 < 1e-24 {
        return None;
    }
    let to_center =
        (ac.length_squared() * n.cross(ab) + ab.length_squared() * ac.cross(n)) / (2.0 * n2);
    let center = a + to_center;
    let radius = (a - center).length();
    Some((center, radius, n.normalize()))
}

fn write_mesh_bin(positions: &[[f32; 3]], triangles: &[[u32; 3]]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16 + positions.len() * 12 + triangles.len() * 12);
    bytes.extend_from_slice(MESH_MAGIC);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(positions.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
    for p in positions {
        for v in p {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    for t in triangles {
        for i in t {
            bytes.extend_from_slice(&i.to_le_bytes());
        }
    }
    bytes
}

fn read_mesh_bin(bytes: &[u8]) -> Result<(Vec<[f32; 3]>, Vec<[u32; 3]>), ProjectError> {
    if bytes.len() < 16 || &bytes[..4] != MESH_MAGIC {
        return Err(ProjectError::Invalid);
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != 1 {
        return Err(ProjectError::Unsupported);
    }
    let n_pos = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let n_tri = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let pos_bytes = n_pos.checked_mul(12).ok_or(ProjectError::Invalid)?;
    let tri_bytes = n_tri.checked_mul(12).ok_or(ProjectError::Invalid)?;
    let need = 16usize
        .checked_add(pos_bytes)
        .and_then(|n| n.checked_add(tri_bytes))
        .ok_or(ProjectError::Invalid)?;
    if bytes.len() < need {
        return Err(ProjectError::Invalid);
    }
    let mut positions = Vec::with_capacity(n_pos);
    let mut off = 16;
    for _ in 0..n_pos {
        positions.push([
            f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()),
            f32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap()),
            f32::from_le_bytes(bytes[off + 8..off + 12].try_into().unwrap()),
        ]);
        off += 12;
    }
    let mut triangles = Vec::with_capacity(n_tri);
    for _ in 0..n_tri {
        let t = [
            u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()),
            u32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap()),
            u32::from_le_bytes(bytes[off + 8..off + 12].try_into().unwrap()),
        ];
        if t.iter().any(|&i| (i as usize) >= positions.len()) {
            return Err(ProjectError::Invalid);
        }
        triangles.push(t);
        off += 12;
    }
    Ok((positions, triangles))
}

fn zip_bytes<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, ProjectError> {
    let mut buf = Vec::new();
    zip.by_name(name)
        .map_err(|_| ProjectError::Invalid)?
        .read_to_end(&mut buf)
        .map_err(|_| ProjectError::Read)?;
    Ok(buf)
}

fn dvec(p: [f64; 3]) -> DVec3 {
    DVec3::new(p[0], p[1], p[2])
}

fn kind_name(kind: ModelKind) -> &'static str {
    match kind {
        ModelKind::Step => "step",
        ModelKind::Brep => "brep",
        ModelKind::Stl => "stl",
        ModelKind::Geometry => "geometry",
    }
}

fn parse_kind(s: &str) -> Option<ModelKind> {
    match s {
        "step" => Some(ModelKind::Step),
        "brep" => Some(ModelKind::Brep),
        "stl" => Some(ModelKind::Stl),
        "geometry" => Some(ModelKind::Geometry),
        _ => None,
    }
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: u32,
    models: Vec<ManifestModel>,
    #[serde(default)]
    groups: Vec<serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
struct ManifestModel {
    name: String,
    kind: String,
    #[serde(default)]
    source: String,
    bodies: Vec<ManifestBody>,
}

#[derive(Serialize, Deserialize)]
struct ManifestBody {
    name: String,
    shape: String,
    file: String,
}

#[derive(Serialize, Deserialize)]
struct WireFile {
    edges: Vec<SavedEdge>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum SavedEdge {
    Line {
        a: [f64; 3],
        b: [f64; 3],
    },
    Circle {
        center: [f64; 3],
        radius: f64,
        axis: [f64; 3],
    },
}

#[derive(Serialize, Deserialize)]
struct VertexFile {
    point: [f64; 3],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::CreateKind;
    use crate::session::Session;
    use std::path::PathBuf;

    fn temp_path(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn box_project_roundtrip() {
        let path = temp_path("oxiprep_project_box.oxiprep");
        let mut session = Session::new();
        let model = CreateKind::r#box().into_model(&session.document).unwrap();
        session.create_model(model).unwrap();
        session.save_to(&path).unwrap();
        assert!(!session.document.dirty);
        session.new_project();
        assert!(session.document.is_empty());
        session.open_project(&path).unwrap();
        assert!(!session.can_undo());
        assert!(!session.document.dirty);
        assert_eq!(session.document.models[0].name, "Box");
        match session.document.models[0].bodies[0].stats {
            crate::document::BodyStats::Solid {
                volume, face_count, ..
            } => {
                assert!((volume - 1.0).abs() < 1e-6);
                assert_eq!(face_count, 6);
            }
            _ => panic!("expected solid"),
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn primitives_and_second_body_roundtrip() {
        let path = temp_path("oxiprep_project_mixed.oxiprep");
        let mut session = Session::new();
        session
            .create_model(CreateKind::line().into_model(&session.document).unwrap())
            .unwrap();
        session
            .create_model(CreateKind::disk().into_model(&session.document).unwrap())
            .unwrap();
        session
            .create_model(CreateKind::point().into_model(&session.document).unwrap())
            .unwrap();
        let box_model = CreateKind::r#box().into_model(&session.document).unwrap();
        session.create_model(box_model).unwrap();
        let sphere = CreateKind::sphere()
            .into_body(&session.document, 3)
            .unwrap();
        session.add_body(3, sphere).unwrap();
        session.save_to(&path).unwrap();
        session.new_project();
        session.open_project(&path).unwrap();
        assert_eq!(session.document.models.len(), 4);
        assert!(matches!(
            session.document.models[0].bodies[0].stats,
            crate::document::BodyStats::Wire { edge_count: 1 }
        ));
        assert!(matches!(
            session.document.models[1].bodies[0].stats,
            crate::document::BodyStats::Wire { edge_count: 1 }
        ));
        assert!(matches!(
            session.document.models[2].bodies[0].stats,
            crate::document::BodyStats::Vertex
        ));
        assert_eq!(session.document.models[3].bodies.len(), 2);
        assert_eq!(session.document.models[3].bodies[1].name, "Sphere");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn mesh_body_roundtrip() {
        let path = temp_path("oxiprep_project_mesh.oxiprep");
        let mut session = Session::new();
        session.document.models.push(Model {
            name: "Mesh".to_string(),
            path: PathBuf::new(),
            kind: ModelKind::Stl,
            bodies: vec![body_from_mesh(
                "Mesh",
                vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                vec![[0, 1, 2]],
            )],
        });
        session.save_to(&path).unwrap();
        session.new_project();
        session.open_project(&path).unwrap();
        let body = &session.document.models[0].bodies[0];
        assert_eq!(body.display.triangles, vec![[0, 1, 2]]);
        assert_eq!(body.display.positions.len(), 3);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn save_does_not_need_original_step() {
        let path = temp_path("oxiprep_project_embedded.oxiprep");
        let mut session = Session::new();
        let model = CreateKind::cylinder()
            .into_model(&session.document)
            .unwrap();
        session.create_model(model).unwrap();
        session.save_to(&path).unwrap();
        let mut opened = Session::new();
        opened.open_project(&path).unwrap();
        match opened.document.models[0].bodies[0].stats {
            crate::document::BodyStats::Solid { volume, .. } => {
                assert!(volume > 0.0);
            }
            _ => panic!("expected solid"),
        }
        let _ = std::fs::remove_file(path);
    }
}

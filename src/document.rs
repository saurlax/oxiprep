use crate::import::{self, ImportError, ModelKind};
use cadrum::{DVec3, Mesh, Solid, Tessellation};
use std::path::{Path, PathBuf};

const DEFAULT_RGB: [u8; 3] = [0x8C, 0xAD, 0xC4];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Selection {
    Model(usize),
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

impl Selection {
    pub fn model(self) -> usize {
        match self {
            Self::Model(m)
            | Self::Body { model: m, .. }
            | Self::Face { model: m, .. }
            | Self::Edge { model: m, .. }
            | Self::Vertex { model: m, .. }
            | Self::Node { model: m, .. }
            | Self::Cell { model: m, .. }
            | Self::MeshEdge { model: m, .. } => m,
        }
    }

    fn remap_after_remove(self, removed: usize) -> Option<Self> {
        let m = self.model();
        if m == removed {
            return None;
        }
        let m = if m > removed { m - 1 } else { m };
        Some(match self {
            Self::Model(_) => Self::Model(m),
            Self::Body { body, .. } => Self::Body { model: m, body },
            Self::Face { body, id, .. } => Self::Face { model: m, body, id },
            Self::Edge { body, id, .. } => Self::Edge { model: m, body, id },
            Self::Vertex { body, index, .. } => Self::Vertex {
                model: m,
                body,
                index,
            },
            Self::Node { body, index, .. } => Self::Node {
                model: m,
                body,
                index,
            },
            Self::Cell { body, index, .. } => Self::Cell {
                model: m,
                body,
                index,
            },
            Self::MeshEdge { body, a, b, .. } => Self::MeshEdge {
                model: m,
                body,
                a,
                b,
            },
        })
    }
}

pub struct Document {
    pub models: Vec<Model>,
    pub selection: Vec<Selection>,
}

pub struct Model {
    pub name: String,
    pub path: PathBuf,
    pub kind: ModelKind,
    pub bodies: Vec<Body>,
}

pub struct Body {
    pub name: String,
    pub display: DisplayMesh,
    pub stats: BodyStats,
    #[allow(dead_code)]
    pub solid: Option<Solid>,
}

pub enum BodyStats {
    Solid {
        volume: f64,
        area: f64,
        center: DVec3,
        face_count: usize,
        edge_count: usize,
    },
    Mesh {
        triangle_count: usize,
    },
}

pub struct DisplayMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub triangle_colors: Vec<[u8; 3]>,
    /// Per-triangle CAD face id. Zero when the body is mesh-only.
    pub triangle_face_ids: Vec<u64>,
    /// NaN-separated polylines in the same convention as cadrum.
    pub edges: Vec<[f32; 3]>,
    pub cad_edges: Vec<CadEdge>,
    pub cad_vertices: Vec<[f32; 3]>,
    pub bbox: [DVec3; 2],
}

#[derive(Clone, Debug)]
pub struct CadEdge {
    pub id: u64,
    pub points: Vec<[f32; 3]>,
}

impl Document {
    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            selection: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    pub fn import_path(&mut self, path: &Path) -> Result<usize, ImportError> {
        let imported = import::load_path(path)?;
        let mut bodies = Vec::with_capacity(imported.bodies.len());
        for (i, body) in imported.bodies.into_iter().enumerate() {
            bodies.push(body_from_imported(i, body)?);
        }
        if bodies.is_empty() {
            return Err(ImportError::Empty);
        }
        let index = self.models.len();
        self.models.push(Model {
            name: file_stem(path),
            path: path.to_path_buf(),
            kind: imported.kind,
            bodies,
        });
        self.selection = vec![Selection::Model(index)];
        Ok(index)
    }

    pub(crate) fn push_imported(
        &mut self,
        name: &str,
        body: import::ImportedBody,
    ) -> Result<usize, ImportError> {
        let body = body_from_imported(0, body)?;
        let index = self.models.len();
        self.models.push(Model {
            name: name.to_string(),
            path: PathBuf::new(),
            kind: ModelKind::Step,
            bodies: vec![body],
        });
        self.selection = vec![Selection::Model(index)];
        Ok(index)
    }

    pub fn close_model(&mut self, model: usize) {
        if model >= self.models.len() {
            return;
        }
        self.models.remove(model);
        self.selection = self
            .selection
            .iter()
            .copied()
            .filter_map(|s| s.remap_after_remove(model))
            .collect();
    }

    pub fn close_selected(&mut self) {
        let Some(model) = self.selection.first().map(|s| s.model()) else {
            return;
        };
        self.close_model(model);
    }

    pub fn selected_model(&self) -> Option<(usize, &Model)> {
        let model = self.selection.first()?.model();
        self.models.get(model).map(|m| (model, m))
    }

    pub fn is_body_selected(&self, model: usize, body: usize) -> bool {
        self.selection.iter().any(|s| match s {
            Selection::Model(m) => *m == model,
            Selection::Body { model: m, body: b } => *m == model && *b == body,
            Selection::Face {
                model: m, body: b, ..
            }
            | Selection::Edge {
                model: m, body: b, ..
            }
            | Selection::Vertex {
                model: m, body: b, ..
            }
            | Selection::Node {
                model: m, body: b, ..
            }
            | Selection::Cell {
                model: m, body: b, ..
            }
            | Selection::MeshEdge {
                model: m, body: b, ..
            } => *m == model && *b == body,
        })
    }

    pub fn highlights_body(&self, model: usize, body: usize) -> bool {
        self.selection.iter().any(|s| match s {
            Selection::Model(m) => *m == model,
            Selection::Body { model: m, body: b } => *m == model && *b == body,
            _ => false,
        })
    }

    pub fn is_face_selected(&self, model: usize, body: usize, id: u64) -> bool {
        self.selection.iter().any(|s| {
            matches!(
                s,
                Selection::Face {
                    model: m,
                    body: b,
                    id: fid,
                } if *m == model && *b == body && *fid == id
            )
        })
    }

    pub fn is_edge_selected(&self, model: usize, body: usize, id: u64) -> bool {
        self.selection.iter().any(|s| {
            matches!(
                s,
                Selection::Edge {
                    model: m,
                    body: b,
                    id: eid,
                } if *m == model && *b == body && *eid == id
            )
        })
    }

    pub fn is_vertex_selected(&self, model: usize, body: usize, index: u32) -> bool {
        self.selection.iter().any(|s| {
            matches!(
                s,
                Selection::Vertex {
                    model: m,
                    body: b,
                    index: i,
                } if *m == model && *b == body && *i == index
            )
        })
    }

    pub fn is_node_selected(&self, model: usize, body: usize, index: u32) -> bool {
        self.selection.iter().any(|s| {
            matches!(
                s,
                Selection::Node {
                    model: m,
                    body: b,
                    index: i,
                } if *m == model && *b == body && *i == index
            )
        })
    }

    pub fn is_cell_selected(&self, model: usize, body: usize, index: u32) -> bool {
        self.selection.iter().any(|s| {
            matches!(
                s,
                Selection::Cell {
                    model: m,
                    body: b,
                    index: i,
                } if *m == model && *b == body && *i == index
            )
        })
    }

    pub fn bbox(&self) -> Option<[DVec3; 2]> {
        bbox_of(self.models.iter().flat_map(|m| m.bodies.iter()))
    }

    pub fn selection_bbox(&self) -> Option<[DVec3; 2]> {
        let mut acc: Option<[DVec3; 2]> = None;
        for s in &self.selection {
            let Some(bb) = self.item_bbox(*s) else {
                continue;
            };
            acc = Some(match acc {
                None => bb,
                Some([min, max]) => [min.min(bb[0]), max.max(bb[1])],
            });
        }
        acc
    }

    pub fn item_bbox(&self, s: Selection) -> Option<[DVec3; 2]> {
        match s {
            Selection::Model(m) => bbox_of(self.models.get(m)?.bodies.iter()),
            Selection::Body { model, body } => self
                .models
                .get(model)?
                .bodies
                .get(body)
                .map(|b| b.display.bbox),
            Selection::Face { model, body, id } => self
                .models
                .get(model)?
                .bodies
                .get(body)
                .and_then(|b| b.display.face_bbox(id)),
            Selection::Edge { model, body, id } => self
                .models
                .get(model)?
                .bodies
                .get(body)
                .and_then(|b| b.display.edge_bbox(id)),
            Selection::Vertex { model, body, index } => self
                .models
                .get(model)?
                .bodies
                .get(body)
                .and_then(|b| b.display.cad_vertices.get(index as usize))
                .map(|p| point_bbox(*p)),
            Selection::Node { model, body, index } => self
                .models
                .get(model)?
                .bodies
                .get(body)
                .and_then(|b| b.display.positions.get(index as usize))
                .map(|p| point_bbox(*p)),
            Selection::Cell { model, body, index } => self
                .models
                .get(model)?
                .bodies
                .get(body)
                .and_then(|b| b.display.cell_bbox(index as usize)),
            Selection::MeshEdge { model, body, a, b } => {
                let mesh = &self.models.get(model)?.bodies.get(body)?.display;
                let pa = *mesh.positions.get(a as usize)?;
                let pb = *mesh.positions.get(b as usize)?;
                Some(bbox_points(&[pa, pb]))
            }
        }
    }
}

pub fn bbox_of_model(model: &Model) -> Option<[DVec3; 2]> {
    bbox_of(model.bodies.iter())
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

fn body_from_imported(index: usize, body: import::ImportedBody) -> Result<Body, ImportError> {
    match body {
        import::ImportedBody::Solid(solid) => {
            let mesh = Solid::mesh(std::iter::once(&solid), Tessellation::default())
                .map_err(|_| ImportError::Tessellate)?;
            let display = DisplayMesh::from_cadrum(&mesh, Some(&solid));
            let face_count = solid.iter_face().count();
            let edge_count = solid.iter_edge().count();
            Ok(Body {
                name: format!("Solid {}", index + 1),
                display,
                stats: BodyStats::Solid {
                    volume: solid.volume(),
                    area: solid.area(),
                    center: solid.center(),
                    face_count,
                    edge_count,
                },
                solid: Some(solid),
            })
        }
        import::ImportedBody::Triangles {
            positions,
            triangles,
        } => {
            let display = DisplayMesh::from_triangles(positions, triangles, DEFAULT_RGB);
            let triangle_count = display.triangles.len();
            Ok(Body {
                name: "Mesh".to_string(),
                display,
                stats: BodyStats::Mesh { triangle_count },
                solid: None,
            })
        }
    }
}

fn bbox_of<'a>(bodies: impl Iterator<Item = &'a Body>) -> Option<[DVec3; 2]> {
    let mut acc: Option<[DVec3; 2]> = None;
    for body in bodies {
        acc = Some(match acc {
            None => body.display.bbox,
            Some([min, max]) => [min.min(body.display.bbox[0]), max.max(body.display.bbox[1])],
        });
    }
    acc
}

impl DisplayMesh {
    fn from_cadrum(mesh: &Mesh, solid: Option<&Solid>) -> Self {
        let positions: Vec<[f32; 3]> = mesh
            .vertices
            .iter()
            .map(|v| [v.x as f32, v.y as f32, v.z as f32])
            .collect();
        let normals: Vec<[f32; 3]> = mesh
            .normals
            .iter()
            .map(|n| [n.x as f32, n.y as f32, n.z as f32])
            .collect();
        let tri_count = mesh.indices.len() / 3;
        let mut triangles = Vec::with_capacity(tri_count);
        let mut triangle_colors = Vec::with_capacity(tri_count);
        let mut triangle_face_ids = Vec::with_capacity(tri_count);
        for ti in 0..tri_count {
            triangles.push([
                mesh.indices[ti * 3] as u32,
                mesh.indices[ti * 3 + 1] as u32,
                mesh.indices[ti * 3 + 2] as u32,
            ]);
            triangle_face_ids.push(mesh.face_ids.get(ti).copied().unwrap_or(0));
            let rgb = mesh
                .colormap
                .get(&mesh.face_ids[ti])
                .map(|c| {
                    [
                        (c.r * 255.0).clamp(0.0, 255.0) as u8,
                        (c.g * 255.0).clamp(0.0, 255.0) as u8,
                        (c.b * 255.0).clamp(0.0, 255.0) as u8,
                    ]
                })
                .unwrap_or(DEFAULT_RGB);
            triangle_colors.push(rgb);
        }
        let edges: Vec<[f32; 3]> = mesh
            .edges
            .iter()
            .map(|p| {
                if p.is_nan() {
                    [f32::NAN; 3]
                } else {
                    [p.x as f32, p.y as f32, p.z as f32]
                }
            })
            .collect();
        let (cad_edges, cad_vertices) = solid.map(cad_from_solid).unwrap_or_default();
        let bbox = bbox_from_positions(&positions);
        Self {
            positions,
            normals,
            triangles,
            triangle_colors,
            triangle_face_ids,
            edges,
            cad_edges,
            cad_vertices,
            bbox,
        }
    }

    fn from_triangles(positions: Vec<[f32; 3]>, triangles: Vec<[u32; 3]>, rgb: [u8; 3]) -> Self {
        let mut normals = vec![[0.0, 0.0, 1.0]; positions.len()];
        for tri in &triangles {
            let p0 = positions[tri[0] as usize];
            let p1 = positions[tri[1] as usize];
            let p2 = positions[tri[2] as usize];
            let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
            let n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
            let n = [n[0] / len, n[1] / len, n[2] / len];
            for &i in tri {
                normals[i as usize] = n;
            }
        }
        let triangle_colors = vec![rgb; triangles.len()];
        let triangle_face_ids = vec![0; triangles.len()];
        let bbox = bbox_from_positions(&positions);
        Self {
            positions,
            normals,
            triangles,
            triangle_colors,
            triangle_face_ids,
            edges: Vec::new(),
            cad_edges: Vec::new(),
            cad_vertices: Vec::new(),
            bbox,
        }
    }

    fn face_bbox(&self, id: u64) -> Option<[DVec3; 2]> {
        let mut pts = Vec::new();
        for (tri, fid) in self.triangles.iter().zip(self.triangle_face_ids.iter()) {
            if *fid != id {
                continue;
            }
            for &i in tri {
                pts.push(self.positions[i as usize]);
            }
        }
        (!pts.is_empty()).then(|| bbox_points(&pts))
    }

    fn edge_bbox(&self, id: u64) -> Option<[DVec3; 2]> {
        let edge = self.cad_edges.iter().find(|e| e.id == id)?;
        (!edge.points.is_empty()).then(|| bbox_points(&edge.points))
    }

    fn cell_bbox(&self, index: usize) -> Option<[DVec3; 2]> {
        let tri = self.triangles.get(index)?;
        Some(bbox_points(&[
            self.positions[tri[0] as usize],
            self.positions[tri[1] as usize],
            self.positions[tri[2] as usize],
        ]))
    }
}

fn cad_from_solid(solid: &Solid) -> (Vec<CadEdge>, Vec<[f32; 3]>) {
    let mut cad_edges = Vec::new();
    let mut cad_vertices = Vec::new();
    for edge in solid.iter_edge() {
        let mut points: Vec<[f32; 3]> = edge
            .approximation_segments(Tessellation::default())
            .into_iter()
            .map(|p| [p.x as f32, p.y as f32, p.z as f32])
            .collect();
        if points.len() < 2 {
            let a = edge.start_point();
            let b = edge.end_point();
            points = vec![
                [a.x as f32, a.y as f32, a.z as f32],
                [b.x as f32, b.y as f32, b.z as f32],
            ];
        }
        if let Some(&p) = points.first() {
            push_unique_vertex(&mut cad_vertices, p);
        }
        if let Some(&p) = points.last() {
            push_unique_vertex(&mut cad_vertices, p);
        }
        if points.len() >= 2 {
            cad_edges.push(CadEdge {
                id: edge.id(),
                points,
            });
        }
    }
    (cad_edges, cad_vertices)
}

fn push_unique_vertex(verts: &mut Vec<[f32; 3]>, p: [f32; 3]) {
    if verts.iter().any(|q| dist2(*q, p) < 1e-12) {
        return;
    }
    verts.push(p);
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    dx * dx + dy * dy + dz * dz
}

fn point_bbox(p: [f32; 3]) -> [DVec3; 2] {
    let v = DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64);
    [v, v]
}

fn bbox_points(pts: &[[f32; 3]]) -> [DVec3; 2] {
    bbox_from_positions(pts)
}

fn bbox_from_positions(positions: &[[f32; 3]]) -> [DVec3; 2] {
    let mut min = DVec3::splat(f64::INFINITY);
    let mut max = DVec3::splat(f64::NEG_INFINITY);
    for p in positions {
        let v = DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64);
        min = min.min(v);
        max = max.max(v);
    }
    if min.x > max.x {
        [DVec3::ZERO, DVec3::ONE]
    } else {
        [min, max]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrum::Solid;
    use std::io::Write;

    #[test]
    fn import_step_tessellates() {
        let solid = Solid::cube(DVec3::ZERO, DVec3::ONE);
        let path = std::env::temp_dir().join("oxiprep_document_test.step");
        {
            let mut file = std::fs::File::create(&path).unwrap();
            Solid::write_step(std::iter::once(&solid), &mut file).unwrap();
            file.flush().unwrap();
        }
        let mut document = Document::new();
        document.import_path(&path).expect("import");
        assert_eq!(document.models.len(), 1);
        let body = &document.models[0].bodies[0];
        assert!(!body.display.triangles.is_empty());
        assert!(!body.display.cad_edges.is_empty());
        assert!(!body.display.cad_vertices.is_empty());
        assert_eq!(
            body.display.triangle_face_ids.len(),
            body.display.triangles.len()
        );
        assert!(body.solid.is_some());
        let _ = std::fs::remove_file(path);
    }
}

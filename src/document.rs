use crate::import::{self, ImportError, ModelKind};
use cadrum::{DVec3, Mesh, Solid, Tessellation};
use std::path::{Path, PathBuf};

const DEFAULT_RGB: [u8; 3] = [0x8C, 0xAD, 0xC4];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    Model(usize),
    Body { model: usize, body: usize },
}

pub struct Document {
    pub models: Vec<Model>,
    pub selection: Option<Selection>,
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
    /// NaN-separated polylines in the same convention as cadrum.
    pub edges: Vec<[f32; 3]>,
    pub bbox: [DVec3; 2],
}

impl Document {
    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            selection: None,
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
        self.selection = Some(Selection::Model(index));
        Ok(index)
    }

    pub fn close_model(&mut self, model: usize) {
        if model >= self.models.len() {
            return;
        }
        self.models.remove(model);
        self.selection = if self.models.is_empty() {
            None
        } else {
            Some(Selection::Model(model.min(self.models.len() - 1)))
        };
    }

    pub fn close_selected(&mut self) {
        let Some(model) = self.selection.map(|s| match s {
            Selection::Model(m) | Selection::Body { model: m, .. } => m,
        }) else {
            return;
        };
        self.close_model(model);
    }

    pub fn selected_model(&self) -> Option<(usize, &Model)> {
        let model = match self.selection? {
            Selection::Model(m) | Selection::Body { model: m, .. } => m,
        };
        self.models.get(model).map(|m| (model, m))
    }

    pub fn is_body_selected(&self, model: usize, body: usize) -> bool {
        self.selection == Some(Selection::Body { model, body })
    }

    pub fn bbox(&self) -> Option<[DVec3; 2]> {
        bbox_of(self.models.iter().flat_map(|m| m.bodies.iter()))
    }

    pub fn selection_bbox(&self) -> Option<[DVec3; 2]> {
        match self.selection? {
            Selection::Model(m) => bbox_of(self.models.get(m)?.bodies.iter()),
            Selection::Body { model, body } => self
                .models
                .get(model)?
                .bodies
                .get(body)
                .map(|b| b.display.bbox),
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
            let display = DisplayMesh::from_cadrum(&mesh);
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
    fn from_cadrum(mesh: &Mesh) -> Self {
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
        for ti in 0..tri_count {
            triangles.push([
                mesh.indices[ti * 3] as u32,
                mesh.indices[ti * 3 + 1] as u32,
                mesh.indices[ti * 3 + 2] as u32,
            ]);
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
        let bbox = bbox_from_positions(&positions);
        Self {
            positions,
            normals,
            triangles,
            triangle_colors,
            edges,
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
        let bbox = bbox_from_positions(&positions);
        Self {
            positions,
            normals,
            triangles,
            triangle_colors,
            edges: Vec::new(),
            bbox,
        }
    }
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
        assert!(body.solid.is_some());
        let _ = std::fs::remove_file(path);
    }
}

use cadrum::{DVec3, Edge, Solid};

use crate::command::CommandError;
use crate::document::{self, Body, Document, Model};
use crate::import::ModelKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn label(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
        }
    }

    pub fn vec(self) -> DVec3 {
        match self {
            Self::X => DVec3::X,
            Self::Y => DVec3::Y,
            Self::Z => DVec3::Z,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plane {
    XY,
    YZ,
    XZ,
}

impl Plane {
    pub fn label(self) -> &'static str {
        match self {
            Self::XY => "XY",
            Self::YZ => "YZ",
            Self::XZ => "XZ",
        }
    }

    pub fn axes(self) -> (DVec3, DVec3) {
        match self {
            Self::XY => (DVec3::X, DVec3::Y),
            Self::YZ => (DVec3::Y, DVec3::Z),
            Self::XZ => (DVec3::X, DVec3::Z),
        }
    }

    pub fn normal(self) -> DVec3 {
        match self {
            Self::XY => DVec3::Z,
            Self::YZ => DVec3::X,
            Self::XZ => DVec3::Y,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CreateKind {
    Point {
        p: [f64; 3],
    },
    Line {
        a: [f64; 3],
        b: [f64; 3],
    },
    Rectangle {
        plane: Plane,
        origin: [f64; 3],
        width: f64,
        height: f64,
    },
    Disk {
        plane: Plane,
        center: [f64; 3],
        radius: f64,
    },
    Box {
        origin: [f64; 3],
        size: [f64; 3],
    },
    Cylinder {
        center: [f64; 3],
        axis: Axis,
        radius: f64,
        height: f64,
    },
    Cone {
        center: [f64; 3],
        axis: Axis,
        r1: f64,
        r2: f64,
        height: f64,
    },
    Sphere {
        center: [f64; 3],
        radius: f64,
    },
}

impl CreateKind {
    pub fn point() -> Self {
        Self::Point { p: [0.0; 3] }
    }

    pub fn line() -> Self {
        Self::Line {
            a: [0.0; 3],
            b: [1.0, 0.0, 0.0],
        }
    }

    pub fn rectangle() -> Self {
        Self::Rectangle {
            plane: Plane::XY,
            origin: [0.0; 3],
            width: 1.0,
            height: 1.0,
        }
    }

    pub fn disk() -> Self {
        Self::Disk {
            plane: Plane::XY,
            center: [0.0; 3],
            radius: 1.0,
        }
    }

    pub fn r#box() -> Self {
        Self::Box {
            origin: [0.0; 3],
            size: [1.0; 3],
        }
    }

    pub fn cylinder() -> Self {
        Self::Cylinder {
            center: [0.0; 3],
            axis: Axis::Z,
            radius: 0.5,
            height: 1.0,
        }
    }

    pub fn cone() -> Self {
        Self::Cone {
            center: [0.0; 3],
            axis: Axis::Z,
            r1: 0.5,
            r2: 0.0,
            height: 1.0,
        }
    }

    pub fn sphere() -> Self {
        Self::Sphere {
            center: [0.0; 3],
            radius: 1.0,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Point { .. } => "Point",
            Self::Line { .. } => "Line",
            Self::Rectangle { .. } => "Rectangle",
            Self::Disk { .. } => "Disk",
            Self::Box { .. } => "Box",
            Self::Cylinder { .. } => "Cylinder",
            Self::Cone { .. } => "Cone",
            Self::Sphere { .. } => "Sphere",
        }
    }

    pub fn valid(self) -> bool {
        match self {
            Self::Point { .. } => true,
            Self::Line { a, b } => dvec(a).distance(dvec(b)) > 1e-12,
            Self::Rectangle { width, height, .. } => width > 1e-12 && height > 1e-12,
            Self::Disk { radius, .. } => radius > 1e-12,
            Self::Box { size, .. } => size.iter().all(|s| *s > 1e-12),
            Self::Cylinder { radius, height, .. } => radius > 1e-12 && height > 1e-12,
            Self::Cone { r1, r2, height, .. } => {
                height > 1e-12 && (r1 > 1e-12 || r2 > 1e-12) && r1 >= 0.0 && r2 >= 0.0
            }
            Self::Sphere { radius, .. } => radius > 1e-12,
        }
    }

    pub fn build_body(self, name: &str) -> Result<Body, CommandError> {
        match self {
            Self::Point { p } => Ok(document::body_from_point(name, dvec(p))),
            Self::Line { a, b } => {
                let edge = Edge::line(dvec(a), dvec(b)).map_err(|_| failed(self))?;
                Ok(document::body_from_edges(name, vec![edge]))
            }
            Self::Rectangle {
                plane,
                origin,
                width,
                height,
            } => {
                let o = dvec(origin);
                let (u, v) = plane.axes();
                let p0 = o;
                let p1 = o + u * width;
                let p2 = o + u * width + v * height;
                let p3 = o + v * height;
                let edges = Edge::polygon(&[p0, p1, p2, p3]).map_err(|_| failed(self))?;
                Ok(document::body_from_edges(name, edges))
            }
            Self::Disk {
                plane,
                center,
                radius,
            } => {
                let edge = Edge::circle(radius, plane.normal())
                    .map_err(|_| failed(self))?
                    .translate(dvec(center));
                Ok(document::body_from_edges(name, vec![edge]))
            }
            Self::Box { origin, size } => {
                let a = dvec(origin);
                let b = a + DVec3::new(size[0], size[1], size[2]);
                let solid = Solid::cube(a, b);
                document::body_from_solid(name, solid).map_err(CommandError::from)
            }
            Self::Cylinder {
                center,
                axis,
                radius,
                height,
            } => {
                let solid = Solid::cylinder(radius, axis.vec() * height).translate(dvec(center));
                document::body_from_solid(name, solid).map_err(CommandError::from)
            }
            Self::Cone {
                center,
                axis,
                r1,
                r2,
                height,
            } => {
                let solid = Solid::cone(r1, r2, axis.vec() * height).translate(dvec(center));
                document::body_from_solid(name, solid).map_err(CommandError::from)
            }
            Self::Sphere { center, radius } => {
                let solid = Solid::sphere(radius).translate(dvec(center));
                document::body_from_solid(name, solid).map_err(CommandError::from)
            }
        }
    }

    pub fn into_model(self, document: &Document) -> Result<Model, CommandError> {
        let base = self.title();
        let name = document.unique_model_name(base);
        let body = self.build_body(&name)?;
        Ok(Model {
            name,
            path: std::path::PathBuf::new(),
            kind: ModelKind::Geometry,
            bodies: vec![body],
        })
    }

    pub fn into_body(self, document: &Document, model: usize) -> Result<Body, CommandError> {
        let name = document.unique_body_name(model, self.title());
        self.build_body(&name)
    }
}

fn dvec(p: [f64; 3]) -> DVec3 {
    DVec3::new(p[0], p[1], p[2])
}

fn failed(kind: CreateKind) -> CommandError {
    CommandError::Failed(format!(
        "Could not create the {}.",
        kind.title().to_ascii_lowercase()
    ))
}

pub struct CreateTool {
    pub kind: CreateKind,
    pub add_to_current: bool,
}

impl CreateTool {
    pub fn new(kind: CreateKind) -> Self {
        Self {
            kind,
            add_to_current: false,
        }
    }

    pub fn line_from_document(document: &Document) -> Self {
        let mut pts = Vec::new();
        for s in &document.selection {
            if let crate::document::Selection::Vertex { model, body, index } = *s
                && let Some(p) = document
                    .models
                    .get(model)
                    .and_then(|m| m.bodies.get(body))
                    .and_then(|b| b.display.cad_vertices.get(index as usize))
            {
                pts.push([p[0] as f64, p[1] as f64, p[2] as f64]);
            }
        }
        let kind = if pts.len() >= 2 {
            CreateKind::Line {
                a: pts[0],
                b: pts[1],
            }
        } else {
            CreateKind::line()
        };
        Self::new(kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_has_volume() {
        let body = CreateKind::r#box().build_body("Box").unwrap();
        match body.stats {
            crate::document::BodyStats::Solid { volume, .. } => {
                assert!((volume - 1.0).abs() < 1e-6);
            }
            _ => panic!("expected solid"),
        }
        assert!(!body.display.triangles.is_empty());
    }

    #[test]
    fn line_is_wire() {
        let body = CreateKind::line().build_body("Line").unwrap();
        assert!(matches!(
            body.stats,
            crate::document::BodyStats::Wire { edge_count: 1 }
        ));
        assert_eq!(body.display.cad_edges.len(), 1);
    }

    #[test]
    fn degenerate_box_is_invalid() {
        let kind = CreateKind::Box {
            origin: [0.0; 3],
            size: [1.0, 0.0, 1.0],
        };
        assert!(!kind.valid());
    }
}

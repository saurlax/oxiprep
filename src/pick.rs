use crate::document::{Document, Selection};
use crate::viewport::{Camera, PickMode};
use cadrum::DVec3;
use eframe::egui::{Pos2, Rect};

const VERTEX_PX: f32 = 10.0;
const EDGE_PX: f32 = 8.0;

pub fn pick(
    document: &Document,
    camera: &Camera,
    rect: Rect,
    pos: Pos2,
    mode: PickMode,
    clip: Option<[f32; 4]>,
) -> Option<Selection> {
    if mode == PickMode::Off || rect.width() < 1.0 || rect.height() < 1.0 {
        return None;
    }
    match mode {
        PickMode::Off => None,
        PickMode::Body => pick_triangle(document, camera, rect, pos, mode, clip)
            .or_else(|| pick_edge(document, camera, rect, pos, clip).map(as_body))
            .or_else(|| pick_vertex(document, camera, rect, pos, clip).map(as_body)),
        PickMode::Face | PickMode::Cell => pick_triangle(document, camera, rect, pos, mode, clip),
        PickMode::Edge => pick_edge(document, camera, rect, pos, clip),
        PickMode::Vertex => pick_vertex(document, camera, rect, pos, clip),
        PickMode::Node => pick_node(document, camera, rect, pos, clip),
    }
}

pub fn apply_click(document: &mut Document, hit: Option<Selection>, add: bool, toggle: bool) {
    match hit {
        None if !add && !toggle => document.selection.clear(),
        None => {}
        Some(item) if toggle => {
            if let Some(i) = document.selection.iter().position(|s| *s == item) {
                document.selection.remove(i);
            } else {
                document.selection.push(item);
            }
        }
        Some(item) if add => {
            if !document.selection.contains(&item) {
                document.selection.push(item);
            }
        }
        Some(item) => document.selection = vec![item],
    }
}

fn as_body(hit: Selection) -> Selection {
    match hit.body() {
        Some(body) => Selection::Body {
            model: hit.model(),
            body,
        },
        None => hit,
    }
}

fn pick_triangle(
    document: &Document,
    camera: &Camera,
    rect: Rect,
    pos: Pos2,
    mode: PickMode,
    clip: Option<[f32; 4]>,
) -> Option<Selection> {
    let (origin, dir) = camera.ray(pos, rect)?;
    let mut best_t = f64::INFINITY;
    let mut best: Option<Selection> = None;
    for (mi, model) in document.models.iter().enumerate() {
        for (bi, body) in model.bodies.iter().enumerate() {
            let mesh = &body.display;
            if mode == PickMode::Cell && !body.has_discrete_mesh() {
                continue;
            }
            for (ti, tri) in mesh.triangles.iter().enumerate() {
                let a = dvec(mesh.positions[tri[0] as usize]);
                let b = dvec(mesh.positions[tri[1] as usize]);
                let c = dvec(mesh.positions[tri[2] as usize]);
                let Some(t) = ray_triangle(origin, dir, a, b, c) else {
                    continue;
                };
                if t >= best_t {
                    continue;
                }
                let hit = origin + dir * t;
                if clipped(hit, clip) {
                    continue;
                }
                best_t = t;
                best = Some(match mode {
                    PickMode::Body => Selection::Body {
                        model: mi,
                        body: bi,
                    },
                    PickMode::Cell => Selection::Cell {
                        model: mi,
                        body: bi,
                        index: mesh.triangle_cells.get(ti).copied().unwrap_or(ti as u32),
                    },
                    PickMode::Face => {
                        let id = mesh.triangle_face_ids.get(ti).copied().unwrap_or(0);
                        if id != 0 {
                            Selection::Face {
                                model: mi,
                                body: bi,
                                id,
                            }
                        } else {
                            Selection::Cell {
                                model: mi,
                                body: bi,
                                index: mesh.triangle_cells.get(ti).copied().unwrap_or(ti as u32),
                            }
                        }
                    }
                    _ => unreachable!(),
                });
            }
        }
    }
    best
}

fn pick_edge(
    document: &Document,
    camera: &Camera,
    rect: Rect,
    pos: Pos2,
    clip: Option<[f32; 4]>,
) -> Option<Selection> {
    let mut best_d = EDGE_PX;
    let mut best: Option<Selection> = None;
    for (mi, model) in document.models.iter().enumerate() {
        for (bi, body) in model.bodies.iter().enumerate() {
            let mesh = &body.display;
            if !mesh.cad_edges.is_empty() {
                for edge in &mesh.cad_edges {
                    for w in edge.points.windows(2) {
                        if clipped(dvec(w[0]), clip) && clipped(dvec(w[1]), clip) {
                            continue;
                        }
                        let Some(d) = screen_seg_dist(camera, rect, pos, w[0], w[1]) else {
                            continue;
                        };
                        if d < best_d {
                            best_d = d;
                            best = Some(Selection::Edge {
                                model: mi,
                                body: bi,
                                id: edge.id,
                            });
                        }
                    }
                }
            } else if body.has_discrete_mesh() {
                for tri in &mesh.triangles {
                    let pts = [
                        mesh.positions[tri[0] as usize],
                        mesh.positions[tri[1] as usize],
                        mesh.positions[tri[2] as usize],
                    ];
                    for (i, j) in [(0, 1), (1, 2), (2, 0)] {
                        if clipped(dvec(pts[i]), clip) && clipped(dvec(pts[j]), clip) {
                            continue;
                        }
                        let Some(d) = screen_seg_dist(camera, rect, pos, pts[i], pts[j]) else {
                            continue;
                        };
                        if d < best_d {
                            best_d = d;
                            let (a, b) = ordered(tri[i], tri[j]);
                            best = Some(Selection::MeshEdge {
                                model: mi,
                                body: bi,
                                a,
                                b,
                            });
                        }
                    }
                }
            }
        }
    }
    best
}

fn pick_vertex(
    document: &Document,
    camera: &Camera,
    rect: Rect,
    pos: Pos2,
    clip: Option<[f32; 4]>,
) -> Option<Selection> {
    let mut best_d = VERTEX_PX;
    let mut best: Option<Selection> = None;
    for (mi, model) in document.models.iter().enumerate() {
        for (bi, body) in model.bodies.iter().enumerate() {
            let mesh = &body.display;
            if !mesh.cad_vertices.is_empty() {
                for (index, p) in mesh.cad_vertices.iter().enumerate() {
                    if clipped(dvec(*p), clip) {
                        continue;
                    }
                    let Some(d) = screen_point_dist(camera, rect, pos, *p) else {
                        continue;
                    };
                    if d < best_d {
                        best_d = d;
                        best = Some(Selection::Vertex {
                            model: mi,
                            body: bi,
                            index: index as u32,
                        });
                    }
                }
            } else if let Some(s) = closest_node(mesh, camera, rect, pos, clip, mi, bi, best_d) {
                best_d = s.1;
                best = Some(s.0);
            }
        }
    }
    best
}

fn pick_node(
    document: &Document,
    camera: &Camera,
    rect: Rect,
    pos: Pos2,
    clip: Option<[f32; 4]>,
) -> Option<Selection> {
    let mut best_d = VERTEX_PX;
    let mut best: Option<Selection> = None;
    for (mi, model) in document.models.iter().enumerate() {
        for (bi, body) in model.bodies.iter().enumerate() {
            if !body.has_discrete_mesh() {
                continue;
            }
            if let Some(s) = closest_node(&body.display, camera, rect, pos, clip, mi, bi, best_d) {
                best_d = s.1;
                best = Some(s.0);
            }
        }
    }
    best
}

#[allow(clippy::too_many_arguments)]
fn closest_node(
    mesh: &crate::document::DisplayMesh,
    camera: &Camera,
    rect: Rect,
    pos: Pos2,
    clip: Option<[f32; 4]>,
    model: usize,
    body: usize,
    mut best_d: f32,
) -> Option<(Selection, f32)> {
    let mut best = None;
    for (index, p) in mesh.positions.iter().enumerate() {
        if clipped(dvec(*p), clip) {
            continue;
        }
        let Some(d) = screen_point_dist(camera, rect, pos, *p) else {
            continue;
        };
        if d < best_d {
            best_d = d;
            best = Some((
                Selection::Node {
                    model,
                    body,
                    index: index as u32,
                },
                d,
            ));
        }
    }
    best
}

fn screen_point_dist(camera: &Camera, rect: Rect, pos: Pos2, p: [f32; 3]) -> Option<f32> {
    let s = camera.project(dvec(p), rect)?;
    Some(s.distance(pos))
}

fn screen_seg_dist(
    camera: &Camera,
    rect: Rect,
    pos: Pos2,
    a: [f32; 3],
    b: [f32; 3],
) -> Option<f32> {
    let sa = camera.project(dvec(a), rect)?;
    let sb = camera.project(dvec(b), rect)?;
    Some(point_seg_dist(pos, sa, sb))
}

fn point_seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 < 1e-8 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn clipped(p: DVec3, plane: Option<[f32; 4]>) -> bool {
    let Some(pl) = plane else {
        return false;
    };
    DVec3::new(pl[0] as f64, pl[1] as f64, pl[2] as f64).dot(p) + f64::from(pl[3]) < 0.0
}

fn dvec(p: [f32; 3]) -> DVec3 {
    DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64)
}

fn ordered(a: u32, b: u32) -> (u32, u32) {
    if a < b { (a, b) } else { (b, a) }
}

/// Möller–Trumbore. `dir` need not be unit; `t` is in the same units as `dir`.
pub fn ray_triangle(origin: DVec3, dir: DVec3, a: DVec3, b: DVec3, c: DVec3) -> Option<f64> {
    let e1 = b - a;
    let e2 = c - a;
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let tvec = origin - a;
    let u = tvec.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = tvec.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 1e-8).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::import::ImportedBody;
    use cadrum::Solid;

    #[test]
    fn ray_hits_unit_triangle() {
        let origin = DVec3::new(0.25, 0.25, 2.0);
        let dir = DVec3::new(0.0, 0.0, -1.0);
        let t = ray_triangle(origin, dir, DVec3::ZERO, DVec3::X, DVec3::Y).unwrap();
        assert!((t - 2.0).abs() < 1e-9);
    }

    #[test]
    fn ray_misses_triangle() {
        let origin = DVec3::new(2.0, 2.0, 2.0);
        let dir = DVec3::new(0.0, 0.0, -1.0);
        assert!(ray_triangle(origin, dir, DVec3::ZERO, DVec3::X, DVec3::Y).is_none());
    }

    #[test]
    fn pick_cube_face_from_plus_x() {
        let solid = Solid::cube(DVec3::ZERO, DVec3::ONE);
        let mut document = Document::new();
        document
            .push_imported("cube", ImportedBody::Solid(solid))
            .unwrap();
        let mut camera = Camera::isometric();
        camera.target = DVec3::splat(0.5);
        camera.look_along(DVec3::X);
        camera.distance = 4.0;
        let rect = Rect::from_min_size(Pos2::ZERO, eframe::egui::vec2(400.0, 400.0));
        let pos = Pos2::new(200.0, 200.0);
        let hit = pick(&document, &camera, rect, pos, PickMode::Face, None);
        let Some(Selection::Face { id, .. }) = hit else {
            panic!("expected face, got {hit:?}");
        };
        assert_ne!(id, 0);
        let body_hit = pick(&document, &camera, rect, pos, PickMode::Body, None);
        assert!(matches!(
            body_hit,
            Some(Selection::Body { model: 0, body: 0 })
        ));
        assert!(pick(&document, &camera, rect, pos, PickMode::Cell, None).is_none());
        assert!(pick(&document, &camera, rect, pos, PickMode::Node, None).is_none());
    }

    #[test]
    fn apply_click_replace_and_toggle() {
        let mut document = Document::new();
        let a = Selection::Body { model: 0, body: 0 };
        let b = Selection::Body { model: 0, body: 1 };
        apply_click(&mut document, Some(a), false, false);
        assert_eq!(document.selection, vec![a]);
        apply_click(&mut document, Some(b), true, false);
        assert_eq!(document.selection, vec![a, b]);
        apply_click(&mut document, Some(a), false, true);
        assert_eq!(document.selection, vec![b]);
        apply_click(&mut document, None, false, false);
        assert!(document.selection.is_empty());
    }
}

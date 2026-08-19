use cadrum::{DVec3, Face, Mesh as CadMesh, Solid, Tessellation};
use robust::{Coord, Coord3D, incircle, insphere, orient2d, orient3d};
use std::collections::{HashMap, HashSet};

use crate::command::CommandError;
use crate::document::{AnalysisMesh, BodyShape, Document};

const MAX_NODES: usize = 80_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshKind {
    Surface,
    Volume,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshTool {
    pub kind: MeshKind,
    pub size: f64,
}

impl MeshTool {
    pub fn new(kind: MeshKind, document: &Document) -> Self {
        Self {
            kind,
            size: default_size(document),
        }
    }

    pub fn title(self) -> &'static str {
        match self.kind {
            MeshKind::Surface => "Surface Mesh",
            MeshKind::Volume => "Volume Mesh",
        }
    }

    pub fn valid(self) -> bool {
        self.size > 0.0
    }
}

pub fn default_size(document: &Document) -> f64 {
    let bbox = document.selection_bbox().or_else(|| document.bbox());
    let Some([min, max]) = bbox else {
        return 0.1;
    };
    let d = (max - min).length();
    if d < 1e-12 { 0.1 } else { d / 8.0 }
}

pub fn mesh_targets(document: &Document) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if document.selection.is_empty() {
        for (mi, model) in document.models.iter().enumerate() {
            for (bi, body) in model.bodies.iter().enumerate() {
                if matches!(body.shape, BodyShape::Solid(_)) {
                    out.push((mi, bi));
                }
            }
        }
        return out;
    }
    for s in &document.selection {
        let mi = s.model();
        if let Some(bi) = s.body() {
            if document
                .models
                .get(mi)
                .and_then(|m| m.bodies.get(bi))
                .is_some_and(|b| matches!(b.shape, BodyShape::Solid(_)))
            {
                out.push((mi, bi));
            }
        } else if let Some(model) = document.models.get(mi) {
            for (bi, body) in model.bodies.iter().enumerate() {
                if matches!(body.shape, BodyShape::Solid(_)) {
                    out.push((mi, bi));
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

pub fn generate(solid: &Solid, kind: MeshKind, size: f64) -> Result<AnalysisMesh, CommandError> {
    if size <= 0.0 {
        return Err(CommandError::Failed(
            "Size must be greater than zero.".to_string(),
        ));
    }
    let (mut nodes, mut triangles, mut face_ids) = surface_mesh(solid, size)?;
    let tets = if kind == MeshKind::Volume {
        tet_fill(solid, &mut nodes, size)?
    } else {
        Vec::new()
    };
    if !tets.is_empty() {
        let (skin, skin_fids) = tet_skin(solid, &nodes, &tets);
        if !skin.is_empty() {
            triangles = skin;
            face_ids = skin_fids;
        }
    }
    Ok(AnalysisMesh {
        nodes,
        triangles,
        triangle_face_ids: face_ids,
        tets,
    })
}

fn surface_mesh(
    solid: &Solid,
    size: f64,
) -> Result<(Vec<[f32; 3]>, Vec<[u32; 3]>, Vec<u64>), CommandError> {
    let diag = {
        let bb = solid.bounding_box();
        (bb[1] - bb[0]).length().max(size)
    };
    let weld = (size * 1e-4).max(diag * 1e-9);
    let mut pts = PointSet::new(weld);
    let chains = discretize_edges(solid, &mut pts, size, diag);
    let tess = cad_tessellation(solid, size, diag)?;
    let mut triangles = Vec::new();
    let mut face_ids = Vec::new();
    for face in solid.iter_face() {
        let loops = face_loops(&face, &chains, &pts.nodes, weld);
        let mut tris = Vec::new();
        if let Some(frame) = planar_frame(&face, &loops, &pts.nodes, size) {
            tris = mesh_planar(&face, &frame, &loops, &mut pts, size);
        }
        if tris.is_empty() {
            tris = mesh_from_tess(&face, &tess, &loops, &mut pts, size);
        }
        if tris.is_empty() {
            continue;
        }
        enforce_size(&face, &loops, &mut pts, &mut tris, size);
        let fid = face.id();
        for t in tris {
            if t[0] == t[1] || t[1] == t[2] || t[2] == t[0] {
                continue;
            }
            let t = orient_outward(&pts.nodes, t, &face);
            triangles.push(orient_from_solid(solid, &pts.nodes, t));
            face_ids.push(fid);
        }
    }
    if triangles.is_empty() {
        return Err(CommandError::Failed(
            "Could not mesh the solid.".to_string(),
        ));
    }
    Ok((pts.nodes, triangles, face_ids))
}

fn cad_tessellation(solid: &Solid, size: f64, diag: f64) -> Result<CadMesh, CommandError> {
    let tess = Tessellation {
        deflection_linear: (size * 0.35).max(diag * 1e-4),
        deflection_angular: (size / (diag * 0.5)).clamp(0.08, 0.45),
        relative_linear: false,
    };
    Solid::mesh(std::iter::once(solid), tess)
        .map_err(|_| CommandError::Failed("Could not mesh the solid.".to_string()))
}

struct PointSet {
    nodes: Vec<[f32; 3]>,
    bins: HashMap<(i32, i32, i32), Vec<u32>>,
    weld: f64,
}

impl PointSet {
    fn new(weld: f64) -> Self {
        Self {
            nodes: Vec::new(),
            bins: HashMap::new(),
            weld: weld.max(1e-15),
        }
    }

    fn insert(&mut self, p: DVec3) -> u32 {
        if self.nodes.len() >= MAX_NODES {
            return self.nearest(p);
        }
        let inv = 1.0 / self.weld;
        let k = quant(p, inv);
        let eps2 = (self.weld * self.weld) as f32;
        let pf = [p.x as f32, p.y as f32, p.z as f32];
        for di in -1..=1 {
            for dj in -1..=1 {
                for dk in -1..=1 {
                    let nk = (k.0 + di, k.1 + dj, k.2 + dk);
                    if let Some(list) = self.bins.get(&nk) {
                        for &i in list {
                            if dist2(self.nodes[i as usize], pf) <= eps2 {
                                return i;
                            }
                        }
                    }
                }
            }
        }
        let i = self.nodes.len() as u32;
        self.nodes.push(pf);
        self.bins.entry(k).or_default().push(i);
        i
    }

    fn nearest(&self, p: DVec3) -> u32 {
        let pf = [p.x as f32, p.y as f32, p.z as f32];
        let mut best = 0;
        let mut best_d = f32::INFINITY;
        for (i, n) in self.nodes.iter().enumerate() {
            let d = dist2(*n, pf);
            if d < best_d {
                best_d = d;
                best = i as u32;
            }
        }
        best
    }
}

fn quant(p: DVec3, inv: f64) -> (i32, i32, i32) {
    (
        (p.x * inv).round() as i32,
        (p.y * inv).round() as i32,
        (p.z * inv).round() as i32,
    )
}

fn discretize_edges(
    solid: &Solid,
    pts: &mut PointSet,
    size: f64,
    diag: f64,
) -> HashMap<u64, Vec<u32>> {
    let tess = Tessellation {
        deflection_linear: (size * 0.12).max(diag * 1e-5),
        deflection_angular: (size / diag).clamp(0.04, 0.35),
        relative_linear: false,
    };
    let mut chains = HashMap::new();
    for edge in solid.iter_edge() {
        let poly = edge.approximation_segments(tess);
        if poly.len() < 2 {
            continue;
        }
        let closed =
            edge.is_closed() || (poly[0] - *poly.last().unwrap()).length() <= pts.weld * 4.0;
        let samples = resample_polyline(&poly, size, closed);
        let mut chain: Vec<u32> = samples.iter().map(|p| pts.insert(*p)).collect();
        chain.dedup();
        if chain.len() > 2 && chain.first() == chain.last() {
            chain.pop();
        }
        if chain.len() >= 2 {
            chains.insert(edge.id(), chain);
        }
    }
    chains
}

fn resample_polyline(poly: &[DVec3], size: f64, closed: bool) -> Vec<DVec3> {
    let mut cum = vec![0.0];
    for w in poly.windows(2) {
        cum.push(cum.last().copied().unwrap() + (w[1] - w[0]).length());
    }
    let len = *cum.last().unwrap_or(&0.0);
    if len < 1e-15 {
        return vec![poly[0]];
    }
    let min_seg = if closed { 3.0 } else { 1.0 };
    let nseg = (len / size).round().max(min_seg) as usize;
    let mut out = Vec::with_capacity(nseg + 1);
    for i in 0..=nseg {
        let s = len * i as f64 / nseg as f64;
        out.push(eval_polyline(poly, &cum, s));
    }
    out
}

fn eval_polyline(poly: &[DVec3], cum: &[f64], s: f64) -> DVec3 {
    if s <= 0.0 {
        return poly[0];
    }
    let total = *cum.last().unwrap_or(&0.0);
    if s >= total {
        return *poly.last().unwrap_or(&poly[0]);
    }
    for i in 1..cum.len() {
        if s <= cum[i] {
            let span = (cum[i] - cum[i - 1]).max(1e-15);
            let t = (s - cum[i - 1]) / span;
            return poly[i - 1] * (1.0 - t) + poly[i] * t;
        }
    }
    *poly.last().unwrap_or(&poly[0])
}

fn face_loops(
    face: &Face,
    chains: &HashMap<u64, Vec<u32>>,
    nodes: &[[f32; 3]],
    weld: f64,
) -> Vec<Vec<u32>> {
    let mut unused: HashMap<u64, Vec<u32>> = HashMap::new();
    for edge in face.iter_edge() {
        if let Some(chain) = chains.get(&edge.id()) {
            unused.insert(edge.id(), chain.clone());
        }
    }
    if unused.is_empty() {
        return Vec::new();
    }
    if unused.len() == 1 {
        let chain = unused.values().next().unwrap();
        if chain.len() >= 3 {
            return vec![chain.clone()];
        }
    }
    let eps2 = (weld * 8.0).powi(2) as f32;
    let same = |a: u32, b: u32| a == b || dist2(nodes[a as usize], nodes[b as usize]) <= eps2;
    let mut loops = Vec::new();
    while !unused.is_empty() {
        let id = unused.keys().copied().min().unwrap();
        let mut ring = unused.remove(&id).unwrap();
        loop {
            if ring.len() > 2 && same(*ring.last().unwrap(), ring[0]) {
                if ring.last() == Some(&ring[0]) {
                    ring.pop();
                }
                break;
            }
            let end = *ring.last().unwrap();
            let mut found = None;
            let mut cand: Vec<_> = unused.keys().copied().collect();
            cand.sort_unstable();
            for eid in cand {
                let chain = unused.get(&eid).unwrap();
                if same(chain[0], end) {
                    found = Some((eid, false));
                    break;
                }
                if same(*chain.last().unwrap(), end) {
                    found = Some((eid, true));
                    break;
                }
            }
            let Some((eid, rev)) = found else {
                break;
            };
            let mut chain = unused.remove(&eid).unwrap();
            if rev {
                chain.reverse();
            }
            ring.extend(chain.into_iter().skip(1));
            if ring.len() > 10_000 {
                break;
            }
        }
        ring.dedup();
        if ring.len() > 2 && same(*ring.last().unwrap(), ring[0]) && ring.last() == Some(&ring[0]) {
            ring.pop();
        }
        if ring.len() >= 3 {
            loops.push(ring);
        }
    }
    loops
}

struct Frame {
    origin: DVec3,
    x: DVec3,
    y: DVec3,
}

fn planar_frame(face: &Face, loops: &[Vec<u32>], nodes: &[[f32; 3]], size: f64) -> Option<Frame> {
    if loops.is_empty() {
        return None;
    }
    let mut pts = Vec::new();
    for ring in loops {
        for &i in ring {
            pts.push(dvec(nodes[i as usize]));
        }
    }
    if pts.len() < 3 {
        return None;
    }
    let origin = pts.iter().copied().sum::<DVec3>() / pts.len() as f64;
    let (_, mut n) = face.project(origin);
    if n.length_squared() < 1e-18 {
        return None;
    }
    n = n.normalize();
    let span = pts
        .iter()
        .map(|p| (*p - origin).length())
        .fold(0.0_f64, f64::max)
        .max(size);
    let max_d = pts
        .iter()
        .map(|p| (*p - origin).dot(n).abs())
        .fold(0.0_f64, f64::max);
    if max_d > (0.02 * span).max(1e-6) {
        return None;
    }
    let x = if n.z.abs() < 0.9 {
        n.cross(DVec3::Z).normalize()
    } else {
        n.cross(DVec3::Y).normalize()
    };
    let y = n.cross(x);
    Some(Frame { origin, x, y })
}

fn to2(frame: &Frame, p: DVec3) -> [f64; 2] {
    let d = p - frame.origin;
    [d.dot(frame.x), d.dot(frame.y)]
}

fn lift(frame: &Frame, uv: [f64; 2]) -> DVec3 {
    frame.origin + frame.x * uv[0] + frame.y * uv[1]
}

fn mesh_planar(
    face: &Face,
    frame: &Frame,
    loops: &[Vec<u32>],
    pts: &mut PointSet,
    size: f64,
) -> Vec<[u32; 3]> {
    let mut ids = Vec::new();
    let mut loops2 = Vec::new();
    for ring in loops {
        let mut r2 = Vec::with_capacity(ring.len());
        for &i in ring {
            ids.push(i);
            r2.push(to2(frame, dvec(pts.nodes[i as usize])));
        }
        if r2.len() >= 3 {
            loops2.push(r2);
        }
    }
    if loops2.is_empty() {
        return Vec::new();
    }
    let (umin, umax, vmin, vmax) = bbox2(loops2.iter().flatten().copied());
    let axis = |len: f64| -> i32 {
        if len <= size * 0.5 {
            0
        } else {
            (len / size).floor().clamp(0.0, 10_000.0) as i32
        }
    };
    let mut nu = axis(umax - umin);
    let mut nv = axis(vmax - vmin);
    while (nu as i64) * (nv as i64) > 20_000 && nu + nv > 2 {
        if nu >= nv {
            nu -= 1;
        } else {
            nv -= 1;
        }
    }
    let min_d = (size * 0.35).max(1e-12);
    if nu * nv > 1 {
        for i in 1..=nu {
            for j in 1..=nv {
                if pts.nodes.len() >= MAX_NODES {
                    break;
                }
                let uv = [
                    umin + (umax - umin) * i as f64 / (nu + 1) as f64,
                    vmin + (vmax - vmin) * j as f64 / (nv + 1) as f64,
                ];
                if !in_loops(uv, &loops2) || dist_to_loops(uv, &loops2) < min_d {
                    continue;
                }
                let p = face.project(lift(frame, uv)).0;
                let id = pts.insert(p);
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }
    let pts2: Vec<[f64; 2]> = ids
        .iter()
        .map(|&i| to2(frame, dvec(pts.nodes[i as usize])))
        .collect();
    let raw = delaunay2(&pts2);
    let mut tris = Vec::new();
    for t in raw {
        let a = pts2[t[0] as usize];
        let b = pts2[t[1] as usize];
        let c = pts2[t[2] as usize];
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() < 1e-18 {
            continue;
        }
        let g = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
        if !in_loops(g, &loops2) {
            continue;
        }
        let tri = [ids[t[0] as usize], ids[t[1] as usize], ids[t[2] as usize]];
        tris.push(orient_outward(&pts.nodes, tri, face));
    }
    tris
}

fn mesh_from_tess(
    face: &Face,
    tess: &CadMesh,
    loops: &[Vec<u32>],
    pts: &mut PointSet,
    size: f64,
) -> Vec<[u32; 3]> {
    let fid = face.id();
    let snap = (size * 0.35).max(pts.weld * 4.0);
    let snap2 = (snap * snap) as f32;
    let loop_nodes: Vec<u32> = loops.iter().flatten().copied().collect();
    let mut map = vec![u32::MAX; tess.vertices.len()];
    let tri_count = tess.indices.len() / 3;
    for ti in 0..tri_count {
        if tess.face_ids.get(ti).copied().unwrap_or(0) != fid {
            continue;
        }
        for k in 0..3 {
            let vi = tess.indices[ti * 3 + k];
            if map[vi] != u32::MAX {
                continue;
            }
            let p = tess.vertices[vi];
            let pf = [p.x as f32, p.y as f32, p.z as f32];
            let mut snapped = None;
            let mut best = snap2;
            for &n in &loop_nodes {
                let d = dist2(pts.nodes[n as usize], pf);
                if d <= best {
                    best = d;
                    snapped = Some(n);
                }
            }
            map[vi] = snapped.unwrap_or_else(|| pts.insert(face.project(p).0));
        }
    }
    let mut tris = Vec::new();
    for ti in 0..tri_count {
        if tess.face_ids.get(ti).copied().unwrap_or(0) != fid {
            continue;
        }
        let t = [
            map[tess.indices[ti * 3]],
            map[tess.indices[ti * 3 + 1]],
            map[tess.indices[ti * 3 + 2]],
        ];
        if t[0] == u32::MAX || t[1] == u32::MAX || t[2] == u32::MAX {
            continue;
        }
        if t[0] == t[1] || t[1] == t[2] || t[2] == t[0] {
            continue;
        }
        tris.push(orient_outward(&pts.nodes, t, face));
    }
    tris
}

fn enforce_size(
    face: &Face,
    loops: &[Vec<u32>],
    pts: &mut PointSet,
    tris: &mut Vec<[u32; 3]>,
    size: f64,
) {
    let max2 = (size * size) as f32;
    for _ in 0..50_000 {
        if pts.nodes.len() >= MAX_NODES {
            break;
        }
        let mut best: Option<(f32, u32, u32)> = None;
        for t in tris.iter() {
            for (i, j) in [(0usize, 1usize), (1, 2), (2, 0)] {
                let a = t[i].min(t[j]);
                let b = t[i].max(t[j]);
                let d2 = dist2(pts.nodes[a as usize], pts.nodes[b as usize]);
                if d2 > max2 && best.is_none_or(|(l, _, _)| d2 > l) {
                    best = Some((d2, a, b));
                }
            }
        }
        let Some((_, a, b)) = best else {
            break;
        };
        let pa = dvec(pts.nodes[a as usize]);
        let pb = dvec(pts.nodes[b as usize]);
        let mid = (pa + pb) * 0.5;
        let q = if is_loop_segment(loops, a, b) {
            project_to_face_edges(face, mid)
        } else {
            face.project(mid).0
        };
        let m = pts.insert(q);
        if m == a || m == b {
            break;
        }
        let mut next = Vec::with_capacity(tris.len() + 1);
        for t in tris.iter() {
            if let Some(k) = edge_index(*t, a, b) {
                let i = t[k];
                let j = t[(k + 1) % 3];
                let opp = t[(k + 2) % 3];
                if opp != m && i != m {
                    next.push([opp, i, m]);
                }
                if opp != m && j != m {
                    next.push([opp, m, j]);
                }
            } else {
                next.push(*t);
            }
        }
        *tris = next;
    }
}

fn is_loop_segment(loops: &[Vec<u32>], a: u32, b: u32) -> bool {
    for ring in loops {
        let n = ring.len();
        for i in 0..n {
            let u = ring[i];
            let v = ring[(i + 1) % n];
            if (u == a && v == b) || (u == b && v == a) {
                return true;
            }
        }
    }
    false
}

fn project_to_face_edges(face: &Face, p: DVec3) -> DVec3 {
    let mut best = p;
    let mut best_d = f64::INFINITY;
    for edge in face.iter_edge() {
        let (q, _) = edge.project(p);
        let d = (q - p).length_squared();
        if d < best_d {
            best_d = d;
            best = q;
        }
    }
    best
}

fn orient_outward(nodes: &[[f32; 3]], mut t: [u32; 3], face: &Face) -> [u32; 3] {
    let a = dvec(nodes[t[0] as usize]);
    let b = dvec(nodes[t[1] as usize]);
    let c = dvec(nodes[t[2] as usize]);
    let g = (a + b + c) / 3.0;
    let (_, n) = face.project(g);
    if (b - a).cross(c - a).dot(n) < 0.0 {
        t.swap(1, 2);
    }
    t
}

fn orient_from_solid(solid: &Solid, nodes: &[[f32; 3]], mut t: [u32; 3]) -> [u32; 3] {
    let a = dvec(nodes[t[0] as usize]);
    let b = dvec(nodes[t[1] as usize]);
    let c = dvec(nodes[t[2] as usize]);
    let n = (b - a).cross(c - a);
    let len = n.length();
    if len < 1e-18 {
        return t;
    }
    let n = n / len;
    let g = (a + b + c) / 3.0;
    let eps = (len.sqrt() * 1e-3).clamp(1e-8, 1e-3);
    let out = solid.contains(g + n * eps);
    let inn = solid.contains(g - n * eps);
    if out && !inn {
        t.swap(1, 2);
    }
    t
}

fn bbox2(pts: impl Iterator<Item = [f64; 2]>) -> (f64, f64, f64, f64) {
    let mut umin = f64::INFINITY;
    let mut umax = f64::NEG_INFINITY;
    let mut vmin = f64::INFINITY;
    let mut vmax = f64::NEG_INFINITY;
    for p in pts {
        umin = umin.min(p[0]);
        umax = umax.max(p[0]);
        vmin = vmin.min(p[1]);
        vmax = vmax.max(p[1]);
    }
    (umin, umax, vmin, vmax)
}

fn in_loops(p: [f64; 2], loops: &[Vec<[f64; 2]>]) -> bool {
    let mut inside = false;
    for ring in loops {
        if pnpoly(p, ring) {
            inside = !inside;
        }
    }
    inside
}

fn pnpoly(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let n = ring.len();
    let mut j = n - 1;
    for i in 0..n {
        let a = ring[i];
        let b = ring[j];
        let inter = (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1] + 0.0) + a[0];
        if inter {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn dist_to_loops(p: [f64; 2], loops: &[Vec<[f64; 2]>]) -> f64 {
    let mut best = f64::INFINITY;
    for ring in loops {
        let n = ring.len();
        for i in 0..n {
            best = best.min(dist_seg(p, ring[i], ring[(i + 1) % n]));
        }
    }
    best
}

fn dist_seg(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let den = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if den < 1e-30 {
        0.0
    } else {
        ((ap[0] * ab[0] + ap[1] * ab[1]) / den).clamp(0.0, 1.0)
    };
    let q = [a[0] + ab[0] * t, a[1] + ab[1] * t];
    let dx = p[0] - q[0];
    let dy = p[1] - q[1];
    (dx * dx + dy * dy).sqrt()
}

fn edge_index(t: [u32; 3], a: u32, b: u32) -> Option<usize> {
    for k in 0..3 {
        let i = t[k];
        let j = t[(k + 1) % 3];
        if (i == a && j == b) || (i == b && j == a) {
            return Some(k);
        }
    }
    None
}

fn tet_fill(
    solid: &Solid,
    nodes: &mut Vec<[f32; 3]>,
    size: f64,
) -> Result<Vec<[u32; 4]>, CommandError> {
    add_steiner_points(solid, nodes, size);
    let tets = delaunay_tets(nodes, size);
    if tets.is_empty() {
        return Err(CommandError::Failed(
            "Could not mesh the solid.".to_string(),
        ));
    }
    Ok(tets)
}

fn tet_skin(solid: &Solid, nodes: &[[f32; 3]], tets: &[[u32; 4]]) -> (Vec<[u32; 3]>, Vec<u64>) {
    let mut count: HashMap<[u32; 3], u32> = HashMap::new();
    for tet in tets {
        for face in tet_faces_undirected(*tet) {
            *count.entry(face).or_insert(0) += 1;
        }
    }
    let mut tris = Vec::new();
    let mut fids = Vec::new();
    for tet in tets {
        for tri in tet_outward_faces(nodes, *tet) {
            let key = face_key(tri[0], tri[1], tri[2]);
            if count.get(&key).copied().unwrap_or(0) != 1 {
                continue;
            }
            let c = (dvec(nodes[tri[0] as usize])
                + dvec(nodes[tri[1] as usize])
                + dvec(nodes[tri[2] as usize]))
                / 3.0;
            fids.push(cad_face_id(solid, c));
            tris.push(tri);
        }
    }
    (tris, fids)
}

/// Four faces of `tet` with right-hand winding whose normal points away from
/// the opposite vertex (out of the tet). Interior shared faces then oppose
/// each other, so back-face culling shows one side without z-fighting.
pub(crate) fn tet_outward_faces(nodes: &[[f32; 3]], tet: [u32; 4]) -> [[u32; 3]; 4] {
    [
        face_away(nodes, tet[0], tet[1], tet[2], tet[3]),
        face_away(nodes, tet[0], tet[1], tet[3], tet[2]),
        face_away(nodes, tet[0], tet[2], tet[3], tet[1]),
        face_away(nodes, tet[1], tet[2], tet[3], tet[0]),
    ]
}

fn face_away(nodes: &[[f32; 3]], a: u32, b: u32, c: u32, opp: u32) -> [u32; 3] {
    let pa = dvec(nodes[a as usize]);
    let pb = dvec(nodes[b as usize]);
    let pc = dvec(nodes[c as usize]);
    let po = dvec(nodes[opp as usize]);
    if (pb - pa).cross(pc - pa).dot(po - pa) > 0.0 {
        [a, c, b]
    } else {
        [a, b, c]
    }
}

fn cad_face_id(solid: &Solid, p: DVec3) -> u64 {
    let mut best = 0;
    let mut best_d = f64::INFINITY;
    for face in solid.iter_face() {
        let (q, _) = face.project(p);
        let d = (q - p).length_squared();
        if d < best_d {
            best_d = d;
            best = face.id();
        }
    }
    best
}

fn covers_surface(tets: &[[u32; 4]], tris: &[[u32; 3]]) -> bool {
    if tets.is_empty() {
        return false;
    }
    let mut faces = HashSet::with_capacity(tets.len() * 4);
    for tet in tets {
        for face in tet_faces_undirected(*tet) {
            faces.insert(face);
        }
    }
    tris.iter()
        .all(|t| faces.contains(&face_key(t[0], t[1], t[2])))
}

fn tet_faces_undirected(tet: [u32; 4]) -> [[u32; 3]; 4] {
    [
        face_key(tet[0], tet[1], tet[2]),
        face_key(tet[0], tet[1], tet[3]),
        face_key(tet[0], tet[2], tet[3]),
        face_key(tet[1], tet[2], tet[3]),
    ]
}

fn face_key(a: u32, b: u32, c: u32) -> [u32; 3] {
    let mut k = [a, b, c];
    k.sort_unstable();
    k
}

fn delaunay_tets(nodes: &[[f32; 3]], _size: f64) -> Vec<[u32; 4]> {
    let pts: Vec<[f64; 3]> = nodes
        .iter()
        .map(|p| [p[0] as f64, p[1] as f64, p[2] as f64])
        .collect();
    let raw = delaunay3(&pts);
    let mut tets = Vec::new();
    for tet in raw {
        let vol = tet_volume(nodes, tet);
        let mut tet = tet;
        if vol < 0.0 {
            tet.swap(0, 1);
        }
        tets.push(tet);
    }
    tets
}

fn add_steiner_points(solid: &Solid, nodes: &mut Vec<[f32; 3]>, size: f64) {
    let bb = solid.bounding_box();
    let dim = bb[1] - bb[0];
    let axis = |len: f64| -> i32 {
        if len <= size * 0.5 {
            0
        } else {
            (len / size).floor().clamp(0.0, 10_000.0) as i32
        }
    };
    let mut nx = axis(dim.x);
    let mut ny = axis(dim.y);
    let mut nz = axis(dim.z);
    let max_pts = MAX_NODES.saturating_sub(nodes.len()).min(40_000) as i64;
    while (nx as i64) * (ny as i64) * (nz as i64) > max_pts && nx + ny + nz > 3 {
        if nx >= ny && nx >= nz {
            nx -= 1;
        } else if ny >= nz {
            ny -= 1;
        } else {
            nz -= 1;
        }
    }
    if (nx as i64) * (ny as i64) * (nz as i64) <= 1 {
        return;
    }
    let min_d2 = (size * 0.35).max(1e-12).powi(2) as f32;
    for i in 1..=nx {
        for j in 1..=ny {
            for k in 1..=nz {
                if nodes.len() >= MAX_NODES {
                    return;
                }
                let p = DVec3::new(
                    bb[0].x + dim.x * i as f64 / (nx + 1) as f64,
                    bb[0].y + dim.y * j as f64 / (ny + 1) as f64,
                    bb[0].z + dim.z * k as f64 / (nz + 1) as f64,
                );
                if !solid.contains(p) {
                    continue;
                }
                let pf = [p.x as f32, p.y as f32, p.z as f32];
                if nodes.iter().any(|n| dist2(*n, pf) < min_d2) {
                    continue;
                }
                nodes.push(pf);
            }
        }
    }
}

fn delaunay3(points: &[[f64; 3]]) -> Vec<[u32; 4]> {
    let n = points.len();
    if n < 4 {
        return Vec::new();
    }
    let mut min = points[0];
    let mut max = points[0];
    for p in points {
        for i in 0..3 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
    }
    let c = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let mut r = 0.0f64;
    for p in points {
        r = r.max(dist3(c, *p));
    }
    r = r.max(1e-12);
    let mut pts = Vec::with_capacity(n + 4);
    let s = 32.0;
    pts.extend_from_slice(&[[s, s, s], [s, -s, -s], [-s, s, -s], [-s, -s, s]]);
    for (i, p) in points.iter().enumerate() {
        let t = 1e-12 * (i as f64 + 1.0);
        pts.push([
            (p[0] - c[0]) / r + t,
            (p[1] - c[1]) / r + 1.234567 * t,
            (p[2] - c[2]) / r + 2.345678 * t,
        ]);
    }

    let mut tets = vec![[0u32, 1, 2, 3]];
    if orient_tet(&pts, tets[0]) < 0.0 {
        tets[0].swap(0, 1);
    }

    let dup2 = 1e-24;
    for pi in 4..pts.len() {
        let p = pts[pi];
        if (0..pi).any(|j| {
            let d = dist3(pts[j], p);
            d * d < dup2
        }) {
            continue;
        }
        let Some(is_bad) = cavity_tets(&pts, &tets, p) else {
            continue;
        };
        let mut faces: HashMap<[u32; 3], Option<[u32; 3]>> = HashMap::new();
        for (ti, tet) in tets.iter().enumerate() {
            if !is_bad[ti] {
                continue;
            }
            for face in tet_inward_faces(*tet) {
                add_cavity_face(&mut faces, face);
            }
        }
        let mut next = Vec::with_capacity(tets.len());
        for (ti, tet) in tets.iter().enumerate() {
            if !is_bad[ti] {
                next.push(*tet);
            }
        }
        let pi = pi as u32;
        for face in faces.into_values().flatten() {
            let mut tet = [face[0], face[1], face[2], pi];
            if orient_tet(&pts, tet) < 0.0 {
                tet.swap(0, 1);
            }
            if orient_tet(&pts, tet) != 0.0 {
                next.push(tet);
            }
        }
        tets = next;
    }

    tets.into_iter()
        .filter(|t| t.iter().all(|&i| i >= 4))
        .map(|t| [t[0] - 4, t[1] - 4, t[2] - 4, t[3] - 4])
        .collect()
}

fn cavity_tets(pts: &[[f64; 3]], tets: &[[u32; 4]], p: [f64; 3]) -> Option<Vec<bool>> {
    let mut start = None;
    for (ti, tet) in tets.iter().enumerate() {
        if point_in_tet(pts, *tet, p) {
            start = Some(ti);
            break;
        }
    }
    let start = start.or_else(|| tets.iter().position(|tet| in_sphere(pts, *tet, p)))?;
    let nbr = tet_neighbors(tets);
    let mut is_bad = vec![false; tets.len()];
    let mut stack = vec![start];
    is_bad[start] = true;
    while let Some(ti) = stack.pop() {
        for n in nbr[ti] {
            let Some(tj) = n else {
                continue;
            };
            if is_bad[tj] {
                continue;
            }
            if in_sphere(pts, tets[tj], p) {
                is_bad[tj] = true;
                stack.push(tj);
            }
        }
    }
    Some(is_bad)
}

fn tet_neighbors(tets: &[[u32; 4]]) -> Vec<[Option<usize>; 4]> {
    let mut face_tet: HashMap<[u32; 3], (usize, usize)> = HashMap::new();
    let mut nbr = vec![[None; 4]; tets.len()];
    for (ti, tet) in tets.iter().enumerate() {
        for (fi, face) in tet_faces_undirected(*tet).iter().enumerate() {
            if let Some((other, ofi)) = face_tet.remove(face) {
                nbr[ti][fi] = Some(other);
                nbr[other][ofi] = Some(ti);
            } else {
                face_tet.insert(*face, (ti, fi));
            }
        }
    }
    nbr
}

fn point_in_tet(pts: &[[f64; 3]], tet: [u32; 4], p: [f64; 3]) -> bool {
    let a = c3(pts[tet[0] as usize]);
    let b = c3(pts[tet[1] as usize]);
    let c = c3(pts[tet[2] as usize]);
    let d = c3(pts[tet[3] as usize]);
    let q = c3(p);
    let v0 = orient3d(a, b, c, d);
    if v0 == 0.0 {
        return false;
    }
    orient3d(q, b, c, d) * v0 >= 0.0
        && orient3d(a, q, c, d) * v0 >= 0.0
        && orient3d(a, b, q, d) * v0 >= 0.0
        && orient3d(a, b, c, q) * v0 >= 0.0
}

fn delaunay2(points: &[[f64; 2]]) -> Vec<[u32; 3]> {
    let n = points.len();
    if n < 3 {
        return Vec::new();
    }
    let mut min = points[0];
    let mut max = points[0];
    for p in points {
        min[0] = min[0].min(p[0]);
        min[1] = min[1].min(p[1]);
        max[0] = max[0].max(p[0]);
        max[1] = max[1].max(p[1]);
    }
    let c = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5];
    let mut r = 1e-12_f64;
    for p in points {
        let dx = p[0] - c[0];
        let dy = p[1] - c[1];
        r = r.max((dx * dx + dy * dy).sqrt());
    }
    let mut pts = Vec::with_capacity(n + 3);
    let s = 40.0;
    pts.extend_from_slice(&[[s, s], [s, -s], [-s, 0.0]]);
    for (i, p) in points.iter().enumerate() {
        let t = 1e-12 * (i as f64 + 1.0);
        pts.push([(p[0] - c[0]) / r + t, (p[1] - c[1]) / r + 1.234567 * t]);
    }
    let mut tris = vec![[0u32, 1, 2]];
    if orient_tri(&pts, tris[0]) < 0.0 {
        tris[0].swap(1, 2);
    }
    let dup2 = 1e-24;
    for pi in 3..pts.len() {
        let p = pts[pi];
        if (0..pi).any(|j| {
            let dx = pts[j][0] - p[0];
            let dy = pts[j][1] - p[1];
            dx * dx + dy * dy < dup2
        }) {
            continue;
        }
        let Some(is_bad) = cavity_tris(&pts, &tris, p) else {
            continue;
        };
        let mut edges: HashMap<[u32; 2], Option<[u32; 2]>> = HashMap::new();
        for (ti, tri) in tris.iter().enumerate() {
            if !is_bad[ti] {
                continue;
            }
            add_cavity_edge(&mut edges, [tri[0], tri[1]]);
            add_cavity_edge(&mut edges, [tri[1], tri[2]]);
            add_cavity_edge(&mut edges, [tri[2], tri[0]]);
        }
        let mut next = Vec::with_capacity(tris.len());
        for (ti, tri) in tris.iter().enumerate() {
            if !is_bad[ti] {
                next.push(*tri);
            }
        }
        let pi = pi as u32;
        for edge in edges.into_values().flatten() {
            let mut tri = [edge[0], edge[1], pi];
            if orient_tri(&pts, tri) < 0.0 {
                tri.swap(0, 1);
            }
            if orient_tri(&pts, tri) != 0.0 {
                next.push(tri);
            }
        }
        tris = next;
    }
    tris.into_iter()
        .filter(|t| t.iter().all(|&i| i >= 3))
        .map(|t| [t[0] - 3, t[1] - 3, t[2] - 3])
        .collect()
}

fn cavity_tris(pts: &[[f64; 2]], tris: &[[u32; 3]], p: [f64; 2]) -> Option<Vec<bool>> {
    let mut start = None;
    for (ti, tri) in tris.iter().enumerate() {
        if point_in_tri(pts, *tri, p) {
            start = Some(ti);
            break;
        }
    }
    let start = start.or_else(|| tris.iter().position(|tri| in_circle(pts, *tri, p)))?;
    let nbr = tri_neighbors(tris);
    let mut is_bad = vec![false; tris.len()];
    let mut stack = vec![start];
    is_bad[start] = true;
    while let Some(ti) = stack.pop() {
        for n in nbr[ti] {
            let Some(tj) = n else {
                continue;
            };
            if is_bad[tj] {
                continue;
            }
            if in_circle(pts, tris[tj], p) {
                is_bad[tj] = true;
                stack.push(tj);
            }
        }
    }
    Some(is_bad)
}

fn tri_neighbors(tris: &[[u32; 3]]) -> Vec<[Option<usize>; 3]> {
    let mut edge_tri: HashMap<[u32; 2], (usize, usize)> = HashMap::new();
    let mut nbr = vec![[None; 3]; tris.len()];
    for (ti, tri) in tris.iter().enumerate() {
        let edges = [
            [tri[0].min(tri[1]), tri[0].max(tri[1])],
            [tri[1].min(tri[2]), tri[1].max(tri[2])],
            [tri[2].min(tri[0]), tri[2].max(tri[0])],
        ];
        for (ei, edge) in edges.iter().enumerate() {
            if let Some((other, oei)) = edge_tri.remove(edge) {
                nbr[ti][ei] = Some(other);
                nbr[other][oei] = Some(ti);
            } else {
                edge_tri.insert(*edge, (ti, ei));
            }
        }
    }
    nbr
}

fn point_in_tri(pts: &[[f64; 2]], tri: [u32; 3], p: [f64; 2]) -> bool {
    let a = c2(pts[tri[0] as usize]);
    let b = c2(pts[tri[1] as usize]);
    let c = c2(pts[tri[2] as usize]);
    let q = c2(p);
    let o = orient2d(a, b, c);
    if o == 0.0 {
        return false;
    }
    orient2d(q, b, c) * o >= 0.0 && orient2d(a, q, c) * o >= 0.0 && orient2d(a, b, q) * o >= 0.0
}

fn add_cavity_edge(edges: &mut HashMap<[u32; 2], Option<[u32; 2]>>, edge: [u32; 2]) {
    let mut key = edge;
    key.sort_unstable();
    edges
        .entry(key)
        .and_modify(|v| *v = None)
        .or_insert(Some(edge));
}

fn add_cavity_face(faces: &mut HashMap<[u32; 3], Option<[u32; 3]>>, face: [u32; 3]) {
    let mut key = face;
    key.sort_unstable();
    faces
        .entry(key)
        .and_modify(|v| *v = None)
        .or_insert(Some(face));
}

fn tet_inward_faces(tet: [u32; 4]) -> [[u32; 3]; 4] {
    [
        [tet[0], tet[1], tet[2]],
        [tet[0], tet[3], tet[1]],
        [tet[0], tet[2], tet[3]],
        [tet[1], tet[3], tet[2]],
    ]
}

fn c2(p: [f64; 2]) -> Coord<f64> {
    Coord { x: p[0], y: p[1] }
}

fn c3(p: [f64; 3]) -> Coord3D<f64> {
    Coord3D {
        x: p[0],
        y: p[1],
        z: p[2],
    }
}

fn orient_tri(pts: &[[f64; 2]], tri: [u32; 3]) -> f64 {
    orient2d(
        c2(pts[tri[0] as usize]),
        c2(pts[tri[1] as usize]),
        c2(pts[tri[2] as usize]),
    )
}

fn in_circle(pts: &[[f64; 2]], tri: [u32; 3], p: [f64; 2]) -> bool {
    let a = c2(pts[tri[0] as usize]);
    let b = c2(pts[tri[1] as usize]);
    let c = c2(pts[tri[2] as usize]);
    let o = orient2d(a, b, c);
    if o == 0.0 {
        return false;
    }
    incircle(a, b, c, c2(p)) * o > 0.0
}

fn orient_tet(pts: &[[f64; 3]], tet: [u32; 4]) -> f64 {
    orient3d(
        c3(pts[tet[0] as usize]),
        c3(pts[tet[1] as usize]),
        c3(pts[tet[2] as usize]),
        c3(pts[tet[3] as usize]),
    )
}

fn in_sphere(pts: &[[f64; 3]], tet: [u32; 4], p: [f64; 3]) -> bool {
    let a = c3(pts[tet[0] as usize]);
    let b = c3(pts[tet[1] as usize]);
    let c = c3(pts[tet[2] as usize]);
    let d = c3(pts[tet[3] as usize]);
    let o = orient3d(a, b, c, d);
    if o == 0.0 {
        return false;
    }
    insphere(a, b, c, d, c3(p)) * o > 0.0
}

fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn tet_volume(nodes: &[[f32; 3]], tet: [u32; 4]) -> f64 {
    let a = dvec(nodes[tet[0] as usize]);
    let b = dvec(nodes[tet[1] as usize]);
    let c = dvec(nodes[tet[2] as usize]);
    let d = dvec(nodes[tet[3] as usize]);
    (d - a).dot((b - a).cross(c - a))
}

fn dvec(p: [f32; 3]) -> DVec3 {
    DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64)
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    dx * dx + dy * dy + dz * dz
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::CreateKind;
    use crate::session::Session;

    #[test]
    fn surface_mesh_respects_size() {
        let mut session = Session::new();
        let model = CreateKind::r#box().into_model(&session.document).unwrap();
        session.create_model(model).unwrap();
        session.mesh_selected(MeshKind::Surface, 0.25).unwrap();
        let mesh = session.document.models[0].bodies[0].mesh.as_ref().unwrap();
        assert!(!mesh.triangles.is_empty());
        assert!(mesh.tets.is_empty());
        let mut max = 0.0f32;
        for t in &mesh.triangles {
            for (i, j) in [(0, 1), (1, 2), (2, 0)] {
                max = max.max(dist2(mesh.nodes[t[i] as usize], mesh.nodes[t[j] as usize]).sqrt());
            }
        }
        assert!(max <= 0.25 * 1.01);
    }

    #[test]
    fn volume_mesh_has_tets_and_undo() {
        let mut session = Session::new();
        let model = CreateKind::r#box().into_model(&session.document).unwrap();
        session.create_model(model).unwrap();
        assert!(session.document.models[0].bodies[0].mesh.is_none());
        session.mesh_selected(MeshKind::Volume, 0.5).unwrap();
        let body = &session.document.models[0].bodies[0];
        let mesh = body.mesh.as_ref().unwrap();
        assert!(!mesh.tets.is_empty());
        let mut hit = vec![0u32; mesh.nodes.len()];
        for tet in &mesh.tets {
            for &i in tet {
                hit[i as usize] += 1;
            }
        }
        let max_hit = *hit.iter().max().unwrap();
        assert!(
            max_hit < mesh.tets.len() as u32,
            "volume mesh must not fan every tet from one node"
        );
        let vol: f64 = mesh
            .tets
            .iter()
            .map(|t| tet_volume(&mesh.nodes, *t).abs())
            .sum::<f64>()
            / 6.0;
        assert!((vol - 1.0).abs() < 0.08, "filled volume {vol}");
        assert!(body.display.triangle_interior.iter().any(|v| *v));
        assert!(body.display.triangle_interior.iter().any(|v| !*v));
        assert!(!body.display.triangles.is_empty());
        session.undo().unwrap();
        assert!(session.document.models[0].bodies[0].mesh.is_none());
        session.redo().unwrap();
        assert!(session.document.models[0].bodies[0].mesh.is_some());
    }

    #[test]
    fn delaunay_fills_unit_cube() {
        let pts = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        let tets = delaunay3(&pts);
        let nodes: Vec<[f32; 3]> = pts
            .iter()
            .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
            .collect();
        let vols: Vec<f64> = tets
            .iter()
            .map(|t| tet_volume(&nodes, *t).abs() / 6.0)
            .collect();
        let vol: f64 = vols.iter().sum();
        assert!(
            (vol - 1.0).abs() < 1e-4,
            "volume {vol} tets={} vols={vols:?}",
            tets.len()
        );
        assert!(tets.len() >= 5, "tets={}", tets.len());
    }

    #[test]
    fn volume_keeps_surface_triangles() {
        let mut session = Session::new();
        let model = CreateKind::r#box().into_model(&session.document).unwrap();
        session.create_model(model).unwrap();
        session.mesh_selected(MeshKind::Volume, 0.5).unwrap();
        let mesh = session.document.models[0].bodies[0].mesh.as_ref().unwrap();
        assert!(covers_surface(&mesh.tets, &mesh.triangles));
    }

    #[test]
    fn tet_faces_point_away_from_opposite_vertex() {
        let nodes = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.5, 0.866, 0.0],
            [0.5, 0.289, 0.816],
        ];
        let tet = [0u32, 1, 2, 3];
        assert!(tet_volume(&nodes, tet) > 0.0);
        for tri in tet_outward_faces(&nodes, tet) {
            let a = dvec(nodes[tri[0] as usize]);
            let b = dvec(nodes[tri[1] as usize]);
            let c = dvec(nodes[tri[2] as usize]);
            let n = (b - a).cross(c - a);
            let opp = tet.iter().copied().find(|i| !tri.contains(i)).unwrap();
            let po = dvec(nodes[opp as usize]);
            assert!(
                n.dot(po - a) < 0.0,
                "face {tri:?} does not face outward of the tet"
            );
        }
    }

    #[test]
    fn box_mesh_normals_point_outward() {
        let center = DVec3::new(0.5, 0.5, 0.5);
        for kind in [MeshKind::Surface, MeshKind::Volume] {
            let mut session = Session::new();
            let model = CreateKind::r#box().into_model(&session.document).unwrap();
            session.create_model(model).unwrap();
            session.mesh_selected(kind, 0.5).unwrap();
            let body = &session.document.models[0].bodies[0];
            let mesh = body.mesh.as_ref().unwrap();
            let mut flipped = 0;
            let mut total = 0;
            if kind == MeshKind::Volume {
                for (ti, t) in body.display.triangles.iter().enumerate() {
                    if body
                        .display
                        .triangle_interior
                        .get(ti)
                        .copied()
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    total += 1;
                    if !triangle_faces_out(&mesh.nodes, *t, center) {
                        flipped += 1;
                    }
                }
            } else {
                for t in &mesh.triangles {
                    total += 1;
                    if !triangle_faces_out(&mesh.nodes, *t, center) {
                        flipped += 1;
                    }
                }
            }
            assert!(total > 0);
            assert_eq!(
                flipped, 0,
                "{kind:?} mesh has {flipped}/{total} inward boundary faces"
            );
        }
    }

    fn triangle_faces_out(nodes: &[[f32; 3]], t: [u32; 3], center: DVec3) -> bool {
        let a = dvec(nodes[t[0] as usize]);
        let b = dvec(nodes[t[1] as usize]);
        let c = dvec(nodes[t[2] as usize]);
        let n = (b - a).cross(c - a);
        let g = (a + b + c) / 3.0;
        n.dot(g - center) > 0.0
    }

    #[test]
    fn volume_skin_is_closed() {
        let mut session = Session::new();
        let model = CreateKind::r#box().into_model(&session.document).unwrap();
        session.create_model(model).unwrap();
        session.mesh_selected(MeshKind::Volume, 0.5).unwrap();
        let mesh = session.document.models[0].bodies[0].mesh.as_ref().unwrap();
        assert_eq!(tet_skin_chi(&mesh.tets), 2);
        let vol = tet_mesh_volume(&mesh.nodes, &mesh.tets);
        assert!((vol - 1.0).abs() < 0.08, "filled volume {vol}");
    }

    #[test]
    fn cylinder_volume_is_closed() {
        let mut session = Session::new();
        let model = CreateKind::cylinder()
            .into_model(&session.document)
            .unwrap();
        session.create_model(model).unwrap();
        session.mesh_selected(MeshKind::Volume, 0.25).unwrap();
        let mesh = session.document.models[0].bodies[0].mesh.as_ref().unwrap();
        assert!(!mesh.tets.is_empty());
        assert_eq!(tet_skin_chi(&mesh.tets), 2);
        let vol = tet_mesh_volume(&mesh.nodes, &mesh.tets);
        let expect = std::f64::consts::PI * 0.25;
        assert!(
            vol > expect * 0.7 && vol < expect * 1.05,
            "cylinder volume {vol} expected ~{expect}"
        );
    }

    fn tet_mesh_volume(nodes: &[[f32; 3]], tets: &[[u32; 4]]) -> f64 {
        tets.iter()
            .map(|t| tet_volume(nodes, *t).abs())
            .sum::<f64>()
            / 6.0
    }

    fn tet_skin_chi(tets: &[[u32; 4]]) -> i32 {
        let mut count: HashMap<[u32; 3], u32> = HashMap::new();
        for tet in tets {
            for face in tet_faces_undirected(*tet) {
                *count.entry(face).or_insert(0) += 1;
            }
        }
        assert!(count.values().all(|&c| c == 1 || c == 2));
        let skin: Vec<_> = count
            .iter()
            .filter(|(_, c)| **c == 1)
            .map(|(f, _)| *f)
            .collect();
        let mut verts = HashSet::new();
        let mut edge_use: HashMap<(u32, u32), u32> = HashMap::new();
        for f in &skin {
            verts.extend(*f);
            for (a, b) in [(f[0], f[1]), (f[1], f[2]), (f[2], f[0])] {
                let e = (a.min(b), a.max(b));
                *edge_use.entry(e).or_insert(0) += 1;
            }
        }
        assert!(
            edge_use.values().all(|&c| c == 2),
            "tet skin has non-manifold edges"
        );
        verts.len() as i32 - edge_use.len() as i32 + skin.len() as i32
    }

    #[test]
    fn cylinder_nodes_stay_on_surface() {
        let mut session = Session::new();
        let model = CreateKind::cylinder()
            .into_model(&session.document)
            .unwrap();
        session.create_model(model).unwrap();
        session.mesh_selected(MeshKind::Surface, 0.25).unwrap();
        let mesh = session.document.models[0].bodies[0].mesh.as_ref().unwrap();
        for p in &mesh.nodes {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            let on_side = (r - 0.5).abs() < 0.03 && p[2] >= -0.03 && p[2] <= 1.03;
            let on_cap = r <= 0.53 && (p[2].abs() < 0.03 || (p[2] - 1.0).abs() < 0.03);
            assert!(on_side || on_cap, "node {p:?} left the cylinder (r={r})");
        }
    }
}

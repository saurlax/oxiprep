use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use cadrum::Solid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelKind {
    Step,
    Brep,
    Stl,
}

impl ModelKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Step => "STEP",
            Self::Brep => "BRep",
            Self::Stl => "STL",
        }
    }
}

pub enum ImportedBody {
    Solid(Solid),
    Triangles {
        positions: Vec<[f32; 3]>,
        triangles: Vec<[u32; 3]>,
    },
}

pub struct ImportedModel {
    pub kind: ModelKind,
    pub bodies: Vec<ImportedBody>,
}

#[derive(Debug)]
pub enum ImportError {
    Io,
    Unsupported,
    Empty,
    Step,
    Brep,
    Stl,
    Tessellate,
}

impl ImportError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Io => "Could not open the file.",
            Self::Unsupported => "This file type cannot be opened.",
            Self::Empty => "The file contains no geometry.",
            Self::Step => "Could not read the STEP file.",
            Self::Brep => "Could not read the BRep file.",
            Self::Stl => "Could not read the STL file.",
            Self::Tessellate => "Could not display the model.",
        }
    }
}

pub fn load_path(path: &Path) -> Result<ImportedModel, ImportError> {
    let kind = kind_from_path(path).ok_or(ImportError::Unsupported)?;
    match kind {
        ModelKind::Step => load_step(path),
        ModelKind::Brep => load_brep(path),
        ModelKind::Stl => load_stl(path),
    }
}

pub fn kind_from_path(path: &Path) -> Option<ModelKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "step" | "stp" => Some(ModelKind::Step),
        "brep" => Some(ModelKind::Brep),
        "stl" => Some(ModelKind::Stl),
        _ => None,
    }
}

fn load_step(path: &Path) -> Result<ImportedModel, ImportError> {
    let mut file = File::open(path).map_err(|_| ImportError::Io)?;
    let solids = Solid::read_step(&mut file).map_err(|_| ImportError::Step)?;
    if solids.is_empty() {
        return Err(ImportError::Empty);
    }
    Ok(ImportedModel {
        kind: ModelKind::Step,
        bodies: solids.into_iter().map(ImportedBody::Solid).collect(),
    })
}

fn load_brep(path: &Path) -> Result<ImportedModel, ImportError> {
    let mut file = File::open(path).map_err(|_| ImportError::Io)?;
    let solids = Solid::read_brep(&mut file).map_err(|_| ImportError::Brep)?;
    if solids.is_empty() {
        return Err(ImportError::Empty);
    }
    Ok(ImportedModel {
        kind: ModelKind::Brep,
        bodies: solids.into_iter().map(ImportedBody::Solid).collect(),
    })
}

fn load_stl(path: &Path) -> Result<ImportedModel, ImportError> {
    let file = File::open(path).map_err(|_| ImportError::Io)?;
    let mut bytes = Vec::new();
    BufReader::new(file)
        .read_to_end(&mut bytes)
        .map_err(|_| ImportError::Io)?;
    let (positions, triangles) = parse_stl(&bytes).ok_or(ImportError::Stl)?;
    if triangles.is_empty() {
        return Err(ImportError::Empty);
    }
    Ok(ImportedModel {
        kind: ModelKind::Stl,
        bodies: vec![ImportedBody::Triangles {
            positions,
            triangles,
        }],
    })
}

fn parse_stl(bytes: &[u8]) -> Option<(Vec<[f32; 3]>, Vec<[u32; 3]>)> {
    if is_ascii_stl(bytes) {
        parse_ascii_stl(bytes)
    } else {
        parse_binary_stl(bytes)
    }
}

fn is_ascii_stl(bytes: &[u8]) -> bool {
    let head = std::str::from_utf8(bytes.get(..5).unwrap_or_default())
        .unwrap_or("")
        .eq_ignore_ascii_case("solid");
    if !head {
        return false;
    }
    // Binary STL can also start with "solid"; a plausible triangle count is the tie-break.
    if bytes.len() < 84 {
        return true;
    }
    let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    let Some(payload) = count.checked_mul(50) else {
        return true;
    };
    let Some(expected) = 84usize.checked_add(payload) else {
        return true;
    };
    expected != bytes.len()
}

fn parse_binary_stl(bytes: &[u8]) -> Option<(Vec<[f32; 3]>, Vec<[u32; 3]>)> {
    if bytes.len() < 84 {
        return None;
    }
    let count = u32::from_le_bytes(bytes[80..84].try_into().ok()?) as usize;
    if bytes.len() < 84 + count * 50 {
        return None;
    }
    let mut positions = Vec::with_capacity(count * 3);
    let mut triangles = Vec::with_capacity(count);
    for i in 0..count {
        let off = 84 + i * 50;
        let mut verts = [[0f32; 3]; 3];
        for v in 0..3 {
            let vo = off + 12 + v * 12;
            verts[v] = [
                f32::from_le_bytes(bytes[vo..vo + 4].try_into().ok()?),
                f32::from_le_bytes(bytes[vo + 4..vo + 8].try_into().ok()?),
                f32::from_le_bytes(bytes[vo + 8..vo + 12].try_into().ok()?),
            ];
        }
        let i0 = positions.len() as u32;
        positions.extend_from_slice(&verts);
        triangles.push([i0, i0 + 1, i0 + 2]);
    }
    Some((positions, triangles))
}

fn parse_ascii_stl(bytes: &[u8]) -> Option<(Vec<[f32; 3]>, Vec<[u32; 3]>)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut positions = Vec::new();
    let mut triangles = Vec::new();
    let mut verts = Vec::with_capacity(3);
    for token_line in text.lines() {
        let line = token_line.trim();
        let Some(rest) = line
            .strip_prefix("vertex")
            .or_else(|| line.strip_prefix("VERTEX"))
        else {
            continue;
        };
        let mut nums = rest.split_whitespace();
        let x: f32 = nums.next()?.parse().ok()?;
        let y: f32 = nums.next()?.parse().ok()?;
        let z: f32 = nums.next()?.parse().ok()?;
        verts.push([x, y, z]);
        if verts.len() == 3 {
            let i0 = positions.len() as u32;
            positions.extend_from_slice(&verts);
            triangles.push([i0, i0 + 1, i0 + 2]);
            verts.clear();
        }
    }
    if triangles.is_empty() {
        None
    } else {
        Some((positions, triangles))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrum::DVec3;
    use std::io::Write;

    #[test]
    fn binary_stl_roundtrip() {
        let mut bytes = vec![0u8; 80];
        bytes.extend_from_slice(&1u32.to_le_bytes());
        fn push_f32s(buf: &mut Vec<u8>, values: [f32; 3]) {
            for v in values {
                buf.extend_from_slice(&v.to_le_bytes());
            }
        }
        push_f32s(&mut bytes, [0.0, 0.0, 1.0]);
        push_f32s(&mut bytes, [0.0, 0.0, 0.0]);
        push_f32s(&mut bytes, [1.0, 0.0, 0.0]);
        push_f32s(&mut bytes, [0.0, 1.0, 0.0]);
        bytes.extend_from_slice(&0u16.to_le_bytes());
        let (positions, triangles) = parse_stl(&bytes).expect("parse");
        assert_eq!(triangles.len(), 1);
        assert_eq!(positions.len(), 3);
        assert_eq!(positions[1], [1.0, 0.0, 0.0]);
    }

    #[test]
    fn ascii_stl_parse() {
        let text = b"solid tri
  facet normal 0 0 1
    outer loop
      vertex 0 0 0
      vertex 1 0 0
      vertex 0 1 0
    endloop
  endfacet
endsolid tri
";
        let (positions, triangles) = parse_stl(text).expect("parse");
        assert_eq!(triangles, vec![[0, 1, 2]]);
        assert_eq!(positions[2], [0.0, 1.0, 0.0]);
    }

    #[test]
    fn step_roundtrip_imports_a_solid() {
        let solid = Solid::cube(DVec3::ZERO, DVec3::ONE);
        let dir = std::env::temp_dir();
        let path = dir.join("oxiprep_import_test.step");
        {
            let mut file = File::create(&path).unwrap();
            Solid::write_step(std::iter::once(&solid), &mut file).unwrap();
            file.flush().unwrap();
        }
        let imported = load_path(&path).expect("import STEP");
        assert_eq!(imported.kind, ModelKind::Step);
        assert_eq!(imported.bodies.len(), 1);
        let _ = std::fs::remove_file(path);
    }
}

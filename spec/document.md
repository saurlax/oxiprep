# Document model

## Project

A **project** is the native save unit (new / open / save / save as / close). It stores:

- Units and display unit preferences
- Geometry bodies and feature history needed to regenerate or at least to persist BRep
- Meshes and mesh settings used to generate them
- Named groups
- Materials and assignments
- Case (physics, BCs) — may be empty
- Results handles — empty until post
- Viewport camera and tree visibility (optional, user convenience)

Imported files are referenced or embedded. Embedding BRep and mesh in the project is required so a `.oxiprep` file opens without the original STEP sitting next to it. Large meshes may be sidecar files next to the project; the spec only requires a defined layout.

Working directory: a project-local folder for mesh scratch, solver decks, and logs. User-settable; default next to the project file.

## Tree

```text
Project
├── Geometry
│   ├── Part / Body
│   │   ├── Solid
│   │   ├── Faces
│   │   ├── Edges
│   │   └── Vertices
│   └── Datum (planes, later axes/CSYS)
├── Mesh
│   ├── Mesh (per component or global)
│   └── Sets (node / element / face / edge)
├── Groups          (named selections; may point at geo or mesh)
├── Materials
├── Case            (later fill; node present)
│   ├── Physics
│   └── Boundary conditions / loads
└── Results         (later; node present, empty)
```

STEP assemblies import as a part tree under Geometry, not a flat list of solids. Each leaf can carry a mesh.

Show/hide, rename, delete, and color apply from the tree. Tree selection and viewport selection stay in sync.

## Identity

Stable IDs for:

- Models/parts, components/bodies
- Geometry subshapes (face/edge/vertex) for picking and groups
- Mesh nodes and elements (local indices that do not reshuffle on append)
- Groups, materials, BC objects

Geometry–mesh map: a geometry edge/face can list the mesh nodes/elements discretizing it. Required after meshing from CAD so BCs on faces survive remesh when the map is rebuilt. Groups should be able to store geometry references (preferred) and resolve to mesh at export time.

## Units

Internal storage: SI (metre, second, kilogram, kelvin). UI shows a user unit system (mm/N/s, inch/lbf, etc.). Properties and mesh size fields convert at the UI boundary. The spec does not require a full units-aware expression engine in v1, only consistent length (and later time/force/temperature) conversion.

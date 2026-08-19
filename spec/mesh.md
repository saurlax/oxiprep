# Mesh

## Representation

Unstructured mesh, self-contained:

- Node coordinates
- 0D / 1D / 2D / 3D elements
- 2D: triangles, quads, polygons (variable node count)
- 3D: tet, hex, wedge/prism, pyramid, polyhedron
- Optional independent face and edge elements (skins, beams)
- Cell and node attributes (quality, later results)
- Optional patches/blocks (CFD families) as named sets

Quadratic elements (tet10, hex20, …) are P1: read/write and display; generation may come later.

## Import / export

Import and export are pluggable by extension. Built-in priority:

| Format | In | Out | Priority |
| --- | --- | --- | --- |
| STL | yes | yes | P0 |
| VTK legacy (`.vtk`) | yes | yes | P0 |
| OBJ | yes | yes | P1 |
| PLY | yes | P2 | P1 |
| MEDIT (`.mesh`, `.meshb`) | yes | yes | P1 |
| Abaqus (`.inp`) mesh | yes | yes | P1 |
| CGNS | yes | yes | P1 |
| Nastran/Patran (`.bdf`) | yes | P2 | P1 |
| Gmsh (`.msh`) | yes | yes | P1 |
| Fluent (`.msh`) | P1 | P1 | P1 |
| OpenFOAM (`.foam`) | P2 | — | P2 |
| SU2 | P2 | P2 | P2 |
| Tecplot / LS-DYNA / PDB | — | — | later if demanded |

Lab-specific mesh formats are not required.

## Generation

Meshing is a backend (default: Gmsh-class surface + volume; optional tetgen-class tet fill). The UI talks to a mesher interface: geometry IDs + size fields + algorithm options → mesh.

**Surface mesh**

- Triangle
- Quad-dominant
- Structured quad on transfinite faces when topology allows
- Algorithm choice: frontal, Delaunay, MeshAdapt-class, automatic
- Recombination for quads (simple / blossom / full-quad)

**Volume mesh**

- Tetrahedra from closed surface or from CAD solids
- Optional hex-dominant / hex later (P2)
- CFD volume + boundary layer: P2 (size field + layers API reserved)

**Sizing**

- Global min/max size, growth rate
- Local size on selected faces, edges, vertices, or a box/sphere region
- From geometry curvature (P1)

**Association**

After a CAD mesh, rebuild geometry–mesh maps and, if requested, create mesh sets from geometry groups.

Long jobs: progress in console; cancel; UI not frozen.

## Sets

Named mesh sets:

- Node set
- Element set
- Face set (skin / boundary faces)
- Edge set

Create from selection, from geometry group, or from a spatial box. Merge and subtract sets. Visible color in the viewport.

Sets are what materials, properties, and BCs attach to. Export writes them as INP elset/nset, CGNS families, Gmsh physical groups, etc.

## Quality

Compute cell quality into a scalar field and a summary table:

Required metrics (show those valid for the cell types present): aspect ratio, min/max angle, scaled Jacobian (or Jacobian), skew, warpage (quads), volume/area (sanity).

Optional extra metrics may be added; the UI lists available measures rather than dumping every academic name by default.

Histogram or binned counts. Clicking a bad bin can highlight those cells (P1). Filter/delete cells below a threshold is a separate mesh-edit command (P1).

## Mesh edit

- Create face from selected nodes (tri/quad)
- Delete faces/elements
- Merge coincident nodes (tolerance)
- Remove unused nodes
- Transform mesh (translate/rotate) without CAD
- Delete a mesh while keeping geometry

## Display properties

Node/element counts by type, bounding box, quality summary if computed.

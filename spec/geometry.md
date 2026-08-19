# Geometry

## Import / export

| Format | Role | Priority |
| --- | --- | --- |
| STEP (`.step`, `.stp`) | Primary CAD exchange, including assemblies | P0 |
| Open CASCADE BRep (`.brep`) | Native BRep | P0 |
| IGES (`.igs`, `.iges`) | Legacy CAD | P1 |
| STL (`.stl`) | Tessellation; treated as mesh, not BRep | P0 (already mesh) |

Export STEP and BRep of the current geometry. STL export is a mesh export.

Import runs off the UI thread. Failures go to the console with the file name.

## Create — points, curves, faces

- Point by coordinates
- Line by two coordinates or two existing vertices
- Rectangle on XY / YZ / XZ
- Disk or sector (center, radius, plane, sweep angle)
- Face from a closed loop of edges (planar when possible; fill otherwise)

## Create — solids

- Box (origin + sizes)
- Cylinder (center, axis, radius, height, sweep angle)
- Cone / frustum (two radii, height, axis, sweep)
- Sphere or spherical sector (center, radius, axis, latitude, sweep)

Creation target: append to current body, new body in current part, or new part — user choice.

## Sketch (P1)

2D sketch on a datum or planar face: line, polyline, rectangle, circle, arc, spline. Finish sketch → profile used by extrude/revolve. Full constraint solver is not required in the first sketch drop; robust profiles are.

Datum plane: create from three points or offset from a face.

## Operations

| Operation | Notes |
| --- | --- |
| Boolean union / cut / intersection | Multi-body |
| Extrude | Face or sketch; keep or consume source as specified |
| Revolve | Face or sketch about an axis |
| Loft | Through sections (P1) |
| Sweep | Profile along a path (P1) |
| Move / rotate | By vector or axis-angle |
| Mirror | Plane |
| Pattern | Linear / circular array (P1) |
| Split | Plane or tool body |
| Fillet / chamfer | Constant radius/distance |
| Variable fillet | P2 |
| Fill hole | Select hole edges/faces |
| Remove face | Defeaturing |
| Fill gap | Sew nearby faces (P1) |
| Delete | Top-level shape; option to keep child topology if shared |

Each operation is one undo step.

## Geometry groups

Named geometry component: a labeled set of vertices, edges, faces, or bodies. Used for mesh sizing, later BCs, and export physical groups. Create from current selection.

## Properties shown for geometry

Name, type, bounding box, surface area, volume (solids), face/edge counts, center of mass. No debug kernel dumps.

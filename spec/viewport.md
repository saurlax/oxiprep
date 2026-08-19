# Viewport and selection

## Camera

Orbit, pan, zoom. Fit all / fit selection. Perspective and orthographic. A small orientation triad.

## Pick modes

Exactly one pick mode at a time, on the viewport bar (default Body):

**Geometry:** vertex, edge, face, body.

**Mesh:** node, edge, cell.

**Off:** navigation only.

Click selects the closest entity under the cursor. Drag still orbits. A miss with no modifier clears. Shift adds; Command (Ctrl on Windows/Linux) toggles. Box / rubber-band select is later.

CAD face and edge use kernel shape IDs. CAD vertices use unique edge endpoints. Mesh-only bodies map Face to a cell and Edge to a node pair.

Selection is the input to measure, groups, BCs, and mesh sizing.

## Display

Independent toggles, also on the viewport bar:

- **Faces:** shaded triangles (CAD display tessellation, or the discrete mesh).
- **Edges:** CAD feature edges. If a body has none (imported mesh), triangle edges are used unless Mesh is already on.
- **Mesh:** analysis-mesh or imported-mesh triangle edges. CAD tessellation is not drawn as a mesh.
- **Vertices:** CAD vertices, or mesh nodes when the body has a discrete mesh.

## Volume / clip

Clip with an axis-aligned plane through the model bounds so the interior is visible. Axis, offset, and flip. This is a view filter, not a geometry split. An interactive free plane for volume meshes can come later.

## Highlight and attributes

Selected entities highlight. Mesh (and later results) can color by a named scalar or vector field. Scalar legend in a side panel. Quality metrics write a cell scalar and reuse this path.

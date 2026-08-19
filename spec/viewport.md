# Viewport and selection

## Camera

Orbit, pan, zoom. Fit all / fit selection. Perspective and orthographic. A small orientation triad.

## Pick modes

Exactly one pick mode at a time:

**Geometry:** vertex, edge, face, body.

**Mesh:** node, edge, face, element (cell).

**Off:** navigation only.

Box / rubber-band select for mesh nodes and cells. Click-add and click-remove (modifier) for multi-select. Selection is the input to measure, groups, BCs, and mesh sizing.

Picking must return stable IDs, not only display triangles.

## Display

Independent toggles, also on the viewport bar:

- **Faces:** shaded triangles.
- **Edges:** CAD feature edges. If a body has none (STL), triangle edges are used unless Mesh is already on.
- **Mesh:** tessellation / mesh triangle edges.
- **Vertices:** node markers.

## Volume / clip

Clip with an axis-aligned plane through the model bounds so the interior is visible. Axis, offset, and flip. This is a view filter, not a geometry split. An interactive free plane for volume meshes can come later.

## Highlight and attributes

Selected entities highlight. Mesh (and later results) can color by a named scalar or vector field. Scalar legend in a side panel. Quality metrics write a cell scalar and reuse this path.

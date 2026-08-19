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

## Volume mesh display

Clip the volume with an interactive plane so interior elements are visible. Optional: show only one side of the plane. This is a view filter, not a geometry split.

## Highlight and attributes

Selected entities highlight. Mesh (and later results) can color by a named scalar or vector field. Scalar legend in a side panel. Quality metrics write a cell scalar and reuse this path.

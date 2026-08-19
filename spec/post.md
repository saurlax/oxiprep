# Postprocessing (later)

Reuse mesh display and attribute coloring.

Minimum result pipeline:

- Open VTK / CGNS / (optional Tecplot) on a mesh or standalone
- Display: points, wireframe, surface, surface+edges
- Clip, slice
- Isosurface / isoline
- Vector glyphs
- Streamlines
- Calculator (derived fields)
- Reflection (symmetry viz)
- Time steps: play, save animation
- 2D plot window (probe / history)
- Save image / video

Results live under the project’s Results node and may be transient (not all dumped into the project file).

Do not invent a second viewport stack; post windows are extra tabs or extra field layers on the same renderer.

Do not add a Post menu until these commands exist.

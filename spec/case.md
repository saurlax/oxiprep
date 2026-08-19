# Case setup

Preprocessor, after mesh. Not a solver. The case is solver-neutral data that exporters turn into decks.

## Materials

Library of materials with typed properties (density, elastic, thermal, fluid — groups appear as they are used). Assign material to a geometry body or an element set. Load/save material library (P1).

## Element properties / sections

Shell thickness, beam section, solid element type — attached to sets. Minimal solid-continuum default is enough for v1 export.

## Boundary conditions and loads

Attach to a geometry group or mesh set. Built-in types (subset enabled by physics):

**Structural:** fixed support, displacement, pressure, concentrated force (P1), acceleration.

**Thermal:** temperature, heat flux (P1).

**Flow:** inlet, outlet, wall, symmetry.

**Generic:** user-defined name + scalar/vector fields for custom exporters.

A physics model object (structural / thermal / CFD / coupled) selects which BC types are offered. Custom physics via exporter plugins later, without renaming the core types.

## Export case

Template or native writers: Abaqus INP (mesh + sets + materials + BCs as far as supported), CGNS boundary families, Gmsh physical groups. Unknown solver decks are a later solve + plugin concern.

# D009 - The window matches Orbistoun's toolkit

**Status:** decided
**Date:** 2026-09-26

`pros-gui` uses the same `eframe` and `egui` versions and the same `wgpu` backend as Orbistoun's
window, pinned once in the workspace manifest.

**Why:** the two windows do the same kind of work over the same library and share `oops-docs`, so
a shared GUI crate stays a move rather than a port. A lossless frame grab also needs a GPU surface
to be displayed on.

**Rejected:**
- The current toolkit release: diverges from the sibling and from `oops-docs`.
- A different backend: a second rendering path for the same kind of window.

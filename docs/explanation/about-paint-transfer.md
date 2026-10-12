# About keeping paint when UVs change

Crate stores paint as texels in UV space (see [About UDIM tiles and paint
storage](about-tile-storage.md)). That makes painting, display and export fast, but it ties paint
to the UV layout: if a reimported mesh has different UVs, the same texels now sit somewhere else
on the model. Reimporting with new UVs is common (an island is repacked, a seam moves, a part
gets more texel density), so Crate has to move paint along.

## Two ways to do it

**Replaying strokes.** Record every stroke (its dabs on the surface, the brush, the camera, the
stencils) and paint them all again onto the new UVs. Substance Painter works roughly this way.
It is exact for paint made in the project, but every paint operation has to be recorded and
replayable forever: the replay must use exactly the pipeline the stroke was painted with, so any
later change to the brush engine changes what old strokes produce, and the log must follow undo,
layer duplication and grouping without a single gap. Paint that never came from a stroke (smart
materials, pasted layers) can't be replayed at all.

**Transferring texels.** Keep the paint as it is and move it across: for every texel of the new
UVs, find the same point of the surface on the old mesh and copy the paint found there. Mari's
Transfer and Blender's "Selected to Active" bake work this way. It covers every kind of paint,
whatever made it, and costs one pass per reimport.

Crate transfers. The cost is that the result is a resampling, not a repaint:

- Each new texel takes the nearest old texel, so paint can look slightly softer or more
  pixelated where the new UVs have a very different texel density than the old ones.
- Where the old UVs overlapped (mirrored halves sharing an island), both halves had the same
  paint already, and still do.
- The surface has to stay where it was. Paint is found within a short distance of the new
  surface (1 % of the mesh's size); where the shape changed more, there's nothing to copy and
  the texels stay unpainted. The reimport reports the share of the new UV area where no old
  surface was found.

## When it happens

Only when the UVs changed. If every UV of the new mesh is the same as the old one, the paint
texels are already right and are kept untouched, as before. The command line's `--no-transfer`
keeps the old behavior for meshes with new UVs too: the texels stay where they are in UV space.

A reimport replaces the paint pools, so the undo history, which refers to their texels, is
cleared.

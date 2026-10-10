# About view transforms

A renderer computes light in scene-linear values that can be far brighter than a screen can
show. A view transform decides how those values become display values: how highlights roll
off, how bright colors desaturate, how much contrast midtones get. The same painted material
can look quite different under two transforms, so Crate offers the ones other applications use,
and matches them closely. A texture judged under AgX in Crate should look the same under AgX in
Blender.

## Where each transform comes from

**Standard, Khronos PBR Neutral and Reinhard** are short published formulas, computed exactly
in the shader. Blender approximates PBR Neutral with a lookup table, which is why Crate and
Blender differ slightly there.

**ACES 1.3 and ACES 2.0** are large reference transforms; ACES 2.0 is a full color appearance
model. Porting them by hand would risk subtle errors, so Crate bakes them from the ACES
configurations that ship with OpenColorIO (BSD-3-Clause); Blender's ACES views give identical
results. Each becomes a 65 × 65 × 65 lookup table over log-encoded input, read with tetrahedral
interpolation, as OpenColorIO does. The baked tables differ from OpenColorIO's
exact result by more than 1 8-bit level in about 0.1 % of colors.

**AgX** is the default in Blender since 4.0. Blender's version is defined by a lookup table
whose license isn't stated, so Crate doesn't include it. Instead Crate uses its own small model
of the same shape (an inset matrix, a log encoding, one tone curve per channel and an outset
matrix) whose constants were fitted to Blender's AgX output, much as the widely used real-time
ACES approximation was fitted to the ACES reference. It matches within half a level on average
and within 2 levels on grays. The difference shows in very saturated colors, where Blender's
AgX removes more of the weakest channel than this model can, so Crate's are slightly less
saturated. The commonly copied real-time AgX formula was tried first; it differs from Blender's
AgX by up to 48 levels, even 14 on grays, because it models an earlier AgX.

`tools/view_transforms.py` rebuilds the tables and the AgX fit and prints how far each
transform is from OpenColorIO and Blender.

## Why AgX is the default

AgX handles very bright, saturated light gracefully: it fades toward white instead of clipping
into a flat, overly saturated patch, which matters for emissive paint and bright HDRIs. It's
also what Blender users see by default. **Khronos PBR Neutral** is the better choice for judging
base colors against reference photos or product colors, because it leaves values below about
0.76 almost untouched. **Standard** shows values as they are, clipped.

## What a view transform doesn't change

Painted values, exported textures and baked mesh maps are scene data and never pass through the
view transform. Only the viewport image does. The working space (linear Rec.709) and the display
(sRGB) are fixed for now; per-texture color spaces and other displays are later steps.

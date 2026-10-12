# Bake exclusion

Meshes often reuse UV space: twenty copies of a bolt, four tiles or two planks laid on the same
UV island, so they share one texture. A texel can hold only one surface, so where islands are
stacked, the [mesh maps](mesh-maps.md) would be baked from whichever copy happens to be drawn
last. **Bake exclusion** marks low-poly triangles that baking leaves out, so stacked texels are
baked from the copy you choose: the **bake source**.

Exclusion belongs to the geometry, not to texels: stacked copies share their texels, so only the
triangles can tell them apart.

## What exclusion changes

| | Excluded triangles |
|---|---|
| Texels | They don't write texels. Each texel of a stacked island comes from the included triangles over it. Where no included triangle covers a texel, it counts as space between islands: it takes the nearest island texel, as [padding](mesh-maps.md#padding) does. |
| Rays (Ambient Occlusion, Curvature, Thickness) | No rays start from them, as they have no texels. They still block the rays of the rest of the mesh: an excluded bolt still shadows the plank it sits on. |
| High poly | Unchanged. All of it is still there to be found by rays and to block them. |
| Position and World Normal | Baked again at once, without the excluded triangles. |
| Other baked maps | Become **Out of date** (see [Bake status](mesh-maps.md#bake-status)). |
| Display | Unchanged: an excluded copy still shows the texture it shares. |
| Painting | Paint lands on texels through the surface they were baked from. Projection painting reaches stacked texels through the bake source, and every copy shows the result. An excluded copy that shares no texels with an included one can't be painted by projection. [Geo Fill](brushes-tools-stencils.md#geo-fill) works on any geometry. |

## Bake Exclude tool

**Bake Exclude**: `X`, or the camera icon in the tool rail. It works in the 3D view and the UV
view. The viewport toolbar shows:

| Control | Description |
|---|---|
| Select | Click, Brush or Box, as for [Geo Fill](brushes-tools-stencils.md#geo-fill). With Brush, **Size** follows. |
| Fill by | Face, UV island, Part, Material or UDIM tile: what one pick selects. |
| **Exclude** | Excludes the selected triangles. |
| **Include** | Includes them again. A stack's bake source that is excluded stops being its source. |
| **Make bake source** | For every [stack](#stacks) with a selected island: makes that island the stack's bake source (see [Make bake source](#make-bake-source)). |
| Count | **N excluded**: the number of excluded triangles. |

| Input | Selection |
|---|---|
| Click, Brush stroke or Box | Replaces the selection with what it picks. |
| `Shift` + click, stroke or Box | Adds to it. |
| Click on nothing | Clears it. |
| `Esc` | Clears it. |

Each Exclude, Include and Make bake source is one undo step. The selection is not saved and not
part of undo.

### Display

While Bake Exclude is the current tool, both views show:

| Item | Shown as |
|---|---|
| Stacks | Each stack's islands tinted in a color of its own. |
| Bake source | Its island outlined in its stack's color. |
| Excluded triangles not in a stack | Darkened. |
| Selection | Tinted white. |
| Fill by outlines | As for [Geo Fill](brushes-tools-stencils.md#outlines-and-highlight). |

The mesh hides them where it covers them, as for Geo Fill.

## Stacks

Two UV islands are **stacked** when at least half of the smaller one's UV area lies on the
other, measured on a grid of 512 cells per UV unit. A **stack** is a group of islands joined by
being stacked, directly or through others. Islands that only touch or overlap a little aren't
stacks: overlapping UVs by mistake are reported by the [Import Log](mesh-and-udims.md#import-diagnostics)
as before.

Stack colors are well-separated hues, given in the order of each stack's first triangle.

## Make bake source

For each stack that has a selected island, the first selected island of that stack becomes its
bake source:

- the source is included,
- every other island of the stack is excluded,
- the source keeps its outline until it is excluded, or another island of the stack is made the
  source.

If no selected island is in a stack, the status bar shows **Select an island that is stacked
with others.** and nothing changes.

## Mesh Maps window

The **Mesh Maps** window lists, below the maps: **Excluded from baking: N triangles, S bake
sources**, and **Clear**, which includes every triangle and clears every bake source (one undo
step).

## Saving and reimport

Exclusion is saved in `project.json` (see [Project format](project-format.md#projectjson)).
After a [reimport](mesh-and-udims.md#reimport), it is kept if the new mesh has the same
triangles with the same UVs; otherwise it is cleared, and the reimport report adds
**; bake exclusion cleared**.

See [How to bake stacked UVs once](../how-to/bake-stacked-uvs-once.md) for the steps.

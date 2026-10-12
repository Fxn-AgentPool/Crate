# Paint buffer

The paint buffer is a multi-channel 2D canvas aligned with the viewport. All brush strokes are
drawn into the buffer. **Baking** projects the buffer's contents onto the selected Paint or Paint
Mask node, through the camera, into UV space.

The [Geo Fill](brushes-tools-stencils.md#geo-fill) and [Curve](curves.md) tools don't use the
buffer.

## Buffer properties

**Paint Buffer** panel:

| Property | Values | Default | Description |
|---|---|---|---|
| Resolution scale | 0.5×, 1×, 2× | 1× | Buffer resolution relative to the viewport. |
| Bake mode | Live, Manual | Live | See "Bake modes". |
| On camera move | Bake, Clear, Lock camera | Bake | Manual mode only. What happens when the camera moves with unbaked paint. |
| Show buffer | on/off | on | Displays unbaked paint over the viewport. |

The buffer holds one value plus coverage for every channel the target node paints.

## Bake modes

| Mode | Behavior |
|---|---|
| Live | After each brush dab, the dirty rectangle of the buffer is baked and cleared. Paint appears on the textures immediately. One undo step per stroke. |
| Manual | Paint stays in the buffer until **Bake** (`Ctrl+B`). The buffer can be transformed before baking. One undo step per bake. |

Toggle with `Ctrl+Shift+B`. Switching from Manual to Live bakes any unbaked paint first.

In Manual mode, unbaked paint is indicated by an orange viewport border and **Unbaked** in the
status bar.

## Buffer transform

Manual mode only. Transforms move the buffer's contents relative to the viewport.

| Action | Input |
|---|---|
| Move | Hold `V`, left-drag |
| Rotate | Hold `V`, `Shift`+left-drag |
| Scale | Hold `V`, `Ctrl`+left-drag |
| Reset transform | `V`, then `R` |
| Clear buffer | `Ctrl+Delete` |

## Projection parameters

**Paint Buffer → Projection**:

| Parameter | Range | Default | Description |
|---|---|---|---|
| Backface culling | on/off | on | Texels on surfaces facing away from the camera are not painted. |
| Falloff start | 0–90° | 60° | Angle between surface normal and view direction where paint strength starts to fall off. |
| Falloff end | 0–90° | 85° | Angle at which paint strength reaches 0. |
| Depth tolerance | ≥ 0 | 0.001 | Relative depth bias for occlusion. Texels hidden behind other geometry are not painted. |
| Project through | on/off | off | Ignores occlusion and paints all surfaces under the buffer, including hidden ones. |

## Bake algorithm

For each tile touched by the buffer region, and for each texel in it:

1. Read the texel's object-space position and normal from the Position and World Normal maps.
2. Project the position into the bake camera's screen space. Skip if outside the buffer region.
3. Unless **Project through** is on, compare with the bake camera's depth pass. Skip if occluded.
4. Compute the strength from the facing angle and the falloff parameters.
5. Sample the buffer (bilinear) and blend into the target node's pages with
   `strength × buffer coverage`.
6. Texels in the mesh maps' padding (8 texels by default) carry the position of their nearest
   covered texel, so the bake paints them too and seams stay hidden.

Baking requires the Position and World Normal mesh maps. If they are missing, they are baked
automatically first.

A buffer painted in the [UV view](uv-view.md#painting) is baked the same way, except for steps
2 to 4: each covered texel's place in the buffer comes from its UV and the UV view's pan and
zoom, there is no depth test, and the strength is 1.

# Curves

A **curve** is a brush stroke along a smooth path through points on the surface. It is drawn with
the **Curve** tool and stays editable: its points and its brush settings can be changed at any
time, and the layer is redrawn.

Curves are held by Paint layers, 2D paint layers, paint mask effects and 2D paint mask effects.
A layer can hold many curves, next to its ordinary (freehand) paint.

## Drawing

**Curve** tool: `K`, or the curve icon in the tool rail. It works in the 3D view. In the UV view
the cursor shows ⊘ and clicks do nothing.

| Input | While drawing a curve | Otherwise |
|---|---|---|
| Click on the mesh | Adds a point at the end of the curve. | Starts a new curve with its first point. |
| Click the curve's first point | With 3 or more points: closes the curve and finishes it. | — |
| `Enter` or `Esc` | Finishes the curve. | — |
| `Backspace` | Removes the last point. | — |

Clicks that miss the mesh do nothing. Choosing another tool, layer or mask finishes the curve.
Finishing a curve with a single point deletes it.

A new curve takes its settings from the current [brush](brushes-tools-stencils.md#brush-properties)
and [brush values](brushes-tools-stencils.md#brush-values), and is named **Curve N**, with N one
more than the highest number in the layer's curve names. Its Size is converted to world units:

| Brush Size space | Curve Size |
|---|---|
| World | Brush Size × 0.01, as World brushes measure it. |
| Screen | The brush's width in pixels, measured at the depth of the curve's first point. |

## Editing

With the Curve tool, while no curve is being drawn:

| Input | Effect |
|---|---|
| Click a curve | Selects it. A curve is hit within 6 points of its line on screen. |
| Drag a point of the selected curve | Moves the point. It stays on the surface under the pointer. |
| `Ctrl` + click the selected curve's line | Inserts a point there. |
| `Ctrl` + click a point of the selected curve | Deletes the point. Deleting one of a curve's last two points deletes the curve. |
| Click the mesh away from any curve | Deselects, and starts a new curve. |
| `Delete` | Deletes the selected curve. With no curve selected, or another tool current, `Delete` deletes the layer as usual. |

Every edit is one undo step; a point drag is one step.

## Display

While the Curve tool is current, the 3D view draws the target's curves over the mesh:

| Item | Shown as |
|---|---|
| Curves | Thin white lines along their paths. |
| Selected curve | An accent-colored line, with its points as small squares. The first point is larger. |
| Curve being drawn | As the selected curve, with a line from its last point to the pointer. |

Lines are drawn over the mesh, including where it hides them.

## Shape

The path is a centripetal Catmull–Rom spline through the points, in object space: it passes
through every point, and its ends follow the first and last segments. A closed curve continues
from its last point back to its first.

The path is then pulled onto the surface: along it, samples about a fifth of the curve's Size
apart are each moved to the nearest surface point whose triangle faces the same way as the
curve there (the interpolated normal of the neighboring points, within 90°). Where two
neighboring samples land more than two steps apart, as on two sides of an edge the spline cuts
through, their midpoint is pulled onto the surface too, and so on until the gaps close. So between
widely spaced points the path follows the surface, over edges, instead of cutting through the
mesh.

Points are stored as positions on the surface. When the surface under them changes (after a
[reimport](mesh-and-udims.md#reimport)), they are pulled onto the new surface in the same way.

## Painting

A curve paints like a brush stroke along its path, but in object space instead of through the
camera. Dabs are placed along the path every **Spacing** percent of **Size**. Each dab is a disc
lying on the surface:

- The tip (round, or the alpha) is oriented along the path, then turned by **Angle**.
- A texel is painted when its surface point is within the disc's radius measured in the disc's
  plane, and within one radius above or below that plane.
- Its strength fades out as the texel's surface turns away from the dab's: full where their
  normals are within 70° of each other, none from 90°. Thin walls aren't painted through.
- There is no occlusion and no angle falloff from a view: a curve looks the same from every
  direction.

Coverage builds up along the curve like a stroke: each dab adds Flow, and Opacity caps the whole
curve. Jitter varies size, angle, position (along the surface) and flow per dab, the same way
every time the curve is drawn.

Curves reach the same texels as ordinary paint: UV islands and their
[padding](mesh-maps.md#padding), also on 2D paint layers.

## Order

A layer's curves are drawn over its freehand paint, in list order: the first curve at the bottom.
Together they make up the layer's content, as blend modes, masks, caches and export see it.

Freehand painting and the Erase tool change only the freehand paint. Curves stay above it until
they are [rasterized](#rasterize).

## Curves in the Properties panel

A Paint or 2D paint layer, or a paint or 2D paint mask effect, shows a **Curves** section in the
[Properties panel](layers.md#properties-panel):

| Part | Contents |
|---|---|
| List | One row per curve, the top row drawn on top. Click selects (as clicking it in the viewport does); double-click renames. |
| Header buttons | **Move up**, **Move down**, **Delete** for the selected curve. |
| Settings | The selected curve's properties (below), its **Values**, and **Copy brush**, which sets them all from the current brush and brush values. |
| **Rasterize** | Merges every curve of the layer into its freehand paint and removes them (see [Rasterize](#rasterize)). |

With no curves, the section shows a hint to draw one with the Curve tool (`K`).

### Curve properties

| Property | Range | Description |
|---|---|---|
| Name | text | **Curve N**. |
| Size | > 0, world units | Width of a dab. |
| Hardness | 0–1 | Edge falloff of the round tip. Ignored when an alpha is set. |
| Flow | 0–1 | Coverage added per dab. |
| Opacity | 0–1 | Maximum coverage of the curve. |
| Spacing | 1–200 % | Distance between dabs, as a percentage of Size. |
| Alpha | image | Tip shape, as for the brush. |
| Angle | 0–360° | Rotation of the tip from the path's direction. |
| Jitter: size / angle / position / flow | 0–1 | Random variation per dab. |
| Closed | on/off | The path continues from the last point back to the first. Needs 3 or more points. |
| Values | per channel | As [Brush values](brushes-tools-stencils.md#brush-values): which channels the curve paints, and with what. A mask target uses **Mask value** instead. |

The brush's Size space, Follow stroke, Pixel and Pressure settings don't apply to curves.

## Rasterize

**Rasterize** (Properties panel; `rasterize-curves` in [scripts](script-commands.md)) writes the
layer's curves into its freehand paint, exactly as they look, then removes them. It is one undo
step.

## Saving and copying

| Operation | Curves |
|---|---|
| Save | Stored in `project.json`, on the node (see [Project format](project-format.md#curves)). Not stored as pages. |
| Duplicate | Copied with the layer. |
| Smart material | Saved with the layer, with the alpha images they use. On another mesh, the points are pulled onto its surface. |
| Reimport | Kept as they are, and drawn on the new surface. Unlike freehand paint, they aren't [transferred](mesh-and-udims.md#reimport). |

## Limits

| Item | Limit |
|---|---|
| Curves per layer | 256 |
| Points per curve | 1000 |
| Dabs per curve | 16 384. A curve that would need more is drawn with its spacing widened to fit. |
| GPU memory | A layer with curves holds a second set of full tiles for them: 40 bytes per texel, or 4 for a mask effect. |

See [About curves and geometry fills](../explanation/about-curves-and-geometry-fills.md) for why
curves work this way.

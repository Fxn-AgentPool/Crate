# How to paint in manual bake mode

This guide shows you how to paint into the buffer, adjust the paint, and bake it when you choose.

## Switch to Manual mode

Press `Ctrl+Shift+B`, or set **Paint Buffer → Bake mode** to **Manual**. The status bar shows
**Manual**.

Choose what happens if you move the camera with unbaked paint, under **On camera move**:

- To keep the paint, choose **Bake** (default).
- To throw it away, choose **Clear**.
- To prevent accidental camera moves, choose **Lock camera**.

## Paint and adjust

1. Select a Paint layer.
2. Paint. The viewport border turns orange while there is unbaked paint.
3. To reposition the paint, hold `V` and drag. Add `Shift` to rotate or `Ctrl` to scale.
4. To start over, press `Ctrl+Delete`.

## Bake

Press `Ctrl+B`. The paint is projected onto the layer, and the buffer clears.

- **If paint doesn't reach surfaces facing away at an angle**, raise **Falloff end** under
  **Paint Buffer → Projection**, or turn the camera to face them.
- **If paint should go through to hidden surfaces**, turn on **Project through** before baking.
- **If paint leaks onto geometry just behind the surface**, lower **Depth tolerance**.

## Return to Live mode

Press `Ctrl+Shift+B`. Any unbaked paint is baked first.

See [Paint buffer](../reference/paint-buffer.md) for all properties.

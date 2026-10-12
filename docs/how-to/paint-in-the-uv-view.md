# How to paint in the UV view

This guide shows you how to paint directly on the UV layout, for example to paint straight lines
along an island, or to reach areas that are hard to see in 3D.

1. Select a paint layer or paint mask in the Layers panel.
2. Press `F1` (or click **Split** in the viewport toolbar). The 3D view moves to the left half and
   the UV view opens on the right.
3. In the UV view, press `F` to frame the used tiles. Zoom with the mouse wheel and pan with the
   middle mouse button.
4. Paint with the brush as in 3D. The stroke appears on the model in the left half as you paint.
5. To paint in the UV view only, press `F3`; `F2` goes back to 3D only.

Painting only reaches the UV islands and their padding: the empty space between islands doesn't
take paint. See [UV view](../reference/uv-view.md#painting) for what else works differently, such
as angle falloff not applying.

## Related

- [UV view](../reference/uv-view.md) (reference)
- [About the paint buffer](../explanation/about-the-paint-buffer.md#painting-in-uv-space)

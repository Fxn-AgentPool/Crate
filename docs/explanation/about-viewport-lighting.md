# About viewport lighting

The viewport's job is to show how a material will look in other renderers: game engines,
Blender, other texturing tools. So it follows the same conventions as they do: the GGX
microfacet model, perceptual roughness and image-based lighting. The settings are listed in
[Viewport lighting](../reference/viewport-lighting.md).

## Why reflections are prefiltered

A rough surface reflects a blurred image of its environment, and the shape of the blur
matters. GGX spreads a bright light into a soft core with long tails. Blurring the environment
any other way, for example by averaging blocks of texels, keeps small lights compact and
sharp-edged. Roughness 0.5 then looks glossier than it does in Blender or Substance 3D Painter.

Integrating the GGX lobe for every pixel, every frame, is too slow. Like most realtime
renderers, Crate splits the work (the "split-sum" approximation). When an environment loads, it
blurs it once with the GGX lobe of several roughness values. While drawing, it reads the blur
level for each pixel's roughness and scales it by a precomputed term for the Fresnel effect and
shadowing. The approximation assumes the view direction equals the surface normal, so stretched
reflections at grazing angles are lost. That is the usual trade-off, and other realtime
viewports make it too.

## Why ambient occlusion is a viewport setting

Image-based lighting comes from every direction, including directions the mesh blocks, so
crevices and contact areas are lit as brightly as open surfaces. Other texturing tools hide this
with the baked ambient occlusion map. Without it, Crate's viewport looks flatter and brighter in
recesses, and a material tuned in Crate looks different elsewhere.

Occlusion is applied to lighting, not baked into the base color, because game engines and
renderers apply it themselves: from the exported `ao` map, from screen-space occlusion or from
ray tracing. Baking it into the color too would darken those areas twice. It is a toggle because
whether the target renderer occludes, and how much, varies. Turning it off shows the material
the way a renderer without occlusion would.

Reflections are occluded more gently than diffuse light. A reflection comes from a narrow cone
around the mirror direction, which on a smooth surface seen head-on usually points out of the
crevice. The specular occlusion formula (from Lagarde and de Rousiers, *Moving Frostbite to
PBR*) estimates how much of that cone the AO cone covers, from AO, the viewing angle and
roughness.

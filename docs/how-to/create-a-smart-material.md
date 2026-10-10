# How to create a smart material

This guide shows you how to save a group of layers as a reusable smart material and apply it to
other projects.

## Prepare the group

1. Select the layers that make up the material (`Ctrl`+click to select several) and press
   `Ctrl+G` to group them.
2. For each value people should be able to adjust, such as base color or wear amount, open the
   layer or mask with `G` and expose the parameter (pin icon in **Properties**).
3. Rename the group to the material's name.

Paint layers inside the group are saved with their paint. This only makes sense for paint that
doesn't depend on a particular mesh's UVs; prefer generators and fills for reusable materials.

## Save it

Frame the asset the way you want the thumbnail to look, then right-click the group and choose
**Save as smart material**. It appears in the **Library** section of the left panel under
**Smart Materials**, with the current viewport as its thumbnail.

## Apply it

Drag the smart material from the Library onto the Layers panel, or double-click it. It's
inserted as one layer above the selected layer, with its exposed parameters listed.

- To change what's inside one instance, select it and press `G`. Edits affect only this instance.
- If the material relies on mesh maps, bake them in the new project first. See
  [How to bake mesh maps](bake-mesh-maps.md).

## Share it

Smart materials are stored as `.smat` files in the user library directory, under
`library/smart-materials` in the user configuration directory
(**Library → Smart Materials → Show in file browser**). Copy the file to the same folder on
another machine and click **Refresh**.

# How to preview and rewire nodes quickly

This guide shows you how to inspect any node's output in the viewport and restructure a graph
with the Node Editor's Node Wrangler-style operations. It assumes you can already
[edit a layer as a graph](edit-a-layer-as-a-graph.md).

## See what a node outputs

1. `Ctrl+Shift` + click the node. The viewport shows its first output, unlit, and the node's
   output socket gets a green ring.
2. To see its next output, `Ctrl+Shift` + click it again.
3. To return to the material, `Ctrl+Shift` + click empty space, or click **Stop preview** in the
   Node Editor's header.

The preview only changes the viewport. Exports, saved projects and undo are unaffected, and
closing the Node Editor turns it off.

## Connect nodes without aiming at sockets

- `Alt` + right-drag from one node to another. Crate picks the best matching output and input,
  preferring the same type and a free input. The target turns green if a connection is possible.
- To choose the sockets yourself, `Shift+Alt` + right-drag, then pick a pair from the menu.
- To feed the active node into every other selected node, press `K`.

## Remove links

`Ctrl` + right-drag across the links you want to remove.

## Combine several results

1. Select the nodes to combine (box-select or `Shift` + click).
2. Press a merge shortcut, for example `Ctrl++` to add or `Ctrl+0` to mix. Selected materials are
   combined into a new Stack instead.

The result node is selected, ready to connect onward. See
[Node Editor](../reference/node-editor.md#merge) for which node each merge creates.

## Restructure

- **Swap**: select two nodes and press `Alt+S` to exchange everything they feed. With one node
  selected, `Alt+S` swaps its two inputs.
- **Dissolve**: `Ctrl+X` deletes the selected nodes and reconnects what fed them to what they fed.
- **Copy and paste**: `Ctrl+C` copies the selection, `Ctrl+V` pastes it at the pointer, also in
  another layer, project or Crate window. Links between the copied nodes are kept.
- **Duplicate**: right-click a node and choose **Duplicate** to copy the selection in place,
  keeping its input links.
- **Align**: `Shift+W`, `Shift+A`, `Shift+S` and `Shift+D` line up the top, left, bottom or right
  edges of the selection; `Shift+=` lines it up in a row or column.
- **Clean up**: `Alt+X` deletes every node that doesn't contribute to the material.

## Reuse settings

- `Shift+C` copies the active node's parameters to the other selected nodes of the same type.
- `Backspace` resets the selected nodes' parameters to their defaults.

All of these are also in the menu that opens when you right-click a node.

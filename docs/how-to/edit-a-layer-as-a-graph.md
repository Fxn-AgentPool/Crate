# How to edit a layer as a node graph

This guide shows you how to open a layer or mask in the Node Editor and build logic the Layers
panel can't express.

## Open the graph

1. Select the layer or mask in the Layers panel.
2. Press `G`. The Node Editor opens at the bottom of the window, showing the layer's content and
   mask connected to a **Layer** node.
3. To close the Node Editor, press `Esc` or `Ctrl+Space`.

To see the whole project graph instead, press `Ctrl+Space` with no layer selected, or click
**Whole graph** in the Node Editor's header. **Selected layer** switches back.

## Add and connect nodes

- To add a node, press `Tab` (or `Shift+A`, or right-click empty space) and type its name.
  `Enter` adds the first match.
- To connect, drag from an output socket to an input socket, or onto a node to use its first
  fitting input. Sockets of incompatible types refuse the connection; while you drag, they are
  dimmed. See the conversion rules in [Nodes](../reference/nodes.md#socket-types).
- To add a node that is already connected, drag from an output and release on empty space. The
  menu lists only nodes that can take the link.
- To add a layer to a Stack, connect a material to its **+ New layer** socket. To add a mask
  effect to a Mask Stack, connect to **+ New effect**.
- To use a baked mesh map, drag it from the **Library** (Mesh Maps) onto the Node Editor.
- To edit one channel of a material separately, insert **Material Split**, modify the channel, and
  rejoin with **Material Combine**.

## Select and arrange nodes

- Click a node to select it; `Shift` + click adds or removes it.
- Drag on empty space to box-select. `A` selects all, `Alt+A` none.
- Drag a selected node to move the whole selection.
- Pan with right-drag, `Alt` + left-drag or middle-drag; zoom with the wheel; `Home` frames all.

New nodes, including those added from the Layers panel, are placed next to the node that reads
them.

## Reuse one result in several places

Connect the same output to several inputs. For example, to drive both a mask and a color from one
noise, connect the Noise node to the mask's input and, through a **Ramp**, to the color.

The layer then appears as a **Graph layer** in the Layers panel.

## Expose parameters on the layer

1. Select a node in the graph.
2. In **Properties**, click the pin icon next to a parameter. It's marked **Exposed**.
3. Close the Node Editor. The exposed parameter appears on the layer in the Layers panel.

## Turn a generator into editable nodes

Select a generator node and click **Ungroup** in Properties. Its internal nodes replace it.

## Related

- [How to preview and rewire nodes quickly](preview-and-rewire-nodes.md)
- [Node Editor](../reference/node-editor.md) and [Nodes](../reference/nodes.md) (reference)

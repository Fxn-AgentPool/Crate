# How to edit a layer as a node graph

This guide shows you how to open a layer or mask in the Node Editor and build logic the Layers
panel can't express.

## Open the graph

1. Select the layer or mask in the Layers panel.
2. Press `G`. The Node Editor opens, showing the selected content or mask stack.
3. To return to the Layers panel, press `Esc`.

## Add and connect nodes

- To add a node, press `Tab` and type its name.
- To connect, drag from an output socket to an input socket. Sockets of incompatible types refuse
  the connection. See the conversion rules in [Nodes](../reference/nodes.md#socket-types).
- To edit one channel of a material separately, insert **Material Split**, modify the channel, and
  rejoin with **Material Combine**.

## Reuse one result in several places

Connect the same output to several inputs. For example, to drive both a mask and a color from one
noise, connect the Noise node to the mask's input and, through a **Ramp**, to the color.

The layer then appears as a **Graph layer** in the Layers panel.

## Expose parameters on the layer

1. Select a node in the graph.
2. In **Properties**, click the pin icon next to a parameter. It's marked **Exposed**.
3. Press `Esc`. The exposed parameter appears on the layer in the Layers panel.

## Turn a generator into editable nodes

Right-click a generator node and choose **Ungroup**. Its internal nodes replace it.

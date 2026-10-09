# About layers and nodes

Texturing tools have historically split into two camps. Layer-based tools such as Substance
Painter or Photoshop present a list, which is quick to read and quick to edit. Node-based tools
such as Mari, Substance Designer or Houdini present a graph, which can express things a list
cannot: reusing one result in several places, branching, and combining masks from many sources.

Painter doesn't make you choose, because the two are not actually different things.

## A layer stack is a graph drawn as a list

Compositing a stack of layers is a chain of merge operations:

```
base ─► merge(layer 1, mask 1) ─► merge(layer 2, mask 2) ─► … ─► output
```

Each step takes the result below, the layer's content, a mask, a blend mode and an opacity.
That is exactly what a Merge node does. A layer stack is therefore a particular *shape* of node
graph: linear, with one input per step.

Painter makes that shape a first-class node, the **Stack** node. The Layers panel is a view of a
Stack node and nothing more. There is no second data model to keep in sync. When you add a layer,
a slot is added to the Stack node; when you group layers, a nested Stack node is created.

## Why not show the whole graph as layers?

An arbitrary graph can't be drawn faithfully as a list. As soon as one result feeds two places,
or a mask is computed from three branches, any list representation either hides information or
duplicates it. Tools that try to do this end up with confusing "linked layer" or "instance"
concepts.

So the rule is asymmetric: **stacks are shown as layers; anything else is shown as one Graph
layer**. A Graph layer is an honest black box in the Layers panel. Its exposed parameters are
visible, and pressing `G` opens what's inside.

This keeps the Layers panel trustworthy. What you see in it is the whole truth about the stack,
even if some rows are summaries.

## Why layers are multi-channel

In Painter, as in Substance Painter, one layer carries every channel at once: color, roughness,
metallic, height and so on. Mari is traditionally organized per channel, with a separate stack or
graph per channel.

Per-channel organization is more flexible and suits film work, where channels are often painted
by different artists or from different sources. For most asset work, though, a scratch is
*one thing* that changes color, roughness and height together. With per-channel stacks, that one
scratch becomes three layers in three places that must be kept in sync by hand.

Painter uses a **Material** socket type to carry all channels down one wire. Per-channel control
is still available, through per-channel toggles and blend modes on each layer, and **Material
Split** and **Material Combine** in the graph, but it is the exception rather than the default.

## Where masks fit

A mask is itself a small stack, a Mask Stack node producing a single value. This is why a mask
can combine a generator, a hand-painted correction and a levels adjustment without opening the
node editor. It is the same idea applied one level down: the common case as a list, with the
graph available when the list isn't enough.

## Smart materials

A smart material is a saved Group or Graph layer with some parameters marked as exposed. Because
groups and graphs are the same underlying thing, a smart material built in the node editor and
one built from layers behave identically once placed in a stack.

## Related

- [Layers and masks](../reference/layers.md) and [Nodes](../reference/nodes.md) (reference)
- [How to edit a layer as a node graph](../how-to/edit-a-layer-as-a-graph.md)

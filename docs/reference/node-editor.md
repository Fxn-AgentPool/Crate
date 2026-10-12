# Node Editor

The Node Editor is a panel at the bottom of the window that shows and edits the node graph. For
the nodes themselves, see [Nodes](nodes.md).

## Opening and views

| Input | Result |
|---|---|
| `Ctrl+Space` | Opens the Node Editor on the selected layer, or on the whole project graph if nothing is selected. Closes it if it's open. |
| `G` | Opens the Node Editor on the selected layer or mask. |
| **Window → Node Editor** | Toggles the Node Editor. |
| `Esc` | Closes the Node Editor (or the open menu inside it). |

| View | Shows |
|---|---|
| Whole graph | Everything that feeds the Material Output, with each layer of a Stack shown as one [layer group](#layer-groups). Shown when no layer is selected, and when the viewed layer is deleted. |
| Selected layer | The selected layer's content and mask subgraphs, plus a **Layer** node whose **Content** and **Mask** inputs are the layer's. The layers of a Stack in it (a Group layer's layers) are shown as layer groups. Follows the selection in the Layers panel: selecting another layer shows that layer; clearing the selection switches to **Whole graph**. |

The header shows **NODE EDITOR**, then a segmented control with the two views, **Whole graph**
and **Selected layer**; the current view is lighter. Click the other one to switch. **Selected
layer** is disabled while no layer is selected. In the Selected layer view, the layer's name
follows the control.

The right end of the header shows the most used inputs as keycaps: `Tab` **Add**,
`Ctrl+Shift+Click` **Preview**, `Ctrl+RMB` **Cut links**. **?** **All shortcuts** lists every
mouse and key input of the editor; hover over it to see the list.

Nodes added in the editor stay visible until connected, even if nothing reads them yet. Nodes
added elsewhere (for example new layers) are placed to the left of the node that reads them, level
with the socket they feed, below any node in the way.

While the pointer is over the Node Editor, its keys take precedence over the application's
shortcuts. `Ctrl+Space`, undo, redo, save, new, open and export still work.

## Nodes

Nodes are 200 points wide. From top to bottom, a node shows:

| Part | Contents |
|---|---|
| Header | Filled with the node's [category color](nodes.md#categories). Collapse arrow (▾ expanded, ▸ collapsed), the node's title, and **cached** when the node is cached. |
| Outputs | One row per output socket, on the right edge. |
| Settings | The node's [settings](nodes.md#inputs-and-settings), edited in place (see below). Settings that depend on another one's value are shown only while they apply. |
| Inputs | One row per input socket, on the left edge. A [value input](nodes.md#inputs-and-settings) shows its field while nothing is connected to it. |

Every row is 26 points high at 100 % zoom, and its controls are the ones of the Properties panel
(see [Controls](windows-and-panels.md#controls)), 24 points high, scaled with the zoom. A Vector
setting, and an unconnected Vector value input, take four rows; everything else takes one.

The **Layer** node has no settings and can't be collapsed. Its header uses the Output color.

### Settings and values

| Kind | On the node |
|---|---|
| Number | A field with the name inside on the left and the value on the right. Drag sideways to change it, or click to type a value. Values are clamped to the parameter's range; a field whose range spans at most 2 shows how far into its range the value is as a blue fill. |
| Color | The name, and a swatch filling the right half of the row; click the swatch to pick a color (see [Color picker](windows-and-panels.md#color-picker)). |
| Vector | The name on a row of its own, then one number field per row for **X**, **Y** and **Z**. |
| On/off | A checkbox with the name. |
| Choice | A dropdown across the row, showing the current option. |
| Image | A button across the row with the image name, or **(none)**; click to choose a file. |
| Ramp stops | A bar showing the gradient; click it to edit the stops (position, color, **×** to remove, **+ Stop** to add, up to 8). |
| ID colors | A button showing how many IDs are selected; click it for the list of IDs, each with a checkbox. |

A value input's field takes the input's rows: a number field with the input's name inside, on
the socket's row; a color as the name and a swatch; for a Vector, the input's name on the
socket's row and the X, Y and Z fields on the three rows below. While a link is connected, the
field isn't shown and the input takes one row with its name. A number field's drag speed is a
500th of its range per point, or 0.01 per point for ranges wider than 2000.

Each change is one undo step; dragging a field is one step from press to release. Clicking a
parameter selects its node and makes it the active node, but doesn't move the node or start a
box selection.

Parameters scale with the zoom. Below 50 % zoom they show their values only and can't be
edited; zoom in to edit them. Rows partly covered by another node show their values only too.

### Collapsing

| Input | Result |
|---|---|
| Click the header arrow | Collapses or expands that node. |
| `H` | Collapses the selected nodes, or expands them if all of them are collapsed. |

A collapsed node shows only its header. Links to it attach to the middle of the header's left
edge (inputs) and right edge (outputs); unconnected sockets aren't shown. To connect to a
collapsed node, drop the link on it: it connects to its first fitting input. To drag a link out
of a collapsed node, or pick one up from its input end, expand it first.

Collapsing changes nothing but the node's size. It's saved with the project (see
[Project format](project-format.md#graph)) and is one undo step.

## Frames

A frame is a labeled box around some nodes, to keep related nodes together. It's drawn behind
the nodes: a translucent gray box with its label (**Frame** until renamed) in the top 26 points.
The box fits its nodes with 16 points around them and grows and shrinks as they move.

| Input | Result |
|---|---|
| `F` | Puts the selected nodes in a new frame and selects the frame. Nodes already in another frame move to the new one; frames left empty are deleted. Layer groups and the Layer node aren't framed. |
| `Alt+P` | Takes the selected nodes out of their frames. Frames left empty are deleted. |
| Left-click a frame (where no node is) | Selects the frame; `Shift` adds it to or removes it from the selection. |
| Left-drag a frame | Moves the frame with all its nodes, and the rest of the selection. |
| Box select | Selects the frames entirely inside the box, and the nodes it touches. |
| `Delete` or `Ctrl+X` with a frame selected | Deletes the frame; its nodes stay. |

The frame's label is edited in the [Properties sidebar](#properties-sidebar) while the frame is
the active item. Moving a node doesn't add it to a frame or take it out: use `F` and `Alt+P`.
A frame is shown while at least one of its nodes is, and fits the nodes shown. Every change to
frames is one undo step. Frames are saved with the project (see
[Project format](project-format.md#graph)), aren't copied with their nodes, and are dropped
when their last node is deleted.

## Reroutes

A reroute is a small node that passes its input on unchanged, to route links around other
nodes. It's drawn as a 32 × 18 point pill in the panel color with its input socket at its left
end and its output socket at its right end, both in the color of the type it carries. It has
no header and can't be collapsed.

| Input | Result |
|---|---|
| `Shift` + right-drag across links | Draws a dashed stroke; on release, puts a reroute on every link the stroke crosses, centered where it crosses. Links that come from the same output share one reroute, placed at the average of their crossings. One undo step. |
| Left-drag the middle of a reroute | Moves it. |
| Drag from its sockets | Connects, like any node's sockets. |
| **Reroute** in the add menu (Layout) | Adds an unconnected reroute. |

A reroute accepts any type, Material included; its output has the type of what's connected to
its input. Links into the Layer node, a Stack's layer inputs and a Mask Stack's effect inputs
aren't rerouted, so that layers keep their kind in the Layers panel.

## Layer groups

In both views, each layer of a Stack is drawn as one **layer group** node instead of the nodes
behind it, so the whole graph shows the layer structure and nothing else:

| Part | Contents |
|---|---|
| Header | Filled with the group color, slate `#3B5959`. The layer's name, and an open button on the right. |
| Outputs | **Content** (Material) when the layer has content, and **Mask** (Float) when it has a mask, linked to the Stack's inputs for that layer. |

The nodes of a layer's content and mask are inside its group, including nodes another layer
reads too. Links between nodes of different layers show only while one of the layers is open.

| Input | Result |
|---|---|
| Double-click a group, or click its open button | Selects the layer in the Layers panel and shows it in the **Selected layer** view. |
| Left-drag a group | Moves it, with the rest of the selection. |
| Click, `Shift`+click, box select, `A` | Select groups like nodes. |

Groups are placed like new nodes the first time they're shown; after that they stay where they
were moved. Their positions are saved with the project (see
[Project format](project-format.md#graph)), and moving them is one undo step. Groups can't be
collapsed, previewed, copied, duplicated, dissolved or deleted in the editor; the operations below
and the alignments skip them. Links can't be dragged out of a group's outputs, and the Stack
inputs fed by a group can't be picked up: change layers in the Layers panel, or open the layer.

## Mouse

| Input | Result |
|---|---|
| Left-click node | Selects it and shows it in the Properties sidebar. |
| `Shift` + left-click node | Adds it to or removes it from the selection. |
| Left-drag node | Moves the selected nodes. |
| Left-drag on empty space | Box select. With `Shift`, adds to the selection. |
| Left-click empty space | Clears the selection. |
| Left-drag from an output | New link. Release on an input to connect, on a node to connect to its first fitting input (inputs without a value of their own first, then exact type matches, then free inputs), or on empty space to open the add menu with only nodes that can take the link; the new node is connected. |
| Left-drag from a connected input | Picks up the link from its input end. |
| Right-drag, `Alt` + left-drag, middle-drag | Pan. |
| Wheel | Zoom about the pointer. |
| Right-click empty space | Add menu. |
| Right-click node | Node menu: preview, collapse or expand, and every operation below except paste, with its shortcut. |
| `Ctrl+Shift` + left-click node | Preview (see below). On empty space: stop previewing. |
| `Alt` + right-drag from node to node | Lazy connect: the best matching output and input. |
| `Shift+Alt` + right-drag from node to node | Lazy connect with a menu of every compatible output → input pair. |
| `Ctrl` + right-drag | Cut: removes every link the stroke crosses. |
| `Shift` + right-drag | Adds [reroutes](#reroutes) on the links the stroke crosses. |
| Drop a mesh map from the Library | Adds a **Mesh Map** node set to that map. |

Lazy connect ranks pairs by inputs without a value of their own first (so Noise goes into Levels' **In**, not **In black**), then by exact type match, then by free inputs. It refuses connections
that would create a cycle; the target node's outline is green when a connection is possible and
red when not.

## Keys

| Input | Operation |
|---|---|
| `Tab` | Add menu at the pointer, also right after clicking or dragging a field on a node. Only while typing in a text field does `Tab` go to the text field instead. Over the Node Editor, `Tab` never moves the keyboard focus between controls. |
| `Delete` | Delete the selected nodes (a selected frame: only the frame). |
| `Ctrl+X` | Dissolve the selected nodes. |
| `Alt+X` | Delete unused nodes. |
| `Ctrl+C` | Copy the selected nodes. |
| `Ctrl+V` | Paste nodes at the pointer. |
| `Shift+W` | Align top edges. |
| `Shift+A` | Align left edges. |
| `Shift+S` | Align bottom edges. |
| `Shift+D` | Align right edges. |
| `Shift+=` | Align in a row or column. |
| `H` | Collapse or expand the selected nodes. |
| `F` | Put the selected nodes in a [frame](#frames). |
| `Alt+P` | Take the selected nodes out of their frames. |
| `Alt+S` | Swap. |
| `K` | Link active to selected. |
| `Shift+C` | Copy settings to selected. |
| `Backspace` | Reset to defaults. |
| `A` / `Alt+A` | Select all / none. |
| `Home` | Frame all nodes. |
| `Ctrl+0` | Merge: Mix. |
| `Ctrl++` (or `Ctrl+=`) | Merge: Add. |
| `Ctrl+-` | Merge: Subtract. |
| `Ctrl+8` | Merge: Multiply. |
| `Ctrl+/` | Merge: Divide. |
| `Ctrl+,` | Merge: Minimum. |
| `Ctrl+.` | Merge: Maximum. |

These keys are fixed. The Material Output and the root Stack can't be deleted, dissolved,
duplicated or copied. Every operation is one undo step. **Duplicate** has no key; it's in the
node menu.

## Add menu

A search field, then the node catalog by [category](nodes.md#categories), in the order of the
categories table: each category's name in small capitals after a square of its color, and its
nodes below. Typing filters by name; `Enter` adds the first match; `Esc` or a click outside
closes the menu. The new node is placed with its header's top-left corner at the point where
the menu was opened, selected, and stays visible until connected. Besides the node types, the
Input category lists **Factor** (a Value node of type Factor) and **Color** (a Vector node of
type Color).

## Preview

Shows one node output in the viewport instead of the material, unlit (as emission on black,
fully rough):

| Output type | Shown as |
|---|---|
| Float | Gray. |
| Color | The color. |
| Material | That material, lit. |

Repeating `Ctrl+Shift` + click on the same node steps through its outputs, then stops. The
previewed node's output socket has a green ring, and the header shows **Previewing <node> ›
<output>** with a **Stop preview** button. The preview is not saved, not part of undo, never
affects exports, and turns off when the Node Editor closes. The `preview` [script
command](script-commands.md) does the same headless.

## Operations

### Merge

Combines the main (first) outputs of the selected nodes, from top to bottom, and selects the
result. Needs at least two nodes.

| Selected outputs | Mix | Add, Subtract, Multiply, Min, Max | Divide |
|---|---|---|---|
| All Float | Merge nodes (Normal), chained through their Float output | Math nodes | Math nodes |
| Any Color or Vector | Merge nodes (Normal) | Merge nodes (Add, Subtract, Multiply, Darken, Lighten) | Math nodes |
| All Material | A new Stack with each as a layer; the topmost node is the top layer | — | — |
| Material and others | — | — | — |

With more than two nodes, merge nodes are chained: the first two are merged, then the result with
the third, and so on. The new nodes are placed to the right of the selection.

### Swap

| Selection | Result |
|---|---|
| Two nodes | Everything that read one node reads the other, using its output of the same type, or its first output. |
| One node, two or more linked inputs | Its first two linked inputs are exchanged. |
| One node, one linked input | The link moves to the next compatible input. |

### Other operations

| Operation | Result |
|---|---|
| Dissolve | Deletes the selected nodes. Each link they fed is reconnected to one of their inputs, preferring an input of the same type as the output that was read. |
| Delete unused | Deletes every node that does not feed the Material Output. |
| Duplicate | Copies the selected nodes with their parameters, label, cache setting and input links, offset down and right. Links between copied nodes point at the copies. |
| Align | If the selection is wider than tall, lines it up in a row at its average height; otherwise in a column at its average position. Evenly spaced. |
| Align top / left / bottom / right | Moves the selected nodes so their top, left, bottom or right edges line up with the outermost one: the highest top, leftmost left, lowest bottom or rightmost right edge. Only that coordinate changes. Uses each node's current height, collapsed or not. Needs two or more nodes; the Layer node isn't moved. |
| Reset to defaults | Restores the parameters of the selected nodes. Links are kept. |
| Copy settings to selected | Copies the active node's parameters to the other selected nodes of the same type. |
| Link active to selected | Lazy-connects the active node (the last one clicked) into each other selected node. |

### Copy and paste

`Ctrl+C` copies the selected nodes to the system clipboard as text. `Ctrl+V` pastes them, so
nodes can be copied between layers, projects and Crate windows. Pasted nodes:

- keep their parameters, label, **Cached** setting and collapsed state;
- keep the links between pasted nodes; links from nodes that weren't copied are dropped;
- keep their Stack layers, but a layer's content or mask is kept only if that node was copied
  too;
- keep their Mask Stack effects whose source node was copied too; other effects are dropped;
- are placed with the copied group's top-left corner at the pointer, selected, and stay visible
  until connected.

Paint and Paint Mask nodes are pasted without their paint. The Material Output, the root Stack
and the Layer node aren't copied. Pasting is one undo step. If the clipboard doesn't hold
copied nodes, `Ctrl+V` does nothing.

## Properties sidebar

Shows the active node's settings, laid out like the Properties panel: the node's title, then
**Label** (a text field), **Cached** (a toggle), **Ungroup** for generators, and the list of its
settings and value inputs with a pin each (only those shown on the node). Click a pin to expose
it on the layer (see
[How to edit a layer as a node graph](../how-to/edit-a-layer-as-a-graph.md#expose-parameters-on-the-layer)).
Values are edited on the node. With several nodes selected, it shows the count and the active
node. While a frame is the active item, it shows **Frame** and its **Label**.

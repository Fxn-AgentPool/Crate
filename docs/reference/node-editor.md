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
| Layer | The selected layer's content and mask subgraphs, plus a **Layer** node whose **Content** and **Mask** inputs are the layer's. Header: **NODE EDITOR / <layer>**, with a **Whole graph** button. |
| Whole project | Everything that feeds the Material Output. Header: **NODE EDITOR / Whole project**, with a **Selected layer** button when a layer is selected. Shown when no layer is selected, and when the viewed layer is deleted. |

The right end of the header shows the most used inputs as keycaps: `Tab` **Add**,
`Ctrl+Shift+Click` **Preview**, `Ctrl+RMB` **Cut links**. **?** **All shortcuts** lists every
mouse and key input of the editor; hover over it to see the list.

Nodes added in the editor stay visible until connected, even if nothing reads them yet. Nodes
added elsewhere (for example new layers) are placed to the left of the node that reads them, level
with the socket they feed, below any node in the way.

While the pointer is over the Node Editor, its keys take precedence over the application's
shortcuts. `Ctrl+Space`, undo, redo, save, new, open and export still work.

## Nodes

From top to bottom, a node shows:

| Part | Contents |
|---|---|
| Header | Collapse arrow (▾ expanded, ▸ collapsed), the node's title, and **cached** when the node is cached. |
| Outputs | One row per output socket, on the right edge. |
| Parameters | One row per parameter, edited in place (see below). |
| Inputs | One row per input socket, on the left edge. |

The **Layer** node has no parameters and can't be collapsed.

### Parameters

Every parameter takes one row:

| Parameter kind | On the node |
|---|---|
| Number | A field showing the name and the value. Drag sideways to change it, or click to type a value. Values are clamped to the parameter's range. |
| Color | The name and a swatch; click the swatch to pick a color. |
| Vector | The name and three number fields. |
| On/off | A checkbox with the name. |
| Choice | A dropdown showing the current option. |
| Image | A button with the image name, or **(none)**; click to choose a file. |
| Ramp stops | A bar showing the gradient; click it to edit the stops (position, color, **×** to remove, **+ Stop** to add, up to 8). |
| ID colors | A button showing how many IDs are selected; click it for the list of IDs, each with a checkbox. |

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

## Mouse

| Input | Result |
|---|---|
| Left-click node | Selects it and shows it in the Properties sidebar. |
| `Shift` + left-click node | Adds it to or removes it from the selection. |
| Left-drag node | Moves the selected nodes. |
| Left-drag on empty space | Box select. With `Shift`, adds to the selection. |
| Left-click empty space | Clears the selection. |
| Left-drag from an output | New link. Release on an input to connect, on a node to connect to its first fitting input, or on empty space to open the add menu with only nodes that can take the link; the new node is connected. |
| Left-drag from a connected input | Picks up the link from its input end. |
| Right-drag, `Alt` + left-drag, middle-drag | Pan. |
| Wheel | Zoom about the pointer. |
| Right-click empty space | Add menu. |
| Right-click node | Node menu: preview, collapse or expand, and every operation below except paste, with its shortcut. |
| `Ctrl+Shift` + left-click node | Preview (see below). On empty space: stop previewing. |
| `Alt` + right-drag from node to node | Lazy connect: the best matching output and input. |
| `Shift+Alt` + right-drag from node to node | Lazy connect with a menu of every compatible output → input pair. |
| `Ctrl` + right-drag | Cut: removes every link the stroke crosses. |
| Drop a mesh map from the Library | Adds a **Mesh Map** node set to that map. |

Lazy connect ranks pairs by exact type match first, then by free inputs. It refuses connections
that would create a cycle; the target node's outline is green when a connection is possible and
red when not.

## Keys

| Input | Operation |
|---|---|
| `Tab` | Add menu at the pointer. |
| `Delete` | Delete the selected nodes. |
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

A search field and the node catalog by category. Typing filters by name; `Enter` adds the first
match; `Esc` or a click outside closes the menu.

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
| Any Color | Merge nodes (Normal) | Merge nodes (Add, Subtract, Multiply, Darken, Lighten) | Math nodes |
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

Shows the active node's settings: label, **Cached**, **Ungroup** for generators, and the list of
its parameters with a pin each. Click a pin to expose the parameter on the layer (see
[How to edit a layer as a node graph](../how-to/edit-a-layer-as-a-graph.md#expose-parameters-on-the-layer)).
Parameter values are edited on the node. With several nodes selected, it shows the count and
the active node.

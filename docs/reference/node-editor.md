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
| Layer | The selected layer's content and mask subgraphs, plus a **Layer** node whose **Content** and **Mask** inputs are the layer's. Header: **Node Editor: <layer>**, with a **Whole graph** button. |
| Whole project | Everything that feeds the Material Output. Header: **Node Editor: whole project**, with a **Selected layer** button when a layer is selected. Shown when no layer is selected, and when the viewed layer is deleted. |

Nodes added in the editor stay visible until connected, even if nothing reads them yet. Nodes
added elsewhere (for example new layers) are placed to the left of the node that reads them, level
with the socket they feed, below any node in the way.

While the pointer is over the Node Editor, its keys take precedence over the application's
shortcuts. `Ctrl+Space`, undo, redo, save, new, open and export still work.

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
| Right-click node | Node menu: preview and every Node Wrangler operation below, with its shortcut. |
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
| `Tab`, `Shift+A` | Add menu at the pointer. |
| `Delete` | Delete the selected nodes. |
| `Ctrl+X` | Dissolve the selected nodes. |
| `Alt+X` | Delete unused nodes. |
| `Shift+D` | Duplicate. |
| `Shift+=` | Align. |
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

These keys are fixed. The Material Output and the root Stack can't be deleted, dissolved or
duplicated. Every operation is one undo step.

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
| Reset to defaults | Restores the parameters of the selected nodes. Links are kept. |
| Copy settings to selected | Copies the active node's parameters to the other selected nodes of the same type. |
| Link active to selected | Lazy-connects the active node (the last one clicked) into each other selected node. |

## Properties sidebar

Shows the active node: label, **Cached**, **Ungroup** for generators, and its parameters. With
several nodes selected, it shows the count and the active node.

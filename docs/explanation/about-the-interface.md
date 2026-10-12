# About the interface

The viewport and the node graph are what you work on, so the interface around them stays out of
the way: quiet chrome, loud content. This page explains the rules the interface follows. The
controls themselves are described in [Windows and panels](../reference/windows-and-panels.md#controls).

## Neutral graphite, color for meaning

Panels, windows and fields use five steps of dark graphite, from the window chrome (darkest) to
fields and hovered controls (lightest), and text uses three steps of gray: values, labels and
hints. Neither ever reaches pure black or white, so a painted texture is always the most
contrasting thing on screen.

Color is reserved for meaning. Blue marks selection and focus. A fixed set of hues marks the
data types of node sockets: amber for color, gray for float and masks, cyan for vectors and
normals, green for materials. Status tags reuse two of them: green for good (**cached**), amber
for attention (**dirty**, a layer whose blend differs per channel). A value that looks colorful
in the interface therefore always means something.

## One control per value

Earlier versions showed most numbers twice: a slider and a number box beside it, each with its
own label. A **field** now does both: its label and value sit inside it, its fill shows where the
value lies in its range, dragging anywhere in it changes the value, and clicking types
one. That saves about 90 points of panel width per row, which goes to longer names and wider
previews.

**Toggles** are for settings that take effect at once, like **Show buffer**. **Checkboxes** are
for choosing items of a set, like the channels a brush paints. A disabled control fades as a
whole, rather than greying only its label, so it never looks half active.

The **color picker** shows sRGB numbers, although Crate stores and renders colors in linear
light. Artists know colors by their display values: a mid gray is 0.5 or `#808080`, and hex codes
travel between applications that all mean the display encoding. Earlier versions showed the
linear values, so a color typed as 0.5 looked like about 74% gray, and a hex code from another
application had to be converted first. The conversion now happens behind the picker, as in
other painting software; the linear values only matter to the renderer.

## The work in the middle

Tools sit in a narrow column of icons at the left edge, and the brush values changed most while
painting (size, flow, hardness) sit in a strip above the viewport, next to the bake mode. The
panels at the sides run the full height of the window, so the viewport column holds only what
acts on the viewport.

## Type and grid

Crate uses Geist for the interface and Geist Mono for every number, file name, resolution and
keyboard shortcut. Mono digits all have the same width, so values in a column line up and don't
jitter while you drag. Both fonts are embedded in the application under the SIL Open Font
License 1.1.

Everything sits on a 4-point grid: controls are 24 points high, rows 26 and section headers 32.
Section headers are small capitals separated by lines across the whole panel, so a long panel reads as a list of
sections instead of a staircase of indentation.

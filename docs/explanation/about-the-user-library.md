# About the user library

The user library is a set of plain folders on disk, one per Library category. Crate doesn't keep
an index or a database of what's in it: each category is whatever its folder holds when it's
scanned.

## Why folders

Most of what goes into a library comes from outside Crate. Environment maps are downloaded from
HDRI sites, alphas and grunge images come in packs, and studios keep shared collections on a
network drive. With folders, adding them is a file copy, and the tools people already use for
files work unchanged: the file browser, sync clients, version control, scripts.

The alternative, importing through Crate into a managed store, would allow tags, previews and
metadata, but every file would have to pass through Crate first, and the store could drift out of
sync with the files it describes. A database can still be added on top of the folders later;
the reverse is harder.

## Prepared and plain categories

The categories differ in how much Crate needs to understand a file:

- **Smart materials** (and, later, materials and smart masks) are layer stacks with exposed
  parameters and a thumbnail. Crate writes them; copying them between machines works, but
  making one by hand doesn't.
- **Textures, environments, alphas and stencils** are ordinary image files. Any file of the right type
  works as is, so these are the folders meant for filling by hand.

Folders you add yourself are treated as plain image collections. They're shown as categories so
a large collection can be split into folders that are quick to browse, without Crate having to
know what they're for. Images from them load as stencils, the most general use of an image.

## No subfolders

Each category shows only the files directly in its folder. Showing nested folders would need a
tree or breadcrumbs inside every category; a flat list per folder keeps browsing to one click,
and extra top-level folders cover most of the need for structure.

## What stays outside projects

Library files are referenced only while you use them. A stencil or brush alpha taken from the
library (or a texture imported from it) is copied into the project, so the project still opens if the library changes or on a
machine without it. An environment is the exception: like **Load environment**, the project
stores its path, and falls back to studio lighting if the file is gone.

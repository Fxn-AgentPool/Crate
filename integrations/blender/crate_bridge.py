"""Crate bridge for Blender.

DEPRECATED: this add-on is kept as a reference implementation only and is no longer
maintained; an official Blender integration is being built separately. It still works with
the command line as documented in docs/reference/command-line.md.

Adds File > Export > Crate Project (reimport): exports the scene (or the selection) as glTF
and runs `crate reimport` on a Crate project, optionally exporting textures afterwards.

Install: Edit > Preferences > Add-ons > Install from Disk, pick this file, enable it, and set
the path to the crate executable in the add-on's preferences (or put crate on PATH).
"""

bl_info = {
    "name": "Crate Bridge",
    "author": "Crate",
    "version": (1, 0, 0),
    "blender": (4, 0, 0),
    "location": "File > Export > Crate Project (reimport)",
    "description": "Export to glTF and reimport into a Crate project (deprecated reference implementation)",
    "warning": "Deprecated: kept as a reference implementation",
    "category": "Import-Export",
}

import json
import os
import subprocess

import bpy
from bpy.props import BoolProperty, StringProperty


class CrateBridgePreferences(bpy.types.AddonPreferences):
    bl_idname = __name__

    crate_path: StringProperty(
        name="Crate executable",
        description="Path to crate (crate.exe on Windows). Empty: use PATH",
        subtype="FILE_PATH",
        default="",
    )

    def draw(self, context):
        self.layout.prop(self, "crate_path")


def crate_executable(context):
    prefs = context.preferences.addons[__name__].preferences if __name__ in context.preferences.addons else None
    path = bpy.path.abspath(prefs.crate_path) if prefs and prefs.crate_path else ""
    return path or "crate"


def reimport(crate, project, mesh, export_dir=""):
    """Runs `crate reimport` and returns its JSON result (raises on failure)."""
    cmd = [crate, "reimport", project, mesh, "--json"]
    if export_dir:
        cmd += ["--export", export_dir]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    try:
        result = json.loads(proc.stdout.strip().splitlines()[-1])
    except (IndexError, json.JSONDecodeError):
        raise RuntimeError(f"crate failed (exit {proc.returncode}): {proc.stderr.strip() or proc.stdout.strip()}")
    if not result.get("ok"):
        raise RuntimeError(result.get("error", f"crate failed (exit {proc.returncode})"))
    return result


class EXPORT_OT_crate_reimport(bpy.types.Operator):
    """Export to glTF and reimport into a Crate project"""

    bl_idname = "export_scene.crate_reimport"
    bl_label = "Export to Crate"
    bl_options = {"REGISTER"}

    project: StringProperty(name="Crate project", description="The .crate folder", subtype="DIR_PATH")
    mesh_path: StringProperty(
        name="Mesh file",
        description="Where to write the glTF (default: <project name>.glb next to the project)",
        subtype="FILE_PATH",
    )
    export_dir: StringProperty(
        name="Export textures to",
        description="Optional: export textures here after the reimport",
        subtype="DIR_PATH",
    )
    selected_only: BoolProperty(name="Selected only", default=False)

    def invoke(self, context, event):
        return context.window_manager.invoke_props_dialog(self, width=420)

    def execute(self, context):
        project = bpy.path.abspath(self.project).rstrip("\\/")
        if not project or not os.path.isfile(os.path.join(project, "project.json")):
            self.report({"ERROR"}, f"Not a Crate project: {project}")
            return {"CANCELLED"}
        # Default: <project name>.glb next to the project, so exported textures are named after it.
        mesh = bpy.path.abspath(self.mesh_path) if self.mesh_path else os.path.splitext(project)[0] + ".glb"
        bpy.ops.export_scene.gltf(filepath=mesh, export_format="GLB", use_selection=self.selected_only)
        export_dir = bpy.path.abspath(self.export_dir) if self.export_dir else ""
        try:
            result = reimport(crate_executable(context), project, mesh, export_dir)
        except (RuntimeError, OSError) as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        msg = f"Crate: reimported in {result['seconds']:.1f}s"
        if result.get("files"):
            msg += f", exported {len(result['files'])} textures"
        self.report({"INFO"}, msg)
        for w in result.get("warnings", []):
            self.report({"WARNING"}, w)
        return {"FINISHED"}


def menu_func(self, context):
    self.layout.operator(EXPORT_OT_crate_reimport.bl_idname, text="Crate Project (reimport)")


classes = (CrateBridgePreferences, EXPORT_OT_crate_reimport)


def register():
    for c in classes:
        bpy.utils.register_class(c)
    bpy.types.TOPBAR_MT_file_export.append(menu_func)


def unregister():
    bpy.types.TOPBAR_MT_file_export.remove(menu_func)
    for c in reversed(classes):
        bpy.utils.unregister_class(c)


if __name__ == "__main__":
    register()

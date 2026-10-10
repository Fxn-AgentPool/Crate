"""Painter bridge for Blender.

Adds File > Export > Painter Project (reimport): exports the scene (or the selection) as glTF
and runs `painter reimport` on a Painter project, optionally exporting textures afterwards.

Install: Edit > Preferences > Add-ons > Install from Disk, pick this file, enable it, and set
the path to the painter executable in the add-on's preferences (or put painter on PATH).
"""

bl_info = {
    "name": "Painter Bridge",
    "author": "Painter",
    "version": (1, 0, 0),
    "blender": (4, 0, 0),
    "location": "File > Export > Painter Project (reimport)",
    "description": "Export to glTF and reimport into a Painter project",
    "category": "Import-Export",
}

import json
import os
import subprocess

import bpy
from bpy.props import BoolProperty, StringProperty


class PainterBridgePreferences(bpy.types.AddonPreferences):
    bl_idname = __name__

    painter_path: StringProperty(
        name="Painter executable",
        description="Path to painter (painter.exe on Windows). Empty: use PATH",
        subtype="FILE_PATH",
        default="",
    )

    def draw(self, context):
        self.layout.prop(self, "painter_path")


def painter_executable(context):
    prefs = context.preferences.addons[__name__].preferences if __name__ in context.preferences.addons else None
    path = bpy.path.abspath(prefs.painter_path) if prefs and prefs.painter_path else ""
    return path or "painter"


def reimport(painter, project, mesh, export_dir=""):
    """Runs `painter reimport` and returns its JSON result (raises on failure)."""
    cmd = [painter, "reimport", project, mesh, "--json"]
    if export_dir:
        cmd += ["--export", export_dir]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    try:
        result = json.loads(proc.stdout.strip().splitlines()[-1])
    except (IndexError, json.JSONDecodeError):
        raise RuntimeError(f"painter failed (exit {proc.returncode}): {proc.stderr.strip() or proc.stdout.strip()}")
    if not result.get("ok"):
        raise RuntimeError(result.get("error", f"painter failed (exit {proc.returncode})"))
    return result


class EXPORT_OT_painter_reimport(bpy.types.Operator):
    """Export to glTF and reimport into a Painter project"""

    bl_idname = "export_scene.painter_reimport"
    bl_label = "Export to Painter"
    bl_options = {"REGISTER"}

    project: StringProperty(name="Painter project", description="The .painter folder", subtype="DIR_PATH")
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
            self.report({"ERROR"}, f"Not a Painter project: {project}")
            return {"CANCELLED"}
        # Default: <project name>.glb next to the project, so exported textures are named after it.
        mesh = bpy.path.abspath(self.mesh_path) if self.mesh_path else os.path.splitext(project)[0] + ".glb"
        bpy.ops.export_scene.gltf(filepath=mesh, export_format="GLB", use_selection=self.selected_only)
        export_dir = bpy.path.abspath(self.export_dir) if self.export_dir else ""
        try:
            result = reimport(painter_executable(context), project, mesh, export_dir)
        except (RuntimeError, OSError) as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        msg = f"Painter: reimported in {result['seconds']:.1f}s"
        if result.get("files"):
            msg += f", exported {len(result['files'])} textures"
        self.report({"INFO"}, msg)
        for w in result.get("warnings", []):
            self.report({"WARNING"}, w)
        return {"FINISHED"}


def menu_func(self, context):
    self.layout.operator(EXPORT_OT_painter_reimport.bl_idname, text="Painter Project (reimport)")


classes = (PainterBridgePreferences, EXPORT_OT_painter_reimport)


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

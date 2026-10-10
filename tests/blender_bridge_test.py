# Tests the Blender add-on in background mode:
#   blender --background --factory-startup --python tests/blender_bridge_test.py -- <painter> <project.painter> <out dir>
# Imports the crate, stretches it, and runs the add-on's operator to reimport into the project.
import os
import sys

import bpy

args = sys.argv[sys.argv.index("--") + 1:]
painter, project, out = args
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "integrations", "blender"))
import painter_bridge  # noqa: E402

painter_bridge.register()
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=os.path.join(os.path.dirname(__file__), "..", "assets", "crate.glb"))
for ob in bpy.context.scene.objects:
    ob.scale.z *= 1.2

# The add-on reads the executable from its preferences; outside an installed add-on, point
# the helper at the given path.
painter_bridge.painter_executable = lambda context: painter
result = bpy.ops.export_scene.painter_reimport(
    project=project,
    mesh_path=os.path.join(out, "from_blender.glb"),
    export_dir=os.path.join(out, "textures"),
)
print("BRIDGE RESULT", result)
sys.exit(0 if result == {"FINISHED"} else 1)

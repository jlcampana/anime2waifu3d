"""Importa un GLB, lo reduce a N triángulos (collapse, conservando bordes de UV) y lo exporta.
blender -b --factory-startup --python decimate.py -- IN.glb OUT.glb TRIANGLES
"""
import sys, time, bpy

src, dst, target = sys.argv[sys.argv.index("--") + 1:]
target = int(target)
for o in list(bpy.data.objects):
    bpy.data.objects.remove(o)
bpy.ops.import_scene.gltf(filepath=src)
meshes = [o for o in bpy.data.objects if o.type == "MESH"]
for ob in meshes:
    tris = sum(len(p.vertices) - 2 for p in ob.data.polygons)
    t = time.time()
    # Une vértices duplicados por las costuras para que el colapso no abra grietas; las UV
    # se conservan porque cada esquina guarda la suya.
    bpy.context.view_layer.objects.active = ob
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.mesh.remove_doubles(threshold=1e-6)
    bpy.ops.object.mode_set(mode="OBJECT")
    mod = ob.modifiers.new("dec", "DECIMATE")
    mod.decimate_type = "COLLAPSE"
    mod.ratio = min(1.0, target / tris)
    mod.use_collapse_triangulate = True
    mod.delimit = {"UV"}
    bpy.ops.object.modifier_apply(modifier=mod.name)
    after = sum(len(p.vertices) - 2 for p in ob.data.polygons)
    print(f"DECIMADO {ob.name}: {tris} -> {after} triángulos en {time.time() - t:.1f}s")
bpy.ops.export_scene.gltf(filepath=dst, export_format="GLB")
print("EXPORTADO", dst)

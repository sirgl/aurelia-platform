"""GLB export of geom.Mesh LODs. Used inside Blender by export_units.py, export_world.py and
export_terrain.py; the manifests are written by manifest_io.py (no bpy).

One GLB per model: three mesh nodes LOD0 / LOD1 / LOD2 with identity transforms.
Vertex color: RGB = palette albedo, A = team mask (1 on "team" parts, else 0).
Each part writes into its own <part>/assets/models/ folder.
"""
from pathlib import Path

import bpy

import geom

LOD_NAMES = ("LOD0", "LOD1", "LOD2")


def clear():
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    for coll in (bpy.data.meshes, bpy.data.materials):
        for item in list(coll):
            coll.remove(item)


def vertex_paint_material():
    """Plain material. With no vertex-color node in the tree, export_vertex_color="ACTIVE"
    writes COLOR_0 as RGBA; a wired vertex-color node would drop the alpha (team mask)."""
    return bpy.data.materials.new("UnitVertexPaint")


def lod_object(name, mesh_data, material):
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(mesh_data.points, [], mesh_data.triangles)
    mesh.update()
    attr = mesh.color_attributes.new(name="Color", type="FLOAT_COLOR", domain="POINT")
    for i, material_name in enumerate(mesh_data.colors):
        r, g, b = geom.PALETTE[material_name]
        attr.data[i].color = (r, g, b, 1.0 if material_name == "team" else 0.0)
    mesh.color_attributes.active_color = attr
    mesh.materials.append(material)
    for poly in mesh.polygons:
        poly.use_smooth = False
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj


def export_lods(path, meshes):
    """Writes the three LOD meshes (model units, z up) as one GLB."""
    clear()
    material = vertex_paint_material()
    objects = [lod_object(name, mesh, material) for name, mesh in zip(LOD_NAMES, meshes)]
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.export_scene.gltf(
        filepath=str(path), export_format="GLB", use_selection=True, export_yup=True,
        export_vertex_color="ACTIVE", export_all_vertex_colors=True, export_materials="EXPORT",
        export_normals=False, export_texcoords=False,
    )


def remove_stale(directory, expected):
    """Deletes GLBs in `directory` that no manifest entry declares."""
    for stale in Path(directory).glob("*.glb"):
        # "._*" are macOS AppleDouble shadows on non-HFS volumes, not assets.
        if stale not in expected and not stale.name.startswith("._"):
            stale.unlink()
            print("removed stale", stale)

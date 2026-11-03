"""Shared low-poly modeling helpers for nation Blender scripts.

Coordinate convention is Blender-native (Z-up): tx = right, ty = forward,
tz = up. rx/ry/rz are degrees, Euler XYZ. sx/sy/sz are full extents.

Vertex paint: POINT-domain FLOAT_COLOR attribute ("Color") where
RGB = baked albedo (sRGB display space, matches terrain palette) and
A = team mask (1.0 -> owner color, 0.0 -> fixed). Bake (build.rs) reads
POSITION + COLOR_0 and computes flat normals itself, so primitive winding
must stay outward-correct (Blender primitives are; do not mirror).
"""

import bpy
import bmesh
from mathutils import Matrix, Euler


def clear_scene():
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    # Purge orphan meshes/materials so exports stay lean.
    for coll in (bpy.data.meshes, bpy.data.materials):
        for x in list(coll):
            coll.remove(x)


def new_workbench(name):
    """Fresh mesh+object, linked and active. Returns (bm, mesh, obj)."""
    mesh = bpy.data.meshes.new(name)
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)
    return bmesh.new(), mesh, obj


def mat(tx=0.0, ty=0.0, tz=0.0, rx=0.0, ry=0.0, rz=0.0, sx=1.0, sy=1.0, sz=1.0):
    """Compose a transform matrix (Blender units, Z-up). Angles in radians."""
    from math import radians

    rot = Euler((radians(rx), radians(ry), radians(rz)), "XYZ").to_matrix().to_4x4()
    return (
        Matrix.Translation((tx, ty, tz))
        @ rot
        @ Matrix.Diagonal((sx, sy, sz)).to_4x4()
    )


def _tracked(bm, op, *args, **kwargs):
    """Run a bmesh op, return faces created by it.

    Reliable because we never delete faces: new faces always take indices
    past the previous face count.
    """
    bm.faces.ensure_lookup_table()
    before = len(bm.faces)
    op(bm, *args, **kwargs)
    bm.faces.ensure_lookup_table()
    return list(bm.faces[before:])


def cube(bm, m):
    """Unit box scaled/posed by m. Returns new faces."""
    return _tracked(bm, bmesh.ops.create_cube, size=2.0, matrix=m)


def cone(bm, m, segments=8, r1=1.0, r2=0.0, depth=2.0):
    """Capped cone (r2=0 -> spike). Returns new faces."""
    return _tracked(
        bm, bmesh.ops.create_cone, cap_ends=True, segments=segments,
        radius1=r1, radius2=r2, depth=depth, matrix=m,
    )


def ball(bm, m, u=8, v=6, radius=1.0):
    return _tracked(
        bm, bmesh.ops.create_uvsphere, u_segments=u, v_segments=v,
        radius=radius, matrix=m,
    )


def wedge(bm, m, w=1.0, h=1.0, d=1.0):
    """Gable-roof prism (Blender coords): base rect 2w x 2d in XY,
    ridge along X at height h. CCW-outward faces."""
    verts = [
        (-w, -d, 0.0), (w, -d, 0.0), (w, d, 0.0), (-w, d, 0.0),
        (-w, 0.0, h), (w, 0.0, h),
    ]
    faces = [(3, 2, 5, 4), (0, 1, 5, 4), (0, 3, 4), (2, 1, 5)]
    tmp = bpy.data.meshes.new("tmpwedge")
    tmp.from_pydata(verts, [], faces)
    nv_before = len(bm.verts)
    nf_before = len(bm.faces)
    bm.from_mesh(tmp)
    bpy.data.meshes.remove(tmp)
    # from_mesh adds elements with index=-1; without index_update() the index
    # slices below are empty or wrong and the roof prism does NOT get the transform
    # (it sits at the origin: the roofs "disappeared" from the bake).
    bm.verts.index_update()
    bm.faces.index_update()
    fresh = list(bm.faces[nf_before:])
    xf = [v for v in bm.verts if v.index >= nv_before]
    bmesh.ops.transform(bm, matrix=m, verts=xf)
    return fresh


def triangulate(bm):
    bmesh.ops.triangulate(bm, faces=list(bm.faces))


def paint_and_flush(bm, mesh, parts):
    """parts: list of (faces, (r,g,b,a)). Paints POINT colors, then
    triangulates (triangulation keeps verts, so colors survive)."""
    bm.verts.ensure_lookup_table()
    bm.faces.ensure_lookup_table()
    ca_data = {}
    for faces, col in parts:
        for f in faces:
            for v in f.verts:
                ca_data[v.index] = col
    triangulate(bm)
    bm.to_mesh(mesh)
    bm.free()
    mesh.update()
    ca = mesh.color_attributes.new(name="Color", type="FLOAT_COLOR", domain="POINT")
    for idx, col in ca_data.items():
        ca.data[idx].color = col
    return mesh


def export_selected(path):
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        use_selection=True,
    )
    print("EXPORTED", path)

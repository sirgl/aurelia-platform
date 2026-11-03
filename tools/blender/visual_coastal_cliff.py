"""Weathered coastal ledges with broad bedding planes; Blender source of truth."""
import bpy, bmesh, math, pathlib, json, sys
from mathutils import Vector
ROOT=pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parent))

def build():
    vertices=[];faces=[];colors=[]
    # Interlocking inclined beds replace the old even stack of five pancakes.
    # Eight-sided contours retain broad uninterrupted front faces and a broken crest.
    from visual_rocks import slab
    slab(vertices,faces,colors,
         [(-.53,-.21),(-.20,-.29),(.29,-.23),(.55,-.06),(.48,.16),(.12,.23),(-.37,.22),(-.57,.02)],
         .32,(.025,.018),(-.12,.045),.98)
    slab(vertices,faces,colors,
         [(-.42,-.10),(-.21,-.17),(.12,-.15),(.31,-.01),(.23,.18),(-.02,.25),(-.39,.21),(-.51,.05)],
         .42,(-.028,.014),(-.08,.045),1.02)
    minimum=min(p[2] for p in vertices);vertices=[(x,y,z-minimum) for x,y,z in vertices]
    height=max(p[2] for p in vertices);vertices=[(x,y,z*.433/height) for x,y,z in vertices]
    mesh=bpy.data.meshes.new('Coastal stratified fractured ledges');mesh.from_pydata(vertices,[],faces);mesh.update()
    bm=bmesh.new();bm.from_mesh(mesh);bmesh.ops.recalc_face_normals(bm,faces=list(bm.faces));bmesh.ops.triangulate(bm,faces=list(bm.faces));bm.to_mesh(mesh);bm.free();mesh.update()
    attr=mesh.color_attributes.new(name='Color',type='FLOAT_COLOR',domain='POINT')
    for i,color in enumerate(colors):attr.data[i].color=color
    mesh.color_attributes.active_color=attr
    obj=bpy.data.objects.new('visual_coastal_cliff',mesh);bpy.context.collection.objects.link(obj)
    material=bpy.data.materials.new('Warm grey coastal stone');material.use_nodes=True
    nodes=material.node_tree.nodes;bsdf=nodes.get('Principled BSDF');bsdf.inputs['Roughness'].default_value=.96
    paint=nodes.new('ShaderNodeVertexColor');paint.layer_name='Color';gamma=nodes.new('ShaderNodeGamma');gamma.inputs['Gamma'].default_value=2.2
    material.node_tree.links.new(paint.outputs['Color'],gamma.inputs['Color']);material.node_tree.links.new(gamma.outputs['Color'],bsdf.inputs['Base Color']);mesh.materials.append(material)
    return obj

def aim(obj,p):obj.rotation_euler=(Vector(p)-obj.location).to_track_quat('-Z','Y').to_euler()
def main():
    bpy.ops.object.select_all(action='SELECT');bpy.ops.object.delete(use_global=False)
    obj=build();bpy.context.view_layer.objects.active=obj;obj.select_set(True)
    bpy.ops.export_scene.gltf(filepath=str(ROOT/'assets/models/visual-coastal-cliff.glb'),export_format='GLB',use_selection=True,export_yup=True,export_vertex_color='ACTIVE',export_all_vertex_colors=True)
    assert len(obj.data.polygons)<=170
    bpy.ops.mesh.primitive_plane_add(size=200,location=(0,0,-.01));mat=bpy.data.materials.new('Sand studio');mat.diffuse_color=(.34,.31,.24,1);bpy.context.object.data.materials.append(mat)
    bpy.ops.object.camera_add(location=(1.1,-2.,1.15));camera=bpy.context.object;aim(camera,(0,0,.17));camera.data.type='ORTHO';camera.data.ortho_scale=1.67;bpy.context.scene.camera=camera
    for loc,power,size,color in [((-3,-4,6),700,4,(1,.94,.83)),((4,-1,4),400,3,(.8,.89,1)),((0,4,5),800,3,(1,.94,.86))]:
        bpy.ops.object.light_add(type='AREA',location=loc);light=bpy.context.object;light.data.energy=power;light.data.size=size;light.data.color=color;aim(light,(0,0,.2))
    scene=bpy.context.scene;scene.render.engine='CYCLES';scene.cycles.samples=24;scene.cycles.use_denoising=True;scene.world.color=(.12,.14,.16);scene.view_settings.view_transform='Standard';scene.view_settings.look='Medium High Contrast';scene.view_settings.exposure=-1.1;scene.render.resolution_x=1400;scene.render.resolution_y=900;scene.render.resolution_percentage=100;scene.render.image_settings.file_format='PNG';scene.render.filepath=str(ROOT/'output/blender-coastal-cliff.png')
    bpy.ops.wm.save_as_mainfile(filepath=str(ROOT/'assets/models/visual-coastal-cliff.blend'))
    stats={'triangles':len(obj.data.polygons),'height':max(v.co.z for v in obj.data.vertices),'half_width':max(abs(v.co.x) for v in obj.data.vertices)}
    (ROOT/'output/blender-coastal-cliff-stats.json').write_text(json.dumps(stats,indent=2));bpy.ops.render.render(write_still=True);print('CLIFF_STATS',stats)
main()

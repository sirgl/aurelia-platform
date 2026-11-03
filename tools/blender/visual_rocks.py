"""Authored fractured slabs: intersections of cutting planes, never spheres or cubes.
Run: blender --background --python tools/blender/visual_rocks.py
"""
import bpy, bmesh, math, pathlib, json
from mathutils import Vector
ROOT=pathlib.Path(__file__).resolve().parents[2]

# Two interlocking strata, not a main rock surrounded by loose pebble primitives.
# Rings retain eight broad faces; only shoulder bevels add narrow facets.
def slab(verts, faces, colors, polygon, height, offset, tilt, tone=1.):
    cx=sum(x for x,y in polygon)/len(polygon)
    cy=sum(y for x,y in polygon)/len(polygon)
    rings=[]
    # Each stratum has truly planar fracture faces. Non-planar loft quads would
    # produce two noisy normals after triangulation, even with flat face colors.
    for scale,z,dx,dy in [(1.,0.,0.,0.),(1.,.045,0.,0.),
                           (.88,height*.62,offset[0],offset[1]),
                           (.64,height,offset[0]-.015,offset[1]+.012)]:
        ring=[]
        for x,y in polygon:
            px=cx+(x-cx)*scale+dx;py=cy+(y-cy)*scale+dy
            ring.append((px,py,0. if z==0. else z+tilt[0]*px+tilt[1]*py))
        rings.append(ring)
    def face(points, color):
        start=len(verts);verts.extend(points);faces.append(tuple(range(start,len(verts))))
        colors.extend([tuple(c*tone for c in color)+(0.,)]*len(points))
    face(list(reversed(rings[0])),(.31,.34,.35))
    face(rings[-1],(.59,.60,.56))
    for layer in range(3):
        for i in range(len(polygon)):
            j=(i+1)%len(polygon)
            # Coherent bedding: warm caps, cool fracture faces, no triangle noise.
            color=(.47,.50,.49) if layer==1 else (.54,.56,.52)
            face([rings[layer][i],rings[layer][j],rings[layer+1][j],rings[layer+1][i]],color)


def make(index):
    verts=[];faces=[];colors=[]
    outlines=[
      [(-.49,-.23),(-.16,-.35),(.13,-.14),(.48,-.09),(.40,.19),(.10,.27),(-.31,.20),(-.51,.02)],
      [(-.51,-.17),(-.27,-.29),(.16,-.12),(.51,-.04),(.38,.15),(.01,.25),(-.33,.19),(-.54,.02)],
      [(-.43,-.19),(-.14,-.31),(.17,-.09),(.43,.02),(.27,.23),(-.02,.31),(-.35,.23),(-.49,.01)]
    ]
    polygon=outlines[index]
    slab(verts,faces,colors,polygon,[.43,.29,.54][index],(.055,-.035),(-.16,.05),1.)
    # Attached, oblique foot slab overlaps the main body: one readable outcrop.
    toe=[(x*.80+.035,y*.53-.19) for x,y in polygon]
    slab(verts,faces,colors,toe,[.16,.13,.18][index],(-.025,.012),(-.10,.04),.94)
    # Keep stable renderer bounds while preserving the sculpted asymmetry.
    minz=min(p[2] for p in verts);radius=max(max(abs(p[0]),abs(p[1])) for p in verts)
    factor=.485/radius;verts=[(p[0]*factor,p[1]*factor,(p[2]-minz)*factor) for p in verts]
    height=max(p[2] for p in verts)
    if height>.76:verts=[(x,y,z*.76/height) for x,y,z in verts]
    mesh=bpy.data.meshes.new('FracturedSlab'+str(index));mesh.from_pydata(verts,[],faces);mesh.update()
    bm=bmesh.new();bm.from_mesh(mesh);bmesh.ops.recalc_face_normals(bm,faces=list(bm.faces));bmesh.ops.triangulate(bm,faces=list(bm.faces));bm.to_mesh(mesh);bm.free();mesh.update()
    attr=mesh.color_attributes.new(name='Color',type='FLOAT_COLOR',domain='POINT')
    for i,c in enumerate(colors):attr.data[i].color=c
    mesh.color_attributes.active_color=attr
    obj=bpy.data.objects.new('visual_rock_'+str(index),mesh);bpy.context.collection.objects.link(obj)
    mat=bpy.data.materials.new('StoneVertexPaint'+str(index));mat.use_nodes=True
    nodes=mat.node_tree.nodes;bsdf=nodes.get('Principled BSDF');bsdf.inputs['Roughness'].default_value=.94
    paint=nodes.new('ShaderNodeVertexColor');paint.layer_name='Color';gamma=nodes.new('ShaderNodeGamma');gamma.inputs['Gamma'].default_value=2.2
    mat.node_tree.links.new(paint.outputs['Color'],gamma.inputs['Color']);mat.node_tree.links.new(gamma.outputs['Color'],bsdf.inputs['Base Color']);mesh.materials.append(mat)
    return obj

def aim(obj,point):obj.rotation_euler=(Vector(point)-obj.location).to_track_quat('-Z','Y').to_euler()
def main():
    bpy.ops.object.select_all(action='SELECT');bpy.ops.object.delete(use_global=False)
    collection=bpy.data.collections.new('Editable fractured rock assets');bpy.context.scene.collection.children.link(collection)
    models=[];stats=[]
    for i in range(3):
        obj=make(i)
        for c in list(obj.users_collection):c.objects.unlink(obj)
        collection.objects.link(obj);bpy.ops.object.select_all(action='DESELECT');obj.select_set(True);bpy.context.view_layer.objects.active=obj
        bpy.ops.export_scene.gltf(filepath=str(ROOT/'assets'/'models'/f'visual-rock-{i}.glb'),export_format='GLB',use_selection=True,export_yup=True,export_vertex_color='ACTIVE',export_all_vertex_colors=True)
        count=len(obj.data.polygons);assert 80<=count<=250,count
        stats.append({'variant':i,'triangles':count,'height':max(v.co.z for v in obj.data.vertices),'radius':max(max(abs(v.co.x),abs(v.co.y)) for v in obj.data.vertices),'broad_components':2})
        models.append(obj)
    collection.hide_render=True;collection.hide_viewport=True
    studio=bpy.data.collections.new('Rock model study');bpy.context.scene.collection.children.link(studio)
    for i,obj in enumerate(models):
        inst=bpy.data.objects.new('Study '+obj.name,obj.data);studio.objects.link(inst);inst.location=((i-1)*1.38,0,0);inst.rotation_euler.z=math.radians(-7)
    bpy.ops.mesh.primitive_plane_add(size=200,location=(0,0,-.004));mat=bpy.data.materials.new('Study warm ground');mat.diffuse_color=(.29,.31,.27,1);bpy.context.object.data.materials.append(mat)
    bpy.ops.object.camera_add(location=(3.6,-6.0,3.2));camera=bpy.context.object;aim(camera,(0,0,.27));camera.data.type='ORTHO';camera.data.ortho_scale=4.75;bpy.context.scene.camera=camera
    for location,power,size,color in [((-3,-4,6),900,4,(1,.91,.78)),((4,-1,4),500,3,(.75,.86,1)),((0,4,5),1000,3,(1,.9,.76))]:
        bpy.ops.object.light_add(type='AREA',location=location);light=bpy.context.object;light.data.energy=power;light.data.size=size;light.data.color=color;aim(light,(0,0,.3))
    scene=bpy.context.scene;scene.render.engine='CYCLES';scene.cycles.samples=24;scene.cycles.use_denoising=True;scene.world.color=(.11,.13,.16);scene.view_settings.view_transform='Standard';scene.view_settings.look='Medium High Contrast';scene.view_settings.exposure=-1.1;scene.render.resolution_x=1500;scene.render.resolution_y=900;scene.render.resolution_percentage=100;scene.render.image_settings.file_format='PNG';scene.render.filepath=str(ROOT/'output'/'blender-rocks.png')
    bpy.ops.wm.save_as_mainfile(filepath=str(ROOT/'assets'/'models'/'visual-rocks.blend'))
    (ROOT/'output'/'blender-rocks-stats.json').write_text(json.dumps(stats,indent=2));bpy.ops.render.render(write_still=True);print('ROCK_STATS',json.dumps(stats))
if __name__ == '__main__':
    main()

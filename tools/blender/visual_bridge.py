"""Editable compact edge bridge, Blender source and same-mesh game export."""
import bpy, math, os
from mathutils import Vector
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '../..'))
bpy.ops.object.select_all(action='SELECT'); bpy.ops.object.delete(use_global=False)
vertices, faces, colors = [], [], []
def face(points, color):
    start=len(vertices); vertices.extend(points); faces.append(tuple(range(start,start+len(points)))); colors.extend([color]*len(points))
def prism(profile, width, color):
    # profile in X/Z; depth along Y
    front=[(x,-width/2,z) for x,z in profile]; back=[(x,width/2,z) for x,z in profile]
    face(list(reversed(front)),color); face(back,color)
    for i in range(len(profile)):
        j=(i+1)%len(profile); face([front[i],front[j],back[j],back[i]],color)
def block(x,y,z,sx,sy,sz,color):
    ps=[(x+a*sx/2,y+b*sy/2,z+c*sz/2) for a,b,c in [(-1,-1,-1),(1,-1,-1),(1,1,-1),(-1,1,-1),(-1,-1,1),(1,-1,1),(1,1,1),(-1,1,1)]]
    for ids in [(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7)]: face([ps[i] for i in ids],color)
STONE=(.68,.65,.55,0); LIGHT=(.82,.77,.64,0); DARK=(.46,.47,.43,0)
# A shallow masonry arch sits UNDER a nearly level road, not on top of it.
# Runtime aligns endpoint deck height .105 to actual bank ground + road lift.
def deck(x): return .105+.016*max(0,1-(x/.43)**2)
for i in range(10):
    a=math.pi*i/10; b=math.pi*(i+1)/10
    profile=[(.265*math.cos(a),.005+.064*math.sin(a)),(.265*math.cos(b),.005+.064*math.sin(b)),(.31*math.cos(b),.005+.092*math.sin(b)),(.31*math.cos(a),.005+.092*math.sin(a))]
    prism(profile,.26,LIGHT if i%3 else STONE)
    pa=(.31*math.cos(a),.005+.092*math.sin(a)); pb=(.31*math.cos(b),.005+.092*math.sin(b))
    prism([pa,pb,(pb[0],deck(pb[0])-.018),(pa[0],deck(pa[0])-.018)],.25,STONE)
for side in [-1,1]:
    block(side*.337,0,.047,.13,.275,.11,STONE)
for i in range(10):
    a=-.43+i*.086; b=a+.086
    prism([(a,deck(a)-.018),(b,deck(b)-.018),(b,deck(b)),(a,deck(a))],.25,LIGHT if i%3 else STONE)
    if i in (0,9):continue
    for y in [-.133,.133]:
        # Slender continuous parapets, only .039 high.
        topa=deck(a)+.039; topb=deck(b)+.039
        start=len(vertices)
        prism([(a,deck(a)),(b,deck(b)),(b,topb),(a,topa)],.025,STONE)
        for k in range(start,len(vertices)):
            x,yy,z=vertices[k];vertices[k]=(x,yy+y,z)
        block((a+b)/2,y,(topa+topb)/2,.085,.035,.010,LIGHT)
mesh=bpy.data.meshes.new('CompactBridgeMesh'); mesh.from_pydata(vertices,[],faces); mesh.update()
obj=bpy.data.objects.new('Compact river edge bridge',mesh); bpy.context.collection.objects.link(obj); bpy.context.view_layer.objects.active=obj; obj.select_set(True)
# Recalculate outward normals and triangulate for the checked build-time loader.
import bmesh
bm=bmesh.new(); bm.from_mesh(mesh); bmesh.ops.recalc_face_normals(bm,faces=list(bm.faces)); bmesh.ops.triangulate(bm,faces=list(bm.faces)); bm.to_mesh(mesh); bm.free()
ca=mesh.color_attributes.new(name='Color',type='FLOAT_COLOR',domain='POINT')
for d,col in zip(ca.data,colors): d.color=col
mat=bpy.data.materials.new('Weathered warm limestone'); mat.use_nodes=True
bs=mat.node_tree.nodes.get('Principled BSDF'); attr=mat.node_tree.nodes.new('ShaderNodeVertexColor'); attr.layer_name='Color'; gamma=mat.node_tree.nodes.new('ShaderNodeGamma'); gamma.inputs['Gamma'].default_value=2.2; mat.node_tree.links.new(attr.outputs['Color'],gamma.inputs['Color']); mat.node_tree.links.new(gamma.outputs['Color'],bs.inputs['Base Color']); bs.inputs['Roughness'].default_value=.82
mesh.materials.append(mat)
bpy.ops.export_scene.gltf(filepath=ROOT+'/assets/models/visual-bridge.glb',export_format='GLB',use_selection=True,export_apply=True)
# Studio scale study uses two hex outlines and reserved tile-centre footprints.
def material(name,color):
    m=bpy.data.materials.new(name); m.diffuse_color=(*color,1); m.use_nodes=True
    bs=m.node_tree.nodes.get('Principled BSDF'); bs.inputs['Base Color'].default_value=(*color,1); bs.inputs['Roughness'].default_value=.82
    return m
bpy.ops.mesh.primitive_plane_add(size=200,location=(0,0,-.07)); floor=bpy.context.object; floor.name='Studio ground'; floor.data.materials.append(material('Ground',(.23,.30,.27)))
for x in [-.8660254,.8660254]:
    curve=bpy.data.curves.new('Neighbour tile boundary','CURVE'); curve.dimensions='3D'; curve.bevel_depth=.005
    spline=curve.splines.new('POLY'); spline.points.add(6)
    for i,p in enumerate(spline.points):
        a=math.radians(-30+60*(i%6)); p.co=(x+math.cos(a),math.sin(a),-.045,1)
    ob=bpy.data.objects.new('Hex boundary',curve); bpy.context.collection.objects.link(ob); ob.data.materials.append(material('Edge',(.62,.73,.56)))
    bpy.ops.mesh.primitive_cylinder_add(vertices=32,radius=.40,depth=.015,location=(x,0,-.035)); bpy.context.object.name='Reserved centre: free for unit or improvement'; bpy.context.object.data.materials.append(material('Reserved centre',(.16,.37,.29)))
bpy.ops.mesh.primitive_plane_add(size=1,location=(0,0,-.012)); water=bpy.context.object; water.scale=(.46,1.4,1); water.name='River .46 wide'; water.data.materials.append(material('River',(.025,.39,.53)))
for x in [-.9,.9]:
    bpy.ops.mesh.primitive_cube_add(size=1,location=(x,0,.0085)); bank=bpy.context.object;bank.scale=(1.17,1.4,.157);bank.data.materials.append(material('River bank',(.25,.39,.14)))
bpy.ops.object.light_add(type='AREA',location=(-3,-4,6)); bpy.context.object.data.energy=450; bpy.context.object.data.shape='DISK'; bpy.context.object.data.size=4
bpy.ops.object.camera_add(location=(1.7,-2.7,2.0)); cam=bpy.context.object; cam.rotation_euler=(Vector((0,0,.08))-cam.location).to_track_quat('-Z','Y').to_euler(); cam.data.type='ORTHO'; cam.data.ortho_scale=2.2
sc=bpy.context.scene; sc.camera=cam; sc.render.engine='CYCLES'; sc.cycles.samples=32; sc.world.color=(.25,.25,.25); sc.render.resolution_x=1400; sc.render.resolution_y=900; sc.render.resolution_percentage=100
sc.view_settings.view_transform='AgX'; sc.render.image_settings.file_format='PNG'; sc.render.filepath=ROOT+'/output/blender-bridge.png'
bpy.ops.wm.save_as_mainfile(filepath=ROOT+'/assets/models/visual-bridge.blend'); bpy.ops.render.render(write_still=True)
print('BRIDGE',len(mesh.polygons),'triangles; footprint',max(abs(v.co.x) for v in mesh.vertices),max(abs(v.co.y) for v in mesh.vertices))

"""Connected terrain cross-section study; no gameplay hex determines mesh topology.
Run: blender --background --python tools/blender/terrain_surface_study.py
"""
import bpy,bmesh,math,random,pathlib,json
from mathutils import Vector
ROOT=pathlib.Path(__file__).resolve().parents[2]
R=random.Random(701)

def material(name,color,roughness=.9):
    m=bpy.data.materials.new(name);m.diffuse_color=(*color,1);m.use_nodes=True
    b=m.node_tree.nodes.get('Principled BSDF');b.inputs['Base Color'].default_value=(*color,1);b.inputs['Roughness'].default_value=roughness
    return m

def mesh(name,vertices,faces,materials,indices=None):
    m=bpy.data.meshes.new(name);m.from_pydata(vertices,[],faces);m.update();o=bpy.data.objects.new(name,m);bpy.context.collection.objects.link(o)
    for mat in materials:m.materials.append(mat)
    if indices:
        for p,i in zip(m.polygons,indices):p.material_index=i
    return o

def coast(y):return .65+.27*math.sin(y*1.42)+.16*math.sin(y*3.1+.5)
def river(y):return -1.55+.45*math.sin(y*1.2)+.28*math.sin(y*2.3+.5)
def centerline(t):
    # Mountain spring in the back-left, then mouth exits the right-hand coast.
    return (-1.45+2.55*t+.35*math.sin(t*math.pi*2),2.9-4.3*t+.45*math.sin(t*math.pi*3))
PATH=[centerline(i/200) for i in range(201)]
def river_info(x,y):
    best=(1e9,0)
    for i,(px,py) in enumerate(PATH):
        distance=(x-px)**2+(y-py)**2
        if distance<best[0]:best=(distance,i)
    return math.sqrt(best[0]),best[1]/200

def ground(x,y):
    shore=coast(y)-x
    hill=1.10*math.exp(-((x+2.5)**2/1.8+(y-1.5)**2/2.5))+.39*math.exp(-((x+2.8)**2/4+(y+1.8)**2/2))
    plateau=.22+.09*math.sin(y*.7)+hill
    cliff=.60+.25*math.sin(y*2.1)
    # Continuous field: sharp coast in the north, soft beaches to the south.
    coastal_rise=max(0.,min(1.,shore/(.17 if y>.4 else 1.05)))
    h=-.10+(plateau+.10)*coastal_rise
    d,t=river_info(x,y);bed=-.090+max(0.,.24*(1-t)**1.5-.024)
    blend=max(0.,min(1.,(d-.31)/.42));blend=blend*blend*(3-2*blend)
    if shore>-.20:h=bed+(h-bed)*blend
    if shore<0:h=-.10-min(.25,-shore*.14)
    return h

def aim(o,p):o.rotation_euler=(Vector(p)-o.location).to_track_quat('-Z','Y').to_euler()

def main():
    bpy.ops.object.select_all(action='SELECT');bpy.ops.object.delete(use_global=False)
    grass=[material('Grass facet '+str(i),c) for i,c in enumerate([(.34,.48,.15),(.37,.50,.16),(.38,.51,.17),(.35,.49,.15),(.39,.51,.18)])]
    stone=[material('Weathered coastal face '+str(i),c) for i,c in enumerate([(.43,.44,.38),(.52,.52,.44),(.60,.59,.48)])]
    sand=material('Warm sand',(.65,.57,.36));bed=material('River gravel',(.31,.37,.31));bottom=material('Seabed',(.17,.32,.30))
    # Irregular staggered shared lattice, 0.22 units on average. No duplicated tile fans.
    nx,ny=47,41;points=[]
    for j in range(ny):
        for i in range(nx):
            x=-4.7+i*.22+(.11 if j%2 else 0);y=-4.0+j*.20
            if i not in (0,nx-1) and j not in (0,ny-1):x+=R.uniform(-.065,.065);y+=R.uniform(-.055,.055)
            points.append((x,y,ground(x,y)))
    faces=[];indices=[]
    for j in range(ny-1):
        for i in range(nx-1):
            a=j*nx+i;b=a+1;c=a+nx;d=c+1
            triangles=[(a,b,c),(b,d,c)] if (i+j)%2 else [(a,b,d),(a,d,c)]
            for f in triangles:
                x=sum(points[k][0] for k in f)/3;y=sum(points[k][1] for k in f)/3;h=sum(points[k][2] for k in f)/3
                dist,t=river_info(x,y);shore=coast(y)-x
                if shore<-.1:idx=10
                elif dist<.28:idx=9
                elif shore<(.22 if y>.4 else 1.05):idx=5+R.randrange(3) if y>.4 else 8
                else:idx=R.randrange(5)
                faces.append(f);indices.append(idx)
    terrain=mesh('ONE connected irregular terrain',points,faces,grass+stone+[sand,bed,bottom],indices)
    watermat=material('River turquoise',(.028,.34,.40),.22);water=[];wf=[]
    for i in range(111):
        t=i/110;x,y=centerline(t);t2=min(1.,t+.003);t1=max(0.,t-.003);a=centerline(t1);b=centerline(t2);dx,dy=b[0]-a[0],b[1]-a[1];n=math.hypot(dx,dy);w=.26+.01*t
        level=.001+max(0.,.24*(1-t)**1.5-.024)
        water.extend([(x-dy/n*w,y+dx/n*w,level),(x+dy/n*w,y-dx/n*w,level)])
        if i:wf.extend([(2*i-2,2*i-1,2*i),(2*i-1,2*i+1,2*i)])
    waterobj=mesh('River fitted to carved terrain corridor',water,wf,[watermat]);waterobj.visible_shadow=False
    sea=[];sf=[];sm=[]
    seamats=[material('Sea shallow',(.028,.34,.40),.26),material('Sea medium',(.018,.29,.40),.25),material('Sea deep',(.022,.19,.31),.28)]
    for j in range(61):
        y=-4+j*.134;cx=coast(y)
        for k in range(15):sea.append((cx+(5.53-cx)*k/14,y,0.))
    for j in range(60):
        for k in range(14):
            a=j*15+k;sf.extend([(a,a+1,a+15),(a+1,a+16,a+15)]);sm.extend([0 if k<2 else 1 if k<6 else 2]*2)
    seaobj=mesh('Shallow to deep sea',sea,sf,seamats,sm)
    foammat=material('Broken white shore foam',(.81,.89,.79),.75);fp=[];ff=[]
    for j in range(135):
        y=-3.8+j*.056
        # Small interrupted strips follow the wet contour, away from the river mouth.
        if j%11 in (0,1,7) or river_info(coast(y),y)[0]<.32:continue
        for yy,off in [(y,.013),(y,.048),(y+.046,.048),(y+.046,.013)]:fp.append((coast(yy)+off,yy,.013))
        z=len(fp);ff.extend([(z-4,z-3,z-2),(z-4,z-2,z-1)])
    foam=mesh('Thin broken foam at contact contour',fp,ff,[foammat])
    # Small scale markers cast shadows onto the same ground; no asset/catalogue scope.
    trunkmat=material('Trunk',(.19,.13,.065));leafmat=material('Pine',(.10,.23,.09))
    for x,y in [(-3.7,.2),(-3.5,.7),(-3.2,-1.3),(-2.8,-2.2),(.15,1.5),(.15,2.3),(-3.7,2.4)]:
        z=ground(x,y);h=.40+R.random()*.22
        bpy.ops.mesh.primitive_cone_add(vertices=6,radius1=.055,radius2=.035,depth=.25,location=(x,y,z+.125));bpy.context.object.data.materials.append(trunkmat)
        for dz,r,depth in [(h*.6,.21,h*.8),(h*.92,.145,h*.72)]:
            bpy.ops.mesh.primitive_cone_add(vertices=7,radius1=r,radius2=.005,depth=depth,location=(x,y,z+dz));bpy.context.object.data.materials.append(leafmat)
    bpy.ops.object.light_add(type='SUN',location=(-3,-4,8));sun=bpy.context.object;sun.data.energy=2.4;sun.data.angle=.055;aim(sun,(1,1,0))
    bpy.ops.object.light_add(type='AREA',location=(0,0,7));bpy.context.object.data.energy=160;bpy.context.object.data.size=8
    bpy.ops.object.camera_add(location=(8,-11,9.5));camera=bpy.context.object;aim(camera,(-.3,0,.1));camera.data.type='ORTHO';camera.data.ortho_scale=13.6;bpy.context.scene.camera=camera
    scene=bpy.context.scene;scene.world.color=(.24,.28,.34);scene.render.engine='CYCLES';scene.cycles.samples=24;scene.cycles.use_denoising=True;scene.view_settings.view_transform='AgX';scene.render.resolution_x=1550;scene.render.resolution_y=1100;scene.render.resolution_percentage=100;scene.render.image_settings.file_format='PNG';scene.render.filepath=str(ROOT/'output/terrain-surface-study.png')
    bpy.ops.wm.save_as_mainfile(filepath=str(ROOT/'assets/models/terrain-surface-study.blend'));bpy.ops.render.render(write_still=True)
    # Separate diagnostic shows triangle topology on the exact same geometry.
    wiremat=material('Diagnostic edges',(.035,.055,.022));wire=terrain.copy();wire.data=terrain.data.copy();bpy.context.collection.objects.link(wire);wire.name='Diagnostic shared triangle edges';wire.location.z=.008;wire.data.materials.clear();wire.data.materials.append(wiremat)
    for p in wire.data.polygons:p.material_index=0
    mod=wire.modifiers.new('Fine topology wires','WIREFRAME');mod.thickness=.005;mod.use_replace=True
    scene.render.filepath=str(ROOT/'output/terrain-surface-study-topology.png');bpy.ops.render.render(write_still=True)
    from mathutils.bvhtree import BVHTree
    bvh=BVHTree.FromPolygons([Vector(p) for p in points],faces,all_triangles=True)
    penetrations=0
    water_checks=water+[tuple(sum(water[k][axis] for k in f)/3 for axis in range(3)) for f in wf]
    for x,y,z in water_checks:
        hit,normal,index,distance=bvh.ray_cast(Vector((x,y,10)),Vector((0,0,-1)))
        if hit and hit.z>z+.005:penetrations+=1
    stats={'water_surface_samples':len(water_checks),'water_samples_below_terrain':penetrations,'terrain_vertices':len(points),'terrain_triangles':len(faces),'river_triangles':len(wf),'sea_triangles':len(sf),'foam_triangles':len(ff),'topology':'shared staggered jittered lattice independent of logical hexes','render':'Blender Cycles offline study; not integrated into runtime'}
    (ROOT/'output/terrain-surface-study-stats.json').write_text(json.dumps(stats,indent=2));print('STUDY_STATS',stats)
main()

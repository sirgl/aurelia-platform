"""CDT terrain probe: constrained river banks and coast; unified river/ocean water.
API: https://docs.blender.org/api/5.1/mathutils.geometry.html
Run blender --background --python tools/blender/terrain_surface_study_v2.py
"""
import bpy,math,random,pathlib,json
from mathutils import Vector
from mathutils.geometry import delaunay_2d_cdt
ROOT=pathlib.Path(__file__).resolve().parents[2]
R=random.Random(720)
def mix(a,b,t):return tuple(x+(y-x)*t for x,y in zip(a,b))
def clamp(t):return max(0.,min(1.,t))
def coast(y):return .65+.27*math.sin(y*1.42)+.16*math.sin(y*3.1+.5)
def center(t):return Vector((-1.45+2.55*t+.35*math.sin(t*math.pi*2),2.9-4.3*t+.18*math.sin(t*math.pi*3)))
def width(t):return (.23+.035*math.sin(t*11))*(.23+.77*clamp(t/.12))
def level(t):return max(0.,.22*(1-t)**1.5-.03)
PATH=[center(i/100) for i in range(101)]
def river_info(p):
    best=(1e9,0.)
    for i in range(100):
        a,b=PATH[i],PATH[i+1];v=b-a;u=clamp((p-a).dot(v)/v.length_squared);d=(p-a-v*u).length
        if d<best[0]:best=(d,(i+u)/100)
    return best
LEFT=[];RIGHT=[]
for i in range(101):
    t=i/100;p=PATH[i];v=center(min(1.,t+.001))-center(max(0.,t-.001));v.normalize();n=Vector((-v.y,v.x))
    LEFT.append(p+n*width(t));RIGHT.append(p-n*width(t))
RIVER=LEFT+list(reversed(RIGHT))
def inside(p,polygon):
    result=False;j=len(polygon)-1
    for i,a in enumerate(polygon):
        b=polygon[j]
        if (a.y>p.y)!=(b.y>p.y) and p.x<(b.x-a.x)*(p.y-a.y)/(b.y-a.y)+a.x:result=not result
        j=i
    return result

def terrain_height(p):
    x,y=p;shore=coast(y)-x
    h=.23+.08*math.sin(y*.7)+1.1*math.exp(-((x+2.5)**2/1.8+(y-1.5)**2/2.5))+.3*math.exp(-((x+2.8)**2/4+(y+1.8)**2/2))
    if shore<0:h=-.025-.32*(1-math.exp(shore*.9))
    else:h*=clamp(shore/(.20 if y>.4 else 1.0))
    d,t=river_info(p);w=width(t)
    if d<w:h=min(h,level(t)-.014-.12*(1-(d/w)**2))
    elif d<w+.40:
        v=clamp((d-w)/.40);h=min(h,level(t)-.014+(h-level(t)+.014)*(v*v*(3-2*v)))
    return h

def mesh(name,verts,faces,colors):
    m=bpy.data.meshes.new(name);m.from_pydata(verts,[],faces);m.update();o=bpy.data.objects.new(name,m);bpy.context.collection.objects.link(o)
    a=m.color_attributes.new(name='Albedo',type='FLOAT_COLOR',domain='POINT')
    for i,c in enumerate(colors):a.data[i].color=(*c,1)
    mat=bpy.data.materials.new(name+' color');mat.use_nodes=True;nodes=mat.node_tree.nodes;bsdf=nodes.get('Principled BSDF');bsdf.inputs['Roughness'].default_value=.87
    paint=nodes.new('ShaderNodeVertexColor');paint.layer_name='Albedo';mat.node_tree.links.new(paint.outputs['Color'],bsdf.inputs['Base Color']);m.materials.append(mat)
    return o

def aim(o,p):o.rotation_euler=(Vector(p)-o.location).to_track_quat('-Z','Y').to_euler()
def main():
    bpy.ops.object.select_all(action='SELECT');bpy.ops.object.delete(use_global=False)
    coords=[];constraints=[];bank_input=set();coast_input=set()
    def line(points,closed=False,tag=None):
        start=len(coords);coords.extend(points)
        for i in range(len(points)-1):constraints.append((start+i,start+i+1))
        if closed:constraints.append((len(coords)-1,start))
        if tag is not None:tag.update(range(start,len(coords)))
    line([Vector((-4.7,-4)),Vector((5.53,-4)),Vector((5.53,4)),Vector((-4.7,4))],True)
    line(RIVER,True,bank_input)
    shoreline=[Vector((coast(-4+i*.08),-4+i*.08)) for i in range(101)]
    line(shoreline,tag=coast_input)
    line([Vector((p.x-(.22 if p.y>.4 else .9),p.y)) for p in shoreline])
    # Jitter samples provide an isotropic low-poly field; constraints own banks.
    for j in range(38):
        for i in range(47):
            p=Vector((-4.6+i*.215+R.uniform(-.07,.07),-3.9+j*.21+R.uniform(-.07,.07)))
            coords.append(p)
    verts,edges,faces,orig_v,orig_e,orig_f=delaunay_2d_cdt(coords,constraints,[],0,1e-6,True)
    waterfaces=[]
    for f in faces:
        p=sum((verts[k] for k in f),Vector((0.,0.)))/len(f)
        if p.x>=coast(p.y) or inside(p,RIVER):waterfaces.append(f)
    water_ids={k for f in waterfaces for k in f};landfaces=[f for f in faces if f not in waterfaces]
    water_edges={}
    for f in waterfaces:
        for a,b in zip(f,f[1:]+f[:1]):
            key=tuple(sorted((a,b)));water_edges[key]=water_edges.get(key,0)+1
    contact_ids={k for edge,count in water_edges.items() if count==1 for k in edge}
    heights=[];waterheights=[];colors=[];watercolors=[]
    for i,p in enumerate(verts):
        d,t=river_info(p);h=terrain_height(p);shore=coast(p.y)-p.x
        # Both surfaces use one constrained vertex at the shoreline.
        if any(k in bank_input for k in orig_v[i]) and shore>0:h=level(t)-.014
        if any(k in coast_input for k in orig_v[i]):h=-.014
        is_bank=any(k in bank_input for k in orig_v[i])
        is_coast=any(k in coast_input for k in orig_v[i])
        wh=level(t) if (inside(p,RIVER) or is_bank) and shore>0 and not is_coast else 0.
        if i in contact_ids:h=wh  # Exact same boundary elevation; only 0.003 render bias.
        elif i in water_ids:h=min(h,wh-.014)
        heights.append(h);waterheights.append(wh)
        if shore<0:c=(.16,.29,.25)
        elif d<width(t)+.08:c=(.37,.43,.23)
        elif shore<(.235 if p.y>.4 else .95):c=(.44,.45,.36) if p.y>.4 else (.72,.58,.32)
        else:
            tone=.94+R.random()*.10;c=tuple(v*tone for v in (.25,.47,.065))
        colors.append(c)
        depth=max(0.,-h);distance=max(0.,-shore);blend=1-math.exp(-distance*.85-depth*.45)
        c=mix((.025,.42,.40),(.012,.13,.28),blend)
        facet=.985+R.random()*.03;watercolors.append(tuple(v*facet for v in c))
    terrain=mesh('CDT shared terrain bank and seabed',[(p.x,p.y,h) for p,h in zip(verts,heights)],faces,colors)
    water=mesh('ONE continuous river and ocean surface',[(p.x,p.y,z+.003) for p,z in zip(verts,waterheights)],waterfaces,watercolors)
    water.visible_shadow=False;water.data.materials[0].node_tree.nodes.get('Principled BSDF').inputs['Roughness'].default_value=.31
    # Natural short tapered crests, not repeated rectangular marks.
    fp=[];ff=[];fc=[];y=-3.85
    while y<3.8:
        length=min(R.uniform(.14,.59),3.82-y);gap=R.uniform(.035,.23);steps=max(4,int(length/.025))
        if river_info(Vector((coast(y+length*.5),y+length*.5)))[0]>.40:
            start=len(fp)
            for j in range(steps+1):
                u=j/steps;yy=y+length*u;width0=.009+.025*math.sin(math.pi*u)**.7;offset=.009+.008*math.sin(yy*19)
                for off in [offset,offset+width0]:fp.append((coast(yy)+off,yy,.009));fc.append((.77,.88,.76))
                if j:idx=start+2*j;ff.extend([(idx-2,idx-1,idx),(idx-1,idx+1,idx)])
        y+=length+gap
    mesh('Irregular tapered shoreline wash',fp,ff,fc)
    for x,y in [(-3.7,.2),(-3.5,.7),(-3.2,-1.3),(-2.8,-2.2),(.15,1.5),(.15,2.3),(-3.7,2.4)]:
        h=terrain_height(Vector((x,y)))
        bpy.ops.mesh.primitive_cone_add(vertices=7,radius1=.18,radius2=.0,depth=.62,location=(x,y,h+.32));o=bpy.context.object
        mat=bpy.data.materials.new('Scale pine');mat.use_nodes=True;mat.node_tree.nodes.get('Principled BSDF').inputs['Base Color'].default_value=(.065,.22,.045,1);o.data.materials.append(mat)
    bpy.ops.object.light_add(type='SUN',location=(-3,-4,8));sun=bpy.context.object;sun.data.energy=2.8;sun.data.angle=.05;sun.data.color=(1.,.9,.73);aim(sun,(1,1,0))
    bpy.ops.object.light_add(type='AREA',location=(1,-2,6));bpy.context.object.data.energy=280;bpy.context.object.data.size=8
    bpy.ops.object.camera_add(location=(8,-11,9.5));camera=bpy.context.object;aim(camera,(-.3,0,.1));camera.data.type='ORTHO';camera.data.ortho_scale=13.6;bpy.context.scene.camera=camera
    scene=bpy.context.scene;scene.world.color=(.27,.32,.38);scene.render.engine='CYCLES';scene.cycles.samples=32;scene.cycles.use_denoising=True;scene.view_settings.view_transform='AgX';scene.view_settings.exposure=.25;scene.render.resolution_x=1550;scene.render.resolution_y=1100;scene.render.resolution_percentage=100;scene.render.image_settings.file_format='PNG';scene.render.filepath=str(ROOT/'output/terrain-surface-study-v2.png')
    bpy.ops.wm.save_as_mainfile(filepath=str(ROOT/'assets/models/terrain-surface-study-v2.blend'));bpy.ops.render.render(write_still=True)
    wire=mesh('Diagnostic constrained topology',[(p.x,p.y,h+.005) for p,h in zip(verts,heights)],landfaces,[(.035,.055,.017)]*len(verts))
    mod=wire.modifiers.new('Topology wire','WIREFRAME');mod.thickness=.004;mod.use_replace=True
    scene.render.filepath=str(ROOT/'output/terrain-surface-study-v2-topology.png');bpy.ops.render.render(write_still=True)
    adj=[set() for f in waterfaces];owners={}
    for i,f in enumerate(waterfaces):
        for a,b in zip(f,f[1:]+f[:1]):
            key=tuple(sorted((a,b)))
            if key in owners:other=owners[key];adj[i].add(other);adj[other].add(i)
            else:owners[key]=i
    seen=set();component_sizes=[]
    for i in range(len(waterfaces)):
        if i in seen:continue
        todo=[i];count=0
        while todo:
            f=todo.pop()
            if f in seen:continue
            seen.add(f);count+=1;todo.extend(adj[f]-seen)
        component_sizes.append(count)
    clearance=min(waterheights[k]+.003-heights[k] for k in water_ids)
    stats={'water_connected_components':component_sizes,'minimum_exact_water_clearance':clearance,'maximum_boundary_contact_bias':max(abs(waterheights[k]+.003-heights[k]) for k in contact_ids),'terrain_vertices':len(verts),'terrain_triangles':len(faces),'unified_water_triangles':len(waterfaces),'foam_triangles':len(ff),'input_constraint_edges':len(constraints),'constraint_output_edges':sum(bool(v) for v in orig_e),'water_components':'river and sea selected from same constrained triangulation; shared mouth vertices','method':'Blender mathutils.geometry.delaunay_2d_cdt output_type=0 epsilon=1e-6','runtime_delivered':False}
    (ROOT/'output/terrain-surface-study-v2-stats.json').write_text(json.dumps(stats,indent=2));print('CDT_STATS',stats)
main()

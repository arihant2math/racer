"""Silhouette IoU of the LOD0 meshes against blueprint.json (side, plan, front, rear).

    blender -b <model.blend> --python iou.py -- <refs/blueprint.json>
"""
import json
import sys

import bpy
import numpy as np

bp = json.load(open(sys.argv[sys.argv.index("--") + 1]))
objs=[o for o in bpy.data.objects if o.type=='MESH' and o.users_collection[0].name=='LOD0']
dg=bpy.context.evaluated_depsgraph_get()
P=[]
for o in objs:
    if 'interior' in o.name: continue
    me=o.evaluated_get(dg).to_mesh(); M=o.matrix_world
    for t in me.loop_triangles:
        P.append([tuple(M@me.vertices[i].co) for i in t.vertices])
P=np.array(P)
def raster(tris2d, ext, res=0.005):
    (u0,u1),(v0,v1)=ext; W=int((u1-u0)/res); H=int((v1-v0)/res)
    img=np.zeros((H,W),bool)
    for tri in tris2d:
        a=(tri-[u0,v0])/res
        mn=np.floor(a.min(0)).astype(int); mx=np.ceil(a.max(0)).astype(int)
        mn=np.maximum(mn,0); mx=np.minimum(mx,[W-1,H-1])
        if (mx<mn).any(): continue
        xs,ys=np.meshgrid(np.arange(mn[0],mx[0]+1)+.5,np.arange(mn[1],mx[1]+1)+.5)
        p=np.stack([xs,ys],-1)
        (x1,y1),(x2,y2),(x3,y3)=a
        d=(y2-y3)*(x1-x3)+(x3-x2)*(y1-y3)
        if abs(d)<1e-12: continue
        l1=((y2-y3)*(p[...,0]-x3)+(x3-x2)*(p[...,1]-y3))/d
        l2=((y3-y1)*(p[...,0]-x3)+(x1-x3)*(p[...,1]-y3))/d
        m=(l1>=-1e-3)&(l2>=-1e-3)&(1-l1-l2>=-1e-3)
        img[mn[1]:mx[1]+1,mn[0]:mx[0]+1]|=m
    return img
def bp_mask(view, ext, res=0.005):
    (u0,u1),(v0,v1)=ext; W=int((u1-u0)/res); H=int((v1-v0)/res)
    us=u0+(np.arange(W)+.5)*res; vs=v0+(np.arange(H)+.5)*res
    if view=='side':
        x=np.array(bp['side']['x']); top=np.interp(us,x,bp['side']['z_top'],left=-1,right=-1); bot=np.interp(us,x,bp['side']['z_bottom'],left=9,right=9)
        return (vs[:,None]<=top[None,:])&(vs[:,None]>=bot[None,:])
    if view=='plan':
        x=np.array(bp['plan']['x']); hw=np.interp(us,x,bp['plan']['half_width'],left=-1,right=-1)
        return np.abs(vs[:,None])<=hw[None,:]
    z=np.array(bp[view]['z']); o=np.argsort(z); hw=np.interp(vs,z[o],np.array(bp[view]['half_width'])[o],left=-1,right=-1)
    return np.abs(us[None,:])<=hw[:,None]
res={}
for view,(proj,ext) in {'side':((1,2),((-2.35,2.35),(-0.05,1.4))),'plan':((1,0),((-2.35,2.35),(-1.1,1.1))),'front':((0,2),((-1.1,1.1),(-0.05,1.4))),'rear':((0,2),((-1.1,1.1),(-0.05,1.4)))}.items():
    t=P[:,:,list(proj)].copy()
    if view=='plan': t[:,:,1]*=-1
    m=raster(t,ext); b=bp_mask(view,ext)
    res[view]=round(float((m&b).sum()/(m|b).sum()),4)
print('IOU',res)

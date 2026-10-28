"""Blender: turn a body grid (.npz from body_grid.py) into a closed mirrored mesh.

Blender frame: X right, Y forward, Z up (exports to glTF/Bevy as -Z forward, +Y up).
"""
import bpy
import numpy as np


def grid_mesh(npz_path, name="body_shell", stride=(1, 1)):
    g = np.load(npz_path)
    X, Y, Z = g["X"][:: stride[0], :: stride[1]], g["Y"][:: stride[0], :: stride[1]], g["Z"][:: stride[0], :: stride[1]]
    ns, nt = X.shape
    # Left half (grid y > 0 is left -> Blender -X), then right half mirrored.
    verts, faces = [], []
    idx = {}

    def vid(i, k, side):
        # Shared seam vertices on the centre line (theta = 0 and theta = pi).
        if k == 0 or k == nt - 1:
            side = 0
        key = (i, k, side)
        if key not in idx:
            sgn = -1.0 if side >= 0 else 1.0
            idx[key] = len(verts)
            verts.append((sgn * Y[i, k] if side != 0 else 0.0, X[i, k], Z[i, k]))
        return idx[key]

    for side in (1, -1):
        for i in range(ns - 1):
            for k in range(nt - 1):
                q = [vid(i, k, side), vid(i + 1, k, side), vid(i + 1, k + 1, side), vid(i, k + 1, side)]
                faces.append(q if side == 1 else q[::-1])
    # End caps: fan-close the first and last stations.
    for i, flip in ((0, True), (ns - 1, False)):
        ring = [vid(i, k, 1) for k in range(nt)] + [vid(i, k, -1) for k in range(nt - 2, 0, -1)]
        c = len(verts)
        verts.append((0.0, X[i, 0], float(np.mean(Z[i]))))
        for a, b in zip(ring, ring[1:] + ring[:1]):
            faces.append((c, b, a) if flip else (c, a, b))
    me = bpy.data.meshes.new(name)
    me.from_pydata(verts, [], faces)
    me.validate()
    me.update()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    for p in me.polygons:
        p.use_smooth = True
    return ob

"""Small bpy/bmesh helpers shared by the car build scripts."""

import math

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector


def clear_scene():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def material(name, color, metallic=0.0, roughness=0.5, alpha=1.0, emission=None,
             emission_strength=0.0, transmission=0.0, coat=0.0):
    m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    m.use_nodes = True
    bsdf = m.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Base Color"].default_value = (*color, 1.0)
    bsdf.inputs["Metallic"].default_value = metallic
    bsdf.inputs["Roughness"].default_value = roughness
    bsdf.inputs["Alpha"].default_value = alpha
    if coat:
        bsdf.inputs["Coat Weight"].default_value = coat
        bsdf.inputs["Coat Roughness"].default_value = 0.03
    if transmission:
        bsdf.inputs["Transmission Weight"].default_value = transmission
    if emission is not None:
        bsdf.inputs["Emission Color"].default_value = (*emission, 1.0)
        bsdf.inputs["Emission Strength"].default_value = emission_strength
    if alpha < 1.0:
        try:
            m.surface_render_method = "BLENDED"
        except AttributeError:
            m.blend_method = "BLEND"
    m.diffuse_color = (*color, alpha)
    return m


def mesh_object(name, verts, faces, uvs=None, mat_index=None, materials=(), smooth=True,
                face_ints=None, collection=None):
    """Object from numpy verts (V, 3) and a list of faces (tuples of indices).

    uvs: per face-corner UVs in the same order as faces (list of (u, v) lists).
    face_ints: {name: array per face} stored as integer face attributes.
    """
    me = bpy.data.meshes.new(name)
    flat = [i for f in faces for i in f]
    loop_start = np.cumsum([0] + [len(f) for f in faces[:-1]])
    loop_total = [len(f) for f in faces]
    me.vertices.add(len(verts))
    me.vertices.foreach_set("co", np.asarray(verts, np.float32).ravel())
    me.loops.add(len(flat))
    me.loops.foreach_set("vertex_index", np.asarray(flat, np.int32))
    me.polygons.add(len(faces))
    me.polygons.foreach_set("loop_start", np.asarray(loop_start, np.int32))
    me.polygons.foreach_set("loop_total", np.asarray(loop_total, np.int32))
    if uvs is not None:
        uv = me.uv_layers.new(name="UVMap")
        uv.data.foreach_set("uv", np.asarray([c for f in uvs for c in f], np.float32).ravel())
    if mat_index is not None:
        me.polygons.foreach_set("material_index", np.asarray(mat_index, np.int32))
    if face_ints:
        for k, v in face_ints.items():
            a = me.attributes.new(k, "INT", "FACE")
            a.data.foreach_set("value", np.asarray(v, np.int32))
    me.update(calc_edges=True)
    me.validate(clean_customdata=False)
    for m in materials:
        me.materials.append(m)
    ob = bpy.data.objects.new(name, me)
    (collection or bpy.context.scene.collection).objects.link(ob)
    if smooth:
        me.shade_smooth()
    return ob


def bm_to_object(name, bm, materials=(), smooth=True, collection=None):
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    for m in materials:
        me.materials.append(m)
    ob = bpy.data.objects.new(name, me)
    (collection or bpy.context.scene.collection).objects.link(ob)
    if smooth:
        me.shade_smooth()
    return ob


def set_sharp_by_angle(ob, degrees):
    me = ob.data
    me.shade_smooth()
    try:
        me.set_sharp_from_angle(angle=math.radians(degrees))
    except AttributeError:
        me.use_auto_smooth = True
        me.auto_smooth_angle = math.radians(degrees)


def box_uv(ob, scale=1.0):
    """Box-projected UVs (per face, by dominant normal axis), in metres * scale."""
    me = ob.data
    uv = me.uv_layers.get("UVMap") or me.uv_layers.new(name="UVMap")
    co = np.zeros(len(me.vertices) * 3, np.float32)
    me.vertices.foreach_get("co", co)
    co = co.reshape(-1, 3)
    data = np.zeros((len(me.loops), 2), np.float32)
    for p in me.polygons:
        n = p.normal
        ax = int(np.argmax(np.abs([n.x, n.y, n.z])))
        a, b = [(1, 2), (0, 2), (0, 1)][ax]
        for li in p.loop_indices:
            v = co[me.loops[li].vertex_index]
            data[li] = (v[a] * scale, v[b] * scale)
    uv.data.foreach_set("uv", data.ravel())


def apply_modifiers(ob):
    bpy.context.view_layer.objects.active = ob
    for m in list(ob.modifiers):
        with bpy.context.temp_override(object=ob, active_object=ob):
            bpy.ops.object.modifier_apply(modifier=m.name)


def boolean(target, cutter, operation="DIFFERENCE", solver="EXACT"):
    mod = target.modifiers.new("bool", "BOOLEAN")
    mod.operation = operation
    mod.object = cutter
    mod.solver = solver
    if solver == "EXACT":
        mod.use_hole_tolerant = True
    apply_modifiers(target)


def revolve(profile, segments, axis="X", u_scale=1.0, closed_profile=False):
    """Surface of revolution of a profile [(r, a), ...] (a along the axis).

    Returns verts, faces, uvs. u runs around (0..1)*u_scale, v along the profile
    arc length.
    """
    prof = np.asarray(profile, float)
    n = len(prof)
    ang = np.linspace(0, 2 * np.pi, segments, endpoint=False)
    verts = []
    for th in ang:
        for r, a in prof:
            if axis == "X":
                verts.append((a, r * math.cos(th), r * math.sin(th)))
            else:
                verts.append((r * math.cos(th), r * math.sin(th), a))
    seg = np.linalg.norm(np.diff(prof, axis=0), axis=1)
    vcoord = np.concatenate([[0], np.cumsum(seg)])
    faces, uvs = [], []
    m = n if closed_profile else n - 1
    for i in range(segments):
        i2 = (i + 1) % segments
        for j in range(m):
            j2 = (j + 1) % n
            faces.append((i * n + j, i2 * n + j, i2 * n + j2, i * n + j2))
            u0, u1 = i / segments * u_scale, (i + 1) / segments * u_scale
            v0, v1 = vcoord[j], vcoord[j2] if j2 else vcoord[-1] + seg[-1] * 0
            uvs.append([(u0, v0), (u1, v0), (u1, v1), (u0, v1)])
    return np.asarray(verts), faces, uvs


def ellipsoid(center, radii, nu=32, nv=16):
    verts, faces, uvs = [], [], []
    for j in range(nv + 1):
        th = math.pi * j / nv
        for i in range(nu):
            ph = 2 * math.pi * i / nu
            verts.append((center[0] + radii[0] * math.sin(th) * math.cos(ph),
                          center[1] + radii[1] * math.sin(th) * math.sin(ph),
                          center[2] + radii[2] * math.cos(th)))
    for j in range(nv):
        for i in range(nu):
            i2 = (i + 1) % nu
            faces.append((j * nu + i, j * nu + i2, (j + 1) * nu + i2, (j + 1) * nu + i))
            uvs.append([(i / nu, j / nv), ((i + 1) / nu, j / nv), ((i + 1) / nu, (j + 1) / nv), (i / nu, (j + 1) / nv)])
    return np.asarray(verts), faces, uvs


def rounded_box(name, size, center, radius=0.01, materials=(), mat=0, segments=2, collection=None):
    """Bevelled box object (size full extents)."""
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    for v in bm.verts:
        v.co = Vector((v.co.x * size[0], v.co.y * size[1], v.co.z * size[2])) + Vector(center)
    if radius > 0:
        bmesh.ops.bevel(bm, geom=list(bm.edges), offset=min(radius, min(size) * 0.45),
                        segments=segments, affect="EDGES", profile=0.5)
    for f in bm.faces:
        f.material_index = mat
    ob = bm_to_object(name, bm, materials, smooth=True, collection=collection)
    bm.free()
    set_sharp_by_angle(ob, 40)
    box_uv(ob)
    return ob


def join(objects, name):
    """Joins objects into the first one and renames it."""
    objects = [o for o in objects if o is not None]
    if not objects:
        return None
    base = objects[0]
    if len(objects) > 1:
        with bpy.context.temp_override(active_object=base, selected_editable_objects=objects,
                                       selected_objects=objects):
            bpy.ops.object.join()
    base.name = name
    base.data.name = name
    return base


def transform(ob, matrix):
    ob.data.transform(matrix)
    ob.data.update()


def empty(name, location, rotation=(0, 0, 0), size=0.1, collection=None):
    e = bpy.data.objects.new(name, None)
    e.empty_display_type = "ARROWS"
    e.empty_display_size = size
    e.location = location
    e.rotation_euler = rotation
    (collection or bpy.context.scene.collection).objects.link(e)
    return e


def look_rotation(forward, up=(0, 0, 1)):
    """Euler for an object whose -Z looks along `forward` (camera/light convention)."""
    f = Vector(forward).normalized()
    return f.to_track_quat("-Z", "Y").to_euler()


def tri_count(ob):
    return sum(len(p.vertices) - 2 for p in ob.data.polygons)


def mat_identity():
    return Matrix.Identity(4)

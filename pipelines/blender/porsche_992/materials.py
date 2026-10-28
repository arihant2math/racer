"""PBR materials for the car (glTF-exportable Principled BSDF setups).

Every material uses only Principled BSDF inputs plus image textures, which
the glTF exporter maps to core PBR + KHR_materials_clearcoat /
KHR_materials_transmission / KHR_materials_ior / KHR_materials_specular /
KHR_materials_emissive_strength. Names are stable: the game looks some of
them up (paint, glass, lights) to swap in its own materials.
"""
import bpy


def _principled(name, color, metallic=0.0, roughness=0.5, **kw):
    m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    b = nt.nodes.get("Principled BSDF")
    b.inputs["Base Color"].default_value = (*color, 1.0)
    b.inputs["Metallic"].default_value = metallic
    b.inputs["Roughness"].default_value = roughness
    for k, v in kw.items():
        b.inputs[k].default_value = v
    m.diffuse_color = (*color, 1.0)  # workbench / viewport preview
    if kw.get("Transmission Weight", 0) > 0.5:
        m.diffuse_color = (0.05, 0.06, 0.07, 1.0)
    m.metallic = metallic
    m.roughness = roughness
    return m


def _image_node(m, img, socket, non_color=False, alpha_socket=None):
    nt = m.node_tree
    b = nt.nodes.get("Principled BSDF")
    t = nt.nodes.new("ShaderNodeTexImage")
    t.image = img
    if non_color:
        img.colorspace_settings.name = "Non-Color"
    nt.links.new(t.outputs["Color"], b.inputs[socket])
    if alpha_socket:
        nt.links.new(t.outputs["Alpha"], b.inputs[alpha_socket])
    return t


def _normal_map(m, img, strength=1.0):
    nt = m.node_tree
    b = nt.nodes.get("Principled BSDF")
    t = nt.nodes.new("ShaderNodeTexImage")
    t.image = img
    img.colorspace_settings.name = "Non-Color"
    n = nt.nodes.new("ShaderNodeNormalMap")
    n.inputs["Strength"].default_value = strength
    nt.links.new(t.outputs["Color"], n.inputs["Color"])
    nt.links.new(n.outputs["Normal"], b.inputs["Normal"])


def build(textures):
    """textures: dict of bpy images from textures.py."""
    M = {}
    # Body paint: GT Silver-ish white? Use Carrara White Metallic default, game swaps colour.
    M["paint"] = _principled(
        "car_paint", (0.80, 0.80, 0.78), metallic=0.35, roughness=0.32,
        **{"Coat Weight": 1.0, "Coat Roughness": 0.03, "Coat IOR": 1.5},
    )
    M["paint_dark"] = _principled("trim_gloss_black", (0.012, 0.012, 0.013), metallic=0.0, roughness=0.12,
                                  **{"Coat Weight": 1.0, "Coat Roughness": 0.02})
    M["trim_satin"] = _principled("trim_satin_black", (0.02, 0.02, 0.022), roughness=0.45)
    M["plastic"] = _principled("plastic_textured_black", (0.025, 0.025, 0.027), roughness=0.75)
    _normal_map(M["plastic"], textures["grain_n"], 0.4)
    M["grille"] = _principled("grille_mesh", (0.02, 0.02, 0.022), roughness=0.6)
    _image_node(M["grille"], textures["grille_mask"], "Alpha", non_color=True)
    M["grille"].node_tree.links.new(
        M["grille"].node_tree.nodes["Image Texture"].outputs["Color"],
        M["grille"].node_tree.nodes["Principled BSDF"].inputs["Alpha"],
    )
    M["grille"].blend_method = "CLIP" if hasattr(M["grille"], "blend_method") else None
    M["chrome"] = _principled("chrome", (0.95, 0.95, 0.95), metallic=1.0, roughness=0.05)
    M["alu_brushed"] = _principled("aluminium_brushed", (0.85, 0.86, 0.87), metallic=1.0, roughness=0.28)
    M["exhaust"] = _principled("exhaust_steel", (0.72, 0.72, 0.72), metallic=1.0, roughness=0.18)
    M["exhaust_inner"] = _principled("exhaust_soot", (0.015, 0.014, 0.013), roughness=0.9)
    # Glass
    M["glass"] = _principled(
        "glass", (0.92, 0.94, 0.95), roughness=0.0,
        **{"Transmission Weight": 1.0, "IOR": 1.52, "Alpha": 0.25},
    )
    M["glass_tint"] = _principled(
        "glass_privacy", (0.35, 0.38, 0.4), roughness=0.0,
        **{"Transmission Weight": 1.0, "IOR": 1.52, "Alpha": 0.55},
    )
    M["frit"] = _principled("glass_frit_black", (0.01, 0.01, 0.01), roughness=0.35)
    M["lens_clear"] = _principled(
        "lens_clear", (0.95, 0.95, 0.95), roughness=0.02,
        **{"Transmission Weight": 1.0, "IOR": 1.49, "Alpha": 0.2},
    )
    M["lens_red"] = _principled(
        "lens_red", (0.55, 0.02, 0.02), roughness=0.05,
        **{"Transmission Weight": 0.6, "IOR": 1.49, "Alpha": 0.85},
    )
    M["lens_smoked"] = _principled(
        "lens_smoked_red", (0.18, 0.01, 0.012), roughness=0.04,
        **{"Coat Weight": 1.0, "Coat Roughness": 0.02},
    )
    M["headlight_chrome"] = _principled("headlight_reflector", (0.9, 0.9, 0.92), metallic=1.0, roughness=0.12)
    M["headlight_inner"] = _principled("headlight_housing", (0.04, 0.042, 0.045), metallic=0.6, roughness=0.35)
    # Emissive (game toggles strengths; names matter)
    M["drl"] = _principled("light_drl", (1, 1, 1), roughness=0.3,
                           **{"Emission Color": (1.0, 0.98, 0.95, 1), "Emission Strength": 6.0})
    M["headlamp"] = _principled("light_headlamp", (0.9, 0.9, 0.9), roughness=0.2,
                                **{"Emission Color": (1.0, 0.97, 0.92, 1), "Emission Strength": 0.0})
    M["tail"] = _principled("light_tail", (0.35, 0.0, 0.0), roughness=0.25,
                            **{"Emission Color": (1.0, 0.02, 0.01, 1), "Emission Strength": 4.0})
    M["brake"] = _principled("light_brake", (0.35, 0.0, 0.0), roughness=0.25,
                             **{"Emission Color": (1.0, 0.01, 0.0, 1), "Emission Strength": 0.0})
    M["indicator"] = _principled("light_indicator", (0.6, 0.35, 0.05), roughness=0.25,
                                 **{"Emission Color": (1.0, 0.45, 0.02, 1), "Emission Strength": 0.0})
    M["reverse"] = _principled("light_reverse", (0.8, 0.8, 0.8), roughness=0.25,
                               **{"Emission Color": (1, 1, 1, 1), "Emission Strength": 0.0})
    # Wheels
    M["rim"] = _principled("wheel_rim_silver", (0.62, 0.63, 0.65), metallic=1.0, roughness=0.3,
                           **{"Coat Weight": 0.6, "Coat Roughness": 0.08})
    M["tyre"] = _principled("tyre_rubber", (0.022, 0.022, 0.022), roughness=0.82)
    _image_node(M["tyre"], textures["tyre_col"], "Base Color")
    _normal_map(M["tyre"], textures["tyre_n"], 1.0)
    M["disc"] = _principled("brake_disc", (0.42, 0.42, 0.42), metallic=1.0, roughness=0.42)
    _image_node(M["disc"], textures["disc_col"], "Base Color")
    M["caliper"] = _principled("brake_caliper_red", (0.55, 0.015, 0.02), roughness=0.3,
                               **{"Coat Weight": 1.0, "Coat Roughness": 0.05})
    M["hub"] = _principled("hub_steel", (0.3, 0.3, 0.3), metallic=1.0, roughness=0.5)
    # Badges / decals
    M["badge"] = _principled("badge_crest", (1, 1, 1), metallic=0.8, roughness=0.25)
    _image_node(M["badge"], textures["crest"], "Base Color")
    M["lettering"] = _principled("lettering_porsche", (0.02, 0.02, 0.02), metallic=0.2, roughness=0.3)
    _image_node(M["lettering"], textures["lettering"], "Base Color", alpha_socket="Alpha")
    M["lettering"].blend_method = "CLIP" if hasattr(M["lettering"], "blend_method") else None
    M["plate"] = _principled("licence_plate", (1, 1, 1), roughness=0.4)
    _image_node(M["plate"], textures["plate"], "Base Color")
    # Interior / underside
    M["leather"] = _principled("interior_leather", (0.03, 0.028, 0.027), roughness=0.55)
    _normal_map(M["leather"], textures["grain_n"], 0.6)
    M["interior_trim"] = _principled("interior_trim", (0.05, 0.05, 0.052), metallic=0.2, roughness=0.35)
    M["screen"] = _principled("interior_screen", (0.0, 0.0, 0.0), roughness=0.05,
                              **{"Emission Color": (0.25, 0.35, 0.5, 1), "Emission Strength": 0.6})
    M["under"] = _principled("underbody", (0.03, 0.03, 0.03), roughness=0.85)
    M["engine_grille"] = _principled("engine_grille_slats", (0.015, 0.015, 0.016), roughness=0.5)
    # Glass alpha in glTF: use BLEND only where needed.
    for k in ("glass", "glass_tint", "lens_clear", "lens_red"):
        mat = M[k]
        if hasattr(mat, "surface_render_method"):
            mat.surface_render_method = "BLENDED"
    return M

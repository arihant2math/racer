// Ground surfaces of the city kit (road, kerbs, sidewalks, road decals):
// StandardMaterial plus procedural road markings, tyre-worn wheel tracks, a
// concrete gutter pan and the global wetness model from PLAN.md section 6
// (darker albedo, lower roughness, puddles that flatten the normal map).
//
// Mesh UV0 on roads is (lateral offset from the centreline, arc length) in
// metres; the base material's uv_transform scales it for the tiling textures.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}

struct SurfaceParams {
    wetness: f32,
    puddles: f32,
    mode: u32,
    lanes_per_side: u32,
    lane_width: f32,
    carriage_half: f32,
    flags: u32,
    seed: f32,
    // (s0, s1, kind, half width): 1 crosswalk, 2 stop line, 3 chequered line
    crossings: array<vec4<f32>, 4>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> surface: SurfaceParams;

const MODE_ROAD: u32 = 0u;
const MODE_PLAIN: u32 = 1u;
const MODE_DECAL: u32 = 2u;

const FLAG_CENTRE: u32 = 1u;
const FLAG_LANES: u32 = 2u;
const FLAG_EDGES: u32 = 4u;
const FLAG_WHEEL_TRACKS: u32 = 8u;
const FLAG_GUTTER: u32 = 16u;

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(hash12(i), hash12(i + vec2(1.0, 0.0)), u.x);
    let b = mix(hash12(i + vec2(0.0, 1.0)), hash12(i + vec2(1.0, 1.0)), u.x);
    return mix(a, b, u.y);
}

fn fbm3(p: vec2<f32>) -> f32 {
    return (0.5 * vnoise(p) + 0.25 * vnoise(p * 2.03 + 17.0) + 0.125 * vnoise(p * 4.01 + 41.0)) / 0.875;
}

fn band(x: f32, half_w: f32, aa: f32) -> f32 {
    return 1.0 - smoothstep(half_w - aa, half_w + aa, abs(x));
}

const WHITE: vec3<f32> = vec3<f32>(0.78, 0.78, 0.74);
const YELLOW: vec3<f32> = vec3<f32>(0.80, 0.52, 0.06);

// Paint colour (rgb) and coverage (a) at road coordinates (u, s).
fn road_markings(uv: vec2<f32>, aa: f32) -> vec4<f32> {
    let u = uv.x;
    let s = uv.y;
    let au = abs(u);
    var col = WHITE;
    var cov = 0.0;
    var in_crossing = false;
    for (var i = 0; i < 4; i++) {
        let c = surface.crossings[i];
        if (c.z < 0.5 || s < c.x || s > c.y || au > c.w) {
            continue;
        }
        in_crossing = true;
        let kind = u32(c.z + 0.5);
        if (kind == 1u) {
            // Continental crosswalk: 0.6 m bars along the road every 1.2 m.
            let bars = band(fract(u / 1.2) - 0.5, 0.25, aa / 1.2);
            let ends = band(s - 0.5 * (c.x + c.y), 0.5 * (c.y - c.x) - 0.05, aa);
            cov = max(cov, bars * ends);
        } else if (kind == 2u) {
            cov = max(cov, band(s - 0.5 * (c.x + c.y), 0.5 * (c.y - c.x), aa));
        } else if (kind == 3u) {
            let cell = (i32(floor(u / 0.5)) + i32(floor(s / 0.5))) & 1;
            col = select(vec3<f32>(0.02), WHITE, cell == 1);
            cov = 1.0;
        }
    }
    if (!in_crossing) {
        if ((surface.flags & FLAG_CENTRE) != 0u) {
            let l = band(au - 0.16, 0.055, aa);
            if (l > cov) {
                col = YELLOW;
                cov = l;
            }
        }
        if ((surface.flags & FLAG_LANES) != 0u) {
            // 3 m dashes every 12 m (US 10 ft / 40 ft).
            let dash = band(fract(s / 12.0) - 0.125, 0.125, aa / 12.0);
            for (var k = 1u; k < surface.lanes_per_side; k++) {
                let l = band(au - f32(k) * surface.lane_width, 0.06, aa) * dash;
                if (l > cov) {
                    col = WHITE;
                    cov = l;
                }
            }
        }
        if ((surface.flags & FLAG_EDGES) != 0u) {
            let l = band(au - (surface.carriage_half - 0.2), 0.075, aa);
            if (l > cov) {
                col = WHITE;
                cov = l;
            }
        }
    }
    return vec4(col, cov);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let world = in.world_position.xyz;
    let geom_n = normalize(in.world_normal);
    // Albedo alpha of opaque ground textures carries height (low = cavity).
    let height = pbr_input.material.base_color.a;
#ifdef VERTEX_UVS_A
    let uv = in.uv;
#else
    let uv = world.xz;
#endif
    let aa = max(length(fwidth(uv)), 0.002) * 0.7;

    var base = pbr_input.material.base_color.rgb;
    var rough = pbr_input.material.perceptual_roughness;
    var paint = 0.0;
    var puddle_bias = 0.0;

    let wear = fbm3(world.xz * 2.7 + surface.seed);
    if (surface.mode == MODE_ROAD) {
        let au = abs(uv.x);
        if ((surface.flags & FLAG_WHEEL_TRACKS) != 0u && au < surface.carriage_half) {
            let lane = floor(au / surface.lane_width);
            let d = abs(au - (lane + 0.5) * surface.lane_width);
            let track = band(d - 0.85, 0.28, 0.2);
            let drip = band(d, 0.35, 0.3);
            base *= 1.0 - 0.18 * track - 0.12 * drip;
            rough -= 0.10 * track;
            puddle_bias += 0.06 * track;
        }
        if ((surface.flags & FLAG_GUTTER) != 0u && au > surface.carriage_half) {
            // Concrete gutter pan with joints every 3 m and grime by the kerb.
            let joint = band(fract(uv.y / 3.0) - 0.5, 0.004, aa / 3.0);
            let grime = fbm3(vec2(uv.y * 0.8, au * 6.0) + surface.seed);
            var pan = vec3<f32>(0.16, 0.155, 0.145) * (0.75 + 0.5 * grime);
            pan *= 1.0 - 0.7 * joint;
            base = mix(base, pan, 0.85);
            rough = 0.8;
            puddle_bias += 0.25;
        }
        let m = road_markings(uv, aa);
        paint = m.a * mix(0.55, 1.0, smoothstep(0.2, 0.5, wear));
        base = mix(base, m.rgb * (0.85 + 0.15 * wear), paint);
        rough = mix(rough, 0.55, paint);
    }

    // Wetness: water fills pores (darker, smoother), puddles collect in low
    // spots, gutters and wheel tracks.
    let wet = surface.wetness;
    if (wet > 0.0) {
        let porous = select(0.5, 0.75, surface.mode == MODE_PLAIN);
        base *= mix(1.0, mix(porous, 0.85, paint), wet);
        rough = mix(rough, mix(0.28, 0.12, paint), wet);
        let n = fbm3(world.xz * 0.33 + surface.seed * 3.1) + (0.5 - height) * 0.18;
        let t = 0.78 - 0.32 * surface.puddles - puddle_bias;
        let puddle = smoothstep(t, t + 0.05, n) * wet * select(1.0, 0.0, surface.mode == MODE_DECAL && paint > 0.5);
        base *= mix(1.0, 0.7, puddle);
        rough = mix(rough, 0.02, puddle);
        pbr_input.N = normalize(mix(pbr_input.N, geom_n, max(puddle, 0.35 * wet)));
    }

    pbr_input.material.base_color = vec4(base, pbr_input.material.base_color.a);
    pbr_input.material.perceptual_roughness = clamp(rough, 0.02, 1.0);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}

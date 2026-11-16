#import bevy_pbr::{
    mesh_bindings::mesh,
    mesh_functions,
    skinning,
    morph::{morph_position, morph_normal, morph_tangent},
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}
#import kjk::lighting::fog

struct JaTint {
    value: vec4<f32>,
}

struct JaParams {
    value: vec4<f32>,
}

struct JaUvOffset {
    value: vec2<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var<uniform> material_tint: JaTint;

@group(#{MATERIAL_BIND_GROUP}) @binding(1)
var base_texture: texture_2d<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(2)
var base_sampler: sampler;

@group(#{MATERIAL_BIND_GROUP}) @binding(3)
var<uniform> material_params: JaParams;

@group(#{MATERIAL_BIND_GROUP}) @binding(4)
var lightmap_texture: texture_2d<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(5)
var lightmap_sampler: sampler;

@group(#{MATERIAL_BIND_GROUP}) @binding(6)
var<uniform> lightmap_enabled: u32;

@group(#{MATERIAL_BIND_GROUP}) @binding(7)
var<uniform> material_uv_offset: JaUvOffset;

struct JaTcModOp {
    kind: vec4<u32>,
    params: vec4<f32>,
    extra: vec4<f32>,
}

struct JaTcModUniform {
    time_count: vec4<f32>,
    ops: array<JaTcModOp, 4>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(8)
var<uniform> material_tc_mods: JaTcModUniform;

struct JaRgbWaveUniform {
    kind: vec4<u32>,
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(9)
var<uniform> material_rgb_wave: JaRgbWaveUniform;

struct JaFogUniform {
    range: vec4<f32>,
    color: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(10)
var<uniform> material_fog: JaFogUniform;

fn ja_wave(wave_kind: u32, phase: f32) -> f32 {
    let x = fract(phase);

    switch wave_kind {
        case 1u: {
            return sin(phase * 6.28318530718);
        }
        case 2u: {
            return 1.0 - 4.0 * abs(x - 0.5);
        }
        case 3u: {
            return select(-1.0, 1.0, x < 0.5);
        }
        case 4u: {
            return x;
        }
        case 5u: {
            return 1.0 - x;
        }
        case 6u: {
            let n = fract(sin(phase * 12.9898 + 78.233) * 43758.5453);
            return n * 2.0 - 1.0;
        }
        default: {
            return 0.0;
        }
    }
}


// The JA vertex path follows Bevy 0.19.1's standard mesh vertex shader.
// tcMods are evaluated per vertex, before rasterisation, as in Quake III.
#ifdef MORPH_TARGETS
fn morph_vertex(vertex_in: Vertex, instance_index: u32) -> Vertex {
    var vertex = vertex_in;
    let first_vertex = mesh[instance_index].first_vertex_index;
    let vertex_index = vertex.index - first_vertex;

    let weight_count = bevy_pbr::morph::layer_count(instance_index);
    for (var i: u32 = 0u; i < weight_count; i ++) {
        let weight = bevy_pbr::morph::weight_at(i, instance_index);
        if weight == 0.0 {
            continue;
        }
        vertex.position += weight * morph_position(vertex_index, i, instance_index);
#ifdef VERTEX_NORMALS
        vertex.normal += weight * morph_normal(vertex_index, i, instance_index);
#endif
#ifdef VERTEX_TANGENTS
        vertex.tangent += vec4(weight * morph_tangent(vertex_index, i, instance_index), 0.0);
#endif
    }
    return vertex;
}
#endif

@vertex
fn vertex(vertex_no_morph: Vertex) -> VertexOutput {
    var out: VertexOutput;

#ifdef MORPH_TARGETS
    var vertex = morph_vertex(vertex_no_morph, vertex_no_morph.instance_index);
#else
    var vertex = vertex_no_morph;
#endif

    let mesh_world_from_local = mesh_functions::get_world_from_local(vertex_no_morph.instance_index);

#ifdef SKINNED
    var world_from_local = skinning::skin_model(
        vertex.joint_indices,
        vertex.joint_weights,
        vertex_no_morph.instance_index
    );
#else
    var world_from_local = mesh_world_from_local;
#endif

#ifdef VERTEX_NORMALS
#ifdef SKINNED
    out.world_normal = skinning::skin_normals(world_from_local, vertex.normal);
#else
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex_no_morph.instance_index
    );
#endif
#endif

#ifdef VERTEX_POSITIONS
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0)
    );
    out.position = position_world_to_clip(out.world_position.xyz);
#endif

    // JA base coordinates normally come from UV0. A material can select
    // UV1 through material_params, but UV1 remains available for lightmaps.
    var uv = vec2<f32>(0.0);

#ifdef VERTEX_UVS_A
    uv = vertex.uv;
#endif

#ifdef VERTEX_UVS_B
    if material_params.value.x > 0.5 {
        uv = vertex.uv_b;
    }
#endif

    uv = uv + material_uv_offset.value;

    let seconds = material_tc_mods.time_count.x;
    let operation_count = u32(material_tc_mods.time_count.y);

    // Preserve the original directive order: each operation consumes
    // the UV coordinates produced by the preceding operation.
    for (var i = 0u; i < operation_count; i = i + 1u) {
        let op = material_tc_mods.ops[i];

        switch op.kind.x {
            // tcMod rotate: degrees per second around the texture centre.
            case 1u: {
                let angle = op.params.x * seconds * 0.017453292519943295;
                let c = cos(angle);
                let s = sin(angle);
                let centred = uv - vec2<f32>(0.5, 0.5);

                uv = vec2<f32>(
                    centred.x * c - centred.y * s,
                    centred.x * s + centred.y * c
                ) + vec2<f32>(0.5, 0.5);
            }

            // tcMod scale.
            case 2u: {
                uv = uv * op.params.xy;
            }

            // tcMod scroll.
            case 3u: {
                uv = uv + op.params.xy * seconds;
            }

            // tcMod stretch.
            case 4u: {
                let wave = ja_wave(
                    op.kind.y,
                    op.params.z + seconds * op.params.w
                );
                let scale = op.params.x + op.params.y * wave;

                if abs(scale) > 0.00001 {
                    uv = (uv - vec2<f32>(0.5, 0.5)) / scale
                        + vec2<f32>(0.5, 0.5);
                }
            }

            // tcMod transform.
            case 5u: {
                let old_uv = uv;
                uv = vec2<f32>(
                    old_uv.x * op.params.x
                        + old_uv.y * op.params.z
                        + op.extra.x,
                    old_uv.x * op.params.y
                        + old_uv.y * op.params.w
                        + op.extra.y
                );
            }

            // Quake III RB_CalcTurbulentTexCoords.
            // vertex.position is in the original mesh/game coordinate space.
            // The Q3 sine-table index is masked to its 1024 entries.
            case 6u: {
                let phase = op.params.z + seconds * op.params.w;

                let s_phase = (
                    (vertex.position.x + vertex.position.z) / 1024.0
                    + phase
                );
                let t_phase = (
                    vertex.position.y / 1024.0
                    + phase
                );

                let s_index = u32(i32(s_phase * 1024.0)) & 1023u;
                let t_index = u32(i32(t_phase * 1024.0)) & 1023u;

                let s_wave = sin(f32(s_index) * (6.28318530718 / 1024.0));
                let t_wave = sin(f32(t_index) * (6.28318530718 / 1024.0));

                uv = uv + vec2<f32>(s_wave, t_wave) * op.params.y;
            }

            default: {}
        }
    }

#ifdef VERTEX_UVS_A
    out.uv = uv;
#endif

#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif

#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        vertex_no_morph.instance_index
    );
#endif

#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif

#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex_no_morph.instance_index;
#endif

#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_no_morph.instance_index,
        mesh_world_from_local[3]
    );
#endif

    return out;
}
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // UV0 has already had the ordered tcMod chain applied per vertex.
    let uv = in.uv;
    let seconds = material_tc_mods.time_count.x;
    var texel = textureSample(base_texture, base_sampler, uv);

    if material_params.value.z > 0.5 {
        texel.a = 1.0;
    }

    var result = texel * material_tint.value;

    // rgbGen wave: base + amplitude * waveform.
    // The shared tcMod clock also drives RGB waves on this material.
    if material_rgb_wave.kind.x != 0u {
        let wave = ja_wave(
            material_rgb_wave.kind.x,
            material_rgb_wave.params.z + seconds * material_rgb_wave.params.w
        );
        let rgb_scale = material_rgb_wave.params.x
            + material_rgb_wave.params.y * wave;
        result = vec4<f32>(result.rgb * rgb_scale, result.a);
    }

#ifdef VERTEX_COLORS
    if material_params.value.y > 0.5 {
        result = result * in.color;
    }
#endif

#ifdef VERTEX_UVS_B
    if lightmap_enabled != 0u {
        let lightmap_sample = textureSample(
            lightmap_texture,
            lightmap_sampler,
            in.uv_b
        );
        let adjusted_lightmap = max(lightmap_sample.rgb, vec3<f32>(0.40));
        result = vec4<f32>(
            result.rgb * adjusted_lightmap,
            result.a
        );
    }
#endif

    if material_fog.range.y > 0.0 {
        result = vec4<f32>(
            fog(result.rgb, in.world_position.xyz, material_fog.range, material_fog.color.rgb),
            result.a
        );
    }

    return result;
}
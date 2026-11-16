// Jedi Academy BSP maps converted into shared runtime assets.
//
// Planar and triangle-soup surfaces use the BSP's existing triangle indexes.
// Bezier patch surfaces are tessellated into ordinary triangle meshes.
// Embedded RGB lightmaps are expanded to RGBA8 textures.

use std::collections::BTreeMap;

use asset_odyssey::{
    model::{IDENTITY, MeshIr, ModelIr, NodeIr},
    texture::{Encoding, TextureFacts, TextureIr},
};
use fmt_ja::bsp::{
    self, BspDrawVert, BspError, LIGHTMAP_CHANNELS, LIGHTMAP_HEIGHT, LIGHTMAP_WIDTH,
};

use crate::UNIT;
use crate::vis::JaVisibility;

const SURFACE_PLANAR: i32 = 1;
const SURFACE_PATCH: i32 = 2;
const SURFACE_TRIANGLE_SOUP: i32 = 3;
const SURFACE_FLARE: i32 = 4;

const PATCH_SUBDIVISIONS: usize = 8;

/// Visibility and mesh-to-surface mapping for a converted BSP map.
#[derive(Debug, Clone)]
pub struct BspVis {
    /// The BSP's PVS and spatial tree.
    pub visibility: JaVisibility,
    /// `mesh_surface[i]` is the original BSP surface that produced mesh `i`.
    pub mesh_surface: Vec<usize>,
    /// `mesh_shaders[i]` is the shader record used by mesh `i`.
    pub mesh_shaders: Vec<bsp::BspShader>,
    /// Embedded lightmaps, keyed by their generated name.
    pub lightmaps: BTreeMap<String, TextureIr>,
}
/// Statistics collected while converting a BSP.
#[derive(Debug, Default)]
struct BspStats {
    planar_converted: usize,
    patch_converted: usize,
    triangle_soup_converted: usize,
    flare_skipped: usize,
    empty_skipped: usize,
    patch_vertices: usize,
    patch_indices: usize,
    lightmapped_meshes: usize,
}

/// Convert a Jedi Academy RBSP file into a static model and its lightmaps.
pub fn model(bytes: &[u8], name: &str) -> Result<(ModelIr, BTreeMap<String, TextureIr>), BspError> {
    let (ir, textures, _, _, _) = model_impl(bytes, name)?;
    Ok((ir, textures))
}

/// Convert a BSP into a model plus visibility data and mesh-to-surface mapping.
pub fn model_full(bytes: &[u8], name: &str) -> Result<(ModelIr, BspVis), BspError> {
    let (ir, textures, _, mesh_surface, mesh_shaders) = model_impl(bytes, name)?;
    let visibility = JaVisibility::parse(bytes)?;

    Ok((
        ir,
        BspVis {
            visibility,
            mesh_surface,
            mesh_shaders,
            lightmaps: textures,
        },
    ))
}

/// Conversion plus diagnostics (test-only).
#[allow(dead_code)]
fn model_with_stats(
    bytes: &[u8],
    name: &str,
) -> Result<(ModelIr, BTreeMap<String, TextureIr>, BspStats), BspError> {
    let (ir, textures, stats, _, _) = model_impl(bytes, name)?;
    Ok((ir, textures, stats))
}

/// The implementation shared by `model`, `model_full` and `model_with_stats`.
fn model_impl(
    bytes: &[u8],
    name: &str,
) -> Result<
    (
        ModelIr,
        BTreeMap<String, TextureIr>,
        BspStats,
        Vec<usize>,
        Vec<bsp::BspShader>,
    ),
    BspError,
> {
    let parsed = bsp::parse(bytes)?;

    let shaders = bsp::parse_shaders(bytes, &parsed)?;
    let draw_verts = bsp::parse_draw_verts(bytes, &parsed)?;
    let draw_indexes = bsp::parse_draw_indexes(bytes, &parsed)?;
    let surfaces = bsp::parse_surfaces(bytes, &parsed)?;
    let lightmaps = bsp::parse_lightmaps(bytes, &parsed)?;

    let mut textures = BTreeMap::new();
    let mut meshes = Vec::new();
    let mut mesh_surface = Vec::new();
    let mut mesh_shaders = Vec::new();
    let mut stats = BspStats::default();

    for (surface_index, surface) in surfaces.iter().enumerate() {
        match surface.surface_type {
            SURFACE_PLANAR | SURFACE_TRIANGLE_SOUP | SURFACE_PATCH => {}
            SURFACE_FLARE => {
                stats.flare_skipped += 1;
                continue;
            }
            other => {
                return Err(BspError::Bad(format!(
                    "surface {surface_index} has unsupported surface type {other}"
                )));
            }
        }

        let empty = if surface.surface_type == SURFACE_PATCH {
            surface.num_verts <= 0
        } else {
            surface.num_verts <= 0 || surface.num_indexes <= 0
        };

        if empty {
            stats.empty_skipped += 1;
            continue;
        }

        let shader_index = usize::try_from(surface.shader).map_err(|_| {
            BspError::Bad(format!(
                "surface {surface_index} has invalid shader index {}",
                surface.shader
            ))
        })?;

        let shader = shaders.get(shader_index).ok_or_else(|| {
            BspError::Bad(format!(
                "surface {surface_index} shader index {} is out of range",
                surface.shader
            ))
        })?;

        let first_vert = usize::try_from(surface.first_vert).map_err(|_| {
            BspError::Bad(format!("surface {surface_index} has negative first vertex"))
        })?;

        let num_verts = usize::try_from(surface.num_verts).map_err(|_| {
            BspError::Bad(format!("surface {surface_index} has negative vertex count"))
        })?;

        let first_index = usize::try_from(surface.first_index).map_err(|_| {
            BspError::Bad(format!("surface {surface_index} has negative first index"))
        })?;

        let num_indexes = usize::try_from(surface.num_indexes).map_err(|_| {
            BspError::Bad(format!("surface {surface_index} has negative index count"))
        })?;

        let vertex_end = first_vert.checked_add(num_verts).ok_or_else(|| {
            BspError::Bad(format!("surface {surface_index} vertex range overflows"))
        })?;

        let index_end = first_index.checked_add(num_indexes).ok_or_else(|| {
            BspError::Bad(format!("surface {surface_index} index range overflows"))
        })?;

        let vertices = draw_verts.get(first_vert..vertex_end).ok_or_else(|| {
            BspError::Bad(format!(
                "surface {surface_index} vertex range is outside the draw-vertex lump"
            ))
        })?;

        let indexes = draw_indexes.get(first_index..index_end).ok_or_else(|| {
            BspError::Bad(format!(
                "surface {surface_index} index range is outside the draw-index lump"
            ))
        })?;

        let (mesh_vertices, mesh_indexes) = if surface.surface_type == SURFACE_PATCH {
            let (vertices, indexes) = tessellate_patch(
                surface_index,
                surface.patch_width,
                surface.patch_height,
                vertices,
            )?;

            stats.patch_vertices += vertices.len();
            stats.patch_indices += indexes.len();

            (vertices, indexes)
        } else {
            let mut mesh_indexes = Vec::with_capacity(indexes.len());

            for &index in indexes {
                let index = u32::try_from(index).map_err(|_| {
                    BspError::Bad(format!(
                        "surface {surface_index} contains negative draw index {index}"
                    ))
                })?;

                if index as usize >= vertices.len() {
                    return Err(BspError::Bad(format!(
                        "surface {surface_index} draw index {index} exceeds its vertex count {}",
                        vertices.len()
                    )));
                }

                mesh_indexes.push(index);
            }

            (vertices.to_vec(), mesh_indexes)
        };

        if mesh_indexes.is_empty() {
            continue;
        }

        let texture = shader.name.clone();
        let lightmap_number = surface.lightmap_num[0];
        let has_lightmap = lightmap_number >= 0;

        let lightmap = if has_lightmap {
            let lightmap_index = usize::try_from(lightmap_number).map_err(|_| {
                BspError::Bad(format!(
                    "surface {surface_index} has invalid lightmap number {lightmap_number}"
                ))
            })?;

            let lightmap_bytes = lightmaps.get(lightmap_index).ok_or_else(|| {
                BspError::Bad(format!(
                    "surface {surface_index} lightmap number {lightmap_number} is out of range"
                ))
            })?;

            let lightmap_name = format!(
                "{}_lightmap_{lightmap_index:04}",
                name.trim_end_matches('/')
            );

            textures
                .entry(lightmap_name.clone())
                .or_insert_with(|| lightmap_texture(lightmap_bytes));

            Some(lightmap_name)
        } else {
            None
        };

        if lightmap.is_some() {
            stats.lightmapped_meshes += 1;
        }

        match surface.surface_type {
            SURFACE_PLANAR => stats.planar_converted += 1,
            SURFACE_PATCH => stats.patch_converted += 1,
            SURFACE_TRIANGLE_SOUP => stats.triangle_soup_converted += 1,
            _ => unreachable!(),
        }

        // Jedi Academy stores lightmap UVs directly in atlas-normalised
        // coordinates. The surface rectangle dimensions are not populated
        // in this BSP, so do not apply an additional rectangle transform.
        let uv2 = if has_lightmap {
            mesh_vertices
                .iter()
                .map(|vertex| vertex.lightmap_coords[0])
                .collect()
        } else {
            Vec::new()
        };

        mesh_surface.push(surface_index);
        mesh_shaders.push(shader.clone());

        meshes.push(MeshIr {
            node: 0,
            positions: mesh_vertices
                .iter()
                .map(|vertex| vertex.position.map(|value| value * UNIT))
                .collect(),
            normals: mesh_vertices
                .iter()
                .map(|vertex| normalize(vertex.normal))
                .collect(),
            uv1: mesh_vertices
                .iter()
                .map(|vertex| vertex.tex_coords)
                .collect(),
            uv2,
            indices: mesh_indexes,
            texture: (!texture.is_empty()).then_some(texture),
            lightmap,
            transparency_hint: 0,
            skinned: false,
            self_illum: [0.0; 3],
            alpha: 1.0,
            ambient: [1.0; 3],
            diffuse: [1.0; 3],
            uv_scroll: None,
            skin: None,
            dangly: None,
        });
    }

    let ir = ModelIr {
        name: name.to_string(),
        supermodel: None,
        nodes: vec![NodeIr {
            name: "root".to_string(),
            parent: None,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            world: IDENTITY,
            animated: false,
        }],
        meshes,
        lights: Vec::new(),
        emitters: Vec::new(),
        animations: Vec::new(),
        animation_scale: 1.0,
        affected_by_fog: false,
        refusals: Vec::new(),
        folded: Vec::new(),
    };

    Ok((ir, textures, stats, mesh_surface, mesh_shaders))
}

fn tessellate_patch(
    surface_index: usize,
    width: i32,
    height: i32,
    control: &[BspDrawVert],
) -> Result<(Vec<BspDrawVert>, Vec<u32>), BspError> {
    let width = usize::try_from(width)
        .map_err(|_| BspError::Bad(format!("patch surface {surface_index} has negative width")))?;
    let height = usize::try_from(height)
        .map_err(|_| BspError::Bad(format!("patch surface {surface_index} has negative height")))?;

    if width < 3 || height < 3 || width % 2 == 0 || height % 2 == 0 {
        return Err(BspError::Bad(format!(
            "patch surface {surface_index} has invalid control-grid dimensions {width}x{height}; expected odd dimensions of at least 3"
        )));
    }

    let expected = width.checked_mul(height).ok_or_else(|| {
        BspError::Bad(format!("patch surface {surface_index} dimensions overflow"))
    })?;

    if expected != control.len() {
        return Err(BspError::Bad(format!(
            "patch surface {surface_index} has {} control vertices but dimensions {width}x{height} require {expected}",
            control.len()
        )));
    }

    let segments_x = (width - 1) / 2;
    let segments_y = (height - 1) / 2;
    let cells_x = segments_x * PATCH_SUBDIVISIONS;
    let cells_y = segments_y * PATCH_SUBDIVISIONS;

    let output_width = cells_x + 1;
    let output_height = cells_y + 1;
    let output_count = output_width.checked_mul(output_height).ok_or_else(|| {
        BspError::Bad(format!(
            "patch surface {surface_index} output size overflows"
        ))
    })?;

    let mut vertices = Vec::with_capacity(output_count);

    for y in 0..=cells_y {
        let segment_y = (y / PATCH_SUBDIVISIONS).min(segments_y - 1);
        let local_y = if y == cells_y {
            1.0
        } else {
            (y % PATCH_SUBDIVISIONS) as f32 / PATCH_SUBDIVISIONS as f32
        };

        for x in 0..=cells_x {
            let segment_x = (x / PATCH_SUBDIVISIONS).min(segments_x - 1);
            let local_x = if x == cells_x {
                1.0
            } else {
                (x % PATCH_SUBDIVISIONS) as f32 / PATCH_SUBDIVISIONS as f32
            };

            let mut row = Vec::with_capacity(3);

            for row_offset in 0..3 {
                let control_y = segment_y * 2 + row_offset;
                let a = &control[control_y * width + segment_x * 2];
                let b = &control[control_y * width + segment_x * 2 + 1];
                let c = &control[control_y * width + segment_x * 2 + 2];

                row.push(bezier_vertex(a, b, c, local_x));
            }

            vertices.push(bezier_vertex(&row[0], &row[1], &row[2], local_y));
        }
    }

    let index_count = cells_x
        .checked_mul(cells_y)
        .and_then(|count| count.checked_mul(6))
        .ok_or_else(|| {
            BspError::Bad(format!(
                "patch surface {surface_index} index count overflows"
            ))
        })?;

    let mut indexes = Vec::with_capacity(index_count);

    for y in 0..cells_y {
        for x in 0..cells_x {
            let top_left = (y * output_width + x) as u32;
            let top_right = top_left + 1;
            let bottom_left = top_left + output_width as u32;
            let bottom_right = bottom_left + 1;

            indexes.extend_from_slice(&[
                top_left,
                bottom_left,
                top_right,
                top_right,
                bottom_left,
                bottom_right,
            ]);
        }
    }

    Ok((vertices, indexes))
}

/// Evaluate a quadratic Bezier curve for all attributes in a draw vertex.
fn bezier_vertex(a: &BspDrawVert, b: &BspDrawVert, c: &BspDrawVert, t: f32) -> BspDrawVert {
    let one_minus_t = 1.0 - t;
    let weights = [one_minus_t * one_minus_t, 2.0 * one_minus_t * t, t * t];

    let blend3 = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| {
        std::array::from_fn(|i| a[i] * weights[0] + b[i] * weights[1] + c[i] * weights[2])
    };

    let blend2 = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        std::array::from_fn(|i| a[i] * weights[0] + b[i] * weights[1] + c[i] * weights[2])
    };

    let mut result = a.clone();

    result.position = blend3(a.position, b.position, c.position);
    result.tex_coords = blend2(a.tex_coords, b.tex_coords, c.tex_coords);
    result.normal = normalize(blend3(a.normal, b.normal, c.normal));

    for lightmap in 0..result.lightmap_coords.len() {
        result.lightmap_coords[lightmap] = blend2(
            a.lightmap_coords[lightmap],
            b.lightmap_coords[lightmap],
            c.lightmap_coords[lightmap],
        );

        for channel in 0..4 {
            let value = a.color[lightmap][channel] as f32 * weights[0]
                + b.color[lightmap][channel] as f32 * weights[1]
                + c.color[lightmap][channel] as f32 * weights[2];

            result.color[lightmap][channel] = value.round().clamp(0.0, 255.0) as u8;
        }
    }

    result
}

fn normalize(vector: [f32; 3]) -> [f32; 3] {
    let length_squared = vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2];

    if length_squared <= f32::EPSILON || !length_squared.is_finite() {
        return [0.0, 0.0, 1.0];
    }

    let inverse_length = length_squared.sqrt().recip();

    [
        vector[0] * inverse_length,
        vector[1] * inverse_length,
        vector[2] * inverse_length,
    ]
}

/// Expand one embedded BSP RGB lightmap into the runtime's RGBA8 texture.
fn lightmap_texture(rgb: &[u8]) -> TextureIr {
    let mut rgba = Vec::with_capacity(LIGHTMAP_WIDTH * LIGHTMAP_HEIGHT * 4);

    for pixel in rgb.chunks_exact(LIGHTMAP_CHANNELS) {
        rgba.extend_from_slice(pixel);
        rgba.push(255);
    }

    TextureIr {
        width: LIGHTMAP_WIDTH as u32,
        height: LIGHTMAP_HEIGHT as u32,
        encoding: Encoding::Rgba8,
        mips: vec![rgba],
        cube: false,
        frames: 1,
        facts: TextureFacts::default(),
        faces: Vec::new(),
        frame_mips: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control_vertex(x: f32, y: f32, z: f32) -> BspDrawVert {
        BspDrawVert {
            position: [x, y, z],
            tex_coords: [x, y],
            lightmap_coords: [[x, y]; 4],
            normal: [0.0, 0.0, 1.0],
            color: [[255; 4]; 4],
        }
    }

    #[test]
    fn tessellates_single_patch() {
        let mut control = Vec::new();

        for y in 0..3 {
            for x in 0..3 {
                control.push(control_vertex(x as f32, y as f32, 0.0));
            }
        }

        let (vertices, indexes) = tessellate_patch(0, 3, 3, &control).unwrap();

        assert_eq!(vertices.len(), 81);
        assert_eq!(indexes.len(), 384);

        assert_eq!(vertices[0].position, [0.0, 0.0, 0.0]);
        assert_eq!(vertices[8].position, [2.0, 0.0, 0.0]);
        assert_eq!(vertices[72].position, [0.0, 2.0, 0.0]);
        assert_eq!(vertices[80].position, [2.0, 2.0, 0.0]);

        assert_eq!(vertices[40].normal, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn tessellates_adjacent_patches() {
        let mut control = Vec::new();

        for y in 0..3 {
            for x in 0..5 {
                control.push(control_vertex(x as f32, y as f32, 0.0));
            }
        }

        let (vertices, indexes) = tessellate_patch(0, 5, 3, &control).unwrap();

        assert_eq!(vertices.len(), 153);
        assert_eq!(indexes.len(), 768);

        assert_eq!(vertices[0].position, [0.0, 0.0, 0.0]);
        assert_eq!(vertices[16].position, [4.0, 0.0, 0.0]);
        assert_eq!(vertices[152].position, [4.0, 2.0, 0.0]);
    }

    #[test]
    fn rejects_invalid_patch_dimensions() {
        let control = vec![control_vertex(0.0, 0.0, 0.0); 9];

        assert!(tessellate_patch(0, 2, 3, &control).is_err());
        assert!(tessellate_patch(0, 3, 4, &control).is_err());
        assert!(tessellate_patch(0, 5, 3, &control).is_err());
    }

    mod real_map_test {
        use std::{collections::BTreeMap, path::Path};

        use vfs::ja::Pk3File;

        #[test]
        #[ignore = "requires local Jedi Academy assets0.pk3"]
        fn real_map() {
            let supplied = std::env::var("JA_BSP")
                .expect("set JA_BSP to the PK3 path followed by the internal BSP path");

            let marker = ".pk3";
            let split = supplied
                .to_ascii_lowercase()
                .find(marker)
                .expect("JA_BSP must contain a .pk3 archive path");

            let archive_path = format!("{}{}", &supplied[..split], marker);
            let entry_path = supplied[split + marker.len()..]
                .trim_start_matches(['\\', '/'])
                .replace('\\', "/")
                .to_ascii_lowercase();

            assert!(
                !entry_path.is_empty(),
                "JA_BSP must include an internal archive path"
            );

            let archive = Pk3File::open(Path::new(&archive_path))
                .unwrap_or_else(|error| panic!("opening {archive_path}: {error}"));

            let entry = archive
                .entries
                .iter()
                .find(|entry| entry.name == entry_path)
                .unwrap_or_else(|| panic!("{} not found in {}", entry_path, archive_path));

            let bytes = archive
                .read(entry)
                .unwrap_or_else(|error| panic!("extracting {entry_path}: {error}"));

            println!("Archive: {archive_path}");
            println!("Entry: {}", entry.name);
            println!("BSP bytes: {}", bytes.len());

            let parsed = fmt_ja::bsp::parse(&bytes).expect("parsing BSP diagnostics");
            let surfaces =
                fmt_ja::bsp::parse_surfaces(&bytes, &parsed).expect("reading BSP surfaces");
            let surface_lump = &parsed.lumps[fmt_ja::bsp::LumpKind::Surfaces as usize];
            println!(
                "Surface lump: offset={}, length={}",
                surface_lump.offset, surface_lump.length
            );

            for index in [0usize, 1, 2, 4, 10] {
                let start = surface_lump.offset as usize + index * fmt_ja::bsp::SURFACE_RECORD_SIZE;
                let record = &bytes[start..start + fmt_ja::bsp::SURFACE_RECORD_SIZE];

                println!(
                    "Surface {index}: parsed type={}, lm_num={:?}, lm_x={:?}, lm_y={:?}, size={}x{}",
                    surfaces[index].surface_type,
                    surfaces[index].lightmap_num,
                    surfaces[index].lightmap_x,
                    surfaces[index].lightmap_y,
                    surfaces[index].lightmap_width,
                    surfaces[index].lightmap_height,
                );

                println!("  Raw bytes 28..92: {:02X?}", &record[28..92]);
            }

            let shaders = fmt_ja::bsp::parse_shaders(&bytes, &parsed).expect("reading BSP shaders");

            let draw_verts =
                fmt_ja::bsp::parse_draw_verts(&bytes, &parsed).expect("reading draw vertices");
            let lightmaps =
                fmt_ja::bsp::parse_lightmaps(&bytes, &parsed).expect("reading lightmaps");

            println!("\nLightmap lump:");
            println!("  Atlas count: {}", lightmaps.len());
            println!("  Atlas dimensions: 128x128 RGB");
            println!("  Draw vertices: {}", draw_verts.len());

            for index in [0usize, 1, 2, 4, 10, 100] {
                if let Some(vertex) = draw_verts.get(index) {
                    println!(
                        "  Vertex {index}: UV={:?}, lightmap UVs={:?}",
                        vertex.tex_coords, vertex.lightmap_coords
                    );
                }
            }

            let with_nonzero_uv = draw_verts
                .iter()
                .filter(|v| v.lightmap_coords[0] != [0.0, 0.0])
                .count();

            println!("  Vertices with non-zero slot-0 lightmap UV: {with_nonzero_uv}");

            println!();
            println!("Explicit Hoth sky shader entries:");
            for shader in shaders
                .iter()
                .filter(|shader| shader.name.to_ascii_lowercase().contains("skies/hoth"))
            {
                println!(
                    "  {}: surface_flags={:#010x}, content_flags={:#010x}",
                    shader.name, shader.surface_flags, shader.content_flags
                );
            }

            let mut lightmap_slot_counts = [0usize; fmt_ja::bsp::MAX_LIGHTMAPS];

            for surface in &surfaces {
                for (slot, &number) in surface.lightmap_num.iter().enumerate() {
                    if number >= 0 {
                        lightmap_slot_counts[slot] += 1;
                    }
                }
            }

            println!();
            println!("Lightmap slot usage:");
            for (slot, count) in lightmap_slot_counts.iter().enumerate() {
                println!("  Slot {slot}: {count} surfaces");
            }

            let planar = surfaces.iter().filter(|s| s.surface_type == 1).count();
            let patches = surfaces.iter().filter(|s| s.surface_type == 2).count();
            let triangle_soup = surfaces.iter().filter(|s| s.surface_type == 3).count();
            let flares = surfaces.iter().filter(|s| s.surface_type == 4).count();
            let unsupported = surfaces
                .iter()
                .filter(|s| !(1..=4).contains(&s.surface_type))
                .count();

            let empty = surfaces
                .iter()
                .filter(|s| {
                    if s.surface_type == 2 {
                        s.num_verts <= 0
                    } else {
                        s.num_verts <= 0 || s.num_indexes <= 0
                    }
                })
                .count();

            let patch_control_vertices: usize = surfaces
                .iter()
                .filter(|s| s.surface_type == 2 && s.patch_width > 0 && s.patch_height > 0)
                .map(|s| s.patch_width as usize * s.patch_height as usize)
                .sum();

            let estimated_patch_vertices: usize = surfaces
                .iter()
                .filter(|s| s.surface_type == 2 && s.patch_width >= 3 && s.patch_height >= 3)
                .map(|s| {
                    let segments_x = (s.patch_width as usize - 1) / 2;
                    let segments_y = (s.patch_height as usize - 1) / 2;

                    (segments_x * 8 + 1) * (segments_y * 8 + 1)
                })
                .sum();

            println!();
            println!("BSP surface diagnostics:");
            println!("  Total surfaces: {}", surfaces.len());
            println!("  Planar: {planar}");
            println!("  Patch: {patches}");
            println!("  Triangle soup: {triangle_soup}");
            println!("  Flare: {flares}");
            println!("  Unsupported: {unsupported}");
            println!("  Empty surfaces: {empty}");
            println!("  Patch control vertices: {patch_control_vertices}");
            println!("  Estimated tessellated patch vertices: {estimated_patch_vertices}");

            let (model, lightmaps, stats) =
                super::super::model_with_stats(&bytes, "hoth2").expect("converting hoth2 BSP");
            let lightmapped: Vec<_> = model
                .meshes
                .iter()
                .filter(|mesh| mesh.lightmap.is_some())
                .collect();
            let uv2_count: usize = lightmapped.iter().map(|mesh| mesh.uv2.len()).sum();

            let uv2_out_of_range = lightmapped
                .iter()
                .flat_map(|mesh| &mesh.uv2)
                .filter(|uv| {
                    !uv[0].is_finite()
                        || !uv[1].is_finite()
                        || uv[0] < 0.0
                        || uv[0] > 1.0
                        || uv[1] < 0.0
                        || uv[1] > 1.0
                })
                .count();

            println!();
            println!("Lightmap UV diagnostics:");
            println!("  Lightmapped meshes: {}", lightmapped.len());
            println!("  UV2 coordinates: {uv2_count}");
            println!("  Out-of-range UV2 coordinates: {uv2_out_of_range}");

            let mesh_count = model.meshes.len();
            let vertex_count: usize = model.meshes.iter().map(|mesh| mesh.positions.len()).sum();
            let index_count: usize = model.meshes.iter().map(|mesh| mesh.indices.len()).sum();

            println!();
            println!("Converted model:");
            println!("  Meshes: {mesh_count}");
            println!("  Vertices: {vertex_count}");
            println!("  Indices: {index_count}");
            println!("  Lightmaps: {}", lightmaps.len());

            println!();
            println!("Actual conversion diagnostics:");
            println!("  Total surfaces: {}", surfaces.len());
            println!("  Planar converted: {}", stats.planar_converted);
            println!("  Patches converted: {}", stats.patch_converted);
            println!(
                "  Triangle soup converted: {}",
                stats.triangle_soup_converted
            );
            println!("  Flares skipped: {}", stats.flare_skipped);
            println!("  Empty surfaces skipped: {}", stats.empty_skipped);
            println!("  Actual patch vertices: {}", stats.patch_vertices);
            println!("  Actual patch indices: {}", stats.patch_indices);
            println!("  Meshes with lightmaps: {}", stats.lightmapped_meshes);

            println!();

            assert!(mesh_count > 0, "map produced no meshes");
            assert!(vertex_count > 0, "map produced no vertices");
            assert!(index_count > 0, "map produced no indices");
            assert_eq!(unsupported, 0, "map contains unsupported surface types");
        }
    }
}

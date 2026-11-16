use std::collections::BTreeMap;
use std::fmt;

pub const BSP_IDENT: &[u8; 4] = b"RBSP";
pub const BSP_VERSION: i32 = 1;
pub const HEADER_LUMPS: usize = 18;
pub const HEADER_SIZE: usize = 8 + HEADER_LUMPS * 8;
pub const INDEX_RECORD_SIZE: usize = 4;

pub const SHADER_NAME_SIZE: usize = 64;
pub const SHADER_RECORD_SIZE: usize = 72;
pub const PLANE_RECORD_SIZE: usize = 16;
pub const NODE_RECORD_SIZE: usize = 36;
pub const LEAF_RECORD_SIZE: usize = 48;
pub const MODEL_RECORD_SIZE: usize = 40;
pub const BRUSH_RECORD_SIZE: usize = 12;
pub const BRUSH_SIDE_RECORD_SIZE: usize = 12;
pub const MAX_LIGHTMAPS: usize = 4;
pub const DRAW_VERT_RECORD_SIZE: usize = 80;
pub const SURFACE_RECORD_SIZE: usize = 148;
pub const FOG_SHADER_NAME_SIZE: usize = 64;
pub const FOG_RECORD_SIZE: usize = 72;
pub const LIGHTMAP_WIDTH: usize = 128;
pub const LIGHTMAP_HEIGHT: usize = 128;
pub const LIGHTMAP_CHANNELS: usize = 3;
pub const LIGHTMAP_SIZE: usize = LIGHTMAP_WIDTH * LIGHTMAP_HEIGHT * LIGHTMAP_CHANNELS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum LumpKind {
    Entities = 0,
    Shaders,
    Planes,
    Nodes,
    Leafs,
    LeafSurfaces,
    LeafBrushes,
    Models,
    Brushes,
    BrushSides,
    DrawVerts,
    DrawIndexes,
    Fogs,
    Surfaces,
    Lightmaps,
    LightGrid,
    Visibility,
    LightArray,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lump {
    pub offset: usize,
    pub length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BspShader {
    pub name: String,
    pub surface_flags: i32,
    pub content_flags: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BspPlane {
    pub normal: [f32; 3],
    pub dist: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BspNode {
    pub plane: i32,
    pub children: [i32; 2],
    pub mins: [i32; 3],
    pub maxs: [i32; 3],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BspLeaf {
    pub cluster: i32,
    pub area: i32,
    pub mins: [i32; 3],
    pub maxs: [i32; 3],
    pub first_leaf_surface: i32,
    pub num_leaf_surfaces: i32,
    pub first_leaf_brush: i32,
    pub num_leaf_brushes: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BspVisibility {
    pub num_clusters: usize,
    pub bytes_per_cluster: usize,
    pub bitsets: Vec<u8>,
}

impl BspVisibility {
    /// Returns whether `target_cluster` is visible from `source_cluster`.
    /// Negative or out-of-range cluster IDs are treated as not visible.
    pub fn is_visible(&self, source_cluster: i32, target_cluster: i32) -> bool {
        let (Ok(source), Ok(target)) = (
            usize::try_from(source_cluster),
            usize::try_from(target_cluster),
        ) else {
            return false;
        };

        if source >= self.num_clusters || target >= self.num_clusters {
            return false;
        }

        let Some(byte_index) = source
            .checked_mul(self.bytes_per_cluster)
            .and_then(|row| row.checked_add(target / 8))
        else {
            return false;
        };

        self.bitsets
            .get(byte_index)
            .is_some_and(|byte| byte & (1u8 << (target % 8)) != 0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BspModel {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub first_surface: i32,
    pub num_surfaces: i32,
    pub first_brush: i32,
    pub num_brushes: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BspBrush {
    pub first_side: i32,
    pub num_sides: i32,
    pub shader: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BspBrushSide {
    pub plane: i32,
    pub shader: i32,
    pub draw_surface: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BspDrawVert {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2],
    pub lightmap_coords: [[f32; 2]; MAX_LIGHTMAPS],
    pub normal: [f32; 3],
    pub color: [[u8; 4]; MAX_LIGHTMAPS],
}

#[derive(Debug, Clone, PartialEq)]
pub struct BspSurface {
    pub shader: i32,
    pub fog: i32,
    pub surface_type: i32,
    pub first_vert: i32,
    pub num_verts: i32,
    pub first_index: i32,
    pub num_indexes: i32,
    pub lightmap_styles: [u8; MAX_LIGHTMAPS],
    pub vertex_styles: [u8; MAX_LIGHTMAPS],
    pub lightmap_num: [i32; MAX_LIGHTMAPS],
    pub lightmap_x: [i32; MAX_LIGHTMAPS],
    pub lightmap_y: [i32; MAX_LIGHTMAPS],
    pub lightmap_width: i32,
    pub lightmap_height: i32,
    pub lightmap_origin: [f32; 3],
    pub lightmap_vecs: [[f32; 3]; 3],
    pub patch_width: i32,
    pub patch_height: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BspFog {
    pub shader: String,
    pub brush: i32,
    pub visible_side: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bsp {
    pub lumps: [Lump; HEADER_LUMPS],
}

impl Bsp {
    pub fn lump(&self, kind: LumpKind) -> &Lump {
        &self.lumps[kind as usize]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BspError {
    Signature,
    Version(i32),
    Truncated { what: String },
    Bad(String),
}

impl fmt::Display for BspError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Signature => write!(f, "bsp: not an RBSP file"),
            Self::Version(v) => write!(f, "bsp: version {v}, expected {BSP_VERSION}"),
            Self::Truncated { what } => write!(f, "bsp: truncated {what}"),
            Self::Bad(what) => write!(f, "bsp: {what}"),
        }
    }
}

impl std::error::Error for BspError {}

/// Parse and validate the BSP header and lump ranges.
pub fn parse(bytes: &[u8]) -> Result<Bsp, BspError> {
    if bytes.len() < HEADER_SIZE {
        return Err(BspError::Truncated {
            what: format!("header ({} bytes, need {HEADER_SIZE})", bytes.len()),
        });
    }
    if &bytes[..4] != BSP_IDENT {
        return Err(BspError::Signature);
    }

    let version = read_i32(bytes, 4)?;
    if version != BSP_VERSION {
        return Err(BspError::Version(version));
    }

    let mut lumps = [Lump {
        offset: 0,
        length: 0,
    }; HEADER_LUMPS];

    for (index, lump) in lumps.iter_mut().enumerate() {
        let at = 8 + index * 8;
        let raw_offset = read_i32(bytes, at)?;
        let raw_length = read_i32(bytes, at + 4)?;
        let offset = usize::try_from(raw_offset)
            .map_err(|_| BspError::Bad(format!("lump {index} has negative offset {raw_offset}")))?;
        let length = usize::try_from(raw_length)
            .map_err(|_| BspError::Bad(format!("lump {index} has negative length {raw_length}")))?;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| BspError::Bad(format!("lump {index} range overflows")))?;

        if end > bytes.len() {
            return Err(BspError::Bad(format!(
                "lump {index} range {offset}..{end} exceeds file size {}",
                bytes.len()
            )));
        }
        *lump = Lump { offset, length };
    }

    Ok(Bsp { lumps })
}

/// Extract a lump after validating its alignment and file range.
fn lump_bytes<'a>(
    bytes: &'a [u8],
    bsp: &Bsp,
    kind: LumpKind,
    name: &str,
) -> Result<&'a [u8], BspError> {
    let lump = bsp.lump(kind);
    let end = lump
        .offset
        .checked_add(lump.length)
        .ok_or_else(|| BspError::Bad(format!("{name} lump range overflows")))?;

    bytes
        .get(lump.offset..end)
        .ok_or_else(|| BspError::Truncated {
            what: format!("{name} lump"),
        })
}

/// Extract a fixed-record lump and return its record count.
fn lump_data<'a>(
    bytes: &'a [u8],
    bsp: &Bsp,
    kind: LumpKind,
    name: &str,
    record_size: usize,
) -> Result<(&'a [u8], usize), BspError> {
    let data = lump_bytes(bytes, bsp, kind, name)?;

    if data.len() % record_size != 0 {
        return Err(BspError::Bad(format!(
            "{name} lump length {} is not divisible by {record_size}",
            data.len()
        )));
    }

    Ok((data, data.len() / record_size))
}

/// Parse the lightmap lump into individual RGB atlases.
pub fn parse_lightmaps(bytes: &[u8], bsp: &Bsp) -> Result<Vec<Vec<u8>>, BspError> {
    let (data, _) = lump_data(bytes, bsp, LumpKind::Lightmaps, "lightmap", LIGHTMAP_SIZE)?;

    Ok(data
        .chunks_exact(LIGHTMAP_SIZE)
        .map(|atlas| atlas.to_vec())
        .collect())
}

/// A key/value entity from the BSP entity lump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BspEntity {
    pub properties: BTreeMap<String, String>,
}

impl BspEntity {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.properties.get(key).map(String::as_str)
    }
}

/// Parse the text-based entity lump.
pub fn parse_entities(bytes: &[u8], bsp: &Bsp) -> Result<Vec<BspEntity>, BspError> {
    let data = lump_bytes(bytes, bsp, LumpKind::Entities, "entities")?;
    let text = std::str::from_utf8(data)
        .map_err(|error| BspError::Bad(format!("entities lump is not UTF-8: {error}")))?
        .trim_end_matches('\0');

    let mut entities = Vec::new();
    let mut chars = text.chars().peekable();

    loop {
        skip_entity_whitespace(&mut chars);

        let Some(next) = chars.next() else {
            break;
        };

        if next != '{' {
            return Err(BspError::Bad(format!(
                "expected '{{' in entities lump, found {next:?}"
            )));
        }

        let mut properties = BTreeMap::new();

        loop {
            skip_entity_whitespace(&mut chars);

            match chars.next() {
                Some('}') => break,
                Some('"') => {}
                Some(other) => {
                    return Err(BspError::Bad(format!(
                        "expected quoted entity key, found {other:?}"
                    )));
                }
                None => {
                    return Err(BspError::Bad("unterminated entity in entities lump".into()));
                }
            }

            let key = read_entity_string(&mut chars)?;

            skip_entity_whitespace(&mut chars);

            if chars.next() != Some('"') {
                return Err(BspError::Bad("expected quoted entity value".into()));
            }

            let value = read_entity_string(&mut chars)?;
            properties.insert(key, value);
        }

        entities.push(BspEntity { properties });
    }

    Ok(entities)
}

fn skip_entity_whitespace(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while chars.peek().is_some_and(|c| c.is_whitespace()) {
        chars.next();
    }
}

fn read_entity_string(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<String, BspError> {
    let mut value = String::new();

    loop {
        match chars.next() {
            Some('"') => return Ok(value),
            Some('\\') => match chars.next() {
                Some('"') => value.push('"'),
                Some('\\') => value.push('\\'),
                Some('n') => value.push('\n'),
                Some('t') => value.push('\t'),
                Some(other) => {
                    value.push('\\');
                    value.push(other);
                }
                None => {
                    return Err(BspError::Bad("unterminated escape in entity string".into()));
                }
            },
            Some(c) => value.push(c),
            None => {
                return Err(BspError::Bad("unterminated quoted entity string".into()));
            }
        }
    }
}

pub fn parse_shaders(bytes: &[u8], bsp: &Bsp) -> Result<Vec<BspShader>, BspError> {
    let (data, count) = lump_data(bytes, bsp, LumpKind::Shaders, "shader", SHADER_RECORD_SIZE)?;

    (0..count)
        .map(|i| {
            let at = i * SHADER_RECORD_SIZE;
            Ok(BspShader {
                name: read_string(&data[at..at + SHADER_NAME_SIZE]),
                surface_flags: read_i32(data, at + 64)?,
                content_flags: read_i32(data, at + 68)?,
            })
        })
        .collect()
}

pub fn parse_fogs(bytes: &[u8], bsp: &Bsp) -> Result<Vec<BspFog>, BspError> {
    let (data, count) = lump_data(bytes, bsp, LumpKind::Fogs, "fog", FOG_RECORD_SIZE)?;

    (0..count)
        .map(|i| {
            let at = i * FOG_RECORD_SIZE;
            Ok(BspFog {
                shader: read_string(&data[at..at + FOG_SHADER_NAME_SIZE]),
                brush: read_i32(data, at + 64)?,
                visible_side: read_i32(data, at + 68)?,
            })
        })
        .collect()
}

pub fn parse_planes(bytes: &[u8], bsp: &Bsp) -> Result<Vec<BspPlane>, BspError> {
    let (data, count) = lump_data(bytes, bsp, LumpKind::Planes, "plane", PLANE_RECORD_SIZE)?;

    (0..count)
        .map(|i| {
            let at = i * PLANE_RECORD_SIZE;
            Ok(BspPlane {
                normal: read_f32_array(data, at)?,
                dist: read_f32(data, at + 12)?,
            })
        })
        .collect()
}

pub fn parse_nodes(bytes: &[u8], bsp: &Bsp) -> Result<Vec<BspNode>, BspError> {
    let (data, count) = lump_data(bytes, bsp, LumpKind::Nodes, "node", NODE_RECORD_SIZE)?;

    (0..count)
        .map(|i| {
            let at = i * NODE_RECORD_SIZE;
            Ok(BspNode {
                plane: read_i32(data, at)?,
                children: read_i32_array(data, at + 4)?,
                mins: read_i32_array(data, at + 12)?,
                maxs: read_i32_array(data, at + 24)?,
            })
        })
        .collect()
}

pub fn parse_leaves(bytes: &[u8], bsp: &Bsp) -> Result<Vec<BspLeaf>, BspError> {
    let (data, count) = lump_data(bytes, bsp, LumpKind::Leafs, "leaf", LEAF_RECORD_SIZE)?;

    (0..count)
        .map(|i| {
            let at = i * LEAF_RECORD_SIZE;
            Ok(BspLeaf {
                cluster: read_i32(data, at)?,
                area: read_i32(data, at + 4)?,
                mins: read_i32_array(data, at + 8)?,
                maxs: read_i32_array(data, at + 20)?,
                first_leaf_surface: read_i32(data, at + 32)?,
                num_leaf_surfaces: read_i32(data, at + 36)?,
                first_leaf_brush: read_i32(data, at + 40)?,
                num_leaf_brushes: read_i32(data, at + 44)?,
            })
        })
        .collect()
}

/// Parse the BSP visibility lump (cluster count, row size, then PVS bitsets).
pub fn parse_visibility(bytes: &[u8], bsp: &Bsp) -> Result<Option<BspVisibility>, BspError> {
    if bsp.lump(LumpKind::Visibility).length == 0 {
        return Ok(None);
    }

    let data = lump_bytes(bytes, bsp, LumpKind::Visibility, "visibility")?;

    if data.len() < 8 {
        return Err(BspError::Bad(
            "visibility lump is shorter than its 8-byte header".into(),
        ));
    }

    let num_clusters = usize::try_from(read_i32(data, 0)?)
        .map_err(|_| BspError::Bad("negative visibility cluster count".into()))?;

    let bytes_per_cluster = usize::try_from(read_i32(data, 4)?)
        .map_err(|_| BspError::Bad("negative visibility row size".into()))?;

    let minimum_row_size = num_clusters / 8 + usize::from(num_clusters % 8 != 0);

    if bytes_per_cluster < minimum_row_size {
        return Err(BspError::Bad(format!(
            "visibility row size {bytes_per_cluster} is too small for {num_clusters} clusters"
        )));
    }

    let expected_size = num_clusters
        .checked_mul(bytes_per_cluster)
        .and_then(|size| size.checked_add(8))
        .ok_or_else(|| BspError::Bad("visibility lump size overflows".into()))?;

    if data.len() != expected_size {
        return Err(BspError::Bad(format!(
            "visibility lump has {} bytes, expected {expected_size}",
            data.len()
        )));
    }

    Ok(Some(BspVisibility {
        num_clusters,
        bytes_per_cluster,
        bitsets: data[8..].to_vec(),
    }))
}

pub fn parse_leaf_surfaces(bytes: &[u8], bsp: &Bsp) -> Result<Vec<i32>, BspError> {
    parse_i32_lump(bytes, bsp, LumpKind::LeafSurfaces, "leaf-surface")
}

pub fn parse_leaf_brushes(bytes: &[u8], bsp: &Bsp) -> Result<Vec<i32>, BspError> {
    parse_i32_lump(bytes, bsp, LumpKind::LeafBrushes, "leaf-brush")
}

pub fn parse_draw_indexes(bytes: &[u8], bsp: &Bsp) -> Result<Vec<i32>, BspError> {
    parse_i32_lump(bytes, bsp, LumpKind::DrawIndexes, "draw-index")
}

pub fn parse_models(bytes: &[u8], bsp: &Bsp) -> Result<Vec<BspModel>, BspError> {
    let (data, count) = lump_data(bytes, bsp, LumpKind::Models, "model", MODEL_RECORD_SIZE)?;

    (0..count)
        .map(|i| {
            let at = i * MODEL_RECORD_SIZE;
            Ok(BspModel {
                mins: read_f32_array(data, at)?,
                maxs: read_f32_array(data, at + 12)?,
                first_surface: read_i32(data, at + 24)?,
                num_surfaces: read_i32(data, at + 28)?,
                first_brush: read_i32(data, at + 32)?,
                num_brushes: read_i32(data, at + 36)?,
            })
        })
        .collect()
}

pub fn parse_brushes(bytes: &[u8], bsp: &Bsp) -> Result<Vec<BspBrush>, BspError> {
    let (data, count) = lump_data(bytes, bsp, LumpKind::Brushes, "brush", BRUSH_RECORD_SIZE)?;

    (0..count)
        .map(|i| {
            let at = i * BRUSH_RECORD_SIZE;
            Ok(BspBrush {
                first_side: read_i32(data, at)?,
                num_sides: read_i32(data, at + 4)?,
                shader: read_i32(data, at + 8)?,
            })
        })
        .collect()
}

pub fn parse_brush_sides(bytes: &[u8], bsp: &Bsp) -> Result<Vec<BspBrushSide>, BspError> {
    let (data, count) = lump_data(
        bytes,
        bsp,
        LumpKind::BrushSides,
        "brush-side",
        BRUSH_SIDE_RECORD_SIZE,
    )?;

    (0..count)
        .map(|i| {
            let at = i * BRUSH_SIDE_RECORD_SIZE;
            Ok(BspBrushSide {
                plane: read_i32(data, at)?,
                shader: read_i32(data, at + 4)?,
                draw_surface: read_i32(data, at + 8)?,
            })
        })
        .collect()
}

pub fn parse_draw_verts(bytes: &[u8], bsp: &Bsp) -> Result<Vec<BspDrawVert>, BspError> {
    let (data, count) = lump_data(
        bytes,
        bsp,
        LumpKind::DrawVerts,
        "draw-vertex",
        DRAW_VERT_RECORD_SIZE,
    )?;

    (0..count)
        .map(|i| {
            let at = i * DRAW_VERT_RECORD_SIZE;
            let mut lightmap_coords = [[0.0; 2]; MAX_LIGHTMAPS];
            let mut color = [[0; 4]; MAX_LIGHTMAPS];

            for style in 0..MAX_LIGHTMAPS {
                lightmap_coords[style] = read_f32_array(data, at + 20 + style * 8)?;
                let color_at = at + 64 + style * 4;
                color[style].copy_from_slice(&data[color_at..color_at + 4]);
            }

            Ok(BspDrawVert {
                position: read_f32_array(data, at)?,
                tex_coords: read_f32_array(data, at + 12)?,
                lightmap_coords,
                normal: read_f32_array(data, at + 52)?,
                color,
            })
        })
        .collect()
}

fn parse_i32_lump(
    bytes: &[u8],
    bsp: &Bsp,
    kind: LumpKind,
    name: &str,
) -> Result<Vec<i32>, BspError> {
    let (data, count) = lump_data(bytes, bsp, kind, name, INDEX_RECORD_SIZE)?;
    (0..count)
        .map(|i| read_i32(data, i * INDEX_RECORD_SIZE))
        .collect()
}

fn read_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn read_i32_array<const N: usize>(bytes: &[u8], at: usize) -> Result<[i32; N], BspError> {
    let mut values = [0; N];
    for (i, value) in values.iter_mut().enumerate() {
        *value = read_i32(bytes, at + i * 4)?;
    }
    Ok(values)
}

fn read_f32_array<const N: usize>(bytes: &[u8], at: usize) -> Result<[f32; N], BspError> {
    let mut values = [0.0; N];
    for (i, value) in values.iter_mut().enumerate() {
        *value = read_f32(bytes, at + i * 4)?;
    }
    Ok(values)
}

fn read_i32(bytes: &[u8], at: usize) -> Result<i32, BspError> {
    let raw = bytes.get(at..at + 4).ok_or_else(|| BspError::Truncated {
        what: format!("32-bit integer at offset {at}"),
    })?;
    Ok(i32::from_le_bytes(raw.try_into().expect("four bytes")))
}

fn read_f32(bytes: &[u8], at: usize) -> Result<f32, BspError> {
    let raw = bytes.get(at..at + 4).ok_or_else(|| BspError::Truncated {
        what: format!("32-bit float at offset {at}"),
    })?;
    Ok(f32::from_bits(u32::from_le_bytes(
        raw.try_into().expect("four bytes"),
    )))
}

pub fn parse_surfaces(bytes: &[u8], bsp: &Bsp) -> Result<Vec<BspSurface>, BspError> {
    let (data, count) = lump_data(
        bytes,
        bsp,
        LumpKind::Surfaces,
        "surface",
        SURFACE_RECORD_SIZE,
    )?;

    (0..count)
        .map(|i| {
            let at = i * SURFACE_RECORD_SIZE;
            let mut lightmap_styles = [0; MAX_LIGHTMAPS];
            let mut vertex_styles = [0; MAX_LIGHTMAPS];
            lightmap_styles.copy_from_slice(&data[at + 28..at + 32]);
            vertex_styles.copy_from_slice(&data[at + 32..at + 36]);

            Ok(BspSurface {
                shader: read_i32(data, at)?,
                fog: read_i32(data, at + 4)?,
                surface_type: read_i32(data, at + 8)?,
                first_vert: read_i32(data, at + 12)?,
                num_verts: read_i32(data, at + 16)?,
                first_index: read_i32(data, at + 20)?,
                num_indexes: read_i32(data, at + 24)?,
                lightmap_styles,
                vertex_styles,
                lightmap_num: read_i32_array(data, at + 36)?,
                lightmap_x: read_i32_array(data, at + 52)?,
                lightmap_y: read_i32_array(data, at + 68)?,
                lightmap_width: read_i32(data, at + 84)?,
                lightmap_height: read_i32(data, at + 88)?,
                lightmap_origin: read_f32_array(data, at + 92)?,
                lightmap_vecs: [
                    read_f32_array(data, at + 104)?,
                    read_f32_array(data, at + 116)?,
                    read_f32_array(data, at + 128)?,
                ],
                patch_width: read_i32(data, at + 140)?,
                patch_height: read_i32(data, at + 144)?,
            })
        })
        .collect()
}

// Shared cross-reference validation helpers.

fn validate_reference(
    value: i32,
    count: usize,
    record: &str,
    target: &str,
    index: usize,
) -> Result<(), BspError> {
    if value < 0 || value as usize >= count {
        return Err(BspError::Bad(format!(
            "{record} {index} references invalid {target} {value}"
        )));
    }
    Ok(())
}

fn checked_range(
    first: i32,
    count: i32,
    total: usize,
    record: &str,
    index: usize,
) -> Result<std::ops::Range<usize>, BspError> {
    let start = usize::try_from(first)
        .map_err(|_| BspError::Bad(format!("{record} {index} has negative start {first}")))?;
    let count = usize::try_from(count)
        .map_err(|_| BspError::Bad(format!("{record} {index} has negative count {count}")))?;
    let end = start
        .checked_add(count)
        .ok_or_else(|| BspError::Bad(format!("{record} {index} range overflows")))?;

    if end > total {
        return Err(BspError::Bad(format!(
            "{record} {index} range {start}..{end} exceeds {total} entries"
        )));
    }
    Ok(start..end)
}

/// Validate node references to planes.
pub fn validate_nodes(nodes: &[BspNode], planes: &[BspPlane]) -> Result<(), BspError> {
    for (i, node) in nodes.iter().enumerate() {
        validate_reference(node.plane, planes.len(), "node", "plane", i)?;
    }
    Ok(())
}

/// Validate brush-side references to planes and shaders.
pub fn validate_brush_sides(
    sides: &[BspBrushSide],
    planes: &[BspPlane],
    shaders: &[BspShader],
) -> Result<(), BspError> {
    for (i, side) in sides.iter().enumerate() {
        validate_reference(side.plane, planes.len(), "brush side", "plane", i)?;
        validate_reference(side.shader, shaders.len(), "brush side", "shader", i)?;
    }
    Ok(())
}

/// Validate surface references to shaders, vertices and draw indexes.
pub fn validate_surfaces(
    surfaces: &[BspSurface],
    shaders: &[BspShader],
    draw_verts: &[BspDrawVert],
    draw_indexes: &[i32],
) -> Result<(), BspError> {
    for (i, surface) in surfaces.iter().enumerate() {
        validate_reference(surface.shader, shaders.len(), "surface", "shader", i)?;
        checked_range(
            surface.first_vert,
            surface.num_verts,
            draw_verts.len(),
            "surface vertex",
            i,
        )?;

        let indexes = checked_range(
            surface.first_index,
            surface.num_indexes,
            draw_indexes.len(),
            "surface index",
            i,
        )?;

        for (offset, &vertex) in draw_indexes[indexes.clone()].iter().enumerate() {
            validate_reference(
                vertex,
                draw_verts.len(),
                "draw index",
                "vertex",
                indexes.start + offset,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_header() -> Vec<u8> {
        let mut bytes = vec![0; HEADER_SIZE];
        bytes[..4].copy_from_slice(BSP_IDENT);
        bytes[4..8].copy_from_slice(&BSP_VERSION.to_le_bytes());
        bytes
    }

    fn write_i32(bytes: &mut [u8], at: usize, value: i32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn set_lump(bytes: &mut Vec<u8>, kind: LumpKind, data: &[u8]) -> Bsp {
        let offset = bytes.len();
        bytes.extend_from_slice(data);
        write_i32(bytes, 8 + kind as usize * 8, offset as i32);
        write_i32(bytes, 12 + kind as usize * 8, data.len() as i32);
        parse(bytes).unwrap()
    }

    fn test_surface() -> BspSurface {
        BspSurface {
            shader: 0,
            fog: -1,
            surface_type: 1,
            first_vert: 0,
            num_verts: 1,
            first_index: 0,
            num_indexes: 1,
            lightmap_styles: [0; MAX_LIGHTMAPS],
            vertex_styles: [0; MAX_LIGHTMAPS],
            lightmap_num: [0; MAX_LIGHTMAPS],
            lightmap_x: [0; MAX_LIGHTMAPS],
            lightmap_y: [0; MAX_LIGHTMAPS],
            lightmap_width: 0,
            lightmap_height: 0,
            lightmap_origin: [0.0; 3],
            lightmap_vecs: [[0.0; 3]; 3],
            patch_width: 0,
            patch_height: 0,
        }
    }

    fn test_shader() -> BspShader {
        BspShader {
            name: "textures/test".into(),
            surface_flags: 0,
            content_flags: 0,
        }
    }

    fn test_draw_vert() -> BspDrawVert {
        BspDrawVert {
            position: [0.0; 3],
            tex_coords: [0.0; 2],
            lightmap_coords: [[0.0; 2]; MAX_LIGHTMAPS],
            normal: [0.0; 3],
            color: [[0; 4]; MAX_LIGHTMAPS],
        }
    }

    #[test]
    fn parses_valid_empty_header() {
        let bsp = parse(&empty_header()).unwrap();
        assert_eq!(bsp.lumps.len(), HEADER_LUMPS);
        assert!(bsp.lumps.iter().all(|l| l.offset == 0 && l.length == 0));
    }

    #[test]
    fn rejects_wrong_identification() {
        let mut bytes = empty_header();
        bytes[..4].copy_from_slice(b"IBSP");
        assert_eq!(parse(&bytes), Err(BspError::Signature));
    }

    #[test]
    fn rejects_unsupported_version() {
        let mut bytes = empty_header();
        write_i32(&mut bytes, 4, 2);
        assert_eq!(parse(&bytes), Err(BspError::Version(2)));
    }

    #[test]
    fn rejects_truncated_header() {
        assert!(matches!(
            parse(&vec![0; HEADER_SIZE - 1]),
            Err(BspError::Truncated { .. })
        ));
    }

    #[test]
    fn rejects_negative_lump_offset() {
        let mut bytes = empty_header();
        write_i32(&mut bytes, 8, -1);
        assert!(matches!(parse(&bytes), Err(BspError::Bad(_))));
    }

    #[test]
    fn rejects_negative_lump_length() {
        let mut bytes = empty_header();
        write_i32(&mut bytes, 12, -1);
        assert!(matches!(parse(&bytes), Err(BspError::Bad(_))));
    }

    #[test]
    fn rejects_lump_offset_beyond_file() {
        let mut bytes = empty_header();
        write_i32(&mut bytes, 8, (HEADER_SIZE + 1) as i32);
        assert!(matches!(parse(&bytes), Err(BspError::Bad(_))));
    }

    #[test]
    fn parses_shader_records() {
        let mut bytes = empty_header();
        let mut record = vec![0; SHADER_RECORD_SIZE];
        record[..13].copy_from_slice(b"textures/test");
        write_i32(&mut record, 64, 123);
        write_i32(&mut record, 68, 456);
        let bsp = set_lump(&mut bytes, LumpKind::Shaders, &record);
        let shaders = parse_shaders(&bytes, &bsp).unwrap();
        assert_eq!(shaders.len(), 1);
        assert_eq!(shaders[0].name, "textures/test");
        assert_eq!(shaders[0].surface_flags, 123);
        assert_eq!(shaders[0].content_flags, 456);
    }

    #[test]
    fn rejects_incomplete_shader_record() {
        let mut bytes = empty_header();
        let bsp = set_lump(&mut bytes, LumpKind::Shaders, &[0; SHADER_RECORD_SIZE - 1]);
        assert!(matches!(parse_shaders(&bytes, &bsp), Err(BspError::Bad(_))));
    }

    #[test]
    fn parses_shader_name_without_null_terminator() {
        let mut bytes = empty_header();
        let bsp = set_lump(
            &mut bytes,
            LumpKind::Shaders,
            &vec![b'a'; SHADER_RECORD_SIZE],
        );
        assert_eq!(
            parse_shaders(&bytes, &bsp).unwrap()[0].name.len(),
            SHADER_NAME_SIZE
        );
    }

    #[test]
    fn parses_plane_records() {
        let mut bytes = empty_header();
        let values = [0.0f32, 1.0, 0.0, 128.5];
        let mut record = vec![0; PLANE_RECORD_SIZE];
        for (i, value) in values.iter().enumerate() {
            record[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let bsp = set_lump(&mut bytes, LumpKind::Planes, &record);
        let planes = parse_planes(&bytes, &bsp).unwrap();
        assert_eq!(planes.len(), 1);
        assert_eq!(planes[0].normal, [0.0, 1.0, 0.0]);
        assert_eq!(planes[0].dist, 128.5);
    }

    #[test]
    fn rejects_incomplete_plane_record() {
        let mut bytes = empty_header();
        let bsp = set_lump(&mut bytes, LumpKind::Planes, &[0; PLANE_RECORD_SIZE - 1]);
        assert!(matches!(parse_planes(&bytes, &bsp), Err(BspError::Bad(_))));
    }

    #[test]
    fn parses_node_records() {
        let mut bytes = empty_header();
        let mut record = vec![0; NODE_RECORD_SIZE];
        for (i, value) in [3, -1, 2, -128, -64, 0, 128, 64, 256].iter().enumerate() {
            write_i32(&mut record, i * 4, *value);
        }
        let bsp = set_lump(&mut bytes, LumpKind::Nodes, &record);
        let nodes = parse_nodes(&bytes, &bsp).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].plane, 3);
        assert_eq!(nodes[0].children, [-1, 2]);
        assert_eq!(nodes[0].mins, [-128, -64, 0]);
        assert_eq!(nodes[0].maxs, [128, 64, 256]);
    }

    #[test]
    fn rejects_incomplete_node_record() {
        let mut bytes = empty_header();
        let bsp = set_lump(&mut bytes, LumpKind::Nodes, &[0; NODE_RECORD_SIZE - 1]);
        assert!(matches!(parse_nodes(&bytes, &bsp), Err(BspError::Bad(_))));
    }

    #[test]
    fn parses_leaf_records() {
        let mut bytes = empty_header();
        let mut record = vec![0; LEAF_RECORD_SIZE];
        for (i, value) in [7, 2, -128, -64, 0, 128, 64, 256, 10, 4, 20, 3]
            .iter()
            .enumerate()
        {
            write_i32(&mut record, i * 4, *value);
        }
        let bsp = set_lump(&mut bytes, LumpKind::Leafs, &record);
        let leaves = parse_leaves(&bytes, &bsp).unwrap();
        assert_eq!(leaves.len(), 1);
        assert_eq!(leaves[0].cluster, 7);
        assert_eq!(leaves[0].area, 2);
        assert_eq!(leaves[0].mins, [-128, -64, 0]);
        assert_eq!(leaves[0].maxs, [128, 64, 256]);
        assert_eq!(leaves[0].first_leaf_surface, 10);
        assert_eq!(leaves[0].num_leaf_surfaces, 4);
        assert_eq!(leaves[0].first_leaf_brush, 20);
        assert_eq!(leaves[0].num_leaf_brushes, 3);
    }

    #[test]
    fn rejects_incomplete_leaf_record() {
        let mut bytes = empty_header();
        let bsp = set_lump(&mut bytes, LumpKind::Leafs, &[0; LEAF_RECORD_SIZE - 1]);
        assert!(matches!(parse_leaves(&bytes, &bsp), Err(BspError::Bad(_))));
    }

    #[test]
    fn parses_leaf_surface_indices() {
        let mut bytes = empty_header();
        let values: [i32; 4] = [3, 7, 12, 25];
        let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let bsp = set_lump(&mut bytes, LumpKind::LeafSurfaces, &data);
        assert_eq!(parse_leaf_surfaces(&bytes, &bsp).unwrap(), values);
    }

    #[test]
    fn rejects_incomplete_leaf_surface_index() {
        let mut bytes = empty_header();
        let bsp = set_lump(
            &mut bytes,
            LumpKind::LeafSurfaces,
            &[0; INDEX_RECORD_SIZE - 1],
        );
        assert!(matches!(
            parse_leaf_surfaces(&bytes, &bsp),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn parses_leaf_brush_indices() {
        let mut bytes = empty_header();
        let values: [i32; 3] = [2, 5, 9];
        let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let bsp = set_lump(&mut bytes, LumpKind::LeafBrushes, &data);
        assert_eq!(parse_leaf_brushes(&bytes, &bsp).unwrap(), values);
    }

    #[test]
    fn rejects_incomplete_leaf_brush_index() {
        let mut bytes = empty_header();
        let bsp = set_lump(
            &mut bytes,
            LumpKind::LeafBrushes,
            &[0; INDEX_RECORD_SIZE - 1],
        );
        assert!(matches!(
            parse_leaf_brushes(&bytes, &bsp),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn parses_model_records() {
        let mut bytes = empty_header();
        let mut record = vec![0; MODEL_RECORD_SIZE];
        let mins = [-128.5f32, -64.0, -32.25];
        let maxs = [128.5f32, 64.0, 32.25];
        for (i, value) in mins.iter().enumerate() {
            record[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (i, value) in maxs.iter().enumerate() {
            let at = 12 + i * 4;
            record[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (i, value) in [10, 4, 20, 3].iter().enumerate() {
            write_i32(&mut record, 24 + i * 4, *value);
        }
        let bsp = set_lump(&mut bytes, LumpKind::Models, &record);
        let models = parse_models(&bytes, &bsp).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].mins, mins);
        assert_eq!(models[0].maxs, maxs);
        assert_eq!(models[0].first_surface, 10);
        assert_eq!(models[0].num_surfaces, 4);
        assert_eq!(models[0].first_brush, 20);
        assert_eq!(models[0].num_brushes, 3);
    }

    #[test]
    fn rejects_incomplete_model_record() {
        let mut bytes = empty_header();
        let bsp = set_lump(&mut bytes, LumpKind::Models, &[0; MODEL_RECORD_SIZE - 1]);
        assert!(matches!(parse_models(&bytes, &bsp), Err(BspError::Bad(_))));
    }

    #[test]
    fn parses_brush_records() {
        let mut bytes = empty_header();
        let values: [i32; 3] = [5, 6, 2];
        let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let bsp = set_lump(&mut bytes, LumpKind::Brushes, &data);
        let brushes = parse_brushes(&bytes, &bsp).unwrap();
        assert_eq!(brushes.len(), 1);
        assert_eq!(brushes[0].first_side, 5);
        assert_eq!(brushes[0].num_sides, 6);
        assert_eq!(brushes[0].shader, 2);
    }

    #[test]
    fn rejects_incomplete_brush_record() {
        let mut bytes = empty_header();
        let bsp = set_lump(&mut bytes, LumpKind::Brushes, &[0; BRUSH_RECORD_SIZE - 1]);
        assert!(matches!(parse_brushes(&bytes, &bsp), Err(BspError::Bad(_))));
    }

    #[test]
    fn parses_brush_side_records() {
        let mut bytes = empty_header();
        let values: [i32; 3] = [12, 3, 7];
        let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let bsp = set_lump(&mut bytes, LumpKind::BrushSides, &data);
        let sides = parse_brush_sides(&bytes, &bsp).unwrap();
        assert_eq!(sides.len(), 1);
        assert_eq!(sides[0].plane, 12);
        assert_eq!(sides[0].shader, 3);
        assert_eq!(sides[0].draw_surface, 7);
    }

    #[test]
    fn rejects_incomplete_brush_side_record() {
        let mut bytes = empty_header();
        let bsp = set_lump(
            &mut bytes,
            LumpKind::BrushSides,
            &[0; BRUSH_SIDE_RECORD_SIZE - 1],
        );
        assert!(matches!(
            parse_brush_sides(&bytes, &bsp),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn parses_draw_vertex_records() {
        let mut bytes = empty_header();
        let mut record = vec![0; DRAW_VERT_RECORD_SIZE];
        for (i, value) in [10.0f32, 20.0, 30.0, 0.25, 0.75].iter().enumerate() {
            let at = i * 4;
            record[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        for style in 0..MAX_LIGHTMAPS {
            let at = 20 + style * 8;
            record[at..at + 4].copy_from_slice(&(style as f32).to_le_bytes());
            record[at + 4..at + 8].copy_from_slice(&((style as f32) + 0.5).to_le_bytes());
        }
        for (i, value) in [0.0f32, 0.0, 1.0].iter().enumerate() {
            let at = 52 + i * 4;
            record[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        for style in 0..MAX_LIGHTMAPS {
            let at = 64 + style * 4;
            record[at..at + 4].copy_from_slice(&[255 - style as u8, 128, 64, 32]);
        }
        let bsp = set_lump(&mut bytes, LumpKind::DrawVerts, &record);
        let vertices = parse_draw_verts(&bytes, &bsp).unwrap();
        assert_eq!(vertices.len(), 1);
        assert_eq!(vertices[0].position, [10.0, 20.0, 30.0]);
        assert_eq!(vertices[0].tex_coords, [0.25, 0.75]);
        assert_eq!(
            vertices[0].lightmap_coords,
            [[0.0, 0.5], [1.0, 1.5], [2.0, 2.5], [3.0, 3.5]]
        );
        assert_eq!(vertices[0].normal, [0.0, 0.0, 1.0]);
        assert_eq!(
            vertices[0].color,
            [
                [255, 128, 64, 32],
                [254, 128, 64, 32],
                [253, 128, 64, 32],
                [252, 128, 64, 32],
            ]
        );
    }

    #[test]
    fn rejects_incomplete_draw_vertex_record() {
        let mut bytes = empty_header();
        let bsp = set_lump(
            &mut bytes,
            LumpKind::DrawVerts,
            &[0; DRAW_VERT_RECORD_SIZE - 1],
        );
        assert!(matches!(
            parse_draw_verts(&bytes, &bsp),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn parses_draw_indexes() {
        let mut bytes = empty_header();
        let values: [i32; 3] = [0, 2, 1];
        let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let bsp = set_lump(&mut bytes, LumpKind::DrawIndexes, &data);
        assert_eq!(parse_draw_indexes(&bytes, &bsp).unwrap(), values);
    }

    #[test]
    fn rejects_incomplete_draw_index() {
        let mut bytes = empty_header();
        let bsp = set_lump(
            &mut bytes,
            LumpKind::DrawIndexes,
            &[0; INDEX_RECORD_SIZE - 1],
        );
        assert!(matches!(
            parse_draw_indexes(&bytes, &bsp),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn parses_fog_records() {
        let mut bytes = empty_header();
        let mut record = vec![0; FOG_RECORD_SIZE];
        record[..12].copy_from_slice(b"textures/fog");
        write_i32(&mut record, 64, 42);
        write_i32(&mut record, 68, 7);
        let bsp = set_lump(&mut bytes, LumpKind::Fogs, &record);
        let fogs = parse_fogs(&bytes, &bsp).unwrap();
        assert_eq!(fogs.len(), 1);
        assert_eq!(fogs[0].shader, "textures/fog");
        assert_eq!(fogs[0].brush, 42);
        assert_eq!(fogs[0].visible_side, 7);
    }

    #[test]
    fn rejects_incomplete_fog_record() {
        let mut bytes = empty_header();
        let bsp = set_lump(&mut bytes, LumpKind::Fogs, &[0; FOG_RECORD_SIZE - 1]);
        assert!(matches!(parse_fogs(&bytes, &bsp), Err(BspError::Bad(_))));
    }

    #[test]
    fn parses_surface_records() {
        let mut record = vec![0u8; SURFACE_RECORD_SIZE];
        for (offset, value) in [
            (0, 1i32),
            (4, 2),
            (8, 3),
            (12, 4),
            (16, 5),
            (20, 6),
            (24, 7),
        ] {
            record[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        record[28..32].copy_from_slice(&[1, 2, 3, 4]);
        record[32..36].copy_from_slice(&[5, 6, 7, 8]);

        for (base, values) in [
            (36, [10i32, 11, 12, 13]),
            (52, [20, 21, 22, 23]),
            (68, [30, 31, 32, 33]),
        ] {
            for (i, value) in values.iter().enumerate() {
                let at = base + i * 4;
                record[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }

        record[84..88].copy_from_slice(&64i32.to_le_bytes());
        record[88..92].copy_from_slice(&128i32.to_le_bytes());

        for (i, value) in [1.0f32, 2.0, 3.0].iter().enumerate() {
            let at = 92 + i * 4;
            record[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }

        let vectors = [[1.0f32, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        for (row, vector) in vectors.iter().enumerate() {
            for (column, value) in vector.iter().enumerate() {
                let at = 104 + row * 12 + column * 4;
                record[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }

        record[140..144].copy_from_slice(&9i32.to_le_bytes());
        record[144..148].copy_from_slice(&10i32.to_le_bytes());

        let mut bytes = empty_header();
        let bsp = set_lump(&mut bytes, LumpKind::Surfaces, &record);
        let surfaces = parse_surfaces(&bytes, &bsp).unwrap();
        assert_eq!(surfaces.len(), 1);

        let s = &surfaces[0];
        assert_eq!(
            [
                s.shader,
                s.fog,
                s.surface_type,
                s.first_vert,
                s.num_verts,
                s.first_index,
                s.num_indexes
            ],
            [1, 2, 3, 4, 5, 6, 7]
        );
        assert_eq!(s.lightmap_styles, [1, 2, 3, 4]);
        assert_eq!(s.vertex_styles, [5, 6, 7, 8]);
        assert_eq!(s.lightmap_num, [10, 11, 12, 13]);
        assert_eq!(s.lightmap_x, [20, 21, 22, 23]);
        assert_eq!(s.lightmap_y, [30, 31, 32, 33]);
        assert_eq!(s.lightmap_width, 64);
        assert_eq!(s.lightmap_height, 128);
        assert_eq!(s.lightmap_origin, [1.0, 2.0, 3.0]);
        assert_eq!(s.lightmap_vecs, vectors);
        assert_eq!(s.patch_width, 9);
        assert_eq!(s.patch_height, 10);
    }

    #[test]
    fn rejects_incomplete_surface_records() {
        let mut bytes = empty_header();
        let bsp = set_lump(
            &mut bytes,
            LumpKind::Surfaces,
            &[0; SURFACE_RECORD_SIZE - 1],
        );
        assert!(matches!(
            parse_surfaces(&bytes, &bsp),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn validates_surface_references() {
        assert!(
            validate_surfaces(
                &[test_surface()],
                &[test_shader()],
                &[test_draw_vert()],
                &[0],
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_surface_with_invalid_shader() {
        let mut surface = test_surface();
        surface.shader = 1;
        assert!(matches!(
            validate_surfaces(&[surface], &[test_shader()], &[], &[]),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn rejects_surface_with_invalid_vertex_range() {
        let mut surface = test_surface();
        surface.first_vert = 2;
        surface.num_verts = 2;
        assert!(matches!(
            validate_surfaces(&[surface], &[test_shader()], &[test_draw_vert()], &[]),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn rejects_surface_with_invalid_index_range() {
        let mut surface = test_surface();
        surface.num_indexes = 2;
        assert!(matches!(
            validate_surfaces(&[surface], &[test_shader()], &[test_draw_vert()], &[0]),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn rejects_surface_with_invalid_draw_index() {
        assert!(matches!(
            validate_surfaces(
                &[test_surface()],
                &[test_shader()],
                &[test_draw_vert()],
                &[1],
            ),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn validates_node_plane_references() {
        let nodes = [BspNode {
            plane: 0,
            children: [0; 2],
            mins: [0; 3],
            maxs: [0; 3],
        }];
        let planes = [BspPlane {
            normal: [0.0; 3],
            dist: 0.0,
        }];
        assert!(validate_nodes(&nodes, &planes).is_ok());
    }

    #[test]
    fn rejects_node_with_invalid_plane() {
        let nodes = [BspNode {
            plane: 1,
            children: [0; 2],
            mins: [0; 3],
            maxs: [0; 3],
        }];
        let planes = [BspPlane {
            normal: [0.0; 3],
            dist: 0.0,
        }];
        assert!(matches!(
            validate_nodes(&nodes, &planes),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn validates_brush_side_references() {
        let sides = [BspBrushSide {
            plane: 0,
            shader: 0,
            draw_surface: -1,
        }];
        let planes = [BspPlane {
            normal: [0.0; 3],
            dist: 0.0,
        }];
        assert!(validate_brush_sides(&sides, &planes, &[test_shader()]).is_ok());
    }

    #[test]
    fn rejects_brush_side_with_invalid_plane() {
        let sides = [BspBrushSide {
            plane: 1,
            shader: 0,
            draw_surface: -1,
        }];
        let planes = [BspPlane {
            normal: [0.0; 3],
            dist: 0.0,
        }];
        assert!(matches!(
            validate_brush_sides(&sides, &planes, &[test_shader()]),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn rejects_brush_side_with_invalid_shader() {
        let sides = [BspBrushSide {
            plane: 0,
            shader: 1,
            draw_surface: -1,
        }];
        let planes = [BspPlane {
            normal: [0.0; 3],
            dist: 0.0,
        }];
        assert!(matches!(
            validate_brush_sides(&sides, &planes, &[test_shader()]),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn parses_empty_lightmap_lump() {
        let mut bytes = empty_header();
        let bsp = set_lump(&mut bytes, LumpKind::Lightmaps, &[]);

        assert!(parse_lightmaps(&bytes, &bsp).unwrap().is_empty());
    }

    #[test]
    fn parses_lightmap_atlas() {
        let pixels = vec![127u8; LIGHTMAP_SIZE];
        let mut bytes = empty_header();
        let bsp = set_lump(&mut bytes, LumpKind::Lightmaps, &pixels);

        let lightmaps = parse_lightmaps(&bytes, &bsp).unwrap();

        assert_eq!(lightmaps.len(), 1);
        assert_eq!(lightmaps[0], pixels);
    }

    #[test]
    fn rejects_incomplete_lightmap_atlas() {
        let pixels = vec![127u8; LIGHTMAP_SIZE - 1];
        let mut bytes = empty_header();
        let bsp = set_lump(&mut bytes, LumpKind::Lightmaps, &pixels);

        assert!(matches!(
            parse_lightmaps(&bytes, &bsp),
            Err(BspError::Bad(_))
        ));
    }
    #[test]
    fn parses_visibility_lump() {
        let mut bytes = empty_header();
        let mut data = vec![0; 8 + 2 * 1];

        write_i32(&mut data, 0, 2);
        write_i32(&mut data, 4, 1);

        // Cluster 0 can see clusters 0 and 1.
        data[8] = 0b0000_0011;

        // Cluster 1 can see cluster 1, but not cluster 0.
        data[9] = 0b0000_0010;

        let bsp = set_lump(&mut bytes, LumpKind::Visibility, &data);
        let visibility = parse_visibility(&bytes, &bsp).unwrap().unwrap();

        assert_eq!(visibility.num_clusters, 2);
        assert_eq!(visibility.bytes_per_cluster, 1);

        assert!(visibility.is_visible(0, 0));
        assert!(visibility.is_visible(0, 1));
        assert!(!visibility.is_visible(1, 0));
        assert!(visibility.is_visible(1, 1));

        assert!(!visibility.is_visible(-1, 0));
        assert!(!visibility.is_visible(0, -1));
        assert!(!visibility.is_visible(2, 0));
        assert!(!visibility.is_visible(0, 2));
    }

    #[test]
    fn returns_none_for_empty_visibility_lump() {
        let mut bytes = empty_header();
        let bsp = set_lump(&mut bytes, LumpKind::Visibility, &[]);

        assert_eq!(parse_visibility(&bytes, &bsp).unwrap(), None);
    }

    #[test]
    fn rejects_visibility_lump_with_short_header() {
        let mut bytes = empty_header();
        let bsp = set_lump(&mut bytes, LumpKind::Visibility, &[0; 7]);

        assert!(matches!(
            parse_visibility(&bytes, &bsp),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn rejects_negative_visibility_cluster_count() {
        let mut bytes = empty_header();
        let mut data = vec![0; 8];

        write_i32(&mut data, 0, -1);

        let bsp = set_lump(&mut bytes, LumpKind::Visibility, &data);

        assert!(matches!(
            parse_visibility(&bytes, &bsp),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn rejects_visibility_row_too_small() {
        let mut bytes = empty_header();
        let mut data = vec![0; 9];

        write_i32(&mut data, 0, 9);
        write_i32(&mut data, 4, 1);

        let bsp = set_lump(&mut bytes, LumpKind::Visibility, &data);

        assert!(matches!(
            parse_visibility(&bytes, &bsp),
            Err(BspError::Bad(_))
        ));
    }

    #[test]
    fn rejects_incomplete_visibility_bitsets() {
        let mut bytes = empty_header();
        let mut data = vec![0; 9];

        write_i32(&mut data, 0, 2);
        write_i32(&mut data, 4, 1);

        let bsp = set_lump(&mut bytes, LumpKind::Visibility, &data);

        assert!(matches!(
            parse_visibility(&bytes, &bsp),
            Err(BspError::Bad(_))
        ));
    }
}

//! A Jedi Academy BSP map as collision (`collision::Level`).
//!
//! Every planar and triangle-soup surface of the world becomes a solid
//! triangle with material 0 (plain, walkable solid). Patches are drawn but
//! not collided; inline brush models (`*N`, doors and platforms) are
//! their own brushes, not the world. Shaders that name a clip-only
//! surface — `common/nodraw`, `common/clip`, `common/caulk`,
//! `common/hint`, `common/skip`, `common/trigger`, `common/areaportal`,
//! `common/origin`, `common/sky`, anything under `skies/` — are left out.
//!
//! BSP coordinates are Quake units; `0.0254` is the metre conversion.

use collision::{Contents, Level, LevelBuilder, Tag};
use fmt_ja::bsp;

const UNIT: f32 = 0.0254;

/// The world geometry of `bytes` as a `collision::Level`.
pub fn build_bsp(bytes: &[u8]) -> Result<Level, bsp::BspError> {
    let parsed = bsp::parse(bytes)?;
    bsp::validate_references(bytes, &parsed)?;
    let surfaces = bsp::parse_surfaces(bytes, &parsed)?;
    let shaders = bsp::parse_shaders(bytes, &parsed)?;
    let verts = bsp::parse_draw_verts(bytes, &parsed)?;
    let indices = bsp::parse_draw_indexes(bytes, &parsed)?;

    let mut builder = LevelBuilder::new();

    for surface in &surfaces {
        if surface.surface_type != 1 && surface.surface_type != 3 {
            continue;
        }
        let shader_name = shaders
            .get(surface.shader as usize)
            .map(|s| s.name.to_ascii_lowercase())
            .unwrap_or_default();
        if [
            "/nodraw",
            "/clip",
            "/caulk",
            "/hint",
            "/skip",
            "/trigger",
            "/areaportal",
            "/origin",
            "/sky",
            "skies/",
        ]
        .iter()
        .any(|needle| shader_name.contains(needle))
        {
            continue;
        }
        let (Ok(first_vert), Ok(first_index), Ok(num_indexes)) = (
            usize::try_from(surface.first_vert),
            usize::try_from(surface.first_index),
            usize::try_from(surface.num_indexes),
        ) else {
            continue;
        };
        let Some(tris) = indices.get(first_index..first_index + num_indexes) else {
            continue;
        };
        for tri in tris.chunks_exact(3) {
            let mut corners = [[0.0f32; 3]; 3];
            let mut ok = true;
            for (slot, &i) in tri.iter().enumerate() {
                let Ok(i) = usize::try_from(i) else {
                    ok = false;
                    break;
                };
                let Some(vert) = verts.get(first_vert + i) else {
                    ok = false;
                    break;
                };
                corners[slot] = [
                    vert.position[0] * UNIT,
                    vert.position[1] * UNIT,
                    vert.position[2] * UNIT,
                ];
            }
            if ok {
                builder.triangle(corners, Tag::level(Contents::SOLID, 0));
            }
        }
    }

    Ok(builder.build())
}
#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn prints_triangle_count() {
        let path = std::env::var("JA_BSP").expect("set JA_BSP to a .bsp path");
        let bytes = std::fs::read(&path).expect("reading .bsp");
        let level = super::build_bsp(&bytes).expect("building level");
        eprintln!("{} brushes", level.brush_count());
    }
}

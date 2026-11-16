//! Loads a Jedi Academy Ghoul2 model for the viewer (`kjk view ja <path>`):
//! the GLM, the GLA its header names, the `model_default.skin` beside it,
//! each surface's shader resolved to its first `map` stage, and the
//! textures decoded. Optionally `animation.cfg` sequences to loop.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use asset_odyssey::anim::AnimationIr;
use asset_odyssey::model::ModelIr;
use asset_odyssey::texture::{Encoding, FrameStrip, TextureFacts, TextureIr};
use fmt_ja::animation_cfg::AnimationConfig;
use fmt_ja::ghoul2::{Gla, Glm};
use fmt_ja::pk3::Pk3Entry;
use store::{Key, Store};
use vfs::ja::{JaResolved, JaSource, JaStack, Pk3File};
use vfs::{DirIndex, Discovery, Game};

use crate::viewer::{BladeSpec, EffectBlend, LoadedScene, Placed, SaberColor};

/// JA's files after pk3 precedence.
pub fn stack(discovery: &Discovery) -> Result<JaStack, String> {
    let install = discovery.get(Game::JediAcademy).ok_or(format!(
        "ja: not found (set {})",
        Game::JediAcademy.env_var()
    ))?;
    let index = DirIndex::of_install(install).map_err(|error| error.to_string())?;
    Ok(JaStack::open(&index))
}

pub struct Files<'a> {
    stack: &'a JaStack,
    all: BTreeMap<&'a str, JaResolved>,
    /// Where large archive entries are kept unpacked, when it opens.
    store: Option<Store>,
    shaders: OnceLock<BTreeMap<String, ShaderLook>>,
    definitions: OnceLock<BTreeMap<String, fmt_ja::shader::Shader>>,
}

/// What a model surface's shader gives its look: the texture of its first
/// `map`/`clampmap` stage that names a file (an animated map's first
/// frame), whether it is drawn from both sides (`cull none`, `disable`
/// or `twosided`; `cull back`, which draws only the back, is drawn from
/// both sides too), and whether that stage clamps its texture
/// (`clampmap`, `clampAnimMap`).
#[derive(Clone, Debug, PartialEq)]
struct ShaderLook {
    texture: String,
    two_sided: bool,
    clamp: bool,
    anim: Option<AnimMap>,
}

/// A stage's animated map (`animMap`, `clampAnimMap`, `oneShotAnimMap`):
/// its frames a second, whether it plays once, and its frames' files (at
/// most eight, as JA's shader parser keeps).
#[derive(Clone, Debug, PartialEq)]
struct AnimMap {
    fps: f32,
    once: bool,
    frames: Vec<String>,
}

/// The most frames JA's shaders keep for an animated map.
const MAX_ANIM_FRAMES: usize = 8;

/// Compressed entries this large and larger are read from the store
/// unpacked (`docs/IMPORT.md`, the JA pack): the skeleton's animations
/// (`_humanoid.gla`, 11.8 MB) took tens of milliseconds to inflate, and
/// hundreds on a busy machine, each time a run read them.
const UNPACKED_FROM: u32 = 1 << 20;

const UNPACKED: &str = "import/ja/unpacked";

impl<'a> Files<'a> {
    /// Every folder's files: `base/`'s, then a mod folder's over them.
    pub fn new(stack: &'a JaStack) -> Files<'a> {
        Files::of(stack, stack.all())
    }

    /// `base/`'s files alone, as JA itself reads them: everything the sim
    /// is built from (the body, its skeleton and `animation.cfg`, the
    /// sabers' blades) and all the view draws but a mod's hilts and
    /// stances, so a mod folder (Movie Duels) changes nothing else.
    pub fn base(stack: &'a JaStack) -> Files<'a> {
        Files::of(stack, stack.base())
    }

    fn of(stack: &'a JaStack, all: BTreeMap<&'a str, JaResolved>) -> Files<'a> {
        Files {
            stack,
            all,
            store: Store::open(),
            shaders: OnceLock::new(),
            definitions: OnceLock::new(),
        }
    }

    /// Whether `name` (a `.sab` block's) is among these files' sabers.
    pub fn has_saber(&self, name: &str) -> bool {
        self.all
            .keys()
            .filter(|path| path.starts_with("ext_data/sabers/") && path.ends_with(".sab"))
            .any(|path| {
                self.read(path)
                    .is_some_and(|bytes| asset_ja::saber(&bytes, name).is_some())
            })
    }

    /// Every file's name, as kept (lower case).
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.all.keys().copied()
    }

    pub(crate) fn read(&self, name: &str) -> Option<Vec<u8>> {
        let name = fmt_ja::game_path(name);
        let found = self.all.get(name.as_str())?;
        if let (Some(store), JaSource::Pk3 { pk3, entry }) = (&self.store, found.source) {
            let pak = &self.stack.pk3s[pk3];
            let entry = &pak.entries[entry];
            if entry.method != 0 && entry.size >= UNPACKED_FROM {
                return unpacked(store, pak, entry);
            }
        }
        self.stack.read(&name, found.source).ok()
    }
}

impl Files<'_> {
    /// What a file holds, cheaply, for keys: an archive entry's CRC and
    /// sizes, a loose file's size and time.
    fn identity(&self, name: &str) -> Vec<u8> {
        let Some(found) = self.all.get(name) else {
            return Vec::new();
        };
        match found.source {
            JaSource::Pk3 { pk3, entry } => {
                let entry = &self.stack.pk3s[pk3].entries[entry];
                [entry.crc, entry.size, entry.compressed]
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect()
            }
            JaSource::Loose { folder } => {
                let meta = self
                    .stack
                    .loose_path(name, folder)
                    .and_then(|path| std::fs::metadata(path).ok());
                let modified = meta
                    .as_ref()
                    .and_then(|meta| meta.modified().ok())
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |since| since.as_nanos());
                let mut out = meta.map_or(0, |meta| meta.len()).to_le_bytes().to_vec();
                out.extend(modified.to_le_bytes());
                out
            }
        }
    }
}

/// `entry` of `pak` from the store, else inflated (and kept). Keyed by the
/// archive's name, length and time and the entry's name, sizes and CRC:
/// an archive that changes reads its entries again.
fn unpacked(store: &Store, pak: &Pk3File, entry: &Pk3Entry) -> Option<Vec<u8>> {
    let modified = std::fs::metadata(&pak.path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    let key = Key::of(
        "ja-unpacked-v1",
        &[
            pak.name.to_ascii_lowercase().as_bytes(),
            &pak.len.to_le_bytes(),
            &modified.to_le_bytes(),
            entry.name.as_bytes(),
            &entry.crc.to_le_bytes(),
            &entry.size.to_le_bytes(),
            &entry.compressed.to_le_bytes(),
        ],
    );
    if let Some(bytes) = store
        .get(UNPACKED, &key)
        .filter(|bytes| bytes.len() == entry.size as usize)
    {
        return Some(bytes);
    }
    let bytes = pak.read(entry).ok()?;
    let _ = store.put(UNPACKED, &key, &bytes);
    Some(bytes)
}

/// Shader name → its look (`ShaderLook`; a texture is a stage's file, not
/// `$lightmap` or `$whiteimage`): read once a run, and kept in the
/// store, keyed by every shader file's name and contents' CRC and size and
/// this code (`KJK_JA_SHADERS`), as reading them all took tens of
/// milliseconds.
fn shader_textures<'f>(files: &'f Files) -> &'f BTreeMap<String, ShaderLook> {
    files.shaders.get_or_init(|| {
        let names: Vec<&str> = files
            .all
            .keys()
            .copied()
            .filter(|name| {
                (name.starts_with("shaders/") || name.starts_with("scripts/"))
                    && name.ends_with(".shader")
            })
            .collect();
        let Some(store) = &files.store else {
            return read_shader_textures(files, &names);
        };
        let mut identity = Vec::new();
        for name in &names {
            identity.extend(name.as_bytes());
            identity.push(0);
            identity.extend(files.identity(name));
        }
        let key = Key::of(
            &format!("ja-shader-textures/{}", env!("KJK_JA_SHADERS")),
            &[&identity],
        );
        let kind = "import/ja/shader-textures";
        if let Some(map) = store.get(kind, &key).and_then(|bytes| {
            let text = String::from_utf8(bytes).ok()?;
            text.lines()
                .map(|line| {
                    let (shader, texture, sides, clamp, anim) = match line
                        .split('\t')
                        .collect::<Vec<_>>()[..]
                    {
                        [shader, texture, sides, clamp] => (shader, texture, sides, clamp, None),
                        [shader, texture, sides, clamp, fps, once, frames] => (
                            shader,
                            texture,
                            sides,
                            clamp,
                            Some(AnimMap {
                                fps: fps.parse().ok()?,
                                once: once == "1",
                                frames: frames.split('|').map(str::to_string).collect(),
                            }),
                        ),
                        _ => return None,
                    };
                    Some((
                        shader.to_string(),
                        ShaderLook {
                            texture: texture.to_string(),
                            two_sided: sides == "2",
                            clamp: clamp == "1",
                            anim,
                        },
                    ))
                })
                .collect::<Option<BTreeMap<_, _>>>()
        }) {
            return map;
        }
        let map = read_shader_textures(files, &names);
        let text: String = map
            .iter()
            .map(|(shader, look)| {
                let sides = if look.two_sided { 2 } else { 1 };
                let anim = look.anim.as_ref().map_or(String::new(), |anim| {
                    format!(
                        "\t{}\t{}\t{}",
                        anim.fps,
                        u8::from(anim.once),
                        anim.frames.join("|")
                    )
                });
                let clamp = u8::from(look.clamp);
                format!("{shader}\t{}\t{sides}\t{clamp}{anim}\n", look.texture)
            })
            .collect();
        let _ = store.put(kind, &key, text.as_bytes());
        map
    })
}

fn read_shader_textures(files: &Files, names: &[&str]) -> BTreeMap<String, ShaderLook> {
    let mut out = BTreeMap::new();
    for name in names {
        let Some(bytes) = files.read(name) else {
            continue;
        };
        let Ok(parsed) = fmt_ja::shader::parse(&bytes) else {
            continue;
        };
        for shader in parsed.shaders {
            let image = shader.stages.iter().flatten().find_map(|line| {
                let keyword = line.first()?.to_ascii_lowercase();
                match keyword.as_str() {
                    "map" | "clampmap" => {
                        let path = line.get(1).filter(|path| !path.starts_with('$'))?;
                        Some((fmt_ja::game_path(path), keyword == "clampmap", None))
                    }
                    "animmap" | "clampanimmap" | "oneshotanimmap" => {
                        let fps = line.get(1)?.parse::<f32>().ok()?;
                        let frames: Vec<String> = line
                            .get(2..)?
                            .iter()
                            .take(MAX_ANIM_FRAMES)
                            .map(|path| fmt_ja::game_path(path))
                            .collect();
                        let anim = AnimMap {
                            fps,
                            once: keyword == "oneshotanimmap",
                            frames,
                        };
                        let clamp = keyword == "clampanimmap";
                        Some((anim.frames.first()?.clone(), clamp, Some(anim)))
                    }
                    _ => None,
                }
            });
            let two_sided = shader.directives.iter().any(|line| {
                line.first()
                    .is_some_and(|word| word.eq_ignore_ascii_case("cull"))
                    && line.get(1).is_some_and(|how| {
                        [
                            "none",
                            "disable",
                            "twosided",
                            "back",
                            "backside",
                            "backsided",
                        ]
                        .iter()
                        .any(|word| how.eq_ignore_ascii_case(word))
                    })
            });
            if let Some((texture, clamp, anim)) = image {
                out.entry(shader.name).or_insert(ShaderLook {
                    texture,
                    two_sided,
                    clamp,
                    anim,
                });
            }
        }
    }
    out
}

/// Complete parsed shader definitions, retaining every ordered stage.
/// Kept separately from ShaderLook so existing texture resolution remains
/// unchanged.
fn shader_definitions<'f>(files: &'f Files) -> &'f BTreeMap<String, fmt_ja::shader::Shader> {
    files.definitions.get_or_init(|| {
        let names: Vec<&str> = files
            .all
            .keys()
            .copied()
            .filter(|name| {
                (name.starts_with("shaders/") || name.starts_with("scripts/"))
                    && name.ends_with(".shader")
            })
            .collect();

        let mut out = BTreeMap::new();

        for name in names {
            let Some(bytes) = files.read(name) else {
                continue;
            };
            let Ok(parsed) = fmt_ja::shader::parse(&bytes) else {
                continue;
            };

            for shader in parsed.shaders {
                out.entry(shader.name.to_ascii_lowercase())
                    .or_insert(shader);
            }
        }

        out
    })
}

/// The `blendFunc` of the first stage that has one, for each named shader
/// (lower-case), as its two factors (lower-case); `add` and `blend` spelt
/// out as the factors they stand for.
pub(crate) fn blend_funcs(files: &Files, names: &[String]) -> BTreeMap<String, (String, String)> {
    let mut out = BTreeMap::new();
    let shader_files: Vec<&str> = files
        .all
        .keys()
        .copied()
        .filter(|name| {
            (name.starts_with("shaders/") || name.starts_with("scripts/"))
                && name.ends_with(".shader")
        })
        .collect();
    for file in shader_files {
        let Some(bytes) = files.read(file) else {
            continue;
        };
        let Ok(parsed) = fmt_ja::shader::parse(&bytes) else {
            continue;
        };
        for shader in parsed.shaders {
            if !names.contains(&shader.name) || out.contains_key(&shader.name) {
                continue;
            }
            let func = shader.stages.iter().flatten().find_map(|line| {
                let keyword = line.first()?.to_ascii_lowercase();
                if keyword != "blendfunc" {
                    return None;
                }
                let words: Vec<String> = line[1..].iter().map(|w| w.to_ascii_lowercase()).collect();
                match words.as_slice() {
                    [one] if one == "add" => Some(("gl_one".into(), "gl_one".into())),
                    [one] if one == "blend" => {
                        Some(("gl_src_alpha".into(), "gl_one_minus_src_alpha".into()))
                    }
                    [one] if one == "filter" => Some(("gl_dst_color".into(), "gl_zero".into())),
                    [src, dst, ..] => Some((src.clone(), dst.clone())),
                    _ => None,
                }
            });
            if let Some(func) = func {
                out.insert(shader.name, func);
            }
        }
    }
    out
}

/// How each effect shader (lower-case) blends: additive or by alpha, from
/// its first stage's `blendFunc`; anything else is drawn additive, with a
/// note.
pub(crate) fn effect_blends(
    files: &Files,
    names: Vec<String>,
    notes: &mut Vec<String>,
) -> BTreeMap<String, EffectBlend> {
    let funcs = blend_funcs(files, &names);
    names
        .into_iter()
        .map(|name| {
            let blend = match funcs.get(&name).map(|(s, d)| (s.as_str(), d.as_str())) {
                Some(("gl_one", "gl_one")) => EffectBlend::Add,
                Some(("gl_src_alpha", "gl_one_minus_src_alpha")) => EffectBlend::Alpha,
                Some(("gl_dst_color", "gl_src_color")) => EffectBlend::Modulate,
                other => {
                    notes.push(format!("{name}: blend {other:?} drawn as GL_ONE GL_ONE"));
                    EffectBlend::Add
                }
            };
            (name, blend)
        })
        .collect()
}

/// An effect file (`effects/….efx`) as templates; what it asks for that
/// is not supported, or its absence, goes to the notes.
pub(crate) fn effect_file(
    files: &Files,
    path: &str,
    notes: &mut Vec<String>,
) -> asset_ja::fx::Effect {
    let parsed = files
        .read(path)
        .ok_or(format!("{path}: not found"))
        .and_then(|bytes| fmt_ja::efx::parse(&bytes).map_err(|e| format!("{path}: {e}")));
    match parsed {
        Ok(group) => {
            let effect = asset_ja::fx::effect(&group);
            if !effect.unsupported.is_empty() {
                notes.push(format!(
                    "{path}: not supported: {}",
                    effect.unsupported.join(", ")
                ));
            }
            effect
        }
        Err(error) => {
            notes.push(error);
            asset_ja::fx::Effect::default()
        }
    }
}

/// A texture by path, trying the name as given and then `.tga`, `.jpg` and
/// `.png` in place of its extension (the engine's image loader looks for
/// the name without extension). TGA has no signature to guess it by, so
/// it is decoded by its extension.
fn texture(files: &Files, path: &str) -> Option<TextureIr> {
    let stem = path.rsplit_once('.').map_or(path, |(stem, _)| stem);
    let (found, bytes) = [path.to_string()]
        .into_iter()
        .chain(["tga", "jpg", "png"].map(|ext| format!("{stem}.{ext}")))
        .find_map(|candidate| files.read(&candidate).map(|bytes| (candidate, bytes)))?;
    let decoded = if found.to_ascii_lowercase().ends_with(".tga") {
        image::load_from_memory_with_format(&bytes, image::ImageFormat::Tga).ok()?
    } else {
        image::load_from_memory(&bytes).ok()?
    };
    let alpha = decoded.color().has_alpha();
    let rgba = decoded.to_rgba8();
    Some(TextureIr {
        width: rgba.width(),
        height: rgba.height(),
        encoding: Encoding::Rgba8,
        mips: vec![rgba.into_raw()],
        cube: false,
        frames: 1,
        facts: TextureFacts {
            alpha,
            ..TextureFacts::default()
        },
        faces: Vec::new(),
        frame_mips: Vec::new(),
    })
}

/// An animated map's frames decoded and stacked top to bottom, each at the
/// first's size (`FrameStrip`); a frame that does not load is left out.
fn frame_strip(files: &Files, anim: &AnimMap) -> Option<TextureIr> {
    let frames: Vec<TextureIr> = anim
        .frames
        .iter()
        .filter_map(|frame| texture(files, frame))
        .collect();
    let first = frames.first()?;
    let (width, height) = (first.width, first.height);
    let mut rgba = Vec::with_capacity(frames.len() * (width * height * 4) as usize);
    for frame in &frames {
        let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.mips[0].clone())?;
        let sized = if (frame.width, frame.height) == (width, height) {
            image
        } else {
            image::imageops::resize(&image, width, height, image::imageops::FilterType::Triangle)
        };
        rgba.extend(sized.into_raw());
    }
    Some(TextureIr {
        height: height * frames.len() as u32,
        mips: vec![rgba],
        facts: TextureFacts {
            alpha: frames.iter().any(|frame| frame.facts.alpha),
            strip: Some(FrameStrip {
                frames: frames.len() as u32,
                fps: anim.fps,
                once: anim.once,
            }),
            ..TextureFacts::default()
        },
        ..first.clone()
    })
}

/// Shader or texture names decoded through the shaders' first stages.
pub(crate) fn textures<'n>(
    files: &Files,
    names: impl IntoIterator<Item = &'n String>,
) -> (BTreeMap<String, TextureIr>, Vec<String>) {
    let shaders = shader_textures(files);
    let mut textures = BTreeMap::new();
    let mut missing = Vec::new();
    for name in names {
        if textures.contains_key(name) || missing.contains(name) {
            continue;
        }
        let look = shaders.get(name);
        let file = look.map_or(name.as_str(), |look| look.texture.as_str());
        let decoded = match look.and_then(|look| look.anim.as_ref()) {
            Some(anim) => frame_strip(files, anim),
            None => texture(files, file),
        };
        match decoded {
            Some(mut decoded) => {
                decoded.facts.two_sided = look.is_some_and(|look| look.two_sided);
                decoded.facts.clamp = look.is_some_and(|look| look.clamp);
                textures.insert(name.clone(), decoded);
            }
            None => missing.push(name.clone()),
        }
    }
    (textures, missing)
}

/// A GLM, the GLA its header names, and the GLA's path.
pub fn glm_and_gla(files: &Files, path: &str) -> Result<(Glm, Gla, String), String> {
    let path = fmt_ja::game_path(path);
    let glm_bytes = files.read(&path).ok_or(format!("{path}: not found"))?;
    let glm = Glm::parse(&glm_bytes).map_err(|error| format!("{path}: {error:?}"))?;
    let gla_path = format!("{}.gla", fmt_ja::game_path(&glm.animation));
    let gla_bytes = files
        .read(&gla_path)
        .ok_or(format!("{gla_path}: not found"))?;
    let gla = Gla::parse(&gla_bytes).map_err(|error| format!("{gla_path}: {error:?}"))?;
    Ok((glm, gla, gla_path))
}

/// The `animation.cfg` beside a GLA.
pub fn animation_cfg(files: &Files, gla_path: &str) -> Result<AnimationConfig, String> {
    let cfg_path = format!(
        "{}/animation.cfg",
        gla_path.rsplit_once('/').map_or("", |(folder, _)| folder)
    );
    files
        .read(&cfg_path)
        .map(|bytes| AnimationConfig::parse(&bytes))
        .ok_or(format!("{cfg_path}: not found"))
}

/// `animation.cfg` sequences beside a GLA, comma-separated, each optionally
/// `NAME:blend_ms`, played one after another (`asset_ja::chain`) and baked
/// every 5 ms through the Ghoul2 player.
pub(crate) fn sequences(
    files: &Files,
    gla: &Gla,
    gla_path: &str,
    wanted: &str,
) -> Result<AnimationIr, String> {
    Ok(sequences_cued(files, gla, gla_path, wanted, false)?.0)
}

/// `sequences`, with when each one starts: (ms, animation name). With
/// `quirks`, the player reproduces its reference quirks
/// (`asset_ja::g2::Player::quirks`) and deaths start unblended
/// (`asset_ja::blend_ms`).
pub(crate) fn sequences_cued(
    files: &Files,
    gla: &Gla,
    gla_path: &str,
    wanted: &str,
    quirks: bool,
) -> Result<(AnimationIr, Vec<(f32, String)>), String> {
    let cfg = animation_cfg(files, gla_path)?;
    let entries: Vec<&str> = wanted.split(',').map(str::trim).collect();
    let moves: Vec<(&fmt_ja::animation_cfg::Sequence, f32)> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let before = i
                .checked_sub(1)
                .map(|j| entries[j].split(':').next().unwrap_or(entries[j]));
            let (name, blend) = match entry.split_once(':') {
                Some((name, blend)) => (
                    name,
                    blend
                        .parse::<f32>()
                        .map_err(|_| format!("{entry}: blend is not a number"))?,
                ),
                None => (*entry, asset_ja::blend_ms(before, entry, quirks)),
            };
            let sequence = cfg
                .sequences
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(name))
                .ok_or(format!("{name}: not in animation.cfg"))?;
            Ok((sequence, blend))
        })
        .collect::<Result<_, String>>()?;
    let (cues, length) =
        asset_ja::chain(gla, &moves).ok_or(format!("{wanted}: frames past the GLA"))?;
    let started = cues
        .iter()
        .map(|cue| (cue.at_ms, cue.sequence.name.clone()))
        .collect();
    let mut player = asset_ja::g2::Player::new(gla);
    player.quirks = quirks;
    Ok((
        asset_ja::g2::bake_with(player, &cues, length, 5.0, wanted),
        started,
    ))
}

pub(crate) fn skin_beside(files: &Files, model: &str) -> BTreeMap<String, String> {
    let folder = model.rsplit_once('/').map_or("", |(folder, _)| folder);
    files
        .read(&format!("{folder}/model_default.skin"))
        .map(|bytes| asset_ja::parse_skin(&bytes))
        .unwrap_or_else(BTreeMap::new)
}

fn bsp_player_entry(entities: &[fmt_ja::bsp::BspEntity]) -> Option<([f32; 3], [f32; 2])> {
    const SPAWN_CLASSES: &[&str] = &[
        "info_player_deathmatch",
        "info_player_start",
        "info_player_coop",
        "info_player_intermission",
        "info_player_duel",
        "info_player_spectator",
        "team_CTF_redplayer",
        "team_CTF_blueplayer",
        "team_redplayer",
        "team_blueplayer",
    ];

    let entity = SPAWN_CLASSES.iter().find_map(|class| {
        entities
            .iter()
            .find(|entity| entity.get("classname") == Some(class))
    })?;

    let origin: [f32; 3] = entity
        .get("origin")?
        .split_whitespace()
        .map(str::parse::<f32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?
        .try_into()
        .ok()?;

    let angle = entity
        .get("angle")
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(0.0)
        .to_radians();

    Some((
        origin.map(|value| value * asset_ja::UNIT),
        [angle.cos(), angle.sin()],
    ))
}

/// Load a Jedi Academy BSP map as a static scene.
fn load_bsp(files: &Files<'_>, path: &str) -> Result<LoadedScene, String> {
    let path = fmt_ja::game_path(path);
    let bytes = files
        .read(&path)
        .ok_or_else(|| format!("{path}: not found"))?;

    let (ir, bsp) =
        asset_ja::bsp::model_full(&bytes, &path).map_err(|error| format!("{path}: {error:?}"))?;

    let collision = collision_odyssey::ja::build_bsp(&bytes).ok().map(Arc::new);
    eprintln!(
        "JA BSP COLLISION: {} brushes",
        collision.as_ref().map_or(0, |l| l.brush_count()),
    );

    let parsed_bsp = fmt_ja::bsp::parse(&bytes).map_err(|error| format!("{path}: {error:?}"))?;
    let bsp_surfaces = fmt_ja::bsp::parse_surfaces(&bytes, &parsed_bsp)
        .map_err(|error| format!("{path}: {error:?}"))?;
    let bsp_shaders = fmt_ja::bsp::parse_shaders(&bytes, &parsed_bsp)
        .map_err(|error| format!("{path}: {error:?}"))?;
    let bsp_fogs = fmt_ja::bsp::parse_fogs(&bytes, &parsed_bsp)
        .map_err(|error| format!("{path}: {error:?}"))?;
    let bsp_brushes = fmt_ja::bsp::parse_brushes(&bytes, &parsed_bsp)
        .map_err(|error| format!("{path}: {error:?}"))?;
    let bsp_brush_sides = fmt_ja::bsp::parse_brush_sides(&bytes, &parsed_bsp)
        .map_err(|error| format!("{path}: {error:?}"))?;
    let bsp_planes = fmt_ja::bsp::parse_planes(&bytes, &parsed_bsp)
        .map_err(|error| format!("{path}: {error:?}"))?;

    eprintln!("\n=== SKY SURFACE TRACE ===");

    let sky_shader_indices: std::collections::BTreeSet<usize> = bsp_shaders
        .iter()
        .enumerate()
        .map(|(index, shader)| {
            eprintln!(
                "BSP SHADER [{index}]: {} surface={:#010x} content={:#010x}",
                shader.name, shader.surface_flags, shader.content_flags,
            );
            index
        })
        .filter(|index| {
            bsp_shaders[*index]
                .name
                .to_ascii_lowercase()
                .contains("skies/")
        })
        .collect();

    for (surface_index, surface) in bsp_surfaces.iter().enumerate() {
        let shader_index = match usize::try_from(surface.shader) {
            Ok(index) if sky_shader_indices.contains(&index) => index,
            _ => continue,
        };

        let mesh_indices: Vec<usize> = bsp
            .mesh_surface
            .iter()
            .enumerate()
            .filter_map(|(mesh_index, &source_surface)| {
                (source_surface == surface_index).then_some(mesh_index)
            })
            .collect();

        eprintln!(
            "SKY SURFACE [{surface_index}]: shader={shader_index} {:?}, type={}, verts={}+{}, indexes={}+{}, meshes={:?}, clusters={:?}",
            bsp_shaders[shader_index].name,
            surface.surface_type,
            surface.first_vert,
            surface.num_verts,
            surface.first_index,
            surface.num_indexes,
            mesh_indices,
            bsp.visibility.surface_clusters(surface_index),
        );
    }

    eprintln!("=== END SKY SURFACE TRACE ===\n");

    let entities = fmt_ja::bsp::parse_entities(&bytes, &parsed_bsp)
        .map_err(|error| format!("{path}: {error:?}"))?;

    let entry = bsp_player_entry(&entities);

    let mut entity_classes = std::collections::BTreeMap::<String, usize>::new();

    for entity in &entities {
        let class = entity.get("classname").unwrap_or("<no classname>");
        *entity_classes.entry(class.to_string()).or_default() += 1;
    }

    eprintln!("BSP ENTITY CLASSES:");
    for (class, count) in &entity_classes {
        eprintln!("  {class}: {count}");
    }

    eprintln!("BSP ENTITY MODEL REFERENCES:");

    for class in [
        "misc_model_static",
        "func_static",
        "func_door",
        "func_breakable",
    ] {
        let mut shown = 0;

        for entity in &entities {
            if entity.get("classname") != Some(class) {
                continue;
            }

            eprintln!("  {class}: {:?}", entity.properties);
            shown += 1;

            if shown >= 3 {
                break;
            }
        }
    }

    let inline_models: Vec<usize> = entities
        .iter()
        .filter_map(|entity| entity.get("model"))
        .filter_map(|model| model.strip_prefix('*'))
        .filter_map(|index| index.parse().ok())
        .collect();

    eprintln!(
        "BSP INLINE MODEL REFERENCES: {} entities, highest index {:?}",
        inline_models.len(),
        inline_models.iter().max()
    );

    eprintln!("BSP player entry: {entry:?}");

    let parsed_bsp = fmt_ja::bsp::parse(&bytes).map_err(|error| format!("{path}: {error:?}"))?;

    let bsp_models = fmt_ja::bsp::parse_models(&bytes, &parsed_bsp)
        .map_err(|error| format!("{path}: {error:?}"))?;

    for (index, model) in bsp_models.iter().enumerate().skip(1) {
        if model.num_surfaces > 0 {
            eprintln!(
                "BSP INLINE {index}: mins={:?}, maxs={:?}, surfaces={}..{}",
                model.mins,
                model.maxs,
                model.first_surface,
                model.first_surface + model.num_surfaces,
            );
        }
    }

    let entities = fmt_ja::bsp::parse_entities(&bytes, &parsed_bsp)
        .map_err(|error| format!("{path}: {error:?}"))?;
    eprintln!("BSP INLINE ENTITY TRANSFORMS:");

    for entity in &entities {
        let Some(model_ref) = entity.get("model") else {
            continue;
        };

        let Some(index) = model_ref
            .strip_prefix('*')
            .and_then(|value| value.parse::<usize>().ok())
        else {
            continue;
        };

        let Some(model) = bsp_models.get(index) else {
            continue;
        };

        if model.num_surfaces == 0 {
            continue;
        }

        eprintln!(
            "  model={index} class={:?} origin={:?} angle={:?} angles={:?}",
            entity.get("classname"),
            entity.get("origin"),
            entity.get("angle"),
            entity.get("angles"),
        );
    }

    for entity in &entities {
        let classname = entity.get("classname").unwrap_or("");

        if classname.starts_with("info_player_") {
            eprintln!(
                "BSP spawn: class={classname}, origin={:?}, angle={:?}, angles={:?}",
                entity.get("origin"),
                entity.get("angle"),
                entity.get("angles"),
            );
        }
    }

    let texture_names: Vec<String> = ir
        .meshes
        .iter()
        .filter_map(|mesh| mesh.texture.clone())
        .collect();
    let distinct: std::collections::BTreeSet<&String> = texture_names.iter().collect();

    eprintln!(
        "BSP: {} meshes, {} with textures, {} distinct shader names",
        ir.meshes.len(),
        texture_names.len(),
        distinct.len()
    );

    let models = vec![Placed {
        ir,
        offset: [0.0; 3],
        facing: 0.0,
        is_static: true,
        attach: None,
        hook: None,
        hook_local: None,
        clip: None,
        envmap: None,
        bsp: Some(Arc::new(bsp.clone())),
    }];

    let mut names = scene_textures(&models, &[]);
    names.extend(bsp_fogs.iter().map(|fog| fog.shader.clone()));

    let definitions = shader_definitions(files);
    let ja_shaders: BTreeMap<String, fmt_ja::shader::Shader> = names
        .iter()
        .filter_map(|name| {
            definitions
                .get(&name.to_ascii_lowercase())
                .map(|shader| (name.clone(), shader.clone()))
        })
        .collect();
    let ja_shader_ir: BTreeMap<String, crate::ja_shader::JaShader> = ja_shaders
        .iter()
        .map(|(name, shader)| (name.clone(), crate::ja_shader::interpret(shader.clone())))
        .collect();

    let ja_fogs: Vec<crate::viewer::JaFog> = bsp_fogs
        .iter()
        .map(|fog| {
            let fog_params = ja_shader_ir.get(&fog.shader).and_then(|shader| {
                shader
                    .directives
                    .iter()
                    .find_map(|directive| match &directive.kind {
                        crate::ja_shader::ShaderDirectiveKind::FogParms(args) => {
                            let values: Vec<f32> = args
                                .iter()
                                .filter_map(|arg| arg.parse::<f32>().ok())
                                .collect();

                            if values.len() >= 4 {
                                Some(([values[0], values[1], values[2]], values[3]))
                            } else {
                                None
                            }
                        }
                        _ => None,
                    })
            });

            let planes = usize::try_from(fog.brush)
                .ok()
                .and_then(|brush_index| bsp_brushes.get(brush_index))
                .and_then(|brush| {
                    let start = usize::try_from(brush.first_side).ok()?;
                    let count = usize::try_from(brush.num_sides).ok()?;
                    let end = start.checked_add(count)?;
                    bsp_brush_sides.get(start..end)
                })
                .map(|sides| {
                    sides
                        .iter()
                        .filter_map(|side| {
                            let plane_index = usize::try_from(side.plane).ok()?;
                            let plane = bsp_planes.get(plane_index)?;
                            Some((plane.normal, plane.dist))
                        })
                        .collect()
                })
                .unwrap_or_default();

            crate::viewer::JaFog {
                shader: fog.shader.clone(),
                brush: fog.brush,
                visible_side: fog.visible_side,
                color: fog_params.map(|params| params.0),
                distance: fog_params.map(|params| params.1),
                planes,
            }
        })
        .collect();

    for (index, fog) in ja_fogs.iter().enumerate() {
        eprintln!(
            "JA BSP FOG [{index}]: shader={}, brush={}, visible_side={}, \
             color={:?}, distance={:?}, planes={}",
            fog.shader,
            fog.brush,
            fog.visible_side,
            fog.color,
            fog.distance,
            fog.planes.len(),
        );
    }
    let mut surface_fog_counts = BTreeMap::<i32, usize>::new();
    for surface in &bsp_surfaces {
        *surface_fog_counts.entry(surface.fog).or_default() += 1;
    }

    eprintln!(
        "JA BSP SURFACE FOG INDICES: surfaces={}, records={}, raw={:?}",
        bsp_surfaces.len(),
        ja_fogs.len(),
        surface_fog_counts,
    );
    // TEMPORARY: inspect sky-related assets and original shader definitions.
    eprintln!("\n=== SKY-RELATED ARCHIVE ENTRIES ===");
    for name in files.all.keys() {
        if name.contains("sky") || name.contains("hoth") || name.contains("taspir") {
            eprintln!("  {name}");
        }
    }

    eprintln!("\n=== ORIGINAL HOTH AND VJUN SKY DEFINITIONS ===");
    for shader_name in ["textures/skies/hoth", "textures/skies/taspir1"] {
        match definitions.get(&shader_name.to_ascii_lowercase()) {
            Some(shader) => eprintln!("{shader_name}:\n{shader:#?}"),
            None => eprintln!("{shader_name}: NOT FOUND IN ARCHIVES"),
        }
    }
    // Load every ordinary image referenced by interpreted JA shader stages.
    for shader in ja_shader_ir.values() {
        for stage in &shader.stages {
            for directive in &stage.directives {
                if let crate::ja_shader::StageDirectiveKind::Map(map) = &directive.kind {
                    let names_to_load: Vec<&str> = match map {
                        crate::ja_shader::TextureMap::Map(name)
                        | crate::ja_shader::TextureMap::ClampMap(name) => vec![name.as_str()],
                        crate::ja_shader::TextureMap::AnimMap { frames, .. }
                        | crate::ja_shader::TextureMap::ClampAnimMap { frames, .. }
                        | crate::ja_shader::TextureMap::OneShotAnimMap { frames, .. } => {
                            frames.iter().map(String::as_str).collect()
                        }
                        crate::ja_shader::TextureMap::VideoMap(_) => Vec::new(),
                    };

                    for name in names_to_load {
                        if !name.starts_with('$') && !names.iter().any(|existing| existing == name)
                        {
                            names.push(name.to_owned());
                        }
                    }
                }
            }
        }
    }

    let (mut textures, missing) = textures(files, &names);

    // Resolve the cubemap named by a BSP shader marked surfaceparm sky.
    // Quake 3 face suffixes are uploaded in wgpu cube-layer order:
    // +X, -X, +Y, -Y, +Z, -Z.
    let skybox = ja_shader_ir.iter().find_map(|(shader_name, shader)| {
        let is_sky = shader.directives.iter().any(|directive| {
            matches!(
                &directive.kind,
                crate::ja_shader::ShaderDirectiveKind::SurfaceParm(value)
                    if value.eq_ignore_ascii_case("sky")
            )
        });

        if !is_sky {
            return None;
        }

        let prefix = shader
            .directives
            .iter()
            .find_map(|directive| match &directive.kind {
                crate::ja_shader::ShaderDirectiveKind::SkyParms(args) => {
                    args.first().map(String::as_str)
                }
                _ => None,
            })?;

        eprintln!(
            "BSP SKY CANDIDATE: {shader_name}, prefix={prefix}, stages={}",
            shader.stages.len()
        );

        if prefix == "-" {
            return None;
        }

        // Bevy/wgpu cube-layer order: +X, -X, +Y, -Y, +Z, -Z.
        let suffixes = ["rt", "lf", "up", "dn", "ft", "bk"];

        // Load each face independently so missing faces do not invalidate the cube.
        let loaded: Vec<Option<TextureIr>> = suffixes
            .iter()
            .map(|suffix| texture(files, &format!("{prefix}_{suffix}")))
            .collect();

        eprintln!(
            "BSP SKY FACES: {shader_name}: {}",
            suffixes
                .iter()
                .zip(loaded.iter())
                .map(|(suffix, face)| format!("{suffix}={}", face.is_some()))
                .collect::<Vec<_>>()
                .join(", ")
        );

        let (width, height) = loaded
            .iter()
            .flatten()
            .next()
            .map(|face| (face.width, face.height))
            .unwrap_or((1, 1));

        if width != height {
            eprintln!("BSP skybox {prefix}: faces must be square");
            return None;
        }

        if loaded
            .iter()
            .flatten()
            .any(|face| face.width != width || face.height != height)
        {
            eprintln!("BSP skybox {prefix}: faces have inconsistent dimensions");
            return None;
        }

        let missing_faces: Vec<&str> = loaded
            .iter()
            .zip(suffixes.iter())
            .filter_map(|(face, suffix)| face.is_none().then_some(*suffix))
            .collect();

        // Neutral white fallback for any missing face.
        let fallback = vec![255u8; width as usize * height as usize * 4];

        let faces: Vec<Vec<Vec<u8>>> = loaded
            .into_iter()
            .map(|face| match face {
                Some(face) => face.mips,
                None => vec![fallback.clone()],
            })
            .collect();

        if !missing_faces.is_empty() {
            eprintln!(
                "BSP skybox {prefix}: using fallback for missing faces: {}",
                missing_faces.join(", ")
            );
        }

        Some(TextureIr {
            width,
            height,
            encoding: Encoding::Rgba8,
            mips: faces[0].clone(),
            cube: true,
            frames: 1,
            facts: TextureFacts::default(),
            faces,
            frame_mips: Vec::new(),
        })
    });

    if skybox.is_some() {
        eprintln!("BSP skybox: loaded six cubemap faces");
    } else {
        eprintln!("BSP skybox: no usable skyParms cubemap");
    }

    eprintln!(
        "BSP: resolved {} textures, {} missing",
        textures.len(),
        missing.len()
    );

    if !missing.is_empty() {
        eprintln!(
            "BSP missing (first {}): {:#?}",
            missing.len().min(10),
            &missing[..missing.len().min(10)]
        );
    }

    eprintln!("JA BSP SHADERS: {}", ja_shaders.len());

    for (name, shader) in &ja_shaders {
        eprintln!("\nJA BSP SHADER: {name}");

        for directive in &shader.directives {
            eprintln!("  SHADER: {}", directive.join(" "));
        }

        for (index, stage) in shader.stages.iter().enumerate() {
            eprintln!("  STAGE {index}:");

            for directive in stage {
                eprintln!("    {}", directive.join(" "));
            }
        }
    }

    // BSP lightmaps are embedded in BspVis and already decoded.
    textures.extend(bsp.lightmaps.clone());

    Ok(LoadedScene {
        name: path,
        models,
        textures,
        missing,
        ja_shaders,
        ja_shader_ir,
        skybox,
        ja_fogs,
        is_ja: true,
        ambient: [1.0; 3],
        lit: true,
        entry,
        collision,
        ..LoadedScene::default()
    })
}

pub fn load(
    discovery: &Discovery,
    path: &str,
    sequence: Option<&str>,
    saber: Option<&str>,
) -> Result<LoadedScene, String> {
    if path.to_ascii_lowercase().ends_with(".bsp") {
        if sequence.is_some() || saber.is_some() {
            return Err("BSP maps do not support model animations or sabers".to_string());
        }

        let stack = stack(discovery)?;
        let files = Files::new(&stack);
        return load_bsp(&files, path);
    }

    let stack = stack(discovery)?;
    let files = Files::new(&stack);
    let (glm, gla, gla_path) = glm_and_gla(&files, path)?;
    let path = fmt_ja::game_path(path);
    let mut ir = asset_ja::model(&glm, &gla, &skin_beside(&files, &path));
    let clip = match sequence {
        None => None,
        Some(wanted) => Some(Arc::new(sequences(&files, &gla, &gla_path, wanted)?)),
    };
    if let Some(clip) = &clip {
        ir.animations.push((**clip).clone());
    }

    let floor = if clip.is_some() {
        asset_ja::ORIGIN_HEIGHT
    } else {
        0.0
    };

    let mut models = vec![Placed {
        ir,
        offset: [0.0, 0.0, floor],
        facing: 0.0,
        is_static: false,
        attach: None,
        hook: None,
        hook_local: None,
        clip,
        envmap: None,
        bsp: None,
    }];

    let mut blades = Vec::new();
    let mut notes = Vec::new();

    if let Some(name) = saber {
        let (hilt, hilt_blades, note) = hilt(&files, name)?;
        let hand = asset_ja::bolt(&glm, "*r_hand").ok_or("*r_hand: no such tag")?;

        notes.push(format!(
            "{note}; *r_hand on bone {} (rigid {})",
            hand.bone, hand.rigid
        ));

        models.push(Placed {
            ir: hilt,
            offset: [0.0; 3],
            facing: 0.0,
            is_static: false,
            attach: None,
            hook: Some((0, hand.bone + 1)),
            hook_local: Some(hand.matrix),
            clip: None,
            envmap: None,
            bsp: None,
        });

        let model = models.len() - 1;
        blades.extend(hilt_blades.into_iter().map(|b| BladeSpec { model, ..b }));
    }

    let (textures, missing) = textures(&files, &scene_textures(&models, &blades));

    Ok(LoadedScene {
        name: path,
        models,
        textures,
        ja_shader_ir: BTreeMap::new(),
        missing,
        missing_body_textures: Vec::new(),
        default_envmap: None,
        probes: Default::default(),
        entry: None,
        missing_models: Vec::new(),
        rooms: Vec::new(),
        own_fx: Vec::new(),
        instances: 0,
        creatures: 0,
        creature_refusals: Vec::new(),
        missing_clips: Vec::new(),
        creature_notes: notes,
        weapons: 0,
        ambient: [1.0; 3],
        fog: None,
        cameras: Vec::new(),
        map: None,
        cutscene_models: BTreeMap::new(),
        lit: false,
        blades,
        shown: Vec::new(),
        effects: crate::viewer::Effects::default(),
        doors: Vec::new(),
        instance_models: BTreeMap::new(),
        creature_views: Vec::new(),
        arms: BTreeMap::new(),
        ..LoadedScene::default()
    })
}

/// Every texture the models' meshes and the blades name.
pub(crate) fn scene_textures<'m>(
    models: impl IntoIterator<Item = &'m Placed>,
    blades: &[BladeSpec],
) -> Vec<String> {
    models
        .into_iter()
        .flat_map(|placed| {
            placed
                .ir
                .meshes
                .iter()
                .filter_map(|mesh| mesh.texture.clone())
        })
        .chain(
            blades
                .iter()
                .flat_map(|b| [b.glow.clone(), b.core.clone(), b.trail_texture.clone()]),
        )
        .collect()
}

/// A saber's `.sab` block (searched in every `ext_data/sabers/*.sab`) and
/// its hilt's GLM.
pub fn saber_and_hilt(files: &Files, name: &str) -> Result<(asset_ja::Saber, Glm), String> {
    let saber = files
        .all
        .keys()
        .filter(|path| path.starts_with("ext_data/sabers/") && path.ends_with(".sab"))
        .find_map(|path| asset_ja::saber(&files.read(path)?, name))
        .ok_or(format!("saber {name}: no .sab block"))?;
    let hilt_bytes = files
        .read(&saber.model)
        .ok_or(format!("{}: not found", saber.model))?;
    let hilt = Glm::parse(&hilt_bytes).map_err(|error| format!("{}: {error:?}", saber.model))?;
    Ok((saber, hilt))
}

/// Where a blade leaves its hilt and which way it runs, from its
/// `*bladeN` bolt: along the bolt's negative X. The axis is single-player's
/// (negative X, noting the GLM exporter turned the tag); multiplayer reads
/// negative Y of a bolt its query has turned a quarter, the same axis (QUIRKS.md Q-0002).
pub(crate) fn blade_axis(bolt: &asset_ja::Bolt) -> ([f32; 3], [f32; 3]) {
    let m = bolt.matrix;
    ([m[0][3], m[1][3], m[2][3]], [-m[0][0], -m[1][0], -m[2][0]])
}

/// A saber from its `.sab` block: the hilt model and its blade from the
/// hilt's `*blade1` bolt (`blade_axis`), blue (the engine's default
/// colour). A saber with more blades has a `*bladeN` bolt for each; those
/// from the first missing tag on are left out, as JA leaves them
/// (`asset_ja::blade_tags`, said so in the note). The blades' `model` is
/// left 0 for the caller to set.
pub(crate) fn hilt(files: &Files, name: &str) -> Result<(ModelIr, Vec<BladeSpec>, String), String> {
    let (saber, hilt) = saber_and_hilt(files, name)?;
    let hilt_gla = if hilt.animation.eq_ignore_ascii_case("*default") {
        asset_ja::default_gla(&hilt.name)
    } else {
        let path = format!("{}.gla", hilt.animation.to_ascii_lowercase());
        Gla::parse(&files.read(&path).ok_or(format!("{path}: not found"))?)
            .map_err(|error| format!("{path}: {error:?}"))?
    };
    let tags = asset_ja::blade_tags(&hilt, saber.lengths.len());
    let blades: Vec<BladeSpec> = tags
        .iter()
        .zip(&saber.lengths)
        .filter_map(|(tag, &length)| {
            let (origin, dir) = blade_axis(&asset_ja::bolt(&hilt, tag)?);
            Some(BladeSpec::new(
                origin,
                dir,
                length,
                saber.radius,
                SaberColor::Blue,
            ))
        })
        .collect();
    // No tag, no blade: the hilt is still held.
    let untagged: Vec<String> = (tags.len() + 1..=saber.lengths.len())
        .map(|n| format!("*blade{n}"))
        .collect();
    let mut note = format!(
        "saber {}: hilt {}, blades {:?}, radius {}, blade axes {:?}",
        saber.name,
        saber.model,
        saber.lengths,
        saber.radius,
        blades.iter().map(|b| b.dir).collect::<Vec<_>>()
    );
    if !untagged.is_empty() {
        note.push_str(&format!(
            ", no blade for the missing {}",
            untagged.join(", ")
        ));
    }
    Ok((
        asset_ja::model(&hilt, &hilt_gla, &skin_beside(files, &saber.model)),
        blades,
        note,
    ))
}

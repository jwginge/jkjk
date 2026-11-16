use fmt_ja::shader::Shader as ParsedShader;

/// A complete interpreted Jedi Academy shader.
///
/// `raw` retains the original parsed definition. The typed fields provide
/// convenient access to directives used by the renderer.
#[derive(Debug, Clone, PartialEq)]
pub struct JaShader {
    pub name: String,
    pub directives: Vec<ShaderDirective>,
    pub stages: Vec<JaStage>,
    pub raw: ParsedShader,
}

/// A shader-level directive.
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderDirective {
    pub keyword: String,
    pub arguments: Vec<String>,
    pub kind: ShaderDirectiveKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShaderDirectiveKind {
    SurfaceParm(String),
    Cull(CullMode),
    SkyParms(Vec<String>),
    DeformVertexes(Vec<String>),
    Sort(Vec<String>),
    FogParms(Vec<String>),
    PolygonOffset,
    NoMipMaps,
    NoPicMip,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CullMode {
    Front,
    Back,
    None,
    Other(String),
}
/// One ordered rendering stage.
#[derive(Debug, Clone, PartialEq)]
pub struct JaStage {
    pub directives: Vec<StageDirective>,
    pub raw: Vec<Vec<String>>,
}

/// A recognised or unrecognised stage directive.
#[derive(Debug, Clone, PartialEq)]
pub struct StageDirective {
    pub keyword: String,
    pub arguments: Vec<String>,
    pub kind: StageDirectiveKind,
}

/// Typed interpretations of common JA stage directives.
///
/// `Other` deliberately preserves directives we have not implemented yet.
#[derive(Debug, Clone, PartialEq)]
pub enum StageDirectiveKind {
    Map(TextureMap),
    BlendFunc(BlendFunction),
    RgbGen(ColourGeneration),
    AlphaGen(AlphaGeneration),
    TcGen(TextureCoordinateGeneration),
    TcMod(TextureCoordinateModifier),
    DepthWrite,
    Detail,
    AlphaFunc(Vec<String>),
    Other,
}

/// Texture sources used by a stage.
#[derive(Debug, Clone, PartialEq)]
pub enum TextureMap {
    Map(String),
    ClampMap(String),
    AnimMap {
        frequency: Option<f32>,
        frames: Vec<String>,
    },
    ClampAnimMap {
        frequency: Option<f32>,
        frames: Vec<String>,
    },
    OneShotAnimMap {
        frequency: Option<f32>,
        frames: Vec<String>,
    },
    VideoMap(String),
}

/// Blend operation, retaining the original arguments.
#[derive(Debug, Clone, PartialEq)]
pub enum BlendFunction {
    Add,
    Filter,
    Blend,
    Explicit { source: String, destination: String },
    Invalid(Vec<String>),
}

/// RGB generation modes.
#[derive(Debug, Clone, PartialEq)]
pub enum ColourGeneration {
    Identity,
    IdentityLighting,
    Vertex,
    ExactVertex,
    Entity,
    OneMinusEntity,
    Wave(Vec<String>),
    Constant(Vec<String>),
    Other(Vec<String>),
}

/// Alpha generation modes.
#[derive(Debug, Clone, PartialEq)]
pub enum AlphaGeneration {
    Identity,
    Skip,
    Vertex,
    Entity,
    OneMinusEntity,
    Wave(Vec<String>),
    Constant(Vec<String>),
    Other(Vec<String>),
}

/// Texture coordinate generation.
#[derive(Debug, Clone, PartialEq)]
pub enum TextureCoordinateGeneration {
    Base,
    Lightmap,
    Environment,
    Vector(Vec<String>),
    Other(Vec<String>),
}

/// Texture coordinate transformations.
#[derive(Debug, Clone, PartialEq)]
pub enum TextureCoordinateModifier {
    Rotate(Vec<String>),
    Scale(Vec<String>),
    Scroll(Vec<String>),
    Stretch(Vec<String>),
    Transform(Vec<String>),
    Turb(Vec<String>),
    EntityTranslate,
    Other(Vec<String>),
}

/// Convert the existing parser output into the typed representation.
///
/// Keywords are normalised for interpretation, but original argument
/// spelling and the complete parsed shader are retained.
pub fn interpret(shader: ParsedShader) -> JaShader {
    let directives = shader
        .directives
        .iter()
        .map(|tokens| {
            let key = keyword(tokens);
            let args = arguments(tokens);
            let kind = interpret_shader_directive(&key, &args);

            ShaderDirective {
                keyword: key,
                arguments: args,
                kind,
            }
        })
        .collect();

    let stages = shader
        .stages
        .iter()
        .map(|raw_stage| {
            let directives = raw_stage
                .iter()
                .map(|tokens| {
                    let key = keyword(tokens);
                    let args = arguments(tokens);
                    let kind = interpret_stage_directive(&key, &args);

                    StageDirective {
                        keyword: key,
                        arguments: args,
                        kind,
                    }
                })
                .collect();

            JaStage {
                directives,
                raw: raw_stage.clone(),
            }
        })
        .collect();

    JaShader {
        name: shader.name.to_ascii_lowercase(),
        directives,
        stages,
        raw: shader,
    }
}

fn keyword(tokens: &[String]) -> String {
    tokens
        .first()
        .map(|word| word.to_ascii_lowercase())
        .unwrap_or_default()
}

fn arguments(tokens: &[String]) -> Vec<String> {
    tokens.iter().skip(1).cloned().collect()
}

fn interpret_shader_directive(keyword: &str, args: &[String]) -> ShaderDirectiveKind {
    match keyword {
        "surfaceparm" => args
            .first()
            .map(|value| ShaderDirectiveKind::SurfaceParm(value.clone()))
            .unwrap_or(ShaderDirectiveKind::Other),

        "cull" => match args.first().map(|v| v.to_ascii_lowercase()).as_deref() {
            Some("front") => ShaderDirectiveKind::Cull(CullMode::Front),
            Some("back") => ShaderDirectiveKind::Cull(CullMode::Back),
            Some("none" | "disable" | "twosided") => ShaderDirectiveKind::Cull(CullMode::None),
            Some(value) => ShaderDirectiveKind::Cull(CullMode::Other(value.to_string())),
            None => ShaderDirectiveKind::Other,
        },

        "skyparms" => ShaderDirectiveKind::SkyParms(args.to_vec()),
        "deformvertexes" => ShaderDirectiveKind::DeformVertexes(args.to_vec()),
        "sort" => ShaderDirectiveKind::Sort(args.to_vec()),
        "fogparms" => ShaderDirectiveKind::FogParms(args.to_vec()),
        "polygonoffset" => ShaderDirectiveKind::PolygonOffset,
        "nomipmaps" => ShaderDirectiveKind::NoMipMaps,
        "nopicmip" => ShaderDirectiveKind::NoPicMip,

        _ => ShaderDirectiveKind::Other,
    }
}

fn interpret_stage_directive(keyword: &str, args: &[String]) -> StageDirectiveKind {
    match keyword {
        "map" => args
            .first()
            .map(|name| StageDirectiveKind::Map(TextureMap::Map(name.clone())))
            .unwrap_or(StageDirectiveKind::Other),

        "clampmap" => args
            .first()
            .map(|name| StageDirectiveKind::Map(TextureMap::ClampMap(name.clone())))
            .unwrap_or(StageDirectiveKind::Other),

        "animmap" => parse_animation(args, false)
            .map(StageDirectiveKind::Map)
            .unwrap_or(StageDirectiveKind::Other),

        "clampanimmap" => parse_animation(args, true)
            .map(StageDirectiveKind::Map)
            .unwrap_or(StageDirectiveKind::Other),

        "oneshotanimmap" => Some(parse_one_shot_animation(args))
            .map(StageDirectiveKind::Map)
            .unwrap_or(StageDirectiveKind::Other),

        "videomap" => args
            .first()
            .map(|name| StageDirectiveKind::Map(TextureMap::VideoMap(name.clone())))
            .unwrap_or(StageDirectiveKind::Other),

        "blendfunc" => StageDirectiveKind::BlendFunc(parse_blend(args)),

        "rgbgen" => StageDirectiveKind::RgbGen(parse_rgb_gen(args)),

        "alphagen" => StageDirectiveKind::AlphaGen(parse_alpha_gen(args)),

        "tcgen" => StageDirectiveKind::TcGen(parse_tc_gen(args)),

        "tcmod" => StageDirectiveKind::TcMod(parse_tc_mod(args)),

        "depthwrite" => StageDirectiveKind::DepthWrite,

        "detail" => StageDirectiveKind::Detail,

        "alphafunc" => StageDirectiveKind::AlphaFunc(args.to_vec()),

        _ => StageDirectiveKind::Other,
    }
}

fn parse_animation(args: &[String], clamp: bool) -> Option<TextureMap> {
    let frequency = args.first()?.parse::<f32>().ok();
    let frames = args
        .iter()
        .skip(1)
        .filter(|word| !word.is_empty())
        .cloned()
        .collect::<Vec<_>>();

    if frames.is_empty() {
        return None;
    }

    Some(if clamp {
        TextureMap::ClampAnimMap { frequency, frames }
    } else {
        TextureMap::AnimMap { frequency, frames }
    })
}

fn parse_one_shot_animation(args: &[String]) -> TextureMap {
    let frequency = args.first().and_then(|word| word.parse::<f32>().ok());
    let frames = args.iter().skip(1).cloned().collect();

    TextureMap::OneShotAnimMap { frequency, frames }
}

fn parse_blend(args: &[String]) -> BlendFunction {
    match args {
        [mode] if mode.eq_ignore_ascii_case("add") => BlendFunction::Add,
        [mode] if mode.eq_ignore_ascii_case("filter") => BlendFunction::Filter,
        [mode] if mode.eq_ignore_ascii_case("blend") => BlendFunction::Blend,
        [source, destination] => BlendFunction::Explicit {
            source: source.clone(),
            destination: destination.clone(),
        },
        _ => BlendFunction::Invalid(args.to_vec()),
    }
}

fn parse_rgb_gen(args: &[String]) -> ColourGeneration {
    let Some(mode) = args.first().map(|word| word.to_ascii_lowercase()) else {
        return ColourGeneration::Other(Vec::new());
    };

    match mode.as_str() {
        "identity" => ColourGeneration::Identity,
        "identitylighting" => ColourGeneration::IdentityLighting,
        "vertex" => ColourGeneration::Vertex,
        "exactvertex" => ColourGeneration::ExactVertex,
        "entity" => ColourGeneration::Entity,
        "oneminusentity" => ColourGeneration::OneMinusEntity,
        "wave" => ColourGeneration::Wave(args.iter().skip(1).cloned().collect()),
        "const" => ColourGeneration::Constant(args.iter().skip(1).cloned().collect()),
        _ => ColourGeneration::Other(args.to_vec()),
    }
}

fn parse_alpha_gen(args: &[String]) -> AlphaGeneration {
    let Some(mode) = args.first().map(|word| word.to_ascii_lowercase()) else {
        return AlphaGeneration::Other(Vec::new());
    };

    match mode.as_str() {
        "identity" => AlphaGeneration::Identity,
        "skip" => AlphaGeneration::Skip,
        "vertex" => AlphaGeneration::Vertex,
        "entity" => AlphaGeneration::Entity,
        "oneminusentity" => AlphaGeneration::OneMinusEntity,
        "wave" => AlphaGeneration::Wave(args.iter().skip(1).cloned().collect()),
        "const" => AlphaGeneration::Constant(args.iter().skip(1).cloned().collect()),
        _ => AlphaGeneration::Other(args.to_vec()),
    }
}

fn parse_tc_gen(args: &[String]) -> TextureCoordinateGeneration {
    let Some(mode) = args.first().map(|word| word.to_ascii_lowercase()) else {
        return TextureCoordinateGeneration::Other(Vec::new());
    };

    match mode.as_str() {
        "base" => TextureCoordinateGeneration::Base,
        "lightmap" => TextureCoordinateGeneration::Lightmap,
        "environment" => TextureCoordinateGeneration::Environment,
        "vector" => TextureCoordinateGeneration::Vector(args.iter().skip(1).cloned().collect()),
        _ => TextureCoordinateGeneration::Other(args.to_vec()),
    }
}

fn parse_tc_mod(args: &[String]) -> TextureCoordinateModifier {
    let Some(mode) = args.first().map(|word| word.to_ascii_lowercase()) else {
        return TextureCoordinateModifier::Other(Vec::new());
    };

    let values = args.iter().skip(1).cloned().collect();

    match mode.as_str() {
        "rotate" => TextureCoordinateModifier::Rotate(values),
        "scale" => TextureCoordinateModifier::Scale(values),
        "scroll" => TextureCoordinateModifier::Scroll(values),
        "stretch" => TextureCoordinateModifier::Stretch(values),
        "transform" => TextureCoordinateModifier::Transform(values),
        "turb" => TextureCoordinateModifier::Turb(values),
        "entitytranslate" => TextureCoordinateModifier::EntityTranslate,
        _ => TextureCoordinateModifier::Other(args.to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shader(directives: Vec<Vec<&str>>, stages: Vec<Vec<Vec<&str>>>) -> ParsedShader {
        ParsedShader {
            name: "Textures/Test/Example".to_string(),
            directives: directives
                .into_iter()
                .map(|line| line.into_iter().map(str::to_string).collect())
                .collect(),
            stages: stages
                .into_iter()
                .map(|stage| {
                    stage
                        .into_iter()
                        .map(|line| line.into_iter().map(str::to_string).collect())
                        .collect()
                })
                .collect(),
        }
    }
    #[test]
    fn interprets_shader_level_rendering_directives() {
        let parsed = shader(
            vec![
                vec!["surfaceparm", "sky"],
                vec!["cull", "none"],
                vec!["skyParms", "env/hoth", "512", "512"],
                vec!["deformVertexes", "wave", "100", "sin", "0", "1", "0", "1"],
                vec!["sort", "additive"],
                vec!["polygonOffset"],
            ],
            vec![],
        );

        let result = interpret(parsed);

        assert!(matches!(
            result.directives[0].kind,
            ShaderDirectiveKind::SurfaceParm(_)
        ));
        assert!(matches!(
            result.directives[1].kind,
            ShaderDirectiveKind::Cull(CullMode::None)
        ));
        assert!(matches!(
            result.directives[2].kind,
            ShaderDirectiveKind::SkyParms(_)
        ));
        assert!(matches!(
            result.directives[3].kind,
            ShaderDirectiveKind::DeformVertexes(_)
        ));
        assert!(matches!(
            result.directives[4].kind,
            ShaderDirectiveKind::Sort(_)
        ));
        assert!(matches!(
            result.directives[5].kind,
            ShaderDirectiveKind::PolygonOffset
        ));
    }
    #[test]
    fn preserves_shader_directives_and_stage_order() {
        let parsed = shader(
            vec![
                vec!["surfaceparm", "trans"],
                vec!["cull", "none"],
                vec!["skyParms", "env/hoth", "512", "512"],
            ],
            vec![
                vec![vec!["map", "textures/hoth/base"], vec!["rgbGen", "vertex"]],
                vec![vec!["map", "textures/hoth/glow"], vec!["blendFunc", "add"]],
            ],
        );

        let result = interpret(parsed);

        assert_eq!(result.name, "textures/test/example");
        assert_eq!(result.directives.len(), 3);
        assert_eq!(result.stages.len(), 2);
        assert_eq!(result.stages[0].raw[0][1], "textures/hoth/base");
        assert_eq!(result.stages[1].raw[0][1], "textures/hoth/glow");
    }

    #[test]
    fn recognises_keywords_without_case_sensitivity() {
        let parsed = shader(
            vec![vec!["SURFACEPARM", "trans"]],
            vec![vec![
                vec!["MAP", "textures/test"],
                vec!["BLENDFUNC", "ADD"],
                vec!["RGBGEN", "EXACTVERTEX"],
            ]],
        );

        let result = interpret(parsed);

        assert_eq!(result.directives[0].keyword, "surfaceparm");

        assert!(matches!(
            result.stages[0].directives[0].kind,
            StageDirectiveKind::Map(TextureMap::Map(_))
        ));

        assert!(matches!(
            result.stages[0].directives[1].kind,
            StageDirectiveKind::BlendFunc(BlendFunction::Add)
        ));

        assert!(matches!(
            result.stages[0].directives[2].kind,
            StageDirectiveKind::RgbGen(ColourGeneration::ExactVertex)
        ));
    }

    #[test]
    fn preserves_stage_without_texture_map() {
        let parsed = shader(
            vec![],
            vec![vec![vec!["depthWrite"], vec!["alphaFunc", "GE128"]]],
        );

        let result = interpret(parsed);

        assert_eq!(result.stages.len(), 1);
        assert_eq!(result.stages[0].directives.len(), 2);
        assert!(matches!(
            result.stages[0].directives[0].kind,
            StageDirectiveKind::DepthWrite
        ));
    }

    #[test]
    fn recognises_animated_texture_maps() {
        let parsed = shader(
            vec![],
            vec![vec![
                vec!["animMap", "4", "frame1", "frame2", "frame3"],
                vec!["clampAnimMap", "2", "frameA", "frameB"],
                vec!["oneshotanimmap", "8", "once1", "once2"],
            ]],
        );

        let result = interpret(parsed);
        let stage = &result.stages[0];

        assert!(matches!(
            &stage.directives[0].kind,
            StageDirectiveKind::Map(TextureMap::AnimMap { frequency: Some(4.0), frames })
                if frames.len() == 3
        ));

        assert!(matches!(
            &stage.directives[1].kind,
            StageDirectiveKind::Map(TextureMap::ClampAnimMap { frequency: Some(2.0), frames })
                if frames.len() == 2
        ));

        assert!(matches!(
            &stage.directives[2].kind,
            StageDirectiveKind::Map(TextureMap::OneShotAnimMap { frequency: Some(8.0), frames })
                if frames.len() == 2
        ));
    }

    #[test]
    fn preserves_multiple_texture_coordinate_modifiers() {
        let parsed = shader(
            vec![],
            vec![vec![
                vec!["tcMod", "scroll", "0.1", "0.2"],
                vec!["tcMod", "scale", "2", "2"],
                vec!["tcMod", "rotate", "45"],
            ]],
        );

        let result = interpret(parsed);
        let stage = &result.stages[0];

        assert_eq!(stage.directives.len(), 3);

        assert!(matches!(
            stage.directives[0].kind,
            StageDirectiveKind::TcMod(TextureCoordinateModifier::Scroll(_))
        ));

        assert!(matches!(
            stage.directives[1].kind,
            StageDirectiveKind::TcMod(TextureCoordinateModifier::Scale(_))
        ));

        assert!(matches!(
            stage.directives[2].kind,
            StageDirectiveKind::TcMod(TextureCoordinateModifier::Rotate(_))
        ));
    }

    #[test]
    fn recognises_generation_and_coordinate_directives() {
        let parsed = shader(
            vec![],
            vec![vec![
                vec!["rgbGen", "wave", "sin", "0.5", "0.5", "0.1", "1"],
                vec!["alphaGen", "const", "0.5"],
                vec!["tcGen", "environment"],
                vec!["tcGen", "vector", "1", "0", "0", "0", "1", "0"],
            ]],
        );

        let result = interpret(parsed);
        let stage = &result.stages[0];

        assert!(matches!(
            stage.directives[0].kind,
            StageDirectiveKind::RgbGen(ColourGeneration::Wave(_))
        ));

        assert!(matches!(
            stage.directives[1].kind,
            StageDirectiveKind::AlphaGen(AlphaGeneration::Constant(_))
        ));

        assert!(matches!(
            stage.directives[2].kind,
            StageDirectiveKind::TcGen(TextureCoordinateGeneration::Environment)
        ));

        assert!(matches!(
            stage.directives[3].kind,
            StageDirectiveKind::TcGen(TextureCoordinateGeneration::Vector(_))
        ));
    }

    #[test]
    fn preserves_unknown_and_malformed_directives() {
        let parsed = shader(
            vec![vec![
                "deformVertexes",
                "wave",
                "100",
                "sin",
                "0",
                "1",
                "0",
                "1",
            ]],
            vec![
                vec![
                    vec!["blendFunc"],
                    vec!["unknownDirective", "some", "arguments"],
                ],
                vec![vec!["map", "textures/still-present"]],
            ],
        );

        let result = interpret(parsed);

        assert_eq!(result.stages.len(), 2);
        assert_eq!(result.stages[0].directives.len(), 2);
        assert!(matches!(
            result.stages[0].directives[0].kind,
            StageDirectiveKind::BlendFunc(BlendFunction::Invalid(_))
        ));
        assert!(matches!(
            result.stages[0].directives[1].kind,
            StageDirectiveKind::Other
        ));
        assert_eq!(result.stages[1].raw[0][1], "textures/still-present");
        assert_eq!(result.raw.directives[0][0], "deformVertexes");
    }
}

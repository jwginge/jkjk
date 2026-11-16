use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendState, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

const JA_SHADER: Handle<Shader> = uuid_handle!("d4b7c921-6e3a-4f85-a012-9c5e7b2d6381");

/// Original texture declaration for a Jedi Academy shader stage.
#[derive(Clone, Debug)]
pub enum JaTextureSource {
    Map {
        name: String,
        clamp: bool,
    },
    Animation {
        frequency: Option<f32>,
        frames: Vec<String>,
        clamp: bool,
        once: bool,
    },
    Video(String),
}

#[derive(Clone, Debug)]
pub struct JaRenderStage {
    pub texture: Option<JaTextureSource>,
    pub blend: Option<crate::ja_shader::BlendFunction>,
    pub depth_write: Option<bool>,
    pub alpha_test: Option<JaAlphaTest>,
    pub rgb_gen: JaRgbGen,
    pub alpha_gen: JaAlphaGen,
    pub tc_gen: JaTcGen,
    pub tc_mods: Vec<JaTcMod>,
}

#[derive(Clone, Debug)]
pub enum JaAlphaTest {
    GreaterZero,
    LessHalf,
    GreaterEqualHalf,
    Other(Vec<String>),
}

#[derive(Clone, Debug)]
pub enum JaRgbGen {
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

#[derive(Clone, Debug)]
pub enum JaAlphaGen {
    Identity,
    Skip,
    Vertex,
    Entity,
    OneMinusEntity,
    Wave(Vec<String>),
    Constant(Vec<String>),
    Other(Vec<String>),
}

#[derive(Clone, Debug)]
pub enum JaTcGen {
    Base,
    Lightmap,
    Environment,
    Vector(Vec<String>),
    Other(Vec<String>),
}

#[derive(Clone, Debug)]
pub enum JaTcMod {
    Rotate(Vec<String>),
    Scale(Vec<String>),
    Scroll(Vec<String>),
    Stretch(Vec<String>),
    Transform(Vec<String>),
    Turb(Vec<String>),
    EntityTranslate,
    Other(Vec<String>),
}

pub fn convert_ja_stage(stage: &crate::ja_shader::JaStage) -> JaRenderStage {
    use crate::ja_shader::{
        AlphaGeneration, ColourGeneration, StageDirectiveKind, TextureCoordinateGeneration,
        TextureCoordinateModifier, TextureMap,
    };

    let mut texture = None;
    let mut blend = None;
    let mut depth_write = None;
    let mut alpha_test = None;
    let mut rgb_gen = JaRgbGen::Identity;
    let mut alpha_gen = JaAlphaGen::Identity;
    let mut tc_gen = JaTcGen::Base;
    let mut tc_mods = Vec::new();

    for directive in &stage.directives {
        match &directive.kind {
            StageDirectiveKind::Map(map) => {
                texture = Some(match map {
                    TextureMap::Map(name) => JaTextureSource::Map {
                        name: name.clone(),
                        clamp: false,
                    },
                    TextureMap::ClampMap(name) => JaTextureSource::Map {
                        name: name.clone(),
                        clamp: true,
                    },
                    TextureMap::AnimMap { frequency, frames } => JaTextureSource::Animation {
                        frequency: *frequency,
                        frames: frames.clone(),
                        clamp: false,
                        once: false,
                    },
                    TextureMap::ClampAnimMap { frequency, frames } => JaTextureSource::Animation {
                        frequency: *frequency,
                        frames: frames.clone(),
                        clamp: true,
                        once: false,
                    },
                    TextureMap::OneShotAnimMap { frequency, frames } => {
                        JaTextureSource::Animation {
                            frequency: *frequency,
                            frames: frames.clone(),
                            clamp: false,
                            once: true,
                        }
                    }
                    TextureMap::VideoMap(name) => JaTextureSource::Video(name.clone()),
                });
            }
            StageDirectiveKind::BlendFunc(value) => {
                blend = Some(value.clone());
            }
            StageDirectiveKind::DepthWrite => {
                depth_write = Some(true);
            }
            StageDirectiveKind::AlphaFunc(args) => {
                alpha_test = Some(match args.first().map(|s| s.to_ascii_lowercase()) {
                    Some(ref s) if s == "gt0" => JaAlphaTest::GreaterZero,
                    Some(ref s) if s == "lt128" => JaAlphaTest::LessHalf,
                    Some(ref s) if s == "ge128" => JaAlphaTest::GreaterEqualHalf,
                    _ => JaAlphaTest::Other(args.clone()),
                });
            }
            StageDirectiveKind::RgbGen(value) => {
                rgb_gen = match value {
                    ColourGeneration::Identity => JaRgbGen::Identity,
                    ColourGeneration::IdentityLighting => JaRgbGen::IdentityLighting,
                    ColourGeneration::Vertex => JaRgbGen::Vertex,
                    ColourGeneration::ExactVertex => JaRgbGen::ExactVertex,
                    ColourGeneration::Entity => JaRgbGen::Entity,
                    ColourGeneration::OneMinusEntity => JaRgbGen::OneMinusEntity,
                    ColourGeneration::Wave(args) => JaRgbGen::Wave(args.clone()),
                    ColourGeneration::Constant(args) => JaRgbGen::Constant(args.clone()),
                    ColourGeneration::Other(args) => JaRgbGen::Other(args.clone()),
                };
            }
            StageDirectiveKind::AlphaGen(value) => {
                alpha_gen = match value {
                    AlphaGeneration::Identity => JaAlphaGen::Identity,
                    AlphaGeneration::Skip => JaAlphaGen::Skip,
                    AlphaGeneration::Vertex => JaAlphaGen::Vertex,
                    AlphaGeneration::Entity => JaAlphaGen::Entity,
                    AlphaGeneration::OneMinusEntity => JaAlphaGen::OneMinusEntity,
                    AlphaGeneration::Wave(args) => JaAlphaGen::Wave(args.clone()),
                    AlphaGeneration::Constant(args) => JaAlphaGen::Constant(args.clone()),
                    AlphaGeneration::Other(args) => JaAlphaGen::Other(args.clone()),
                };
            }
            StageDirectiveKind::TcGen(value) => {
                tc_gen = match value {
                    TextureCoordinateGeneration::Base => JaTcGen::Base,
                    TextureCoordinateGeneration::Lightmap => JaTcGen::Lightmap,
                    TextureCoordinateGeneration::Environment => JaTcGen::Environment,
                    TextureCoordinateGeneration::Vector(args) => JaTcGen::Vector(args.clone()),
                    TextureCoordinateGeneration::Other(args) => JaTcGen::Other(args.clone()),
                };
            }
            StageDirectiveKind::TcMod(value) => {
                tc_mods.push(match value {
                    TextureCoordinateModifier::Rotate(args) => JaTcMod::Rotate(args.clone()),
                    TextureCoordinateModifier::Scale(args) => JaTcMod::Scale(args.clone()),
                    TextureCoordinateModifier::Scroll(args) => JaTcMod::Scroll(args.clone()),
                    TextureCoordinateModifier::Stretch(args) => JaTcMod::Stretch(args.clone()),
                    TextureCoordinateModifier::Transform(args) => JaTcMod::Transform(args.clone()),
                    TextureCoordinateModifier::Turb(args) => JaTcMod::Turb(args.clone()),
                    TextureCoordinateModifier::EntityTranslate => JaTcMod::EntityTranslate,
                    TextureCoordinateModifier::Other(args) => JaTcMod::Other(args.clone()),
                });
            }
            StageDirectiveKind::Detail | StageDirectiveKind::Other => {}
        }
    }

    JaRenderStage {
        texture,
        blend,
        depth_write,
        alpha_test,
        rgb_gen,
        alpha_gen,
        tc_gen,
        tc_mods,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct JaMaterialKey {
    pub blend: Option<BlendState>,
    pub depth_write: bool,
    pub cull: bool,
}

/// One ordered Jedi Academy texture-coordinate operation.
/// meta.x = operation kind; meta.y = waveform kind.
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct JaTcModOp {
    pub meta: UVec4,
    pub params: Vec4,
    pub extra: Vec4,
}

/// Four ordered tcMod operations and their shared animation clock.
/// time_count.x = seconds; time_count.y = number of active operations.
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct JaTcModUniform {
    pub time_count: Vec4,
    pub ops: [JaTcModOp; 4],
}

/// RGB waveform: kind.x = waveform code; params = base, amplitude, phase, frequency.
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct JaRgbWaveUniform {
    pub meta: UVec4,
    pub params: Vec4,
}

/// Linear fog range and colour, in Bevy metres and linear RGB.
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct JaFogUniform {
    pub range: Vec4,
    pub color: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[bind_group_data(JaMaterialKey)]
pub struct JaMaterial {
    #[uniform(0)]
    pub tint: Vec4,

    #[texture(1)]
    #[sampler(2)]
    pub texture: Handle<Image>,

    // x: texture coordinates (0 = base UV, 1 = lightmap UV)
    // y: multiply by vertex colour (0 = no, 1 = yes)
    // z: force diffuse texture alpha to 1 (opaque texture)
    #[uniform(3)]
    pub params: Vec4,

    #[texture(4)]
    #[sampler(5)]
    pub lightmap: Option<Handle<Image>>,

    #[uniform(6)]
    pub lightmap_enabled: u32, // 1 if lightmap.is_some()

    #[uniform(7)]
    pub uv_offset: Vec2,

    #[uniform(8)]
    pub tc_mods: JaTcModUniform,

    #[uniform(9)]
    pub rgb_wave: JaRgbWaveUniform,

    #[uniform(10)]
    pub fog: JaFogUniform,

    pub blend: Option<BlendState>,
    pub alpha_mode: AlphaMode,
    pub depth_write: bool,
    pub cull: bool,
    pub stage_order: u32,
    pub transparent: bool,
}

impl From<&JaMaterial> for JaMaterialKey {
    fn from(material: &JaMaterial) -> Self {
        Self {
            blend: material.blend,
            depth_write: material.depth_write,
            cull: material.cull,
        }
    }
}

impl Material for JaMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(JA_SHADER)
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(JA_SHADER)
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn depth_bias(&self) -> f32 {
        (self.stage_order as f32) * 0.001
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let key = key.bind_group_data;

        descriptor.primitive.cull_mode = key
            .cull
            .then_some(bevy::render::render_resource::Face::Back);

        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = key.blend;
            }
        }

        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(key.depth_write);
            depth.depth_compare =
                Some(bevy::render::render_resource::CompareFunction::GreaterEqual);
        }

        Ok(())
    }
}

pub struct JaMaterialPlugin;

impl Plugin for JaMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, JA_SHADER, "ja.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<JaMaterial>::default());
    }
}

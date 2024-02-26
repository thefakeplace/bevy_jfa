#![feature(const_type_id)]

//! A Bevy library for computing the Jump Flooding Algorithm.
//!
//! The **jump flooding algorithm** (JFA) is a fast screen-space algorithm for
//! computing distance fields. Currently, this crate provides a plugin for
//! adding outlines to arbitrary meshes.
//!
//! Outlines adapted from ["The Quest for Very Wide Outlines" by Ben Golus][0].
//!
//! [0]: https://bgolus.medium.com/the-quest-for-very-wide-outlines-ba82ed442cd9
//!
//! # Setup
//!
//! To add an outline to a mesh:
//!
//! 1. Add the [`OutlinePlugin`] to the base `App`.
//! 2. Add the desired [`OutlineStyle`] as an `Asset`.
//! 3. Add a [`CameraOutline`] component with the desired `OutlineStyle` to the
//!    camera which should render the outline.  Currently, outline styling is
//!    tied to the camera rather than the mesh.
//! 4. Add an [`Outline`] component to the mesh with `enabled: true`.

use std::{any::TypeId, ops::Range};

use bevy::{
    app::prelude::*, asset::{Asset, AssetApp, AssetId, Assets, Handle, UntypedAssetId, UntypedHandle}, core_pipeline::core_3d, ecs::{prelude::*, query::QueryItem, system::{lifetimeless::SRes, SystemParamItem}}, math::Mat4, pbr::{DrawMesh, MaterialBindGroupId, Mesh3d, MeshPipelineKey, MeshTransforms, MeshUniform, RenderMeshInstances, SetMeshBindGroup, SetMeshViewBindGroup}, prelude::Camera3d, reflect::{TypePath, TypeUuid}, render::{
        batching::{batch_and_prepare_render_phase, GetBatchData}, extract_resource::ExtractResource, prelude::*, render_asset::{PrepareAssetError, RenderAsset, RenderAssetPlugin, RenderAssets}, render_graph::RenderGraph, render_phase::{
            AddRenderCommand, CachedRenderPipelinePhaseItem, DrawFunctionId, DrawFunctions,
            PhaseItem, RenderPhase, SetItemPipeline,
        }, render_resource::*, renderer::{RenderDevice, RenderQueue}, view::{ExtractedView, VisibleEntities}, Extract, Render, RenderApp, RenderSet
    }, transform::components::GlobalTransform, utils::{nonmax::NonMaxU32, FloatOrd, Uuid}
};

use crate::{
    graph::OutlineDriverNode,
    mask::MeshMaskPipeline,
    outline::{GpuOutlineParams, OutlineParams},
    resources::OutlineResources,
};

mod graph;
mod jfa;
mod jfa_init;
mod mask;
mod outline;
mod resources;

#[derive(Component)]
pub struct ExtractedOutline {
    mesh: Handle<Mesh>,
    transform: Mat4,
}

const JFA_TEXTURE_FORMAT: TextureFormat = TextureFormat::Rg16Snorm;
const FULLSCREEN_PRIMITIVE_STATE: PrimitiveState = PrimitiveState {
    topology: PrimitiveTopology::TriangleList,
    strip_index_format: None,
    front_face: FrontFace::Ccw,
    cull_mode: Some(Face::Back),
    unclipped_depth: false,
    polygon_mode: PolygonMode::Fill,
    conservative: false,
};

/// Top-level plugin for enabling outlines.
#[derive(Default)]
pub struct OutlinePlugin;

/// Performance and visual quality settings for JFA-based outlines.
#[derive(Clone, ExtractResource, Resource)]
pub struct OutlineSettings {
    pub(crate) half_resolution: bool,
}

impl OutlineSettings {
    /// Returns whether the half-resolution setting is enabled.
    pub fn half_resolution(&self) -> bool {
        self.half_resolution
    }

    /// Sets whether the half-resolution setting is enabled.
    pub fn set_half_resolution(&mut self, value: bool) {
        self.half_resolution = value;
    }
}

impl Default for OutlineSettings {
    fn default() -> Self {
        println!("creating outline settings");
        Self {
            half_resolution: false,
        }
    }
}

const MASK_SHADER_HANDLE: UntypedHandle =
    UntypedHandle::Weak(UntypedAssetId::Uuid {
        type_id: TypeId::of::<Shader>(),
        uuid: Uuid::from_u128(10400755559809425757),
    });
const JFA_INIT_SHADER_HANDLE: UntypedHandle =
    UntypedHandle::Weak(UntypedAssetId::Uuid {
        type_id: TypeId::of::<Shader>(),
        uuid: Uuid::from_u128(11038189062916158841),
    });
const JFA_SHADER_HANDLE: UntypedHandle =
    UntypedHandle::Weak(UntypedAssetId::Uuid {
        type_id: TypeId::of::<Shader>(),
        uuid: Uuid::from_u128(5227804998548228051),
    });
const FULLSCREEN_SHADER_HANDLE: UntypedHandle =
    UntypedHandle::Weak(UntypedAssetId::Uuid {
        type_id: TypeId::of::<Shader>(),
        uuid: Uuid::from_u128(12099561278220359682),
    });
const OUTLINE_SHADER_HANDLE: UntypedHandle =
    UntypedHandle::Weak(UntypedAssetId::Uuid {
        type_id: TypeId::of::<Shader>(),
        uuid: Uuid::from_u128(11094028876979933159),
    });
const DIMENSIONS_SHADER_HANDLE: UntypedHandle =
    UntypedHandle::Weak(UntypedAssetId::Uuid {
        type_id: TypeId::of::<Shader>(),
        uuid: Uuid::from_u128(11721531257850828867),
    });

use crate::graph::outline as outline_graph;

impl Plugin for OutlinePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(RenderAssetPlugin::<OutlineStyle>::default())
            .init_asset::<OutlineStyle>()
            .init_resource::<OutlineSettings>();

        let mut shaders = app.world.get_resource_mut::<Assets<Shader>>().unwrap();

        let mask_shader = Shader::from_wgsl(include_str!("shaders/mask.wgsl"), "shaders/mask.wgsl");
        let jfa_init_shader = Shader::from_wgsl(include_str!("shaders/jfa_init.wgsl"), "shaders/jfa_init.wgsl");
        let jfa_shader = Shader::from_wgsl(include_str!("shaders/jfa.wgsl"), "shaders/jfa.wgsl");
        let fullscreen_shader = Shader::from_wgsl(include_str!("shaders/fullscreen.wgsl"), "shaders/fullscreen.wgsl")
            .with_import_path("outline::fullscreen");
        let outline_shader = Shader::from_wgsl(include_str!("shaders/outline.wgsl"), "shaders/outline.wgsl");
        let dimensions_shader = Shader::from_wgsl(include_str!("shaders/dimensions.wgsl"), "shaders/dimensions.wgsl")
            .with_import_path("outline::dimensions");

        shaders.insert(MASK_SHADER_HANDLE, mask_shader);
        shaders.insert(JFA_INIT_SHADER_HANDLE, jfa_init_shader);
        shaders.insert(JFA_SHADER_HANDLE, jfa_shader);
        shaders.insert(FULLSCREEN_SHADER_HANDLE, fullscreen_shader);
        shaders.insert(OUTLINE_SHADER_HANDLE, outline_shader);
        shaders.insert(DIMENSIONS_SHADER_HANDLE, dimensions_shader);
    }

    fn finish(&self, app: &mut App) {
        let render_app = match app.get_sub_app_mut(RenderApp) {
            Ok(r) => r,
            Err(_) => return,
        };

        render_app
            .init_resource::<DrawFunctions<MeshMask>>()
            .add_render_command::<MeshMask, SetItemPipeline>()
            .add_render_command::<MeshMask, DrawMeshMask>()
            .init_resource::<resources::OutlineResources>()
            .init_resource::<mask::MeshMaskPipeline>()
            .init_resource::<SpecializedMeshPipelines<mask::MeshMaskPipeline>>()
            .init_resource::<jfa_init::JfaInitPipeline>()
            .init_resource::<jfa::JfaPipeline>()
            .init_resource::<outline::OutlinePipeline>()
            .init_resource::<SpecializedRenderPipelines<outline::OutlinePipeline>>()
            .add_systems(ExtractSchedule, (
                extract_outline_settings,
                extract_camera_outlines,
                extract_mask_camera_phase,
                extract_outline_targets))
            .add_systems(Render, (
                resources::recreate_outline_resources,
                queue_mesh_masks,
            ).in_set(RenderSet::QueueMeshes))
            .add_systems(Render, (
                batch_and_prepare_render_phase::<MeshMask, MeshMaskPipeline>
            ).in_set(RenderSet::PrepareResources));

        let render_app = match app.get_sub_app_mut(RenderApp) {
            Ok(r) => r,
            Err(_) => return,
        };

        let outline_graph = graph::outline(render_app).unwrap();

        let mut root_graph = render_app.world.resource_mut::<RenderGraph>();
        let draw_3d_graph = root_graph.get_sub_graph_mut(core_3d::CORE_3D).unwrap();

        draw_3d_graph.add_sub_graph(outline_graph::NAME, outline_graph);
        draw_3d_graph.add_node(OutlineDriverNode::NAME, OutlineDriverNode);
        draw_3d_graph.add_node_edge(core_3d::graph::node::MAIN_TRANSPARENT_PASS, OutlineDriverNode::NAME);
    }
}

struct MeshMask {
    distance: f32,
    pipeline: CachedRenderPipelineId,
    entity: Entity,
    draw_function: DrawFunctionId,
    batch_range: Range<u32>,
    dynamic_offset: Option<NonMaxU32>,
}

impl PhaseItem for MeshMask {
    type SortKey = FloatOrd;

    fn sort_key(&self) -> Self::SortKey {
        FloatOrd(self.distance)
    }

    fn draw_function(&self) -> DrawFunctionId {
        self.draw_function
    }

    fn entity(&self) -> Entity {
        self.entity
    }

    fn batch_range(&self) -> &std::ops::Range<u32> {
        &self.batch_range
    }

    fn batch_range_mut(&mut self) -> &mut std::ops::Range<u32> {
        &mut self.batch_range
    }

    fn dynamic_offset(&self) -> Option<bevy::utils::nonmax::NonMaxU32> {
        self.dynamic_offset
    }

    fn dynamic_offset_mut(&mut self) -> &mut Option<bevy::utils::nonmax::NonMaxU32> {
        &mut self.dynamic_offset
    }
}

impl CachedRenderPipelinePhaseItem for MeshMask {
    fn cached_pipeline(&self) -> CachedRenderPipelineId {
        self.pipeline
    }
}

type DrawMeshMask = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMeshBindGroup<1>,
    DrawMesh,
);

/// Visual style for an outline.
#[derive(Asset, TypePath, Clone, Debug, PartialEq)]
pub struct OutlineStyle {
    pub color: Color,
    pub inner_color: Color,
    pub width: f32,
}

impl RenderAsset for OutlineStyle {
    type ExtractedAsset = OutlineParams;
    type PreparedAsset = GpuOutlineParams;
    type Param = (
        Res<'static, RenderDevice>,
        Res<'static, RenderQueue>,
        Res<'static, OutlineResources>,
    );

    fn extract_asset(&self) -> Self::ExtractedAsset {
        OutlineParams::new(self.color, self.inner_color, self.width)
    }

    fn prepare_asset(
        extracted_asset: Self::ExtractedAsset,
        (device, queue, outline_res): &mut SystemParamItem<Self::Param>,
    ) -> Result<Self::PreparedAsset, PrepareAssetError<Self::ExtractedAsset>> {
        let mut buffer = UniformBuffer::from(extracted_asset.clone());
        buffer.write_buffer(device, queue);

        let bind_group = device.create_bind_group(None,
            &outline_res.outline_params_bind_group_layout,
            &[BindGroupEntry {
                binding: 0,
                resource: buffer.buffer().unwrap().as_entire_binding(),
            }]);

        Ok(GpuOutlineParams {
            params: extracted_asset,
            _buffer: buffer,
            bind_group,
        })
    }
}

/// Component for enabling outlines when rendering with a given camera.
#[derive(Clone, Debug, PartialEq, Component)]
pub struct CameraOutline {
    pub enabled: bool,
    pub style: Handle<OutlineStyle>,
}

/// Component for entities that should be outlined.
#[derive(Clone, Debug, PartialEq, Component)]
pub struct Outline {
    pub enabled: bool,
}

fn extract_outline_settings(mut commands: Commands, settings: Extract<Res<OutlineSettings>>) {
    commands.insert_resource(settings.clone());
}

fn extract_camera_outlines(
    mut commands: Commands,
    mut previous_outline_len: Local<usize>,
    cam_outline_query: Extract<Query<(Entity, &CameraOutline), With<Camera>>>,
) {
    let mut batches = Vec::with_capacity(*previous_outline_len);
    batches.extend(
        cam_outline_query
            .iter()
            .filter_map(|(entity, outline)| outline.enabled.then(|| (entity, (outline.clone(),)))),
    );
    *previous_outline_len = batches.len();
    commands.insert_or_spawn_batch(batches);
}

fn extract_mask_camera_phase(
    mut commands: Commands,
    cameras: Extract<Query<Entity, (With<Camera3d>, With<CameraOutline>)>>,
) {
    for entity in cameras.iter() {
        commands
            .get_or_spawn(entity)
            .insert(RenderPhase::<MeshMask>::default());
    }
}

fn extract_outline_targets(
    mut commands: Commands,
    query: Extract<Query<(Entity, &Outline, &Handle<Mesh>, &GlobalTransform)>>,
) {
    for (entity, outline, mesh, global_transform) in query.iter() {
        if outline.enabled {
            let cmds = &mut commands.get_or_spawn(entity);
                cmds.insert(ExtractedOutline {
                    mesh: mesh.clone(),
                    transform: global_transform.compute_matrix(),
                });
        }
    }
}

fn queue_mesh_masks(
    mesh_mask_draw_functions: Res<DrawFunctions<MeshMask>>,
    mesh_mask_pipeline: Res<MeshMaskPipeline>,
    mut pipelines: ResMut<SpecializedMeshPipelines<MeshMaskPipeline>>,
    mut pipeline_cache: ResMut<PipelineCache>,
    render_meshes: Res<RenderAssets<Mesh>>,
    outline_meshes: Query<(Entity, &ExtractedOutline)>,
    mut views: Query<(
        &ExtractedView,
        &mut VisibleEntities,
        &mut RenderPhase<MeshMask>,
    )>,
) {
    let draw_outline = mesh_mask_draw_functions
        .read()
        .get_id::<DrawMeshMask>()
        .unwrap();

    for (view, visible_entities, mut mesh_mask_phase) in views.iter_mut() {
        let view_matrix = view.transform.compute_matrix();
        let inv_view_row_2 = view_matrix.inverse().row(2);

        for visible_entity in visible_entities.entities.iter().copied() {
            let (entity, extracted_outline) = match outline_meshes.get(visible_entity) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let mesh = match render_meshes.get(&extracted_outline.mesh) {
                Some(m) => m,
                None => continue,
            };

            let key = MeshPipelineKey::from_primitive_topology(mesh.primitive_topology);

            let pipeline = pipelines
                .specialize(&mut pipeline_cache, &mesh_mask_pipeline, key, &mesh.layout)
                .unwrap();

            mesh_mask_phase.add(MeshMask {
                entity,
                pipeline,
                draw_function: draw_outline,
                distance: inv_view_row_2.dot(extracted_outline.transform.col(2)),
                batch_range: 0..1,
                dynamic_offset: None,
            });
        }
    }
}

impl GetBatchData for MeshMaskPipeline {
    type Param = SRes<RenderMeshInstances>;
    type Query = Entity;
    type QueryFilter = With<Mesh3d>;
    type CompareData = (MaterialBindGroupId, AssetId<Mesh>);
    type BufferData = MeshUniform;

    fn get_batch_data(
        mesh_instances: &SystemParamItem<Self::Param>,
        entity: &QueryItem<Self::Query>,
    ) -> (Self::BufferData, Option<Self::CompareData>) {
        let mesh_instance = mesh_instances
            .get(entity)
            .expect("Failed to find render mesh instance");
        (
            (&mesh_instance.transforms).into(),
            mesh_instance.automatic_batching.then_some((
                mesh_instance.material_bind_group_id,
                mesh_instance.mesh_asset_id,
            )),
        )
    }
}

use bevy::{
    ecs::system::lifetimeless::Read, pbr::{MeshPipeline, MeshPipelineKey, MeshPipelineViewLayoutKey}, prelude::*, render::{
        batching::GetBatchData, mesh::MeshVertexBufferLayout, render_graph::{Node, RenderGraphContext, SlotInfo, SlotType}, render_phase::RenderPhase, render_resource::{
            BlendComponent, BlendFactor, BlendOperation, BlendState, ColorTargetState, ColorWrites, FragmentState, LoadOp, MultisampleState, Operations, RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor, RenderPipelineDescriptor, SpecializedMeshPipeline, SpecializedMeshPipelineError, TextureFormat
        }, renderer::RenderContext, view::ViewDepthTexture
    }
};

use crate::{resources::OutlineResources, MeshMask, MASK_SHADER_HANDLE};

#[derive(Resource)]
pub struct MeshMaskPipeline {
    mesh_pipeline: MeshPipeline,
}

impl FromWorld for MeshMaskPipeline {
    fn from_world(world: &mut World) -> Self {
        let mesh_pipeline = world.get_resource::<MeshPipeline>().unwrap().clone();

        MeshMaskPipeline { mesh_pipeline }
    }
}

impl SpecializedMeshPipeline for MeshMaskPipeline {
    type Key = MeshPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        layout: &MeshVertexBufferLayout,
    ) -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        let mut desc = self.mesh_pipeline.specialize(key, layout)?;

        desc.layout = vec![
            self.mesh_pipeline.get_view_layout(MeshPipelineViewLayoutKey::MULTISAMPLED).clone(),
            self.mesh_pipeline.mesh_layouts.model_only.clone(),
        ];

        desc.primitive.cull_mode = None;
        desc.vertex.shader = MASK_SHADER_HANDLE.typed::<Shader>();

        desc.fragment = Some(FragmentState {
            shader: MASK_SHADER_HANDLE.typed::<Shader>(),
            shader_defs: vec![],
            entry_point: "fragment".into(),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: Some(BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::One,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent {
                        src_factor: BlendFactor::One,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                }),
                write_mask: ColorWrites::ALL,
            })],
        });
        desc.depth_stencil = None;

        desc.multisample = MultisampleState {
            count: 4,
            mask: !0,
            alpha_to_coverage_enabled: false,
        };

        desc.label = Some("mesh_stencil_pipeline".into());
        Ok(desc)
    }
}

/// Render graph node for producing stencils from meshes.
pub struct MeshMaskNode {
    query: QueryState<&'static RenderPhase<MeshMask>>,
}

impl MeshMaskNode {
    /// The produced stencil buffer.
    ///
    /// This has format `TextureFormat::Depth24PlusStencil8`. Fragments covered
    /// by a mesh are assigned a value of 255. All other fragments are assigned
    /// a value of 0. The depth aspect is unused.
    pub const OUT_MASK: &'static str = "stencil";

    pub fn new(world: &mut World) -> MeshMaskNode {
        MeshMaskNode {
            query: QueryState::new(world),
        }
    }
}

impl Node for MeshMaskNode {
    fn input(&self) -> Vec<SlotInfo> {
        vec![]
    }

    fn output(&self) -> Vec<SlotInfo> {
        vec![SlotInfo::new(Self::OUT_MASK, SlotType::TextureView)]
    }

    fn update(&mut self, world: &mut World) {
        self.query.update_archetypes(world);
    }

    fn run(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), bevy::render::render_graph::NodeRunError> {
        let res = world.get_resource::<OutlineResources>().unwrap();

        graph
            .set_output(Self::OUT_MASK, res.mask_multisample.default_view.clone())
            .unwrap();

        let view_entity = graph.view_entity();
        let Ok(stencil_phase) = self.query.get_manual(world, view_entity) else {
            return Ok(());
        };

        let mut tracked_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("outline_stencil_render_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &res.mask_multisample.default_view,
                resolve_target: Some(&res.mask_output.default_view),
                ops: Operations {
                    load: LoadOp::Clear(Color::BLACK.into()),
                    store: true,
                },
            })],
            depth_stencil_attachment: None,
        });

        stencil_phase.render(&mut tracked_pass, world, view_entity);

        Ok(())
    }
}

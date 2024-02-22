use bevy::{
    prelude::*,
    render::{
        render_graph::{
            Node, NodeRunError, RenderGraph, RenderGraphContext, RenderGraphError, SlotInfo,
            SlotType,
        },
        render_resource::TextureFormat,
        renderer::RenderContext,
        texture::BevyDefault,
    },
};

use crate::{jfa::JfaNode, jfa_init::JfaInitNode, mask::MeshMaskNode, outline::OutlineNode};

pub(crate) mod outline {
    pub const NAME: &str = "outline_graph";

    pub mod node {
        pub const MASK_PASS: &str = "mask_pass";
        pub const JFA_INIT_PASS: &str = "jfa_init_pass";
        pub const JFA_PASS: &str = "jfa_pass";
        pub const OUTLINE_PASS: &str = "outline_pass";
    }
}

pub struct OutlineDriverNode;

impl OutlineDriverNode {
    pub const NAME: &'static str = "outline_driver";
}

impl Node for OutlineDriverNode {
    fn run(
        &self,
        graph: &mut RenderGraphContext,
        _render_context: &mut RenderContext,
        _world: &World,
    ) -> Result<(), NodeRunError> {
        let view_ent = graph.view_entity();
        graph.run_sub_graph(outline::NAME, vec![], Some(view_ent))?;

        Ok(())
    }

    fn input(&self) -> Vec<SlotInfo> {
        vec![]
    }
}

/// Builds the render graph for applying the JFA outline.
pub fn outline(render_app: &mut App) -> Result<RenderGraph, RenderGraphError> {
    let mut graph = RenderGraph::default();

    // Graph order:
    // 1. Mask
    // 2. JFA Init
    // 3. JFA
    // 4. Outline

    let mask_node = MeshMaskNode::new(&mut render_app.world);
    let jfa_node = JfaNode::from_world(&mut render_app.world);
    // TODO: BevyDefault for surface texture format is an anti-pattern;
    // the target texture format should be queried from the window when
    // Bevy exposes that functionality.
    let outline_node = OutlineNode::new(&mut render_app.world, TextureFormat::bevy_default());

    graph.add_node(outline::node::MASK_PASS, mask_node);
    graph.add_node(outline::node::JFA_INIT_PASS, JfaInitNode);
    graph.add_node(outline::node::JFA_PASS, jfa_node);
    graph.add_node(outline::node::OUTLINE_PASS, outline_node);

    // Mask -> JFA Init
    graph.add_slot_edge(
        outline::node::MASK_PASS,
        MeshMaskNode::OUT_MASK,
        outline::node::JFA_INIT_PASS,
        JfaInitNode::IN_MASK,
    );

    // JFA Init -> JFA
    graph.add_slot_edge(
        outline::node::JFA_INIT_PASS,
        JfaInitNode::OUT_JFA_INIT,
        outline::node::JFA_PASS,
        JfaNode::IN_BASE,
    );

    // JFA -> Outline
    graph.add_slot_edge(
        outline::node::JFA_PASS,
        JfaNode::OUT_JUMP,
        outline::node::OUTLINE_PASS,
        OutlineNode::IN_JFA,
    );

    Ok(graph)
}

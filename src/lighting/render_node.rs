use std::sync::Mutex;

use bevy::ecs::query::QueryItem;
use bevy::prelude::*;
use bevy::render::render_graph::{NodeRunError, RenderGraphContext, RenderLabel, ViewNode};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, Operations, PipelineCache, RenderPassColorAttachment,
    RenderPassDescriptor, TextureViewId,
};
use bevy::render::renderer::RenderContext;
use bevy::render::view::ViewTarget;

use super::pipeline::LightingPipeline;
use super::uniforms::LightUniformBuffer;

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub struct LightingNodeLabel;

#[derive(Default)]
pub struct LightingNode {
    cached_texture_bind_group: Mutex<Option<(TextureViewId, BindGroup)>>,
}

impl ViewNode for LightingNode {
    type ViewQuery = &'static ViewTarget;

    fn run<'w>(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        view_target: QueryItem<'w, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let pipeline_res = world.resource::<LightingPipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();

        let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_res.pipeline_id) else {
            return Ok(());
        };

        let Some(uniform_buf) = world.get_resource::<LightUniformBuffer>() else {
            return Ok(());
        };
        let Some(uniform_binding) = uniform_buf.buffer.binding() else {
            return Ok(());
        };

        let post_process = view_target.post_process_write();

        // Create or reuse the texture bind group (group 0)
        let source_id = post_process.source.id();
        let mut cached = self.cached_texture_bind_group.lock().unwrap();
        let needs_rebuild = cached.as_ref().is_none_or(|(id, _)| *id != source_id);

        if needs_rebuild {
            let bind_group = render_context.render_device().create_bind_group(
                "lighting_texture_bg",
                &pipeline_res.texture_layout,
                &BindGroupEntries::sequential((
                    post_process.source,
                    &pipeline_res.sampler,
                )),
            );
            *cached = Some((source_id, bind_group));
        }
        let texture_bind_group = &cached.as_ref().unwrap().1;

        // Create the uniform bind group (group 1)
        let uniform_bind_group = render_context.render_device().create_bind_group(
            "lighting_uniform_bg",
            &pipeline_res.uniform_layout,
            &BindGroupEntries::sequential((uniform_binding,)),
        );

        let mut render_pass =
            render_context
                .command_encoder()
                .begin_render_pass(&RenderPassDescriptor {
                    label: Some("lighting_post_pass"),
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: post_process.destination,
                        resolve_target: None,
                        ops: Operations::default(),
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, texture_bind_group, &[]);
        render_pass.set_bind_group(1, &uniform_bind_group, &[]);
        render_pass.draw(0..3, 0..1);

        Ok(())
    }
}

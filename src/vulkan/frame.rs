use super::VulkanApp;
use crate::shader::ShadertoyUniforms;
use ash::vk;

impl VulkanApp {
    //
    // Recorded fresh every frame for the swapchain image that was just
    // acquired. The swapchain cycles through several images; recording
    // once against a single framebuffer would present unrendered images
    // and make the triangle blink.
    //

    /// Records commands for the swapchain image selected by `image_index`.
    ///
    /// A command buffer is not a drawing operation by itself. It is a list of
    /// commands that will later be executed by a Vulkan queue. Here the list
    /// is deliberately small:
    ///
    /// `begin -> begin render pass -> bind pipeline -> draw 3 vertices -> end`
    ///
    /// The framebuffer is selected from the acquired swapchain image index.
    /// For Shadertoy shaders, `shadertoy` carries this frame's uniform
    /// values, pushed before the draw.
    pub(crate) unsafe fn record_command_buffer(
        &self,
        image_index: u32,
        shadertoy: Option<&ShadertoyUniforms>,
    ) {
        unsafe {
            self.context
                .device
                .begin_command_buffer(self.commands.buffer, &vk::CommandBufferBeginInfo::default())
                .expect("begin command buffer");

            self.pipeline.record(
                &self.context.device,
                self.commands.buffer,
                &self.swapchain,
                image_index,
                shadertoy,
            );

            self.context
                .device
                .end_command_buffer(self.commands.buffer)
                .expect("end command buffer");
        }
    }
}

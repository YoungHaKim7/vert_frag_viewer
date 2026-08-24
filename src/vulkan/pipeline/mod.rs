mod compute;
mod graphics;

use super::{device::DeviceBundle, swapchain::SwapchainBundle};
use crate::shader::RenderMode;
use ash::{Device, vk};

/// Mode-specific Vulkan objects created around the compiled shader module.
pub(crate) enum Pipeline {
    /// Classic vertex + fragment rendering through a render pass.
    Graphics(graphics::Graphics),
    /// Playground-style compute pass into an offscreen image that is
    /// blitted to the swapchain.
    Compute(compute::Compute),
}

impl Pipeline {
    /// Creates the shader modules the mode needs and builds the matching
    /// pipeline from them. Modules are only used by pipeline creation, so
    /// they are destroyed before returning.
    pub(crate) unsafe fn new(
        context: &DeviceBundle,
        swapchain: &SwapchainBundle,
        mode: &RenderMode,
    ) -> Self {
        unsafe {
            let device = &context.device;

            match mode {
                RenderMode::Graphics {
                    vertex_spirv,
                    fragment_spirv,
                    vertex_entry,
                    fragment_entry,
                } => {
                    let vertex_module = create_shader_module(device, vertex_spirv);

                    let fragment_module = create_shader_module(device, fragment_spirv);

                    let graphics = graphics::Graphics::new(
                        context,
                        swapchain,
                        vertex_module,
                        fragment_module,
                        vertex_entry,
                        fragment_entry,
                    );

                    device.destroy_shader_module(vertex_module, None);

                    device.destroy_shader_module(fragment_module, None);

                    Self::Graphics(graphics)
                }

                RenderMode::Compute {
                    spirv,
                    entry,
                    group_size,
                    parameters,
                } => {
                    let shader_module = create_shader_module(device, spirv);

                    let compute = compute::Compute::new(
                        context,
                        swapchain,
                        shader_module,
                        entry,
                        group_size,
                        parameters,
                    );

                    device.destroy_shader_module(shader_module, None);

                    Self::Compute(compute)
                }
            }
        }
    }

    /// Appends this pipeline's commands to the command buffer.
    pub(crate) unsafe fn record(
        &self,
        device: &Device,
        command_buffer: vk::CommandBuffer,
        swapchain: &SwapchainBundle,
        image_index: u32,
    ) {
        unsafe {
            match self {
                Self::Graphics(graphics) => {
                    graphics.record(device, command_buffer, swapchain, image_index)
                }
                Self::Compute(compute) => {
                    compute.record(device, command_buffer, swapchain, image_index)
                }
            }
        }
    }

    /// Releases Vulkan resources in dependency-safe reverse order.
    ///
    /// Vulkan does not automatically destroy handles merely because a Rust
    /// variable goes out of scope. Every created Vulkan object must be
    /// explicitly destroyed (or wrapped in an RAII abstraction that performs
    /// the same operation).
    ///
    /// Destruction must respect dependencies. For example, framebuffers use
    /// image views and a render pass, so they are destroyed before those
    /// objects. The device is destroyed only after device-owned resources are
    /// gone, and the instance is destroyed last.
    pub(crate) unsafe fn destroy(&self, device: &Device) {
        unsafe {
            match self {
                Self::Graphics(graphics) => graphics.destroy(device),
                Self::Compute(compute) => compute.destroy(device),
            }
        }
    }

    /// Stage at which the draw submission waits for the acquired image.
    /// Graphics waits before the render pass touches the color attachment;
    /// compute only needs the image by the blit.
    pub(crate) fn wait_stage(&self) -> vk::PipelineStageFlags {
        match self {
            Self::Graphics(_) => vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            Self::Compute(_) => vk::PipelineStageFlags::TRANSFER,
        }
    }
}

fn create_shader_module(device: &Device, spirv: &[u32]) -> vk::ShaderModule {
    let module_info = vk::ShaderModuleCreateInfo::default().code(spirv);

    unsafe {
        device
            .create_shader_module(&module_info, None)
            .expect("shader module")
    }
}

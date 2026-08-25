mod commands;
mod destroy;
mod device;
mod frame;
mod pipeline;
mod swapchain;
mod sync;

use ash::vk;
use commands::Commands;
use device::DeviceBundle;
use pipeline::Pipeline;
use swapchain::SwapchainBundle;
use sync::SyncObjects;
use winit::window::Window;

use crate::shader::{CompiledShader, RenderMode, ShadertoyUniforms};

/// Owns every Vulkan object the viewer creates: device context, swapchain,
/// the mode-specific pipeline (graphics or compute), the reusable command
/// buffer, and synchronization.
///
/// # Vulkan object hierarchy
///
/// The important lifetime relationships are roughly:
///
/// `Entry -> Instance -> PhysicalDevice -> Device -> Queue`
///
/// and, for presentation:
///
/// `Instance -> Surface -> Swapchain -> Images -> ImageViews -> Framebuffers`
///
/// The graphics pipeline depends on the render pass, while command buffers
/// refer to the pipeline and the framebuffer selected for the acquired
/// swapchain image.
///
/// Vulkan handles are generally lightweight, non-owning values. The Rust
/// struct therefore acts as the owner of the corresponding Vulkan resources,
/// and `destroy()` releases them in dependency-safe reverse order.
///
/// Everything the viewer needs to present frames: one bundle per concern,
/// torn down in reverse creation order by destroy().
pub(crate) struct VulkanApp {
    context: DeviceBundle,
    swapchain: SwapchainBundle,
    pipeline: Pipeline,
    commands: Commands,
    sync: SyncObjects,
}

impl VulkanApp {
    /// Creates all Vulkan state needed by the compiled shader's display mode.
    ///
    /// Vulkan exposes a relatively explicit initialization model. In broad
    /// terms this function performs these steps:
    ///
    /// 1. Load the Vulkan loader (`Entry`).
    /// 2. Create a Vulkan `Instance`.
    /// 3. Create a window `Surface` that Vulkan can present to.
    /// 4. Select a physical GPU and a queue family supporting graphics and presentation.
    /// 5. Create a logical `Device` and obtain a graphics queue.
    /// 6. Query surface capabilities and create a `Swapchain`.
    /// 7. Create image views for the swapchain images.
    /// 8. Create a render pass describing the color attachment.
    /// 9. Load Slang-generated SPIR-V and create shader modules.
    /// 10. Build the graphics pipeline.
    /// 11. Create framebuffers, command infrastructure, and synchronization.
    ///
    /// Most Vulkan functions are `unsafe` here because Vulkan's C API cannot
    /// express resource validity, synchronization, or lifetime dependencies
    /// in its type system. The surrounding Rust code establishes those
    /// invariants manually.
    pub(crate) unsafe fn new(window: &Window, compiled: &CompiledShader) -> Self {
        unsafe {
            let context = DeviceBundle::new(window);

            let swapchain = SwapchainBundle::new(&context, window);

            //
            // Pipeline for the compiled shader; shader modules are created
            // (and dropped) inside, since only pipeline creation needs them.
            //

            let pipeline = Pipeline::new(&context, &swapchain, &compiled.mode);

            let commands = Commands::new(&context);

            let sync = SyncObjects::new(&context.device, swapchain.images.len());

            Self {
                context,
                swapchain,
                pipeline,
                commands,
                sync,
            }
        }
    }

    /// Rebuilds every extent-dependent Vulkan object after the window has
    /// been resized.
    ///
    /// The swapchain images and views, the framebuffers, the pipeline
    /// (viewport and scissor are fixed at creation), and — on the compute
    /// path — the offscreen image and its dispatch dimensions are all
    /// sized by the swapchain extent, so they are destroyed and rebuilt
    /// against the window's current size. The device, surface, and command
    /// pool survive; `device_wait_idle()` makes the swap safe.
    pub(crate) unsafe fn recreate(&mut self, window: &Window, mode: &RenderMode) {
        unsafe {
            self.context
                .device
                .device_wait_idle()
                .expect("wait idle before swapchain recreation");

            // Reverse creation order, mirroring destroy(): the pipeline's
            // framebuffers reference the swapchain's image views, so the
            // pipeline is torn down before them.
            self.sync.destroy(&self.context.device);

            self.pipeline.destroy(&self.context.device);

            self.swapchain.destroy(&self.context.device);

            self.swapchain = SwapchainBundle::new(&self.context, window);

            self.pipeline = Pipeline::new(&self.context, &self.swapchain, mode);

            // One render-finished semaphore per swapchain image; the
            // image count can change with the new surface capabilities.
            self.sync = SyncObjects::new(&self.context.device, self.swapchain.images.len());
        }
    }

    /// Executes one complete frame.
    ///
    /// The CPU/GPU sequence is:
    ///
    /// 1. Wait until the previous use of our reusable command buffer is done.
    /// 2. Acquire a swapchain image.
    /// 3. Record commands targeting that image's framebuffer.
    /// 4. Submit those commands to the graphics queue.
    /// 5. Present the same swapchain image after rendering finishes.
    ///
    /// A swapchain that no longer matches the window (`ERROR_OUT_OF_DATE_KHR`)
    /// or is merely no longer optimal (`SUBOPTIMAL_KHR`) is rebuilt inline;
    /// an out-of-date frame is skipped and the next redraw presents at the
    /// new size.
    ///
    /// For Shadertoy shaders, `shadertoy` carries this frame's uniform
    /// values; the swapchain extent is the authoritative `iResolution`.
    /// The semaphores establish GPU-to-GPU ordering; the fence establishes
    /// CPU-to-GPU reuse ordering.
    pub(crate) unsafe fn draw(
        &mut self,
        window: &Window,
        mode: &RenderMode,
        mut shadertoy: Option<ShadertoyUniforms>,
    ) {
        unsafe {
            self.context
                .device
                .wait_for_fences(&[self.sync.in_flight], true, u64::MAX)
                .expect("wait fence");

            self.context
                .device
                .reset_fences(&[self.sync.in_flight])
                .expect("reset fence");

            let (image_index, suboptimal) = match self.swapchain.loader.acquire_next_image(
                self.swapchain.swapchain,
                u64::MAX,
                self.sync.image_available,
                vk::Fence::null(),
            ) {
                Ok(acquired) => acquired,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    self.recreate(window, mode);
                    return;
                }
                Err(error) => panic!("acquire image: {error}"),
            };

            if suboptimal {
                self.recreate(window, mode);
                return;
            }

            if let Some(uniforms) = &mut shadertoy {
                let extent = self.swapchain.extent;

                uniforms.i_resolution = [extent.width as f32, extent.height as f32, 1.0];
            }

            self.record_command_buffer(image_index, shadertoy.as_ref());

            let wait_semaphores = [self.sync.image_available];

            let signal_semaphores = [self.sync.render_finished[image_index as usize]];

            // The semaphore wait is consumed before the color-attachment
            // output stage. In other words, the GPU must not start writing
            // the acquired swapchain image until image acquisition signals
            // `image_available`.
            let wait_stages = [self.pipeline.wait_stage()];

            let command_buffers = [self.commands.buffer];

            let submit_info = vk::SubmitInfo::default()
                .wait_semaphores(&wait_semaphores)
                .wait_dst_stage_mask(&wait_stages)
                .command_buffers(&command_buffers)
                .signal_semaphores(&signal_semaphores);

            self.context
                .device
                .queue_submit(self.context.queue, &[submit_info], self.sync.in_flight)
                .expect("queue submit");

            let swapchains = [self.swapchain.swapchain];

            let image_indices = [image_index];

            let present_info = vk::PresentInfoKHR::default()
                .wait_semaphores(&signal_semaphores)
                .swapchains(&swapchains)
                .image_indices(&image_indices);

            let suboptimal = match self
                .swapchain
                .loader
                .queue_present(self.context.queue, &present_info)
            {
                Ok(suboptimal) => suboptimal,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    self.recreate(window, mode);
                    return;
                }
                Err(error) => panic!("queue present: {error}"),
            };

            if suboptimal {
                self.recreate(window, mode);
            }
        }
    }
}

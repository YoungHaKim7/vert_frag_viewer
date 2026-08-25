//! # Rust + Slang + Vulkan — Documentation Guide
//! - [https://vulkan-tutorial.com/](https://vulkan-tutorial.com/)
//! ## 1. What this program is
//!
//! `vert_frag_viewer` is a small Vulkan viewer for Slang shaders. Given a
//! `.slang` module (a path on the command line, or piped through stdin), a
//! `.vert` + `.frag` pair, or a Shadertoy-style `.glsl` file, it compiles the
//! shader at startup, decides from reflection how to display it, and renders
//! it into a winit window.
//!
//! It is built from:
//!
//! - **winit** — creates the native application window and drives the event loop.
//! - **ash** — Rust bindings for the Vulkan API.
//! - **ash-window** — connects the winit window/display handles to a Vulkan surface.
//! - **slangc** — the Slang compiler, run as an external tool at startup from
//!   `PATH` (it ships with the Vulkan SDK). Slang is *not* a Rust dependency.
//! - **spirv-as** — reassembles `spirv-dis` text dumps back into SPIR-V binaries.
//! - **SPIR-V** — the shader intermediate representation consumed by Vulkan.
//!
//! There is no `build.rs`. Compilation happens once, in `shader::compile`,
//! before any window or Vulkan object exists; the resulting SPIR-V words and
//! entry-point names are all the renderer ever sees.
//!
//! The graphics rendering path is:
//!
//! ```text
//! winit Window
//!     │
//!     ▼
//! Vulkan Surface
//!     │
//!     ▼
//! Swapchain
//!     │
//!     ├── Swapchain Image
//!     │       │
//!     │       ▼
//!     │   Image View
//!     │       │
//!     │       ▼
//!     │   Framebuffer
//!     │
//!     ▼
//! Render Pass
//!     │
//!     ▼
//! Graphics Pipeline
//!     ├── Vertex Shader
//!     └── Fragment Shader
//!     │
//!     ▼
//! Command Buffer
//!     │
//!     ▼
//! Graphics Queue
//!     │
//!     ▼
//! Presentation
//! ```
//!
//! ## 2. How the viewer decides what to display
//!
//! `shader::resolve_input` classifies the command line:
//!
//! - two or more arguments whose extensions (or contents) identify a vertex
//!   and a fragment stage form a `.vert` + `.frag` pair;
//! - a single argument is one `.slang` module (it must be an existing file);
//! - with no arguments and stdin piped (e.g. `viewer < demo.slang`), the
//!   source is dumped to a temp file and treated as one module, because
//!   slangc only reads files;
//! - anything else prints usage and exits.
//!
//! `shader::compile` then produces a `CompiledShader`, and its `RenderMode`
//! selects everything the Vulkan side builds:
//!
//! ```text
//! command line / stdin
//!         │
//!         ▼
//! resolve_input()      one .slang module, or a .vert + .frag pair
//!         │
//!         ▼
//! compile()            slangc / spirv-as / raw .spv -> SPIR-V + reflection JSON
//!         │
//!         ├── module defines mainImage (a Shadertoy export)
//!         │       ──► wrapped as fullscreen GLSL ──► Graphics + push constants
//!         ▼
//! reflection           which entry points and parameters exist?
//!         │
//!         ├── vertex + fragment, no resource parameters ──► Graphics pipeline
//!         └── compute entry point ──────────────────────► Compute + blit
//! ```
//!
//! Two rules shape the supported inputs:
//!
//! - **Graphics** requires a vertex and a fragment entry point *and* zero
//!   resource parameters — the viewer supplies no vertex data, buffers or
//!   textures, so the module must be self-contained (an `SV_VertexID`
//!   triangle, a fullscreen procedural shader, ...).
//! - **Compute** follows the Slang Playground conventions: the kernel writes
//!   pixels through `drawPixel`, and its parameters may only be an
//!   `RWStructuredBuffer<float>` (which the viewer fills with random floats)
//!   and the screen-sized output texture. Anything else is rejected before a
//!   window opens.
//!
//! Playground demos rely on a prelude the web playground injects
//! (`drawPixel`, `outputTexture`, the `[playground::...]` attributes). When a
//! file does not compile on its own, the vendored prelude from
//! `assets/playground/` is written to a scratch directory, the imports are
//! prepended, and the compile is retried — so playground demos run unchanged.
//!
//! ## 3. The Vulkan object model
//!
//! A useful way to understand Vulkan is to separate **creation/discovery objects** from
//! **GPU execution resources**.
//!
//! ### `VkInstance`
//!
//! The instance is the application's connection to the Vulkan implementation.
//!
//! In this program:
//!
//! ```rust,ignore
//! let entry = Entry::load().expect("failed to load Vulkan");
//!
//! let instance = entry
//!     .create_instance(&create_info, None)
//!     .expect("failed to create Vulkan instance");
//! ```
//!
//! `Entry` loads the Vulkan loader. `Instance` is then used to enumerate physical
//! devices, create surfaces, and obtain other instance-level functionality.
//!
//! ### `VkPhysicalDevice`
//!
//! A physical device represents an actual GPU.
//!
//! ```rust,ignore
//! let physical_devices = instance
//!     .enumerate_physical_devices()
//!     .expect("failed to enumerate physical devices");
//! ```
//!
//! The program selects a device whose queue family supports both:
//!
//! ```text
//! GRAPHICS
//! +
//! PRESENT
//! ```
//!
//! That is important because this example submits graphics commands and presents
//! through the same queue.
//!
//! ### `VkDevice`
//!
//! The logical device is the application's interface to the selected physical GPU.
//!
//! ```rust,ignore
//! let device = instance
//!     .create_device(physical_device, &device_create_info, None)
//!     .expect("failed to create logical device");
//! ```
//!
//! A `VkDevice` owns access to device-level resources such as:
//!
//! - queues
//! - images, buffers and device memory
//! - image views
//! - render passes
//! - pipelines and descriptor sets
//! - command pools
//! - semaphores
//! - fences
//! - shader modules
//!
//! ### Queue
//!
//! A queue is where recorded GPU work is submitted.
//!
//! ```rust,ignore
//! let queue = device.get_device_queue(queue_family_index, 0);
//! ```
//!
//! The program later performs:
//!
//! ```rust,ignore
//! device.queue_submit(queue, ...);
//! ```
//!
//! and:
//!
//! ```rust,ignore
//! swapchain_loader.queue_present(queue, ...);
//! ```
//!
//! ## 4. Surface and swapchain
//!
//! A Vulkan surface represents the application's window as a presentation target.
//!
//! The surface itself does not contain the rendered pixels. Instead, the swapchain
//! provides a set of images that can be rendered to and presented.
//!
//! ```rust,ignore
//! let swapchain = swapchain_loader
//!     .create_swapchain(&swapchain_create_info, None)
//!     .expect("failed to create swapchain");
//! ```
//!
//! The configuration is chosen from the surface capabilities:
//!
//! - **format**: `B8G8R8A8_UNORM` when the surface offers it, otherwise the
//!   first reported format;
//! - **extent**: the surface's current extent, falling back to the window's
//!   current size when the window system does not fix one
//!   (`current_extent.width == u32::MAX`), clamped to the surface's
//!   min/max image extent;
//! - **present mode**: `MAILBOX` when available, otherwise the mandatory `FIFO`;
//! - **image count**: `min_image_count + 1`, clamped to `max_image_count`
//!   when the surface enforces a maximum.
//!
//! The swapchain images are created with
//! `COLOR_ATTACHMENT | TRANSFER_DST` usage: the graphics path renders into
//! them through the render pass, and the compute path blits its offscreen
//! result into them.
//!
//! Conceptually:
//!
//! ```text
//! Swapchain
//!    │
//!    ├── Image 0 ── ImageView 0 ── Framebuffer 0
//!    ├── Image 1 ── ImageView 1 ── Framebuffer 1
//!    └── Image 2 ── ImageView 2 ── Framebuffer 2
//! ```
//!
//! ### Why does `acquire_next_image()` return an index?
//!
//! Because Vulkan may give the application any currently available swapchain image:
//!
//! ```rust,ignore
//! let (image_index, _) = swapchain_loader.acquire_next_image(...)?;
//! ```
//!
//! The index is then used to select the matching framebuffer:
//!
//! ```rust,ignore
//! self.framebuffers[image_index as usize]
//! ```
//!
//! This is why the command buffer is recorded after acquiring the image.
//!
//! ## 5. Image vs. image view
//!
//! A Vulkan `Image` is the underlying GPU image resource.
//!
//! An `ImageView` describes how that image is accessed:
//!
//! ```rust,ignore
//! let info = vk::ImageViewCreateInfo::default()
//!     .image(image)
//!     .view_type(vk::ImageViewType::TYPE_2D)
//!     .format(surface_format.format)
//!     .subresource_range(subresource);
//! ```
//!
//! The render pass does not directly receive the raw swapchain image here. The
//! framebuffer contains the image view.
//!
//! ```text
//! VkImage
//!    │
//!    ▼
//! VkImageView
//!    │
//!    ▼
//! VkFramebuffer
//! ```
//!
//! ## 6. Render pass
//!
//! The graphics mode uses one color attachment.
//!
//! The important settings are:
//!
//! ```rust,ignore
//! .load_op(vk::AttachmentLoadOp::CLEAR)
//! .store_op(vk::AttachmentStoreOp::STORE)
//! .initial_layout(vk::ImageLayout::UNDEFINED)
//! .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)
//! ```
//!
//! This means, conceptually:
//!
//! ```text
//! begin render pass
//!     │
//!     ├── discard/initialize old contents
//!     ├── clear the image
//!     ├── draw triangle
//!     └── make image suitable for presentation
//! ```
//!
//! The render pass also contains a subpass:
//!
//! ```rust,ignore
//! .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
//! .color_attachments(&color_refs)
//! ```
//!
//! A subpass describes how the graphics pipeline uses the attachment.
//! The compute mode skips the render pass entirely; it manages layouts with
//! explicit barriers instead (see section 11).
//!
//! ## 7. Shaders: compiled at startup, not build time
//!
//! All shader compilation happens when the program starts, by running the
//! external `slangc` (and, for disassembled inputs, `spirv-as`) from `PATH`.
//! Nothing is embedded with `include_bytes!` and there is no `build.rs`.
//!
//! ### One `.slang` module
//!
//! A single `slangc` invocation compiles the whole file — no `-entry` flag,
//! so every entry point lands in one SPIR-V module — and reflection JSON is
//! requested alongside:
//!
//! ```text
//! slangc demo.slang -target spirv -profile spirv_1_3 \
//!          -fvk-use-entrypoint-name -reflection-json reflection.json -o shader.spv
//! ```
//!
//! - `-profile spirv_1_3` — SPIR-V 1.3 is the newest version Vulkan 1.1 accepts;
//! - `-fvk-use-entrypoint-name` — keeps `vertMain`/`fragMain`/`imageMain`
//!   instead of renaming every entry point to `"main"`;
//! - the reflection JSON is what section 2's display-mode decision reads.
//!
//! If the plain compile fails or yields nothing displayable, the same
//! invocation is retried with the vendored playground prelude: the imports
//! `import playground;` / `import rendering;` are prepended (unless the
//! source already imports them) and the prelude directory is passed with
//! `-I`. When both attempts fail, the diagnostics of the *plain* attempt are
//! shown, because they describe the user's actual file.
//!
//! ### A `.vert` + `.frag` pair
//!
//! Each stage is built on its own. `classify_stage` sniffs the file contents
//! to accept three formats per stage:
//!
//! ```text
//! starts with "; SPIR-V"                ──► spirv-dis text
//! starts with the SPIR-V magic bytes    ──► raw binary
//! anything else                         ──► Slang/GLSL source
//! ```
//!
//! | Format | How it is built | Entry-point name |
//! |---|---|---|
//! | source (`.vert`/`.vs`, `.frag`/`.fs`) | `slangc -stage vertex\|fragment` | from the reflection JSON, else `"main"` |
//! | `spirv-dis` text | `spirv-as --target-env vulkan1.1` (retried unversioned if the module needs newer SPIR-V) | the quoted name on the `OpEntryPoint` line, else `"main"` |
//! | raw SPIR-V binary | loaded as-is | `"main"` (slangc's default without `-fvk-use-entrypoint-name`) |
//!
//! The stage itself comes from the extension; when the extension does not say
//! (a misnamed dump or binary), it is recovered from the module — the
//! `OpEntryPoint Vertex/Fragment` keyword in disassembly, or the execution
//! model of the first entry point in a binary (Vertex = 0, Fragment = 4).
//!
//! ### Shadertoy-style `.glsl`
//!
//! A Shadertoy export is fragment-only GLSL around
//! `void mainImage(out vec4 fragColor, in vec2 fragCoord)`, relying on
//! uniforms (`iTime`, `iResolution`, ...) that Shadertoy's own environment
//! injects. The viewer detects it by the `mainImage(` definition and wraps
//! the file instead of compiling it raw:
//!
//! ```text
//! #version 450                        (only when the export has no #version)
//! layout(push_constant) uniform ShadertoyUniforms { ... iTime, iResolution, ... };
//! #line 1 "<original file>"           (errors keep the user's name and lines)
//! <the export, verbatim>
//! void main() { mainImage(color, bottom-left gl_FragCoord); }
//! ```
//!
//! The wrapped fragment is built with the same `slangc -stage fragment`
//! invocation a `.frag` pair member uses, alongside a viewer-owned GLSL
//! vertex stage that emits a fullscreen triangle from `SV_VertexID`.
//!
//! The built-ins travel as **push constants** — small (here 60 bytes),
//! written straight into the command buffer, and needing no descriptor
//! sets or buffers. The pipeline layout declares one fragment-stage
//! `VkPushConstantRange`, and every frame writes
//! `shader::ShadertoyUniforms` (the `repr(C)` mirror of the GLSL block)
//! with `vkCmdPushConstants` right before the draw. Exports that read
//! `iChannel*` textures or declare their own `uniform`s are rejected up
//! front, because nothing could supply those resources at draw time.
//!
//! The flow for every input is:
//!
//! ```text
//! .slang / .vert+.frag / .spv / disassembly
//!     │
//!     │ slangc / spirv-as / (nothing)
//!     ▼
//! SPIR-V
//!     │
//!     ▼
//! VkShaderModule
//!     │
//!     ▼
//! Graphics / Compute Pipeline
//! ```
//!
//! A shader module contains the compiled shader code. It is needed to create the
//! pipeline, but the viewer destroys the modules immediately after pipeline
//! creation, since nothing else references them:
//!
//! ```rust,ignore
//! device.destroy_shader_module(vertex_module, None);
//! device.destroy_shader_module(fragment_module, None);
//! ```
//!
//! ## 8. Why there is no vertex buffer
//!
//! This is one of the most educational parts of the graphics mode.
//!
//! Normally a Vulkan application might do:
//!
//! ```text
//! vertex buffer
//!     │
//!     ▼
//! vertex input
//!     │
//!     ▼
//! vertex shader
//! ```
//!
//! This example deliberately avoids a vertex buffer.
//!
//! The shader obtains the vertex number through `SV_VertexID`.
//!
//! The Rust side simply submits:
//!
//! ```rust,ignore
//! device.cmd_draw(command_buffer, 3, 1, 0, 0);
//! ```
//!
//! The three vertices have IDs:
//!
//! ```text
//! vertex 0
//! vertex 1
//! vertex 2
//! ```
//!
//! The vertex shader converts those IDs into the triangle's positions.
//!
//! Therefore:
//!
//! ```text
//! CPU:
//!     vkCmdDraw(..., vertexCount = 3, ...)
//!
//! GPU:
//!     vertex ID 0 -> triangle vertex 0
//!     vertex ID 1 -> triangle vertex 1
//!     vertex ID 2 -> triangle vertex 2
//! ```
//!
//! No vertex buffer is required — which is exactly why the viewer can display
//! any self-contained `.slang` module without knowing its vertex layout.
//!
//! ## 9. Graphics pipeline
//!
//! The graphics pipeline combines many fixed rendering decisions.
//!
//! This example configures:
//!
//! ### Vertex input
//!
//! ```rust,ignore
//! let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
//! ```
//!
//! It is empty because there is no vertex buffer.
//!
//! ### Primitive assembly
//!
//! ```rust,ignore
//! .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
//! ```
//!
//! Three vertices form one triangle.
//!
//! ### Viewport
//!
//! ```rust,ignore
//! let viewport = vk::Viewport {
//!     x: 0.0,
//!     y: 0.0,
//!     width: extent.width as f32,
//!     height: extent.height as f32,
//!     min_depth: 0.0,
//!     max_depth: 1.0,
//! };
//! ```
//!
//! The viewport maps normalized/device coordinates to the framebuffer.
//!
//! ### Rasterization
//!
//! ```rust,ignore
//! .polygon_mode(vk::PolygonMode::FILL)
//! .cull_mode(vk::CullModeFlags::NONE)
//! ```
//!
//! The triangle is filled and back-face culling is disabled.
//!
//! ### Multisampling
//!
//! ```rust,ignore
//! .rasterization_samples(vk::SampleCountFlags::TYPE_1)
//! ```
//!
//! This example uses one sample per pixel.
//!
//! ### Color blending
//!
//! ```rust,ignore
//! .blend_enable(false)
//! ```
//!
//! The fragment color is written directly rather than alpha-blended with the
//! existing framebuffer color.
//!
//! ## 10. Command buffers
//!
//! Vulkan separates command recording from command execution.
//!
//! The viewer allocates one primary command buffer from a pool created with
//! `RESET_COMMAND_BUFFER`, so the same buffer can be reset and re-recorded
//! every frame.
//!
//! The program records:
//!
//! ```txt
//! begin command buffer
//!     │
//!     ├── begin render pass        (graphics mode)
//!     ├── bind graphics pipeline   (graphics mode)
//!     ├── draw 3 vertices          (graphics mode)
//!     ├── end render pass          (graphics mode)
//!     │
//!     ├── barriers + dispatch      (compute mode)
//!     ├── barriers + blit          (compute mode)
//!     └── barrier to present       (compute mode)
//! end command buffer
//! ```
//!
//! Only later does the queue execute it:
//!
//! ```rust,ignore
//! device.queue_submit(queue, &[submit_info], fence);
//! ```
//!
//! This separation is one of Vulkan's major design characteristics. It allows
//! applications to prepare GPU work explicitly and submit it efficiently.
//!
//! ## 11. The compute (playground) path
//!
//! A module with a compute entry point is displayed playground-style: the
//! kernel fills an offscreen image, and the image is blitted into the
//! acquired swapchain image. No render pass or framebuffer is involved.
//!
//! ### Resources created at startup
//!
//! - **Offscreen image** — `R8G8B8A8_UNORM`, sized to the swapchain extent,
//!   device-local, used as `STORAGE | TRANSFER_SRC`. `rgba8` matches the
//!   `[format("rgba8")]` on the playground's `outputTexture`; the blit
//!   converts to the swapchain format if it differs.
//! - **Random-float buffer** — when the kernel declares an
//!   `RWStructuredBuffer<float>`, a storage buffer of `[playground::RAND(n)]`
//!   elements (defaulting to 131 072) is allocated in host-visible,
//!   host-coherent memory and filled once with uniform randoms from a
//!   xorshift64* generator — the same way the playground fills its `RAND`
//!   buffers.
//! - **Descriptors** — one binding per reflection parameter, at the binding
//!   index slangc assigned (`STORAGE_BUFFER` for the random buffer,
//!   `STORAGE_IMAGE` for the output texture), collected into a single
//!   descriptor set.
//!
//! ### The per-frame image lifecycle
//!
//! ```text
//! offscreen:  UNDEFINED ──► GENERAL            (compute writes pixels)
//!                     dispatch (one thread group per threadGroupSize tile)
//! offscreen:  GENERAL ──► TRANSFER_SRC_OPTIMAL
//! swapchain:  UNDEFINED ──► TRANSFER_DST_OPTIMAL
//!                     vkCmdBlitImage (LINEAR; scales and converts format)
//! swapchain:  TRANSFER_DST_OPTIMAL ──► PRESENT_SRC_KHR
//! ```
//!
//! Each arrow is an explicit `vkCmdPipelineBarrier`; compute shaders write
//! storage images in the `GENERAL` layout, and presentation requires
//! `PRESENT_SRC_KHR`, so the layout must be walked both ways every frame.
//!
//! The dispatch covers the whole image:
//!
//! ```text
//! group_count.x = ceil(extent.width  / threadGroupSize.x)
//! group_count.y = ceil(extent.height / threadGroupSize.y)
//! group_count.z = 1
//! ```
//!
//! The core 1.0 `vkCmdBlitImage` is used rather than the `*2` variant,
//! which would require Vulkan 1.3 or `KHR_copy_commands2`.
//!
//! ## 12. Synchronization
//!
//! Synchronization is essential in Vulkan because CPU and GPU operations are
//! asynchronous.
//!
//! This example uses:
//!
//! ### Semaphore 1: `image_available`
//!
//! ```rust,ignore
//! image_available
//! ```
//!
//! It signals:
//!
//! > The swapchain image acquired by the application is ready for rendering.
//!
//! The queue waits for this semaphore before a mode-dependent stage — before
//! the color-attachment output stage in graphics mode, before the transfer
//! stage in compute mode (the image is only needed by the blit).
//!
//! ### Semaphores 2: `render_finished[image_index]`
//!
//! There is **one per swapchain image**, not a single one. A present
//! operation's semaphore wait is not covered by the in-flight fence, so with
//! a single semaphore a later frame could signal it again while an earlier
//! present still waits on it. Indexing by the acquired image means each
//! image has its own binary semaphore.
//!
//! Each signals:
//!
//! > Rendering into swapchain image `image_index` has completed.
//!
//! Presentation of that image waits for it.
//!
//! ### Fence: `in_flight`
//!
//! The fence is different because the CPU waits on it:
//!
//! ```rust,ignore
//! device.wait_for_fences(&[self.sync.in_flight], true, u64::MAX);
//! ```
//!
//! It tells the CPU that the previous queue submission has finished, so the
//! single reusable command buffer is no longer in use. It is created in the
//! signaled state (otherwise the very first frame would wait forever) and
//! reset right before each submit.
//!
//! A useful mental model is:
//!
//! ```text
//! CPU
//!  │
//!  │ wait + reset fence
//!  ▼
//! Acquire image ─── signals image_available
//!  │
//!  ▼
//! Record commands
//!  │
//!  ▼
//! Submit ────────────────► GPU queue
//!  │                         │
//!  │ image_available         │
//!  │────────────────────────►│
//!  │                         ▼
//!  │                    Render / dispatch + blit
//!  │                         │
//!  │                         │ render_finished[image_index]
//!  │                         ▼
//!  │                    Presentation
//!  │
//!  └──── in_flight signals when the submission is complete
//! ```
//!
//! ## 13. Why the command buffer is recorded every frame
//!
//! The program has:
//!
//! ```rust,ignore
//! let (image_index, _) = acquire_next_image(...);
//! ```
//!
//! and then:
//!
//! ```rust,ignore
//! self.record_command_buffer(image_index);
//! ```
//!
//! This matters because the acquired image can change.
//!
//! If one command buffer permanently referenced only framebuffer 0, then presenting
//! framebuffer 1 or 2 would not automatically make the commands target that image.
//!
//! The example therefore records the command buffer using the framebuffer associated
//! with the newly acquired image. The command pool is created with
//! `RESET_COMMAND_BUFFER` precisely to allow this reset-and-rerecord cycle with a
//! single buffer.
//!
//! ## 14. Vulkan ownership and Rust ownership
//! - [Ownership Concepts(C++ VS Rust), If you want to learn more, come to my blog.](https://younghakim7.github.io/blog/posts/unique_ptr_vs_shared_ptr_ownership/)
//!   Vulkan itself uses explicit handle lifetime management.
//!
//! For example:
//!
//! ```rust,ignore
//! let image_view = device.create_image_view(...);
//! ```
//!
//! must eventually be paired with:
//!
//! ```rust,ignore
//! device.destroy_image_view(image_view, None);
//! ```
//!
//! Rust does not automatically know that a raw Vulkan handle needs that particular
//! destructor.
//!
//! The viewer groups its handles into bundles, one per concern, and makes
//! `VulkanApp` the practical owner of them:
//!
//! ```rust,ignore
//! struct VulkanApp {
//!     context: DeviceBundle,      // entry, instance, surface, device, queue
//!     swapchain: SwapchainBundle, // swapchain, images, image views, format
//!     pipeline: Pipeline,         // graphics or compute pipeline (+ resources)
//!     commands: Commands,         // command pool + the single buffer
//!     sync: SyncObjects,          // semaphores + fence
//! }
//! ```
//!
//! Each bundle exposes its own `destroy()`, and the top-level `destroy()`
//! calls them in reverse creation order.
//!
//! This resembles RAII in C++:
//!
//! ```cpp
//! class VulkanApp {
//! public:
//!     ~VulkanApp() {
//!         // destroy Vulkan objects
//!     }
//! };
//! ```
//!
//! but this example uses an explicit Rust method rather than relying on `Drop`.
//!
//! A production Rust Vulkan wrapper can instead implement `Drop` or use higher-level
//! RAII-style abstractions so resource cleanup happens automatically.
//!
//! ## 15. Correct destruction order
//!
//! The destruction order is important because Vulkan objects have dependencies.
//!
//! After `device_wait_idle()`, the program performs:
//!
//! ```text
//! wait for GPU
//!     │
//!     ▼
//! sync           semaphores, fence
//!     │
//!     ▼
//! commands       command pool (also frees its buffers)
//!     │
//!     ▼
//! pipeline       graphics: framebuffers, pipeline, pipeline layout, render pass
//!                compute:  pipeline, pipeline layout, descriptor pool,
//!                          descriptor set layout, image view, image, memory,
//!                          random buffer + memory
//!     │
//!     ▼
//! swapchain      image views, swapchain
//!     │
//!     ▼
//! logical device
//!     │
//!     ▼
//! surface
//!     │
//!     ▼
//! instance
//! ```
//!
//! The general rule is:
//!
//! > Destroy dependent objects before the objects they depend on.
//!
//! For example, a framebuffer refers to an image view, so the framebuffer should
//! be destroyed before the image view.
//!
//! Likewise, device-owned resources should be destroyed before destroying the
//! logical device.
//!
//! ## 16. Why `device_wait_idle()` is used before destruction
//!
//! The program starts destruction with:
//!
//! ```rust,ignore
//! self.device.device_wait_idle().unwrap();
//! ```
//!
//! This establishes that submitted GPU work has finished before resources used by
//! that work are destroyed.
//!
//! Without an appropriate synchronization strategy, destroying an object while
//! the GPU is still using it would be invalid.
//!
//! ## 17. Vulkan + Rust safety boundary
//!
//! Most of the Vulkan operations in this example are inside:
//!
//! ```rust,ignore
//! unsafe {
//!     ...
//! }
//! ```
//!
//! This does **not** mean Vulkan is randomly unsafe.
//!
//! It means Rust cannot prove certain invariants from the types alone.
//!
//! Examples include:
//!
//! - a Vulkan handle must refer to a live object;
//! - an image must be used in an appropriate layout;
//! - synchronization must be correct;
//! - a command buffer must be in a valid recording/submission state;
//! - a Vulkan object must outlive resources that depend on it.
//!
//! The programmer is responsible for maintaining these invariants.
//!
//! Rust still provides valuable safety for ordinary program structure, references,
//! ownership of the `VulkanApp`, and other CPU-side data.
//!
//! ## 18. `shaderDrawParameters`
//!
//! The graphics path explicitly enables:
//!
//! ```rust,ignore
//! .shader_draw_parameters(true);
//! ```
//!
//! The reason: slangc declares an (unused) `BuiltIn BaseVertex` input for
//! `SV_VertexID`, which pulls in the `DrawParameters` SPIR-V capability, and
//! that capability is only legal when the `shaderDrawParameters` device
//! feature (Vulkan 1.1) is enabled. The initialization therefore:
//!
//! 1. requires Vulkan 1.1;
//! 2. queries `PhysicalDeviceVulkan11Features`;
//! 3. checks `shader_draw_parameters`;
//! 4. enables the feature.
//!
//! This is a good example of an important Vulkan rule:
//!
//! > A shader capability is not necessarily available merely because the GPU
//! > supports Vulkan. The required device feature must also be enabled.
//!
//! ## 19. Complete per-frame lifecycle
//!
//! The whole frame can be reduced to:
//!
//! ```text
//! wait for previous frame (fence)
//!         │
//!         ▼
//! acquire swapchain image ── signals image_available
//!         │
//!         ▼
//! record command buffer
//!         │
//!         ├── graphics: render pass, bind pipeline, draw 3 vertices
//!         └── compute:  barriers, dispatch, barriers, blit
//!         │
//!         ▼
//! submit to graphics queue
//!         │           waits image_available, signals render_finished[i] + fence
//!         ▼
//! present swapchain image
//!                     waits render_finished[i]
//! ```
//!
//! winit drives this continuously: `about_to_wait` requests a redraw on every
//! event-loop iteration, so the shader animates if it uses time-varying
//! inputs, and the frame is otherwise re-rendered as fast as the present
//! mode allows.
//!
//! That is the central Vulkan rendering loop represented by this program.
//!
//! ## 20. The most important concepts to learn next
//!
//! After understanding this example, the next useful Vulkan topics are:
//!
//! 1. **Vulkan synchronization 2** — modern synchronization APIs.
//! 2. **Image layouts and barriers** — how Vulkan controls image usage transitions
//!    (the compute path already uses them explicitly).
//! 3. **Descriptor sets** — passing buffers/textures/samplers to shaders (used
//!    by the compute path).
//! 4. **Uniform buffers** — passing larger per-frame data. (The Shadertoy
//!    path already passes its small uniform block via **push constants**;
//!    uniform buffers are the scalable, descriptor-based mechanism.)
//! 5. **Vertex buffers** — replacing `SV_VertexID` with explicit vertex data.
//! 6. **Depth buffers** — adding depth testing.
//! 7. **Multiple frames in flight** — increasing CPU/GPU overlap.
//! 8. **Swapchain recreation** — how a resized window drives `OUT_OF_DATE_KHR`
//!    and `SUBOPTIMAL_KHR` handling (the viewer rebuilds the swapchain and the
//!    extent-dependent objects on resize).
//! 9. **Command buffer reuse** — reducing per-frame recording overhead.
//! 10. **Vulkan validation layers** — catching incorrect API usage during development.
//!
//! ## 21. Suggested mental model
//!
//! When reading Vulkan code, ask these five questions:
//!
//! ```text
//! 1. What GPU resource is being created?
//! 2. What object owns or depends on that resource?
//! 3. What state/layout is the resource currently in?
//! 4. What synchronization makes the next operation legal?
//! 5. In what order must the resource be destroyed?
//! ```
//!
//! If those five questions are clear, most Vulkan code becomes substantially easier
//! to understand.

mod app;
pub mod shader;
pub mod vulkan;

pub use app::run;

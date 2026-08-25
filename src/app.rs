use crate::{
    app,
    shader::{self, CompiledShader, RenderMode, ShadertoyUniforms},
    vulkan::VulkanApp,
};

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, WindowEvent},
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes, WindowId},
};

use shader::ParamKind;
use winit::event_loop::EventLoop;

/// Window size; also the swapchain's fallback extent when the surface
/// does not report one.
pub(crate) const WIDTH: u32 = 800;
pub(crate) const HEIGHT: u32 = 600;

/// Creates the event loop and runs the application until the window closes.
pub fn run() {
    let workdir = shader::create_workdir();

    let input = shader::resolve_input(&workdir);

    let title = shader::display_name(&input);

    let compiled = shader::compile(&workdir, input);

    // The viewer can only supply random buffers and the output texture;
    // reject anything else before any window or device exists.
    if let RenderMode::Compute { parameters, .. } = &compiled.mode {
        for param in parameters {
            if let ParamKind::Unsupported(what) = &param.kind {
                eprintln!(
                    "error: parameter '{}' is {what}; the viewer can only supply \
                     random float buffers and the output texture",
                    param.name
                );

                std::process::exit(1);
            }
        }
    }

    let event_loop = EventLoop::new().expect("event loop");

    let mut app = app::App::new(title, compiled);

    let result = event_loop.run_app(&mut app);

    // Scratch files are no longer needed once the app is done.
    let _ = std::fs::remove_dir_all(workdir);

    result.expect("event loop error");
}

/// Small winit application state.
///
/// `window` must remain alive while the Vulkan surface is being used. The
/// `VulkanApp` is therefore kept alongside the window rather than creating
/// and immediately dropping the window after initialization.
pub(crate) struct App {
    window: Option<Window>,
    vulkan: Option<VulkanApp>,

    /// File name shown in the window title.
    shader_name: String,
    compiled: Option<CompiledShader>,

    /// CPU-side state behind the Shadertoy uniforms; only present when the
    /// viewed shader is Shadertoy-style GLSL.
    shadertoy: Option<ShadertoyClock>,
}

impl App {
    pub(crate) fn new(shader_name: String, compiled: CompiledShader) -> Self {
        let shadertoy = if matches!(
            compiled.mode,
            RenderMode::Graphics {
                shadertoy: true,
                ..
            }
        ) {
            Some(ShadertoyClock::new())
        } else {
            None
        };

        Self {
            window: None,
            vulkan: None,
            shader_name,
            compiled: Some(compiled),
            shadertoy,
        }
    }
}

/// CPU-side state feeding the Shadertoy push-constant block every frame:
/// the animation clock, the frame counter, and the tracked mouse.
struct ShadertoyClock {
    start: Instant,
    last_frame: Instant,
    frame: i32,

    /// xy: last cursor position, zw: last click (pixels, origin at the
    /// bottom-left corner like Shadertoy).
    mouse: [f32; 4],
}

impl ShadertoyClock {
    fn new() -> Self {
        let now = Instant::now();

        Self {
            start: now,
            last_frame: now,
            frame: 0,
            mouse: [0.0; 4],
        }
    }

    /// Advances the clock by one frame and fills every uniform except
    /// `i_resolution`, which is written from the swapchain extent at draw
    /// time (the authoritative pixel size of the output).
    fn tick(&mut self) -> ShadertoyUniforms {
        let now = Instant::now();

        let time = now.duration_since(self.start).as_secs_f32();

        // Guarded so the first frame (and any 0 s gap) does not divide by
        // zero when deriving the frame rate.
        let delta = now.duration_since(self.last_frame).as_secs_f32().max(1e-6);

        self.last_frame = now;

        let frame = self.frame;

        self.frame += 1;

        ShadertoyUniforms {
            i_resolution: [1.0; 3],
            i_time: time,
            i_mouse: self.mouse,
            i_date: date_now(),
            i_time_delta: delta,
            i_frame_rate: 1.0 / delta,
            i_frame: frame,
        }
    }

    /// Records the cursor position; `y` already flipped to Shadertoy's
    /// bottom-left origin by the caller.
    fn move_mouse(&mut self, x: f32, y: f32) {
        self.mouse[0] = x;
        self.mouse[1] = y;
    }

    /// Latches the current cursor position as the last click.
    fn click(&mut self) {
        self.mouse[2] = self.mouse[0];
        self.mouse[3] = self.mouse[1];
    }
}

/// The current UTC date as Shadertoy's `iDate` vector:
/// (year, month, day, seconds into the day).
fn date_now() -> [f32; 4] {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since_epoch| since_epoch.as_secs() as i64)
        .unwrap_or(0);

    let days = seconds.div_euclid(86_400);

    let time_of_day = seconds.rem_euclid(86_400);

    let (year, month, day) = civil_from_days(days);

    [year as f32, month as f32, day as f32, time_of_day as f32]
}

/// Converts days since the Unix epoch into a (year, month, day) triple —
/// Howard Hinnant's `civil_from_days` algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;

    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;

    let day_of_era = z - era * 146_097;

    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;

    let year = year_of_era + era * 400;

    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);

    let month_pre = (5 * day_of_year + 2) / 153;

    let day = (day_of_year - (153 * month_pre + 2) / 5 + 1) as u32;

    let month = if month_pre < 10 {
        month_pre + 3
    } else {
        month_pre - 9
    } as u32;

    (if month <= 2 { year + 1 } else { year }, month, day)
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = WindowAttributes::default()
            .with_title(format!("Slang Viewer — {}", self.shader_name))
            .with_inner_size(LogicalSize::new(WIDTH, HEIGHT))
            // The viewer does not recreate the swapchain on resize yet.
            .with_resizable(false);

        let window = event_loop.create_window(attributes).expect("window");

        let compiled = self
            .compiled
            .as_ref()
            .expect("shader must be compiled before the window opens");

        let vulkan = unsafe { VulkanApp::new(&window, compiled) };

        self.window = Some(window);
        self.vulkan = Some(vulkan);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                // take() clears the field: winit still delivers a pending
                // RedrawRequested after this handler on X11, and it must
                // not touch the destroyed Vulkan objects.
                if let Some(vulkan) = self.vulkan.take() {
                    unsafe {
                        vulkan.destroy();
                    }
                }

                event_loop.exit();
            }

            WindowEvent::CursorMoved { position, .. } => {
                // winit reports top-left pixel coordinates; Shadertoy's
                // iMouse counts from the bottom-left, so flip y.
                if let (Some(clock), Some(window)) = (&mut self.shadertoy, &self.window) {
                    let height = window.inner_size().height as f32;

                    clock.move_mouse(position.x as f32, height - position.y as f32);
                }
            }

            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            } => {
                if let Some(clock) = &mut self.shadertoy {
                    clock.click();
                }
            }

            WindowEvent::RedrawRequested => {
                if let Some(vulkan) = &self.vulkan {
                    let shadertoy = self.shadertoy.as_mut().map(|clock| clock.tick());

                    unsafe {
                        vulkan.draw(shadertoy);
                    }
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

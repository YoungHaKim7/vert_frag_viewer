# `vert` & `frag` shader viewer

<br />

<div align="center">
  <!-- Crates version -->
  <a href="https://crates.io/crates/vert_frag_viewer">
    <img src="https://img.shields.io/crates/v/vert_frag_viewer.svg?style=flat-square"
    alt="Crates.io version" />
  </a>
  <!-- Downloads -->
  <a href="https://crates.io/crates/vert_frag_viewer">
    <img src="https://img.shields.io/crates/d/vert_frag_viewer.svg?style=flat-square"
      alt="Download" />
  </a>
  <!-- docs.rs docs -->
  <a href="https://docs.rs/vert_frag_viewer">
    <img src="https://img.shields.io/badge/docs-latest-blue.svg?style=flat-square"
      alt="docs.rs docs" />
  </a>
</div>

<br />

# vert_frag_viewer

A small Vulkan viewer for [Slang](https://shader-slang.org/) shaders, written
in Rust with **winit** + **ash**. It compiles a shader at startup with
`slangc`, picks a display mode from the emitted reflection, and renders it
into a window.

Two display modes are supported:

- **Graphics** — a `.slang` module (or a `.vert` + `.frag` pair) with vertex +
  fragment entry points and no resource parameters. The viewer draws 3
  vertices; the shader positions them with `SV_VertexID`, so no vertex buffer
  is needed.
- **Shadertoy GLSL** — a single `.glsl` file written against Shadertoy's
  `mainImage(fragColor, fragCoord)` convention. The viewer wraps it with the
  built-in uniforms (`iTime`, `iResolution`, `iMouse`, `iDate`, ...) and a
  fullscreen-triangle vertex stage, then feeds the uniforms every frame
  through push constants.
- **Compute** — a playground-style kernel (`[shader("compute")]`) that writes
  pixels through the Slang Playground's `drawPixel`. The viewer supplies the
  screen-sized output texture and fills any
  `RWStructuredBuffer<float>` (e.g. `[playground::RAND(n)]`) with random
  floats. The result is blitted to the window.

# Requirements

- A GPU and driver with **Vulkan 1.1** (the graphics path enables the
  `shaderDrawParameters` feature).
- **`slangc`** on `PATH` (ships with the Vulkan SDK, `x86_64/bin/slangc`).
- **`spirv-as`** on `PATH` — only needed for `spirv-dis` text inputs
  (`x86_64/bin/spirv-as` from the SDK's SPIR-V tools).

# Run

```bash
# one .slang module (graphics or compute, decided by reflection)
cargo r --release ./assets/slang_lang/triangle.slang

# vertex + fragment pair
cargo r --release ./assets/triangle.vert ./assets/triangle.frag

# a Shadertoy-style GLSL export (animated fullscreen shader)
cargo r --release ./assets/glsl_lang/circle.glsl

# playground-style compute demo (2D gaussian splatter)
cargo r --release ./assets/slang_lang/2d_splatter.slang

# a shader piped through stdin
cat demo.slang | cargo r --release
```

# Supported inputs

| Input | Formats | How it is built |
|---|---|---|
| `.slang` module (path or stdin) | Slang source | one `slangc` invocation for all entry points; if it fails or has nothing displayable, retried with the vendored playground prelude (`import playground; import rendering;`) |
| Shadertoy `.glsl` (path or stdin) | GLSL source defining `mainImage` | wrapped with the built-in uniforms and a `main()` entry point, compiled as the fragment stage of a fullscreen triangle; the uniforms are fed as push constants every frame |
| `.vert` + `.frag` pair | Slang/GLSL source | `slangc -stage vertex` / `-stage fragment` |
| | `spirv-dis` text (`; SPIR-V` header) | reassembled with `spirv-as` |
| | raw SPIR-V binary (`.spv`) | loaded as-is (entry point assumed `main`) |

Stage pairing works by extension (`.vert`/`.vs`, `.frag`/`.fs`), or — for
misnamed files — by sniffing the `OpEntryPoint` stage from the module.

# Notes

- The window opens at 1600×1200 and is freely resizable (the swapchain and the
  extent-dependent objects are rebuilt on resize).
- Rendering runs continuously (a redraw is requested on every event-loop turn).
- The window title shows the viewed file name(s).
- Compilation happens before the window opens; the scratch directory is
  removed when the app exits.
- For the full Vulkan walkthrough (object model, synchronization, destruction
  order, the compute/blit path), see the crate-level documentation:
  `cargo doc --open`.

# slang viewer
- https://github.com/YoungHaKim7/slang_files_viewer_shaders

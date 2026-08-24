# file sample
- https://github.com/shader-slang/slang/blob/master/examples/model-viewer/shaders.slang

# slang playground
- https://shader-slang.org/slang-playground/
- https://shader-slang.org/docs/first-slang-shader

# drawing ex)
- https://vulkan.lunarg.com/doc/view/1.4.321.0/mac/antora/tutorial/latest/03_Drawing_a_triangle/02_Graphics_pipeline_basics/01_Shader_modules.html

# slang -> `frag` & `vert`

```bash
$ slangc triangle.slang \
      -entry fragMain \
      -stage fragment \
      -target spirv \
      -o frag.spv

$ slangc triangle.slang \
      -entry vertMain \
      -stage vertex \
      -target spirv \
      -o vert.spv


$ spirv-dis frag.spv -o triangle.frag

$ spirv-dis vert.spv -o triangle.vert

```

# view with this crate

The viewer accepts every artifact of the workflow above directly, plus the
plain sources and raw binaries:

```bash
# spirv-dis text pair (as produced above)
cargo r --release ./assets/triangle.vert ./assets/triangle.frag

# or the raw SPIR-V binaries
cargo r --release frag.spv vert.spv
```

Note: the commands above run slangc *without* `-fvk-use-entrypoint-name`, so
the entry points are named `"main"` in the disassembly — the viewer reads the
quoted name from the `OpEntryPoint` line and handles both cases.


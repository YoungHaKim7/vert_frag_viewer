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


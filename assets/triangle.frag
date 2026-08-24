; SPIR-V
; Version: 1.5
; Generator: Khronos Slang Compiler; 0
; Bound: 17
; Schema: 0
               OpCapability Shader
               OpMemoryModel Logical GLSL450
               OpEntryPoint Fragment %fragMain "main" %entryPointParam_fragMain %input_color
               OpExecutionMode %fragMain OriginUpperLeft
               OpSource Slang 1
               OpName %input_color "input.color"
               OpName %entryPointParam_fragMain "entryPointParam_fragMain"
               OpName %fragMain "fragMain"
               OpDecorate %input_color Location 0
               OpDecorate %entryPointParam_fragMain Location 0
       %void = OpTypeVoid
          %3 = OpTypeFunction %void
      %float = OpTypeFloat 32
    %v3float = OpTypeVector %float 3
%_ptr_Input_v3float = OpTypePointer Input %v3float
    %v4float = OpTypeVector %float 4
    %float_1 = OpConstant %float 1
%_ptr_Output_v4float = OpTypePointer Output %v4float
%input_color = OpVariable %_ptr_Input_v3float Input
%entryPointParam_fragMain = OpVariable %_ptr_Output_v4float Output
   %fragMain = OpFunction %void None %3
          %4 = OpLabel
          %7 = OpLoad %v3float %input_color
         %11 = OpCompositeConstruct %v4float %7 %float_1
               OpStore %entryPointParam_fragMain %11
               OpReturn
               OpFunctionEnd

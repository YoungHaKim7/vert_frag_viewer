; SPIR-V
; Version: 1.5
; Generator: Khronos Slang Compiler; 0
; Bound: 68
; Schema: 0
               OpCapability DrawParameters
               OpCapability Shader
               OpMemoryModel Logical GLSL450
               OpEntryPoint Vertex %vertMain "main" %gl_Position %entryPointParam_vertMain_color %gl_VertexIndex %25
               OpSource Slang 1
               OpName %entryPointParam_vertMain_color "entryPointParam_vertMain.color"
               OpName %vertMain "vertMain"
               OpDecorate %25 BuiltIn BaseVertex
               OpDecorate %gl_VertexIndex BuiltIn VertexIndex
               OpDecorate %gl_Position BuiltIn Position
               OpDecorate %entryPointParam_vertMain_color Location 0
       %void = OpTypeVoid
          %3 = OpTypeFunction %void
      %float = OpTypeFloat 32
    %v4float = OpTypeVector %float 4
    %v3float = OpTypeVector %float 3
    %v2float = OpTypeVector %float 2
        %int = OpTypeInt 32 1
      %int_3 = OpConstant %int 3
%_arr_v2float_int_3 = OpTypeArray %v2float %int_3
%_ptr_Function__arr_v2float_int_3 = OpTypePointer Function %_arr_v2float_int_3
%_arr_v3float_int_3 = OpTypeArray %v3float %int_3
%_ptr_Function__arr_v3float_int_3 = OpTypePointer Function %_arr_v3float_int_3
%_ptr_Input_int = OpTypePointer Input %int
       %uint = OpTypeInt 32 0
 %float_n0_5 = OpConstant %float -0.5
         %32 = OpConstantComposite %v2float %float_n0_5 %float_n0_5
  %float_0_5 = OpConstant %float 0.5
         %34 = OpConstantComposite %v2float %float_0_5 %float_n0_5
    %float_0 = OpConstant %float 0
         %36 = OpConstantComposite %v2float %float_0 %float_0_5
         %31 = OpConstantComposite %_arr_v2float_int_3 %32 %34 %36
%_ptr_Function_v2float = OpTypePointer Function %v2float
    %float_1 = OpConstant %float 1
%_ptr_Function_v3float = OpTypePointer Function %v3float
         %53 = OpConstantComposite %v3float %float_1 %float_0 %float_0
         %54 = OpConstantComposite %v3float %float_0 %float_1 %float_0
         %55 = OpConstantComposite %v3float %float_0 %float_0 %float_1
         %52 = OpConstantComposite %_arr_v3float_int_3 %53 %54 %55
%_ptr_Output_v4float = OpTypePointer Output %v4float
%_ptr_Output_v3float = OpTypePointer Output %v3float
         %25 = OpVariable %_ptr_Input_int Input
%gl_VertexIndex = OpVariable %_ptr_Input_int Input
%gl_Position = OpVariable %_ptr_Output_v4float Output
%entryPointParam_vertMain_color = OpVariable %_ptr_Output_v3float Output
   %vertMain = OpFunction %void None %3
          %4 = OpLabel
         %16 = OpVariable %_ptr_Function__arr_v2float_int_3 Function
         %19 = OpVariable %_ptr_Function__arr_v3float_int_3 Function
         %23 = OpLoad %int %25
         %26 = OpLoad %int %gl_VertexIndex
         %28 = OpISub %int %26 %23
         %30 = OpBitcast %uint %28
               OpStore %16 %31
         %40 = OpAccessChain %_ptr_Function_v2float %16 %30
         %41 = OpLoad %v2float %40
         %42 = OpCompositeConstruct %v4float %41 %float_0 %float_1
         %48 = OpLoad %int %25
         %49 = OpLoad %int %gl_VertexIndex
         %50 = OpISub %int %49 %48
         %51 = OpBitcast %uint %50
               OpStore %19 %52
         %57 = OpAccessChain %_ptr_Function_v3float %19 %51
         %58 = OpLoad %v3float %57
               OpStore %gl_Position %42
               OpStore %entryPointParam_vertMain_color %58
               OpReturn
               OpFunctionEnd

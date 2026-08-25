//! Runtime compilation of user-supplied shaders.
//!
//! The viewer accepts either:
//!
//! - one `.slang` module (path or stdin): compiled as a whole in one
//!   `slangc` invocation, and inspected through the emitted reflection JSON
//!   to decide how to display it (vertex + fragment -> graphics pipeline,
//!   compute entry point -> playground-style compute pass);
//! - a Shadertoy-style `.glsl` file (any text module defining `mainImage`):
//!   wrapped with the Shadertoy built-in uniforms and a `main()` entry
//!   point, and rendered as a fullscreen pass fed by push constants;
//! - a `.vert` + `.frag` pair: each stage is built on its own. SPIR-V
//!   disassembly (the output of `spirv-dis`, e.g. from the slangc workflow
//!   in assets/README.md) is assembled with `spirv-as`; plain slang/GLSL
//!   source is compiled by `slangc -stage`. Raw `.spv` binaries load as-is.
//!
//! Playground demos (e.g. the 2D gaussian splatter) rely on a prelude that
//! the web playground injects (`drawPixel`, the screen-sized output texture,
//! the `[playground::...]` attributes). When a file does not compile on its
//! own, the vendored prelude in `assets/playground/` is written next to it
//! and the compile is retried with the matching imports prepended.

use serde_json::Value;
use std::{
    env, fs,
    io::{IsTerminal, Read},
    path::{Path, PathBuf},
    process::Command,
};

const PLAYGROUND_PRELUDE: &str = include_str!("../assets/playground/playground.slang");

const RENDERING_PRELUDE: &str = include_str!("../assets/playground/rendering.slang");

/// Default element count for an unattributed `RWStructuredBuffer<float>`.
///
/// The playground's gaussian-splat demo reads `[playground::RAND(131072)]`;
/// local copies of the demo usually have the attribute stripped, so the
/// viewer fills any unattributed float buffer with this many randoms.
pub const DEFAULT_RAND_COUNT: u32 = 131_072;

/// The .slang file to view: either a path given on the command line or the
/// contents of stdin dumped to disk (slangc only reads files).
pub struct SourceFile {
    /// Shown in the window title.
    pub display_name: String,
    /// Path of the source on disk.
    pub path: PathBuf,
}

/// What the user asked to view on the command line.
pub enum ShaderInput {
    /// One .slang module compiled as a whole.
    Module(SourceFile),
    /// Separate vertex + fragment files built stage by stage.
    StagePair(StagePair),
}

/// A vertex + fragment file pair.
pub struct StagePair {
    /// Both file names, shown in the window title.
    pub display_name: String,
    pub vertex: PathBuf,
    pub fragment: PathBuf,
}

/// The two graphics stages a `.vert`/`.frag` pair can supply.
#[derive(Clone, Copy)]
enum Stage {
    Vertex,
    Fragment,
}

impl Stage {
    fn slang_flag(self) -> &'static str {
        match self {
            Stage::Vertex => "vertex",
            Stage::Fragment => "fragment",
        }
    }

    /// Entry-point keyword used in SPIR-V disassembly
    /// (`OpEntryPoint Vertex ...` / `OpEntryPoint Fragment ...`).
    fn disasm_keyword(self) -> &'static str {
        match self {
            Stage::Vertex => "OpEntryPoint Vertex ",
            Stage::Fragment => "OpEntryPoint Fragment ",
        }
    }
}

/// How a compiled module is displayed.
pub enum RenderMode {
    /// Classic vertex + fragment rendering through the render pass. The
    /// stages may come from one module or from separate per-stage binaries.
    Graphics {
        vertex_spirv: Vec<u32>,
        fragment_spirv: Vec<u32>,
        vertex_entry: String,
        fragment_entry: String,
        /// Shadertoy mode: the fragment stage reads the Shadertoy
        /// built-ins (`iTime`, `iResolution`, ...) from the push-constant
        /// block the viewer's wrapper declares, and the viewer feeds that
        /// block every frame.
        shadertoy: bool,
    },
    /// Compute kernel writing pixels through the playground's `drawPixel`.
    Compute {
        spirv: Vec<u32>,
        entry: String,
        group_size: [u32; 3],
        parameters: Vec<ShaderParam>,
    },
}

/// The per-frame values of the Shadertoy built-ins, laid out exactly like
/// the std140 push-constant block the wrapper GLSL declares (field order
/// must not drift from [`SHADERTOY_UNIFORM_BLOCK`]).
///
/// [`i_resolution`](Self::i_resolution) is filled in at draw time from the
/// swapchain extent; the rest come from the frame clock in `app.rs`.
#[repr(C)]
pub struct ShadertoyUniforms {
    pub i_resolution: [f32; 3],
    pub i_time: f32,
    pub i_mouse: [f32; 4],
    pub i_date: [f32; 4],
    pub i_time_delta: f32,
    pub i_frame_rate: f32,
    pub i_frame: i32,
}

impl ShadertoyUniforms {
    /// The bytes to hand to `vkCmdPushConstants`. `repr(C)` with only
    /// 4-byte-aligned fields produces exactly the std140 layout slangc
    /// emits (offsets 0/12/16/32/48/52/56); the size assert below turns
    /// any accidental layout change into a compile error.
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                self as *const Self as *const u8,
                std::mem::size_of::<Self>(),
            )
        }
    }
}

const _: () = assert!(std::mem::size_of::<ShadertoyUniforms>() == 60);

/// A module-level shader parameter the viewer must bind.
pub struct ShaderParam {
    pub name: String,
    pub binding: u32,
    pub kind: ParamKind,
    /// Element count from `[playground::RAND(n)]`, if present.
    pub rand_count: Option<u32>,
}

pub enum ParamKind {
    /// `RWStructuredBuffer<float>` filled with uniform randoms.
    RandomFloatBuffer,
    /// Screen-sized storage image the kernel writes to.
    OutputTexture,
    /// Anything the viewer does not know how to supply.
    Unsupported(String),
}

pub struct CompiledShader {
    pub mode: RenderMode,
}

/// Per-run scratch directory for the prelude files, the stdin dump and the
/// compiler outputs. Lives for the whole process; slangc and the SPIR-V
/// read happen up front.
pub fn create_workdir() -> PathBuf {
    let dir = env::temp_dir().join(format!("slang-viewer-{}", std::process::id()));

    fs::create_dir_all(&dir).expect("create temp workdir");

    dir
}

/// Resolves what to view: a `.vert` + `.frag` pair when both are named on
/// the command line, else the first argument as one `.slang` module, else
/// stdin when it is piped (e.g. `viewer < demo.slang`), else usage.
pub fn resolve_input(workdir: &Path) -> ShaderInput {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.len() >= 2 {
        let mut vertex = None;

        let mut fragment = None;

        for arg in &args {
            match classify_stage(Path::new(arg)) {
                Some((Stage::Vertex, _)) => vertex = Some(PathBuf::from(arg)),
                Some((Stage::Fragment, _)) => fragment = Some(PathBuf::from(arg)),
                None => {}
            }
        }

        if let (Some(vertex), Some(fragment)) = (vertex, fragment) {
            return ShaderInput::StagePair(StagePair {
                display_name: format!("{} + {}", file_name(&vertex), file_name(&fragment)),
                vertex,
                fragment,
            });
        }
    }

    if let Some(arg) = args.first() {
        let path = PathBuf::from(arg);

        if !path.is_file() {
            eprintln!("error: no such file: {arg}");

            std::process::exit(2);
        }

        return ShaderInput::Module(SourceFile {
            display_name: file_name(&path),
            path,
        });
    }

    if !std::io::stdin().is_terminal() {
        let mut source = String::new();

        std::io::stdin()
            .read_to_string(&mut source)
            .expect("read shader from stdin");

        if source.trim().is_empty() {
            eprintln!("error: no shader source received on stdin");

            std::process::exit(2);
        }

        let path = workdir.join("stdin.slang");

        fs::write(&path, source).expect("write stdin shader to temp file");

        return ShaderInput::Module(SourceFile {
            display_name: "stdin".to_string(),
            path,
        });
    }

    eprintln!("usage: slang_files_viewer_shaders <path/to/shader.slang>");
    eprintln!("       slang_files_viewer_shaders <vertex.vert> <fragment.frag>");
    eprintln!("       slang_files_viewer_shaders <shadertoy.glsl>  (mainImage-style GLSL)");
    eprintln!("       cat shader.slang | slang_files_viewer_shaders");

    std::process::exit(2);
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Classifies a pair member's stage and file format.
///
/// Extensions decide when they can (.vert/.vs, .frag/.fs); otherwise the
/// content is sniffed, so `.spv` binaries and misnamed `spirv-dis` dumps
/// still form valid pairs.
fn classify_stage(path: &Path) -> Option<(Stage, StageFormat)> {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    let from_ext = match ext.as_deref() {
        Some("vert" | "vs") => Some(Stage::Vertex),
        Some("frag" | "fs") => Some(Stage::Fragment),
        _ => None,
    };

    let bytes = fs::read(path).ok()?;

    if bytes.starts_with(b"; SPIR-V") {
        let text = String::from_utf8_lossy(&bytes);

        let stage = from_ext.or_else(|| disasm_stage(&text))?;

        Some((stage, StageFormat::Disassembly))
    } else if bytes.starts_with(&SPIRV_MAGIC_LE) {
        let stage = from_ext.or_else(|| binary_stage(&bytes))?;

        Some((stage, StageFormat::Binary))
    } else {
        from_ext.map(|stage| (stage, StageFormat::Source))
    }
}

const SPIRV_MAGIC_LE: [u8; 4] = 0x0723_0203u32.to_le_bytes();

/// Stage of a disassembled module, from its OpEntryPoint keyword.
fn disasm_stage(disasm: &str) -> Option<Stage> {
    if disasm.contains("OpEntryPoint Vertex ") {
        Some(Stage::Vertex)
    } else if disasm.contains("OpEntryPoint Fragment ") {
        Some(Stage::Fragment)
    } else {
        None
    }
}

/// Stage of a raw SPIR-V binary, from the execution model of its first
/// OpEntryPoint instruction (Vertex = 0, Fragment = 4).
fn binary_stage(bytes: &[u8]) -> Option<Stage> {
    let words: Vec<u32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_le_bytes(*word))
        .collect();

    let mut offset = 5;

    while offset < words.len() {
        let word_count = (words[offset] >> 16) as usize;

        if word_count == 0 {
            return None;
        }

        if words[offset] & 0xFFFF == 15 && offset + 2 < words.len() {
            match words[offset + 1] {
                0 => return Some(Stage::Vertex),
                4 => return Some(Stage::Fragment),
                _ => {}
            }
        }

        offset += word_count;
    }

    None
}

/// The three forms a per-stage file can take.
enum StageFormat {
    /// `spirv-dis` text output; assembled back to binary with `spirv-as`.
    Disassembly,
    /// Raw SPIR-V binary; loaded directly.
    Binary,
    /// Slang/GLSL source compiled by slangc for one stage.
    Source,
}

/// The name shown in the window title for the resolved input.
pub fn display_name(input: &ShaderInput) -> String {
    match input {
        ShaderInput::Module(source) => source.display_name.clone(),
        ShaderInput::StagePair(pair) => pair.display_name.clone(),
    }
}

/// Compiles whatever was resolved from the command line / stdin.
///
/// Exits the process with diagnostics on any user-facing failure.
pub fn compile(workdir: &Path, input: ShaderInput) -> CompiledShader {
    match input {
        ShaderInput::Module(source) => compile_module(workdir, &source),
        ShaderInput::StagePair(pair) => compile_pair(&pair),
    }
}

/// Builds a `.vert`/`.frag` pair stage by stage into separate SPIR-V binaries.
fn compile_pair(pair: &StagePair) -> CompiledShader {
    let vertex = build_stage(pair.vertex.as_path(), Stage::Vertex);

    let fragment = build_stage(pair.fragment.as_path(), Stage::Fragment);

    CompiledShader {
        mode: RenderMode::Graphics {
            vertex_spirv: vertex.spirv,
            fragment_spirv: fragment.spirv,
            vertex_entry: vertex.entry,
            fragment_entry: fragment.entry,
            shadertoy: false,
        },
    }
}

/// One compiled graphics stage: its SPIR-V and entry-point name.
struct BuiltStage {
    spirv: Vec<u32>,
    entry: String,
}

fn build_stage(path: &Path, stage: Stage) -> BuiltStage {
    let (_, format) = match classify_stage(path) {
        Some(classified) => classified,
        None => {
            eprintln!(
                "error: cannot tell the stage of {}; expected .vert/.vs or .frag/.fs",
                path.display()
            );

            std::process::exit(2);
        }
    };

    match format {
        StageFormat::Binary => {
            let words = read_spirv(path);

            // Raw binaries carry no readable name; slangc defaults to
            // "main" unless -fvk-use-entrypoint-name was used at build time.
            BuiltStage {
                spirv: words,
                entry: "main".to_string(),
            }
        }

        StageFormat::Disassembly => {
            let text = fs::read_to_string(path).unwrap_or_else(|err| {
                eprintln!("error: cannot read {}: {err}", path.display());

                std::process::exit(1);
            });

            let entry = parse_disasm_entry_point(&text, stage).unwrap_or_else(|| "main".into());

            let spirv = assemble_disassembly(path);

            BuiltStage { spirv, entry }
        }

        StageFormat::Source => compile_stage_source(path, stage),
    }
}

/// Assembles `spirv-dis` text back to a SPIR-V binary.
///
/// Vulkan 1.1 accepts SPIR-V up to 1.3, so the target environment is pinned;
/// if the module needs something newer the assembler is retried unversioned.
fn assemble_disassembly(path: &Path) -> Vec<u32> {
    let workdir = create_workdir();

    let out = workdir.join("stage.spv");

    let pinned = Command::new("spirv-as")
        .arg(path)
        .arg("-o")
        .arg(&out)
        .arg("--target-env")
        .arg("vulkan1.1")
        .output();

    let output = match pinned {
        Ok(output) if !output.status.success() => Command::new("spirv-as")
            .arg(path)
            .arg("-o")
            .arg(&out)
            .output()
            .expect("run spirv-as"),
        Ok(output) => output,
        Err(_) => {
            eprintln!("error: spirv-as not found on PATH");

            eprintln!("       it ships with the Vulkan SDK (x86_64/bin/spirv-as)");

            std::process::exit(1);
        }
    };

    if !output.status.success() {
        eprintln!("error: spirv-as failed to assemble {}:", file_name(path));

        eprint!("{}", String::from_utf8_lossy(&output.stderr));

        std::process::exit(1);
    }

    read_spirv(&out)
}

/// Compiles one stage of shader source with slangc.
fn compile_stage_source(path: &Path, stage: Stage) -> BuiltStage {
    let workdir = create_workdir();

    let spirv_out = workdir.join("stage.spv");

    let reflection_path = workdir.join("reflection.json");

    let output = Command::new("slangc")
        .arg(path)
        .arg("-stage")
        .arg(stage.slang_flag())
        .arg("-target")
        .arg("spirv")
        .arg("-profile")
        .arg("spirv_1_3")
        .arg("-fvk-use-entrypoint-name")
        .arg("-reflection-json")
        .arg(&reflection_path)
        .arg("-o")
        .arg(&spirv_out)
        .output();

    let output = match output {
        Ok(output) => output,
        Err(_) => {
            eprintln!("error: slangc not found on PATH");

            eprintln!("       it ships with the Vulkan SDK (x86_64/bin/slangc)");

            std::process::exit(1);
        }
    };

    if !output.status.success() {
        eprintln!(
            "error: slangc failed to compile {} as {}:",
            file_name(path),
            stage.slang_flag()
        );

        eprint!("{}", String::from_utf8_lossy(&output.stderr));

        std::process::exit(1);
    }

    // The emitted entry point name comes from reflection; source without
    // [shader(...)] attributes that slang resolved still names it there.
    let entry = fs::read_to_string(&reflection_path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|reflection| {
            reflection["entryPoints"]
                .as_array()
                .and_then(|entries| {
                    entries
                        .iter()
                        .find(|entry| entry["stage"].as_str() == Some(stage.slang_flag()))
                })
                .and_then(|entry| entry["name"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "main".to_string());

    BuiltStage {
        spirv: read_spirv(&spirv_out),
        entry,
    }
}

/// Extracts the user-visible entry point name from a disassembled module:
/// the quoted string on the `OpEntryPoint <Stage> %symbol "<name>"` line.
fn parse_disasm_entry_point(disasm: &str, stage: Stage) -> Option<String> {
    let line = disasm
        .lines()
        .find(|line| line.contains(stage.disasm_keyword()))?;

    let open = line.find('"')? + 1;

    let close = line[open..].find('"')? + open;

    Some(line[open..close].to_string())
}

/// Compiles one .slang module and picks a display mode from reflection.
///
/// Exits the process with diagnostics on any user-facing failure.
fn compile_module(workdir: &Path, source: &SourceFile) -> CompiledShader {
    if is_shadertoy(&source.path) {
        return compile_shadertoy(workdir, source);
    }

    let spirv_path = workdir.join("shader.spv");

    let reflection_path = workdir.join("reflection.json");

    // First attempt: the file as-is.
    let plain = invoke_slangc(&source.path, &spirv_path, &reflection_path, None);

    if let Ok(()) = &plain
        && let Some(compiled) = finish(&spirv_path, &reflection_path)
    {
        return compiled;
    }
    // Compiles standalone but nothing displayable; the scaffold retry
    // below may reveal playground entry points.

    // Retry with the playground prelude available and imported.
    let scaffold_dir = workdir.join("prelude");

    fs::create_dir_all(&scaffold_dir).expect("create prelude dir");

    fs::write(scaffold_dir.join("playground.slang"), PLAYGROUND_PRELUDE)
        .expect("write playground prelude");

    fs::write(scaffold_dir.join("rendering.slang"), RENDERING_PRELUDE)
        .expect("write rendering prelude");

    let scaffold_source = with_playground_imports(&String::from_utf8_lossy(
        &fs::read(&source.path).expect("read shader source"),
    ));

    let scaffold_path = workdir.join("with-prelude.slang");

    fs::write(&scaffold_path, scaffold_source).expect("write combined shader source");

    let scaffold = invoke_slangc(
        &scaffold_path,
        &spirv_path,
        &reflection_path,
        Some(&scaffold_dir),
    );

    if let Ok(()) = &scaffold
        && let Some(compiled) = finish(&spirv_path, &reflection_path)
    {
        return compiled;
    }

    // Nothing displayable, or no build succeeded. When the file failed to
    // compile on its own, slangc's plain diagnostics describe the user's
    // actual file best; otherwise explain what the viewer supports.
    match plain {
        Err(plain_stderr) => {
            eprintln!("error: slangc failed to compile {}:", source.display_name);

            eprint!("{plain_stderr}");

            std::process::exit(1);
        }
        Ok(()) => report_not_displayable(),
    }
}

/// Runs slangc on the whole module (no `-entry`, so every entry point is
/// emitted into one SPIR-V file) and requests reflection JSON alongside.
fn invoke_slangc(
    source: &Path,
    spirv_out: &Path,
    reflection_out: &Path,
    include_dir: Option<&Path>,
) -> Result<(), String> {
    let mut command = Command::new("slangc");

    command
        .arg(source)
        .arg("-target")
        .arg("spirv")
        // SPIR-V 1.3 is the newest version Vulkan 1.1 accepts.
        .arg("-profile")
        .arg("spirv_1_3")
        // Keep entry point names (vertMain/fragMain/imageMain) instead of
        // renaming every entry to "main".
        .arg("-fvk-use-entrypoint-name")
        .arg("-reflection-json")
        .arg(reflection_out)
        .arg("-o")
        .arg(spirv_out);

    if let Some(dir) = include_dir {
        command.arg("-I").arg(dir);
    }

    let output = match command.output() {
        Ok(output) => output,
        Err(_) => {
            eprintln!("error: slangc not found on PATH");

            eprintln!("       it ships with the Vulkan SDK (x86_64/bin/slangc)");

            std::process::exit(1);
        }
    };

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

/// Prepends the playground imports unless the source already has them.
fn with_playground_imports(source: &str) -> String {
    if source.contains("import rendering") {
        source.to_string()
    } else {
        format!("import playground;\nimport rendering;\n\n{source}")
    }
}

//
// ------------------------------------------------------------
// Shadertoy-style GLSL
// ------------------------------------------------------------
//
// Shadertoy exports are fragment-only GLSL around a `mainImage(out, in)`
// entry point, relying on uniforms (`iTime`, `iResolution`, ...) that
// Shadertoy's own environment injects. The viewer wraps the export so
// slangc accepts it, and feeds the uniforms as push constants.
//

/// True when the module is a Shadertoy-style export rather than a slangc
/// module: text GLSL defining `mainImage`, with no `[shader(...)]`
/// attributes of its own. SPIR-V inputs are never text and never wrapped.
fn is_shadertoy(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };

    if bytes.starts_with(b"; SPIR-V") || bytes.starts_with(&SPIRV_MAGIC_LE) {
        return false;
    }

    let text = String::from_utf8_lossy(&bytes);

    text.contains("mainImage(") && !text.contains("[shader(")
}

/// The Shadertoy built-in uniforms, as a push-constant block. slangc's
/// GLSL front-end accepts `layout(push_constant)`, and push constants
/// need no descriptor sets — only a range in the pipeline layout and one
/// command per frame. Field order and types must match
/// [`ShadertoyUniforms`].
const SHADERTOY_UNIFORM_BLOCK: &str = "\
layout(push_constant) uniform ShadertoyUniforms
{
    vec3  iResolution;      // viewport resolution in pixels (z = 1.0)
    float iTime;            // seconds since the viewer started
    vec4  iMouse;           // xy: cursor, zw: last click (origin bottom-left)
    vec4  iDate;            // year, month, day, seconds into the day (UTC)
    float iTimeDelta;       // seconds since the previous frame
    float iFrameRate;       // estimated frames per second
    int   iFrame;           // frame counter
};
";

/// The entry point Shadertoy's environment would inject: call the user's
/// `mainImage`, translating Vulkan's top-left `gl_FragCoord` into
/// Shadertoy's bottom-left `fragCoord`.
const SHADERTOY_EPILOGUE: &str = "\
layout(location = 0) out vec4 _shadertoy_outColor;
void main()
{
    vec4 _shadertoy_color;
    mainImage(_shadertoy_color, vec2(gl_FragCoord.x, iResolution.y - gl_FragCoord.y));
    _shadertoy_outColor = _shadertoy_color;
}
";

/// The viewer-owned vertex stage for Shadertoy files: one triangle
/// spanning the whole viewport, generated from the vertex index because
/// the viewer supplies no vertex buffer. GLSL keeps it in the same
/// language as the wrapped fragment stage.
const SHADERTOY_FULLSCREEN_VERT: &str = "\
#version 450
void main(int vertexID: SV_VertexID)
{
    vec2 uv = vec2(float((vertexID << 1) & 2), float(vertexID & 2));
    gl_Position = vec4(uv * 2.0 - 1.0, 0.0, 1.0);
}
";

/// Builds a Shadertoy-style `.glsl` file into a fullscreen graphics pass.
///
/// The export is wrapped (uniform block + `main()` entry point) and
/// compiled as the fragment stage; the viewer's fullscreen-triangle vertex
/// stage is compiled alongside it. Both go through the same per-stage
/// slangc invocation as a `.vert`/`.frag` pair.
fn compile_shadertoy(workdir: &Path, source: &SourceFile) -> CompiledShader {
    let glsl = fs::read_to_string(&source.path).unwrap_or_else(|err| {
        eprintln!("error: cannot read {}: {err}", source.path.display());

        std::process::exit(1);
    });

    reject_unsupported_shadertoy(&glsl, &source.display_name);

    let fragment_path = workdir.join("shadertoy.frag");

    fs::write(&fragment_path, wrap_shadertoy(&glsl, &source.display_name))
        .expect("write wrapped shadertoy source");

    let vertex_path = workdir.join("fullscreen.vert");

    fs::write(&vertex_path, SHADERTOY_FULLSCREEN_VERT).expect("write fullscreen vertex source");

    let vertex = build_stage(&vertex_path, Stage::Vertex);

    let fragment = build_stage(&fragment_path, Stage::Fragment);

    CompiledShader {
        mode: RenderMode::Graphics {
            vertex_spirv: vertex.spirv,
            fragment_spirv: fragment.spirv,
            vertex_entry: vertex.entry,
            fragment_entry: fragment.entry,
            shadertoy: true,
        },
    }
}

/// Rejects Shadertoy inputs the viewer cannot feed: `iChannel*` texture
/// inputs and `uniform` declarations of the shader's own. Both would
/// compile into resources the pipeline layout does not provide, so they
/// are reported up front instead of failing at draw time.
fn reject_unsupported_shadertoy(source: &str, name: &str) {
    let stripped = strip_glsl_comments(source);

    for token in stripped.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        if token.starts_with("iChannel") {
            eprintln!(
                "error: {name} reads the {token} texture input; the viewer supplies no textures yet"
            );

            std::process::exit(1);
        }

        if token == "uniform" {
            eprintln!(
                "error: {name} declares its own uniforms; the viewer supplies only the Shadertoy built-ins"
            );

            std::process::exit(1);
        }
    }
}

/// Removes `//` line comments and `/* */` block comments so token scans do
/// not match words that only appear in comments.
fn strip_glsl_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());

    let mut chars = source.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();

                let mut previous = '\0';

                for c in chars.by_ref() {
                    if previous == '*' && c == '/' {
                        break;
                    }

                    previous = c;
                }
            }
            _ => out.push(c),
        }
    }

    out
}

/// Wraps a Shadertoy export so slangc's GLSL front-end accepts it.
///
/// `#version` must stay on the first line of a GLSL file: the export's own
/// line is kept when present, otherwise the viewer supplies `#version 450`.
/// `#line` then resets numbering, so slangc reports errors from the user's
/// code against the original file name and line numbers.
fn wrap_shadertoy(source: &str, name: &str) -> String {
    let trimmed = source.trim_start_matches(['\u{feff}', ' ', '\t', '\r', '\n']);

    let (version, body) = if trimmed.starts_with("#version") {
        let end = trimmed.find('\n').unwrap_or(trimmed.len());

        (trimmed[..end].to_string(), &trimmed[end..])
    } else {
        ("#version 450".to_string(), trimmed)
    };

    format!(
        "{version}\n\
// --- vert_frag_viewer: Shadertoy prelude (built-in uniforms) ---
{SHADERTOY_UNIFORM_BLOCK}\
#line 1 \"{name}\"\n\
{body}\
// --- vert_frag_viewer: epilogue (mainImage -> main) ---
{SHADERTOY_EPILOGUE}"
    )
}

/// Loads the SPIR-V and reflection output and selects a display mode.
///
/// Returns `None` when the module compiled but contains nothing the
/// viewer knows how to display.
fn finish(spirv_path: &Path, reflection_path: &Path) -> Option<CompiledShader> {
    let words = read_spirv(spirv_path);

    let reflection: Value =
        serde_json::from_str(&fs::read_to_string(reflection_path).expect("read reflection json"))
            .expect("parse reflection json");

    let entries = reflection["entryPoints"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    (
                        entry["name"].as_str().unwrap_or_default().to_string(),
                        entry["stage"].as_str().unwrap_or_default().to_string(),
                        entry["threadGroupSize"].clone(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let parameters = parse_parameters(&reflection);

    let vertex = entries
        .iter()
        .find(|(_, stage, _)| stage == "vertex")
        .map(|(name, _, _)| name.clone());

    let fragment = entries
        .iter()
        .find(|(_, stage, _)| stage == "fragment")
        .map(|(name, _, _)| name.clone());

    let compute = entries
        .iter()
        .find(|(_, stage, _)| stage == "compute")
        .cloned();

    // Graphics needs a vertex + fragment pair and must not declare any
    // resource parameters; the viewer supplies no vertex data or textures.
    if let (Some(vertex_entry), Some(fragment_entry)) = (&vertex, &fragment)
        && parameters.is_empty()
    {
        return Some(CompiledShader {
            mode: RenderMode::Graphics {
                vertex_spirv: words.clone(),
                fragment_spirv: words,
                vertex_entry: vertex_entry.clone(),
                fragment_entry: fragment_entry.clone(),
                shadertoy: false,
            },
        });
    }

    if let Some((entry, _, thread_group)) = compute {
        let group_size = [
            thread_group[0].as_u64().unwrap_or(1) as u32,
            thread_group[1].as_u64().unwrap_or(1) as u32,
            thread_group[2].as_u64().unwrap_or(1) as u32,
        ];

        return Some(CompiledShader {
            mode: RenderMode::Compute {
                spirv: words,
                entry,
                group_size,
                parameters,
            },
        });
    }

    None
}

fn parse_parameters(reflection: &Value) -> Vec<ShaderParam> {
    reflection["parameters"]
        .as_array()
        .map(|params| {
            params
                .iter()
                .map(|param| {
                    let name = param["name"].as_str().unwrap_or_default().to_string();

                    let binding = param["binding"]["index"].as_u64().unwrap_or(0) as u32;

                    let base_shape = param["type"]["baseShape"].as_str().unwrap_or("");

                    let access = param["type"]["access"].as_str().unwrap_or("");

                    let is_float =
                        param["type"]["resultType"]["scalarType"].as_str() == Some("float32");

                    let kind = match (base_shape, access) {
                        ("structuredBuffer", "readWrite") if is_float => {
                            ParamKind::RandomFloatBuffer
                        }
                        ("texture2D", "write") => ParamKind::OutputTexture,
                        _ => ParamKind::Unsupported(format!("{base_shape} ({access})")),
                    };

                    // `[playground::RAND(count)]` survives reflection as a
                    // userAttrib named playground_RAND.
                    let rand_count = param["userAttribs"]
                        .as_array()
                        .and_then(|attribs| {
                            attribs
                                .iter()
                                .find(|attrib| attrib["name"].as_str() == Some("playground_RAND"))
                        })
                        .and_then(|attrib| attrib["arguments"].as_array())
                        .and_then(|args| args.first())
                        .and_then(Value::as_u64)
                        .map(|count| count as u32);

                    ShaderParam {
                        name,
                        binding,
                        kind,
                        rand_count,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_spirv(path: &Path) -> Vec<u32> {
    let bytes = fs::read(path).expect("read compiled SPIR-V");

    let words: Vec<u32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_le_bytes(*word))
        .collect();

    assert_eq!(
        words.first(),
        Some(&0x0723_0203),
        "slangc did not emit valid SPIR-V"
    );

    words
}

fn report_not_displayable() -> ! {
    eprintln!("error: the module has no entry point this viewer can display.");
    eprintln!("       supported: vertex + fragment stages with no resource parameters,");
    eprintln!("                  or a compute kernel using the playground's drawPixel.");

    std::process::exit(1);
}

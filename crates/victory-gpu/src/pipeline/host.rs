//! Host pipelines for title pipelines (decision 0007, step 3): the
//! title's graphics state with the translated shaders, under a root
//! signature of our own that the translated shaders' bindings define
//! (`victory_shader::emit`): per stage one block of root constants for the
//! user data, root views for the title's root views, static samplers for
//! the immediate samplers, and directly indexed heaps.

use std::ffi::{CStr, CString};
use std::sync::Arc;

use victory_shader::emit::{Heaps, ROOT_CBV_SPACE, SAMPLER_SPACE, USER_DATA_SPACE};
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::PCSTR;

use super::{Shader, Translated};
use crate::host::host;

/// A stream's graphics state, owned (the title's stream lives only for the
/// creation call). Subobjects the stream leaves out keep D3D12's defaults.
pub(crate) struct GraphicsState {
    blend: D3D12_BLEND_DESC,
    sample_mask: u32,
    rasterizer: D3D12_RASTERIZER_DESC,
    depth: D3D12_DEPTH_STENCIL_DESC,
    /// The depth bounds test, which the plain description has no field for: a
    /// pipeline that enables it is made from the stream form, whose
    /// `D3D12_DEPTH_STENCIL_DESC1` carries it (`graphics_stream`).
    depth_bounds: bool,
    /// Input elements, their names pointing into `names`.
    inputs: Vec<D3D12_INPUT_ELEMENT_DESC>,
    names: Vec<CString>,
    strip_cut: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE,
    pub(crate) topology: D3D12_PRIMITIVE_TOPOLOGY_TYPE,
    pub(crate) targets: D3D12_RT_FORMAT_ARRAY,
    depth_format: DXGI_FORMAT,
    samples: DXGI_SAMPLE_DESC,
}

impl Default for GraphicsState {
    fn default() -> Self {
        let target = D3D12_RENDER_TARGET_BLEND_DESC {
            SrcBlend: D3D12_BLEND_ONE,
            DestBlend: D3D12_BLEND_ZERO,
            BlendOp: D3D12_BLEND_OP_ADD,
            SrcBlendAlpha: D3D12_BLEND_ONE,
            DestBlendAlpha: D3D12_BLEND_ZERO,
            BlendOpAlpha: D3D12_BLEND_OP_ADD,
            LogicOp: D3D12_LOGIC_OP_NOOP,
            RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
            ..Default::default()
        };
        let keep = D3D12_DEPTH_STENCILOP_DESC {
            StencilFailOp: D3D12_STENCIL_OP_KEEP,
            StencilDepthFailOp: D3D12_STENCIL_OP_KEEP,
            StencilPassOp: D3D12_STENCIL_OP_KEEP,
            StencilFunc: D3D12_COMPARISON_FUNC_ALWAYS,
        };
        GraphicsState {
            blend: D3D12_BLEND_DESC {
                RenderTarget: [target; 8],
                ..Default::default()
            },
            sample_mask: u32::MAX,
            rasterizer: D3D12_RASTERIZER_DESC {
                FillMode: D3D12_FILL_MODE_SOLID,
                CullMode: D3D12_CULL_MODE_BACK,
                DepthClipEnable: true.into(),
                ..Default::default()
            },
            depth_bounds: false,
            depth: D3D12_DEPTH_STENCIL_DESC {
                DepthEnable: true.into(),
                DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ALL,
                DepthFunc: D3D12_COMPARISON_FUNC_LESS,
                StencilReadMask: 0xff,
                StencilWriteMask: 0xff,
                FrontFace: keep,
                BackFace: keep,
                ..Default::default()
            },
            inputs: Vec::new(),
            names: Vec::new(),
            strip_cut: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED,
            topology: D3D12_PRIMITIVE_TOPOLOGY_TYPE_UNDEFINED,
            targets: D3D12_RT_FORMAT_ARRAY::default(),
            depth_format: DXGI_FORMAT_UNKNOWN,
            samples: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
        }
    }
}

/// A state's fields, written as text: numbers separated by a space, input
/// element names escaped so a name never splits the field.
#[derive(Default)]
struct Fields(String);

impl Fields {
    fn n(&mut self, v: i64) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        self.0.push_str(&v.to_string());
    }

    fn name(&mut self, name: &str) {
        self.0.push(' ');
        for b in name.bytes() {
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.0.push(b as char);
            } else {
                self.0.push_str(&format!("%{b:02x}"));
            }
        }
    }
}

/// The bytes an escaped name stood for, or  for an escape this does not
/// know.
fn unescape(name: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(name.len());
    let mut it = name.bytes();
    while let Some(b) = it.next() {
        if b != b'%' {
            bytes.push(b);
        } else {
            let hex = [it.next()?, it.next()?];
            bytes.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
        }
    }
    Some(bytes)
}

/// Reads a state's fields back.
struct Read<'a> {
    it: std::str::Split<'a, char>,
}

impl<'a> Read<'a> {
    fn n(&mut self) -> Option<i64> {
        self.it.next()?.parse::<i64>().ok()
    }

    fn word(&mut self) -> Option<&'a str> {
        self.it.next()
    }

    fn done(&mut self) -> bool {
        self.it.next().is_none()
    }
}

impl GraphicsState {
    /// Takes in one subobject of a stream: its type, then its data at +4
    /// (+8 for those holding pointers).
    ///
    /// # Safety
    /// `at` points at a whole subobject of type `kind`.
    pub(crate) unsafe fn read(&mut self, kind: u32, at: *const u8) {
        // SAFETY: the caller passes a whole subobject.
        unsafe {
            let data = at.add(4);
            match kind {
                8 => self.blend = data.cast::<D3D12_BLEND_DESC>().read_unaligned(),
                9 => self.sample_mask = data.cast::<u32>().read_unaligned(),
                10 => self.rasterizer = data.cast::<D3D12_RASTERIZER_DESC>().read_unaligned(),
                11 => self.depth = data.cast::<D3D12_DEPTH_STENCIL_DESC>().read_unaligned(),
                21 => {
                    let d = data.cast::<D3D12_DEPTH_STENCIL_DESC1>().read_unaligned();
                    self.depth_bounds = d.DepthBoundsTestEnable.as_bool();
                    self.depth = D3D12_DEPTH_STENCIL_DESC {
                        DepthEnable: d.DepthEnable,
                        DepthWriteMask: d.DepthWriteMask,
                        DepthFunc: d.DepthFunc,
                        StencilEnable: d.StencilEnable,
                        StencilReadMask: d.StencilReadMask,
                        StencilWriteMask: d.StencilWriteMask,
                        FrontFace: d.FrontFace,
                        BackFace: d.BackFace,
                    };
                }
                12 => {
                    let layout = at.add(8).cast::<D3D12_INPUT_LAYOUT_DESC>().read_unaligned();
                    if layout.pInputElementDescs.is_null() {
                        return;
                    }
                    let elements = std::slice::from_raw_parts(
                        layout.pInputElementDescs,
                        layout.NumElements as usize,
                    );
                    self.names = elements
                        .iter()
                        .map(|e| CStr::from_ptr(e.SemanticName.0.cast()).to_owned())
                        .collect();
                    self.inputs = elements
                        .iter()
                        .zip(&self.names)
                        .map(|(e, name)| D3D12_INPUT_ELEMENT_DESC {
                            SemanticName: PCSTR(name.as_ptr().cast()),
                            Format: DXGI_FORMAT(
                                crate::resource::pc_format(e.Format.0 as u32) as i32
                            ),
                            ..*e
                        })
                        .collect();
                }
                13 => {
                    self.strip_cut =
                        D3D12_INDEX_BUFFER_STRIP_CUT_VALUE(data.cast::<i32>().read_unaligned())
                }
                14 => {
                    self.topology =
                        D3D12_PRIMITIVE_TOPOLOGY_TYPE(data.cast::<i32>().read_unaligned())
                }
                15 => {
                    self.targets = data.cast::<D3D12_RT_FORMAT_ARRAY>().read_unaligned();
                    for f in &mut self.targets.RTFormats {
                        f.0 = crate::resource::target_format(crate::resource::pc_format(
                            f.0 as u32,
                        )) as i32;
                    }
                }
                16 => {
                    let f = data.cast::<u32>().read_unaligned();
                    self.depth_format = DXGI_FORMAT(crate::resource::pc_format(f) as i32);
                }
                17 => self.samples = data.cast::<DXGI_SAMPLE_DESC>().read_unaligned(),
                _ => {}
            }
        }
    }

    /// Everything the state holds, as text: part of a pipeline's name in
    /// the library and of the key a title pipeline is started early by.
    pub(crate) fn describe(&self) -> String {
        let inputs: Vec<String> = self
            .inputs
            .iter()
            .zip(&self.names)
            .map(|(e, name)| {
                format!(
                    "{name:?} {} {} {} {} {} {}",
                    e.SemanticIndex, e.Format.0, e.InputSlot, e.AlignedByteOffset, e.InputSlotClass.0, e.InstanceDataStepRate
                )
            })
            .collect();
        let bounds = if self.depth_bounds { " bounds" } else { "" };
        format!(
            "{:?} {} {:?} {:?} {inputs:?} {} {} {:?} {} {:?}{bounds}",
            self.blend,
            self.sample_mask,
            self.rasterizer,
            self.depth,
            self.strip_cut.0,
            self.topology.0,
            self.targets,
            self.depth_format.0,
            self.samples
        )
    }

    /// The state's fields as integers, in `describe()`'s order, so a state can
    /// be written to a pipeline dump and read back exactly.
    ///
    /// `describe()` of the state [`GraphicsState::from_fields`] rebuilds is
    /// byte-identical to this one's, which is what makes a precompiled
    /// library's entry names the names the title asks for: a name is
    /// `library::name(kind, root, code, describe())`, so any field this codec
    /// loses or reorders is an entry nothing will ever find.
    pub(crate) fn fields(&self) -> String {
        let mut out = Fields::default();
        // Fields version: bumped if this order or these meanings change, so an
        // old dump is refused instead of read as a different state.
        out.n(1);
        out.n(self.blend.AlphaToCoverageEnable.0 as i64);
        out.n(self.blend.IndependentBlendEnable.0 as i64);
        for rt in &self.blend.RenderTarget {
            for v in [
                rt.BlendEnable.0,
                rt.LogicOpEnable.0,
                rt.SrcBlend.0,
                rt.DestBlend.0,
                rt.BlendOp.0,
                rt.SrcBlendAlpha.0,
                rt.DestBlendAlpha.0,
                rt.BlendOpAlpha.0,
                rt.LogicOp.0,
                i32::from(rt.RenderTargetWriteMask),
            ] {
                out.n(v as i64);
            }
        }
        out.n(self.sample_mask as i64);
        let r = &self.rasterizer;
        for v in [
            r.FillMode.0,
            r.CullMode.0,
            r.FrontCounterClockwise.0,
            r.DepthBias,
            r.DepthBiasClamp.to_bits() as i32,
            r.SlopeScaledDepthBias.to_bits() as i32,
            r.DepthClipEnable.0,
            r.MultisampleEnable.0,
            r.AntialiasedLineEnable.0,
            r.ForcedSampleCount as i32,
            r.ConservativeRaster.0,
        ] {
            out.n(v as i64);
        }
        let d = &self.depth;
        for v in [d.DepthEnable.0, d.DepthWriteMask.0, d.DepthFunc.0, d.StencilEnable.0] {
            out.n(v as i64);
        }
        out.n(i32::from(d.StencilReadMask) as i64);
        out.n(i32::from(d.StencilWriteMask) as i64);
        for face in [&d.FrontFace, &d.BackFace] {
            for v in [
                face.StencilFailOp.0,
                face.StencilDepthFailOp.0,
                face.StencilPassOp.0,
                face.StencilFunc.0,
            ] {
                out.n(v as i64);
            }
        }
        out.n(self.inputs.len() as i64);
        for (e, name) in self.inputs.iter().zip(&self.names) {
            // A name is a semantic ("POSITION", "TEXCOORD") and holds no space,
            // but an escape keeps a title's odd name from splitting the field.
            out.name(&name.to_string_lossy());
            for v in [
                e.SemanticIndex as i64,
                e.Format.0 as i64,
                e.InputSlot as i64,
                e.AlignedByteOffset as i64,
                e.InputSlotClass.0 as i64,
                e.InstanceDataStepRate as i64,
            ] {
                out.n(v);
            }
        }
        out.n(self.strip_cut.0 as i64);
        out.n(self.topology.0 as i64);
        for f in &self.targets.RTFormats {
            out.n(f.0 as i64);
        }
        out.n(self.targets.NumRenderTargets as i64);
        out.n(self.depth_format.0 as i64);
        out.n(self.samples.Count as i64);
        out.n(self.samples.Quality as i64);
        // Trailing and optional: a state that does not enable the depth bounds
        // test writes exactly the text it wrote before this field existed, so a
        // warm library's entry names still match.
        if self.depth_bounds {
            out.n(1);
        }
        out.0
    }

    /// The state [`GraphicsState::fields`] wrote, or `None` if the text is not
    /// exactly one such state (another fields version, a line cut short, an
    /// escape this does not know).
    pub(crate) fn from_fields(text: &str) -> Option<GraphicsState> {
        let mut read = Read { it: text.split(' ') };
        if read.word()? != "1" {
            return None;
        }
        let b = |v: Option<i64>| -> Option<bool> { Some(v? != 0) };
        let i = |v: Option<i64>| -> Option<i32> { i32::try_from(v?).ok() };
        let u = |v: Option<i64>| -> Option<u32> { u32::try_from(v?).ok() };
        let mut state = GraphicsState::default();
        state.blend.AlphaToCoverageEnable = b(read.n())?.into();
        state.blend.IndependentBlendEnable = b(read.n())?.into();
        for rt in &mut state.blend.RenderTarget {
            rt.BlendEnable = b(read.n())?.into();
            rt.LogicOpEnable = b(read.n())?.into();
            rt.SrcBlend = D3D12_BLEND(i(read.n())?);
            rt.DestBlend = D3D12_BLEND(i(read.n())?);
            rt.BlendOp = D3D12_BLEND_OP(i(read.n())?);
            rt.SrcBlendAlpha = D3D12_BLEND(i(read.n())?);
            rt.DestBlendAlpha = D3D12_BLEND(i(read.n())?);
            rt.BlendOpAlpha = D3D12_BLEND_OP(i(read.n())?);
            rt.LogicOp = D3D12_LOGIC_OP(i(read.n())?);
            rt.RenderTargetWriteMask = u8::try_from(read.n()?).ok()?;
        }
        state.sample_mask = u(read.n())?;
        let r = &mut state.rasterizer;
        r.FillMode = D3D12_FILL_MODE(i(read.n())?);
        r.CullMode = D3D12_CULL_MODE(i(read.n())?);
        r.FrontCounterClockwise = b(read.n())?.into();
        r.DepthBias = i(read.n())?;
        r.DepthBiasClamp = f32::from_bits(u(read.n())?);
        r.SlopeScaledDepthBias = f32::from_bits(u(read.n())?);
        r.DepthClipEnable = b(read.n())?.into();
        r.MultisampleEnable = b(read.n())?.into();
        r.AntialiasedLineEnable = b(read.n())?.into();
        r.ForcedSampleCount = u(read.n())?;
        r.ConservativeRaster = D3D12_CONSERVATIVE_RASTERIZATION_MODE(i(read.n())?);
        let d = &mut state.depth;
        d.DepthEnable = b(read.n())?.into();
        d.DepthWriteMask = D3D12_DEPTH_WRITE_MASK(i(read.n())?);
        d.DepthFunc = D3D12_COMPARISON_FUNC(i(read.n())?);
        d.StencilEnable = b(read.n())?.into();
        d.StencilReadMask = u8::try_from(read.n()?).ok()?;
        d.StencilWriteMask = u8::try_from(read.n()?).ok()?;
        for face in [&mut d.FrontFace, &mut d.BackFace] {
            face.StencilFailOp = D3D12_STENCIL_OP(i(read.n())?);
            face.StencilDepthFailOp = D3D12_STENCIL_OP(i(read.n())?);
            face.StencilPassOp = D3D12_STENCIL_OP(i(read.n())?);
            face.StencilFunc = D3D12_COMPARISON_FUNC(i(read.n())?);
        }
        for _ in 0..usize::try_from(read.n()?).ok()? {
            let name = unescape(read.word()?)?;
            state.names.push(CString::new(name).ok()?);
            state.inputs.push(D3D12_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR(std::ptr::null()),
                SemanticIndex: u(read.n())?,
                Format: DXGI_FORMAT(i(read.n())?),
                InputSlot: u(read.n())?,
                AlignedByteOffset: u(read.n())?,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION(i(read.n())?),
                InstanceDataStepRate: u(read.n())?,
            });
        }
        for (e, name) in state.inputs.iter_mut().zip(&state.names) {
            e.SemanticName = PCSTR(name.as_ptr().cast());
        }
        state.strip_cut = D3D12_INDEX_BUFFER_STRIP_CUT_VALUE(i(read.n())?);
        state.topology = D3D12_PRIMITIVE_TOPOLOGY_TYPE(i(read.n())?);
        for f in &mut state.targets.RTFormats {
            f.0 = i(read.n())?;
        }
        state.targets.NumRenderTargets = u(read.n())?;
        state.depth_format = DXGI_FORMAT(i(read.n())?);
        state.samples = DXGI_SAMPLE_DESC { Count: u(read.n())?, Quality: u(read.n())? };
        // The optional trailing field: absent for every state that does not
        // enable the depth bounds test (so old dumps read as themselves), and
        // only ever 0 or 1 when present.
        state.depth_bounds = match read.n() {
            Some(0) => false,
            Some(1) => true,
            Some(_) => return None,
            None => false,
        };
        read.done().then_some(state)
    }

    /// The vertex buffer slots the input layout reads.
    fn slots(&self) -> u32 {
        self.inputs
            .iter()
            .fold(0, |m, e| m | 1 << e.InputSlot.min(31))
    }

    /// The input elements with their byte offsets worked out.
    fn elements(&self) -> Vec<Input> {
        let mut next = [0u32; 32];
        self.inputs
            .iter()
            .map(|e| {
                let slot = e.InputSlot.min(31) as usize;
                let format = e.Format.0 as u32;
                let offset = if e.AlignedByteOffset == D3D12_APPEND_ALIGNED_ELEMENT {
                    next[slot]
                } else {
                    e.AlignedByteOffset
                };
                let bytes = crate::resource::element(format).map_or(0, |(b, _)| b as u32);
                next[slot] = offset + bytes;
                Input {
                    slot: slot as u32,
                    offset,
                    format,
                    per_instance: e.InputSlotClass == D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
                }
            })
            .collect()
    }
}

/// One input element: vertex buffer slot, byte offset, format.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Input {
    pub(crate) slot: u32,
    pub(crate) offset: u32,
    pub(crate) format: u32,
    pub(crate) per_instance: bool,
}

/// The console's own primitive topology types.
const RECTANGLES: i32 = 19;
const QUADS: i32 = 32;

/// Whether the title's root SRVs are bound as one-view tables of the
/// buffer's host copy, whose reads past its end return zeros, rather than
/// as root descriptors, which have no size: a mesh shader's vertex fetch
/// has no record limit, and a read past the copy reaches whatever the host
/// placed after it (memory it has freed: the device is removed, upstream
/// PR #493). `VICTORY_ROOT_SRV_DIRECT=1` goes back to root descriptors.
pub(crate) fn bounded_root_srvs() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_ROOT_SRV_DIRECT").is_none())
}

/// One stage of a host pipeline and where its bindings go.
pub(crate) struct Stage {
    pub(crate) shader: Arc<Shader>,
    /// The root parameter of its user data, and its size in dwords.
    pub(crate) user_data: Option<(u32, u32)>,
    /// Root parameters of its root views: (title root parameter, ours).
    pub(crate) root_cbvs: Vec<(u8, u32)>,
    pub(crate) root_srvs: Vec<(u8, u32)>,
    /// Root UAVs: one-view tables like the SRVs (or root descriptors with
    /// `VICTORY_ROOT_SRV_DIRECT=1`).
    pub(crate) root_uavs: Vec<(u8, u32)>,
}

impl Stage {
    pub(crate) fn translated(&self) -> &Translated {
        self.shader
            .translated()
            .expect("host pipelines are made from translated shaders")
    }
}

pub(crate) struct HostPipeline {
    /// A compute pipeline (else graphics).
    pub(crate) compute: bool,
    /// A mesh pipeline (graphics without vertex input).
    pub(crate) mesh: bool,
    pub(crate) pso: ID3D12PipelineState,
    pub(crate) root: ID3D12RootSignature,
    /// Vertex, then pixel stage, or the compute stage.
    pub(crate) stages: Vec<Stage>,
    /// The vertex buffer slots it reads (bit per slot).
    pub(crate) slots: u32,
    pub(crate) inputs: Vec<Input>,
    /// Has the geometry stage that completes the console's rectangles
    /// (`victory_shader::emit::rect_list_gs`): its rectangle draws go out
    /// as the title gives them, three vertices a rectangle.
    pub(crate) rects: bool,
    /// Its shaders, for reports of slow submissions.
    pub(crate) name: String,
    /// The library's key for it (`library::name`): what a load asks the
    /// library for, and therefore the only thing a caller can compare against
    /// a list of wanted pipelines.
    pub(crate) key: String,
    /// Its depth, stencil, cull and blend state in short, for the frame
    /// listing (empty for compute).
    #[cfg_attr(not(feature = "diagnostics"), allow(dead_code))]
    pub(crate) state: String,
}

/// Logs a pipeline that gets no host pipeline, once per reason.
fn refuse(reason: &'static str, shaders: &[Arc<Shader>]) -> Option<HostPipeline> {
    static SEEN: std::sync::Mutex<Vec<&str>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if !seen.contains(&reason) {
        seen.push(reason);
        tracing::warn!(?shaders, "no host pipeline: {reason} (logged once)");
    }
    None
}

impl HostPipeline {
    /// A host pipeline for vertex and pixel shaders with the graphics
    /// state `g`, or for one compute shader.
    pub(crate) fn new(shaders: &[Arc<Shader>], g: Option<&GraphicsState>) -> Option<HostPipeline> {
        // For the report when the host device is removed: what this thread
        // has in the library from here on.
        super::library::label(format!("{shaders:?}"));
        let find = |stage| shaders.iter().find(|s| s.stage == stage);
        let compute = match shaders {
            [cs] if cs.stage == "compute" => Some(cs),
            _ => None,
        };
        if compute.is_none()
            && shaders
                .iter()
                .any(|s| !matches!(s.stage, "vertex" | "pixel" | "mesh"))
        {
            return refuse(
                "only vertex, mesh, pixel and compute stages are translated",
                shaders,
            );
        }
        let mesh = find("mesh");
        let stages = if let Some(cs) = compute {
            vec![(cs, D3D12_SHADER_VISIBILITY_ALL)]
        } else {
            let first = match (mesh, find("vertex")) {
                (Some(ms), _) => (ms, D3D12_SHADER_VISIBILITY_MESH),
                (None, Some(vs)) => (vs, D3D12_SHADER_VISIBILITY_VERTEX),
                (None, None) => return refuse("no vertex or mesh shader", shaders),
            };
            let mut stages = vec![first];
            stages.extend(find("pixel").map(|ps| (ps, D3D12_SHADER_VISIBILITY_PIXEL)));
            stages
        };
        // A module this build's compiler faults on is refused before the
        // create: the session loses those draws, not itself.
        if let Some((s, _)) = stages
            .iter()
            .find(|(s, _)| super::poison::poisoned(s.stage, &s.hash))
        {
            super::poison::note_refusal(s.stage, &s.hash);
            return refuse("a module the driver's compiler faults on", shaders);
        }
        if mesh.is_some() && !host()?.mesh_shaders() {
            return refuse("the host GPU has no mesh shaders (or no shader model 6.6)", shaders);
        }
        for (s, _) in &stages {
            s.translated()?;
        }
        let host = host()?;

        // The root signature the translated shaders' bindings define.
        let mut params: Vec<D3D12_ROOT_PARAMETER1> = Vec::new();
        // The ranges table parameters point at, alive until the root
        // signature is made.
        let mut ranges: Vec<Box<D3D12_DESCRIPTOR_RANGE1>> = Vec::new();
        let mut samplers = Vec::new();
        let mut bound = Vec::new();
        // The heaps the stages' code indexes: the signature declares those
        // and no others.
        let mut heaps = Heaps::default();
        for &(shader, visibility) in &stages {
            let t = &shader.translated()?.host;
            let stage_heaps = t.heaps();
            heaps.views |= stage_heaps.views;
            heaps.samplers |= stage_heaps.samplers;
            let mut stage = Stage {
                shader: shader.clone(),
                user_data: None,
                root_cbvs: Vec::new(),
                root_srvs: Vec::new(),
                root_uavs: Vec::new(),
            };
            let dwords = t.user_data_dwords();
            if dwords > 0 {
                stage.user_data = Some((params.len() as u32, dwords));
                params.push(D3D12_ROOT_PARAMETER1 {
                    ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                    Anonymous: D3D12_ROOT_PARAMETER1_0 {
                        Constants: D3D12_ROOT_CONSTANTS {
                            ShaderRegister: 0,
                            RegisterSpace: USER_DATA_SPACE,
                            Num32BitValues: dwords,
                        },
                    },
                    ShaderVisibility: visibility,
                });
            }
            for (slots, kind, out) in [
                (
                    &t.root_cbvs,
                    D3D12_ROOT_PARAMETER_TYPE_CBV,
                    &mut stage.root_cbvs,
                ),
                (
                    &t.root_srvs,
                    D3D12_ROOT_PARAMETER_TYPE_SRV,
                    &mut stage.root_srvs,
                ),
                (
                    &t.root_uavs,
                    D3D12_ROOT_PARAMETER_TYPE_UAV,
                    &mut stage.root_uavs,
                ),
            ] {
                for &slot in slots {
                    let uav = kind == D3D12_ROOT_PARAMETER_TYPE_UAV;
                    // Ours, not the title's, stay root descriptors: the
                    // table of strides, the directory of title memory by
                    // address and its fault notes.
                    let ours = if uav {
                        slot == victory_shader::emit::SLOT_FAULTS
                    } else {
                        slot == victory_shader::emit::SLOT_STRIDES
                            || slot == victory_shader::emit::SLOT_MEMORY
                    };
                    if (uav || kind == D3D12_ROOT_PARAMETER_TYPE_SRV) && !ours && bounded_root_srvs() {
                        // A one-view table: the view's size bounds the
                        // reads (and a root UAV's writes), which a root
                        // descriptor leaves unbounded.
                        // (Our own table of strides stays a descriptor.)
                        ranges.push(Box::new(D3D12_DESCRIPTOR_RANGE1 {
                            RangeType: if uav {
                                D3D12_DESCRIPTOR_RANGE_TYPE_UAV
                            } else {
                                D3D12_DESCRIPTOR_RANGE_TYPE_SRV
                            },
                            NumDescriptors: 1,
                            BaseShaderRegister: u32::from(slot),
                            RegisterSpace: ROOT_CBV_SPACE,
                            Flags: D3D12_DESCRIPTOR_RANGE_FLAG_DESCRIPTORS_VOLATILE
                                | D3D12_DESCRIPTOR_RANGE_FLAG_DATA_VOLATILE,
                            OffsetInDescriptorsFromTableStart: 0,
                        }));
                        let range: *const D3D12_DESCRIPTOR_RANGE1 = &**ranges.last()?;
                        out.push((slot, params.len() as u32));
                        params.push(D3D12_ROOT_PARAMETER1 {
                            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                            Anonymous: D3D12_ROOT_PARAMETER1_0 {
                                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE1 {
                                    NumDescriptorRanges: 1,
                                    pDescriptorRanges: range,
                                },
                            },
                            ShaderVisibility: visibility,
                        });
                        continue;
                    }
                    out.push((slot, params.len() as u32));
                    params.push(D3D12_ROOT_PARAMETER1 {
                        ParameterType: kind,
                        Anonymous: D3D12_ROOT_PARAMETER1_0 {
                            Descriptor: D3D12_ROOT_DESCRIPTOR1 {
                                ShaderRegister: u32::from(slot),
                                RegisterSpace: ROOT_CBV_SPACE,
                                Flags: D3D12_ROOT_DESCRIPTOR_FLAG_DATA_VOLATILE,
                            },
                        },
                        ShaderVisibility: visibility,
                    });
                }
            }
            for (n, s) in t.static_samplers.iter().enumerate() {
                samplers.push(D3D12_STATIC_SAMPLER_DESC {
                    Filter: D3D12_FILTER(s.filter as i32),
                    AddressU: D3D12_TEXTURE_ADDRESS_MODE(s.address[0] as i32),
                    AddressV: D3D12_TEXTURE_ADDRESS_MODE(s.address[1] as i32),
                    AddressW: D3D12_TEXTURE_ADDRESS_MODE(s.address[2] as i32),
                    MipLODBias: s.mip_lod_bias,
                    MaxAnisotropy: s.max_anisotropy,
                    ComparisonFunc: D3D12_COMPARISON_FUNC(s.comparison as i32),
                    BorderColor: D3D12_STATIC_BORDER_COLOR(s.border as i32),
                    MinLOD: s.min_lod,
                    MaxLOD: s.max_lod,
                    ShaderRegister: n as u32,
                    RegisterSpace: SAMPLER_SPACE,
                    ShaderVisibility: visibility,
                });
            }
            bound.push(stage);
        }
        #[cfg(feature = "diagnostics")]
        let root_started = std::time::Instant::now();
        let made = root_signature(&host.device, &params, &samplers, compute.is_some(), heaps);
        #[cfg(feature = "diagnostics")]
        super::timing::add(super::timing::Part::RootSignature, root_started.elapsed());
        let (root, root_blob) = match made {
            Ok(root) => root,
            Err(e) => {
                tracing::warn!(error = %e, ?shaders, "host root signature refused");
                return None;
            }
        };
        let code = |i: usize| {
            stages.get(i).and_then(|(s, _)| s.translated()).map_or(
                D3D12_SHADER_BYTECODE::default(),
                |t| D3D12_SHADER_BYTECODE {
                    pShaderBytecode: t.host.dxil.as_ptr().cast(),
                    BytecodeLength: t.host.dxil.len(),
                },
            )
        };
        if compute.is_some() {
            let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
                // SAFETY: a borrowed pointer the description never releases.
                pRootSignature: unsafe { std::mem::transmute_copy(&root) },
                CS: code(0),
                ..Default::default()
            };
            let name = super::library::name("compute", &root_blob, &[&code_bytes(&code(0))], "");
            // `VICTORY_ASKED_NAMES`: only the pipelines a real session asked for
            // are worth a driver compile. The name is known before the create, and
            // a pipeline nobody asked for costs the same seconds of driver time.
            if super::library::only_asked(&name) {
                return None;
            }
            // A fault inside the driver leaves no line of its own, so what the
            // create was given has to be in the log before it is given
            // (`VICTORY_PIPELINE_ARGS=1`, diagnostics builds). A description
            // whose root signature or shader bytes are empty is the shape a
            // null inside the compiler would explain.
            #[cfg(feature = "diagnostics")]
            if std::env::var_os("VICTORY_PIPELINE_ARGS").is_some() {
                let tables = params
                    .iter()
                    .filter(|p| p.ParameterType == D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE)
                    .count();
                tracing::warn!(
                    pipeline = %name,
                    cs_bytes = desc.CS.BytecodeLength,
                    root_bytes = root_blob.len(),
                    params = params.len(),
                    tables,
                    samplers = samplers.len(),
                    bound_root = desc.pRootSignature.is_some(),
                    node = desc.NodeMask,
                    "compute create arguments"
                );
            }
            let made = super::library::pipeline(
                &name,
                "compute",
                // SAFETY: the description points at live data.
                |library, name| unsafe { library.LoadComputePipeline(name, &desc) },
                // SAFETY: as above.
                || unsafe { host.device.CreateComputePipelineState(&desc) },
            );
            let pso = match made {
                Ok(pso) => pso,
                Err(e) => {
                    // What the description carried, so a refusal and a later
                    // creation of the same module can be compared: an empty
                    // `CS` is the refusal this call cannot distinguish from a
                    // module the driver dislikes.
                    tracing::warn!(
                        error = %e,
                        ?shaders,
                        cs_bytes = desc.CS.BytecodeLength,
                        root = desc.pRootSignature.is_some(),
                        node = desc.NodeMask,
                        flags = desc.Flags.0,
                        "host pipeline refused"
                    );
                    host.log_messages();
                    return None;
                }
            };
            tracing::info!(
                ?shaders,
                cs_bytes = desc.CS.BytecodeLength,
                root = desc.pRootSignature.is_some(),
                node = desc.NodeMask,
                flags = desc.Flags.0,
                "host pipeline"
            );
            crate::host::name(&pso, || format!("{shaders:?}"));
            return Some(HostPipeline {
                compute: true,
                mesh: false,
                pso,
                root,
                stages: bound,
                name: format!("{shaders:?}"),
                key: name.clone(),
                slots: 0,
                inputs: Vec::new(),
                rects: false,
                state: String::new(),
            });
        }
        let Some(g) = g else {
            return refuse("graphics state not kept (plain description)", shaders);
        };
        let state = g.describe();
        if mesh.is_some() {
            // "triangles": the stream names its topology since it did not
            // load from the library without one (see `mesh_pipeline`); a
            // new name keeps such entries, stored before, from being
            // looked up again.
            let name = super::library::name(
                "mesh triangles",
                &root_blob,
                &[&code_bytes(&code(0)), &code_bytes(&code(1))],
                &state,
            );
            let pso = match mesh_pipeline(&host.device, &root, code(0), code(1), g, &name) {
                Ok(pso) => pso,
                Err(e) => {
                    tracing::warn!(error = %e, ?shaders, "host mesh pipeline refused");
                    host.log_messages();
                    return None;
                }
            };
            tracing::info!(?shaders, "host mesh pipeline");
            crate::host::name(&pso, || format!("{shaders:?}"));
            return Some(HostPipeline {
                compute: false,
                mesh: true,
                pso,
                root,
                stages: bound,
                name: format!("{shaders:?}"),
                key: name.clone(),
                slots: 0,
                inputs: Vec::new(),
                rects: false,
                state: String::new(),
            });
        }

        // A pipeline the title makes for rectangles gets a geometry stage
        // that completes each one from the three vertices the vertex
        // shader made. Without it (refused here or by the host) the draws
        // fall back on what `draw` does for rectangles.
        let mut rect_gs = None;
        if g.topology.0 == RECTANGLES {
            let ps = stages.get(1).and_then(|(s, _)| s.translated());
            let vs = &stages[0].0.translated()?.host.dxil;
            match victory_shader::emit::rect_list_gs(vs, ps.map(|t| &t.host.dxil[..])) {
                Ok(gs) => rect_gs = Some(gs),
                Err(e) => tracing::warn!(error = %e, ?shaders, "no rectangle geometry stage"),
            }
        }
        let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            // SAFETY: a borrowed pointer the description never releases.
            pRootSignature: unsafe { std::mem::transmute_copy(&root) },
            VS: code(0),
            PS: code(1),
            GS: rect_gs.as_ref().map_or(D3D12_SHADER_BYTECODE::default(), |gs| {
                D3D12_SHADER_BYTECODE {
                    pShaderBytecode: gs.as_ptr().cast(),
                    BytecodeLength: gs.len(),
                }
            }),
            BlendState: g.blend,
            SampleMask: g.sample_mask,
            RasterizerState: g.rasterizer,
            DepthStencilState: g.depth,
            InputLayout: D3D12_INPUT_LAYOUT_DESC {
                pInputElementDescs: g.inputs.as_ptr(),
                NumElements: g.inputs.len() as u32,
            },
            IBStripCutValue: g.strip_cut,
            // The console's rectangle and quad types draw as triangles on
            // the host (see `draw`).
            PrimitiveTopologyType: match g.topology.0 {
                RECTANGLES | QUADS => D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
                _ => g.topology,
            },
            NumRenderTargets: g.targets.NumRenderTargets,
            RTVFormats: g.targets.RTFormats,
            DSVFormat: g.depth_format,
            SampleDesc: g.samples,
            ..Default::default()
        };
        // A pipeline with the geometry stage has a name of its own in the
        // library; the others keep theirs.
        let name = |gs: Option<&Vec<u8>>| match gs {
            Some(gs) => super::library::name(
                "graphics rectangles",
                &root_blob,
                &[&code_bytes(&code(0)), &code_bytes(&code(1)), &gs[..]],
                &state,
            ),
            None => super::library::name(
                "graphics",
                &root_blob,
                &[&code_bytes(&code(0)), &code_bytes(&code(1))],
                &state,
            ),
        };
        // The depth bounds test has no field in the plain description, so a
        // pipeline that enables it is made (and stored) from the stream form.
        let bounds = g.depth_bounds && depth_bounds_supported(&host.device);
        let make = |desc: &D3D12_GRAPHICS_PIPELINE_STATE_DESC, name: &str| {
            if bounds {
                let stream =
                    GraphicsStream::of(desc, windows::core::Interface::as_raw(&root));
                let sdesc = stream.desc();
                return super::library::pipeline(
                    name,
                    "graphics bounds",
                    // SAFETY: the stream holds whole subobjects pointing at live data.
                    |library, name| unsafe { library.LoadPipeline(name, &sdesc) },
                    // SAFETY: as above.
                    || unsafe {
                        windows::core::Interface::cast::<ID3D12Device2>(&host.device)
                            .and_then(|device| device.CreatePipelineState(&sdesc))
                    },
                );
            }
            super::library::pipeline(
                name,
                "graphics",
                // SAFETY: the description points at live data.
                |library, name| unsafe { library.LoadGraphicsPipeline(name, desc) },
                // SAFETY: as above.
                || unsafe { host.device.CreateGraphicsPipelineState(desc) },
            )
        };
        // The key the successful create stored under: a rectangle pipeline that
        // falls back to no geometry stage is stored under that name instead.
        let mut key_used = name(rect_gs.as_ref());
        if super::library::only_asked(&key_used) {
            return None;
        }
        let mut made = make(&desc, &key_used);
        if made.is_err() && rect_gs.is_some() {
            // The host refused the geometry stage: once more without it.
            tracing::warn!(?shaders, "host pipeline refused with the rectangle geometry stage");
            host.log_messages();
            desc.GS = D3D12_SHADER_BYTECODE::default();
            rect_gs = None;
            key_used = name(None);
            made = make(&desc, &key_used);
        }
        let pso = match made {
            Ok(pso) => pso,
            Err(e) => {
                // What the description carried, as the compute path logs it: a
                // refusal and a later creation of the same pair can then be
                // compared instead of guessed at.
                tracing::warn!(
                    error = %e,
                    ?shaders,
                    vs_bytes = desc.VS.BytecodeLength,
                    ps_bytes = desc.PS.BytecodeLength,
                    root = desc.pRootSignature.is_some(),
                    node = desc.NodeMask,
                    flags = desc.Flags.0,
                    "host pipeline refused"
                );
                host.log_messages();
                return None;
            }
        };
        tracing::info!(
            ?shaders,
            vs_bytes = desc.VS.BytecodeLength,
            ps_bytes = desc.PS.BytecodeLength,
            root = desc.pRootSignature.is_some(),
            node = desc.NodeMask,
            flags = desc.Flags.0,
            "host pipeline"
        );
        crate::host::name(&pso, || format!("{shaders:?}"));
        tracing::debug!(
            ?shaders,
            depth = ?g.depth,
            dsv = g.depth_format.0,
            targets = ?&g.targets.RTFormats[..g.targets.NumRenderTargets.min(8) as usize],
            blend = ?g.blend.RenderTarget[0],
            cull = g.rasterizer.CullMode.0,
            "host graphics state"
        );
        Some(HostPipeline {
            compute: false,
            mesh: false,
            pso,
            root,
            stages: bound,
            name: format!("{shaders:?}"),
            key: key_used,
            slots: g.slots(),
            inputs: g.elements(),
            rects: rect_gs.is_some(),
            state: {
                let (d, r, b) = (&g.depth, &g.rasterizer, &g.blend.RenderTarget[0]);
                let face = |f: &D3D12_DEPTH_STENCILOP_DESC| {
                    format!(
                        "fail {} depth-fail {} pass {} func {}",
                        f.StencilFailOp.0, f.StencilDepthFailOp.0, f.StencilPassOp.0, f.StencilFunc.0
                    )
                };
                format!(
                    "depth {} write {} func {}; stencil {} read {:#x} write {:#x} front [{}] back [{}]; cull {} front-ccw {}; blend {} src {} dst {} op {} mask {:#x}",
                    d.DepthEnable.as_bool(),
                    d.DepthWriteMask.0,
                    d.DepthFunc.0,
                    d.StencilEnable.as_bool(),
                    d.StencilReadMask,
                    d.StencilWriteMask,
                    face(&d.FrontFace),
                    face(&d.BackFace),
                    r.CullMode.0,
                    r.FrontCounterClockwise.as_bool(),
                    b.BlendEnable.as_bool(),
                    b.SrcBlend.0,
                    b.DestBlend.0,
                    b.BlendOp.0,
                    b.RenderTargetWriteMask
                )
            },
        })
    }
}

/// A stream subobject: its type, then its description, pointer-aligned.
#[repr(C, align(8))]
struct Sub<T>(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE, T);

/// The graphics state in the pipeline-state stream form, which is the only way
/// to carry the depth bounds test: `D3D12_GRAPHICS_PIPELINE_STATE_DESC`'s
/// `DepthStencilState` is the plain `D3D12_DEPTH_STENCIL_DESC`, and the stream's
/// `DEPTH_STENCIL1` subobject is the one with `DepthBoundsTestEnable`.
#[repr(C)]
struct GraphicsStream {
    root: Sub<*mut std::ffi::c_void>,
    vs: Sub<D3D12_SHADER_BYTECODE>,
    ps: Sub<D3D12_SHADER_BYTECODE>,
    gs: Sub<D3D12_SHADER_BYTECODE>,
    blend: Sub<D3D12_BLEND_DESC>,
    mask: Sub<u32>,
    raster: Sub<D3D12_RASTERIZER_DESC>,
    depth: Sub<D3D12_DEPTH_STENCIL_DESC1>,
    layout: Sub<D3D12_INPUT_LAYOUT_DESC>,
    strip: Sub<D3D12_INDEX_BUFFER_STRIP_CUT_VALUE>,
    topology: Sub<D3D12_PRIMITIVE_TOPOLOGY_TYPE>,
    targets: Sub<D3D12_RT_FORMAT_ARRAY>,
    dsv: Sub<DXGI_FORMAT>,
    samples: Sub<DXGI_SAMPLE_DESC>,
}

impl GraphicsStream {
    fn of(desc: &D3D12_GRAPHICS_PIPELINE_STATE_DESC, root: *mut std::ffi::c_void) -> Self {
        let d = &desc.DepthStencilState;
        GraphicsStream {
            root: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE, root),
            vs: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS, desc.VS),
            ps: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS, desc.PS),
            gs: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_GS, desc.GS),
            blend: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND, desc.BlendState),
            mask: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_MASK, desc.SampleMask),
            raster: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER, desc.RasterizerState),
            depth: Sub(
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL1,
                D3D12_DEPTH_STENCIL_DESC1 {
                    DepthEnable: d.DepthEnable,
                    DepthWriteMask: d.DepthWriteMask,
                    DepthFunc: d.DepthFunc,
                    StencilEnable: d.StencilEnable,
                    StencilReadMask: d.StencilReadMask,
                    StencilWriteMask: d.StencilWriteMask,
                    FrontFace: d.FrontFace,
                    BackFace: d.BackFace,
                    DepthBoundsTestEnable: true.into(),
                },
            ),
            layout: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT, desc.InputLayout),
            strip: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_IB_STRIP_CUT_VALUE, desc.IBStripCutValue),
            topology: Sub(
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PRIMITIVE_TOPOLOGY,
                desc.PrimitiveTopologyType,
            ),
            targets: Sub(
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS,
                D3D12_RT_FORMAT_ARRAY {
                    RTFormats: desc.RTVFormats,
                    NumRenderTargets: desc.NumRenderTargets,
                },
            ),
            dsv: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT, desc.DSVFormat),
            samples: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_DESC, desc.SampleDesc),
        }
    }

    fn desc(&self) -> D3D12_PIPELINE_STATE_STREAM_DESC {
        D3D12_PIPELINE_STATE_STREAM_DESC {
            SizeInBytes: size_of::<GraphicsStream>(),
            // SAFETY: the stream outlives every use of this description.
            pPipelineStateSubobjectStream: (self as *const Self as *mut std::ffi::c_void),
        }
    }
}

/// Whether the host device runs the depth bounds test at all. A device that
/// does not is told so once, rather than a pipeline quietly passing at every
/// depth.
fn depth_bounds_supported(device: &ID3D12Device) -> bool {
    static SUPPORT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SUPPORT.get_or_init(|| {
        let mut options = D3D12_FEATURE_DATA_D3D12_OPTIONS2::default();
        // SAFETY: the size is the structure's own.
        let asked = unsafe {
            device.CheckFeatureSupport(
                D3D12_FEATURE_D3D12_OPTIONS2,
                (&raw mut options).cast(),
                size_of_val(&options) as u32,
            )
        };
        let supported = asked.is_ok() && options.DepthBoundsTestSupported.as_bool();
        if !supported {
            tracing::warn!(
                "the host device does not support the depth bounds test: those draws pass at every depth"
            );
        }
        supported
    })
}

/// A mesh pipeline from the title's graphics state: the pipeline-state
/// stream form (no input layout).
///
/// The stream names a triangle topology, which D3D12 does not need for a
/// mesh shader: without it the pipeline library refuses to load a stored
/// pipeline with the very stream it was made from (`E_INVALIDARG`; debug
/// layer message 972, "does not match the one used to create the PSO"), so
/// every mesh pipeline was compiled again, and its store then refused (970,
/// "already exists"). The translation makes triangle mesh shaders only
/// (`mesh_prologue` refuses others), and the debug layer wants the two to
/// agree (1323).
fn mesh_pipeline(
    device: &ID3D12Device,
    root: &ID3D12RootSignature,
    ms: D3D12_SHADER_BYTECODE,
    ps: D3D12_SHADER_BYTECODE,
    g: &GraphicsState,
    name: &str,
) -> windows::core::Result<ID3D12PipelineState> {
    use windows::core::Interface;
    #[repr(C)]
    struct Stream {
        root: Sub<*mut std::ffi::c_void>,
        ms: Sub<D3D12_SHADER_BYTECODE>,
        ps: Sub<D3D12_SHADER_BYTECODE>,
        blend: Sub<D3D12_BLEND_DESC>,
        mask: Sub<u32>,
        raster: Sub<D3D12_RASTERIZER_DESC>,
        depth: Sub<D3D12_DEPTH_STENCIL_DESC>,
        targets: Sub<D3D12_RT_FORMAT_ARRAY>,
        dsv: Sub<DXGI_FORMAT>,
        samples: Sub<DXGI_SAMPLE_DESC>,
        topology: Sub<D3D12_PRIMITIVE_TOPOLOGY_TYPE>,
    }
    let mut stream = Stream {
        root: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE, root.as_raw()),
        ms: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS, ms),
        ps: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS, ps),
        blend: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND, g.blend),
        mask: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_MASK, g.sample_mask),
        raster: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER, g.rasterizer),
        depth: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL, g.depth),
        targets: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS, g.targets),
        dsv: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT, g.depth_format),
        samples: Sub(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_DESC, g.samples),
        topology: Sub(
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PRIMITIVE_TOPOLOGY,
            D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
        ),
    };
    let desc = D3D12_PIPELINE_STATE_STREAM_DESC {
        SizeInBytes: size_of::<Stream>(),
        pPipelineStateSubobjectStream: (&raw mut stream).cast(),
    };
    let device = device.cast::<ID3D12Device2>()?;
    super::library::pipeline(
        name,
        "mesh",
        // SAFETY: the stream holds whole subobjects pointing at live data.
        |library, name| unsafe { library.LoadPipeline(name, &desc) },
        // SAFETY: as above.
        || unsafe { device.CreatePipelineState(&desc) },
    )
}

/// The bytes of shader code (none for an absent stage).
fn code_bytes(code: &D3D12_SHADER_BYTECODE) -> Vec<u8> {
    if code.pShaderBytecode.is_null() {
        return Vec::new();
    }
    // SAFETY: a translation's own bytes, alive while its shader is.
    unsafe { std::slice::from_raw_parts(code.pShaderBytecode.cast::<u8>(), code.BytecodeLength) }.to_vec()
}

/// The signature's flags: what the stages need (input layout, heaps), and
/// nothing more. Declared from the code, not from the parameters: a shader
/// that indexes a heap needs its flag (the validator refuses it otherwise),
/// and a device without the feature may refuse a flag whose heap no stage
/// touches.
fn heap_flags(
    input: D3D12_ROOT_SIGNATURE_FLAGS,
    heaps: Heaps,
) -> D3D12_ROOT_SIGNATURE_FLAGS {
    let mut flags = input;
    if heaps.views {
        flags |= D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED;
    }
    if heaps.samplers {
        flags |= D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED;
    }
    flags
}

fn root_signature(
    device: &ID3D12Device,
    params: &[D3D12_ROOT_PARAMETER1],
    samplers: &[D3D12_STATIC_SAMPLER_DESC],
    compute: bool,
    heaps: Heaps,
) -> windows::core::Result<(ID3D12RootSignature, Vec<u8>)> {
    let input = if compute {
        D3D12_ROOT_SIGNATURE_FLAG_NONE
    } else {
        D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT
    };
    let flags = heap_flags(input, heaps);
    let desc = D3D12_VERSIONED_ROOT_SIGNATURE_DESC {
        Version: windows::Win32::Graphics::Direct3D12::D3D_ROOT_SIGNATURE_VERSION_1_1,
        Anonymous: D3D12_VERSIONED_ROOT_SIGNATURE_DESC_0 {
            Desc_1_1: D3D12_ROOT_SIGNATURE_DESC1 {
                NumParameters: params.len() as u32,
                pParameters: params.as_ptr(),
                NumStaticSamplers: samplers.len() as u32,
                pStaticSamplers: samplers.as_ptr(),
                Flags: flags,
            },
        },
    };
    // SAFETY: the description points at live arrays; the blob is read in
    // full before it is released.
    unsafe {
        let mut blob: Option<ID3DBlob> = None;
        let mut error: Option<ID3DBlob> = None;
        if let Err(e) = D3D12SerializeVersionedRootSignature(&desc, &mut blob, Some(&mut error)) {
            if let Some(error) = error {
                let text = std::slice::from_raw_parts(
                    error.GetBufferPointer().cast::<u8>(),
                    error.GetBufferSize(),
                );
                tracing::warn!("root signature: {}", String::from_utf8_lossy(text));
            }
            return Err(e);
        }
        let blob = blob.ok_or_else(windows::core::Error::empty)?;
        let bytes =
            std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize());
        Ok((device.CreateRootSignature(0, bytes)?, bytes.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::Ordering;

    use super::*;

    /// A state written to a dump and read back describes the same pipeline, and
    /// the description is what a library entry is named by: an entry whose name
    /// differs by one field is an entry the title never finds.
    #[test]
    fn a_state_survives_its_fields() {
        let names = ["POSITION", "TEXCOORD", "odd name%with spaces"];
        let mut state = GraphicsState::default();
        state.names = names.iter().map(|n| CString::new(*n).unwrap()).collect();
        state.inputs = names
            .iter()
            .zip(&state.names)
            .enumerate()
            .map(|(i, (_, name))| D3D12_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR(name.as_ptr().cast()),
                SemanticIndex: i as u32,
                Format: DXGI_FORMAT(6 + i as i32),
                InputSlot: i as u32,
                AlignedByteOffset: D3D12_APPEND_ALIGNED_ELEMENT,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
                InstanceDataStepRate: i as u32 + 1,
            })
            .collect();
        state.blend.RenderTarget[2].BlendEnable = true.into();
        state.blend.RenderTarget[2].SrcBlend = D3D12_BLEND_SRC_ALPHA;
        state.blend.IndependentBlendEnable = true.into();
        state.rasterizer.DepthBias = -7;
        state.rasterizer.DepthBiasClamp = 0.125;
        state.rasterizer.SlopeScaledDepthBias = 3.5;
        state.rasterizer.CullMode = D3D12_CULL_MODE_NONE;
        state.depth.FrontFace.StencilFunc = D3D12_COMPARISON_FUNC_EQUAL;
        state.depth.StencilReadMask = 0x3f;
        state.sample_mask = 0x80;
        state.strip_cut = D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_0xFFFF;
        state.topology = D3D12_PRIMITIVE_TOPOLOGY_TYPE_PATCH;
        state.targets.NumRenderTargets = 3;
        state.targets.RTFormats[0] = DXGI_FORMAT_R8G8B8A8_UNORM;
        state.targets.RTFormats[2] = DXGI_FORMAT_R11G11B10_FLOAT;
        state.depth_format = DXGI_FORMAT_D32_FLOAT;
        state.samples = DXGI_SAMPLE_DESC { Count: 4, Quality: 1 };
        let fields = state.fields();
        let back = GraphicsState::from_fields(&fields).expect("the state it wrote, read back");
        assert_eq!(back.describe(), state.describe(), "fields: {fields}");
        // And what it wrote is what it wrote: the text does not drift.
        assert_eq!(back.fields(), fields);
    }

    /// The depth bounds test rides on the state: a pipeline that enables it is
    /// made from the stream form (the plain description has no field for it), so
    /// the flag has to survive the dump and has to be part of the name.
    #[test]
    fn the_depth_bounds_test_survives_its_fields() {
        let mut state = GraphicsState::default();
        let plain = state.describe();
        let plain_fields = state.fields();
        assert!(!plain.contains("bounds"), "a default state names no bounds: {plain}");
        // The optional field is absent when it is off, so every name a warm
        // library already holds stays the name it holds.
        assert_eq!(
            GraphicsState::from_fields(&plain_fields).unwrap().fields(),
            plain_fields
        );

        // The title's own enablement comes in as subobject 21.
        let desc = D3D12_DEPTH_STENCIL_DESC1 {
            DepthEnable: true.into(),
            DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ALL,
            DepthFunc: D3D12_COMPARISON_FUNC_LESS,
            StencilEnable: false.into(),
            StencilReadMask: 0,
            StencilWriteMask: 0,
            FrontFace: D3D12_DEPTH_STENCILOP_DESC::default(),
            BackFace: D3D12_DEPTH_STENCILOP_DESC::default(),
            DepthBoundsTestEnable: true.into(),
        };
        // SAFETY: the stream pointer the title would pass points at one
        // subobject; this passes one of its own.
        unsafe {
            let mut bytes = vec![0u8; 4 + size_of::<D3D12_DEPTH_STENCIL_DESC1>()];
            bytes[0..4].copy_from_slice(&21u32.to_le_bytes());
            std::ptr::copy_nonoverlapping(
                (&raw const desc).cast::<u8>(),
                bytes[4..].as_mut_ptr(),
                size_of::<D3D12_DEPTH_STENCIL_DESC1>(),
            );
            state.read(21, bytes.as_ptr());
        }
        assert!(state.depth.DepthEnable.as_bool());
        assert!(state.describe().ends_with(" bounds"), "{}", state.describe());
        assert_ne!(state.describe(), plain, "the name differs from a plain pipeline");
        let fields = state.fields();
        let back = GraphicsState::from_fields(&fields).expect("a bounds state reads back");
        assert_eq!(back.describe(), state.describe());
        assert_eq!(back.fields(), fields);
    }

    /// A line that is not exactly one state is refused rather than read as a
    /// state with defaulted fields: a silently defaulted field is a wrong name.
    #[test]
    fn a_cut_or_foreign_dump_line_is_refused() {
        let fields = GraphicsState::default().fields();
        assert!(GraphicsState::from_fields(&fields).is_some());
        assert!(GraphicsState::from_fields(&fields[..fields.len() - 3]).is_none());
        // A trailing token is the depth bounds field (0 or 1); anything else,
        // or a second token, is a foreign line.
        assert!(GraphicsState::from_fields(&format!("{fields} 2")).is_none());
        assert!(GraphicsState::from_fields(&format!("{fields} 0 0")).is_none());
        assert!(GraphicsState::from_fields(&format!("2{}", &fields[1..])).is_none());
        assert!(GraphicsState::from_fields("").is_none());
    }

    const MS: &str = "struct V { float4 p : SV_Position; };
        [outputtopology(\"triangle\")] [numthreads(1, 1, 1)]
        void main(out vertices V v[3], out indices uint3 t[1]) {
            SetMeshOutputCounts(3, 1);
            v[0].p = float4(0, 0, 0, 1);
            v[1].p = float4(1, 0, 0, 1);
            v[2].p = float4(0, 1, 0, 1);
            t[0] = uint3(0, 1, 2);
        }";
    const PS: &str = "float4 main() : SV_Target { return 1; }";

    /// The PC `dxc.exe` (`VICTORY_PC_DXC`, else the newest Windows SDK's).
    fn dxc() -> Option<PathBuf> {
        let dxc = match std::env::var_os("VICTORY_PC_DXC") {
            Some(p) => PathBuf::from(p),
            None => victory_shader::emit::validator::Validator::default_path()?
                .with_file_name("dxc.exe"),
        };
        dxc.exists().then_some(dxc)
    }

    fn compile(dxc: &Path, dir: &Path, src: &str, profile: &str) -> Vec<u8> {
        let (hlsl, out) = (
            dir.join(format!("{profile}.hlsl")),
            dir.join(format!("{profile}.dxil")),
        );
        std::fs::write(&hlsl, src).unwrap();
        let status = std::process::Command::new(dxc)
            .args(["-nologo", "-T", profile, "-E", "main", "-Fo"])
            .arg(&out)
            .arg(&hlsl)
            .status()
            .unwrap();
        assert!(status.success(), "dxc failed for {profile}");
        std::fs::read(out).unwrap()
    }

    fn code(bytes: &[u8]) -> D3D12_SHADER_BYTECODE {
        D3D12_SHADER_BYTECODE {
            pShaderBytecode: bytes.as_ptr().cast(),
            BytecodeLength: bytes.len(),
        }
    }

    /// A mesh pipeline made once is loaded from the library the next time,
    /// not compiled again (a stream without a topology never loads).
    #[test]
    fn a_mesh_pipeline_loads_from_the_library() {
        let Some(host) = host() else {
            return; // no host GPU here
        };
        if !host.mesh_shaders() {
            eprintln!("no host mesh shader support; skipped");
            return;
        }
        let Some(dxc) = dxc() else {
            eprintln!("no PC dxc.exe; skipped");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let ms = compile(&dxc, dir.path(), MS, "ms_6_5");
        let ps = compile(&dxc, dir.path(), PS, "ps_6_5");
        let (root, root_blob) =
            root_signature(&host.device, &[], &[], false, Heaps::all()).unwrap();
        let mut g = GraphicsState::default();
        // This pipeline has only a color target, with no depth buffer.
        g.depth.DepthEnable = false.into();
        g.targets.NumRenderTargets = 1;
        g.targets.RTFormats[0] = DXGI_FORMAT_R8G8B8A8_UNORM;
        let name = super::super::library::name("mesh test", &root_blob, &[&ms, &ps], &g.describe());
        mesh_pipeline(&host.device, &root, code(&ms), code(&ps), &g, &name).expect("made");
        let loaded = super::super::library::LOADED.load(Ordering::Relaxed);
        mesh_pipeline(&host.device, &root, code(&ms), code(&ps), &g, &name).expect("made again");
        assert!(
            super::super::library::LOADED.load(Ordering::Relaxed) > loaded,
            "the mesh pipeline was compiled again instead of loaded from the library"
        );
    }

    /// What the refusal log's fields are for: a compute description with no
    /// bytecode is refused by the device, so `cs_bytes = 0` on a refusal names
    /// an empty `CS` rather than a module the driver dislikes.
    #[test]
    fn a_compute_pipeline_with_no_bytecode_is_refused() {
        let Some(host) = host() else {
            return; // no host GPU here
        };
        let (root, _) = root_signature(&host.device, &[], &[], true, Heaps::all()).unwrap();
        let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            // SAFETY: a borrowed pointer the description never releases.
            pRootSignature: unsafe { std::mem::transmute_copy(&root) },
            CS: D3D12_SHADER_BYTECODE::default(),
            ..Default::default()
        };
        assert_eq!(desc.CS.BytecodeLength, 0, "the field the refusal log prints");
        // SAFETY: a description over live data; the call is expected to fail.
        let made = unsafe { host.device.CreateComputePipelineState::<ID3D12PipelineState>(&desc) };
        assert!(
            made.is_err(),
            "an empty CS must be refused, or cs_bytes = 0 on a refusal says nothing"
        );
    }

    /// A signature's heap flags follow its code, never its parameters: the
    /// bounded one-view tables `bounded_root_srvs()` emits are tables in a
    /// signature whose shaders index no heap, and such a signature must
    /// declare no heap.
    #[test]
    fn heap_flags_follow_the_code() {
        let table = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
        assert_eq!(
            heap_flags(D3D12_ROOT_SIGNATURE_FLAG_NONE, Heaps::default()).0,
            0,
            "a signature whose code indexes no heap declares none"
        );
        let views =
            heap_flags(D3D12_ROOT_SIGNATURE_FLAG_NONE, Heaps { views: true, samplers: false });
        assert_ne!(
            views.0 & D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED.0,
            0,
            "a shader that indexes the view heap needs its flag"
        );
        assert_eq!(
            views.0 & D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED.0,
            0,
            "and one that does not index the sampler heap does not declare it"
        );
        let both = heap_flags(
            D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
            Heaps::all(),
        );
        assert_ne!(
            both.0 & D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT.0,
            0,
            "the layout flag the caller asked for survives"
        );
        // The table parameter kind is what the bounded root SRVs emit; it
        // neither adds a heap flag nor removes one.
        let _ = table;
    }
}

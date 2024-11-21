//! This library describes the API surface of WebGPU that is agnostic of the backend.
//! This API is used for targeting both Web and Native.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![allow(
    // We don't use syntax sugar where it's not necessary.
    clippy::match_like_matches_macro,
)]
#![warn(clippy::ptr_as_ptr, missing_docs, unsafe_op_in_unsafe_fn)]

#[cfg(any(feature = "serde", test))]
use serde::Deserialize;
#[cfg(any(feature = "serde", test))]
use serde::Serialize;
use std::hash::{Hash, Hasher};
use std::mem::size_of;
use std::path::PathBuf;
use std::{num::NonZeroU32, ops::Range};

pub mod assertions;
mod counters;
pub mod math;

pub use counters::*;

// Use this macro instead of the one provided by the bitflags_serde_shim crate
// because the latter produces an error when deserializing bits that are not
// specified in the bitflags, while we want deserialization to succeed and
// and unspecified bits to lead to errors handled in wgpu-core.
// Note that plainly deriving Serialize and Deserialized would have a similar
// behavior to this macro (unspecified bit do not produce an error).
macro_rules! impl_bitflags {
    ($name:ident) => {
        #[cfg(feature = "serde")]
        impl serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                self.bits().serialize(serializer)
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<$name, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = <_ as serde::Deserialize<'de>>::deserialize(deserializer)?;
                Ok($name::from_bits_retain(value))
            }
        }

        impl $name {
            /// Returns true if the bitflags contains bits that are not part of
            /// the bitflags definition.
            #[must_use]
            pub fn contains_invalid_bits(&self) -> bool {
                let all = Self::all().bits();
                (self.bits() | all) != all
            }
        }
    };
}

/// Integral type used for buffer offsets.
pub type BufferAddress = u64;
/// Integral type used for buffer slice sizes.
pub type BufferSize = std::num::NonZeroU64;
/// Integral type used for binding locations in shaders.
pub type ShaderLocation = u32;
/// Integral type used for dynamic bind group offsets.
pub type DynamicOffset = u32;

/// Buffer-Texture copies must have [`bytes_per_row`] aligned to this number.
///
/// This doesn't apply to [`Queue::write_texture`][Qwt].
///
/// [`bytes_per_row`]: ImageDataLayout::bytes_per_row
/// [Qwt]: ../wgpu/struct.Queue.html#method.write_texture
pub const COPY_BYTES_PER_ROW_ALIGNMENT: u32 = 256;
/// An offset into the query resolve buffer has to be aligned to this.
pub const QUERY_RESOLVE_BUFFER_ALIGNMENT: BufferAddress = 256;
/// Buffer to buffer copy as well as buffer clear offsets and sizes must be aligned to this number.
pub const COPY_BUFFER_ALIGNMENT: BufferAddress = 4;
/// Size to align mappings.
pub const MAP_ALIGNMENT: BufferAddress = 8;
/// Vertex buffer strides have to be aligned to this number.
pub const VERTEX_STRIDE_ALIGNMENT: BufferAddress = 4;
/// Alignment all push constants need
pub const PUSH_CONSTANT_ALIGNMENT: u32 = 4;
/// Maximum queries in a query set
pub const QUERY_SET_MAX_QUERIES: u32 = 4096;
/// Size of a single piece of query data.
pub const QUERY_SIZE: u32 = 8;

/// Backends supported by wgpu.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Backend {
    /// Dummy backend, used for testing.
    Empty = 0,
    /// Vulkan API (Windows, Linux, Android, MacOS via `vulkan-portability`/MoltenVK)
    Vulkan = 1,
    /// Metal API (Apple platforms)
    Metal = 2,
    /// Direct3D-12 (Windows)
    Dx12 = 3,
    /// OpenGL 3.3+ (Windows), OpenGL ES 3.0+ (Linux, Android, MacOS via Angle), and WebGL2
    Gl = 4,
    /// WebGPU in the browser
    BrowserWebGpu = 5,
}

impl Backend {
    /// Returns the string name of the backend.
    #[must_use]
    pub const fn to_str(self) -> &'static str {
        match self {
            Backend::Empty => "empty",
            Backend::Vulkan => "vulkan",
            Backend::Metal => "metal",
            Backend::Dx12 => "dx12",
            Backend::Gl => "gl",
            Backend::BrowserWebGpu => "webgpu",
        }
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_str())
    }
}

/// Power Preference when choosing a physical adapter.
///
/// Corresponds to [WebGPU `GPUPowerPreference`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpupowerpreference).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum PowerPreference {
    #[default]
    /// Power usage is not considered when choosing an adapter.
    None = 0,
    /// Adapter that uses the least possible power. This is often an integrated GPU.
    LowPower = 1,
    /// Adapter that has the highest performance. This is often a discrete GPU.
    HighPerformance = 2,
}

bitflags::bitflags! {
    /// Represents the backends that wgpu will use.
    #[repr(transparent)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
    pub struct Backends: u32 {
        /// Supported on Windows, Linux/Android, and macOS/iOS via Vulkan Portability (with the Vulkan feature enabled)
        const VULKAN = 1 << Backend::Vulkan as u32;
        /// Supported on Linux/Android, the web through webassembly via WebGL, and Windows and
        /// macOS/iOS via ANGLE
        const GL = 1 << Backend::Gl as u32;
        /// Supported on macOS/iOS
        const METAL = 1 << Backend::Metal as u32;
        /// Supported on Windows 10 and later
        const DX12 = 1 << Backend::Dx12 as u32;
        /// Supported when targeting the web through webassembly with the `webgpu` feature enabled.
        ///
        /// The WebGPU backend is special in several ways:
        /// It is not not implemented by `wgpu_core` and instead by the higher level `wgpu` crate.
        /// Whether WebGPU is targeted is decided upon the creation of the `wgpu::Instance`,
        /// *not* upon adapter creation. See `wgpu::Instance::new`.
        const BROWSER_WEBGPU = 1 << Backend::BrowserWebGpu as u32;
        /// All the apis that wgpu offers first tier of support for.
        ///
        /// * [`Backends::VULKAN`]
        /// * [`Backends::METAL`]
        /// * [`Backends::DX12`]
        /// * [`Backends::BROWSER_WEBGPU`]
        const PRIMARY = Self::VULKAN.bits()
            | Self::METAL.bits()
            | Self::DX12.bits()
            | Self::BROWSER_WEBGPU.bits();
        /// All the apis that wgpu offers second tier of support for. These may
        /// be unsupported/still experimental.
        ///
        /// * [`Backends::GL`]
        const SECONDARY = Self::GL.bits();
    }
}

impl Default for Backends {
    fn default() -> Self {
        Self::all()
    }
}

impl_bitflags!(Backends);

impl From<Backend> for Backends {
    fn from(backend: Backend) -> Self {
        Self::from_bits(1 << backend as u32).unwrap()
    }
}

/// Options for requesting adapter.
///
/// Corresponds to [WebGPU `GPURequestAdapterOptions`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpurequestadapteroptions).
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct RequestAdapterOptions<S> {
    /// Power preference for the adapter.
    pub power_preference: PowerPreference,
    /// Indicates that only a fallback adapter can be returned. This is generally a "software"
    /// implementation on the system.
    pub force_fallback_adapter: bool,
    /// Surface that is required to be presentable with the requested adapter. This does not
    /// create the surface, only guarantees that the adapter can present to said surface.
    pub compatible_surface: Option<S>,
}

impl<S> Default for RequestAdapterOptions<S> {
    fn default() -> Self {
        Self {
            power_preference: PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: None,
        }
    }
}

//TODO: make robust resource access configurable

bitflags::bitflags! {
    /// Features that are not guaranteed to be supported.
    ///
    /// These are either part of the webgpu standard, or are extension features supported by
    /// wgpu when targeting native.
    ///
    /// If you want to use a feature, you need to first verify that the adapter supports
    /// the feature. If the adapter does not support the feature, requesting a device with it enabled
    /// will panic.
    ///
    /// Corresponds to [WebGPU `GPUFeatureName`](
    /// https://gpuweb.github.io/gpuweb/#enumdef-gpufeaturename).
    #[repr(transparent)]
    #[derive(Default)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
    pub struct Features: u64 {
        //
        // ---- Start numbering at 1 << 0 ----
        //
        // WebGPU features:
        //

        // API:

        /// By default, polygon depth is clipped to 0-1 range before/during rasterization.
        /// Anything outside of that range is rejected, and respective fragments are not touched.
        ///
        /// With this extension, we can disabling clipping. That allows
        /// shadow map occluders to be rendered into a tighter depth range.
        ///
        /// Supported platforms:
        /// - desktops
        /// - some mobile chips
        ///
        /// This is a web and native feature.
        const DEPTH_CLIP_CONTROL = 1 << 0;

        /// Allows for explicit creation of textures of format [`TextureFormat::Depth32FloatStencil8`]
        ///
        /// Supported platforms:
        /// - Vulkan (mostly)
        /// - DX12
        /// - Metal
        /// - OpenGL
        ///
        /// This is a web and native feature.
        const DEPTH32FLOAT_STENCIL8 = 1 << 1;

        /// Enables BCn family of compressed textures. All BCn textures use 4x4 pixel blocks
        /// with 8 or 16 bytes per block.
        ///
        /// Compressed textures sacrifice some quality in exchange for significantly reduced
        /// bandwidth usage.
        ///
        /// Support for this feature guarantees availability of [`TextureUsages::COPY_SRC | TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING`] for BCn formats.
        /// [`Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`] may enable additional usages.
        ///
        /// This feature guarantees availability of sliced-3d textures for BC formats when combined with TEXTURE_COMPRESSION_BC_SLICED_3D.
        ///
        /// Supported Platforms:
        /// - desktops
        /// - Mobile (All Apple9 and some Apple7 and Apple8 devices)
        ///
        /// This is a web and native feature.
        const TEXTURE_COMPRESSION_BC = 1 << 2;


        /// Allows the 3d dimension for textures with BC compressed formats.
        ///
        /// This feature must be used in combination with TEXTURE_COMPRESSION_BC to enable 3D textures with BC compression.
        /// It does not enable the BC formats by itself.
        ///
        /// Supported Platforms:
        /// - desktops
        /// - Mobile (All Apple9 and some Apple7 and Apple8 devices)
        ///
        /// This is a web and native feature.
        const TEXTURE_COMPRESSION_BC_SLICED_3D = 1 << 3;

        /// Enables ETC family of compressed textures. All ETC textures use 4x4 pixel blocks.
        /// ETC2 RGB and RGBA1 are 8 bytes per block. RTC2 RGBA8 and EAC are 16 bytes per block.
        ///
        /// Compressed textures sacrifice some quality in exchange for significantly reduced
        /// bandwidth usage.
        ///
        /// Support for this feature guarantees availability of [`TextureUsages::COPY_SRC | TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING`] for ETC2 formats.
        /// [`Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`] may enable additional usages.
        ///
        /// Supported Platforms:
        /// - Vulkan on Intel
        /// - Mobile (some)
        ///
        /// This is a web and native feature.
        const TEXTURE_COMPRESSION_ETC2 = 1 << 4;

        /// Enables ASTC family of compressed textures. ASTC textures use pixel blocks varying from 4x4 to 12x12.
        /// Blocks are always 16 bytes.
        ///
        /// Compressed textures sacrifice some quality in exchange for significantly reduced
        /// bandwidth usage.
        ///
        /// Support for this feature guarantees availability of [`TextureUsages::COPY_SRC | TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING`] for ASTC formats with Unorm/UnormSrgb channel type.
        /// [`Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`] may enable additional usages.
        ///
        /// Supported Platforms:
        /// - Vulkan on Intel
        /// - Mobile (some)
        ///
        /// This is a web and native feature.
        const TEXTURE_COMPRESSION_ASTC = 1 << 5;

        /// Enables use of Timestamp Queries. These queries tell the current gpu timestamp when
        /// all work before the query is finished.
        ///
        /// This feature allows the use of
        /// - [`RenderPassDescriptor::timestamp_writes`]
        /// - [`ComputePassDescriptor::timestamp_writes`]
        /// to write out timestamps.
        ///
        /// For arbitrary timestamp write commands on encoders refer to [`Features::TIMESTAMP_QUERY_INSIDE_ENCODERS`].
        /// For arbitrary timestamp write commands on passes refer to [`Features::TIMESTAMP_QUERY_INSIDE_PASSES`].
        ///
        /// They must be resolved using [`CommandEncoder::resolve_query_set`] into a buffer,
        /// then the result must be multiplied by the timestamp period [`Queue::get_timestamp_period`]
        /// to get the timestamp in nanoseconds. Multiple timestamps can then be diffed to get the
        /// time for operations between them to finish.
        ///
        /// Supported Platforms:
        /// - Vulkan
        /// - DX12
        /// - Metal
        ///
        /// This is a web and native feature.
        const TIMESTAMP_QUERY = 1 << 6;

        /// Allows non-zero value for the `first_instance` member in indirect draw calls.
        ///
        /// If this feature is not enabled, and the `first_instance` member is non-zero, the behavior may be:
        /// - The draw call is ignored.
        /// - The draw call is executed as if the `first_instance` is zero.
        /// - The draw call is executed with the correct `first_instance` value.
        ///
        /// Supported Platforms:
        /// - Vulkan (mostly)
        /// - DX12
        /// - Metal on Apple3+ or Mac1+
        /// - OpenGL (Desktop 4.2+ with ARB_shader_draw_parameters only)
        ///
        /// Not Supported:
        /// - OpenGL ES / WebGL
        ///
        /// This is a web and native feature.
        const INDIRECT_FIRST_INSTANCE = 1 << 7;

        /// Allows shaders to acquire the FP16 ability
        ///
        /// Note: this is not supported in `naga` yet, only through `spirv-passthrough` right now.
        ///
        /// Supported Platforms:
        /// - Vulkan
        /// - Metal
        ///
        /// This is a web and native feature.
        const SHADER_F16 = 1 << 8;


        /// Allows for usage of textures of format [`TextureFormat::Rg11b10Ufloat`] as a render target
        ///
        /// Supported platforms:
        /// - Vulkan
        /// - DX12
        /// - Metal
        ///
        /// This is a web and native feature.
        const RG11B10UFLOAT_RENDERABLE = 1 << 9;

        /// Allows the [`wgpu::TextureUsages::STORAGE_BINDING`] usage on textures with format [`TextureFormat::Bgra8unorm`]
        ///
        /// Supported Platforms:
        /// - Vulkan
        /// - DX12
        /// - Metal
        ///
        /// This is a web and native feature.
        const BGRA8UNORM_STORAGE = 1 << 10;


        /// Allows textures with formats "r32float", "rg32float", and "rgba32float" to be filterable.
        ///
        /// Supported Platforms:
        /// - Vulkan (mainly on Desktop GPUs)
        /// - DX12
        /// - Metal on macOS or Apple9+ GPUs, optional on iOS/iPadOS with Apple7/8 GPUs
        /// - GL with one of `GL_ARB_color_buffer_float`/`GL_EXT_color_buffer_float`/`OES_texture_float_linear`
        ///
        /// This is a web and native feature.
        const FLOAT32_FILTERABLE = 1 << 11;

        // Bits 12-19 available for webgpu features. Should you chose to use some of them for
        // for native features, don't forget to update `all_webgpu_mask` and `all_native_mask`
        // accordingly.

        //
        // ---- Restart Numbering for Native Features ---
        //
        // Native Features:
        //

        // The features starting with a ? are features that might become part of the spec or
        // at the very least we can implement as native features; since they should cover all
        // possible formats and capabilities across backends.
        //
        // ? const FORMATS_TIER_1 = 1 << ??; (https://github.com/gpuweb/gpuweb/issues/3837)
        // ? const RW_STORAGE_TEXTURE_TIER_1 = 1 << ??; (https://github.com/gpuweb/gpuweb/issues/3838)
        // ? const NORM16_FILTERABLE = 1 << ??; (https://github.com/gpuweb/gpuweb/issues/3839)
        // ? const NORM16_RESOLVE = 1 << ??; (https://github.com/gpuweb/gpuweb/issues/3839)
        // ? const FLOAT32_BLENDABLE = 1 << ??; (https://github.com/gpuweb/gpuweb/issues/3556)
        // ? const 32BIT_FORMAT_MULTISAMPLE = 1 << ??; (https://github.com/gpuweb/gpuweb/issues/3844)
        // ? const 32BIT_FORMAT_RESOLVE = 1 << ??; (https://github.com/gpuweb/gpuweb/issues/3844)
        // ? const TEXTURE_COMPRESSION_ASTC_HDR = 1 << ??; (https://github.com/gpuweb/gpuweb/issues/3856)
        // TEXTURE_FORMAT_16BIT_NORM & TEXTURE_COMPRESSION_ASTC_HDR will most likely become web features as well
        // TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES might not be necessary if we have all the texture features implemented

        // Texture Formats:

        /// Enables normalized `16-bit` texture formats.
        ///
        /// Supported platforms:
        /// - Vulkan
        /// - DX12
        /// - Metal
        ///
        /// This is a native only feature.
        const TEXTURE_FORMAT_16BIT_NORM = 1 << 20;
        /// Enables ASTC HDR family of compressed textures.
        ///
        /// Compressed textures sacrifice some quality in exchange for significantly reduced
        /// bandwidth usage.
        ///
        /// Support for this feature guarantees availability of [`TextureUsages::COPY_SRC | TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING`] for ASTC formats with the HDR channel type.
        /// [`Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`] may enable additional usages.
        ///
        /// Supported Platforms:
        /// - Metal
        /// - Vulkan
        /// - OpenGL
        ///
        /// This is a native only feature.
        const TEXTURE_COMPRESSION_ASTC_HDR = 1 << 21;
        /// Enables device specific texture format features.
        ///
        /// See `TextureFormatFeatures` for a listing of the features in question.
        ///
        /// By default only texture format properties as defined by the WebGPU specification are allowed.
        /// Enabling this feature flag extends the features of each format to the ones supported by the current device.
        /// Note that without this flag, read/write storage access is not allowed at all.
        ///
        /// This extension does not enable additional formats.
        ///
        /// This is a native only feature.
        const TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES = 1 << 22;

        // API:

        /// Enables use of Pipeline Statistics Queries. These queries tell the count of various operations
        /// performed between the start and stop call. Call [`RenderPass::begin_pipeline_statistics_query`] to start
        /// a query, then call [`RenderPass::end_pipeline_statistics_query`] to stop one.
        ///
        /// They must be resolved using [`CommandEncoder::resolve_query_set`] into a buffer.
        /// The rules on how these resolve into buffers are detailed in the documentation for [`PipelineStatisticsTypes`].
        ///
        /// Supported Platforms:
        /// - Vulkan
        /// - DX12
        ///
        /// This is a native only feature with a [proposal](https://github.com/gpuweb/gpuweb/blob/0008bd30da2366af88180b511a5d0d0c1dffbc36/proposals/pipeline-statistics-query.md) for the web.
        const PIPELINE_STATISTICS_QUERY = 1 << 23;
        /// Allows for timestamp queries directly on command encoders.
        ///
        /// Implies [`Features::TIMESTAMP_QUERY`] is supported.
        ///
        /// Additionally allows for timestamp writes on command encoders
        /// using  [`CommandEncoder::write_timestamp`].
        ///
        /// Supported platforms:
        /// - Vulkan
        /// - DX12
        /// - Metal
        ///
        /// This is a native only feature.
        const TIMESTAMP_QUERY_INSIDE_ENCODERS = 1 << 24;
        /// Allows for timestamp queries directly on command encoders.
        ///
        /// Implies [`Features::TIMESTAMP_QUERY`] & [`Features::TIMESTAMP_QUERY_INSIDE_ENCODERS`] is supported.
        ///
        /// Additionally allows for timestamp queries to be used inside render & compute passes using:
        /// - [`RenderPass::write_timestamp`]
        /// - [`ComputePass::write_timestamp`]
        ///
        /// Supported platforms:
        /// - Vulkan
        /// - DX12
        /// - Metal (AMD & Intel, not Apple GPUs)
        ///
        /// This is generally not available on tile-based rasterization GPUs.
        ///
        /// This is a native only feature with a [proposal](https://github.com/gpuweb/gpuweb/blob/0008bd30da2366af88180b511a5d0d0c1dffbc36/proposals/timestamp-query-inside-passes.md) for the web.
        const TIMESTAMP_QUERY_INSIDE_PASSES = 1 << 25;
        /// Webgpu only allows the MAP_READ and MAP_WRITE buffer usage to be matched with
        /// COPY_DST and COPY_SRC respectively. This removes this requirement.
        ///
        /// This is only beneficial on systems that share memory between CPU and GPU. If enabled
        /// on a system that doesn't, this can severely hinder performance. Only use if you understand
        /// the consequences.
        ///
        /// Supported platforms:
        /// - Vulkan
        /// - DX12
        /// - Metal
        ///
        /// This is a native only feature.
        const MAPPABLE_PRIMARY_BUFFERS = 1 << 26;
        /// Allows the user to create uniform arrays of textures in shaders:
        ///
        /// ex.
        /// - `var textures: binding_array<texture_2d<f32>, 10>` (WGSL)
        /// - `uniform texture2D textures[10]` (GLSL)
        ///
        /// If [`Features::STORAGE_RESOURCE_BINDING_ARRAY`] is supported as well as this, the user
        /// may also create uniform arrays of storage textures.
        ///
        /// ex.
        /// - `var textures: array<texture_storage_2d<r32float, write>, 10>` (WGSL)
        /// - `uniform image2D textures[10]` (GLSL)
        ///
        /// This capability allows them to exist and to be indexed by dynamically uniform
        /// values.
        ///
        /// Supported platforms:
        /// - DX12
        /// - Metal (with MSL 2.0+ on macOS 10.13+)
        /// - Vulkan
        ///
        /// This is a native only feature.
        const TEXTURE_BINDING_ARRAY = 1 << 27;
        /// Allows the user to create arrays of buffers in shaders:
        ///
        /// ex.
        /// - `var<uniform> buffer_array: array<MyBuffer, 10>` (WGSL)
        /// - `uniform myBuffer { ... } buffer_array[10]` (GLSL)
        ///
        /// This capability allows them to exist and to be indexed by dynamically uniform
        /// values.
        ///
        /// If [`Features::STORAGE_RESOURCE_BINDING_ARRAY`] is supported as well as this, the user
        /// may also create arrays of storage buffers.
        ///
        /// ex.
        /// - `var<storage> buffer_array: array<MyBuffer, 10>` (WGSL)
        /// - `buffer myBuffer { ... } buffer_array[10]` (GLSL)
        ///
        /// Supported platforms:
        /// - DX12
        /// - Vulkan
        ///
        /// This is a native only feature.
        const BUFFER_BINDING_ARRAY = 1 << 28;
        /// Allows the user to create uniform arrays of storage buffers or textures in shaders,
        /// if resp. [`Features::BUFFER_BINDING_ARRAY`] or [`Features::TEXTURE_BINDING_ARRAY`]
        /// is supported.
        ///
        /// This capability allows them to exist and to be indexed by dynamically uniform
        /// values.
        ///
        /// Supported platforms:
        /// - Metal (with MSL 2.2+ on macOS 10.13+)
        /// - Vulkan
        ///
        /// This is a native only feature.
        const STORAGE_RESOURCE_BINDING_ARRAY = 1 << 29;
        /// Allows shaders to index sampled texture and storage buffer resource arrays with dynamically non-uniform values:
        ///
        /// ex. `texture_array[vertex_data]`
        ///
        /// In order to use this capability, the corresponding GLSL extension must be enabled like so:
        ///
        /// `#extension GL_EXT_nonuniform_qualifier : require`
        ///
        /// and then used either as `nonuniformEXT` qualifier in variable declaration:
        ///
        /// ex. `layout(location = 0) nonuniformEXT flat in int vertex_data;`
        ///
        /// or as `nonuniformEXT` constructor:
        ///
        /// ex. `texture_array[nonuniformEXT(vertex_data)]`
        ///
        /// WGSL and HLSL do not need any extension.
        ///
        /// Supported platforms:
        /// - DX12
        /// - Metal (with MSL 2.0+ on macOS 10.13+)
        /// - Vulkan 1.2+ (or VK_EXT_descriptor_indexing)'s shaderSampledImageArrayNonUniformIndexing & shaderStorageBufferArrayNonUniformIndexing feature)
        ///
        /// This is a native only feature.
        const SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING = 1 << 30;
        /// Allows shaders to index uniform buffer and storage texture resource arrays with dynamically non-uniform values:
        ///
        /// ex. `texture_array[vertex_data]`
        ///
        /// In order to use this capability, the corresponding GLSL extension must be enabled like so:
        ///
        /// `#extension GL_EXT_nonuniform_qualifier : require`
        ///
        /// and then used either as `nonuniformEXT` qualifier in variable declaration:
        ///
        /// ex. `layout(location = 0) nonuniformEXT flat in int vertex_data;`
        ///
        /// or as `nonuniformEXT` constructor:
        ///
        /// ex. `texture_array[nonuniformEXT(vertex_data)]`
        ///
        /// WGSL and HLSL do not need any extension.
        ///
        /// Supported platforms:
        /// - DX12
        /// - Metal (with MSL 2.0+ on macOS 10.13+)
        /// - Vulkan 1.2+ (or VK_EXT_descriptor_indexing)'s shaderUniformBufferArrayNonUniformIndexing & shaderStorageTextureArrayNonUniformIndexing feature)
        ///
        /// This is a native only feature.
        const UNIFORM_BUFFER_AND_STORAGE_TEXTURE_ARRAY_NON_UNIFORM_INDEXING = 1 << 31;
        /// Allows the user to create bind groups containing arrays with less bindings than the BindGroupLayout.
        ///
        /// This is a native only feature.
        const PARTIALLY_BOUND_BINDING_ARRAY = 1 << 32;
        /// Allows the user to call [`RenderPass::multi_draw_indirect`] and [`RenderPass::multi_draw_indexed_indirect`].
        ///
        /// Allows multiple indirect calls to be dispatched from a single buffer.
        ///
        /// Supported platforms:
        /// - DX12
        /// - Vulkan
        /// - Metal on Apple3+ or Mac1+ (Emulated on top of `draw_indirect` and `draw_indexed_indirect`)
        ///
        /// This is a native only feature.
        ///
        /// [`RenderPass::multi_draw_indirect`]: ../wgpu/struct.RenderPass.html#method.multi_draw_indirect
        /// [`RenderPass::multi_draw_indexed_indirect`]: ../wgpu/struct.RenderPass.html#method.multi_draw_indexed_indirect
        const MULTI_DRAW_INDIRECT = 1 << 33;
        /// Allows the user to call [`RenderPass::multi_draw_indirect_count`] and [`RenderPass::multi_draw_indexed_indirect_count`].
        ///
        /// This allows the use of a buffer containing the actual number of draw calls.
        ///
        /// Supported platforms:
        /// - DX12
        /// - Vulkan 1.2+ (or VK_KHR_draw_indirect_count)
        ///
        /// This is a native only feature.
        ///
        /// [`RenderPass::multi_draw_indirect_count`]: ../wgpu/struct.RenderPass.html#method.multi_draw_indirect_count
        /// [`RenderPass::multi_draw_indexed_indirect_count`]: ../wgpu/struct.RenderPass.html#method.multi_draw_indexed_indirect_count
        const MULTI_DRAW_INDIRECT_COUNT = 1 << 34;
        /// Allows the use of push constants: small, fast bits of memory that can be updated
        /// inside a [`RenderPass`].
        ///
        /// Allows the user to call [`RenderPass::set_push_constants`], provide a non-empty array
        /// to [`PipelineLayoutDescriptor`], and provide a non-zero limit to [`Limits::max_push_constant_size`].
        ///
        /// A block of push constants can be declared in WGSL with `var<push_constant>`:
        ///
        /// ```rust,ignore
        /// struct PushConstants { example: f32, }
        /// var<push_constant> c: PushConstants;
        /// ```
        ///
        /// In GLSL, this corresponds to `layout(push_constant) uniform Name {..}`.
        ///
        /// Supported platforms:
        /// - DX12
        /// - Vulkan
        /// - Metal
        /// - OpenGL (emulated with uniforms)
        ///
        /// This is a native only feature.
        ///
        /// [`RenderPass`]: ../wgpu/struct.RenderPass.html
        /// [`PipelineLayoutDescriptor`]: ../wgpu/struct.PipelineLayoutDescriptor.html
        /// [`RenderPass::set_push_constants`]: ../wgpu/struct.RenderPass.html#method.set_push_constants
        const PUSH_CONSTANTS = 1 << 35;
        /// Allows the use of [`AddressMode::ClampToBorder`] with a border color
        /// of [`SamplerBorderColor::Zero`].
        ///
        /// Supported platforms:
        /// - DX12
        /// - Vulkan
        /// - Metal
        /// - OpenGL
        ///
        /// This is a native only feature.
        const ADDRESS_MODE_CLAMP_TO_ZERO = 1 << 36;
        /// Allows the use of [`AddressMode::ClampToBorder`] with a border color
        /// other than [`SamplerBorderColor::Zero`].
        ///
        /// Supported platforms:
        /// - DX12
        /// - Vulkan
        /// - Metal (macOS 10.12+ only)
        /// - OpenGL
        ///
        /// This is a native only feature.
        const ADDRESS_MODE_CLAMP_TO_BORDER = 1 << 37;
        /// Allows the user to set [`PolygonMode::Line`] in [`PrimitiveState::polygon_mode`]
        ///
        /// This allows drawing polygons/triangles as lines (wireframe) instead of filled
        ///
        /// Supported platforms:
        /// - DX12
        /// - Vulkan
        /// - Metal
        ///
        /// This is a native only feature.
        const POLYGON_MODE_LINE = 1 << 38;
        /// Allows the user to set [`PolygonMode::Point`] in [`PrimitiveState::polygon_mode`]
        ///
        /// This allows only drawing the vertices of polygons/triangles instead of filled
        ///
        /// Supported platforms:
        /// - Vulkan
        ///
        /// This is a native only feature.
        const POLYGON_MODE_POINT = 1 << 39;
        /// Allows the user to set a overestimation-conservative-rasterization in [`PrimitiveState::conservative`]
        ///
        /// Processing of degenerate triangles/lines is hardware specific.
        /// Only triangles are supported.
        ///
        /// Supported platforms:
        /// - Vulkan
        ///
        /// This is a native only feature.
        const CONSERVATIVE_RASTERIZATION = 1 << 40;
        /// Enables bindings of writable storage buffers and textures visible to vertex shaders.
        ///
        /// Note: some (tiled-based) platforms do not support vertex shaders with any side-effects.
        ///
        /// Supported Platforms:
        /// - All
        ///
        /// This is a native only feature.
        const VERTEX_WRITABLE_STORAGE = 1 << 41;
        /// Enables clear to zero for textures.
        ///
        /// Supported platforms:
        /// - All
        ///
        /// This is a native only feature.
        const CLEAR_TEXTURE = 1 << 42;
        /// Enables creating shader modules from SPIR-V binary data (unsafe).
        ///
        /// SPIR-V data is not parsed or interpreted in any way; you can use
        /// [`wgpu::make_spirv_raw!`] to check for alignment and magic number when converting from
        /// raw bytes.
        ///
        /// Supported platforms:
        /// - Vulkan, in case shader's requested capabilities and extensions agree with
        /// Vulkan implementation.
        ///
        /// This is a native only feature.
        const SPIRV_SHADER_PASSTHROUGH = 1 << 43;
        /// Enables multiview render passes and `builtin(view_index)` in vertex shaders.
        ///
        /// Supported platforms:
        /// - Vulkan
        /// - OpenGL (web only)
        ///
        /// This is a native only feature.
        const MULTIVIEW = 1 << 44;
        /// Enables using 64-bit types for vertex attributes.
        ///
        /// Requires SHADER_FLOAT64.
        ///
        /// Supported Platforms: N/A
        ///
        /// This is a native only feature.
        const VERTEX_ATTRIBUTE_64BIT = 1 << 45;
        /// Allows for creation of textures of format [`TextureFormat::NV12`]
        ///
        /// Supported platforms:
        /// - DX12
        /// - Vulkan
        ///
        /// This is a native only feature.
        const TEXTURE_FORMAT_NV12 = 1 << 47;
        /// ***THIS IS EXPERIMENTAL:*** Features enabled by this may have
        /// major bugs in them and are expected to be subject to breaking changes, suggestions
        /// for the API exposed by this should be posted on [the ray-tracing issue](https://github.com/gfx-rs/wgpu/issues/1040)
        ///
        /// Allows for the creation of ray-tracing acceleration structures. Currently,
        /// ray-tracing acceleration structures are only useful when used with [Features::EXPERIMENTAL_RAY_QUERY]
        ///
        /// Supported platforms:
        /// - Vulkan
        ///
        /// This is a native-only feature.
        const EXPERIMENTAL_RAY_TRACING_ACCELERATION_STRUCTURE = 1 << 48;

        // Shader:

        /// ***THIS IS EXPERIMENTAL:*** Features enabled by this may have
        /// major bugs in it and are expected to be subject to breaking changes, suggestions
        /// for the API exposed by this should be posted on [the ray-tracing issue](https://github.com/gfx-rs/wgpu/issues/1040)
        ///
        /// Allows for the creation of ray-tracing queries within shaders.
        ///
        /// Supported platforms:
        /// - Vulkan
        ///
        /// This is a native-only feature.
        const EXPERIMENTAL_RAY_QUERY = 1 << 49;
        /// Enables 64-bit floating point types in SPIR-V shaders.
        ///
        /// Note: even when supported by GPU hardware, 64-bit floating point operations are
        /// frequently between 16 and 64 _times_ slower than equivalent operations on 32-bit floats.
        ///
        /// Supported Platforms:
        /// - Vulkan
        ///
        /// This is a native only feature.
        const SHADER_F64 = 1 << 50;
        /// Allows shaders to use i16. Not currently supported in `naga`, only available through `spirv-passthrough`.
        ///
        /// Supported platforms:
        /// - Vulkan
        ///
        /// This is a native only feature.
        const SHADER_I16 = 1 << 51;
        /// Enables `builtin(primitive_index)` in fragment shaders.
        ///
        /// Note: enables geometry processing for pipelines using the builtin.
        /// This may come with a significant performance impact on some hardware.
        /// Other pipelines are not affected.
        ///
        /// Supported platforms:
        /// - Vulkan
        /// - DX12
        /// - Metal (some)
        /// - OpenGL (some)
        ///
        /// This is a native only feature.
        const SHADER_PRIMITIVE_INDEX = 1 << 52;
        /// Allows shaders to use the `early_depth_test` attribute.
        ///
        /// Supported platforms:
        /// - GLES 3.1+
        ///
        /// This is a native only feature.
        const SHADER_EARLY_DEPTH_TEST = 1 << 53;
        /// Allows two outputs from a shader to be used for blending.
        /// Note that dual-source blending doesn't support multiple render targets.
        ///
        /// For more info see the OpenGL ES extension GL_EXT_blend_func_extended.
        ///
        /// Supported platforms:
        /// - OpenGL ES (with GL_EXT_blend_func_extended)
        /// - Metal (with MSL 1.2+)
        /// - Vulkan (with dualSrcBlend)
        /// - DX12
        const DUAL_SOURCE_BLENDING = 1 << 54;
        /// Allows shaders to use i64 and u64.
        ///
        /// Supported platforms:
        /// - Vulkan
        /// - DX12 (DXC only)
        /// - Metal (with MSL 2.3+)
        ///
        /// This is a native only feature.
        const SHADER_INT64 = 1 << 55;
        /// Allows compute and fragment shaders to use the subgroup operation built-ins
        ///
        /// Supported Platforms:
        /// - Vulkan
        /// - DX12
        /// - Metal
        ///
        /// This is a native only feature.
        const SUBGROUP = 1 << 56;
        /// Allows vertex shaders to use the subgroup operation built-ins
        ///
        /// Supported Platforms:
        /// - Vulkan
        ///
        /// This is a native only feature.
        const SUBGROUP_VERTEX = 1 << 57;
        /// Allows shaders to use the subgroup barrier
        ///
        /// Supported Platforms:
        /// - Vulkan
        /// - Metal
        ///
        /// This is a native only feature.
        const SUBGROUP_BARRIER = 1 << 58;
        /// Allows the use of pipeline cache objects
        ///
        /// Supported platforms:
        /// - Vulkan
        ///
        /// Unimplemented Platforms:
        /// - DX12
        /// - Metal
        const PIPELINE_CACHE = 1 << 59;
        /// Allows shaders to use i64 and u64 atomic min and max.
        ///
        /// Supported platforms:
        /// - Vulkan (with VK_KHR_shader_atomic_int64)
        /// - DX12 (with SM 6.6+)
        /// - Metal (with MSL 2.4+)
        ///
        /// This is a native only feature.
        const SHADER_INT64_ATOMIC_MIN_MAX = 1 << 60;
        /// Allows shaders to use all i64 and u64 atomic operations.
        ///
        /// Supported platforms:
        /// - Vulkan (with VK_KHR_shader_atomic_int64)
        /// - DX12 (with SM 6.6+)
        ///
        /// This is a native only feature.
        const SHADER_INT64_ATOMIC_ALL_OPS = 1 << 61;
        /// Allows using the [VK_GOOGLE_display_timing] Vulkan extension.
        ///
        /// This is used for frame pacing to reduce latency, and is generally only available on Android.
        ///
        /// This feature does not have a `wgpu`-level API, and so users of wgpu wishing
        /// to use this functionality must access it using various `as_hal` functions,
        /// primarily [`Surface::as_hal()`], to then use.
        ///
        /// Supported platforms:
        /// - Vulkan (with [VK_GOOGLE_display_timing])
        ///
        /// This is a native only feature.
        ///
        /// [VK_GOOGLE_display_timing]: https://registry.khronos.org/vulkan/specs/1.3-extensions/man/html/VK_GOOGLE_display_timing.html
        /// [`Surface::as_hal()`]: https://docs.rs/wgpu/latest/wgpu/struct.Surface.html#method.as_hal
        const VULKAN_GOOGLE_DISPLAY_TIMING = 1 << 62;
    }
}

impl_bitflags!(Features);

impl Features {
    /// Mask of all features which are part of the upstream WebGPU standard.
    #[must_use]
    pub const fn all_webgpu_mask() -> Self {
        Self::from_bits_truncate(0xFFFFF)
    }

    /// Mask of all features that are only available when targeting native (not web).
    #[must_use]
    pub const fn all_native_mask() -> Self {
        Self::from_bits_truncate(!Self::all_webgpu_mask().bits())
    }

    /// Vertex formats allowed for creating and building BLASes
    #[must_use]
    pub fn allowed_vertex_formats_for_blas(&self) -> Vec<VertexFormat> {
        let mut formats = Vec::new();
        if self.contains(Self::EXPERIMENTAL_RAY_TRACING_ACCELERATION_STRUCTURE) {
            formats.push(VertexFormat::Float32x3);
        }
        formats
    }
}

bitflags::bitflags! {
    /// Instance debugging flags.
    ///
    /// These are not part of the webgpu standard.
    ///
    /// Defaults to enabling debugging-related flags if the build configuration has `debug_assertions`.
    #[repr(transparent)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
    pub struct InstanceFlags: u32 {
        /// Generate debug information in shaders and objects.
        const DEBUG = 1 << 0;
        /// Enable validation, if possible.
        const VALIDATION = 1 << 1;
        /// Don't pass labels to wgpu-hal.
        const DISCARD_HAL_LABELS = 1 << 2;
        /// Whether wgpu should expose adapters that run on top of non-compliant adapters.
        ///
        /// Turning this on might mean that some of the functionality provided by the wgpu
        /// adapter/device is not working or is broken. It could be that all the functionality
        /// wgpu currently exposes works but we can't tell for sure since we have no additional
        /// transparency into what is working and what is not on the underlying adapter.
        ///
        /// This mainly applies to a Vulkan driver's compliance version. If the major compliance version
        /// is `0`, then the driver is ignored. This flag allows that driver to be enabled for testing.
        const ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER = 1 << 3;
        /// Enable GPU-based validation. Implies [`Self::VALIDATION`]. Currently, this only changes
        /// behavior on the DX12 and Vulkan backends.
        ///
        /// Supported platforms:
        ///
        /// - D3D12; called ["GPU-based validation", or
        ///   "GBV"](https://web.archive.org/web/20230206120404/https://learn.microsoft.com/en-us/windows/win32/direct3d12/using-d3d12-debug-layer-gpu-based-validation)
        /// - Vulkan, via the `VK_LAYER_KHRONOS_validation` layer; called ["GPU-Assisted
        ///   Validation"](https://github.com/KhronosGroup/Vulkan-ValidationLayers/blob/e45aeb85079e0835694cb8f03e6681fd18ae72c9/docs/gpu_validation.md#gpu-assisted-validation)
        const GPU_BASED_VALIDATION = 1 << 4;
    }
}

impl Default for InstanceFlags {
    fn default() -> Self {
        Self::from_build_config()
    }
}

impl InstanceFlags {
    /// Enable recommended debugging and validation flags.
    #[must_use]
    pub fn debugging() -> Self {
        InstanceFlags::DEBUG | InstanceFlags::VALIDATION
    }

    /// Enable advanced debugging and validation flags (potentially very slow).
    #[must_use]
    pub fn advanced_debugging() -> Self {
        Self::debugging() | InstanceFlags::GPU_BASED_VALIDATION
    }

    /// Infer good defaults from the build type
    ///
    /// Returns the default flags and add debugging flags if the build configuration has `debug_assertions`.
    #[must_use]
    pub fn from_build_config() -> Self {
        if cfg!(debug_assertions) {
            return InstanceFlags::debugging();
        }

        InstanceFlags::empty()
    }

    /// Returns this set of flags, affected by environment variables.
    ///
    /// The presence of an environment variable implies that the corresponding flag should be set
    /// unless the value is "0" in which case the flag is unset. If the environment variable is
    /// not present, then the flag is unaffected.
    ///
    /// For example `let flags = InstanceFlags::debugging().with_env();` with `WGPU_VALIDATION=0`
    /// does not contain `InstanceFlags::VALIDATION`.
    ///
    /// The environment variables are named after the flags prefixed with "WGPU_". For example:
    /// - WGPU_DEBUG
    /// - WGPU_VALIDATION
    #[must_use]
    pub fn with_env(mut self) -> Self {
        fn env(key: &str) -> Option<bool> {
            std::env::var(key).ok().map(|s| match s.as_str() {
                "0" => false,
                _ => true,
            })
        }

        if let Some(bit) = env("WGPU_VALIDATION") {
            self.set(Self::VALIDATION, bit);
        }
        if let Some(bit) = env("WGPU_DEBUG") {
            self.set(Self::DEBUG, bit);
        }
        if let Some(bit) = env("WGPU_ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER") {
            self.set(Self::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER, bit);
        }
        if let Some(bit) = env("WGPU_GPU_BASED_VALIDATION") {
            self.set(Self::GPU_BASED_VALIDATION, bit);
        }

        self
    }
}

/// Represents the sets of limits an adapter/device supports.
///
/// We provide three different defaults.
/// - [`Limits::downlevel_defaults()`]. This is a set of limits that is guaranteed to work on almost
///   all backends, including "downlevel" backends such as OpenGL and D3D11, other than WebGL. For
///   most applications we recommend using these limits, assuming they are high enough for your
///   application, and you do not intent to support WebGL.
/// - [`Limits::downlevel_webgl2_defaults()`] This is a set of limits that is lower even than the
///   [`downlevel_defaults()`], configured to be low enough to support running in the browser using
///   WebGL2.
/// - [`Limits::default()`]. This is the set of limits that is guaranteed to work on all modern
///   backends and is guaranteed to be supported by WebGPU. Applications needing more modern
///   features can use this as a reasonable set of limits if they are targeting only desktop and
///   modern mobile devices.
///
/// We recommend starting with the most restrictive limits you can and manually increasing the
/// limits you need boosted. This will let you stay running on all hardware that supports the limits
/// you need.
///
/// Limits "better" than the default must be supported by the adapter and requested when requesting
/// a device. If limits "better" than the adapter supports are requested, requesting a device will
/// panic. Once a device is requested, you may only use resources up to the limits requested _even_
/// if the adapter supports "better" limits.
///
/// Requesting limits that are "better" than you need may cause performance to decrease because the
/// implementation needs to support more than is needed. You should ideally only request exactly
/// what you need.
///
/// Corresponds to [WebGPU `GPUSupportedLimits`](
/// https://gpuweb.github.io/gpuweb/#gpusupportedlimits).
///
/// [`downlevel_defaults()`]: Limits::downlevel_defaults
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase", default))]
pub struct Limits {
    /// Maximum allowed value for the `size.width` of a texture created with `TextureDimension::D1`.
    /// Defaults to 8192. Higher is "better".
    #[cfg_attr(feature = "serde", serde(rename = "maxTextureDimension1D"))]
    pub max_texture_dimension_1d: u32,
    /// Maximum allowed value for the `size.width` and `size.height` of a texture created with `TextureDimension::D2`.
    /// Defaults to 8192. Higher is "better".
    #[cfg_attr(feature = "serde", serde(rename = "maxTextureDimension2D"))]
    pub max_texture_dimension_2d: u32,
    /// Maximum allowed value for the `size.width`, `size.height`, and `size.depth_or_array_layers`
    /// of a texture created with `TextureDimension::D3`.
    /// Defaults to 2048. Higher is "better".
    #[cfg_attr(feature = "serde", serde(rename = "maxTextureDimension3D"))]
    pub max_texture_dimension_3d: u32,
    /// Maximum allowed value for the `size.depth_or_array_layers` of a texture created with `TextureDimension::D2`.
    /// Defaults to 256. Higher is "better".
    pub max_texture_array_layers: u32,
    /// Amount of bind groups that can be attached to a pipeline at the same time. Defaults to 4. Higher is "better".
    pub max_bind_groups: u32,
    /// Maximum binding index allowed in `create_bind_group_layout`. Defaults to 1000. Higher is "better".
    pub max_bindings_per_bind_group: u32,
    /// Amount of uniform buffer bindings that can be dynamic in a single pipeline. Defaults to 8. Higher is "better".
    pub max_dynamic_uniform_buffers_per_pipeline_layout: u32,
    /// Amount of storage buffer bindings that can be dynamic in a single pipeline. Defaults to 4. Higher is "better".
    pub max_dynamic_storage_buffers_per_pipeline_layout: u32,
    /// Amount of sampled textures visible in a single shader stage. Defaults to 16. Higher is "better".
    pub max_sampled_textures_per_shader_stage: u32,
    /// Amount of samplers visible in a single shader stage. Defaults to 16. Higher is "better".
    pub max_samplers_per_shader_stage: u32,
    /// Amount of storage buffers visible in a single shader stage. Defaults to 8. Higher is "better".
    pub max_storage_buffers_per_shader_stage: u32,
    /// Amount of storage textures visible in a single shader stage. Defaults to 4. Higher is "better".
    pub max_storage_textures_per_shader_stage: u32,
    /// Amount of uniform buffers visible in a single shader stage. Defaults to 12. Higher is "better".
    pub max_uniform_buffers_per_shader_stage: u32,
    /// Maximum size in bytes of a binding to a uniform buffer. Defaults to 64 KiB. Higher is "better".
    pub max_uniform_buffer_binding_size: u32,
    /// Maximum size in bytes of a binding to a storage buffer. Defaults to 128 MiB. Higher is "better".
    pub max_storage_buffer_binding_size: u32,
    /// Maximum length of `VertexState::buffers` when creating a `RenderPipeline`.
    /// Defaults to 8. Higher is "better".
    pub max_vertex_buffers: u32,
    /// A limit above which buffer allocations are guaranteed to fail.
    /// Defaults to 256 MiB. Higher is "better".
    ///
    /// Buffer allocations below the maximum buffer size may not succeed depending on available memory,
    /// fragmentation and other factors.
    pub max_buffer_size: u64,
    /// Maximum length of `VertexBufferLayout::attributes`, summed over all `VertexState::buffers`,
    /// when creating a `RenderPipeline`.
    /// Defaults to 16. Higher is "better".
    pub max_vertex_attributes: u32,
    /// Maximum value for `VertexBufferLayout::array_stride` when creating a `RenderPipeline`.
    /// Defaults to 2048. Higher is "better".
    pub max_vertex_buffer_array_stride: u32,
    /// Required `BufferBindingType::Uniform` alignment for `BufferBinding::offset`
    /// when creating a `BindGroup`, or for `set_bind_group` `dynamicOffsets`.
    /// Defaults to 256. Lower is "better".
    pub min_uniform_buffer_offset_alignment: u32,
    /// Required `BufferBindingType::Storage` alignment for `BufferBinding::offset`
    /// when creating a `BindGroup`, or for `set_bind_group` `dynamicOffsets`.
    /// Defaults to 256. Lower is "better".
    pub min_storage_buffer_offset_alignment: u32,
    /// Maximum allowed number of components (scalars) of input or output locations for
    /// inter-stage communication (vertex outputs to fragment inputs). Defaults to 60.
    /// Higher is "better".
    pub max_inter_stage_shader_components: u32,
    /// The maximum allowed number of color attachments.
    pub max_color_attachments: u32,
    /// The maximum number of bytes necessary to hold one sample (pixel or subpixel) of render
    /// pipeline output data, across all color attachments.
    pub max_color_attachment_bytes_per_sample: u32,
    /// Maximum number of bytes used for workgroup memory in a compute entry point. Defaults to
    /// 16384. Higher is "better".
    pub max_compute_workgroup_storage_size: u32,
    /// Maximum value of the product of the `workgroup_size` dimensions for a compute entry-point.
    /// Defaults to 256. Higher is "better".
    pub max_compute_invocations_per_workgroup: u32,
    /// The maximum value of the workgroup_size X dimension for a compute stage `ShaderModule` entry-point.
    /// Defaults to 256. Higher is "better".
    pub max_compute_workgroup_size_x: u32,
    /// The maximum value of the workgroup_size Y dimension for a compute stage `ShaderModule` entry-point.
    /// Defaults to 256. Higher is "better".
    pub max_compute_workgroup_size_y: u32,
    /// The maximum value of the workgroup_size Z dimension for a compute stage `ShaderModule` entry-point.
    /// Defaults to 64. Higher is "better".
    pub max_compute_workgroup_size_z: u32,
    /// The maximum value for each dimension of a `ComputePass::dispatch(x, y, z)` operation.
    /// Defaults to 65535. Higher is "better".
    pub max_compute_workgroups_per_dimension: u32,

    /// Minimal number of invocations in a subgroup. Higher is "better".
    pub min_subgroup_size: u32,
    /// Maximal number of invocations in a subgroup. Lower is "better".
    pub max_subgroup_size: u32,
    /// Amount of storage available for push constants in bytes. Defaults to 0. Higher is "better".
    /// Requesting more than 0 during device creation requires [`Features::PUSH_CONSTANTS`] to be enabled.
    ///
    /// Expect the size to be:
    /// - Vulkan: 128-256 bytes
    /// - DX12: 256 bytes
    /// - Metal: 4096 bytes
    /// - OpenGL doesn't natively support push constants, and are emulated with uniforms,
    ///   so this number is less useful but likely 256.
    pub max_push_constant_size: u32,
    /// Maximum number of live non-sampler bindings.
    ///
    /// This limit only affects the d3d12 backend. Using a large number will allow the device
    /// to create many bind groups at the cost of a large up-front allocation at device creation.
    pub max_non_sampler_bindings: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self::defaults()
    }
}

impl Limits {
    // Rust doesn't allow const in trait implementations, so we break this out
    // to allow reusing these defaults in const contexts like `downlevel_defaults`
    const fn defaults() -> Self {
        Self {
            max_texture_dimension_1d: 8192,
            max_texture_dimension_2d: 8192,
            max_texture_dimension_3d: 2048,
            max_texture_array_layers: 256,
            max_bind_groups: 4,
            max_bindings_per_bind_group: 1000,
            max_dynamic_uniform_buffers_per_pipeline_layout: 8,
            max_dynamic_storage_buffers_per_pipeline_layout: 4,
            max_sampled_textures_per_shader_stage: 16,
            max_samplers_per_shader_stage: 16,
            max_storage_buffers_per_shader_stage: 8,
            max_storage_textures_per_shader_stage: 4,
            max_uniform_buffers_per_shader_stage: 12,
            max_uniform_buffer_binding_size: 64 << 10, // (64 KiB)
            max_storage_buffer_binding_size: 128 << 20, // (128 MiB)
            max_vertex_buffers: 8,
            max_buffer_size: 256 << 20, // (256 MiB)
            max_vertex_attributes: 16,
            max_vertex_buffer_array_stride: 2048,
            min_uniform_buffer_offset_alignment: 256,
            min_storage_buffer_offset_alignment: 256,
            max_inter_stage_shader_components: 60,
            max_color_attachments: 8,
            max_color_attachment_bytes_per_sample: 32,
            max_compute_workgroup_storage_size: 16384,
            max_compute_invocations_per_workgroup: 256,
            max_compute_workgroup_size_x: 256,
            max_compute_workgroup_size_y: 256,
            max_compute_workgroup_size_z: 64,
            max_compute_workgroups_per_dimension: 65535,
            min_subgroup_size: 0,
            max_subgroup_size: 0,
            max_push_constant_size: 0,
            max_non_sampler_bindings: 1_000_000,
        }
    }

    /// These default limits are guaranteed to be compatible with GLES-3.1, and D3D11
    ///
    /// Those limits are as follows (different from default are marked with *):
    /// ```rust
    /// # use wgpu_types::Limits;
    /// assert_eq!(Limits::downlevel_defaults(), Limits {
    ///     max_texture_dimension_1d: 2048, // *
    ///     max_texture_dimension_2d: 2048, // *
    ///     max_texture_dimension_3d: 256, // *
    ///     max_texture_array_layers: 256,
    ///     max_bind_groups: 4,
    ///     max_bindings_per_bind_group: 1000,
    ///     max_dynamic_uniform_buffers_per_pipeline_layout: 8,
    ///     max_dynamic_storage_buffers_per_pipeline_layout: 4,
    ///     max_sampled_textures_per_shader_stage: 16,
    ///     max_samplers_per_shader_stage: 16,
    ///     max_storage_buffers_per_shader_stage: 4, // *
    ///     max_storage_textures_per_shader_stage: 4,
    ///     max_uniform_buffers_per_shader_stage: 12,
    ///     max_uniform_buffer_binding_size: 16 << 10, // * (16 KiB)
    ///     max_storage_buffer_binding_size: 128 << 20, // (128 MiB)
    ///     max_vertex_buffers: 8,
    ///     max_vertex_attributes: 16,
    ///     max_vertex_buffer_array_stride: 2048,
    ///     min_subgroup_size: 0,
    ///     max_subgroup_size: 0,
    ///     max_push_constant_size: 0,
    ///     min_uniform_buffer_offset_alignment: 256,
    ///     min_storage_buffer_offset_alignment: 256,
    ///     max_inter_stage_shader_components: 60,
    ///     max_color_attachments: 8,
    ///     max_color_attachment_bytes_per_sample: 32,
    ///     max_compute_workgroup_storage_size: 16352, // *
    ///     max_compute_invocations_per_workgroup: 256,
    ///     max_compute_workgroup_size_x: 256,
    ///     max_compute_workgroup_size_y: 256,
    ///     max_compute_workgroup_size_z: 64,
    ///     max_compute_workgroups_per_dimension: 65535,
    ///     max_buffer_size: 256 << 20, // (256 MiB)
    ///     max_non_sampler_bindings: 1_000_000,
    /// });
    /// ```
    #[must_use]
    pub const fn downlevel_defaults() -> Self {
        Self {
            max_texture_dimension_1d: 2048,
            max_texture_dimension_2d: 2048,
            max_texture_dimension_3d: 256,
            max_storage_buffers_per_shader_stage: 4,
            max_uniform_buffer_binding_size: 16 << 10, // (16 KiB)
            // see: https://developer.apple.com/metal/Metal-Feature-Set-Tables.pdf#page=7
            max_compute_workgroup_storage_size: 16352,
            ..Self::defaults()
        }
    }

    /// These default limits are guaranteed to be compatible with GLES-3.0, and D3D11, and WebGL2
    ///
    /// Those limits are as follows (different from `downlevel_defaults` are marked with +,
    /// *'s from `downlevel_defaults` shown as well.):
    /// ```rust
    /// # use wgpu_types::Limits;
    /// assert_eq!(Limits::downlevel_webgl2_defaults(), Limits {
    ///     max_texture_dimension_1d: 2048, // *
    ///     max_texture_dimension_2d: 2048, // *
    ///     max_texture_dimension_3d: 256, // *
    ///     max_texture_array_layers: 256,
    ///     max_bind_groups: 4,
    ///     max_bindings_per_bind_group: 1000,
    ///     max_dynamic_uniform_buffers_per_pipeline_layout: 8,
    ///     max_dynamic_storage_buffers_per_pipeline_layout: 0, // +
    ///     max_sampled_textures_per_shader_stage: 16,
    ///     max_samplers_per_shader_stage: 16,
    ///     max_storage_buffers_per_shader_stage: 0, // * +
    ///     max_storage_textures_per_shader_stage: 0, // +
    ///     max_uniform_buffers_per_shader_stage: 11, // +
    ///     max_uniform_buffer_binding_size: 16 << 10, // * (16 KiB)
    ///     max_storage_buffer_binding_size: 0, // * +
    ///     max_vertex_buffers: 8,
    ///     max_vertex_attributes: 16,
    ///     max_vertex_buffer_array_stride: 255, // +
    ///     min_subgroup_size: 0,
    ///     max_subgroup_size: 0,
    ///     max_push_constant_size: 0,
    ///     min_uniform_buffer_offset_alignment: 256,
    ///     min_storage_buffer_offset_alignment: 256,
    ///     max_inter_stage_shader_components: 31,
    ///     max_color_attachments: 8,
    ///     max_color_attachment_bytes_per_sample: 32,
    ///     max_compute_workgroup_storage_size: 0, // +
    ///     max_compute_invocations_per_workgroup: 0, // +
    ///     max_compute_workgroup_size_x: 0, // +
    ///     max_compute_workgroup_size_y: 0, // +
    ///     max_compute_workgroup_size_z: 0, // +
    ///     max_compute_workgroups_per_dimension: 0, // +
    ///     max_buffer_size: 256 << 20, // (256 MiB),
    ///     max_non_sampler_bindings: 1_000_000,
    /// });
    /// ```
    #[must_use]
    pub const fn downlevel_webgl2_defaults() -> Self {
        Self {
            max_uniform_buffers_per_shader_stage: 11,
            max_storage_buffers_per_shader_stage: 0,
            max_storage_textures_per_shader_stage: 0,
            max_dynamic_storage_buffers_per_pipeline_layout: 0,
            max_storage_buffer_binding_size: 0,
            max_vertex_buffer_array_stride: 255,
            max_compute_workgroup_storage_size: 0,
            max_compute_invocations_per_workgroup: 0,
            max_compute_workgroup_size_x: 0,
            max_compute_workgroup_size_y: 0,
            max_compute_workgroup_size_z: 0,
            max_compute_workgroups_per_dimension: 0,
            min_subgroup_size: 0,
            max_subgroup_size: 0,

            // Value supported by Intel Celeron B830 on Windows (OpenGL 3.1)
            max_inter_stage_shader_components: 31,

            // Most of the values should be the same as the downlevel defaults
            ..Self::downlevel_defaults()
        }
    }

    /// Modify the current limits to use the resolution limits of the other.
    ///
    /// This is useful because the swapchain might need to be larger than any other image in the application.
    ///
    /// If your application only needs 512x512, you might be running on a 4k display and need extremely high resolution limits.
    #[must_use]
    pub const fn using_resolution(self, other: Self) -> Self {
        Self {
            max_texture_dimension_1d: other.max_texture_dimension_1d,
            max_texture_dimension_2d: other.max_texture_dimension_2d,
            max_texture_dimension_3d: other.max_texture_dimension_3d,
            ..self
        }
    }

    /// Modify the current limits to use the buffer alignment limits of the adapter.
    ///
    /// This is useful for when you'd like to dynamically use the "best" supported buffer alignments.
    #[must_use]
    pub const fn using_alignment(self, other: Self) -> Self {
        Self {
            min_uniform_buffer_offset_alignment: other.min_uniform_buffer_offset_alignment,
            min_storage_buffer_offset_alignment: other.min_storage_buffer_offset_alignment,
            ..self
        }
    }

    /// Compares every limits within self is within the limits given in `allowed`.
    ///
    /// If you need detailed information on failures, look at [`Limits::check_limits_with_fail_fn`].
    #[must_use]
    pub fn check_limits(&self, allowed: &Self) -> bool {
        let mut within = true;
        self.check_limits_with_fail_fn(allowed, true, |_, _, _| within = false);
        within
    }

    /// Compares every limits within self is within the limits given in `allowed`.
    /// For an easy to use binary choice, use [`Limits::check_limits`].
    ///
    /// If a value is not within the allowed limit, this function calls the `fail_fn`
    /// with the:
    ///  - limit name
    ///  - self's limit
    ///  - allowed's limit.
    ///
    /// If fatal is true, a single failure bails out the comparison after a single failure.
    pub fn check_limits_with_fail_fn(
        &self,
        allowed: &Self,
        fatal: bool,
        mut fail_fn: impl FnMut(&'static str, u64, u64),
    ) {
        use std::cmp::Ordering;

        macro_rules! compare {
            ($name:ident, $ordering:ident) => {
                match self.$name.cmp(&allowed.$name) {
                    Ordering::$ordering | Ordering::Equal => (),
                    _ => {
                        fail_fn(stringify!($name), self.$name as u64, allowed.$name as u64);
                        if fatal {
                            return;
                        }
                    }
                }
            };
        }

        compare!(max_texture_dimension_1d, Less);
        compare!(max_texture_dimension_2d, Less);
        compare!(max_texture_dimension_3d, Less);
        compare!(max_texture_array_layers, Less);
        compare!(max_bind_groups, Less);
        compare!(max_bindings_per_bind_group, Less);
        compare!(max_dynamic_uniform_buffers_per_pipeline_layout, Less);
        compare!(max_dynamic_storage_buffers_per_pipeline_layout, Less);
        compare!(max_sampled_textures_per_shader_stage, Less);
        compare!(max_samplers_per_shader_stage, Less);
        compare!(max_storage_buffers_per_shader_stage, Less);
        compare!(max_storage_textures_per_shader_stage, Less);
        compare!(max_uniform_buffers_per_shader_stage, Less);
        compare!(max_uniform_buffer_binding_size, Less);
        compare!(max_storage_buffer_binding_size, Less);
        compare!(max_vertex_buffers, Less);
        compare!(max_buffer_size, Less);
        compare!(max_vertex_attributes, Less);
        compare!(max_vertex_buffer_array_stride, Less);
        compare!(min_uniform_buffer_offset_alignment, Greater);
        compare!(min_storage_buffer_offset_alignment, Greater);
        compare!(max_inter_stage_shader_components, Less);
        compare!(max_color_attachments, Less);
        compare!(max_color_attachment_bytes_per_sample, Less);
        compare!(max_compute_workgroup_storage_size, Less);
        compare!(max_compute_invocations_per_workgroup, Less);
        compare!(max_compute_workgroup_size_x, Less);
        compare!(max_compute_workgroup_size_y, Less);
        compare!(max_compute_workgroup_size_z, Less);
        compare!(max_compute_workgroups_per_dimension, Less);
        if self.min_subgroup_size > 0 && self.max_subgroup_size > 0 {
            compare!(min_subgroup_size, Greater);
            compare!(max_subgroup_size, Less);
        }
        compare!(max_push_constant_size, Less);
        compare!(max_non_sampler_bindings, Less);
    }
}

/// Represents the sets of additional limits on an adapter,
/// which take place when running on downlevel backends.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DownlevelLimits {}

#[allow(clippy::derivable_impls)]
impl Default for DownlevelLimits {
    fn default() -> Self {
        DownlevelLimits {}
    }
}

/// Lists various ways the underlying platform does not conform to the WebGPU standard.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DownlevelCapabilities {
    /// Combined boolean flags.
    pub flags: DownlevelFlags,
    /// Additional limits
    pub limits: DownlevelLimits,
    /// Which collections of features shaders support. Defined in terms of D3D's shader models.
    pub shader_model: ShaderModel,
}

impl Default for DownlevelCapabilities {
    fn default() -> Self {
        Self {
            flags: DownlevelFlags::all(),
            limits: DownlevelLimits::default(),
            shader_model: ShaderModel::Sm5,
        }
    }
}

impl DownlevelCapabilities {
    /// Returns true if the underlying platform offers complete support of the baseline WebGPU standard.
    ///
    /// If this returns false, some parts of the API will result in validation errors where they would not normally.
    /// These parts can be determined by the values in this structure.
    #[must_use]
    pub fn is_webgpu_compliant(&self) -> bool {
        self.flags.contains(DownlevelFlags::compliant())
            && self.limits == DownlevelLimits::default()
            && self.shader_model >= ShaderModel::Sm5
    }
}

bitflags::bitflags! {
    /// Binary flags listing features that may or may not be present on downlevel adapters.
    ///
    /// A downlevel adapter is a GPU adapter that WGPU supports, but with potentially limited
    /// features, due to the lack of hardware feature support.
    ///
    /// Flags that are **not** present for a downlevel adapter or device usually indicates
    /// non-compliance with the WebGPU specification, but not always.
    ///
    /// You can check whether a set of flags is compliant through the
    /// [`DownlevelCapabilities::is_webgpu_compliant()`] function.
    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct DownlevelFlags: u32 {
        /// The device supports compiling and using compute shaders.
        ///
        /// WebGL2, and GLES3.0 devices do not support compute.
        const COMPUTE_SHADERS = 1 << 0;
        /// Supports binding storage buffers and textures to fragment shaders.
        const FRAGMENT_WRITABLE_STORAGE = 1 << 1;
        /// Supports indirect drawing and dispatching.
        ///
        /// WebGL2, GLES 3.0, and Metal on Apple1/Apple2 GPUs do not support indirect.
        const INDIRECT_EXECUTION = 1 << 2;
        /// Supports non-zero `base_vertex` parameter to direct indexed draw calls.
        ///
        /// Indirect calls, if supported, always support non-zero `base_vertex`.
        ///
        /// Supported by:
        /// - Vulkan
        /// - DX12
        /// - Metal on Apple3+ or Mac1+
        /// - OpenGL 3.2+
        /// - OpenGL ES 3.2
        const BASE_VERTEX = 1 << 3;
        /// Supports reading from a depth/stencil texture while using it as a read-only
        /// depth/stencil attachment.
        ///
        /// The WebGL2 and GLES backends do not support RODS.
        const READ_ONLY_DEPTH_STENCIL = 1 << 4;
        /// Supports textures with mipmaps which have a non power of two size.
        const NON_POWER_OF_TWO_MIPMAPPED_TEXTURES = 1 << 5;
        /// Supports textures that are cube arrays.
        const CUBE_ARRAY_TEXTURES = 1 << 6;
        /// Supports comparison samplers.
        const COMPARISON_SAMPLERS = 1 << 7;
        /// Supports different blend operations per color attachment.
        const INDEPENDENT_BLEND = 1 << 8;
        /// Supports storage buffers in vertex shaders.
        const VERTEX_STORAGE = 1 << 9;

        /// Supports samplers with anisotropic filtering. Note this isn't actually required by
        /// WebGPU, the implementation is allowed to completely ignore aniso clamp. This flag is
        /// here for native backends so they can communicate to the user of aniso is enabled.
        ///
        /// All backends and all devices support anisotropic filtering.
        const ANISOTROPIC_FILTERING = 1 << 10;

        /// Supports storage buffers in fragment shaders.
        const FRAGMENT_STORAGE = 1 << 11;

        /// Supports sample-rate shading.
        const MULTISAMPLED_SHADING = 1 << 12;

        /// Supports copies between depth textures and buffers.
        ///
        /// GLES/WebGL don't support this.
        const DEPTH_TEXTURE_AND_BUFFER_COPIES = 1 << 13;

        /// Supports all the texture usages described in WebGPU. If this isn't supported, you
        /// should call `get_texture_format_features` to get how you can use textures of a given format
        const WEBGPU_TEXTURE_FORMAT_SUPPORT = 1 << 14;

        /// Supports buffer bindings with sizes that aren't a multiple of 16.
        ///
        /// WebGL doesn't support this.
        const BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED = 1 << 15;

        /// Supports buffers to combine [`BufferUsages::INDEX`] with usages other than [`BufferUsages::COPY_DST`] and [`BufferUsages::COPY_SRC`].
        /// Furthermore, in absence of this feature it is not allowed to copy index buffers from/to buffers with a set of usage flags containing
        /// [`BufferUsages::VERTEX`]/[`BufferUsages::UNIFORM`]/[`BufferUsages::STORAGE`] or [`BufferUsages::INDIRECT`].
        ///
        /// WebGL doesn't support this.
        const UNRESTRICTED_INDEX_BUFFER = 1 << 16;

        /// Supports full 32-bit range indices (2^32-1 as opposed to 2^24-1 without this flag)
        ///
        /// Corresponds to Vulkan's `VkPhysicalDeviceFeatures.fullDrawIndexUint32`
        const FULL_DRAW_INDEX_UINT32 = 1 << 17;

        /// Supports depth bias clamping
        ///
        /// Corresponds to Vulkan's `VkPhysicalDeviceFeatures.depthBiasClamp`
        const DEPTH_BIAS_CLAMP = 1 << 18;

        /// Supports specifying which view format values are allowed when create_view() is called on a texture.
        ///
        /// The WebGL and GLES backends doesn't support this.
        const VIEW_FORMATS = 1 << 19;

        /// With this feature not present, there are the following restrictions on `Queue::copy_external_image_to_texture`:
        /// - The source must not be [`web_sys::OffscreenCanvas`]
        /// - [`ImageCopyExternalImage::origin`] must be zero.
        /// - [`ImageCopyTextureTagged::color_space`] must be srgb.
        /// - If the source is an [`web_sys::ImageBitmap`]:
        ///   - [`ImageCopyExternalImage::flip_y`] must be false.
        ///   - [`ImageCopyTextureTagged::premultiplied_alpha`] must be false.
        ///
        /// WebGL doesn't support this. WebGPU does.
        const UNRESTRICTED_EXTERNAL_TEXTURE_COPIES = 1 << 20;

        /// Supports specifying which view formats are allowed when calling create_view on the texture returned by get_current_texture.
        ///
        /// The GLES/WebGL and Vulkan on Android doesn't support this.
        const SURFACE_VIEW_FORMATS = 1 << 21;

        /// If this is true, calls to `CommandEncoder::resolve_query_set` will be performed on the queue timeline.
        ///
        /// If this is false, calls to `CommandEncoder::resolve_query_set` will be performed on the device (i.e. cpu) timeline
        /// and will block that timeline until the query has data. You may work around this limitation by waiting until the submit
        /// whose queries you are resolving is fully finished (through use of `queue.on_submitted_work_done`) and only
        /// then submitting the resolve_query_set command. The queries will be guaranteed finished, so will not block.
        ///
        /// Supported by:
        /// - Vulkan,
        /// - DX12
        /// - Metal
        /// - OpenGL 4.4+
        ///
        /// Not Supported by:
        /// - GL ES / WebGL
        const NONBLOCKING_QUERY_RESOLVE = 1 << 22;

        /// If this is true, use of `@builtin(vertex_index)` and `@builtin(instance_index)` will properly take into consideration
        /// the `first_vertex` and `first_instance` parameters of indirect draw calls.
        ///
        /// If this is false, `@builtin(vertex_index)` and `@builtin(instance_index)` will start by counting from 0, ignoring the
        /// `first_vertex` and `first_instance` parameters.
        ///
        /// For example, if you had a draw call like this:
        /// - `first_vertex: 4,`
        /// - `vertex_count: 12,`
        ///
        /// When this flag is present, `@builtin(vertex_index)` will start at 4 and go up to 15 (12 invocations).
        ///
        /// When this flag is not present, `@builtin(vertex_index)` will start at 0 and go up to 11 (12 invocations).
        ///
        /// This only affects the builtins in the shaders,
        /// vertex buffers and instance rate vertex buffers will behave like expected with this flag disabled.
        ///
        /// See also [`Features::`]
        ///
        /// Supported By:
        /// - Vulkan
        /// - Metal
        /// - OpenGL
        ///
        /// Will be implemented in the future by:
        /// - DX12 ([#2471](https://github.com/gfx-rs/wgpu/issues/2471))
        const VERTEX_AND_INSTANCE_INDEX_RESPECTS_RESPECTIVE_FIRST_VALUE_IN_INDIRECT_DRAW = 1 << 23;
    }
}

impl_bitflags!(DownlevelFlags);

impl DownlevelFlags {
    /// All flags that indicate if the backend is WebGPU compliant
    #[must_use]
    pub const fn compliant() -> Self {
        // We use manual bit twiddling to make this a const fn as `Sub` and `.remove` aren't const

        // WebGPU doesn't actually require aniso
        Self::from_bits_truncate(Self::all().bits() & !Self::ANISOTROPIC_FILTERING.bits())
    }
}

/// Collections of shader features a device supports if they support less than WebGPU normally allows.
// TODO: Fill out the differences between shader models more completely
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ShaderModel {
    /// Extremely limited shaders, including a total instruction limit.
    Sm2,
    /// Missing minor features and storage images.
    Sm4,
    /// WebGPU supports shader module 5.
    Sm5,
}

/// Supported physical device types.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DeviceType {
    /// Other or Unknown.
    Other,
    /// Integrated GPU with shared CPU/GPU memory.
    IntegratedGpu,
    /// Discrete GPU with separate CPU/GPU memory.
    DiscreteGpu,
    /// Virtual / Hosted.
    VirtualGpu,
    /// Cpu / Software Rendering.
    Cpu,
}

//TODO: convert `vendor` and `device` to `u32`

/// Information about an adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AdapterInfo {
    /// Adapter name
    pub name: String,
    /// [`Backend`]-specific vendor ID of the adapter
    ///
    /// This generally is a 16-bit PCI vendor ID in the least significant bytes of this field.
    /// However, more significant bytes may be non-zero if the backend uses a different
    /// representation.
    ///
    /// * For [`Backend::Vulkan`], the [`VkPhysicalDeviceProperties::vendorID`] is used, which is
    ///     a superset of PCI IDs.
    ///
    /// [`VkPhysicalDeviceProperties::vendorID`]: https://registry.khronos.org/vulkan/specs/1.3-extensions/man/html/VkPhysicalDeviceProperties.html
    pub vendor: u32,
    /// [`Backend`]-specific device ID of the adapter
    ///
    ///
    /// This generally is a 16-bit PCI device ID in the least significant bytes of this field.
    /// However, more significant bytes may be non-zero if the backend uses a different
    /// representation.
    ///
    /// * For [`Backend::Vulkan`], the [`VkPhysicalDeviceProperties::deviceID`] is used, which is
    ///    a superset of PCI IDs.
    ///
    /// [`VkPhysicalDeviceProperties::deviceID`]: https://registry.khronos.org/vulkan/specs/1.3-extensions/man/html/VkPhysicalDeviceProperties.html
    pub device: u32,
    /// Type of device
    pub device_type: DeviceType,
    /// Driver name
    pub driver: String,
    /// Driver info
    pub driver_info: String,
    /// Backend used for device
    pub backend: Backend,
}

/// Hints to the device about the memory allocation strategy.
///
/// Some backends may ignore these hints.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MemoryHints {
    /// Favor performance over memory usage (the default value).
    #[default]
    Performance,
    /// Favor memory usage over performance.
    MemoryUsage,
    /// Applications that have control over the content that is rendered
    /// (typically games) may find an optimal compromise between memory
    /// usage and performance by specifying the allocation configuration.
    Manual {
        /// Defines the range of allowed memory block sizes for sub-allocated
        /// resources.
        ///
        /// The backend may attempt to group multiple resources into fewer
        /// device memory blocks (sub-allocation) for performance reasons.
        /// The start of the provided range specifies the initial memory
        /// block size for sub-allocated resources. After running out of
        /// space in existing memory blocks, the backend may chose to
        /// progressively increase the block size of subsequent allocations
        /// up to a limit specified by the end of the range.
        ///
        /// This does not limit resource sizes. If a resource does not fit
        /// in the specified range, it will typically be placed in a dedicated
        /// memory block.
        suballocated_device_memory_block_size: Range<u64>,
    },
}

/// Describes a [`Device`](../wgpu/struct.Device.html).
///
/// Corresponds to [WebGPU `GPUDeviceDescriptor`](
/// https://gpuweb.github.io/gpuweb/#gpudevicedescriptor).
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DeviceDescriptor<L> {
    /// Debug label for the device.
    pub label: L,
    /// Specifies the features that are required by the device request.
    /// The request will fail if the adapter cannot provide these features.
    ///
    /// Exactly the specified set of features, and no more or less,
    /// will be allowed in validation of API calls on the resulting device.
    pub required_features: Features,
    /// Specifies the limits that are required by the device request.
    /// The request will fail if the adapter cannot provide these limits.
    ///
    /// Exactly the specified limits, and no better or worse,
    /// will be allowed in validation of API calls on the resulting device.
    pub required_limits: Limits,
    /// Hints for memory allocation strategies.
    pub memory_hints: MemoryHints,
}

impl<L> DeviceDescriptor<L> {
    /// Takes a closure and maps the label of the device descriptor into another.
    #[must_use]
    pub fn map_label<K>(&self, fun: impl FnOnce(&L) -> K) -> DeviceDescriptor<K> {
        DeviceDescriptor {
            label: fun(&self.label),
            required_features: self.required_features,
            required_limits: self.required_limits.clone(),
            memory_hints: self.memory_hints.clone(),
        }
    }
}

bitflags::bitflags! {
    /// Describes the shader stages that a binding will be visible from.
    ///
    /// These can be combined so something that is visible from both vertex and fragment shaders can be defined as:
    ///
    /// `ShaderStages::VERTEX | ShaderStages::FRAGMENT`
    ///
    /// Corresponds to [WebGPU `GPUShaderStageFlags`](
    /// https://gpuweb.github.io/gpuweb/#typedefdef-gpushaderstageflags).
    #[repr(transparent)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
    pub struct ShaderStages: u32 {
        /// Binding is not visible from any shader stage.
        const NONE = 0;
        /// Binding is visible from the vertex shader of a render pipeline.
        const VERTEX = 1 << 0;
        /// Binding is visible from the fragment shader of a render pipeline.
        const FRAGMENT = 1 << 1;
        /// Binding is visible from the compute shader of a compute pipeline.
        const COMPUTE = 1 << 2;
        /// Binding is visible from the vertex and fragment shaders of a render pipeline.
        const VERTEX_FRAGMENT = Self::VERTEX.bits() | Self::FRAGMENT.bits();
    }
}

impl_bitflags!(ShaderStages);

/// Dimensions of a particular texture view.
///
/// Corresponds to [WebGPU `GPUTextureViewDimension`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gputextureviewdimension).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TextureViewDimension {
    /// A one dimensional texture. `texture_1d` in WGSL and `texture1D` in GLSL.
    #[cfg_attr(feature = "serde", serde(rename = "1d"))]
    D1,
    /// A two dimensional texture. `texture_2d` in WGSL and `texture2D` in GLSL.
    #[cfg_attr(feature = "serde", serde(rename = "2d"))]
    #[default]
    D2,
    /// A two dimensional array texture. `texture_2d_array` in WGSL and `texture2DArray` in GLSL.
    #[cfg_attr(feature = "serde", serde(rename = "2d-array"))]
    D2Array,
    /// A cubemap texture. `texture_cube` in WGSL and `textureCube` in GLSL.
    #[cfg_attr(feature = "serde", serde(rename = "cube"))]
    Cube,
    /// A cubemap array texture. `texture_cube_array` in WGSL and `textureCubeArray` in GLSL.
    #[cfg_attr(feature = "serde", serde(rename = "cube-array"))]
    CubeArray,
    /// A three dimensional texture. `texture_3d` in WGSL and `texture3D` in GLSL.
    #[cfg_attr(feature = "serde", serde(rename = "3d"))]
    D3,
}

impl TextureViewDimension {
    /// Get the texture dimension required of this texture view dimension.
    #[must_use]
    pub fn compatible_texture_dimension(self) -> TextureDimension {
        match self {
            Self::D1 => TextureDimension::D1,
            Self::D2 | Self::D2Array | Self::Cube | Self::CubeArray => TextureDimension::D2,
            Self::D3 => TextureDimension::D3,
        }
    }
}

/// Alpha blend factor.
///
/// Corresponds to [WebGPU `GPUBlendFactor`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpublendfactor). Values using `Src1`
/// require [`Features::DUAL_SOURCE_BLENDING`] and can only be used with the first
/// render target.
///
/// For further details on how the blend factors are applied, see the analogous
/// functionality in OpenGL: <https://www.khronos.org/opengl/wiki/Blending#Blending_Parameters>.
#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum BlendFactor {
    /// 0.0
    Zero = 0,
    /// 1.0
    One = 1,
    /// S.component
    Src = 2,
    /// 1.0 - S.component
    OneMinusSrc = 3,
    /// S.alpha
    SrcAlpha = 4,
    /// 1.0 - S.alpha
    OneMinusSrcAlpha = 5,
    /// D.component
    Dst = 6,
    /// 1.0 - D.component
    OneMinusDst = 7,
    /// D.alpha
    DstAlpha = 8,
    /// 1.0 - D.alpha
    OneMinusDstAlpha = 9,
    /// min(S.alpha, 1.0 - D.alpha)
    SrcAlphaSaturated = 10,
    /// Constant
    Constant = 11,
    /// 1.0 - Constant
    OneMinusConstant = 12,
    /// S1.component
    Src1 = 13,
    /// 1.0 - S1.component
    OneMinusSrc1 = 14,
    /// S1.alpha
    Src1Alpha = 15,
    /// 1.0 - S1.alpha
    OneMinusSrc1Alpha = 16,
}

impl BlendFactor {
    /// Returns `true` if the blend factor references the second blend source.
    ///
    /// Note that the usage of those blend factors require [`Features::DUAL_SOURCE_BLENDING`].
    #[must_use]
    pub fn ref_second_blend_source(&self) -> bool {
        match self {
            BlendFactor::Src1
            | BlendFactor::OneMinusSrc1
            | BlendFactor::Src1Alpha
            | BlendFactor::OneMinusSrc1Alpha => true,
            _ => false,
        }
    }
}

/// Alpha blend operation.
///
/// Corresponds to [WebGPU `GPUBlendOperation`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpublendoperation).
///
/// For further details on how the blend operations are applied, see
/// the analogous functionality in OpenGL: <https://www.khronos.org/opengl/wiki/Blending#Blend_Equations>.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum BlendOperation {
    /// Src + Dst
    #[default]
    Add = 0,
    /// Src - Dst
    Subtract = 1,
    /// Dst - Src
    ReverseSubtract = 2,
    /// min(Src, Dst)
    Min = 3,
    /// max(Src, Dst)
    Max = 4,
}

/// Describes a blend component of a [`BlendState`].
///
/// Corresponds to [WebGPU `GPUBlendComponent`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpublendcomponent).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct BlendComponent {
    /// Multiplier for the source, which is produced by the fragment shader.
    pub src_factor: BlendFactor,
    /// Multiplier for the destination, which is stored in the target.
    pub dst_factor: BlendFactor,
    /// The binary operation applied to the source and destination,
    /// multiplied by their respective factors.
    pub operation: BlendOperation,
}

impl BlendComponent {
    /// Default blending state that replaces destination with the source.
    pub const REPLACE: Self = Self {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::Zero,
        operation: BlendOperation::Add,
    };

    /// Blend state of (1 * src) + ((1 - src_alpha) * dst)
    pub const OVER: Self = Self {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    };

    /// Returns true if the state relies on the constant color, which is
    /// set independently on a render command encoder.
    #[must_use]
    pub fn uses_constant(&self) -> bool {
        match (self.src_factor, self.dst_factor) {
            (BlendFactor::Constant, _)
            | (BlendFactor::OneMinusConstant, _)
            | (_, BlendFactor::Constant)
            | (_, BlendFactor::OneMinusConstant) => true,
            (_, _) => false,
        }
    }
}

impl Default for BlendComponent {
    fn default() -> Self {
        Self::REPLACE
    }
}

/// Describe the blend state of a render pipeline,
/// within [`ColorTargetState`].
///
/// Corresponds to [WebGPU `GPUBlendState`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpublendstate).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct BlendState {
    /// Color equation.
    pub color: BlendComponent,
    /// Alpha equation.
    pub alpha: BlendComponent,
}

impl BlendState {
    /// Blend mode that does no color blending, just overwrites the output with the contents of the shader.
    pub const REPLACE: Self = Self {
        color: BlendComponent::REPLACE,
        alpha: BlendComponent::REPLACE,
    };

    /// Blend mode that does standard alpha blending with non-premultiplied alpha.
    pub const ALPHA_BLENDING: Self = Self {
        color: BlendComponent {
            src_factor: BlendFactor::SrcAlpha,
            dst_factor: BlendFactor::OneMinusSrcAlpha,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent::OVER,
    };

    /// Blend mode that does standard alpha blending with premultiplied alpha.
    pub const PREMULTIPLIED_ALPHA_BLENDING: Self = Self {
        color: BlendComponent::OVER,
        alpha: BlendComponent::OVER,
    };
}

/// Describes the color state of a render pipeline.
///
/// Corresponds to [WebGPU `GPUColorTargetState`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpucolortargetstate).
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ColorTargetState {
    /// The [`TextureFormat`] of the image that this pipeline will render to. Must match the format
    /// of the corresponding color attachment in [`CommandEncoder::begin_render_pass`][CEbrp]
    ///
    /// [CEbrp]: ../wgpu/struct.CommandEncoder.html#method.begin_render_pass
    pub format: TextureFormat,
    /// The blending that is used for this pipeline.
    #[cfg_attr(feature = "serde", serde(default))]
    pub blend: Option<BlendState>,
    /// Mask which enables/disables writes to different color/alpha channel.
    #[cfg_attr(feature = "serde", serde(default))]
    pub write_mask: ColorWrites,
}

impl From<TextureFormat> for ColorTargetState {
    fn from(format: TextureFormat) -> Self {
        Self {
            format,
            blend: None,
            write_mask: ColorWrites::ALL,
        }
    }
}

/// Primitive type the input mesh is composed of.
///
/// Corresponds to [WebGPU `GPUPrimitiveTopology`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpuprimitivetopology).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum PrimitiveTopology {
    /// Vertex data is a list of points. Each vertex is a new point.
    PointList = 0,
    /// Vertex data is a list of lines. Each pair of vertices composes a new line.
    ///
    /// Vertices `0 1 2 3` create two lines `0 1` and `2 3`
    LineList = 1,
    /// Vertex data is a strip of lines. Each set of two adjacent vertices form a line.
    ///
    /// Vertices `0 1 2 3` create three lines `0 1`, `1 2`, and `2 3`.
    LineStrip = 2,
    /// Vertex data is a list of triangles. Each set of 3 vertices composes a new triangle.
    ///
    /// Vertices `0 1 2 3 4 5` create two triangles `0 1 2` and `3 4 5`
    #[default]
    TriangleList = 3,
    /// Vertex data is a triangle strip. Each set of three adjacent vertices form a triangle.
    ///
    /// Vertices `0 1 2 3 4 5` create four triangles `0 1 2`, `2 1 3`, `2 3 4`, and `4 3 5`
    TriangleStrip = 4,
}

impl PrimitiveTopology {
    /// Returns true for strip topologies.
    #[must_use]
    pub fn is_strip(&self) -> bool {
        match *self {
            Self::PointList | Self::LineList | Self::TriangleList => false,
            Self::LineStrip | Self::TriangleStrip => true,
        }
    }
}

/// Vertex winding order which classifies the "front" face of a triangle.
///
/// Corresponds to [WebGPU `GPUFrontFace`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpufrontface).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum FrontFace {
    /// Triangles with vertices in counter clockwise order are considered the front face.
    ///
    /// This is the default with right handed coordinate spaces.
    #[default]
    Ccw = 0,
    /// Triangles with vertices in clockwise order are considered the front face.
    ///
    /// This is the default with left handed coordinate spaces.
    Cw = 1,
}

/// Face of a vertex.
///
/// Corresponds to [WebGPU `GPUCullMode`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpucullmode),
/// except that the `"none"` value is represented using `Option<Face>` instead.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum Face {
    /// Front face
    Front = 0,
    /// Back face
    Back = 1,
}

/// Type of drawing mode for polygons
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum PolygonMode {
    /// Polygons are filled
    #[default]
    Fill = 0,
    /// Polygons are drawn as line segments
    Line = 1,
    /// Polygons are drawn as points
    Point = 2,
}

/// Describes the state of primitive assembly and rasterization in a render pipeline.
///
/// Corresponds to [WebGPU `GPUPrimitiveState`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpuprimitivestate).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct PrimitiveState {
    /// The primitive topology used to interpret vertices.
    pub topology: PrimitiveTopology,
    /// When drawing strip topologies with indices, this is the required format for the index buffer.
    /// This has no effect on non-indexed or non-strip draws.
    ///
    /// Specifying this value enables primitive restart, allowing individual strips to be separated
    /// with the index value `0xFFFF` when using `Uint16`, or `0xFFFFFFFF` when using `Uint32`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub strip_index_format: Option<IndexFormat>,
    /// The face to consider the front for the purpose of culling and stencil operations.
    #[cfg_attr(feature = "serde", serde(default))]
    pub front_face: FrontFace,
    /// The face culling mode.
    #[cfg_attr(feature = "serde", serde(default))]
    pub cull_mode: Option<Face>,
    /// If set to true, the polygon depth is not clipped to 0-1 before rasterization.
    ///
    /// Enabling this requires `Features::DEPTH_CLIP_CONTROL` to be enabled.
    #[cfg_attr(feature = "serde", serde(default))]
    pub unclipped_depth: bool,
    /// Controls the way each polygon is rasterized. Can be either `Fill` (default), `Line` or `Point`
    ///
    /// Setting this to `Line` requires `Features::POLYGON_MODE_LINE` to be enabled.
    ///
    /// Setting this to `Point` requires `Features::POLYGON_MODE_POINT` to be enabled.
    #[cfg_attr(feature = "serde", serde(default))]
    pub polygon_mode: PolygonMode,
    /// If set to true, the primitives are rendered with conservative overestimation. I.e. any rastered pixel touched by it is filled.
    /// Only valid for PolygonMode::Fill!
    ///
    /// Enabling this requires `Features::CONSERVATIVE_RASTERIZATION` to be enabled.
    pub conservative: bool,
}

/// Describes the multi-sampling state of a render pipeline.
///
/// Corresponds to [WebGPU `GPUMultisampleState`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpumultisamplestate).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct MultisampleState {
    /// The number of samples calculated per pixel (for MSAA). For non-multisampled textures,
    /// this should be `1`
    pub count: u32,
    /// Bitmask that restricts the samples of a pixel modified by this pipeline. All samples
    /// can be enabled using the value `!0`
    pub mask: u64,
    /// When enabled, produces another sample mask per pixel based on the alpha output value, that
    /// is ANDed with the sample_mask and the primitive coverage to restrict the set of samples
    /// affected by a primitive.
    ///
    /// The implicit mask produced for alpha of zero is guaranteed to be zero, and for alpha of one
    /// is guaranteed to be all 1-s.
    pub alpha_to_coverage_enabled: bool,
}

impl Default for MultisampleState {
    fn default() -> Self {
        MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        }
    }
}

bitflags::bitflags! {
    /// Feature flags for a texture format.
    #[repr(transparent)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
    pub struct TextureFormatFeatureFlags: u32 {
        /// If not present, the texture can't be sampled with a filtering sampler.
        /// This may overwrite TextureSampleType::Float.filterable
        const FILTERABLE = 1 << 0;
        /// Allows [`TextureDescriptor::sample_count`] to be `2`.
        const MULTISAMPLE_X2 = 1 << 1;
        /// Allows [`TextureDescriptor::sample_count`] to be `4`.
        const MULTISAMPLE_X4 = 1 << 2 ;
        /// Allows [`TextureDescriptor::sample_count`] to be `8`.
        const MULTISAMPLE_X8 = 1 << 3 ;
        /// Allows [`TextureDescriptor::sample_count`] to be `16`.
        const MULTISAMPLE_X16 = 1 << 4;
        /// Allows a texture of this format to back a view passed as `resolve_target`
        /// to a render pass for an automatic driver-implemented resolve.
        const MULTISAMPLE_RESOLVE = 1 << 5;
        /// When used as a STORAGE texture, then a texture with this format can be bound with
        /// [`StorageTextureAccess::ReadOnly`] or [`StorageTextureAccess::ReadWrite`].
        const STORAGE_READ_WRITE = 1 << 6;
        /// If not present, the texture can't be blended into the render target.
        const BLENDABLE = 1 << 7;
    }
}

impl TextureFormatFeatureFlags {
    /// Sample count supported by a given texture format.
    ///
    /// returns `true` if `count` is a supported sample count.
    #[must_use]
    pub fn sample_count_supported(&self, count: u32) -> bool {
        use TextureFormatFeatureFlags as tfsc;

        match count {
            1 => true,
            2 => self.contains(tfsc::MULTISAMPLE_X2),
            4 => self.contains(tfsc::MULTISAMPLE_X4),
            8 => self.contains(tfsc::MULTISAMPLE_X8),
            16 => self.contains(tfsc::MULTISAMPLE_X16),
            _ => false,
        }
    }

    /// A `Vec` of supported sample counts.
    #[must_use]
    pub fn supported_sample_counts(&self) -> Vec<u32> {
        let all_possible_sample_counts: [u32; 5] = [1, 2, 4, 8, 16];
        all_possible_sample_counts
            .into_iter()
            .filter(|&sc| self.sample_count_supported(sc))
            .collect()
    }
}

impl_bitflags!(TextureFormatFeatureFlags);

/// Features supported by a given texture format
///
/// Features are defined by WebGPU specification unless `Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` is enabled.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextureFormatFeatures {
    /// Valid bits for `TextureDescriptor::Usage` provided for format creation.
    pub allowed_usages: TextureUsages,
    /// Additional property flags for the format.
    pub flags: TextureFormatFeatureFlags,
}

/// ASTC block dimensions
#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AstcBlock {
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px).
    B4x4,
    /// 5x4 block compressed texture. 16 bytes per block (6.4 bit/px).
    B5x4,
    /// 5x5 block compressed texture. 16 bytes per block (5.12 bit/px).
    B5x5,
    /// 6x5 block compressed texture. 16 bytes per block (4.27 bit/px).
    B6x5,
    /// 6x6 block compressed texture. 16 bytes per block (3.56 bit/px).
    B6x6,
    /// 8x5 block compressed texture. 16 bytes per block (3.2 bit/px).
    B8x5,
    /// 8x6 block compressed texture. 16 bytes per block (2.67 bit/px).
    B8x6,
    /// 8x8 block compressed texture. 16 bytes per block (2 bit/px).
    B8x8,
    /// 10x5 block compressed texture. 16 bytes per block (2.56 bit/px).
    B10x5,
    /// 10x6 block compressed texture. 16 bytes per block (2.13 bit/px).
    B10x6,
    /// 10x8 block compressed texture. 16 bytes per block (1.6 bit/px).
    B10x8,
    /// 10x10 block compressed texture. 16 bytes per block (1.28 bit/px).
    B10x10,
    /// 12x10 block compressed texture. 16 bytes per block (1.07 bit/px).
    B12x10,
    /// 12x12 block compressed texture. 16 bytes per block (0.89 bit/px).
    B12x12,
}

/// ASTC RGBA channel
#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AstcChannel {
    /// 8 bit integer RGBA, [0, 255] converted to/from linear-color float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE_COMPRESSION_ASTC`] must be enabled to use this channel.
    Unorm,
    /// 8 bit integer RGBA, Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE_COMPRESSION_ASTC`] must be enabled to use this channel.
    UnormSrgb,
    /// floating-point RGBA, linear-color float can be outside of the [0, 1] range.
    ///
    /// [`Features::TEXTURE_COMPRESSION_ASTC_HDR`] must be enabled to use this channel.
    Hdr,
}

/// Underlying texture data format.
///
/// If there is a conversion in the format (such as srgb -> linear), the conversion listed here is for
/// loading from texture in a shader. When writing to the texture, the opposite conversion takes place.
///
/// Corresponds to [WebGPU `GPUTextureFormat`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gputextureformat).
#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub enum TextureFormat {
    // Normal 8 bit formats
    /// Red channel only. 8 bit integer per channel. [0, 255] converted to/from float [0, 1] in shader.
    R8Unorm,
    /// Red channel only. 8 bit integer per channel. [-127, 127] converted to/from float [-1, 1] in shader.
    R8Snorm,
    /// Red channel only. 8 bit integer per channel. Unsigned in shader.
    R8Uint,
    /// Red channel only. 8 bit integer per channel. Signed in shader.
    R8Sint,

    // Normal 16 bit formats
    /// Red channel only. 16 bit integer per channel. Unsigned in shader.
    R16Uint,
    /// Red channel only. 16 bit integer per channel. Signed in shader.
    R16Sint,
    /// Red channel only. 16 bit integer per channel. [0, 65535] converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE_FORMAT_16BIT_NORM`] must be enabled to use this texture format.
    R16Unorm,
    /// Red channel only. 16 bit integer per channel. [0, 65535] converted to/from float [-1, 1] in shader.
    ///
    /// [`Features::TEXTURE_FORMAT_16BIT_NORM`] must be enabled to use this texture format.
    R16Snorm,
    /// Red channel only. 16 bit float per channel. Float in shader.
    R16Float,
    /// Red and green channels. 8 bit integer per channel. [0, 255] converted to/from float [0, 1] in shader.
    Rg8Unorm,
    /// Red and green channels. 8 bit integer per channel. [-127, 127] converted to/from float [-1, 1] in shader.
    Rg8Snorm,
    /// Red and green channels. 8 bit integer per channel. Unsigned in shader.
    Rg8Uint,
    /// Red and green channels. 8 bit integer per channel. Signed in shader.
    Rg8Sint,

    // Normal 32 bit formats
    /// Red channel only. 32 bit integer per channel. Unsigned in shader.
    R32Uint,
    /// Red channel only. 32 bit integer per channel. Signed in shader.
    R32Sint,
    /// Red channel only. 32 bit float per channel. Float in shader.
    R32Float,
    /// Red and green channels. 16 bit integer per channel. Unsigned in shader.
    Rg16Uint,
    /// Red and green channels. 16 bit integer per channel. Signed in shader.
    Rg16Sint,
    /// Red and green channels. 16 bit integer per channel. [0, 65535] converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE_FORMAT_16BIT_NORM`] must be enabled to use this texture format.
    Rg16Unorm,
    /// Red and green channels. 16 bit integer per channel. [0, 65535] converted to/from float [-1, 1] in shader.
    ///
    /// [`Features::TEXTURE_FORMAT_16BIT_NORM`] must be enabled to use this texture format.
    Rg16Snorm,
    /// Red and green channels. 16 bit float per channel. Float in shader.
    Rg16Float,
    /// Red, green, blue, and alpha channels. 8 bit integer per channel. [0, 255] converted to/from float [0, 1] in shader.
    Rgba8Unorm,
    /// Red, green, blue, and alpha channels. 8 bit integer per channel. Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader.
    Rgba8UnormSrgb,
    /// Red, green, blue, and alpha channels. 8 bit integer per channel. [-127, 127] converted to/from float [-1, 1] in shader.
    Rgba8Snorm,
    /// Red, green, blue, and alpha channels. 8 bit integer per channel. Unsigned in shader.
    Rgba8Uint,
    /// Red, green, blue, and alpha channels. 8 bit integer per channel. Signed in shader.
    Rgba8Sint,
    /// Blue, green, red, and alpha channels. 8 bit integer per channel. [0, 255] converted to/from float [0, 1] in shader.
    Bgra8Unorm,
    /// Blue, green, red, and alpha channels. 8 bit integer per channel. Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader.
    Bgra8UnormSrgb,

    // Packed 32 bit formats
    /// Packed unsigned float with 9 bits mantisa for each RGB component, then a common 5 bits exponent
    Rgb9e5Ufloat,
    /// Red, green, blue, and alpha channels. 10 bit integer for RGB channels, 2 bit integer for alpha channel. Unsigned in shader.
    Rgb10a2Uint,
    /// Red, green, blue, and alpha channels. 10 bit integer for RGB channels, 2 bit integer for alpha channel. [0, 1023] ([0, 3] for alpha) converted to/from float [0, 1] in shader.
    Rgb10a2Unorm,
    /// Red, green, and blue channels. 11 bit float with no sign bit for RG channels. 10 bit float with no sign bit for blue channel. Float in shader.
    Rg11b10Ufloat,

    // Normal 64 bit formats
    /// Red and green channels. 32 bit integer per channel. Unsigned in shader.
    Rg32Uint,
    /// Red and green channels. 32 bit integer per channel. Signed in shader.
    Rg32Sint,
    /// Red and green channels. 32 bit float per channel. Float in shader.
    Rg32Float,
    /// Red, green, blue, and alpha channels. 16 bit integer per channel. Unsigned in shader.
    Rgba16Uint,
    /// Red, green, blue, and alpha channels. 16 bit integer per channel. Signed in shader.
    Rgba16Sint,
    /// Red, green, blue, and alpha channels. 16 bit integer per channel. [0, 65535] converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE_FORMAT_16BIT_NORM`] must be enabled to use this texture format.
    Rgba16Unorm,
    /// Red, green, blue, and alpha. 16 bit integer per channel. [0, 65535] converted to/from float [-1, 1] in shader.
    ///
    /// [`Features::TEXTURE_FORMAT_16BIT_NORM`] must be enabled to use this texture format.
    Rgba16Snorm,
    /// Red, green, blue, and alpha channels. 16 bit float per channel. Float in shader.
    Rgba16Float,

    // Normal 128 bit formats
    /// Red, green, blue, and alpha channels. 32 bit integer per channel. Unsigned in shader.
    Rgba32Uint,
    /// Red, green, blue, and alpha channels. 32 bit integer per channel. Signed in shader.
    Rgba32Sint,
    /// Red, green, blue, and alpha channels. 32 bit float per channel. Float in shader.
    Rgba32Float,

    // Depth and stencil formats
    /// Stencil format with 8 bit integer stencil.
    Stencil8,
    /// Special depth format with 16 bit integer depth.
    Depth16Unorm,
    /// Special depth format with at least 24 bit integer depth.
    Depth24Plus,
    /// Special depth/stencil format with at least 24 bit integer depth and 8 bits integer stencil.
    Depth24PlusStencil8,
    /// Special depth format with 32 bit floating point depth.
    Depth32Float,
    /// Special depth/stencil format with 32 bit floating point depth and 8 bits integer stencil.
    ///
    /// [`Features::DEPTH32FLOAT_STENCIL8`] must be enabled to use this texture format.
    Depth32FloatStencil8,

    /// YUV 4:2:0 chroma subsampled format.
    ///
    /// Contains two planes:
    /// - 0: Single 8 bit channel luminance.
    /// - 1: Dual 8 bit channel chrominance at half width and half height.
    ///
    /// Valid view formats for luminance are [`TextureFormat::R8Unorm`].
    ///
    /// Valid view formats for chrominance are [`TextureFormat::Rg8Unorm`].
    ///
    /// Width and height must be even.
    ///
    /// [`Features::TEXTURE_FORMAT_NV12`] must be enabled to use this texture format.
    NV12,

    // Compressed textures usable with `TEXTURE_COMPRESSION_BC` feature. `TEXTURE_COMPRESSION_SLICED_3D` is required to use with 3D textures.
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). 4 color + alpha pallet. 5 bit R + 6 bit G + 5 bit B + 1 bit alpha.
    /// [0, 63] ([0, 1] for alpha) converted to/from float [0, 1] in shader.
    ///
    /// Also known as DXT1.
    ///
    /// [`Features::TEXTURE_COMPRESSION_BC`] must be enabled to use this texture format.
    /// [`Features::TEXTURE_COMPRESSION_BC_SLICED_3D`] must be enabled to use this texture format with 3D dimension.
    Bc1RgbaUnorm,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). 4 color + alpha pallet. 5 bit R + 6 bit G + 5 bit B + 1 bit alpha.
    /// Srgb-color [0, 63] ([0, 1] for alpha) converted to/from linear-color float [0, 1] in shader.
    ///
    /// Also known as DXT1.
    ///
    /// [`Features::TEXTURE_COMPRESSION_BC`] must be enabled to use this texture format.
    /// [`Features::TEXTURE_COMPRESSION_BC_SLICED_3D`] must be enabled to use this texture format with 3D dimension.
    Bc1RgbaUnormSrgb,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). 4 color pallet. 5 bit R + 6 bit G + 5 bit B + 4 bit alpha.
    /// [0, 63] ([0, 15] for alpha) converted to/from float [0, 1] in shader.
    ///
    /// Also known as DXT3.
    ///
    /// [`Features::TEXTURE_COMPRESSION_BC`] must be enabled to use this texture format.
    /// [`Features::TEXTURE_COMPRESSION_BC_SLICED_3D`] must be enabled to use this texture format with 3D dimension.
    Bc2RgbaUnorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). 4 color pallet. 5 bit R + 6 bit G + 5 bit B + 4 bit alpha.
    /// Srgb-color [0, 63] ([0, 255] for alpha) converted to/from linear-color float [0, 1] in shader.
    ///
    /// Also known as DXT3.
    ///
    /// [`Features::TEXTURE_COMPRESSION_BC`] must be enabled to use this texture format.
    /// [`Features::TEXTURE_COMPRESSION_BC_SLICED_3D`] must be enabled to use this texture format with 3D dimension.
    Bc2RgbaUnormSrgb,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). 4 color pallet + 8 alpha pallet. 5 bit R + 6 bit G + 5 bit B + 8 bit alpha.
    /// [0, 63] ([0, 255] for alpha) converted to/from float [0, 1] in shader.
    ///
    /// Also known as DXT5.
    ///
    /// [`Features::TEXTURE_COMPRESSION_BC`] must be enabled to use this texture format.
    /// [`Features::TEXTURE_COMPRESSION_BC_SLICED_3D`] must be enabled to use this texture format with 3D dimension.
    Bc3RgbaUnorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). 4 color pallet + 8 alpha pallet. 5 bit R + 6 bit G + 5 bit B + 8 bit alpha.
    /// Srgb-color [0, 63] ([0, 255] for alpha) converted to/from linear-color float [0, 1] in shader.
    ///
    /// Also known as DXT5.
    ///
    /// [`Features::TEXTURE_COMPRESSION_BC`] must be enabled to use this texture format.
    /// [`Features::TEXTURE_COMPRESSION_BC_SLICED_3D`] must be enabled to use this texture format with 3D dimension.
    Bc3RgbaUnormSrgb,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). 8 color pallet. 8 bit R.
    /// [0, 255] converted to/from float [0, 1] in shader.
    ///
    /// Also known as RGTC1.
    ///
    /// [`Features::TEXTURE_COMPRESSION_BC`] must be enabled to use this texture format.
    /// [`Features::TEXTURE_COMPRESSION_BC_SLICED_3D`] must be enabled to use this texture format with 3D dimension.
    Bc4RUnorm,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). 8 color pallet. 8 bit R.
    /// [-127, 127] converted to/from float [-1, 1] in shader.
    ///
    /// Also known as RGTC1.
    ///
    /// [`Features::TEXTURE_COMPRESSION_BC`] must be enabled to use this texture format.
    /// [`Features::TEXTURE_COMPRESSION_BC_SLICED_3D`] must be enabled to use this texture format with 3D dimension.
    Bc4RSnorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). 8 color red pallet + 8 color green pallet. 8 bit RG.
    /// [0, 255] converted to/from float [0, 1] in shader.
    ///
    /// Also known as RGTC2.
    ///
    /// [`Features::TEXTURE_COMPRESSION_BC`] must be enabled to use this texture format.
    /// [`Features::TEXTURE_COMPRESSION_BC_SLICED_3D`] must be enabled to use this texture format with 3D dimension.
    Bc5RgUnorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). 8 color red pallet + 8 color green pallet. 8 bit RG.
    /// [-127, 127] converted to/from float [-1, 1] in shader.
    ///
    /// Also known as RGTC2.
    ///
    /// [`Features::TEXTURE_COMPRESSION_BC`] must be enabled to use this texture format.
    /// [`Features::TEXTURE_COMPRESSION_BC_SLICED_3D`] must be enabled to use this texture format with 3D dimension.
    Bc5RgSnorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Variable sized pallet. 16 bit unsigned float RGB. Float in shader.
    ///
    /// Also known as BPTC (float).
    ///
    /// [`Features::TEXTURE_COMPRESSION_BC`] must be enabled to use this texture format.
    /// [`Features::TEXTURE_COMPRESSION_BC_SLICED_3D`] must be enabled to use this texture format with 3D dimension.
    Bc6hRgbUfloat,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Variable sized pallet. 16 bit signed float RGB. Float in shader.
    ///
    /// Also known as BPTC (float).
    ///
    /// [`Features::TEXTURE_COMPRESSION_BC`] must be enabled to use this texture format.
    /// [`Features::TEXTURE_COMPRESSION_BC_SLICED_3D`] must be enabled to use this texture format with 3D dimension.
    Bc6hRgbFloat,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Variable sized pallet. 8 bit integer RGBA.
    /// [0, 255] converted to/from float [0, 1] in shader.
    ///
    /// Also known as BPTC (unorm).
    ///
    /// [`Features::TEXTURE_COMPRESSION_BC`] must be enabled to use this texture format.
    /// [`Features::TEXTURE_COMPRESSION_BC_SLICED_3D`] must be enabled to use this texture format with 3D dimension.
    Bc7RgbaUnorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Variable sized pallet. 8 bit integer RGBA.
    /// Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader.
    ///
    /// Also known as BPTC (unorm).
    ///
    /// [`Features::TEXTURE_COMPRESSION_BC`] must be enabled to use this texture format.
    /// [`Features::TEXTURE_COMPRESSION_BC_SLICED_3D`] must be enabled to use this texture format with 3D dimension.
    Bc7RgbaUnormSrgb,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 8 bit integer RGB.
    /// [0, 255] converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE_COMPRESSION_ETC2`] must be enabled to use this texture format.
    Etc2Rgb8Unorm,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 8 bit integer RGB.
    /// Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE_COMPRESSION_ETC2`] must be enabled to use this texture format.
    Etc2Rgb8UnormSrgb,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 8 bit integer RGB + 1 bit alpha.
    /// [0, 255] ([0, 1] for alpha) converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE_COMPRESSION_ETC2`] must be enabled to use this texture format.
    Etc2Rgb8A1Unorm,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 8 bit integer RGB + 1 bit alpha.
    /// Srgb-color [0, 255] ([0, 1] for alpha) converted to/from linear-color float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE_COMPRESSION_ETC2`] must be enabled to use this texture format.
    Etc2Rgb8A1UnormSrgb,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Complex pallet. 8 bit integer RGB + 8 bit alpha.
    /// [0, 255] converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE_COMPRESSION_ETC2`] must be enabled to use this texture format.
    Etc2Rgba8Unorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Complex pallet. 8 bit integer RGB + 8 bit alpha.
    /// Srgb-color [0, 255] converted to/from linear-color float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE_COMPRESSION_ETC2`] must be enabled to use this texture format.
    Etc2Rgba8UnormSrgb,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 11 bit integer R.
    /// [0, 255] converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE_COMPRESSION_ETC2`] must be enabled to use this texture format.
    EacR11Unorm,
    /// 4x4 block compressed texture. 8 bytes per block (4 bit/px). Complex pallet. 11 bit integer R.
    /// [-127, 127] converted to/from float [-1, 1] in shader.
    ///
    /// [`Features::TEXTURE_COMPRESSION_ETC2`] must be enabled to use this texture format.
    EacR11Snorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Complex pallet. 11 bit integer R + 11 bit integer G.
    /// [0, 255] converted to/from float [0, 1] in shader.
    ///
    /// [`Features::TEXTURE_COMPRESSION_ETC2`] must be enabled to use this texture format.
    EacRg11Unorm,
    /// 4x4 block compressed texture. 16 bytes per block (8 bit/px). Complex pallet. 11 bit integer R + 11 bit integer G.
    /// [-127, 127] converted to/from float [-1, 1] in shader.
    ///
    /// [`Features::TEXTURE_COMPRESSION_ETC2`] must be enabled to use this texture format.
    EacRg11Snorm,
    /// block compressed texture. 16 bytes per block.
    ///
    /// Features [`TEXTURE_COMPRESSION_ASTC`] or [`TEXTURE_COMPRESSION_ASTC_HDR`]
    /// must be enabled to use this texture format.
    ///
    /// [`TEXTURE_COMPRESSION_ASTC`]: Features::TEXTURE_COMPRESSION_ASTC
    /// [`TEXTURE_COMPRESSION_ASTC_HDR`]: Features::TEXTURE_COMPRESSION_ASTC_HDR
    Astc {
        /// compressed block dimensions
        block: AstcBlock,
        /// ASTC RGBA channel
        channel: AstcChannel,
    },
}

#[cfg(any(feature = "serde", test))]
impl<'de> Deserialize<'de> for TextureFormat {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Error, Unexpected};

        struct TextureFormatVisitor;

        impl<'de> de::Visitor<'de> for TextureFormatVisitor {
            type Value = TextureFormat;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a valid texture format")
            }

            fn visit_str<E: Error>(self, s: &str) -> Result<Self::Value, E> {
                let format = match s {
                    "r8unorm" => TextureFormat::R8Unorm,
                    "r8snorm" => TextureFormat::R8Snorm,
                    "r8uint" => TextureFormat::R8Uint,
                    "r8sint" => TextureFormat::R8Sint,
                    "r16uint" => TextureFormat::R16Uint,
                    "r16sint" => TextureFormat::R16Sint,
                    "r16unorm" => TextureFormat::R16Unorm,
                    "r16snorm" => TextureFormat::R16Snorm,
                    "r16float" => TextureFormat::R16Float,
                    "rg8unorm" => TextureFormat::Rg8Unorm,
                    "rg8snorm" => TextureFormat::Rg8Snorm,
                    "rg8uint" => TextureFormat::Rg8Uint,
                    "rg8sint" => TextureFormat::Rg8Sint,
                    "r32uint" => TextureFormat::R32Uint,
                    "r32sint" => TextureFormat::R32Sint,
                    "r32float" => TextureFormat::R32Float,
                    "rg16uint" => TextureFormat::Rg16Uint,
                    "rg16sint" => TextureFormat::Rg16Sint,
                    "rg16unorm" => TextureFormat::Rg16Unorm,
                    "rg16snorm" => TextureFormat::Rg16Snorm,
                    "rg16float" => TextureFormat::Rg16Float,
                    "rgba8unorm" => TextureFormat::Rgba8Unorm,
                    "rgba8unorm-srgb" => TextureFormat::Rgba8UnormSrgb,
                    "rgba8snorm" => TextureFormat::Rgba8Snorm,
                    "rgba8uint" => TextureFormat::Rgba8Uint,
                    "rgba8sint" => TextureFormat::Rgba8Sint,
                    "bgra8unorm" => TextureFormat::Bgra8Unorm,
                    "bgra8unorm-srgb" => TextureFormat::Bgra8UnormSrgb,
                    "rgb10a2uint" => TextureFormat::Rgb10a2Uint,
                    "rgb10a2unorm" => TextureFormat::Rgb10a2Unorm,
                    "rg11b10ufloat" => TextureFormat::Rg11b10Ufloat,
                    "rg32uint" => TextureFormat::Rg32Uint,
                    "rg32sint" => TextureFormat::Rg32Sint,
                    "rg32float" => TextureFormat::Rg32Float,
                    "rgba16uint" => TextureFormat::Rgba16Uint,
                    "rgba16sint" => TextureFormat::Rgba16Sint,
                    "rgba16unorm" => TextureFormat::Rgba16Unorm,
                    "rgba16snorm" => TextureFormat::Rgba16Snorm,
                    "rgba16float" => TextureFormat::Rgba16Float,
                    "rgba32uint" => TextureFormat::Rgba32Uint,
                    "rgba32sint" => TextureFormat::Rgba32Sint,
                    "rgba32float" => TextureFormat::Rgba32Float,
                    "stencil8" => TextureFormat::Stencil8,
                    "depth32float" => TextureFormat::Depth32Float,
                    "depth32float-stencil8" => TextureFormat::Depth32FloatStencil8,
                    "depth16unorm" => TextureFormat::Depth16Unorm,
                    "depth24plus" => TextureFormat::Depth24Plus,
                    "depth24plus-stencil8" => TextureFormat::Depth24PlusStencil8,
                    "nv12" => TextureFormat::NV12,
                    "rgb9e5ufloat" => TextureFormat::Rgb9e5Ufloat,
                    "bc1-rgba-unorm" => TextureFormat::Bc1RgbaUnorm,
                    "bc1-rgba-unorm-srgb" => TextureFormat::Bc1RgbaUnormSrgb,
                    "bc2-rgba-unorm" => TextureFormat::Bc2RgbaUnorm,
                    "bc2-rgba-unorm-srgb" => TextureFormat::Bc2RgbaUnormSrgb,
                    "bc3-rgba-unorm" => TextureFormat::Bc3RgbaUnorm,
                    "bc3-rgba-unorm-srgb" => TextureFormat::Bc3RgbaUnormSrgb,
                    "bc4-r-unorm" => TextureFormat::Bc4RUnorm,
                    "bc4-r-snorm" => TextureFormat::Bc4RSnorm,
                    "bc5-rg-unorm" => TextureFormat::Bc5RgUnorm,
                    "bc5-rg-snorm" => TextureFormat::Bc5RgSnorm,
                    "bc6h-rgb-ufloat" => TextureFormat::Bc6hRgbUfloat,
                    "bc6h-rgb-float" => TextureFormat::Bc6hRgbFloat,
                    "bc7-rgba-unorm" => TextureFormat::Bc7RgbaUnorm,
                    "bc7-rgba-unorm-srgb" => TextureFormat::Bc7RgbaUnormSrgb,
                    "etc2-rgb8unorm" => TextureFormat::Etc2Rgb8Unorm,
                    "etc2-rgb8unorm-srgb" => TextureFormat::Etc2Rgb8UnormSrgb,
                    "etc2-rgb8a1unorm" => TextureFormat::Etc2Rgb8A1Unorm,
                    "etc2-rgb8a1unorm-srgb" => TextureFormat::Etc2Rgb8A1UnormSrgb,
                    "etc2-rgba8unorm" => TextureFormat::Etc2Rgba8Unorm,
                    "etc2-rgba8unorm-srgb" => TextureFormat::Etc2Rgba8UnormSrgb,
                    "eac-r11unorm" => TextureFormat::EacR11Unorm,
                    "eac-r11snorm" => TextureFormat::EacR11Snorm,
                    "eac-rg11unorm" => TextureFormat::EacRg11Unorm,
                    "eac-rg11snorm" => TextureFormat::EacRg11Snorm,
                    other => {
                        if let Some(parts) = other.strip_prefix("astc-") {
                            let (block, channel) = parts
                                .split_once('-')
                                .ok_or_else(|| E::invalid_value(Unexpected::Str(s), &self))?;

                            let block = match block {
                                "4x4" => AstcBlock::B4x4,
                                "5x4" => AstcBlock::B5x4,
                                "5x5" => AstcBlock::B5x5,
                                "6x5" => AstcBlock::B6x5,
                                "6x6" => AstcBlock::B6x6,
                                "8x5" => AstcBlock::B8x5,
                                "8x6" => AstcBlock::B8x6,
                                "8x8" => AstcBlock::B8x8,
                                "10x5" => AstcBlock::B10x5,
                                "10x6" => AstcBlock::B10x6,
                                "10x8" => AstcBlock::B10x8,
                                "10x10" => AstcBlock::B10x10,
                                "12x10" => AstcBlock::B12x10,
                                "12x12" => AstcBlock::B12x12,
                                _ => return Err(E::invalid_value(Unexpected::Str(s), &self)),
                            };

                            let channel = match channel {
                                "unorm" => AstcChannel::Unorm,
                                "unorm-srgb" => AstcChannel::UnormSrgb,
                                "hdr" => AstcChannel::Hdr,
                                _ => return Err(E::invalid_value(Unexpected::Str(s), &self)),
                            };

                            TextureFormat::Astc { block, channel }
                        } else {
                            return Err(E::invalid_value(Unexpected::Str(s), &self));
                        }
                    }
                };

                Ok(format)
            }
        }

        deserializer.deserialize_str(TextureFormatVisitor)
    }
}

#[cfg(any(feature = "serde", test))]
impl Serialize for TextureFormat {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s: String;
        let name = match *self {
            TextureFormat::R8Unorm => "r8unorm",
            TextureFormat::R8Snorm => "r8snorm",
            TextureFormat::R8Uint => "r8uint",
            TextureFormat::R8Sint => "r8sint",
            TextureFormat::R16Uint => "r16uint",
            TextureFormat::R16Sint => "r16sint",
            TextureFormat::R16Unorm => "r16unorm",
            TextureFormat::R16Snorm => "r16snorm",
            TextureFormat::R16Float => "r16float",
            TextureFormat::Rg8Unorm => "rg8unorm",
            TextureFormat::Rg8Snorm => "rg8snorm",
            TextureFormat::Rg8Uint => "rg8uint",
            TextureFormat::Rg8Sint => "rg8sint",
            TextureFormat::R32Uint => "r32uint",
            TextureFormat::R32Sint => "r32sint",
            TextureFormat::R32Float => "r32float",
            TextureFormat::Rg16Uint => "rg16uint",
            TextureFormat::Rg16Sint => "rg16sint",
            TextureFormat::Rg16Unorm => "rg16unorm",
            TextureFormat::Rg16Snorm => "rg16snorm",
            TextureFormat::Rg16Float => "rg16float",
            TextureFormat::Rgba8Unorm => "rgba8unorm",
            TextureFormat::Rgba8UnormSrgb => "rgba8unorm-srgb",
            TextureFormat::Rgba8Snorm => "rgba8snorm",
            TextureFormat::Rgba8Uint => "rgba8uint",
            TextureFormat::Rgba8Sint => "rgba8sint",
            TextureFormat::Bgra8Unorm => "bgra8unorm",
            TextureFormat::Bgra8UnormSrgb => "bgra8unorm-srgb",
            TextureFormat::Rgb10a2Uint => "rgb10a2uint",
            TextureFormat::Rgb10a2Unorm => "rgb10a2unorm",
            TextureFormat::Rg11b10Ufloat => "rg11b10ufloat",
            TextureFormat::Rg32Uint => "rg32uint",
            TextureFormat::Rg32Sint => "rg32sint",
            TextureFormat::Rg32Float => "rg32float",
            TextureFormat::Rgba16Uint => "rgba16uint",
            TextureFormat::Rgba16Sint => "rgba16sint",
            TextureFormat::Rgba16Unorm => "rgba16unorm",
            TextureFormat::Rgba16Snorm => "rgba16snorm",
            TextureFormat::Rgba16Float => "rgba16float",
            TextureFormat::Rgba32Uint => "rgba32uint",
            TextureFormat::Rgba32Sint => "rgba32sint",
            TextureFormat::Rgba32Float => "rgba32float",
            TextureFormat::Stencil8 => "stencil8",
            TextureFormat::Depth32Float => "depth32float",
            TextureFormat::Depth16Unorm => "depth16unorm",
            TextureFormat::Depth32FloatStencil8 => "depth32float-stencil8",
            TextureFormat::Depth24Plus => "depth24plus",
            TextureFormat::Depth24PlusStencil8 => "depth24plus-stencil8",
            TextureFormat::NV12 => "nv12",
            TextureFormat::Rgb9e5Ufloat => "rgb9e5ufloat",
            TextureFormat::Bc1RgbaUnorm => "bc1-rgba-unorm",
            TextureFormat::Bc1RgbaUnormSrgb => "bc1-rgba-unorm-srgb",
            TextureFormat::Bc2RgbaUnorm => "bc2-rgba-unorm",
            TextureFormat::Bc2RgbaUnormSrgb => "bc2-rgba-unorm-srgb",
            TextureFormat::Bc3RgbaUnorm => "bc3-rgba-unorm",
            TextureFormat::Bc3RgbaUnormSrgb => "bc3-rgba-unorm-srgb",
            TextureFormat::Bc4RUnorm => "bc4-r-unorm",
            TextureFormat::Bc4RSnorm => "bc4-r-snorm",
            TextureFormat::Bc5RgUnorm => "bc5-rg-unorm",
            TextureFormat::Bc5RgSnorm => "bc5-rg-snorm",
            TextureFormat::Bc6hRgbUfloat => "bc6h-rgb-ufloat",
            TextureFormat::Bc6hRgbFloat => "bc6h-rgb-float",
            TextureFormat::Bc7RgbaUnorm => "bc7-rgba-unorm",
            TextureFormat::Bc7RgbaUnormSrgb => "bc7-rgba-unorm-srgb",
            TextureFormat::Etc2Rgb8Unorm => "etc2-rgb8unorm",
            TextureFormat::Etc2Rgb8UnormSrgb => "etc2-rgb8unorm-srgb",
            TextureFormat::Etc2Rgb8A1Unorm => "etc2-rgb8a1unorm",
            TextureFormat::Etc2Rgb8A1UnormSrgb => "etc2-rgb8a1unorm-srgb",
            TextureFormat::Etc2Rgba8Unorm => "etc2-rgba8unorm",
            TextureFormat::Etc2Rgba8UnormSrgb => "etc2-rgba8unorm-srgb",
            TextureFormat::EacR11Unorm => "eac-r11unorm",
            TextureFormat::EacR11Snorm => "eac-r11snorm",
            TextureFormat::EacRg11Unorm => "eac-rg11unorm",
            TextureFormat::EacRg11Snorm => "eac-rg11snorm",
            TextureFormat::Astc { block, channel } => {
                let block = match block {
                    AstcBlock::B4x4 => "4x4",
                    AstcBlock::B5x4 => "5x4",
                    AstcBlock::B5x5 => "5x5",
                    AstcBlock::B6x5 => "6x5",
                    AstcBlock::B6x6 => "6x6",
                    AstcBlock::B8x5 => "8x5",
                    AstcBlock::B8x6 => "8x6",
                    AstcBlock::B8x8 => "8x8",
                    AstcBlock::B10x5 => "10x5",
                    AstcBlock::B10x6 => "10x6",
                    AstcBlock::B10x8 => "10x8",
                    AstcBlock::B10x10 => "10x10",
                    AstcBlock::B12x10 => "12x10",
                    AstcBlock::B12x12 => "12x12",
                };

                let channel = match channel {
                    AstcChannel::Unorm => "unorm",
                    AstcChannel::UnormSrgb => "unorm-srgb",
                    AstcChannel::Hdr => "hdr",
                };

                s = format!("astc-{block}-{channel}");
                &s
            }
        };
        serializer.serialize_str(name)
    }
}

impl TextureAspect {
    /// Returns the texture aspect for a given plane.
    #[must_use]
    pub fn from_plane(plane: u32) -> Option<Self> {
        Some(match plane {
            0 => Self::Plane0,
            1 => Self::Plane1,
            2 => Self::Plane2,
            _ => return None,
        })
    }
}

impl TextureFormat {
    /// Returns the aspect-specific format of the original format
    ///
    /// see <https://gpuweb.github.io/gpuweb/#abstract-opdef-resolving-gputextureaspect>
    #[must_use]
    pub fn aspect_specific_format(&self, aspect: TextureAspect) -> Option<Self> {
        match (*self, aspect) {
            (Self::Stencil8, TextureAspect::StencilOnly) => Some(*self),
            (
                Self::Depth16Unorm | Self::Depth24Plus | Self::Depth32Float,
                TextureAspect::DepthOnly,
            ) => Some(*self),
            (
                Self::Depth24PlusStencil8 | Self::Depth32FloatStencil8,
                TextureAspect::StencilOnly,
            ) => Some(Self::Stencil8),
            (Self::Depth24PlusStencil8, TextureAspect::DepthOnly) => Some(Self::Depth24Plus),
            (Self::Depth32FloatStencil8, TextureAspect::DepthOnly) => Some(Self::Depth32Float),
            (Self::NV12, TextureAspect::Plane0) => Some(Self::R8Unorm),
            (Self::NV12, TextureAspect::Plane1) => Some(Self::Rg8Unorm),
            // views to multi-planar formats must specify the plane
            (format, TextureAspect::All) if !format.is_multi_planar_format() => Some(format),
            _ => None,
        }
    }

    /// Returns `true` if `self` is a depth or stencil component of the given
    /// combined depth-stencil format
    #[must_use]
    pub fn is_depth_stencil_component(&self, combined_format: Self) -> bool {
        match (combined_format, *self) {
            (Self::Depth24PlusStencil8, Self::Depth24Plus | Self::Stencil8)
            | (Self::Depth32FloatStencil8, Self::Depth32Float | Self::Stencil8) => true,
            _ => false,
        }
    }

    /// Returns `true` if the format is a depth and/or stencil format
    ///
    /// see <https://gpuweb.github.io/gpuweb/#depth-formats>
    #[must_use]
    pub fn is_depth_stencil_format(&self) -> bool {
        match *self {
            Self::Stencil8
            | Self::Depth16Unorm
            | Self::Depth24Plus
            | Self::Depth24PlusStencil8
            | Self::Depth32Float
            | Self::Depth32FloatStencil8 => true,
            _ => false,
        }
    }

    /// Returns `true` if the format is a combined depth-stencil format
    ///
    /// see <https://gpuweb.github.io/gpuweb/#combined-depth-stencil-format>
    #[must_use]
    pub fn is_combined_depth_stencil_format(&self) -> bool {
        match *self {
            Self::Depth24PlusStencil8 | Self::Depth32FloatStencil8 => true,
            _ => false,
        }
    }

    /// Returns `true` if the format is a multi-planar format
    #[must_use]
    pub fn is_multi_planar_format(&self) -> bool {
        self.planes().is_some()
    }

    /// Returns the number of planes a multi-planar format has.
    #[must_use]
    pub fn planes(&self) -> Option<u32> {
        match *self {
            Self::NV12 => Some(2),
            _ => None,
        }
    }

    /// Returns `true` if the format has a color aspect
    #[must_use]
    pub fn has_color_aspect(&self) -> bool {
        !self.is_depth_stencil_format()
    }

    /// Returns `true` if the format has a depth aspect
    #[must_use]
    pub fn has_depth_aspect(&self) -> bool {
        match *self {
            Self::Depth16Unorm
            | Self::Depth24Plus
            | Self::Depth24PlusStencil8
            | Self::Depth32Float
            | Self::Depth32FloatStencil8 => true,
            _ => false,
        }
    }

    /// Returns `true` if the format has a stencil aspect
    #[must_use]
    pub fn has_stencil_aspect(&self) -> bool {
        match *self {
            Self::Stencil8 | Self::Depth24PlusStencil8 | Self::Depth32FloatStencil8 => true,
            _ => false,
        }
    }

    /// Returns the size multiple requirement for a texture using this format.
    #[must_use]
    pub fn size_multiple_requirement(&self) -> (u32, u32) {
        match *self {
            Self::NV12 => (2, 2),
            _ => self.block_dimensions(),
        }
    }

    /// Returns the dimension of a [block](https://gpuweb.github.io/gpuweb/#texel-block) of texels.
    ///
    /// Uncompressed formats have a block dimension of `(1, 1)`.
    #[must_use]
    pub fn block_dimensions(&self) -> (u32, u32) {
        match *self {
            Self::R8Unorm
            | Self::R8Snorm
            | Self::R8Uint
            | Self::R8Sint
            | Self::R16Uint
            | Self::R16Sint
            | Self::R16Unorm
            | Self::R16Snorm
            | Self::R16Float
            | Self::Rg8Unorm
            | Self::Rg8Snorm
            | Self::Rg8Uint
            | Self::Rg8Sint
            | Self::R32Uint
            | Self::R32Sint
            | Self::R32Float
            | Self::Rg16Uint
            | Self::Rg16Sint
            | Self::Rg16Unorm
            | Self::Rg16Snorm
            | Self::Rg16Float
            | Self::Rgba8Unorm
            | Self::Rgba8UnormSrgb
            | Self::Rgba8Snorm
            | Self::Rgba8Uint
            | Self::Rgba8Sint
            | Self::Bgra8Unorm
            | Self::Bgra8UnormSrgb
            | Self::Rgb9e5Ufloat
            | Self::Rgb10a2Uint
            | Self::Rgb10a2Unorm
            | Self::Rg11b10Ufloat
            | Self::Rg32Uint
            | Self::Rg32Sint
            | Self::Rg32Float
            | Self::Rgba16Uint
            | Self::Rgba16Sint
            | Self::Rgba16Unorm
            | Self::Rgba16Snorm
            | Self::Rgba16Float
            | Self::Rgba32Uint
            | Self::Rgba32Sint
            | Self::Rgba32Float
            | Self::Stencil8
            | Self::Depth16Unorm
            | Self::Depth24Plus
            | Self::Depth24PlusStencil8
            | Self::Depth32Float
            | Self::Depth32FloatStencil8
            | Self::NV12 => (1, 1),

            Self::Bc1RgbaUnorm
            | Self::Bc1RgbaUnormSrgb
            | Self::Bc2RgbaUnorm
            | Self::Bc2RgbaUnormSrgb
            | Self::Bc3RgbaUnorm
            | Self::Bc3RgbaUnormSrgb
            | Self::Bc4RUnorm
            | Self::Bc4RSnorm
            | Self::Bc5RgUnorm
            | Self::Bc5RgSnorm
            | Self::Bc6hRgbUfloat
            | Self::Bc6hRgbFloat
            | Self::Bc7RgbaUnorm
            | Self::Bc7RgbaUnormSrgb => (4, 4),

            Self::Etc2Rgb8Unorm
            | Self::Etc2Rgb8UnormSrgb
            | Self::Etc2Rgb8A1Unorm
            | Self::Etc2Rgb8A1UnormSrgb
            | Self::Etc2Rgba8Unorm
            | Self::Etc2Rgba8UnormSrgb
            | Self::EacR11Unorm
            | Self::EacR11Snorm
            | Self::EacRg11Unorm
            | Self::EacRg11Snorm => (4, 4),

            Self::Astc { block, .. } => match block {
                AstcBlock::B4x4 => (4, 4),
                AstcBlock::B5x4 => (5, 4),
                AstcBlock::B5x5 => (5, 5),
                AstcBlock::B6x5 => (6, 5),
                AstcBlock::B6x6 => (6, 6),
                AstcBlock::B8x5 => (8, 5),
                AstcBlock::B8x6 => (8, 6),
                AstcBlock::B8x8 => (8, 8),
                AstcBlock::B10x5 => (10, 5),
                AstcBlock::B10x6 => (10, 6),
                AstcBlock::B10x8 => (10, 8),
                AstcBlock::B10x10 => (10, 10),
                AstcBlock::B12x10 => (12, 10),
                AstcBlock::B12x12 => (12, 12),
            },
        }
    }

    /// Returns `true` for compressed formats.
    #[must_use]
    pub fn is_compressed(&self) -> bool {
        self.block_dimensions() != (1, 1)
    }

    /// Returns `true` for BCn compressed formats.
    #[must_use]
    pub fn is_bcn(&self) -> bool {
        self.required_features() == Features::TEXTURE_COMPRESSION_BC
    }

    /// Returns the required features (if any) in order to use the texture.
    #[must_use]
    pub fn required_features(&self) -> Features {
        match *self {
            Self::R8Unorm
            | Self::R8Snorm
            | Self::R8Uint
            | Self::R8Sint
            | Self::R16Uint
            | Self::R16Sint
            | Self::R16Float
            | Self::Rg8Unorm
            | Self::Rg8Snorm
            | Self::Rg8Uint
            | Self::Rg8Sint
            | Self::R32Uint
            | Self::R32Sint
            | Self::R32Float
            | Self::Rg16Uint
            | Self::Rg16Sint
            | Self::Rg16Float
            | Self::Rgba8Unorm
            | Self::Rgba8UnormSrgb
            | Self::Rgba8Snorm
            | Self::Rgba8Uint
            | Self::Rgba8Sint
            | Self::Bgra8Unorm
            | Self::Bgra8UnormSrgb
            | Self::Rgb9e5Ufloat
            | Self::Rgb10a2Uint
            | Self::Rgb10a2Unorm
            | Self::Rg11b10Ufloat
            | Self::Rg32Uint
            | Self::Rg32Sint
            | Self::Rg32Float
            | Self::Rgba16Uint
            | Self::Rgba16Sint
            | Self::Rgba16Float
            | Self::Rgba32Uint
            | Self::Rgba32Sint
            | Self::Rgba32Float
            | Self::Stencil8
            | Self::Depth16Unorm
            | Self::Depth24Plus
            | Self::Depth24PlusStencil8
            | Self::Depth32Float => Features::empty(),

            Self::Depth32FloatStencil8 => Features::DEPTH32FLOAT_STENCIL8,

            Self::NV12 => Features::TEXTURE_FORMAT_NV12,

            Self::R16Unorm
            | Self::R16Snorm
            | Self::Rg16Unorm
            | Self::Rg16Snorm
            | Self::Rgba16Unorm
            | Self::Rgba16Snorm => Features::TEXTURE_FORMAT_16BIT_NORM,

            Self::Bc1RgbaUnorm
            | Self::Bc1RgbaUnormSrgb
            | Self::Bc2RgbaUnorm
            | Self::Bc2RgbaUnormSrgb
            | Self::Bc3RgbaUnorm
            | Self::Bc3RgbaUnormSrgb
            | Self::Bc4RUnorm
            | Self::Bc4RSnorm
            | Self::Bc5RgUnorm
            | Self::Bc5RgSnorm
            | Self::Bc6hRgbUfloat
            | Self::Bc6hRgbFloat
            | Self::Bc7RgbaUnorm
            | Self::Bc7RgbaUnormSrgb => Features::TEXTURE_COMPRESSION_BC,

            Self::Etc2Rgb8Unorm
            | Self::Etc2Rgb8UnormSrgb
            | Self::Etc2Rgb8A1Unorm
            | Self::Etc2Rgb8A1UnormSrgb
            | Self::Etc2Rgba8Unorm
            | Self::Etc2Rgba8UnormSrgb
            | Self::EacR11Unorm
            | Self::EacR11Snorm
            | Self::EacRg11Unorm
            | Self::EacRg11Snorm => Features::TEXTURE_COMPRESSION_ETC2,

            Self::Astc { channel, .. } => match channel {
                AstcChannel::Hdr => Features::TEXTURE_COMPRESSION_ASTC_HDR,
                AstcChannel::Unorm | AstcChannel::UnormSrgb => Features::TEXTURE_COMPRESSION_ASTC,
            },
        }
    }

    /// Returns the format features guaranteed by the WebGPU spec.
    ///
    /// Additional features are available if `Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` is enabled.
    #[must_use]
    pub fn guaranteed_format_features(&self, device_features: Features) -> TextureFormatFeatures {
        // Multisampling
        let noaa = TextureFormatFeatureFlags::empty();
        let msaa = TextureFormatFeatureFlags::MULTISAMPLE_X4;
        let msaa_resolve = msaa | TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE;

        // Flags
        let basic =
            TextureUsages::COPY_SRC | TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING;
        let attachment = basic | TextureUsages::RENDER_ATTACHMENT;
        let storage = basic | TextureUsages::STORAGE_BINDING;
        let binding = TextureUsages::TEXTURE_BINDING;
        let all_flags = TextureUsages::all();
        let rg11b10f = if device_features.contains(Features::RG11B10UFLOAT_RENDERABLE) {
            attachment
        } else {
            basic
        };
        let bgra8unorm = if device_features.contains(Features::BGRA8UNORM_STORAGE) {
            attachment | TextureUsages::STORAGE_BINDING
        } else {
            attachment
        };

        #[rustfmt::skip] // lets make a nice table
        let (
            mut flags,
            allowed_usages,
        ) = match *self {
            Self::R8Unorm =>              (msaa_resolve, attachment),
            Self::R8Snorm =>              (        noaa,      basic),
            Self::R8Uint =>               (        msaa, attachment),
            Self::R8Sint =>               (        msaa, attachment),
            Self::R16Uint =>              (        msaa, attachment),
            Self::R16Sint =>              (        msaa, attachment),
            Self::R16Float =>             (msaa_resolve, attachment),
            Self::Rg8Unorm =>             (msaa_resolve, attachment),
            Self::Rg8Snorm =>             (        noaa,      basic),
            Self::Rg8Uint =>              (        msaa, attachment),
            Self::Rg8Sint =>              (        msaa, attachment),
            Self::R32Uint =>              (        noaa,  all_flags),
            Self::R32Sint =>              (        noaa,  all_flags),
            Self::R32Float =>             (        msaa,  all_flags),
            Self::Rg16Uint =>             (        msaa, attachment),
            Self::Rg16Sint =>             (        msaa, attachment),
            Self::Rg16Float =>            (msaa_resolve, attachment),
            Self::Rgba8Unorm =>           (msaa_resolve,  all_flags),
            Self::Rgba8UnormSrgb =>       (msaa_resolve, attachment),
            Self::Rgba8Snorm =>           (        noaa,    storage),
            Self::Rgba8Uint =>            (        msaa,  all_flags),
            Self::Rgba8Sint =>            (        msaa,  all_flags),
            Self::Bgra8Unorm =>           (msaa_resolve, bgra8unorm),
            Self::Bgra8UnormSrgb =>       (msaa_resolve, attachment),
            Self::Rgb10a2Uint =>          (        msaa, attachment),
            Self::Rgb10a2Unorm =>         (msaa_resolve, attachment),
            Self::Rg11b10Ufloat =>        (        msaa,   rg11b10f),
            Self::Rg32Uint =>             (        noaa,  all_flags),
            Self::Rg32Sint =>             (        noaa,  all_flags),
            Self::Rg32Float =>            (        noaa,  all_flags),
            Self::Rgba16Uint =>           (        msaa,  all_flags),
            Self::Rgba16Sint =>           (        msaa,  all_flags),
            Self::Rgba16Float =>          (msaa_resolve,  all_flags),
            Self::Rgba32Uint =>           (        noaa,  all_flags),
            Self::Rgba32Sint =>           (        noaa,  all_flags),
            Self::Rgba32Float =>          (        noaa,  all_flags),

            Self::Stencil8 =>             (        msaa, attachment),
            Self::Depth16Unorm =>         (        msaa, attachment),
            Self::Depth24Plus =>          (        msaa, attachment),
            Self::Depth24PlusStencil8 =>  (        msaa, attachment),
            Self::Depth32Float =>         (        msaa, attachment),
            Self::Depth32FloatStencil8 => (        msaa, attachment),

            // We only support sampling nv12 textures until we implement transfer plane data.
            Self::NV12 =>                 (        noaa,    binding),

            Self::R16Unorm =>             (        msaa,    storage),
            Self::R16Snorm =>             (        msaa,    storage),
            Self::Rg16Unorm =>            (        msaa,    storage),
            Self::Rg16Snorm =>            (        msaa,    storage),
            Self::Rgba16Unorm =>          (        msaa,    storage),
            Self::Rgba16Snorm =>          (        msaa,    storage),

            Self::Rgb9e5Ufloat =>         (        noaa,      basic),

            Self::Bc1RgbaUnorm =>         (        noaa,      basic),
            Self::Bc1RgbaUnormSrgb =>     (        noaa,      basic),
            Self::Bc2RgbaUnorm =>         (        noaa,      basic),
            Self::Bc2RgbaUnormSrgb =>     (        noaa,      basic),
            Self::Bc3RgbaUnorm =>         (        noaa,      basic),
            Self::Bc3RgbaUnormSrgb =>     (        noaa,      basic),
            Self::Bc4RUnorm =>            (        noaa,      basic),
            Self::Bc4RSnorm =>            (        noaa,      basic),
            Self::Bc5RgUnorm =>           (        noaa,      basic),
            Self::Bc5RgSnorm =>           (        noaa,      basic),
            Self::Bc6hRgbUfloat =>        (        noaa,      basic),
            Self::Bc6hRgbFloat =>         (        noaa,      basic),
            Self::Bc7RgbaUnorm =>         (        noaa,      basic),
            Self::Bc7RgbaUnormSrgb =>     (        noaa,      basic),

            Self::Etc2Rgb8Unorm =>        (        noaa,      basic),
            Self::Etc2Rgb8UnormSrgb =>    (        noaa,      basic),
            Self::Etc2Rgb8A1Unorm =>      (        noaa,      basic),
            Self::Etc2Rgb8A1UnormSrgb =>  (        noaa,      basic),
            Self::Etc2Rgba8Unorm =>       (        noaa,      basic),
            Self::Etc2Rgba8UnormSrgb =>   (        noaa,      basic),
            Self::EacR11Unorm =>          (        noaa,      basic),
            Self::EacR11Snorm =>          (        noaa,      basic),
            Self::EacRg11Unorm =>         (        noaa,      basic),
            Self::EacRg11Snorm =>         (        noaa,      basic),

            Self::Astc { .. } =>          (        noaa,      basic),
        };

        // Get whether the format is filterable, taking features into account
        let sample_type1 = self.sample_type(None, Some(device_features));
        let is_filterable = sample_type1 == Some(TextureSampleType::Float { filterable: true });

        // Features that enable filtering don't affect blendability
        let sample_type2 = self.sample_type(None, None);
        let is_blendable = sample_type2 == Some(TextureSampleType::Float { filterable: true });

        flags.set(TextureFormatFeatureFlags::FILTERABLE, is_filterable);
        flags.set(TextureFormatFeatureFlags::BLENDABLE, is_blendable);

        TextureFormatFeatures {
            allowed_usages,
            flags,
        }
    }

    /// Returns the sample type compatible with this format and aspect.
    ///
    /// Returns `None` only if this is a combined depth-stencil format or a multi-planar format
    /// and `TextureAspect::All` or no `aspect` was provided.
    #[must_use]
    pub fn sample_type(
        &self,
        aspect: Option<TextureAspect>,
        device_features: Option<Features>,
    ) -> Option<TextureSampleType> {
        let float = TextureSampleType::Float { filterable: true };
        let unfilterable_float = TextureSampleType::Float { filterable: false };
        let float32_sample_type = TextureSampleType::Float {
            filterable: device_features
                .unwrap_or(Features::empty())
                .contains(Features::FLOAT32_FILTERABLE),
        };
        let depth = TextureSampleType::Depth;
        let uint = TextureSampleType::Uint;
        let sint = TextureSampleType::Sint;

        match *self {
            Self::R8Unorm
            | Self::R8Snorm
            | Self::Rg8Unorm
            | Self::Rg8Snorm
            | Self::Rgba8Unorm
            | Self::Rgba8UnormSrgb
            | Self::Rgba8Snorm
            | Self::Bgra8Unorm
            | Self::Bgra8UnormSrgb
            | Self::R16Float
            | Self::Rg16Float
            | Self::Rgba16Float
            | Self::Rgb10a2Unorm
            | Self::Rg11b10Ufloat => Some(float),

            Self::R32Float | Self::Rg32Float | Self::Rgba32Float => Some(float32_sample_type),

            Self::R8Uint
            | Self::Rg8Uint
            | Self::Rgba8Uint
            | Self::R16Uint
            | Self::Rg16Uint
            | Self::Rgba16Uint
            | Self::R32Uint
            | Self::Rg32Uint
            | Self::Rgba32Uint
            | Self::Rgb10a2Uint => Some(uint),

            Self::R8Sint
            | Self::Rg8Sint
            | Self::Rgba8Sint
            | Self::R16Sint
            | Self::Rg16Sint
            | Self::Rgba16Sint
            | Self::R32Sint
            | Self::Rg32Sint
            | Self::Rgba32Sint => Some(sint),

            Self::Stencil8 => Some(uint),
            Self::Depth16Unorm | Self::Depth24Plus | Self::Depth32Float => Some(depth),
            Self::Depth24PlusStencil8 | Self::Depth32FloatStencil8 => match aspect {
                Some(TextureAspect::DepthOnly) => Some(depth),
                Some(TextureAspect::StencilOnly) => Some(uint),
                _ => None,
            },

            Self::NV12 => match aspect {
                Some(TextureAspect::Plane0) | Some(TextureAspect::Plane1) => {
                    Some(unfilterable_float)
                }
                _ => None,
            },

            Self::R16Unorm
            | Self::R16Snorm
            | Self::Rg16Unorm
            | Self::Rg16Snorm
            | Self::Rgba16Unorm
            | Self::Rgba16Snorm => Some(float),

            Self::Rgb9e5Ufloat => Some(float),

            Self::Bc1RgbaUnorm
            | Self::Bc1RgbaUnormSrgb
            | Self::Bc2RgbaUnorm
            | Self::Bc2RgbaUnormSrgb
            | Self::Bc3RgbaUnorm
            | Self::Bc3RgbaUnormSrgb
            | Self::Bc4RUnorm
            | Self::Bc4RSnorm
            | Self::Bc5RgUnorm
            | Self::Bc5RgSnorm
            | Self::Bc6hRgbUfloat
            | Self::Bc6hRgbFloat
            | Self::Bc7RgbaUnorm
            | Self::Bc7RgbaUnormSrgb => Some(float),

            Self::Etc2Rgb8Unorm
            | Self::Etc2Rgb8UnormSrgb
            | Self::Etc2Rgb8A1Unorm
            | Self::Etc2Rgb8A1UnormSrgb
            | Self::Etc2Rgba8Unorm
            | Self::Etc2Rgba8UnormSrgb
            | Self::EacR11Unorm
            | Self::EacR11Snorm
            | Self::EacRg11Unorm
            | Self::EacRg11Snorm => Some(float),

            Self::Astc { .. } => Some(float),
        }
    }

    /// The number of bytes one [texel block](https://gpuweb.github.io/gpuweb/#texel-block) occupies during an image copy, if applicable.
    ///
    /// Known as the [texel block copy footprint](https://gpuweb.github.io/gpuweb/#texel-block-copy-footprint).
    ///
    /// Note that for uncompressed formats this is the same as the size of a single texel,
    /// since uncompressed formats have a block size of 1x1.
    ///
    /// Returns `None` if any of the following are true:
    ///  - the format is a combined depth-stencil and no `aspect` was provided
    ///  - the format is a multi-planar format and no `aspect` was provided
    ///  - the format is `Depth24Plus`
    ///  - the format is `Depth24PlusStencil8` and `aspect` is depth.
    #[deprecated(since = "0.19.0", note = "Use `block_copy_size` instead.")]
    #[must_use]
    pub fn block_size(&self, aspect: Option<TextureAspect>) -> Option<u32> {
        self.block_copy_size(aspect)
    }

    /// The number of bytes one [texel block](https://gpuweb.github.io/gpuweb/#texel-block) occupies during an image copy, if applicable.
    ///
    /// Known as the [texel block copy footprint](https://gpuweb.github.io/gpuweb/#texel-block-copy-footprint).
    ///
    /// Note that for uncompressed formats this is the same as the size of a single texel,
    /// since uncompressed formats have a block size of 1x1.
    ///
    /// Returns `None` if any of the following are true:
    ///  - the format is a combined depth-stencil and no `aspect` was provided
    ///  - the format is a multi-planar format and no `aspect` was provided
    ///  - the format is `Depth24Plus`
    ///  - the format is `Depth24PlusStencil8` and `aspect` is depth.
    #[must_use]
    pub fn block_copy_size(&self, aspect: Option<TextureAspect>) -> Option<u32> {
        match *self {
            Self::R8Unorm | Self::R8Snorm | Self::R8Uint | Self::R8Sint => Some(1),

            Self::Rg8Unorm | Self::Rg8Snorm | Self::Rg8Uint | Self::Rg8Sint => Some(2),
            Self::R16Unorm | Self::R16Snorm | Self::R16Uint | Self::R16Sint | Self::R16Float => {
                Some(2)
            }

            Self::Rgba8Unorm
            | Self::Rgba8UnormSrgb
            | Self::Rgba8Snorm
            | Self::Rgba8Uint
            | Self::Rgba8Sint
            | Self::Bgra8Unorm
            | Self::Bgra8UnormSrgb => Some(4),
            Self::Rg16Unorm
            | Self::Rg16Snorm
            | Self::Rg16Uint
            | Self::Rg16Sint
            | Self::Rg16Float => Some(4),
            Self::R32Uint | Self::R32Sint | Self::R32Float => Some(4),
            Self::Rgb9e5Ufloat | Self::Rgb10a2Uint | Self::Rgb10a2Unorm | Self::Rg11b10Ufloat => {
                Some(4)
            }

            Self::Rgba16Unorm
            | Self::Rgba16Snorm
            | Self::Rgba16Uint
            | Self::Rgba16Sint
            | Self::Rgba16Float => Some(8),
            Self::Rg32Uint | Self::Rg32Sint | Self::Rg32Float => Some(8),

            Self::Rgba32Uint | Self::Rgba32Sint | Self::Rgba32Float => Some(16),

            Self::Stencil8 => Some(1),
            Self::Depth16Unorm => Some(2),
            Self::Depth32Float => Some(4),
            Self::Depth24Plus => None,
            Self::Depth24PlusStencil8 => match aspect {
                Some(TextureAspect::DepthOnly) => None,
                Some(TextureAspect::StencilOnly) => Some(1),
                _ => None,
            },
            Self::Depth32FloatStencil8 => match aspect {
                Some(TextureAspect::DepthOnly) => Some(4),
                Some(TextureAspect::StencilOnly) => Some(1),
                _ => None,
            },

            Self::NV12 => match aspect {
                Some(TextureAspect::Plane0) => Some(1),
                Some(TextureAspect::Plane1) => Some(2),
                _ => None,
            },

            Self::Bc1RgbaUnorm | Self::Bc1RgbaUnormSrgb | Self::Bc4RUnorm | Self::Bc4RSnorm => {
                Some(8)
            }
            Self::Bc2RgbaUnorm
            | Self::Bc2RgbaUnormSrgb
            | Self::Bc3RgbaUnorm
            | Self::Bc3RgbaUnormSrgb
            | Self::Bc5RgUnorm
            | Self::Bc5RgSnorm
            | Self::Bc6hRgbUfloat
            | Self::Bc6hRgbFloat
            | Self::Bc7RgbaUnorm
            | Self::Bc7RgbaUnormSrgb => Some(16),

            Self::Etc2Rgb8Unorm
            | Self::Etc2Rgb8UnormSrgb
            | Self::Etc2Rgb8A1Unorm
            | Self::Etc2Rgb8A1UnormSrgb
            | Self::EacR11Unorm
            | Self::EacR11Snorm => Some(8),
            Self::Etc2Rgba8Unorm
            | Self::Etc2Rgba8UnormSrgb
            | Self::EacRg11Unorm
            | Self::EacRg11Snorm => Some(16),

            Self::Astc { .. } => Some(16),
        }
    }

    /// The number of bytes occupied per pixel in a color attachment
    /// <https://gpuweb.github.io/gpuweb/#render-target-pixel-byte-cost>
    #[must_use]
    pub fn target_pixel_byte_cost(&self) -> Option<u32> {
        match *self {
            Self::R8Unorm | Self::R8Snorm | Self::R8Uint | Self::R8Sint => Some(1),
            Self::Rg8Unorm
            | Self::Rg8Snorm
            | Self::Rg8Uint
            | Self::Rg8Sint
            | Self::R16Uint
            | Self::R16Sint
            | Self::R16Unorm
            | Self::R16Snorm
            | Self::R16Float => Some(2),
            Self::Rgba8Uint
            | Self::Rgba8Sint
            | Self::Rg16Uint
            | Self::Rg16Sint
            | Self::Rg16Unorm
            | Self::Rg16Snorm
            | Self::Rg16Float
            | Self::R32Uint
            | Self::R32Sint
            | Self::R32Float => Some(4),
            Self::Rgba8Unorm
            | Self::Rgba8UnormSrgb
            | Self::Rgba8Snorm
            | Self::Bgra8Unorm
            | Self::Bgra8UnormSrgb
            | Self::Rgba16Uint
            | Self::Rgba16Sint
            | Self::Rgba16Unorm
            | Self::Rgba16Snorm
            | Self::Rgba16Float
            | Self::Rg32Uint
            | Self::Rg32Sint
            | Self::Rg32Float
            | Self::Rgb10a2Uint
            | Self::Rgb10a2Unorm
            | Self::Rg11b10Ufloat => Some(8),
            Self::Rgba32Uint | Self::Rgba32Sint | Self::Rgba32Float => Some(16),
            Self::Stencil8
            | Self::Depth16Unorm
            | Self::Depth24Plus
            | Self::Depth24PlusStencil8
            | Self::Depth32Float
            | Self::Depth32FloatStencil8
            | Self::NV12
            | Self::Rgb9e5Ufloat
            | Self::Bc1RgbaUnorm
            | Self::Bc1RgbaUnormSrgb
            | Self::Bc2RgbaUnorm
            | Self::Bc2RgbaUnormSrgb
            | Self::Bc3RgbaUnorm
            | Self::Bc3RgbaUnormSrgb
            | Self::Bc4RUnorm
            | Self::Bc4RSnorm
            | Self::Bc5RgUnorm
            | Self::Bc5RgSnorm
            | Self::Bc6hRgbUfloat
            | Self::Bc6hRgbFloat
            | Self::Bc7RgbaUnorm
            | Self::Bc7RgbaUnormSrgb
            | Self::Etc2Rgb8Unorm
            | Self::Etc2Rgb8UnormSrgb
            | Self::Etc2Rgb8A1Unorm
            | Self::Etc2Rgb8A1UnormSrgb
            | Self::Etc2Rgba8Unorm
            | Self::Etc2Rgba8UnormSrgb
            | Self::EacR11Unorm
            | Self::EacR11Snorm
            | Self::EacRg11Unorm
            | Self::EacRg11Snorm
            | Self::Astc { .. } => None,
        }
    }

    /// See <https://gpuweb.github.io/gpuweb/#render-target-component-alignment>
    #[must_use]
    pub fn target_component_alignment(&self) -> Option<u32> {
        match *self {
            Self::R8Unorm
            | Self::R8Snorm
            | Self::R8Uint
            | Self::R8Sint
            | Self::Rg8Unorm
            | Self::Rg8Snorm
            | Self::Rg8Uint
            | Self::Rg8Sint
            | Self::Rgba8Unorm
            | Self::Rgba8UnormSrgb
            | Self::Rgba8Snorm
            | Self::Rgba8Uint
            | Self::Rgba8Sint
            | Self::Bgra8Unorm
            | Self::Bgra8UnormSrgb => Some(1),
            Self::R16Uint
            | Self::R16Sint
            | Self::R16Unorm
            | Self::R16Snorm
            | Self::R16Float
            | Self::Rg16Uint
            | Self::Rg16Sint
            | Self::Rg16Unorm
            | Self::Rg16Snorm
            | Self::Rg16Float
            | Self::Rgba16Uint
            | Self::Rgba16Sint
            | Self::Rgba16Unorm
            | Self::Rgba16Snorm
            | Self::Rgba16Float => Some(2),
            Self::R32Uint
            | Self::R32Sint
            | Self::R32Float
            | Self::Rg32Uint
            | Self::Rg32Sint
            | Self::Rg32Float
            | Self::Rgba32Uint
            | Self::Rgba32Sint
            | Self::Rgba32Float
            | Self::Rgb10a2Uint
            | Self::Rgb10a2Unorm
            | Self::Rg11b10Ufloat => Some(4),
            Self::Stencil8
            | Self::Depth16Unorm
            | Self::Depth24Plus
            | Self::Depth24PlusStencil8
            | Self::Depth32Float
            | Self::Depth32FloatStencil8
            | Self::NV12
            | Self::Rgb9e5Ufloat
            | Self::Bc1RgbaUnorm
            | Self::Bc1RgbaUnormSrgb
            | Self::Bc2RgbaUnorm
            | Self::Bc2RgbaUnormSrgb
            | Self::Bc3RgbaUnorm
            | Self::Bc3RgbaUnormSrgb
            | Self::Bc4RUnorm
            | Self::Bc4RSnorm
            | Self::Bc5RgUnorm
            | Self::Bc5RgSnorm
            | Self::Bc6hRgbUfloat
            | Self::Bc6hRgbFloat
            | Self::Bc7RgbaUnorm
            | Self::Bc7RgbaUnormSrgb
            | Self::Etc2Rgb8Unorm
            | Self::Etc2Rgb8UnormSrgb
            | Self::Etc2Rgb8A1Unorm
            | Self::Etc2Rgb8A1UnormSrgb
            | Self::Etc2Rgba8Unorm
            | Self::Etc2Rgba8UnormSrgb
            | Self::EacR11Unorm
            | Self::EacR11Snorm
            | Self::EacRg11Unorm
            | Self::EacRg11Snorm
            | Self::Astc { .. } => None,
        }
    }

    /// Returns the number of components this format has.
    #[must_use]
    pub fn components(&self) -> u8 {
        self.components_with_aspect(TextureAspect::All)
    }

    /// Returns the number of components this format has taking into account the `aspect`.
    ///
    /// The `aspect` is only relevant for combined depth-stencil formats and multi-planar formats.
    #[must_use]
    pub fn components_with_aspect(&self, aspect: TextureAspect) -> u8 {
        match *self {
            Self::R8Unorm
            | Self::R8Snorm
            | Self::R8Uint
            | Self::R8Sint
            | Self::R16Unorm
            | Self::R16Snorm
            | Self::R16Uint
            | Self::R16Sint
            | Self::R16Float
            | Self::R32Uint
            | Self::R32Sint
            | Self::R32Float => 1,

            Self::Rg8Unorm
            | Self::Rg8Snorm
            | Self::Rg8Uint
            | Self::Rg8Sint
            | Self::Rg16Unorm
            | Self::Rg16Snorm
            | Self::Rg16Uint
            | Self::Rg16Sint
            | Self::Rg16Float
            | Self::Rg32Uint
            | Self::Rg32Sint
            | Self::Rg32Float => 2,

            Self::Rgba8Unorm
            | Self::Rgba8UnormSrgb
            | Self::Rgba8Snorm
            | Self::Rgba8Uint
            | Self::Rgba8Sint
            | Self::Bgra8Unorm
            | Self::Bgra8UnormSrgb
            | Self::Rgba16Unorm
            | Self::Rgba16Snorm
            | Self::Rgba16Uint
            | Self::Rgba16Sint
            | Self::Rgba16Float
            | Self::Rgba32Uint
            | Self::Rgba32Sint
            | Self::Rgba32Float => 4,

            Self::Rgb9e5Ufloat | Self::Rg11b10Ufloat => 3,
            Self::Rgb10a2Uint | Self::Rgb10a2Unorm => 4,

            Self::Stencil8 | Self::Depth16Unorm | Self::Depth24Plus | Self::Depth32Float => 1,

            Self::Depth24PlusStencil8 | Self::Depth32FloatStencil8 => match aspect {
                TextureAspect::DepthOnly | TextureAspect::StencilOnly => 1,
                _ => 2,
            },

            Self::NV12 => match aspect {
                TextureAspect::Plane0 => 1,
                TextureAspect::Plane1 => 2,
                _ => 3,
            },

            Self::Bc4RUnorm | Self::Bc4RSnorm => 1,
            Self::Bc5RgUnorm | Self::Bc5RgSnorm => 2,
            Self::Bc6hRgbUfloat | Self::Bc6hRgbFloat => 3,
            Self::Bc1RgbaUnorm
            | Self::Bc1RgbaUnormSrgb
            | Self::Bc2RgbaUnorm
            | Self::Bc2RgbaUnormSrgb
            | Self::Bc3RgbaUnorm
            | Self::Bc3RgbaUnormSrgb
            | Self::Bc7RgbaUnorm
            | Self::Bc7RgbaUnormSrgb => 4,

            Self::EacR11Unorm | Self::EacR11Snorm => 1,
            Self::EacRg11Unorm | Self::EacRg11Snorm => 2,
            Self::Etc2Rgb8Unorm | Self::Etc2Rgb8UnormSrgb => 3,
            Self::Etc2Rgb8A1Unorm
            | Self::Etc2Rgb8A1UnormSrgb
            | Self::Etc2Rgba8Unorm
            | Self::Etc2Rgba8UnormSrgb => 4,

            Self::Astc { .. } => 4,
        }
    }

    /// Strips the `Srgb` suffix from the given texture format.
    #[must_use]
    pub fn remove_srgb_suffix(&self) -> TextureFormat {
        match *self {
            Self::Rgba8UnormSrgb => Self::Rgba8Unorm,
            Self::Bgra8UnormSrgb => Self::Bgra8Unorm,
            Self::Bc1RgbaUnormSrgb => Self::Bc1RgbaUnorm,
            Self::Bc2RgbaUnormSrgb => Self::Bc2RgbaUnorm,
            Self::Bc3RgbaUnormSrgb => Self::Bc3RgbaUnorm,
            Self::Bc7RgbaUnormSrgb => Self::Bc7RgbaUnorm,
            Self::Etc2Rgb8UnormSrgb => Self::Etc2Rgb8Unorm,
            Self::Etc2Rgb8A1UnormSrgb => Self::Etc2Rgb8A1Unorm,
            Self::Etc2Rgba8UnormSrgb => Self::Etc2Rgba8Unorm,
            Self::Astc {
                block,
                channel: AstcChannel::UnormSrgb,
            } => Self::Astc {
                block,
                channel: AstcChannel::Unorm,
            },
            _ => *self,
        }
    }

    /// Adds an `Srgb` suffix to the given texture format, if the format supports it.
    #[must_use]
    pub fn add_srgb_suffix(&self) -> TextureFormat {
        match *self {
            Self::Rgba8Unorm => Self::Rgba8UnormSrgb,
            Self::Bgra8Unorm => Self::Bgra8UnormSrgb,
            Self::Bc1RgbaUnorm => Self::Bc1RgbaUnormSrgb,
            Self::Bc2RgbaUnorm => Self::Bc2RgbaUnormSrgb,
            Self::Bc3RgbaUnorm => Self::Bc3RgbaUnormSrgb,
            Self::Bc7RgbaUnorm => Self::Bc7RgbaUnormSrgb,
            Self::Etc2Rgb8Unorm => Self::Etc2Rgb8UnormSrgb,
            Self::Etc2Rgb8A1Unorm => Self::Etc2Rgb8A1UnormSrgb,
            Self::Etc2Rgba8Unorm => Self::Etc2Rgba8UnormSrgb,
            Self::Astc {
                block,
                channel: AstcChannel::Unorm,
            } => Self::Astc {
                block,
                channel: AstcChannel::UnormSrgb,
            },
            _ => *self,
        }
    }

    /// Returns `true` for srgb formats.
    #[must_use]
    pub fn is_srgb(&self) -> bool {
        *self != self.remove_srgb_suffix()
    }
}

#[test]
fn texture_format_serialize() {
    assert_eq!(
        serde_json::to_string(&TextureFormat::R8Unorm).unwrap(),
        "\"r8unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::R8Snorm).unwrap(),
        "\"r8snorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::R8Uint).unwrap(),
        "\"r8uint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::R8Sint).unwrap(),
        "\"r8sint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::R16Uint).unwrap(),
        "\"r16uint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::R16Sint).unwrap(),
        "\"r16sint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::R16Unorm).unwrap(),
        "\"r16unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::R16Snorm).unwrap(),
        "\"r16snorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::R16Float).unwrap(),
        "\"r16float\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rg8Unorm).unwrap(),
        "\"rg8unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rg8Snorm).unwrap(),
        "\"rg8snorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rg8Uint).unwrap(),
        "\"rg8uint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rg8Sint).unwrap(),
        "\"rg8sint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::R32Uint).unwrap(),
        "\"r32uint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::R32Sint).unwrap(),
        "\"r32sint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::R32Float).unwrap(),
        "\"r32float\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rg16Uint).unwrap(),
        "\"rg16uint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rg16Sint).unwrap(),
        "\"rg16sint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rg16Unorm).unwrap(),
        "\"rg16unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rg16Snorm).unwrap(),
        "\"rg16snorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rg16Float).unwrap(),
        "\"rg16float\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgba8Unorm).unwrap(),
        "\"rgba8unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgba8UnormSrgb).unwrap(),
        "\"rgba8unorm-srgb\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgba8Snorm).unwrap(),
        "\"rgba8snorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgba8Uint).unwrap(),
        "\"rgba8uint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgba8Sint).unwrap(),
        "\"rgba8sint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bgra8Unorm).unwrap(),
        "\"bgra8unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bgra8UnormSrgb).unwrap(),
        "\"bgra8unorm-srgb\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgb10a2Uint).unwrap(),
        "\"rgb10a2uint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgb10a2Unorm).unwrap(),
        "\"rgb10a2unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rg11b10Ufloat).unwrap(),
        "\"rg11b10ufloat\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rg32Uint).unwrap(),
        "\"rg32uint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rg32Sint).unwrap(),
        "\"rg32sint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rg32Float).unwrap(),
        "\"rg32float\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgba16Uint).unwrap(),
        "\"rgba16uint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgba16Sint).unwrap(),
        "\"rgba16sint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgba16Unorm).unwrap(),
        "\"rgba16unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgba16Snorm).unwrap(),
        "\"rgba16snorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgba16Float).unwrap(),
        "\"rgba16float\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgba32Uint).unwrap(),
        "\"rgba32uint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgba32Sint).unwrap(),
        "\"rgba32sint\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgba32Float).unwrap(),
        "\"rgba32float\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Stencil8).unwrap(),
        "\"stencil8\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Depth32Float).unwrap(),
        "\"depth32float\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Depth16Unorm).unwrap(),
        "\"depth16unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Depth32FloatStencil8).unwrap(),
        "\"depth32float-stencil8\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Depth24Plus).unwrap(),
        "\"depth24plus\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Depth24PlusStencil8).unwrap(),
        "\"depth24plus-stencil8\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Rgb9e5Ufloat).unwrap(),
        "\"rgb9e5ufloat\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bc1RgbaUnorm).unwrap(),
        "\"bc1-rgba-unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bc1RgbaUnormSrgb).unwrap(),
        "\"bc1-rgba-unorm-srgb\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bc2RgbaUnorm).unwrap(),
        "\"bc2-rgba-unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bc2RgbaUnormSrgb).unwrap(),
        "\"bc2-rgba-unorm-srgb\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bc3RgbaUnorm).unwrap(),
        "\"bc3-rgba-unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bc3RgbaUnormSrgb).unwrap(),
        "\"bc3-rgba-unorm-srgb\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bc4RUnorm).unwrap(),
        "\"bc4-r-unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bc4RSnorm).unwrap(),
        "\"bc4-r-snorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bc5RgUnorm).unwrap(),
        "\"bc5-rg-unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bc5RgSnorm).unwrap(),
        "\"bc5-rg-snorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bc6hRgbUfloat).unwrap(),
        "\"bc6h-rgb-ufloat\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bc6hRgbFloat).unwrap(),
        "\"bc6h-rgb-float\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bc7RgbaUnorm).unwrap(),
        "\"bc7-rgba-unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Bc7RgbaUnormSrgb).unwrap(),
        "\"bc7-rgba-unorm-srgb\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Etc2Rgb8Unorm).unwrap(),
        "\"etc2-rgb8unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Etc2Rgb8UnormSrgb).unwrap(),
        "\"etc2-rgb8unorm-srgb\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Etc2Rgb8A1Unorm).unwrap(),
        "\"etc2-rgb8a1unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Etc2Rgb8A1UnormSrgb).unwrap(),
        "\"etc2-rgb8a1unorm-srgb\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Etc2Rgba8Unorm).unwrap(),
        "\"etc2-rgba8unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::Etc2Rgba8UnormSrgb).unwrap(),
        "\"etc2-rgba8unorm-srgb\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::EacR11Unorm).unwrap(),
        "\"eac-r11unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::EacR11Snorm).unwrap(),
        "\"eac-r11snorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::EacRg11Unorm).unwrap(),
        "\"eac-rg11unorm\"".to_string()
    );
    assert_eq!(
        serde_json::to_string(&TextureFormat::EacRg11Snorm).unwrap(),
        "\"eac-rg11snorm\"".to_string()
    );
}

#[test]
fn texture_format_deserialize() {
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"r8unorm\"").unwrap(),
        TextureFormat::R8Unorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"r8snorm\"").unwrap(),
        TextureFormat::R8Snorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"r8uint\"").unwrap(),
        TextureFormat::R8Uint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"r8sint\"").unwrap(),
        TextureFormat::R8Sint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"r16uint\"").unwrap(),
        TextureFormat::R16Uint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"r16sint\"").unwrap(),
        TextureFormat::R16Sint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"r16unorm\"").unwrap(),
        TextureFormat::R16Unorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"r16snorm\"").unwrap(),
        TextureFormat::R16Snorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"r16float\"").unwrap(),
        TextureFormat::R16Float
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rg8unorm\"").unwrap(),
        TextureFormat::Rg8Unorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rg8snorm\"").unwrap(),
        TextureFormat::Rg8Snorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rg8uint\"").unwrap(),
        TextureFormat::Rg8Uint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rg8sint\"").unwrap(),
        TextureFormat::Rg8Sint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"r32uint\"").unwrap(),
        TextureFormat::R32Uint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"r32sint\"").unwrap(),
        TextureFormat::R32Sint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"r32float\"").unwrap(),
        TextureFormat::R32Float
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rg16uint\"").unwrap(),
        TextureFormat::Rg16Uint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rg16sint\"").unwrap(),
        TextureFormat::Rg16Sint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rg16unorm\"").unwrap(),
        TextureFormat::Rg16Unorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rg16snorm\"").unwrap(),
        TextureFormat::Rg16Snorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rg16float\"").unwrap(),
        TextureFormat::Rg16Float
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgba8unorm\"").unwrap(),
        TextureFormat::Rgba8Unorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgba8unorm-srgb\"").unwrap(),
        TextureFormat::Rgba8UnormSrgb
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgba8snorm\"").unwrap(),
        TextureFormat::Rgba8Snorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgba8uint\"").unwrap(),
        TextureFormat::Rgba8Uint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgba8sint\"").unwrap(),
        TextureFormat::Rgba8Sint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bgra8unorm\"").unwrap(),
        TextureFormat::Bgra8Unorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bgra8unorm-srgb\"").unwrap(),
        TextureFormat::Bgra8UnormSrgb
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgb10a2uint\"").unwrap(),
        TextureFormat::Rgb10a2Uint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgb10a2unorm\"").unwrap(),
        TextureFormat::Rgb10a2Unorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rg11b10ufloat\"").unwrap(),
        TextureFormat::Rg11b10Ufloat
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rg32uint\"").unwrap(),
        TextureFormat::Rg32Uint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rg32sint\"").unwrap(),
        TextureFormat::Rg32Sint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rg32float\"").unwrap(),
        TextureFormat::Rg32Float
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgba16uint\"").unwrap(),
        TextureFormat::Rgba16Uint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgba16sint\"").unwrap(),
        TextureFormat::Rgba16Sint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgba16unorm\"").unwrap(),
        TextureFormat::Rgba16Unorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgba16snorm\"").unwrap(),
        TextureFormat::Rgba16Snorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgba16float\"").unwrap(),
        TextureFormat::Rgba16Float
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgba32uint\"").unwrap(),
        TextureFormat::Rgba32Uint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgba32sint\"").unwrap(),
        TextureFormat::Rgba32Sint
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgba32float\"").unwrap(),
        TextureFormat::Rgba32Float
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"stencil8\"").unwrap(),
        TextureFormat::Stencil8
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"depth32float\"").unwrap(),
        TextureFormat::Depth32Float
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"depth16unorm\"").unwrap(),
        TextureFormat::Depth16Unorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"depth32float-stencil8\"").unwrap(),
        TextureFormat::Depth32FloatStencil8
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"depth24plus\"").unwrap(),
        TextureFormat::Depth24Plus
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"depth24plus-stencil8\"").unwrap(),
        TextureFormat::Depth24PlusStencil8
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"rgb9e5ufloat\"").unwrap(),
        TextureFormat::Rgb9e5Ufloat
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bc1-rgba-unorm\"").unwrap(),
        TextureFormat::Bc1RgbaUnorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bc1-rgba-unorm-srgb\"").unwrap(),
        TextureFormat::Bc1RgbaUnormSrgb
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bc2-rgba-unorm\"").unwrap(),
        TextureFormat::Bc2RgbaUnorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bc2-rgba-unorm-srgb\"").unwrap(),
        TextureFormat::Bc2RgbaUnormSrgb
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bc3-rgba-unorm\"").unwrap(),
        TextureFormat::Bc3RgbaUnorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bc3-rgba-unorm-srgb\"").unwrap(),
        TextureFormat::Bc3RgbaUnormSrgb
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bc4-r-unorm\"").unwrap(),
        TextureFormat::Bc4RUnorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bc4-r-snorm\"").unwrap(),
        TextureFormat::Bc4RSnorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bc5-rg-unorm\"").unwrap(),
        TextureFormat::Bc5RgUnorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bc5-rg-snorm\"").unwrap(),
        TextureFormat::Bc5RgSnorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bc6h-rgb-ufloat\"").unwrap(),
        TextureFormat::Bc6hRgbUfloat
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bc6h-rgb-float\"").unwrap(),
        TextureFormat::Bc6hRgbFloat
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bc7-rgba-unorm\"").unwrap(),
        TextureFormat::Bc7RgbaUnorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"bc7-rgba-unorm-srgb\"").unwrap(),
        TextureFormat::Bc7RgbaUnormSrgb
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"etc2-rgb8unorm\"").unwrap(),
        TextureFormat::Etc2Rgb8Unorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"etc2-rgb8unorm-srgb\"").unwrap(),
        TextureFormat::Etc2Rgb8UnormSrgb
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"etc2-rgb8a1unorm\"").unwrap(),
        TextureFormat::Etc2Rgb8A1Unorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"etc2-rgb8a1unorm-srgb\"").unwrap(),
        TextureFormat::Etc2Rgb8A1UnormSrgb
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"etc2-rgba8unorm\"").unwrap(),
        TextureFormat::Etc2Rgba8Unorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"etc2-rgba8unorm-srgb\"").unwrap(),
        TextureFormat::Etc2Rgba8UnormSrgb
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"eac-r11unorm\"").unwrap(),
        TextureFormat::EacR11Unorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"eac-r11snorm\"").unwrap(),
        TextureFormat::EacR11Snorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"eac-rg11unorm\"").unwrap(),
        TextureFormat::EacRg11Unorm
    );
    assert_eq!(
        serde_json::from_str::<TextureFormat>("\"eac-rg11snorm\"").unwrap(),
        TextureFormat::EacRg11Snorm
    );
}

bitflags::bitflags! {
    /// Color write mask. Disabled color channels will not be written to.
    ///
    /// Corresponds to [WebGPU `GPUColorWriteFlags`](
    /// https://gpuweb.github.io/gpuweb/#typedefdef-gpucolorwriteflags).
    #[repr(transparent)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
    pub struct ColorWrites: u32 {
        /// Enable red channel writes
        const RED = 1 << 0;
        /// Enable green channel writes
        const GREEN = 1 << 1;
        /// Enable blue channel writes
        const BLUE = 1 << 2;
        /// Enable alpha channel writes
        const ALPHA = 1 << 3;
        /// Enable red, green, and blue channel writes
        const COLOR = Self::RED.bits() | Self::GREEN.bits() | Self::BLUE.bits();
        /// Enable writes to all channels.
        const ALL = Self::RED.bits() | Self::GREEN.bits() | Self::BLUE.bits() | Self::ALPHA.bits();
    }
}

impl_bitflags!(ColorWrites);

impl Default for ColorWrites {
    fn default() -> Self {
        Self::ALL
    }
}

/// Passed to `Device::poll` to control how and if it should block.
#[derive(Clone, Debug)]
pub enum Maintain<T> {
    /// On wgpu-core based backends, block until the given submission has
    /// completed execution, and any callbacks have been invoked.
    ///
    /// On WebGPU, this has no effect. Callbacks are invoked from the
    /// window event loop.
    WaitForSubmissionIndex(T),
    /// Same as WaitForSubmissionIndex but waits for the most recent submission.
    Wait,
    /// Check the device for a single time without blocking.
    Poll,
}

impl<T> Maintain<T> {
    /// Construct a wait variant
    #[must_use]
    pub fn wait() -> Self {
        // This function seems a little silly, but it is useful to allow
        // <https://github.com/gfx-rs/wgpu/pull/5012> to be split up, as
        // it has meaning in that PR.
        Self::Wait
    }

    /// Construct a WaitForSubmissionIndex variant
    #[must_use]
    pub fn wait_for(submission_index: T) -> Self {
        // This function seems a little silly, but it is useful to allow
        // <https://github.com/gfx-rs/wgpu/pull/5012> to be split up, as
        // it has meaning in that PR.
        Self::WaitForSubmissionIndex(submission_index)
    }

    /// This maintain represents a wait of some kind.
    #[must_use]
    pub fn is_wait(&self) -> bool {
        match *self {
            Self::WaitForSubmissionIndex(..) | Self::Wait => true,
            Self::Poll => false,
        }
    }

    /// Map on the wait index type.
    #[must_use]
    pub fn map_index<U, F>(self, func: F) -> Maintain<U>
    where
        F: FnOnce(T) -> U,
    {
        match self {
            Self::WaitForSubmissionIndex(i) => Maintain::WaitForSubmissionIndex(func(i)),
            Self::Wait => Maintain::Wait,
            Self::Poll => Maintain::Poll,
        }
    }
}

/// Result of a maintain operation.
pub enum MaintainResult {
    /// There are no active submissions in flight as of the beginning of the poll call.
    /// Other submissions may have been queued on other threads at the same time.
    ///
    /// This implies that the given poll is complete.
    SubmissionQueueEmpty,
    /// More information coming soon <https://github.com/gfx-rs/wgpu/pull/5012>
    Ok,
}

impl MaintainResult {
    /// Returns true if the result is [`Self::SubmissionQueueEmpty`]`.
    #[must_use]
    pub fn is_queue_empty(&self) -> bool {
        matches!(self, Self::SubmissionQueueEmpty)
    }

    /// Panics if the MaintainResult is not Ok.
    pub fn panic_on_timeout(self) {
        let _ = self;
    }
}

/// State of the stencil operation (fixed-pipeline stage).
///
/// For use in [`DepthStencilState`].
///
/// Corresponds to a portion of [WebGPU `GPUDepthStencilState`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpudepthstencilstate).
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct StencilState {
    /// Front face mode.
    pub front: StencilFaceState,
    /// Back face mode.
    pub back: StencilFaceState,
    /// Stencil values are AND'd with this mask when reading and writing from the stencil buffer. Only low 8 bits are used.
    pub read_mask: u32,
    /// Stencil values are AND'd with this mask when writing to the stencil buffer. Only low 8 bits are used.
    pub write_mask: u32,
}

impl StencilState {
    /// Returns true if the stencil test is enabled.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        (self.front != StencilFaceState::IGNORE || self.back != StencilFaceState::IGNORE)
            && (self.read_mask != 0 || self.write_mask != 0)
    }
    /// Returns true if the state doesn't mutate the target values.
    #[must_use]
    pub fn is_read_only(&self, cull_mode: Option<Face>) -> bool {
        // The rules are defined in step 7 of the "Device timeline initialization steps"
        // subsection of the "Render Pipeline Creation" section of WebGPU
        // (link to the section: https://gpuweb.github.io/gpuweb/#render-pipeline-creation)

        if self.write_mask == 0 {
            return true;
        }

        let front_ro = cull_mode == Some(Face::Front) || self.front.is_read_only();
        let back_ro = cull_mode == Some(Face::Back) || self.back.is_read_only();

        front_ro && back_ro
    }
    /// Returns true if the stencil state uses the reference value for testing.
    #[must_use]
    pub fn needs_ref_value(&self) -> bool {
        self.front.needs_ref_value() || self.back.needs_ref_value()
    }
}

/// Describes the biasing setting for the depth target.
///
/// For use in [`DepthStencilState`].
///
/// Corresponds to a portion of [WebGPU `GPUDepthStencilState`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpudepthstencilstate).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DepthBiasState {
    /// Constant depth biasing factor, in basic units of the depth format.
    pub constant: i32,
    /// Slope depth biasing factor.
    pub slope_scale: f32,
    /// Depth bias clamp value (absolute).
    pub clamp: f32,
}

impl DepthBiasState {
    /// Returns true if the depth biasing is enabled.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.constant != 0 || self.slope_scale != 0.0
    }
}

impl Hash for DepthBiasState {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.constant.hash(state);
        self.slope_scale.to_bits().hash(state);
        self.clamp.to_bits().hash(state);
    }
}

impl PartialEq for DepthBiasState {
    fn eq(&self, other: &Self) -> bool {
        (self.constant == other.constant)
            && (self.slope_scale.to_bits() == other.slope_scale.to_bits())
            && (self.clamp.to_bits() == other.clamp.to_bits())
    }
}

impl Eq for DepthBiasState {}

/// Describes the depth/stencil state in a render pipeline.
///
/// Corresponds to [WebGPU `GPUDepthStencilState`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpudepthstencilstate).
#[repr(C)]
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DepthStencilState {
    /// Format of the depth/stencil buffer, must be special depth format. Must match the format
    /// of the depth/stencil attachment in [`CommandEncoder::begin_render_pass`][CEbrp].
    ///
    /// [CEbrp]: ../wgpu/struct.CommandEncoder.html#method.begin_render_pass
    pub format: TextureFormat,
    /// If disabled, depth will not be written to.
    pub depth_write_enabled: bool,
    /// Comparison function used to compare depth values in the depth test.
    pub depth_compare: CompareFunction,
    /// Stencil state.
    #[cfg_attr(feature = "serde", serde(default))]
    pub stencil: StencilState,
    /// Depth bias state.
    #[cfg_attr(feature = "serde", serde(default))]
    pub bias: DepthBiasState,
}

impl DepthStencilState {
    /// Returns true if the depth testing is enabled.
    #[must_use]
    pub fn is_depth_enabled(&self) -> bool {
        self.depth_compare != CompareFunction::Always || self.depth_write_enabled
    }

    /// Returns true if the state doesn't mutate the depth buffer.
    #[must_use]
    pub fn is_depth_read_only(&self) -> bool {
        !self.depth_write_enabled
    }

    /// Returns true if the state doesn't mutate the stencil.
    #[must_use]
    pub fn is_stencil_read_only(&self, cull_mode: Option<Face>) -> bool {
        self.stencil.is_read_only(cull_mode)
    }

    /// Returns true if the state doesn't mutate either depth or stencil of the target.
    #[must_use]
    pub fn is_read_only(&self, cull_mode: Option<Face>) -> bool {
        self.is_depth_read_only() && self.is_stencil_read_only(cull_mode)
    }
}

/// Format of indices used with pipeline.
///
/// Corresponds to [WebGPU `GPUIndexFormat`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpuindexformat).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum IndexFormat {
    /// Indices are 16 bit unsigned integers.
    Uint16 = 0,
    /// Indices are 32 bit unsigned integers.
    #[default]
    Uint32 = 1,
}

/// Operation to perform on the stencil value.
///
/// Corresponds to [WebGPU `GPUStencilOperation`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpustenciloperation).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum StencilOperation {
    /// Keep stencil value unchanged.
    #[default]
    Keep = 0,
    /// Set stencil value to zero.
    Zero = 1,
    /// Replace stencil value with value provided in most recent call to
    /// [`RenderPass::set_stencil_reference`][RPssr].
    ///
    /// [RPssr]: ../wgpu/struct.RenderPass.html#method.set_stencil_reference
    Replace = 2,
    /// Bitwise inverts stencil value.
    Invert = 3,
    /// Increments stencil value by one, clamping on overflow.
    IncrementClamp = 4,
    /// Decrements stencil value by one, clamping on underflow.
    DecrementClamp = 5,
    /// Increments stencil value by one, wrapping on overflow.
    IncrementWrap = 6,
    /// Decrements stencil value by one, wrapping on underflow.
    DecrementWrap = 7,
}

/// Describes stencil state in a render pipeline.
///
/// If you are not using stencil state, set this to [`StencilFaceState::IGNORE`].
///
/// Corresponds to [WebGPU `GPUStencilFaceState`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpustencilfacestate).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct StencilFaceState {
    /// Comparison function that determines if the fail_op or pass_op is used on the stencil buffer.
    pub compare: CompareFunction,
    /// Operation that is performed when stencil test fails.
    pub fail_op: StencilOperation,
    /// Operation that is performed when depth test fails but stencil test succeeds.
    pub depth_fail_op: StencilOperation,
    /// Operation that is performed when stencil test success.
    pub pass_op: StencilOperation,
}

impl StencilFaceState {
    /// Ignore the stencil state for the face.
    pub const IGNORE: Self = StencilFaceState {
        compare: CompareFunction::Always,
        fail_op: StencilOperation::Keep,
        depth_fail_op: StencilOperation::Keep,
        pass_op: StencilOperation::Keep,
    };

    /// Returns true if the face state uses the reference value for testing or operation.
    #[must_use]
    pub fn needs_ref_value(&self) -> bool {
        self.compare.needs_ref_value()
            || self.fail_op == StencilOperation::Replace
            || self.depth_fail_op == StencilOperation::Replace
            || self.pass_op == StencilOperation::Replace
    }

    /// Returns true if the face state doesn't mutate the target values.
    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.pass_op == StencilOperation::Keep
            && self.depth_fail_op == StencilOperation::Keep
            && self.fail_op == StencilOperation::Keep
    }
}

impl Default for StencilFaceState {
    fn default() -> Self {
        Self::IGNORE
    }
}

/// Comparison function used for depth and stencil operations.
///
/// Corresponds to [WebGPU `GPUCompareFunction`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpucomparefunction).
#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum CompareFunction {
    /// Function never passes
    Never = 1,
    /// Function passes if new value less than existing value
    Less = 2,
    /// Function passes if new value is equal to existing value. When using
    /// this compare function, make sure to mark your Vertex Shader's `@builtin(position)`
    /// output as `@invariant` to prevent artifacting.
    Equal = 3,
    /// Function passes if new value is less than or equal to existing value
    LessEqual = 4,
    /// Function passes if new value is greater than existing value
    Greater = 5,
    /// Function passes if new value is not equal to existing value. When using
    /// this compare function, make sure to mark your Vertex Shader's `@builtin(position)`
    /// output as `@invariant` to prevent artifacting.
    NotEqual = 6,
    /// Function passes if new value is greater than or equal to existing value
    GreaterEqual = 7,
    /// Function always passes
    Always = 8,
}

impl CompareFunction {
    /// Returns true if the comparison depends on the reference value.
    #[must_use]
    pub fn needs_ref_value(self) -> bool {
        match self {
            Self::Never | Self::Always => false,
            _ => true,
        }
    }
}

/// Whether a vertex buffer is indexed by vertex or by instance.
///
/// Consider a call to [`RenderPass::draw`] like this:
///
/// ```ignore
/// render_pass.draw(vertices, instances)
/// ```
///
/// where `vertices` is a `Range<u32>` of vertex indices, and
/// `instances` is a `Range<u32>` of instance indices.
///
/// For this call, `wgpu` invokes the vertex shader entry point once
/// for every possible `(v, i)` pair, where `v` is drawn from
/// `vertices` and `i` is drawn from `instances`. These invocations
/// may happen in any order, and will usually run in parallel.
///
/// Each vertex buffer has a step mode, established by the
/// [`step_mode`] field of its [`VertexBufferLayout`], given when the
/// pipeline was created. Buffers whose step mode is [`Vertex`] use
/// `v` as the index into their contents, whereas buffers whose step
/// mode is [`Instance`] use `i`. The indicated buffer element then
/// contributes zero or more attribute values for the `(v, i)` vertex
/// shader invocation to use, based on the [`VertexBufferLayout`]'s
/// [`attributes`] list.
///
/// You can visualize the results from all these vertex shader
/// invocations as a matrix with a row for each `i` from `instances`,
/// and with a column for each `v` from `vertices`. In one sense, `v`
/// and `i` are symmetrical: both are used to index vertex buffers and
/// provide attribute values.  But the key difference between `v` and
/// `i` is that line and triangle primitives are built from the values
/// of each row, along which `i` is constant and `v` varies, not the
/// columns.
///
/// An indexed draw call works similarly:
///
/// ```ignore
/// render_pass.draw_indexed(indices, base_vertex, instances)
/// ```
///
/// The only difference is that `v` values are drawn from the contents
/// of the index buffer&mdash;specifically, the subrange of the index
/// buffer given by `indices`&mdash;instead of simply being sequential
/// integers, as they are in a `draw` call.
///
/// A non-instanced call, where `instances` is `0..1`, is simply a
/// matrix with only one row.
///
/// Corresponds to [WebGPU `GPUVertexStepMode`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpuvertexstepmode).
///
/// [`RenderPass::draw`]: ../wgpu/struct.RenderPass.html#method.draw
/// [`VertexBufferLayout`]: ../wgpu/struct.VertexBufferLayout.html
/// [`step_mode`]: ../wgpu/struct.VertexBufferLayout.html#structfield.step_mode
/// [`attributes`]: ../wgpu/struct.VertexBufferLayout.html#structfield.attributes
/// [`Vertex`]: VertexStepMode::Vertex
/// [`Instance`]: VertexStepMode::Instance
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum VertexStepMode {
    /// Vertex data is advanced every vertex.
    #[default]
    Vertex = 0,
    /// Vertex data is advanced every instance.
    Instance = 1,
}

/// Vertex inputs (attributes) to shaders.
///
/// Arrays of these can be made with the [`vertex_attr_array`]
/// macro. Vertex attributes are assumed to be tightly packed.
///
/// Corresponds to [WebGPU `GPUVertexAttribute`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpuvertexattribute).
///
/// [`vertex_attr_array`]: ../wgpu/macro.vertex_attr_array.html
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct VertexAttribute {
    /// Format of the input
    pub format: VertexFormat,
    /// Byte offset of the start of the input
    pub offset: BufferAddress,
    /// Location for this input. Must match the location in the shader.
    pub shader_location: ShaderLocation,
}

/// Vertex Format for a [`VertexAttribute`] (input).
///
/// Corresponds to [WebGPU `GPUVertexFormat`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpuvertexformat).
#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum VertexFormat {
    /// Two unsigned bytes (u8). `vec2<u32>` in shaders.
    Uint8x2 = 0,
    /// Four unsigned bytes (u8). `vec4<u32>` in shaders.
    Uint8x4 = 1,
    /// Two signed bytes (i8). `vec2<i32>` in shaders.
    Sint8x2 = 2,
    /// Four signed bytes (i8). `vec4<i32>` in shaders.
    Sint8x4 = 3,
    /// Two unsigned bytes (u8). [0, 255] converted to float [0, 1] `vec2<f32>` in shaders.
    Unorm8x2 = 4,
    /// Four unsigned bytes (u8). [0, 255] converted to float [0, 1] `vec4<f32>` in shaders.
    Unorm8x4 = 5,
    /// Two signed bytes (i8). [-127, 127] converted to float [-1, 1] `vec2<f32>` in shaders.
    Snorm8x2 = 6,
    /// Four signed bytes (i8). [-127, 127] converted to float [-1, 1] `vec4<f32>` in shaders.
    Snorm8x4 = 7,
    /// Two unsigned shorts (u16). `vec2<u32>` in shaders.
    Uint16x2 = 8,
    /// Four unsigned shorts (u16). `vec4<u32>` in shaders.
    Uint16x4 = 9,
    /// Two signed shorts (i16). `vec2<i32>` in shaders.
    Sint16x2 = 10,
    /// Four signed shorts (i16). `vec4<i32>` in shaders.
    Sint16x4 = 11,
    /// Two unsigned shorts (u16). [0, 65535] converted to float [0, 1] `vec2<f32>` in shaders.
    Unorm16x2 = 12,
    /// Four unsigned shorts (u16). [0, 65535] converted to float [0, 1] `vec4<f32>` in shaders.
    Unorm16x4 = 13,
    /// Two signed shorts (i16). [-32767, 32767] converted to float [-1, 1] `vec2<f32>` in shaders.
    Snorm16x2 = 14,
    /// Four signed shorts (i16). [-32767, 32767] converted to float [-1, 1] `vec4<f32>` in shaders.
    Snorm16x4 = 15,
    /// Two half-precision floats (no Rust equiv). `vec2<f32>` in shaders.
    Float16x2 = 16,
    /// Four half-precision floats (no Rust equiv). `vec4<f32>` in shaders.
    Float16x4 = 17,
    /// One single-precision float (f32). `f32` in shaders.
    Float32 = 18,
    /// Two single-precision floats (f32). `vec2<f32>` in shaders.
    Float32x2 = 19,
    /// Three single-precision floats (f32). `vec3<f32>` in shaders.
    Float32x3 = 20,
    /// Four single-precision floats (f32). `vec4<f32>` in shaders.
    Float32x4 = 21,
    /// One unsigned int (u32). `u32` in shaders.
    Uint32 = 22,
    /// Two unsigned ints (u32). `vec2<u32>` in shaders.
    Uint32x2 = 23,
    /// Three unsigned ints (u32). `vec3<u32>` in shaders.
    Uint32x3 = 24,
    /// Four unsigned ints (u32). `vec4<u32>` in shaders.
    Uint32x4 = 25,
    /// One signed int (i32). `i32` in shaders.
    Sint32 = 26,
    /// Two signed ints (i32). `vec2<i32>` in shaders.
    Sint32x2 = 27,
    /// Three signed ints (i32). `vec3<i32>` in shaders.
    Sint32x3 = 28,
    /// Four signed ints (i32). `vec4<i32>` in shaders.
    Sint32x4 = 29,
    /// One double-precision float (f64). `f32` in shaders. Requires [`Features::VERTEX_ATTRIBUTE_64BIT`].
    Float64 = 30,
    /// Two double-precision floats (f64). `vec2<f32>` in shaders. Requires [`Features::VERTEX_ATTRIBUTE_64BIT`].
    Float64x2 = 31,
    /// Three double-precision floats (f64). `vec3<f32>` in shaders. Requires [`Features::VERTEX_ATTRIBUTE_64BIT`].
    Float64x3 = 32,
    /// Four double-precision floats (f64). `vec4<f32>` in shaders. Requires [`Features::VERTEX_ATTRIBUTE_64BIT`].
    Float64x4 = 33,
    /// Three unsigned 10-bit integers and one 2-bit integer, packed into a 32-bit integer (u32). [0, 1024] converted to float [0, 1] `vec4<f32>` in shaders.
    #[cfg_attr(feature = "serde", serde(rename = "unorm10-10-10-2"))]
    Unorm10_10_10_2 = 34,
}

impl VertexFormat {
    /// Returns the byte size of the format.
    #[must_use]
    pub const fn size(&self) -> u64 {
        match self {
            Self::Uint8x2 | Self::Sint8x2 | Self::Unorm8x2 | Self::Snorm8x2 => 2,
            Self::Uint8x4
            | Self::Sint8x4
            | Self::Unorm8x4
            | Self::Snorm8x4
            | Self::Uint16x2
            | Self::Sint16x2
            | Self::Unorm16x2
            | Self::Snorm16x2
            | Self::Float16x2
            | Self::Float32
            | Self::Uint32
            | Self::Sint32
            | Self::Unorm10_10_10_2 => 4,
            Self::Uint16x4
            | Self::Sint16x4
            | Self::Unorm16x4
            | Self::Snorm16x4
            | Self::Float16x4
            | Self::Float32x2
            | Self::Uint32x2
            | Self::Sint32x2
            | Self::Float64 => 8,
            Self::Float32x3 | Self::Uint32x3 | Self::Sint32x3 => 12,
            Self::Float32x4 | Self::Uint32x4 | Self::Sint32x4 | Self::Float64x2 => 16,
            Self::Float64x3 => 24,
            Self::Float64x4 => 32,
        }
    }
}

bitflags::bitflags! {
    /// Different ways that you can use a buffer.
    ///
    /// The usages determine what kind of memory the buffer is allocated from and what
    /// actions the buffer can partake in.
    ///
    /// Corresponds to [WebGPU `GPUBufferUsageFlags`](
    /// https://gpuweb.github.io/gpuweb/#typedefdef-gpubufferusageflags).
    #[repr(transparent)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
    pub struct BufferUsages: u32 {
        /// Allow a buffer to be mapped for reading using [`Buffer::map_async`] + [`Buffer::get_mapped_range`].
        /// This does not include creating a buffer with [`BufferDescriptor::mapped_at_creation`] set.
        ///
        /// If [`Features::MAPPABLE_PRIMARY_BUFFERS`] isn't enabled, the only other usage a buffer
        /// may have is COPY_DST.
        const MAP_READ = 1 << 0;
        /// Allow a buffer to be mapped for writing using [`Buffer::map_async`] + [`Buffer::get_mapped_range_mut`].
        /// This does not include creating a buffer with `mapped_at_creation` set.
        ///
        /// If [`Features::MAPPABLE_PRIMARY_BUFFERS`] feature isn't enabled, the only other usage a buffer
        /// may have is COPY_SRC.
        const MAP_WRITE = 1 << 1;
        /// Allow a buffer to be the source buffer for a [`CommandEncoder::copy_buffer_to_buffer`] or [`CommandEncoder::copy_buffer_to_texture`]
        /// operation.
        const COPY_SRC = 1 << 2;
        /// Allow a buffer to be the destination buffer for a [`CommandEncoder::copy_buffer_to_buffer`], [`CommandEncoder::copy_texture_to_buffer`],
        /// [`CommandEncoder::clear_buffer`] or [`Queue::write_buffer`] operation.
        const COPY_DST = 1 << 3;
        /// Allow a buffer to be the index buffer in a draw operation.
        const INDEX = 1 << 4;
        /// Allow a buffer to be the vertex buffer in a draw operation.
        const VERTEX = 1 << 5;
        /// Allow a buffer to be a [`BufferBindingType::Uniform`] inside a bind group.
        const UNIFORM = 1 << 6;
        /// Allow a buffer to be a [`BufferBindingType::Storage`] inside a bind group.
        const STORAGE = 1 << 7;
        /// Allow a buffer to be the indirect buffer in an indirect draw call.
        const INDIRECT = 1 << 8;
        /// Allow a buffer to be the destination buffer for a [`CommandEncoder::resolve_query_set`] operation.
        const QUERY_RESOLVE = 1 << 9;
        /// Allows a buffer to be used as input for a bottom level acceleration structure build
        const BLAS_INPUT = 1 << 10;
        /// Allows a buffer to be used as input for a top level acceleration structure build
        const TLAS_INPUT = 1 << 11;
    }
}

impl_bitflags!(BufferUsages);

/// Describes a [`Buffer`](../wgpu/struct.Buffer.html).
///
/// Corresponds to [WebGPU `GPUBufferDescriptor`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpubufferdescriptor).
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BufferDescriptor<L> {
    /// Debug label of a buffer. This will show up in graphics debuggers for easy identification.
    pub label: L,
    /// Size of a buffer, in bytes.
    pub size: BufferAddress,
    /// Usages of a buffer. If the buffer is used in any way that isn't specified here, the operation
    /// will panic.
    pub usage: BufferUsages,
    /// Allows a buffer to be mapped immediately after they are made. It does not have to be [`BufferUsages::MAP_READ`] or
    /// [`BufferUsages::MAP_WRITE`], all buffers are allowed to be mapped at creation.
    ///
    /// If this is `true`, [`size`](#structfield.size) must be a multiple of
    /// [`COPY_BUFFER_ALIGNMENT`].
    pub mapped_at_creation: bool,
}

impl<L> BufferDescriptor<L> {
    /// Takes a closure and maps the label of the buffer descriptor into another.
    #[must_use]
    pub fn map_label<K>(&self, fun: impl FnOnce(&L) -> K) -> BufferDescriptor<K> {
        BufferDescriptor {
            label: fun(&self.label),
            size: self.size,
            usage: self.usage,
            mapped_at_creation: self.mapped_at_creation,
        }
    }
}

/// Describes a [`CommandEncoder`](../wgpu/struct.CommandEncoder.html).
///
/// Corresponds to [WebGPU `GPUCommandEncoderDescriptor`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpucommandencoderdescriptor).
#[repr(C)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CommandEncoderDescriptor<L> {
    /// Debug label for the command encoder. This will show up in graphics debuggers for easy identification.
    pub label: L,
}

impl<L> CommandEncoderDescriptor<L> {
    /// Takes a closure and maps the label of the command encoder descriptor into another.
    #[must_use]
    pub fn map_label<K>(&self, fun: impl FnOnce(&L) -> K) -> CommandEncoderDescriptor<K> {
        CommandEncoderDescriptor {
            label: fun(&self.label),
        }
    }
}

impl<T> Default for CommandEncoderDescriptor<Option<T>> {
    fn default() -> Self {
        Self { label: None }
    }
}

/// Behavior of the presentation engine based on frame rate.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PresentMode {
    /// Chooses FifoRelaxed -> Fifo based on availability.
    ///
    /// Because of the fallback behavior, it is supported everywhere.
    AutoVsync = 0,
    /// Chooses Immediate -> Mailbox -> Fifo (on web) based on availability.
    ///
    /// Because of the fallback behavior, it is supported everywhere.
    AutoNoVsync = 1,
    /// Presentation frames are kept in a First-In-First-Out queue approximately 3 frames
    /// long. Every vertical blanking period, the presentation engine will pop a frame
    /// off the queue to display. If there is no frame to display, it will present the same
    /// frame again until the next vblank.
    ///
    /// When a present command is executed on the gpu, the presented image is added on the queue.
    ///
    /// No tearing will be observed.
    ///
    /// Calls to get_current_texture will block until there is a spot in the queue.
    ///
    /// Supported on all platforms.
    ///
    /// If you don't know what mode to choose, choose this mode. This is traditionally called "Vsync On".
    #[default]
    Fifo = 2,
    /// Presentation frames are kept in a First-In-First-Out queue approximately 3 frames
    /// long. Every vertical blanking period, the presentation engine will pop a frame
    /// off the queue to display. If there is no frame to display, it will present the
    /// same frame until there is a frame in the queue. The moment there is a frame in the
    /// queue, it will immediately pop the frame off the queue.
    ///
    /// When a present command is executed on the gpu, the presented image is added on the queue.
    ///
    /// Tearing will be observed if frames last more than one vblank as the front buffer.
    ///
    /// Calls to get_current_texture will block until there is a spot in the queue.
    ///
    /// Supported on AMD on Vulkan.
    ///
    /// This is traditionally called "Adaptive Vsync"
    FifoRelaxed = 3,
    /// Presentation frames are not queued at all. The moment a present command
    /// is executed on the GPU, the presented image is swapped onto the front buffer
    /// immediately.
    ///
    /// Tearing can be observed.
    ///
    /// Supported on most platforms except older DX12 and Wayland.
    ///
    /// This is traditionally called "Vsync Off".
    Immediate = 4,
    /// Presentation frames are kept in a single-frame queue. Every vertical blanking period,
    /// the presentation engine will pop a frame from the queue. If there is no frame to display,
    /// it will present the same frame again until the next vblank.
    ///
    /// When a present command is executed on the gpu, the frame will be put into the queue.
    /// If there was already a frame in the queue, the new frame will _replace_ the old frame
    /// on the queue.
    ///
    /// No tearing will be observed.
    ///
    /// Supported on DX12 on Windows 10, NVidia on Vulkan and Wayland on Vulkan.
    ///
    /// This is traditionally called "Fast Vsync"
    Mailbox = 5,
}

/// Specifies how the alpha channel of the textures should be handled during
/// compositing.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum CompositeAlphaMode {
    /// Chooses either `Opaque` or `Inherit` automatically，depending on the
    /// `alpha_mode` that the current surface can support.
    Auto = 0,
    /// The alpha channel, if it exists, of the textures is ignored in the
    /// compositing process. Instead, the textures is treated as if it has a
    /// constant alpha of 1.0.
    Opaque = 1,
    /// The alpha channel, if it exists, of the textures is respected in the
    /// compositing process. The non-alpha channels of the textures are
    /// expected to already be multiplied by the alpha channel by the
    /// application.
    PreMultiplied = 2,
    /// The alpha channel, if it exists, of the textures is respected in the
    /// compositing process. The non-alpha channels of the textures are not
    /// expected to already be multiplied by the alpha channel by the
    /// application; instead, the compositor will multiply the non-alpha
    /// channels of the texture by the alpha channel during compositing.
    PostMultiplied = 3,
    /// The alpha channel, if it exists, of the textures is unknown for processing
    /// during compositing. Instead, the application is responsible for setting
    /// the composite alpha blending mode using native WSI command. If not set,
    /// then a platform-specific default will be used.
    Inherit = 4,
}

impl Default for CompositeAlphaMode {
    fn default() -> Self {
        Self::Auto
    }
}

bitflags::bitflags! {
    /// Different ways that you can use a texture.
    ///
    /// The usages determine what kind of memory the texture is allocated from and what
    /// actions the texture can partake in.
    ///
    /// Corresponds to [WebGPU `GPUTextureUsageFlags`](
    /// https://gpuweb.github.io/gpuweb/#typedefdef-gputextureusageflags).
    #[repr(transparent)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
    pub struct TextureUsages: u32 {
        /// Allows a texture to be the source in a [`CommandEncoder::copy_texture_to_buffer`] or
        /// [`CommandEncoder::copy_texture_to_texture`] operation.
        const COPY_SRC = 1 << 0;
        /// Allows a texture to be the destination in a  [`CommandEncoder::copy_buffer_to_texture`],
        /// [`CommandEncoder::copy_texture_to_texture`], or [`Queue::write_texture`] operation.
        const COPY_DST = 1 << 1;
        /// Allows a texture to be a [`BindingType::Texture`] in a bind group.
        const TEXTURE_BINDING = 1 << 2;
        /// Allows a texture to be a [`BindingType::StorageTexture`] in a bind group.
        const STORAGE_BINDING = 1 << 3;
        /// Allows a texture to be an output attachment of a render pass.
        const RENDER_ATTACHMENT = 1 << 4;
    }
}

impl_bitflags!(TextureUsages);

/// Defines the capabilities of a given surface and adapter.
#[derive(Debug)]
pub struct SurfaceCapabilities {
    /// List of supported formats to use with the given adapter. The first format in the vector is preferred.
    ///
    /// Returns an empty vector if the surface is incompatible with the adapter.
    pub formats: Vec<TextureFormat>,
    /// List of supported presentation modes to use with the given adapter.
    ///
    /// Returns an empty vector if the surface is incompatible with the adapter.
    pub present_modes: Vec<PresentMode>,
    /// List of supported alpha modes to use with the given adapter.
    ///
    /// Will return at least one element, CompositeAlphaMode::Opaque or CompositeAlphaMode::Inherit.
    pub alpha_modes: Vec<CompositeAlphaMode>,
    /// Bitflag of supported texture usages for the surface to use with the given adapter.
    ///
    /// The usage TextureUsages::RENDER_ATTACHMENT is guaranteed.
    pub usages: TextureUsages,
}

impl Default for SurfaceCapabilities {
    fn default() -> Self {
        Self {
            formats: Vec::new(),
            present_modes: Vec::new(),
            alpha_modes: vec![CompositeAlphaMode::Opaque],
            usages: TextureUsages::RENDER_ATTACHMENT,
        }
    }
}

/// Configures a [`Surface`] for presentation.
///
/// [`Surface`]: ../wgpu/struct.Surface.html
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SurfaceConfiguration<V> {
    /// The usage of the swap chain. The only supported usage is `RENDER_ATTACHMENT`.
    pub usage: TextureUsages,
    /// The texture format of the swap chain. The only formats that are guaranteed are
    /// `Bgra8Unorm` and `Bgra8UnormSrgb`
    pub format: TextureFormat,
    /// Width of the swap chain. Must be the same size as the surface, and nonzero.
    ///
    /// If this is not the same size as the underlying surface (e.g. if it is
    /// set once, and the window is later resized), the behaviour is defined
    /// but platform-specific, and may change in the future (currently macOS
    /// scales the surface, other platforms may do something else).
    pub width: u32,
    /// Height of the swap chain. Must be the same size as the surface, and nonzero.
    ///
    /// If this is not the same size as the underlying surface (e.g. if it is
    /// set once, and the window is later resized), the behaviour is defined
    /// but platform-specific, and may change in the future (currently macOS
    /// scales the surface, other platforms may do something else).
    pub height: u32,
    /// Presentation mode of the swap chain. Fifo is the only mode guaranteed to be supported.
    /// FifoRelaxed, Immediate, and Mailbox will crash if unsupported, while AutoVsync and
    /// AutoNoVsync will gracefully do a designed sets of fallbacks if their primary modes are
    /// unsupported.
    pub present_mode: PresentMode,
    /// Desired maximum number of frames that the presentation engine should queue in advance.
    ///
    /// This is a hint to the backend implementation and will always be clamped to the supported range.
    /// As a consequence, either the maximum frame latency is set directly on the swap chain,
    /// or waits on present are scheduled to avoid exceeding the maximum frame latency if supported,
    /// or the swap chain size is set to (max-latency + 1).
    ///
    /// Defaults to 2 when created via `wgpu::Surface::get_default_config`.
    ///
    /// Typical values range from 3 to 1, but higher values are possible:
    /// * Choose 2 or higher for potentially smoother frame display, as it allows to be at least one frame
    ///   to be queued up. This typically avoids starving the GPU's work queue.
    ///   Higher values are useful for achieving a constant flow of frames to the display under varying load.
    /// * Choose 1 for low latency from frame recording to frame display.
    ///   ⚠️ If the backend does not support waiting on present, this will cause the CPU to wait for the GPU
    ///   to finish all work related to the previous frame when calling `wgpu::Surface::get_current_texture`,
    ///   causing CPU-GPU serialization (i.e. when `wgpu::Surface::get_current_texture` returns, the GPU might be idle).
    ///   It is currently not possible to query this. See <https://github.com/gfx-rs/wgpu/issues/2869>.
    /// * A value of 0 is generally not supported and always clamped to a higher value.
    pub desired_maximum_frame_latency: u32,
    /// Specifies how the alpha channel of the textures should be handled during compositing.
    pub alpha_mode: CompositeAlphaMode,
    /// Specifies what view formats will be allowed when calling create_view() on texture returned by get_current_texture().
    ///
    /// View formats of the same format as the texture are always allowed.
    ///
    /// Note: currently, only the srgb-ness is allowed to change. (ex: Rgba8Unorm texture + Rgba8UnormSrgb view)
    pub view_formats: V,
}

impl<V: Clone> SurfaceConfiguration<V> {
    /// Map view_formats of the texture descriptor into another.
    pub fn map_view_formats<M>(&self, fun: impl FnOnce(V) -> M) -> SurfaceConfiguration<M> {
        SurfaceConfiguration {
            usage: self.usage,
            format: self.format,
            width: self.width,
            height: self.height,
            present_mode: self.present_mode,
            desired_maximum_frame_latency: self.desired_maximum_frame_latency,
            alpha_mode: self.alpha_mode,
            view_formats: fun(self.view_formats.clone()),
        }
    }
}

/// Status of the received surface image.
#[repr(C)]
#[derive(Debug)]
pub enum SurfaceStatus {
    /// No issues.
    Good,
    /// The swap chain is operational, but it does no longer perfectly
    /// match the surface. A re-configuration is needed.
    Suboptimal,
    /// Unable to get the next frame, timed out.
    Timeout,
    /// The surface under the swap chain has changed.
    Outdated,
    /// The surface under the swap chain is lost.
    Lost,
}

/// Nanosecond timestamp used by the presentation engine.
///
/// The specific clock depends on the window system integration (WSI) API used.
///
/// <table>
/// <tr>
///     <td>WSI</td>
///     <td>Clock</td>
/// </tr>
/// <tr>
///     <td>IDXGISwapchain</td>
///     <td><a href="https://docs.microsoft.com/en-us/windows/win32/api/profileapi/nf-profileapi-queryperformancecounter">QueryPerformanceCounter</a></td>
/// </tr>
/// <tr>
///     <td>IPresentationManager</td>
///     <td><a href="https://docs.microsoft.com/en-us/windows/win32/api/realtimeapiset/nf-realtimeapiset-queryinterrupttimeprecise">QueryInterruptTimePrecise</a></td>
/// </tr>
/// <tr>
///     <td>CAMetalLayer</td>
///     <td><a href="https://developer.apple.com/documentation/kernel/1462446-mach_absolute_time">mach_absolute_time</a></td>
/// </tr>
/// <tr>
///     <td>VK_GOOGLE_display_timing</td>
///     <td><a href="https://linux.die.net/man/3/clock_gettime">clock_gettime(CLOCK_MONOTONIC)</a></td>
/// </tr>
/// </table>
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PresentationTimestamp(
    /// Timestamp in nanoseconds.
    pub u128,
);

impl PresentationTimestamp {
    /// A timestamp that is invalid due to the platform not having a timestamp system.
    pub const INVALID_TIMESTAMP: Self = Self(u128::MAX);

    /// Returns true if this timestamp is the invalid timestamp.
    #[must_use]
    pub fn is_invalid(self) -> bool {
        self == Self::INVALID_TIMESTAMP
    }
}

/// RGBA double precision color.
///
/// This is not to be used as a generic color type, only for specific wgpu interfaces.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Color {
    /// Red component of the color
    pub r: f64,
    /// Green component of the color
    pub g: f64,
    /// Blue component of the color
    pub b: f64,
    /// Alpha component of the color
    pub a: f64,
}

#[allow(missing_docs)]
impl Color {
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    pub const RED: Self = Self {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    pub const GREEN: Self = Self {
        r: 0.0,
        g: 1.0,
        b: 0.0,
        a: 1.0,
    };
    pub const BLUE: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    };
}

/// Dimensionality of a texture.
///
/// Corresponds to [WebGPU `GPUTextureDimension`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gputexturedimension).
#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TextureDimension {
    /// 1D texture
    #[cfg_attr(feature = "serde", serde(rename = "1d"))]
    D1,
    /// 2D texture
    #[cfg_attr(feature = "serde", serde(rename = "2d"))]
    D2,
    /// 3D texture
    #[cfg_attr(feature = "serde", serde(rename = "3d"))]
    D3,
}

/// Origin of a copy from a 2D image.
///
/// Corresponds to [WebGPU `GPUOrigin2D`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpuorigin2ddict).
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Origin2d {
    #[allow(missing_docs)]
    pub x: u32,
    #[allow(missing_docs)]
    pub y: u32,
}

impl Origin2d {
    /// Zero origin.
    pub const ZERO: Self = Self { x: 0, y: 0 };

    /// Adds the third dimension to this origin
    #[must_use]
    pub fn to_3d(self, z: u32) -> Origin3d {
        Origin3d {
            x: self.x,
            y: self.y,
            z,
        }
    }
}

impl std::fmt::Debug for Origin2d {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (self.x, self.y).fmt(f)
    }
}

/// Origin of a copy to/from a texture.
///
/// Corresponds to [WebGPU `GPUOrigin3D`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpuorigin3ddict).
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Origin3d {
    /// X position of the origin
    pub x: u32,
    /// Y position of the origin
    pub y: u32,
    /// Z position of the origin
    pub z: u32,
}

impl Origin3d {
    /// Zero origin.
    pub const ZERO: Self = Self { x: 0, y: 0, z: 0 };

    /// Removes the third dimension from this origin
    #[must_use]
    pub fn to_2d(self) -> Origin2d {
        Origin2d {
            x: self.x,
            y: self.y,
        }
    }
}

impl Default for Origin3d {
    fn default() -> Self {
        Self::ZERO
    }
}

impl std::fmt::Debug for Origin3d {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (self.x, self.y, self.z).fmt(f)
    }
}

/// Extent of a texture related operation.
///
/// Corresponds to [WebGPU `GPUExtent3D`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpuextent3ddict).
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Extent3d {
    /// Width of the extent
    pub width: u32,
    /// Height of the extent
    pub height: u32,
    /// The depth of the extent or the number of array layers
    #[cfg_attr(feature = "serde", serde(default = "default_depth"))]
    pub depth_or_array_layers: u32,
}

impl std::fmt::Debug for Extent3d {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (self.width, self.height, self.depth_or_array_layers).fmt(f)
    }
}

#[cfg(feature = "serde")]
fn default_depth() -> u32 {
    1
}

impl Default for Extent3d {
    fn default() -> Self {
        Self {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        }
    }
}

impl Extent3d {
    /// Calculates the [physical size] backing a texture of the given
    /// format and extent.  This includes padding to the block width
    /// and height of the format.
    ///
    /// This is the texture extent that you must upload at when uploading to _mipmaps_ of compressed textures.
    ///
    /// [physical size]: https://gpuweb.github.io/gpuweb/#physical-miplevel-specific-texture-extent
    #[must_use]
    pub fn physical_size(&self, format: TextureFormat) -> Self {
        let (block_width, block_height) = format.block_dimensions();

        let width = ((self.width + block_width - 1) / block_width) * block_width;
        let height = ((self.height + block_height - 1) / block_height) * block_height;

        Self {
            width,
            height,
            depth_or_array_layers: self.depth_or_array_layers,
        }
    }

    /// Calculates the maximum possible count of mipmaps.
    ///
    /// Treats the depth as part of the mipmaps. If calculating
    /// for a 2DArray texture, which does not mipmap depth, set depth to 1.
    #[must_use]
    pub fn max_mips(&self, dim: TextureDimension) -> u32 {
        match dim {
            TextureDimension::D1 => 1,
            TextureDimension::D2 => {
                let max_dim = self.width.max(self.height);
                32 - max_dim.leading_zeros()
            }
            TextureDimension::D3 => {
                let max_dim = self.width.max(self.height.max(self.depth_or_array_layers));
                32 - max_dim.leading_zeros()
            }
        }
    }

    /// Calculates the extent at a given mip level.
    /// Does *not* account for memory size being a multiple of block size.
    ///
    /// <https://gpuweb.github.io/gpuweb/#logical-miplevel-specific-texture-extent>
    #[must_use]
    pub fn mip_level_size(&self, level: u32, dim: TextureDimension) -> Self {
        Self {
            width: u32::max(1, self.width >> level),
            height: match dim {
                TextureDimension::D1 => 1,
                _ => u32::max(1, self.height >> level),
            },
            depth_or_array_layers: match dim {
                TextureDimension::D1 => 1,
                TextureDimension::D2 => self.depth_or_array_layers,
                TextureDimension::D3 => u32::max(1, self.depth_or_array_layers >> level),
            },
        }
    }
}

#[test]
fn test_physical_size() {
    let format = TextureFormat::Bc1RgbaUnormSrgb; // 4x4 blocks
    assert_eq!(
        Extent3d {
            width: 7,
            height: 7,
            depth_or_array_layers: 1
        }
        .physical_size(format),
        Extent3d {
            width: 8,
            height: 8,
            depth_or_array_layers: 1
        }
    );
    // Doesn't change, already aligned
    assert_eq!(
        Extent3d {
            width: 8,
            height: 8,
            depth_or_array_layers: 1
        }
        .physical_size(format),
        Extent3d {
            width: 8,
            height: 8,
            depth_or_array_layers: 1
        }
    );
    let format = TextureFormat::Astc {
        block: AstcBlock::B8x5,
        channel: AstcChannel::Unorm,
    }; // 8x5 blocks
    assert_eq!(
        Extent3d {
            width: 7,
            height: 7,
            depth_or_array_layers: 1
        }
        .physical_size(format),
        Extent3d {
            width: 8,
            height: 10,
            depth_or_array_layers: 1
        }
    );
}

#[test]
fn test_max_mips() {
    // 1D
    assert_eq!(
        Extent3d {
            width: 240,
            height: 1,
            depth_or_array_layers: 1
        }
        .max_mips(TextureDimension::D1),
        1
    );
    // 2D
    assert_eq!(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1
        }
        .max_mips(TextureDimension::D2),
        1
    );
    assert_eq!(
        Extent3d {
            width: 60,
            height: 60,
            depth_or_array_layers: 1
        }
        .max_mips(TextureDimension::D2),
        6
    );
    assert_eq!(
        Extent3d {
            width: 240,
            height: 1,
            depth_or_array_layers: 1000
        }
        .max_mips(TextureDimension::D2),
        8
    );
    // 3D
    assert_eq!(
        Extent3d {
            width: 16,
            height: 30,
            depth_or_array_layers: 60
        }
        .max_mips(TextureDimension::D3),
        6
    );
}

/// Describes a [`Texture`](../wgpu/struct.Texture.html).
///
/// Corresponds to [WebGPU `GPUTextureDescriptor`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gputexturedescriptor).
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TextureDescriptor<L, V> {
    /// Debug label of the texture. This will show up in graphics debuggers for easy identification.
    pub label: L,
    /// Size of the texture. All components must be greater than zero. For a
    /// regular 1D/2D texture, the unused sizes will be 1. For 2DArray textures,
    /// Z is the number of 2D textures in that array.
    pub size: Extent3d,
    /// Mip count of texture. For a texture with no extra mips, this must be 1.
    pub mip_level_count: u32,
    /// Sample count of texture. If this is not 1, texture must have [`BindingType::Texture::multisampled`] set to true.
    pub sample_count: u32,
    /// Dimensions of the texture.
    pub dimension: TextureDimension,
    /// Format of the texture.
    pub format: TextureFormat,
    /// Allowed usages of the texture. If used in other ways, the operation will panic.
    pub usage: TextureUsages,
    /// Specifies what view formats will be allowed when calling create_view() on this texture.
    ///
    /// View formats of the same format as the texture are always allowed.
    ///
    /// Note: currently, only the srgb-ness is allowed to change. (ex: Rgba8Unorm texture + Rgba8UnormSrgb view)
    pub view_formats: V,
}

impl<L, V> TextureDescriptor<L, V> {
    /// Takes a closure and maps the label of the texture descriptor into another.
    #[must_use]
    pub fn map_label<K>(&self, fun: impl FnOnce(&L) -> K) -> TextureDescriptor<K, V>
    where
        V: Clone,
    {
        TextureDescriptor {
            label: fun(&self.label),
            size: self.size,
            mip_level_count: self.mip_level_count,
            sample_count: self.sample_count,
            dimension: self.dimension,
            format: self.format,
            usage: self.usage,
            view_formats: self.view_formats.clone(),
        }
    }

    /// Maps the label and view_formats of the texture descriptor into another.
    #[must_use]
    pub fn map_label_and_view_formats<K, M>(
        &self,
        l_fun: impl FnOnce(&L) -> K,
        v_fun: impl FnOnce(V) -> M,
    ) -> TextureDescriptor<K, M>
    where
        V: Clone,
    {
        TextureDescriptor {
            label: l_fun(&self.label),
            size: self.size,
            mip_level_count: self.mip_level_count,
            sample_count: self.sample_count,
            dimension: self.dimension,
            format: self.format,
            usage: self.usage,
            view_formats: v_fun(self.view_formats.clone()),
        }
    }

    /// Calculates the extent at a given mip level.
    ///
    /// If the given mip level is larger than possible, returns None.
    ///
    /// Treats the depth as part of the mipmaps. If calculating
    /// for a 2DArray texture, which does not mipmap depth, set depth to 1.
    ///
    /// ```rust
    /// # use wgpu_types as wgpu;
    /// # type TextureDescriptor<'a> = wgpu::TextureDescriptor<(), &'a [wgpu::TextureFormat]>;
    /// let desc  = TextureDescriptor {
    ///   label: (),
    ///   size: wgpu::Extent3d { width: 100, height: 60, depth_or_array_layers: 1 },
    ///   mip_level_count: 7,
    ///   sample_count: 1,
    ///   dimension: wgpu::TextureDimension::D3,
    ///   format: wgpu::TextureFormat::Rgba8Sint,
    ///   usage: wgpu::TextureUsages::empty(),
    ///   view_formats: &[],
    /// };
    ///
    /// assert_eq!(desc.mip_level_size(0), Some(wgpu::Extent3d { width: 100, height: 60, depth_or_array_layers: 1 }));
    /// assert_eq!(desc.mip_level_size(1), Some(wgpu::Extent3d { width: 50, height: 30, depth_or_array_layers: 1 }));
    /// assert_eq!(desc.mip_level_size(2), Some(wgpu::Extent3d { width: 25, height: 15, depth_or_array_layers: 1 }));
    /// assert_eq!(desc.mip_level_size(3), Some(wgpu::Extent3d { width: 12, height: 7, depth_or_array_layers: 1 }));
    /// assert_eq!(desc.mip_level_size(4), Some(wgpu::Extent3d { width: 6, height: 3, depth_or_array_layers: 1 }));
    /// assert_eq!(desc.mip_level_size(5), Some(wgpu::Extent3d { width: 3, height: 1, depth_or_array_layers: 1 }));
    /// assert_eq!(desc.mip_level_size(6), Some(wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }));
    /// assert_eq!(desc.mip_level_size(7), None);
    /// ```
    #[must_use]
    pub fn mip_level_size(&self, level: u32) -> Option<Extent3d> {
        if level >= self.mip_level_count {
            return None;
        }

        Some(self.size.mip_level_size(level, self.dimension))
    }

    /// Computes the render extent of this texture.
    ///
    /// <https://gpuweb.github.io/gpuweb/#abstract-opdef-compute-render-extent>
    #[must_use]
    pub fn compute_render_extent(&self, mip_level: u32) -> Extent3d {
        Extent3d {
            width: u32::max(1, self.size.width >> mip_level),
            height: u32::max(1, self.size.height >> mip_level),
            depth_or_array_layers: 1,
        }
    }

    /// Returns the number of array layers.
    ///
    /// <https://gpuweb.github.io/gpuweb/#abstract-opdef-array-layer-count>
    #[must_use]
    pub fn array_layer_count(&self) -> u32 {
        match self.dimension {
            TextureDimension::D1 | TextureDimension::D3 => 1,
            TextureDimension::D2 => self.size.depth_or_array_layers,
        }
    }
}

/// Kind of data the texture holds.
///
/// Corresponds to [WebGPU `GPUTextureAspect`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gputextureaspect).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum TextureAspect {
    /// Depth, Stencil, and Color.
    #[default]
    All,
    /// Stencil.
    StencilOnly,
    /// Depth.
    DepthOnly,
    /// Plane 0.
    Plane0,
    /// Plane 1.
    Plane1,
    /// Plane 2.
    Plane2,
}

/// How edges should be handled in texture addressing.
///
/// Corresponds to [WebGPU `GPUAddressMode`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpuaddressmode).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum AddressMode {
    /// Clamp the value to the edge of the texture
    ///
    /// -0.25 -> 0.0
    /// 1.25  -> 1.0
    #[default]
    ClampToEdge = 0,
    /// Repeat the texture in a tiling fashion
    ///
    /// -0.25 -> 0.75
    /// 1.25 -> 0.25
    Repeat = 1,
    /// Repeat the texture, mirroring it every repeat
    ///
    /// -0.25 -> 0.25
    /// 1.25 -> 0.75
    MirrorRepeat = 2,
    /// Clamp the value to the border of the texture
    /// Requires feature [`Features::ADDRESS_MODE_CLAMP_TO_BORDER`]
    ///
    /// -0.25 -> border
    /// 1.25 -> border
    ClampToBorder = 3,
}

/// Texel mixing mode when sampling between texels.
///
/// Corresponds to [WebGPU `GPUFilterMode`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpufiltermode).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum FilterMode {
    /// Nearest neighbor sampling.
    ///
    /// This creates a pixelated effect when used as a mag filter
    #[default]
    Nearest = 0,
    /// Linear Interpolation
    ///
    /// This makes textures smooth but blurry when used as a mag filter.
    Linear = 1,
}

/// A range of push constant memory to pass to a shader stage.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PushConstantRange {
    /// Stage push constant range is visible from. Each stage can only be served by at most one range.
    /// One range can serve multiple stages however.
    pub stages: ShaderStages,
    /// Range in push constant memory to use for the stage. Must be less than [`Limits::max_push_constant_size`].
    /// Start and end must be aligned to the 4s.
    pub range: Range<u32>,
}

/// Describes a [`CommandBuffer`](../wgpu/struct.CommandBuffer.html).
///
/// Corresponds to [WebGPU `GPUCommandBufferDescriptor`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpucommandbufferdescriptor).
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CommandBufferDescriptor<L> {
    /// Debug label of this command buffer.
    pub label: L,
}

impl<L> CommandBufferDescriptor<L> {
    /// Takes a closure and maps the label of the command buffer descriptor into another.
    #[must_use]
    pub fn map_label<K>(&self, fun: impl FnOnce(&L) -> K) -> CommandBufferDescriptor<K> {
        CommandBufferDescriptor {
            label: fun(&self.label),
        }
    }
}

/// Describes the depth/stencil attachment for render bundles.
///
/// Corresponds to a portion of [WebGPU `GPURenderBundleEncoderDescriptor`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpurenderbundleencoderdescriptor).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RenderBundleDepthStencil {
    /// Format of the attachment.
    pub format: TextureFormat,
    /// If the depth aspect of the depth stencil attachment is going to be written to.
    ///
    /// This must match the [`RenderPassDepthStencilAttachment::depth_ops`] of the renderpass this render bundle is executed in.
    /// If depth_ops is `Some(..)` this must be false. If it is `None` this must be true.
    ///
    /// [`RenderPassDepthStencilAttachment::depth_ops`]: ../wgpu/struct.RenderPassDepthStencilAttachment.html#structfield.depth_ops
    pub depth_read_only: bool,

    /// If the stencil aspect of the depth stencil attachment is going to be written to.
    ///
    /// This must match the [`RenderPassDepthStencilAttachment::stencil_ops`] of the renderpass this render bundle is executed in.
    /// If depth_ops is `Some(..)` this must be false. If it is `None` this must be true.
    ///
    /// [`RenderPassDepthStencilAttachment::stencil_ops`]: ../wgpu/struct.RenderPassDepthStencilAttachment.html#structfield.stencil_ops
    pub stencil_read_only: bool,
}

/// Describes a [`RenderBundle`](../wgpu/struct.RenderBundle.html).
///
/// Corresponds to [WebGPU `GPURenderBundleDescriptor`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpurenderbundledescriptor).
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct RenderBundleDescriptor<L> {
    /// Debug label of the render bundle encoder. This will show up in graphics debuggers for easy identification.
    pub label: L,
}

impl<L> RenderBundleDescriptor<L> {
    /// Takes a closure and maps the label of the render bundle descriptor into another.
    #[must_use]
    pub fn map_label<K>(&self, fun: impl FnOnce(&L) -> K) -> RenderBundleDescriptor<K> {
        RenderBundleDescriptor {
            label: fun(&self.label),
        }
    }
}

impl<T> Default for RenderBundleDescriptor<Option<T>> {
    fn default() -> Self {
        Self { label: None }
    }
}

/// Layout of a texture in a buffer's memory.
///
/// The bytes per row and rows per image can be hard to figure out so here are some examples:
///
/// | Resolution | Format | Bytes per block | Pixels per block | Bytes per row                          | Rows per image               |
/// |------------|--------|-----------------|------------------|----------------------------------------|------------------------------|
/// | 256x256    | RGBA8  | 4               | 1 * 1 * 1        | 256 * 4 = Some(1024)                   | None                         |
/// | 32x16x8    | RGBA8  | 4               | 1 * 1 * 1        | 32 * 4 = 128 padded to 256 = Some(256) | None                         |
/// | 256x256    | BC3    | 16              | 4 * 4 * 1        | 16 * (256 / 4) = 1024 = Some(1024)     | None                         |
/// | 64x64x8    | BC3    | 16              | 4 * 4 * 1        | 16 * (64 / 4) = 256 = Some(256)        | 64 / 4 = 16 = Some(16)       |
///
/// Corresponds to [WebGPU `GPUImageDataLayout`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpuimagedatalayout).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageDataLayout {
    /// Offset into the buffer that is the start of the texture. Must be a multiple of texture block size.
    /// For non-compressed textures, this is 1.
    pub offset: BufferAddress,
    /// Bytes per "row" in an image.
    ///
    /// A row is one row of pixels or of compressed blocks in the x direction.
    ///
    /// This value is required if there are multiple rows (i.e. height or depth is more than one pixel or pixel block for compressed textures)
    ///
    /// Must be a multiple of 256 for [`CommandEncoder::copy_buffer_to_texture`][CEcbtt]
    /// and [`CommandEncoder::copy_texture_to_buffer`][CEcttb]. You must manually pad the
    /// image such that this is a multiple of 256. It will not affect the image data.
    ///
    /// [`Queue::write_texture`][Qwt] does not have this requirement.
    ///
    /// Must be a multiple of the texture block size. For non-compressed textures, this is 1.
    ///
    /// [CEcbtt]: ../wgpu/struct.CommandEncoder.html#method.copy_buffer_to_texture
    /// [CEcttb]: ../wgpu/struct.CommandEncoder.html#method.copy_texture_to_buffer
    /// [Qwt]: ../wgpu/struct.Queue.html#method.write_texture
    pub bytes_per_row: Option<u32>,
    /// "Rows" that make up a single "image".
    ///
    /// A row is one row of pixels or of compressed blocks in the x direction.
    ///
    /// An image is one layer in the z direction of a 3D image or 2DArray texture.
    ///
    /// The amount of rows per image may be larger than the actual amount of rows of data.
    ///
    /// Required if there are multiple images (i.e. the depth is more than one).
    pub rows_per_image: Option<u32>,
}

/// Specific type of a buffer binding.
///
/// Corresponds to [WebGPU `GPUBufferBindingType`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpubufferbindingtype).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum BufferBindingType {
    /// A buffer for uniform values.
    ///
    /// Example WGSL syntax:
    /// ```rust,ignore
    /// struct Globals {
    ///     a_uniform: vec2<f32>,
    ///     another_uniform: vec2<f32>,
    /// }
    /// @group(0) @binding(0)
    /// var<uniform> globals: Globals;
    /// ```
    ///
    /// Example GLSL syntax:
    /// ```cpp,ignore
    /// layout(std140, binding = 0)
    /// uniform Globals {
    ///     vec2 aUniform;
    ///     vec2 anotherUniform;
    /// };
    /// ```
    #[default]
    Uniform,
    /// A storage buffer.
    ///
    /// Example WGSL syntax:
    /// ```rust,ignore
    /// @group(0) @binding(0)
    /// var<storage, read_write> my_element: array<vec4<f32>>;
    /// ```
    ///
    /// Example GLSL syntax:
    /// ```cpp,ignore
    /// layout (set=0, binding=0) buffer myStorageBuffer {
    ///     vec4 myElement[];
    /// };
    /// ```
    Storage {
        /// If `true`, the buffer can only be read in the shader,
        /// and it:
        /// - may or may not be annotated with `read` (WGSL).
        /// - must be annotated with `readonly` (GLSL).
        ///
        /// Example WGSL syntax:
        /// ```rust,ignore
        /// @group(0) @binding(0)
        /// var<storage, read> my_element: array<vec4<f32>>;
        /// ```
        ///
        /// Example GLSL syntax:
        /// ```cpp,ignore
        /// layout (set=0, binding=0) readonly buffer myStorageBuffer {
        ///     vec4 myElement[];
        /// };
        /// ```
        read_only: bool,
    },
}

/// Specific type of a sample in a texture binding.
///
/// Corresponds to [WebGPU `GPUTextureSampleType`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gputexturesampletype).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TextureSampleType {
    /// Sampling returns floats.
    ///
    /// Example WGSL syntax:
    /// ```rust,ignore
    /// @group(0) @binding(0)
    /// var t: texture_2d<f32>;
    /// ```
    ///
    /// Example GLSL syntax:
    /// ```cpp,ignore
    /// layout(binding = 0)
    /// uniform texture2D t;
    /// ```
    Float {
        /// If this is `false`, the texture can't be sampled with
        /// a filtering sampler.
        ///
        /// Even if this is `true`, it's possible to sample with
        /// a **non-filtering** sampler.
        filterable: bool,
    },
    /// Sampling does the depth reference comparison.
    ///
    /// This is also compatible with a non-filtering sampler.
    ///
    /// Example WGSL syntax:
    /// ```rust,ignore
    /// @group(0) @binding(0)
    /// var t: texture_depth_2d;
    /// ```
    ///
    /// Example GLSL syntax:
    /// ```cpp,ignore
    /// layout(binding = 0)
    /// uniform texture2DShadow t;
    /// ```
    Depth,
    /// Sampling returns signed integers.
    ///
    /// Example WGSL syntax:
    /// ```rust,ignore
    /// @group(0) @binding(0)
    /// var t: texture_2d<i32>;
    /// ```
    ///
    /// Example GLSL syntax:
    /// ```cpp,ignore
    /// layout(binding = 0)
    /// uniform itexture2D t;
    /// ```
    Sint,
    /// Sampling returns unsigned integers.
    ///
    /// Example WGSL syntax:
    /// ```rust,ignore
    /// @group(0) @binding(0)
    /// var t: texture_2d<u32>;
    /// ```
    ///
    /// Example GLSL syntax:
    /// ```cpp,ignore
    /// layout(binding = 0)
    /// uniform utexture2D t;
    /// ```
    Uint,
}

impl Default for TextureSampleType {
    fn default() -> Self {
        Self::Float { filterable: true }
    }
}

/// Specific type of a sample in a texture binding.
///
/// For use in [`BindingType::StorageTexture`].
///
/// Corresponds to [WebGPU `GPUStorageTextureAccess`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpustoragetextureaccess).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum StorageTextureAccess {
    /// The texture can only be written in the shader and it:
    /// - may or may not be annotated with `write` (WGSL).
    /// - must be annotated with `writeonly` (GLSL).
    ///
    /// Example WGSL syntax:
    /// ```rust,ignore
    /// @group(0) @binding(0)
    /// var my_storage_image: texture_storage_2d<r32float, write>;
    /// ```
    ///
    /// Example GLSL syntax:
    /// ```cpp,ignore
    /// layout(set=0, binding=0, r32f) writeonly uniform image2D myStorageImage;
    /// ```
    WriteOnly,
    /// The texture can only be read in the shader and it must be annotated with `read` (WGSL) or
    /// `readonly` (GLSL).
    ///
    /// [`Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`] must be enabled to use this access
    /// mode. This is a native-only extension.
    ///
    /// Example WGSL syntax:
    /// ```rust,ignore
    /// @group(0) @binding(0)
    /// var my_storage_image: texture_storage_2d<r32float, read>;
    /// ```
    ///
    /// Example GLSL syntax:
    /// ```cpp,ignore
    /// layout(set=0, binding=0, r32f) readonly uniform image2D myStorageImage;
    /// ```
    ReadOnly,
    /// The texture can be both read and written in the shader and must be annotated with
    /// `read_write` in WGSL.
    ///
    /// [`Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`] must be enabled to use this access
    /// mode.  This is a nonstandard, native-only extension.
    ///
    /// Example WGSL syntax:
    /// ```rust,ignore
    /// @group(0) @binding(0)
    /// var my_storage_image: texture_storage_2d<r32float, read_write>;
    /// ```
    ///
    /// Example GLSL syntax:
    /// ```cpp,ignore
    /// layout(set=0, binding=0, r32f) uniform image2D myStorageImage;
    /// ```
    ReadWrite,
}

/// Specific type of a sampler binding.
///
/// For use in [`BindingType::Sampler`].
///
/// Corresponds to [WebGPU `GPUSamplerBindingType`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpusamplerbindingtype).
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum SamplerBindingType {
    /// The sampling result is produced based on more than a single color sample from a texture,
    /// e.g. when bilinear interpolation is enabled.
    Filtering,
    /// The sampling result is produced based on a single color sample from a texture.
    NonFiltering,
    /// Use as a comparison sampler instead of a normal sampler.
    /// For more info take a look at the analogous functionality in OpenGL: <https://www.khronos.org/opengl/wiki/Sampler_Object#Comparison_mode>.
    Comparison,
}

/// Specific type of a binding.
///
/// For use in [`BindGroupLayoutEntry`].
///
/// Corresponds to WebGPU's mutually exclusive fields within [`GPUBindGroupLayoutEntry`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpubindgrouplayoutentry).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum BindingType {
    /// A buffer binding.
    ///
    /// Corresponds to [WebGPU `GPUBufferBindingLayout`](
    /// https://gpuweb.github.io/gpuweb/#dictdef-gpubufferbindinglayout).
    Buffer {
        /// Sub-type of the buffer binding.
        ty: BufferBindingType,

        /// Indicates that the binding has a dynamic offset.
        ///
        /// One offset must be passed to [`RenderPass::set_bind_group`][RPsbg]
        /// for each dynamic binding in increasing order of binding number.
        ///
        /// [RPsbg]: ../wgpu/struct.RenderPass.html#method.set_bind_group
        #[cfg_attr(feature = "serde", serde(default))]
        has_dynamic_offset: bool,

        /// The minimum size for a [`BufferBinding`] matching this entry, in bytes.
        ///
        /// If this is `Some(size)`:
        ///
        /// - When calling [`create_bind_group`], the resource at this bind point
        ///   must be a [`BindingResource::Buffer`] whose effective size is at
        ///   least `size`.
        ///
        /// - When calling [`create_render_pipeline`] or [`create_compute_pipeline`],
        ///   `size` must be at least the [minimum buffer binding size] for the
        ///   shader module global at this bind point: large enough to hold the
        ///   global's value, along with one element of a trailing runtime-sized
        ///   array, if present.
        ///
        /// If this is `None`:
        ///
        /// - Each draw or dispatch command checks that the buffer range at this
        ///   bind point satisfies the [minimum buffer binding size].
        ///
        /// [`BufferBinding`]: ../wgpu/struct.BufferBinding.html
        /// [`create_bind_group`]: ../wgpu/struct.Device.html#method.create_bind_group
        /// [`BindingResource::Buffer`]: ../wgpu/enum.BindingResource.html#variant.Buffer
        /// [minimum buffer binding size]: https://www.w3.org/TR/webgpu/#minimum-buffer-binding-size
        /// [`create_render_pipeline`]: ../wgpu/struct.Device.html#method.create_render_pipeline
        /// [`create_compute_pipeline`]: ../wgpu/struct.Device.html#method.create_compute_pipeline
        #[cfg_attr(feature = "serde", serde(default))]
        min_binding_size: Option<BufferSize>,
    },
    /// A sampler that can be used to sample a texture.
    ///
    /// Example WGSL syntax:
    /// ```rust,ignore
    /// @group(0) @binding(0)
    /// var s: sampler;
    /// ```
    ///
    /// Example GLSL syntax:
    /// ```cpp,ignore
    /// layout(binding = 0)
    /// uniform sampler s;
    /// ```
    ///
    /// Corresponds to [WebGPU `GPUSamplerBindingLayout`](
    /// https://gpuweb.github.io/gpuweb/#dictdef-gpusamplerbindinglayout).
    Sampler(SamplerBindingType),
    /// A texture binding.
    ///
    /// Example WGSL syntax:
    /// ```rust,ignore
    /// @group(0) @binding(0)
    /// var t: texture_2d<f32>;
    /// ```
    ///
    /// Example GLSL syntax:
    /// ```cpp,ignore
    /// layout(binding = 0)
    /// uniform texture2D t;
    /// ```
    ///
    /// Corresponds to [WebGPU `GPUTextureBindingLayout`](
    /// https://gpuweb.github.io/gpuweb/#dictdef-gputexturebindinglayout).
    Texture {
        /// Sample type of the texture binding.
        sample_type: TextureSampleType,
        /// Dimension of the texture view that is going to be sampled.
        view_dimension: TextureViewDimension,
        /// True if the texture has a sample count greater than 1. If this is true,
        /// the texture must be declared as `texture_multisampled_2d` or
        /// `texture_depth_multisampled_2d` in the shader, and read using `textureLoad`.
        multisampled: bool,
    },
    /// A storage texture.
    ///
    /// Example WGSL syntax:
    /// ```rust,ignore
    /// @group(0) @binding(0)
    /// var my_storage_image: texture_storage_2d<r32float, write>;
    /// ```
    ///
    /// Example GLSL syntax:
    /// ```cpp,ignore
    /// layout(set=0, binding=0, r32f) writeonly uniform image2D myStorageImage;
    /// ```
    /// Note that the texture format must be specified in the shader, along with the
    /// access mode. For WGSL, the format must be one of the enumerants in the list
    /// of [storage texel formats](https://gpuweb.github.io/gpuweb/wgsl/#storage-texel-formats).
    ///
    /// Corresponds to [WebGPU `GPUStorageTextureBindingLayout`](
    /// https://gpuweb.github.io/gpuweb/#dictdef-gpustoragetexturebindinglayout).
    StorageTexture {
        /// Allowed access to this texture.
        access: StorageTextureAccess,
        /// Format of the texture.
        format: TextureFormat,
        /// Dimension of the texture view that is going to be sampled.
        view_dimension: TextureViewDimension,
    },

    /// A ray-tracing acceleration structure binding.
    ///
    /// Example WGSL syntax:
    /// ```rust,ignore
    /// @group(0) @binding(0)
    /// var as: acceleration_structure;
    /// ```
    ///
    /// Example GLSL syntax:
    /// ```cpp,ignore
    /// layout(binding = 0)
    /// uniform accelerationStructureEXT as;
    /// ```
    AccelerationStructure,
}

impl BindingType {
    /// Returns true for buffer bindings with dynamic offset enabled.
    #[must_use]
    pub fn has_dynamic_offset(&self) -> bool {
        match *self {
            Self::Buffer {
                has_dynamic_offset, ..
            } => has_dynamic_offset,
            _ => false,
        }
    }
}

/// Describes a single binding inside a bind group.
///
/// Corresponds to [WebGPU `GPUBindGroupLayoutEntry`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpubindgrouplayoutentry).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BindGroupLayoutEntry {
    /// Binding index. Must match shader index and be unique inside a BindGroupLayout. A binding
    /// of index 1, would be described as `layout(set = 0, binding = 1) uniform` in shaders.
    pub binding: u32,
    /// Which shader stages can see this binding.
    pub visibility: ShaderStages,
    /// The type of the binding
    pub ty: BindingType,
    /// If this value is Some, indicates this entry is an array. Array size must be 1 or greater.
    ///
    /// If this value is Some and `ty` is `BindingType::Texture`, [`Features::TEXTURE_BINDING_ARRAY`] must be supported.
    ///
    /// If this value is Some and `ty` is any other variant, bind group creation will fail.
    #[cfg_attr(feature = "serde", serde(default))]
    pub count: Option<NonZeroU32>,
}

/// View of a buffer which can be used to copy to/from a texture.
///
/// Corresponds to [WebGPU `GPUImageCopyBuffer`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpuimagecopybuffer).
#[repr(C)]
#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageCopyBuffer<B> {
    /// The buffer to be copied to/from.
    pub buffer: B,
    /// The layout of the texture data in this buffer.
    pub layout: ImageDataLayout,
}

/// View of a texture which can be used to copy to/from a buffer/texture.
///
/// Corresponds to [WebGPU `GPUImageCopyTexture`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpuimagecopytexture).
#[repr(C)]
#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageCopyTexture<T> {
    /// The texture to be copied to/from.
    pub texture: T,
    /// The target mip level of the texture.
    pub mip_level: u32,
    /// The base texel of the texture in the selected `mip_level`. Together
    /// with the `copy_size` argument to copy functions, defines the
    /// sub-region of the texture to copy.
    #[cfg_attr(feature = "serde", serde(default))]
    pub origin: Origin3d,
    /// The copy aspect.
    #[cfg_attr(feature = "serde", serde(default))]
    pub aspect: TextureAspect,
}

impl<T> ImageCopyTexture<T> {
    /// Adds color space and premultiplied alpha information to make this
    /// descriptor tagged.
    pub fn to_tagged(
        self,
        color_space: PredefinedColorSpace,
        premultiplied_alpha: bool,
    ) -> ImageCopyTextureTagged<T> {
        ImageCopyTextureTagged {
            texture: self.texture,
            mip_level: self.mip_level,
            origin: self.origin,
            aspect: self.aspect,
            color_space,
            premultiplied_alpha,
        }
    }
}

/// View of an external texture that can be used to copy to a texture.
///
/// Corresponds to [WebGPU `GPUImageCopyExternalImage`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpuimagecopyexternalimage).
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug)]
pub struct ImageCopyExternalImage {
    /// The texture to be copied from. The copy source data is captured at the moment
    /// the copy is issued.
    pub source: ExternalImageSource,
    /// The base texel used for copying from the external image. Together
    /// with the `copy_size` argument to copy functions, defines the
    /// sub-region of the image to copy.
    ///
    /// Relative to the top left of the image.
    ///
    /// Must be [`Origin2d::ZERO`] if [`DownlevelFlags::UNRESTRICTED_EXTERNAL_TEXTURE_COPIES`] is not supported.
    pub origin: Origin2d,
    /// If the Y coordinate of the image should be flipped. Even if this is
    /// true, `origin` is still relative to the top left.
    pub flip_y: bool,
}

/// Source of an external texture copy.
///
/// Corresponds to the [implicit union type on WebGPU `GPUImageCopyExternalImage.source`](
/// https://gpuweb.github.io/gpuweb/#dom-gpuimagecopyexternalimage-source).
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug)]
pub enum ExternalImageSource {
    /// Copy from a previously-decoded image bitmap.
    ImageBitmap(web_sys::ImageBitmap),
    /// Copy from an image element.
    HTMLImageElement(web_sys::HtmlImageElement),
    /// Copy from a current frame of a video element.
    HTMLVideoElement(web_sys::HtmlVideoElement),
    /// Copy from an image.
    ImageData(web_sys::ImageData),
    /// Copy from a on-screen canvas.
    HTMLCanvasElement(web_sys::HtmlCanvasElement),
    /// Copy from a off-screen canvas.
    ///
    /// Requires [`DownlevelFlags::UNRESTRICTED_EXTERNAL_TEXTURE_COPIES`]
    OffscreenCanvas(web_sys::OffscreenCanvas),
    /// Copy from a video frame.
    #[cfg(web_sys_unstable_apis)]
    VideoFrame(web_sys::VideoFrame),
}

#[cfg(target_arch = "wasm32")]
impl ExternalImageSource {
    /// Gets the pixel, not css, width of the source.
    pub fn width(&self) -> u32 {
        match self {
            ExternalImageSource::ImageBitmap(b) => b.width(),
            ExternalImageSource::HTMLImageElement(i) => i.width(),
            ExternalImageSource::HTMLVideoElement(v) => v.video_width(),
            ExternalImageSource::ImageData(i) => i.width(),
            ExternalImageSource::HTMLCanvasElement(c) => c.width(),
            ExternalImageSource::OffscreenCanvas(c) => c.width(),
            #[cfg(web_sys_unstable_apis)]
            ExternalImageSource::VideoFrame(v) => v.display_width(),
        }
    }

    /// Gets the pixel, not css, height of the source.
    pub fn height(&self) -> u32 {
        match self {
            ExternalImageSource::ImageBitmap(b) => b.height(),
            ExternalImageSource::HTMLImageElement(i) => i.height(),
            ExternalImageSource::HTMLVideoElement(v) => v.video_height(),
            ExternalImageSource::ImageData(i) => i.height(),
            ExternalImageSource::HTMLCanvasElement(c) => c.height(),
            ExternalImageSource::OffscreenCanvas(c) => c.height(),
            #[cfg(web_sys_unstable_apis)]
            ExternalImageSource::VideoFrame(v) => v.display_height(),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl std::ops::Deref for ExternalImageSource {
    type Target = js_sys::Object;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::ImageBitmap(b) => b,
            Self::HTMLImageElement(i) => i,
            Self::HTMLVideoElement(v) => v,
            Self::ImageData(i) => i,
            Self::HTMLCanvasElement(c) => c,
            Self::OffscreenCanvas(c) => c,
            #[cfg(web_sys_unstable_apis)]
            Self::VideoFrame(v) => v,
        }
    }
}

#[cfg(all(
    target_arch = "wasm32",
    feature = "fragile-send-sync-non-atomic-wasm",
    not(target_feature = "atomics")
))]
unsafe impl Send for ExternalImageSource {}
#[cfg(all(
    target_arch = "wasm32",
    feature = "fragile-send-sync-non-atomic-wasm",
    not(target_feature = "atomics")
))]
unsafe impl Sync for ExternalImageSource {}

/// Color spaces supported on the web.
///
/// Corresponds to [HTML Canvas `PredefinedColorSpace`](
/// https://html.spec.whatwg.org/multipage/canvas.html#predefinedcolorspace).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum PredefinedColorSpace {
    /// sRGB color space
    Srgb,
    /// Display-P3 color space
    DisplayP3,
}

/// View of a texture which can be used to copy to a texture, including
/// color space and alpha premultiplication information.
///
/// Corresponds to [WebGPU `GPUImageCopyTextureTagged`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpuimagecopytexturetagged).
#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageCopyTextureTagged<T> {
    /// The texture to be copied to/from.
    pub texture: T,
    /// The target mip level of the texture.
    pub mip_level: u32,
    /// The base texel of the texture in the selected `mip_level`.
    pub origin: Origin3d,
    /// The copy aspect.
    pub aspect: TextureAspect,
    /// The color space of this texture.
    pub color_space: PredefinedColorSpace,
    /// The premultiplication of this texture
    pub premultiplied_alpha: bool,
}

impl<T> ImageCopyTextureTagged<T> {
    /// Removes the colorspace information from the type.
    pub fn to_untagged(self) -> ImageCopyTexture<T> {
        ImageCopyTexture {
            texture: self.texture,
            mip_level: self.mip_level,
            origin: self.origin,
            aspect: self.aspect,
        }
    }
}

/// Subresource range within an image
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ImageSubresourceRange {
    /// Aspect of the texture. Color textures must be [`TextureAspect::All`][TAA].
    ///
    /// [TAA]: ../wgpu/enum.TextureAspect.html#variant.All
    pub aspect: TextureAspect,
    /// Base mip level.
    pub base_mip_level: u32,
    /// Mip level count.
    /// If `Some(count)`, `base_mip_level + count` must be less or equal to underlying texture mip count.
    /// If `None`, considered to include the rest of the mipmap levels, but at least 1 in total.
    pub mip_level_count: Option<u32>,
    /// Base array layer.
    pub base_array_layer: u32,
    /// Layer count.
    /// If `Some(count)`, `base_array_layer + count` must be less or equal to the underlying array count.
    /// If `None`, considered to include the rest of the array layers, but at least 1 in total.
    pub array_layer_count: Option<u32>,
}

impl ImageSubresourceRange {
    /// Returns if the given range represents a full resource, with a texture of the given
    /// layer count and mip count.
    ///
    /// ```rust
    /// # use wgpu_types as wgpu;
    ///
    /// let range_none = wgpu::ImageSubresourceRange {
    ///     aspect: wgpu::TextureAspect::All,
    ///     base_mip_level: 0,
    ///     mip_level_count: None,
    ///     base_array_layer: 0,
    ///     array_layer_count: None,
    /// };
    /// assert_eq!(range_none.is_full_resource(wgpu::TextureFormat::Stencil8, 5, 10), true);
    ///
    /// let range_some = wgpu::ImageSubresourceRange {
    ///     aspect: wgpu::TextureAspect::All,
    ///     base_mip_level: 0,
    ///     mip_level_count: Some(5),
    ///     base_array_layer: 0,
    ///     array_layer_count: Some(10),
    /// };
    /// assert_eq!(range_some.is_full_resource(wgpu::TextureFormat::Stencil8, 5, 10), true);
    ///
    /// let range_mixed = wgpu::ImageSubresourceRange {
    ///     aspect: wgpu::TextureAspect::StencilOnly,
    ///     base_mip_level: 0,
    ///     // Only partial resource
    ///     mip_level_count: Some(3),
    ///     base_array_layer: 0,
    ///     array_layer_count: None,
    /// };
    /// assert_eq!(range_mixed.is_full_resource(wgpu::TextureFormat::Stencil8, 5, 10), false);
    /// ```
    #[must_use]
    pub fn is_full_resource(
        &self,
        format: TextureFormat,
        mip_levels: u32,
        array_layers: u32,
    ) -> bool {
        // Mip level count and array layer count need to deal with both the None and Some(count) case.
        let mip_level_count = self.mip_level_count.unwrap_or(mip_levels);
        let array_layer_count = self.array_layer_count.unwrap_or(array_layers);

        let aspect_eq = Some(format) == format.aspect_specific_format(self.aspect);

        let base_mip_level_eq = self.base_mip_level == 0;
        let mip_level_count_eq = mip_level_count == mip_levels;

        let base_array_layer_eq = self.base_array_layer == 0;
        let array_layer_count_eq = array_layer_count == array_layers;

        aspect_eq
            && base_mip_level_eq
            && mip_level_count_eq
            && base_array_layer_eq
            && array_layer_count_eq
    }

    /// Returns the mip level range of a subresource range describes for a specific texture.
    #[must_use]
    pub fn mip_range(&self, mip_level_count: u32) -> Range<u32> {
        self.base_mip_level..match self.mip_level_count {
            Some(mip_level_count) => self.base_mip_level + mip_level_count,
            None => mip_level_count,
        }
    }

    /// Returns the layer range of a subresource range describes for a specific texture.
    #[must_use]
    pub fn layer_range(&self, array_layer_count: u32) -> Range<u32> {
        self.base_array_layer..match self.array_layer_count {
            Some(array_layer_count) => self.base_array_layer + array_layer_count,
            None => array_layer_count,
        }
    }
}

/// Color variation to use when sampler addressing mode is [`AddressMode::ClampToBorder`]
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SamplerBorderColor {
    /// [0, 0, 0, 0]
    TransparentBlack,
    /// [0, 0, 0, 1]
    OpaqueBlack,
    /// [1, 1, 1, 1]
    OpaqueWhite,

    /// On the Metal backend, this is equivalent to `TransparentBlack` for
    /// textures that have an alpha component, and equivalent to `OpaqueBlack`
    /// for textures that do not have an alpha component. On other backends,
    /// this is equivalent to `TransparentBlack`. Requires
    /// [`Features::ADDRESS_MODE_CLAMP_TO_ZERO`]. Not supported on the web.
    Zero,
}

/// Describes how to create a QuerySet.
///
/// Corresponds to [WebGPU `GPUQuerySetDescriptor`](
/// https://gpuweb.github.io/gpuweb/#dictdef-gpuquerysetdescriptor).
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QuerySetDescriptor<L> {
    /// Debug label for the query set.
    pub label: L,
    /// Kind of query that this query set should contain.
    pub ty: QueryType,
    /// Total count of queries the set contains. Must not be zero.
    /// Must not be greater than [`QUERY_SET_MAX_QUERIES`].
    pub count: u32,
}

impl<L> QuerySetDescriptor<L> {
    /// Takes a closure and maps the label of the query set descriptor into another.
    #[must_use]
    pub fn map_label<'a, K>(&'a self, fun: impl FnOnce(&'a L) -> K) -> QuerySetDescriptor<K> {
        QuerySetDescriptor {
            label: fun(&self.label),
            ty: self.ty,
            count: self.count,
        }
    }
}

/// Type of query contained in a QuerySet.
///
/// Corresponds to [WebGPU `GPUQueryType`](
/// https://gpuweb.github.io/gpuweb/#enumdef-gpuquerytype).
#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum QueryType {
    /// Query returns a single 64-bit number, serving as an occlusion boolean.
    Occlusion,
    /// Query returns up to 5 64-bit numbers based on the given flags.
    ///
    /// See [`PipelineStatisticsTypes`]'s documentation for more information
    /// on how they get resolved.
    ///
    /// [`Features::PIPELINE_STATISTICS_QUERY`] must be enabled to use this query type.
    PipelineStatistics(PipelineStatisticsTypes),
    /// Query returns a 64-bit number indicating the GPU-timestamp
    /// where all previous commands have finished executing.
    ///
    /// Must be multiplied by [`Queue::get_timestamp_period`][Qgtp] to get
    /// the value in nanoseconds. Absolute values have no meaning,
    /// but timestamps can be subtracted to get the time it takes
    /// for a string of operations to complete.
    ///
    /// [`Features::TIMESTAMP_QUERY`] must be enabled to use this query type.
    ///
    /// [Qgtp]: ../wgpu/struct.Queue.html#method.get_timestamp_period
    Timestamp,
}

bitflags::bitflags! {
    /// Flags for which pipeline data should be recorded.
    ///
    /// The amount of values written when resolved depends
    /// on the amount of flags. If 3 flags are enabled, 3
    /// 64-bit values will be written per-query.
    ///
    /// The order they are written is the order they are declared
    /// in this bitflags. If you enabled `CLIPPER_PRIMITIVES_OUT`
    /// and `COMPUTE_SHADER_INVOCATIONS`, it would write 16 bytes,
    /// the first 8 bytes being the primitive out value, the last 8
    /// bytes being the compute shader invocation count.
    #[repr(transparent)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
    pub struct PipelineStatisticsTypes : u8 {
        /// Amount of times the vertex shader is ran. Accounts for
        /// the vertex cache when doing indexed rendering.
        const VERTEX_SHADER_INVOCATIONS = 1 << 0;
        /// Amount of times the clipper is invoked. This
        /// is also the amount of triangles output by the vertex shader.
        const CLIPPER_INVOCATIONS = 1 << 1;
        /// Amount of primitives that are not culled by the clipper.
        /// This is the amount of triangles that are actually on screen
        /// and will be rasterized and rendered.
        const CLIPPER_PRIMITIVES_OUT = 1 << 2;
        /// Amount of times the fragment shader is ran. Accounts for
        /// fragment shaders running in 2x2 blocks in order to get
        /// derivatives.
        const FRAGMENT_SHADER_INVOCATIONS = 1 << 3;
        /// Amount of times a compute shader is invoked. This will
        /// be equivalent to the dispatch count times the workgroup size.
        const COMPUTE_SHADER_INVOCATIONS = 1 << 4;
    }
}

impl_bitflags!(PipelineStatisticsTypes);

/// Argument buffer layout for draw_indirect commands.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct DrawIndirectArgs {
    /// The number of vertices to draw.
    pub vertex_count: u32,
    /// The number of instances to draw.
    pub instance_count: u32,
    /// The Index of the first vertex to draw.
    pub first_vertex: u32,
    /// The instance ID of the first instance to draw.
    ///
    /// Has to be 0, unless [`Features::INDIRECT_FIRST_INSTANCE`](crate::Features::INDIRECT_FIRST_INSTANCE) is enabled.
    pub first_instance: u32,
}

impl DrawIndirectArgs {
    /// Returns the bytes representation of the struct, ready to be written in a buffer.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::mem::transmute(std::slice::from_raw_parts(
                std::ptr::from_ref(self).cast::<u8>(),
                size_of::<Self>(),
            ))
        }
    }
}

/// Argument buffer layout for draw_indexed_indirect commands.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct DrawIndexedIndirectArgs {
    /// The number of indices to draw.
    pub index_count: u32,
    /// The number of instances to draw.
    pub instance_count: u32,
    /// The first index within the index buffer.
    pub first_index: u32,
    /// The value added to the vertex index before indexing into the vertex buffer.
    pub base_vertex: i32,
    /// The instance ID of the first instance to draw.
    ///
    /// Has to be 0, unless [`Features::INDIRECT_FIRST_INSTANCE`](crate::Features::INDIRECT_FIRST_INSTANCE) is enabled.
    pub first_instance: u32,
}

impl DrawIndexedIndirectArgs {
    /// Returns the bytes representation of the struct, ready to be written in a buffer.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::mem::transmute(std::slice::from_raw_parts(
                std::ptr::from_ref(self).cast::<u8>(),
                size_of::<Self>(),
            ))
        }
    }
}

/// Argument buffer layout for dispatch_indirect commands.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct DispatchIndirectArgs {
    /// The number of work groups in X dimension.
    pub x: u32,
    /// The number of work groups in Y dimension.
    pub y: u32,
    /// The number of work groups in Z dimension.
    pub z: u32,
}

impl DispatchIndirectArgs {
    /// Returns the bytes representation of the struct, ready to be written into a buffer.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::mem::transmute(std::slice::from_raw_parts(
                std::ptr::from_ref(self).cast::<u8>(),
                size_of::<Self>(),
            ))
        }
    }
}

/// Describes how shader bound checks should be performed.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShaderBoundChecks {
    runtime_checks: bool,
}

impl ShaderBoundChecks {
    /// Creates a new configuration where the shader is bound checked.
    #[must_use]
    pub fn new() -> Self {
        ShaderBoundChecks {
            runtime_checks: true,
        }
    }

    /// Creates a new configuration where the shader isn't bound checked.
    ///
    /// # Safety
    ///
    /// The caller MUST ensure that all shaders built with this configuration
    /// don't perform any out of bounds reads or writes.
    ///
    /// Note that `wgpu_core`, in particular, initializes only those portions of
    /// buffers that it expects might be read, and it does not expect contents
    /// outside the ranges bound in bindgroups to be accessible, so using this
    /// configuration with ill-behaved shaders could expose uninitialized GPU
    /// memory contents to the application.
    #[must_use]
    pub unsafe fn unchecked() -> Self {
        ShaderBoundChecks {
            runtime_checks: false,
        }
    }

    /// Query whether runtime bound checks are enabled in this configuration
    #[must_use]
    pub fn runtime_checks(&self) -> bool {
        self.runtime_checks
    }
}

impl Default for ShaderBoundChecks {
    fn default() -> Self {
        Self::new()
    }
}

/// Selects which DX12 shader compiler to use.
///
/// If the `Dxc` option is selected, but `dxcompiler.dll` and `dxil.dll` files aren't found,
/// then this will fall back to the Fxc compiler at runtime and log an error.
///
/// `wgpu::utils::init::dx12_shader_compiler_from_env` can be used to set the compiler
/// from the `WGPU_DX12_SHADER_COMPILER` environment variable, but this should only be used for testing.
#[derive(Clone, Debug, Default)]
pub enum Dx12Compiler {
    /// The Fxc compiler (default) is old, slow and unmaintained.
    ///
    /// However, it doesn't require any additional .dlls to be shipped with the application.
    #[default]
    Fxc,
    /// The Dxc compiler is new, fast and maintained.
    ///
    /// However, it requires both `dxcompiler.dll` and `dxil.dll` to be shipped with the application.
    /// These files can be downloaded from <https://github.com/microsoft/DirectXShaderCompiler/releases>.
    ///
    /// Minimum supported version: [v1.5.2010](https://github.com/microsoft/DirectXShaderCompiler/releases/tag/v1.5.2010)
    ///
    /// It also requires WDDM 2.1 (Windows 10 version 1607).
    Dxc {
        /// Path to the `dxil.dll` file, or path to the directory containing `dxil.dll` file. Passing `None` will use standard platform specific dll loading rules.
        dxil_path: Option<PathBuf>,
        /// Path to the `dxcompiler.dll` file, or path to the directory containing `dxcompiler.dll` file. Passing `None` will use standard platform specific dll loading rules.
        dxc_path: Option<PathBuf>,
    },
    /// The statically-linked variant of Dxc, sourced from the [`mach-dxcompiler-rs`](https://crates.io/crates/mach-dxcompiler-rs) crate.
    /// The `mach-dxcompiler-rs` feature is required to use this.
    MachDxc,
}

/// Selects which OpenGL ES 3 minor version to request.
///
/// When using ANGLE as an OpenGL ES/EGL implementation, explicitly requesting `Version1` can provide a non-conformant ES 3.1 on APIs like D3D11.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum Gles3MinorVersion {
    /// No explicit minor version is requested, the driver automatically picks the highest available.
    #[default]
    Automatic,

    /// Request an ES 3.0 context.
    Version0,

    /// Request an ES 3.1 context.
    Version1,

    /// Request an ES 3.2 context.
    Version2,
}

/// Options for creating an instance.
#[derive(Debug)]
pub struct InstanceDescriptor {
    /// Which `Backends` to enable.
    pub backends: Backends,
    /// Flags to tune the behavior of the instance.
    pub flags: InstanceFlags,
    /// Which DX12 shader compiler to use.
    pub dx12_shader_compiler: Dx12Compiler,
    /// Which OpenGL ES 3 minor version to request. Will be ignored if OpenGL is available.
    pub gles_minor_version: Gles3MinorVersion,
}

impl Default for InstanceDescriptor {
    fn default() -> Self {
        Self {
            backends: Backends::all(),
            flags: InstanceFlags::default(),
            dx12_shader_compiler: Dx12Compiler::default(),
            gles_minor_version: Gles3MinorVersion::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
/// Descriptor for all size defining attributes of a single triangle geometry inside a bottom level acceleration structure.
pub struct BlasTriangleGeometrySizeDescriptor {
    /// Format of a vertex position, must be [VertexFormat::Float32x3]
    /// with just [Features::EXPERIMENTAL_RAY_TRACING_ACCELERATION_STRUCTURE]
    /// but later features may add more formats.
    pub vertex_format: VertexFormat,
    /// Number of vertices.
    pub vertex_count: u32,
    /// Format of an index. Only needed if an index buffer is used.
    /// If `index_format` is provided `index_count` is required.
    pub index_format: Option<IndexFormat>,
    /// Number of indices. Only needed if an index buffer is used.
    /// If `index_count` is provided `index_format` is required.
    pub index_count: Option<u32>,
    /// Flags for the geometry.
    pub flags: AccelerationStructureGeometryFlags,
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
/// Descriptor for all size defining attributes of all geometries inside a bottom level acceleration structure.
pub enum BlasGeometrySizeDescriptors {
    /// Triangle geometry version.
    Triangles {
        /// Descriptor for each triangle geometry.
        descriptors: Vec<BlasTriangleGeometrySizeDescriptor>,
    },
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
/// Update mode for acceleration structure builds.
pub enum AccelerationStructureUpdateMode {
    /// Always perform a full build.
    Build,
    /// If possible, perform an incremental update.
    ///
    /// Not advised for major topology changes.
    /// (Useful for e.g. skinning)
    PreferUpdate,
}

#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
/// Descriptor for creating a bottom level acceleration structure.
pub struct CreateBlasDescriptor<L> {
    /// Label for the bottom level acceleration structure.
    pub label: L,
    /// Flags for the bottom level acceleration structure.
    pub flags: AccelerationStructureFlags,
    /// Update mode for the bottom level acceleration structure.
    pub update_mode: AccelerationStructureUpdateMode,
}

impl<L> CreateBlasDescriptor<L> {
    /// Takes a closure and maps the label of the blas descriptor into another.
    pub fn map_label<K>(&self, fun: impl FnOnce(&L) -> K) -> CreateBlasDescriptor<K> {
        CreateBlasDescriptor {
            label: fun(&self.label),
            flags: self.flags,
            update_mode: self.update_mode,
        }
    }
}

#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
/// Descriptor for creating a top level acceleration structure.
pub struct CreateTlasDescriptor<L> {
    /// Label for the top level acceleration structure.
    pub label: L,
    /// Number of instances that can be stored in the acceleration structure.
    pub max_instances: u32,
    /// Flags for the bottom level acceleration structure.
    pub flags: AccelerationStructureFlags,
    /// Update mode for the bottom level acceleration structure.
    pub update_mode: AccelerationStructureUpdateMode,
}

impl<L> CreateTlasDescriptor<L> {
    /// Takes a closure and maps the label of the blas descriptor into another.
    pub fn map_label<K>(&self, fun: impl FnOnce(&L) -> K) -> CreateTlasDescriptor<K> {
        CreateTlasDescriptor {
            label: fun(&self.label),
            flags: self.flags,
            update_mode: self.update_mode,
            max_instances: self.max_instances,
        }
    }
}

bitflags::bitflags!(
    /// Flags for acceleration structures
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct AccelerationStructureFlags: u8 {
        /// Allow for incremental updates (no change in size), currently this is unimplemented
        /// and will build as normal (this is fine, update vs build should be unnoticeable)
        const ALLOW_UPDATE = 1 << 0;
        /// Allow the acceleration structure to be compacted in a copy operation, the function
        /// to compact is not currently implemented.
        const ALLOW_COMPACTION = 1 << 1;
        /// Optimize for fast ray tracing performance, recommended if the geometry is unlikely
        /// to change (e.g. in a game: non-interactive scene geometry)
        const PREFER_FAST_TRACE = 1 << 2;
        /// Optimize for fast build time, recommended if geometry is likely to change frequently
        /// (e.g. in a game: player model).
        const PREFER_FAST_BUILD = 1 << 3;
        /// Optimize for low memory footprint (both while building and in the output BLAS).
        const LOW_MEMORY = 1 << 4;
    }
);
impl_bitflags!(AccelerationStructureFlags);

bitflags::bitflags!(
    /// Flags for acceleration structure geometries
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct AccelerationStructureGeometryFlags: u8 {
        /// Is OPAQUE (is there no alpha test) recommended as currently in naga there is no
        /// candidate intersections yet so currently BLASes without this flag will not have hits.
        /// Not enabling this makes the BLAS unable to be interacted with in WGSL.
        const OPAQUE = 1 << 0;
        /// NO_DUPLICATE_ANY_HIT_INVOCATION, not useful unless using hal with wgpu, ray-tracing
        /// pipelines are not supported in wgpu so any-hit shaders do not exist. For when any-hit
        /// shaders are implemented (or experienced users who combine this with an underlying library:
        /// for any primitive (triangle or AABB) multiple any-hit shaders sometimes may be invoked
        /// (especially in AABBs like a sphere), if this flag in present only one hit on a primitive may
        /// invoke an any-hit shader.
        const NO_DUPLICATE_ANY_HIT_INVOCATION = 1 << 1;
    }
);
impl_bitflags!(AccelerationStructureGeometryFlags);

/// Alignment requirement for transform buffers used in acceleration structure builds
pub const TRANSFORM_BUFFER_ALIGNMENT: BufferAddress = 16;

/// Alignment requirement for instance buffers used in acceleration structure builds (`build_acceleration_structures_unsafe_tlas`)
pub const INSTANCE_BUFFER_ALIGNMENT: BufferAddress = 16;

pub use send_sync::*;

#[doc(hidden)]
mod send_sync {
    pub trait WasmNotSendSync: WasmNotSend + WasmNotSync {}
    impl<T: WasmNotSend + WasmNotSync> WasmNotSendSync for T {}
    #[cfg(any(
        not(target_arch = "wasm32"),
        all(
            feature = "fragile-send-sync-non-atomic-wasm",
            not(target_feature = "atomics")
        )
    ))]
    pub trait WasmNotSend: Send {}
    #[cfg(any(
        not(target_arch = "wasm32"),
        all(
            feature = "fragile-send-sync-non-atomic-wasm",
            not(target_feature = "atomics")
        )
    ))]
    impl<T: Send> WasmNotSend for T {}
    #[cfg(not(any(
        not(target_arch = "wasm32"),
        all(
            feature = "fragile-send-sync-non-atomic-wasm",
            not(target_feature = "atomics")
        )
    )))]
    pub trait WasmNotSend {}
    #[cfg(not(any(
        not(target_arch = "wasm32"),
        all(
            feature = "fragile-send-sync-non-atomic-wasm",
            not(target_feature = "atomics")
        )
    )))]
    impl<T> WasmNotSend for T {}

    #[cfg(any(
        not(target_arch = "wasm32"),
        all(
            feature = "fragile-send-sync-non-atomic-wasm",
            not(target_feature = "atomics")
        )
    ))]
    pub trait WasmNotSync: Sync {}
    #[cfg(any(
        not(target_arch = "wasm32"),
        all(
            feature = "fragile-send-sync-non-atomic-wasm",
            not(target_feature = "atomics")
        )
    ))]
    impl<T: Sync> WasmNotSync for T {}
    #[cfg(not(any(
        not(target_arch = "wasm32"),
        all(
            feature = "fragile-send-sync-non-atomic-wasm",
            not(target_feature = "atomics")
        )
    )))]
    pub trait WasmNotSync {}
    #[cfg(not(any(
        not(target_arch = "wasm32"),
        all(
            feature = "fragile-send-sync-non-atomic-wasm",
            not(target_feature = "atomics")
        )
    )))]
    impl<T> WasmNotSync for T {}
}

/// Reason for "lose the device".
///
/// Corresponds to [WebGPU `GPUDeviceLostReason`](https://gpuweb.github.io/gpuweb/#enumdef-gpudevicelostreason).
#[repr(u8)]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DeviceLostReason {
    /// Triggered by driver
    Unknown = 0,
    /// After Device::destroy
    Destroyed = 1,
    /// After Device::drop
    ///
    /// WebGPU does not invoke the device lost callback when the device is
    /// dropped to prevent garbage collection from being observable. In wgpu,
    /// we invoke the callback on drop to help with managing memory owned by
    /// the callback.
    Dropped = 2,
    /// After replacing the device_lost_callback
    ///
    /// WebGPU does not have a concept of a device lost callback, but wgpu
    /// does. wgpu guarantees that any supplied callback will be invoked
    /// exactly once before it is dropped, which helps with managing the
    /// memory owned by the callback.
    ReplacedCallback = 3,
}

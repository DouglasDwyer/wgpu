//! Tests for how backends translate `@early_depth_test`.
//!
//! The snapshot tests cover the exact output for individual shaders. These tests cover
//! behavior that depends on how the backend is driven, such as compiling a single entry point.

#![cfg(all(feature = "wgsl-in", feature = "hlsl-out"))]

use naga::{
    back::hlsl,
    front::wgsl,
    ir::ShaderStage,
    valid::{Capabilities, ValidationFlags, Validator},
};

/// Two fragment entry points that write depth through the same output struct.
const SHARED_OUTPUT: &str = "
struct Output {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@fragment
@early_depth_test(less_equal)
fn conservative(@builtin(position) pos: vec4<f32>) -> Output {
    return Output(vec4<f32>(1.0), pos.z - 0.1);
}

@fragment
fn unrestricted(@builtin(position) pos: vec4<f32>) -> Output {
    return Output(vec4<f32>(1.0), pos.z * 2.0);
}
";

/// Translates `source` to HLSL. If `entry_point` is `Some`, only that
/// fragment entry point is written, as `wgpu` does when creating a pipeline.
fn compile_hlsl(source: &str, entry_point: Option<&str>) -> String {
    let module = wgsl::parse_str(source).expect("shader should parse");
    let info = Validator::new(ValidationFlags::all(), Capabilities::EARLY_DEPTH_TEST)
        .validate(&module)
        .expect("shader should be valid");

    let options = hlsl::Options::default();
    let pipeline_options = hlsl::PipelineOptions {
        entry_point: entry_point.map(|name| (ShaderStage::Fragment, name.to_string())),
    };

    let mut output = String::new();
    hlsl::Writer::new(&mut output, &options, &pipeline_options)
        .write(&module, &info, None)
        .expect("shader should translate");
    output
}

/// Wraps a body in a fragment entry point that returns a depth value.
fn depth_shader(attribute: &str) -> String {
    format!(
        "
@fragment
{attribute}
fn main(@builtin(position) pos: vec4<f32>) -> @builtin(frag_depth) f32 {{
    return pos.z;
}}
"
    )
}

#[test]
fn conservative_depth_selects_semantic() {
    for (attribute, semantic) in [
        (
            "@early_depth_test(greater_equal)",
            ") : SV_DepthGreaterEqual\n",
        ),
        ("@early_depth_test(less_equal)", ") : SV_DepthLessEqual\n"),
        ("@early_depth_test(unchanged)", ") : SV_Depth\n"),
        ("", ") : SV_Depth\n"),
    ] {
        let output = compile_hlsl(&depth_shader(attribute), None);
        assert!(
            output.contains(semantic),
            "`{attribute}` should produce `{semantic:?}`:\n{output}"
        );
    }
}

#[test]
fn force_adds_attribute() {
    let forced = compile_hlsl(&depth_shader("@early_depth_test(force)"), None);
    assert!(
        forced.contains("[earlydepthstencil]\nfloat main("),
        "forced early depth test should add the attribute:\n{forced}"
    );
    assert!(forced.contains(") : SV_Depth\n"), "{forced}");

    for attribute in [
        "",
        "@early_depth_test(greater_equal)",
        "@early_depth_test(less_equal)",
        "@early_depth_test(unchanged)",
    ] {
        let output = compile_hlsl(&depth_shader(attribute), None);
        assert!(
            !output.contains("earlydepthstencil"),
            "`{attribute}` should not force early tests:\n{output}"
        );
    }
}

/// The semantic is part of the struct, so a struct shared with an entry point that
/// makes no promises about depth must not promise anything either.
#[test]
fn shared_struct_with_disagreeing_entry_points_is_unrestricted() {
    let output = compile_hlsl(SHARED_OUTPUT, None);
    assert!(output.contains("float depth : SV_Depth;"), "{output}");
    assert!(!output.contains("SV_DepthLessEqual"), "{output}");
}

/// `wgpu` translates one entry point at a time. Entry points that are not being
/// written must not influence the semantic of the one that is.
#[test]
fn compiling_one_entry_point_ignores_the_others() {
    let conservative = compile_hlsl(SHARED_OUTPUT, Some("conservative"));
    assert!(
        conservative.contains("float depth : SV_DepthLessEqual;"),
        "{conservative}"
    );

    let unrestricted = compile_hlsl(SHARED_OUTPUT, Some("unrestricted"));
    assert!(
        unrestricted.contains("float depth : SV_Depth;"),
        "{unrestricted}"
    );
}

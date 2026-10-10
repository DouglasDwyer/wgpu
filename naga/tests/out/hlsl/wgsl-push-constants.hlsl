struct NagaConstants {
    int first_vertex;
    int first_instance;
    uint other;
};
ConstantBuffer<NagaConstants> _NagaConstants: register(b0, space1);

struct ImmediateData {
    float multiplier;
};

struct FragmentIn {
    float4 color : LOC0;
};

cbuffer im_block : register(b0) { uint4 im_raw[1]; };
static ImmediateData im;
ImmediateData ConstructImmediateData(float arg0) {
    ImmediateData ret = (ImmediateData)0;
    ret.multiplier = arg0;
    return ret;
}


struct FragmentInput_main {
    float4 color : LOC0;
};

float4 vert_main(float2 pos : LOC0, uint ii : SV_InstanceID, uint vi : SV_VertexID) : SV_Position
{
    im = ConstructImmediateData(asfloat(im_raw[0].x));
    float _e8 = im.multiplier;
    return float4((((float((_NagaConstants.first_instance + ii)) * float((_NagaConstants.first_vertex + vi))) * _e8) * pos), 0.0, 1.0);
}

float4 main(FragmentInput_main fragmentinput_main) : SV_Target0
{
    FragmentIn in_ = { fragmentinput_main.color };
    im = ConstructImmediateData(asfloat(im_raw[0].x));
    float _e4 = im.multiplier;
    return (in_.color * _e4);
}

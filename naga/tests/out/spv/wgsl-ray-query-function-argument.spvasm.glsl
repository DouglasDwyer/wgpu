#version 460
#extension GL_EXT_ray_query : require
layout(local_size_x = 1, local_size_y = 1, local_size_z = 1) in;

struct _9
{
    uint _m0;
    uint _m1;
    float _m2;
    float _m3;
    vec3 _m4;
    vec3 _m5;
};

struct _13
{
    uint _m0;
    float _m1;
    uint _m2;
    uint _m3;
    uint _m4;
    uint _m5;
    uint _m6;
    vec2 _m7;
    bool _m8;
    mat4x3 _m9;
    mat4x3 _m10;
};

layout(set = 0, binding = 0) uniform accelerationStructureEXT _14;

void _36(rayQueryEXT _37, accelerationStructureEXT _38, _9 _39, inout uint _40, out float _41)
{
    bool _68 = (_39._m0 & 256u) != 0u;
    bool _76 = (_39._m0 & 16u) != 0u;
    bool _79 = (_39._m0 & 32u) != 0u;
    bool _87 = (_39._m0 & 1u) != 0u;
    bool _90 = (_39._m0 & 2u) != 0u;
    bool _93 = (_39._m0 & 64u) != 0u;
    bool _96 = (_39._m0 & 128u) != 0u;
    _41 = _39._m3;
    if (((((((!((((((_90 && _87) || (_96 && _87)) || (_96 && _90)) || (_96 && _93)) || (_93 && _87)) || (_93 && _90))) && (_39._m2 <= _39._m3)) && (_39._m2 >= 0.0)) && (!(any(isnan(_39._m4)) || any(isinf(_39._m4))))) && (!(any(isnan(_39._m5)) || any(isinf(_39._m5))))) && (!(((_39._m0 & 512u) != 0u) && _68))) && (!(((_76 && _68) || (_79 && _68)) || (_79 && _76))))
    {
        rayQueryInitializeEXT(_37, _38, _39._m0, _39._m1, _39._m4, _39._m2, _39._m5, _39._m3);
        _40 = 1u;
    }
    else
    {
    }
}

bool _143(rayQueryEXT _144, inout uint _145)
{
    bool _147 = false;
    if ((!((_145 & 4u) != 0u)) && ((_145 & 1u) != 0u))
    {
        bool _158 = rayQueryProceedEXT(_144);
        _147 = _158;
        _145 |= (_158 ? 2u : 6u);
    }
    return _147;
}

_13 _171(rayQueryEXT _172, uint _173)
{
    _13 _176 = _13(0u, 0.0, 0u, 0u, 0u, 0u, 0u, vec2(0.0), false, mat4x3(vec3(0.0), vec3(0.0), vec3(0.0), vec3(0.0)), mat4x3(vec3(0.0), vec3(0.0), vec3(0.0), vec3(0.0)));
    if ((!((_173 & 4u) != 0u)) && ((_173 & 2u) != 0u))
    {
        uint _186 = rayQueryGetIntersectionTypeEXT(_172, bool(0u));
        uint _188 = (_186 == 0u) ? 1u : 3u;
        _176._m0 = _188;
        if (_188 != 0u)
        {
            uint _193 = rayQueryGetIntersectionInstanceCustomIndexEXT(_172, bool(0u));
            uint _194 = rayQueryGetIntersectionInstanceIdEXT(_172, bool(0u));
            uint _195 = rayQueryGetIntersectionInstanceShaderBindingTableRecordOffsetEXT(_172, bool(0u));
            uint _196 = rayQueryGetIntersectionGeometryIndexEXT(_172, bool(0u));
            uint _197 = rayQueryGetIntersectionPrimitiveIndexEXT(_172, bool(0u));
            mat4x3 _198 = rayQueryGetIntersectionObjectToWorldEXT(_172, bool(0u));
            mat4x3 _199 = rayQueryGetIntersectionWorldToObjectEXT(_172, bool(0u));
            _176._m2 = _193;
            _176._m3 = _194;
            _176._m4 = _195;
            _176._m5 = _196;
            _176._m6 = _197;
            _176._m9 = _198;
            _176._m10 = _199;
            if (_188 == 1u)
            {
                float _213 = rayQueryGetIntersectionTEXT(_172, bool(0u));
                _176._m1 = _213;
                vec2 _215 = rayQueryGetIntersectionBarycentricsEXT(_172, bool(0u));
                bool _216 = rayQueryGetIntersectionFrontFaceEXT(_172, bool(0u));
                _176._m7 = _215;
                _176._m8 = _216;
            }
        }
    }
    return _176;
}

void _230(rayQueryEXT _231, uint _232, float _233, float _234)
{
    if ((!((_232 & 4u) != 0u)) && ((_232 & 2u) != 0u))
    {
        uint _247 = rayQueryGetIntersectionTypeEXT(_231, bool(0u));
        float _249 = rayQueryGetRayTMinEXT(_231);
        uint _250 = rayQueryGetIntersectionTypeEXT(_231, bool(1u));
        float _237;
        if (_250 == 0u)
        {
            _237 = _234;
        }
        else
        {
            float _256 = rayQueryGetIntersectionTEXT(_231, bool(0u));
            _237 = _256;
        }
        if (((_233 >= _249) && (_233 <= _237)) && (_247 == 1u))
        {
            rayQueryGenerateIntersectionEXT(_231, _233);
        }
    }
}

void _271(rayQueryEXT _272, uint _273)
{
    if ((!((_273 & 4u) != 0u)) && ((_273 & 2u) != 0u))
    {
        uint _284 = rayQueryGetIntersectionTypeEXT(_272, bool(0u));
        if (_284 == 0u)
        {
            rayQueryConfirmIntersectionEXT(_272);
        }
    }
}

void _290(rayQueryEXT _291, uint _292)
{
    if ((!((_292 & 4u) != 0u)) && ((_292 & 2u) != 0u))
    {
        rayQueryTerminateEXT(_291);
    }
}

void _24(rayQueryEXT _17, vec3 _18, vec3 _19, inout uint _22, inout float _23)
{
    uvec2 _129 = uvec2(4294967295u);
    _36(_17, _14, _9(4u, 255u, 0.100000001490116119384765625, 100.0, _18, _19), _22, _23);
    for (;;)
    {
        if (all(equal(uvec2(0u), _129)))
        {
            break;
        }
        _129 -= uvec2(uint(_129.y == 0u), 1u);
        bool _140 = _143(_17, _22);
        if (!_140)
        {
            break;
        }
        _13 _222 = _171(_17, _22);
        if (_222._m0 == 3u)
        {
            _230(_17, _22, _222._m1, _23);
        }
        else
        {
            if (_222._m0 == 1u)
            {
                _271(_17, _22);
            }
            else
            {
                _290(_17, _22);
            }
        }
        continue;
    }
}

_13 _314(rayQueryEXT _315, uint _316)
{
    _13 _318 = _13(0u, 0.0, 0u, 0u, 0u, 0u, 0u, vec2(0.0), false, mat4x3(vec3(0.0), vec3(0.0), vec3(0.0), vec3(0.0)), mat4x3(vec3(0.0), vec3(0.0), vec3(0.0), vec3(0.0)));
    if (((_316 & 4u) != 0u) && ((_316 & 2u) != 0u))
    {
        uint _327 = rayQueryGetIntersectionTypeEXT(_315, bool(1u));
        _318._m0 = _327;
        if (_327 != 0u)
        {
            uint _332 = rayQueryGetIntersectionInstanceCustomIndexEXT(_315, bool(1u));
            uint _333 = rayQueryGetIntersectionInstanceIdEXT(_315, bool(1u));
            uint _334 = rayQueryGetIntersectionInstanceShaderBindingTableRecordOffsetEXT(_315, bool(1u));
            uint _335 = rayQueryGetIntersectionGeometryIndexEXT(_315, bool(1u));
            uint _336 = rayQueryGetIntersectionPrimitiveIndexEXT(_315, bool(1u));
            mat4x3 _337 = rayQueryGetIntersectionObjectToWorldEXT(_315, bool(1u));
            mat4x3 _338 = rayQueryGetIntersectionWorldToObjectEXT(_315, bool(1u));
            _318._m2 = _332;
            _318._m3 = _333;
            _318._m4 = _334;
            _318._m5 = _335;
            _318._m6 = _336;
            _318._m9 = _337;
            _318._m10 = _338;
            float _349 = rayQueryGetIntersectionTEXT(_315, bool(1u));
            _318._m1 = _349;
            if (_327 == 1u)
            {
                vec2 _351 = rayQueryGetIntersectionBarycentricsEXT(_315, bool(1u));
                bool _352 = rayQueryGetIntersectionFrontFaceEXT(_315, bool(1u));
                _318._m7 = _351;
                _318._m8 = _352;
            }
        }
    }
    return _318;
}

float _309(rayQueryEXT _304, vec3 _305, vec3 _306, inout uint _307, inout float _308)
{
    _24(_304, _305, _306, _307, _308);
    return _314(_304, _307)._m1;
}

float _366(rayQueryEXT _359, float _360, rayQueryEXT _361, uint _362, float _363, uint _364, float _365)
{
    return _360 * min(_314(_359, _362)._m1, _314(_361, _364)._m1);
}

void main()
{
    uint _383 = 0u;
    uint _386 = 0u;
    float _384 = 0.0;
    float _387 = 0.0;
    rayQueryEXT _382;
    float _389 = _309(_382, vec3(0.0), vec3(0.0, 1.0, 0.0), _383, _384);
    vec3 _390 = -vec3(0.0, 1.0, 0.0);
    rayQueryEXT _385;
    _24(_385, vec3(0.0), _390, _386, _387);
}


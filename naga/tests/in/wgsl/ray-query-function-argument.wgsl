enable wgpu_ray_query;

@group(0) @binding(0)
var acc_struct: acceleration_structure;

// Takes ownership of the query and runs it to completion.
fn trace(rq: ptr<function, ray_query>, pos: vec3<f32>, dir: vec3<f32>) {
    rayQueryInitialize(rq, acc_struct, RayDesc(RAY_FLAG_TERMINATE_ON_FIRST_HIT, 0xFFu, 0.1, 100.0, pos, dir));
    while (rayQueryProceed(rq)) {
        let candidate = rayQueryGetCandidateIntersection(rq);
        if (candidate.kind == RAY_QUERY_INTERSECTION_AABB) {
            let hit_t = candidate.t;
            rayQueryGenerateIntersection(rq, hit_t);
        } else if (candidate.kind == RAY_QUERY_INTERSECTION_TRIANGLE) {
            rayQueryConfirmIntersection(rq);
        } else {
            rayQueryTerminate(rq);
        }
    }
}

// Passes its argument along to another function.
fn trace_and_get(rq: ptr<function, ray_query>, pos: vec3<f32>, dir: vec3<f32>) -> f32 {
    trace(rq, pos, dir);
    return rayQueryGetCommittedIntersection(rq).t;
}

// Uses more than one query argument, with an ordinary argument in between.
fn closest_of_two(a: ptr<function, ray_query>, scale: f32, b: ptr<function, ray_query>) -> f32 {
    return scale * min(
        rayQueryGetCommittedIntersection(a).t,
        rayQueryGetCommittedIntersection(b).t,
    );
}

@compute @workgroup_size(1)
fn main() {
    var first: ray_query;
    var second: ray_query;

    let pos = vec3<f32>(0.0);
    let dir = vec3<f32>(0.0, 1.0, 0.0);

    let t = trace_and_get(&first, pos, dir);
    trace(&second, pos, -dir);
    let closest = closest_of_two(&first, t, &second);
}

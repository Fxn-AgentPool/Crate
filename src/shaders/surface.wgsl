// Shared by every shader that evaluates the material graph.

// Everything the material graph can read about a surface point.
struct Surface {
    pos: vec3f,        // object space
    pos_norm: vec3f,   // object space normalized to the bounding box, [0,1]^3
    normal: vec3f,     // object-space geometric/vertex normal
    uv: vec2f,         // UDIM uv
    tile_uv: vec2f,    // uv within the tile, [0,1)^2
    layer: i32,        // texture array layer of the tile, -1 if none
    material: u32,
};

struct Material {
    base_color: vec3f,
    roughness: f32,
    metallic: f32,
    height: f32,
    normal: vec3f,     // tangent space, [-1,1]
    emissive: vec3f,
    opacity: f32,
};

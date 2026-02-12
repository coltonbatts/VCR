// Retro Wave Pyramid Shader
// 3D Rotating Pyramid in a Void with Dithering

fn sdPyramid(p_input: vec3<f32>, h: f32) -> f32 {
    var p = p_input;
    let m2 = h*h + 0.25;
    
    p.x = abs(p.x);
    p.z = abs(p.z);
    if (p.z > p.x) {
        let tmp = p.x;
        p.x = p.z;
        p.z = tmp;
    }
    p.x -= 0.5;
    p.z -= 0.5;

    let q = vec3<f32>(p.z, h*p.y - 0.5*p.x, h*p.x + 0.5*p.y);
    
    let s = max(-q.x, 0.0);
    let t = clamp((q.y - 0.5*p.z) / (m2 + 0.25), 0.0, 1.0);
    
    let a = q.x + s;
    let b = q.y - t*0.5;
    let c = q.z - t*h;
    
    let d2 = min(q.y*q.y + q.z*q.z, a*a + b*b + c*c);
    
    return sqrt(d2) * sign(max(q.y, -q.x));
}

fn rotateY(p: vec3<f32>, a: f32) -> vec3<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec3<f32>(p.x * c - p.z * s, p.y, p.x * s + p.z * c);
}

fn rotateX(p: vec3<f32>, a: f32) -> vec3<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec3<f32>(p.x, p.y * c - p.z * s, p.y * s + p.z * c);
}

fn dither4x4(pos: vec2<f32>, brightness: f32) -> f32 {
    var bayer = array<f32, 16>(
        0.0 / 16.0, 0.5 / 16.0, 0.125 / 16.0, 0.625 / 16.0,
        0.75 / 16.0, 0.25 / 16.0, 0.875 / 16.0, 0.375 / 16.0,
        0.1875 / 16.0, 0.6875 / 16.0, 0.0625 / 16.0, 0.5625 / 16.0,
        0.9375 / 16.0, 0.4375 / 16.0, 0.8125 / 16.0, 0.3125 / 16.0
    );
    let index = u32(pos.x) % 4u + (u32(pos.y) % 4u) * 4u;
    if (brightness > bayer[index]) { return 1.0; }
    return 0.0;
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let aspect = u.resolution.x / u.resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    // Camera setup
    let ro = vec3<f32>(0.0, 0.5, -2.5);
    let rd = normalize(vec3<f32>(p, 1.5));
    
    // Animation
    let time = u.time;
    let rot_y = time * 1.5;
    let rot_x = sin(time * 0.5) * 0.3;
    
    // Ray marching
    var d = 0.0;
    var t = 0.0;
    var hit = false;
    var normal = vec3<f32>(0.0);
    
    for (var i = 0; i < 64; i++) {
        var pos = ro + rd * t;
        pos = rotateY(pos, rot_y);
        pos = rotateX(pos, rot_x);
        
        d = sdPyramid(pos - vec3<f32>(0.0, -0.3, 0.0), 1.0);
        if (d < 0.001) {
            hit = true;
            break;
        }
        t += d;
        if (t > 10.0) { break; }
    }
    
    if (hit) {
        let pos = ro + rd * t;
        let p_rot = rotateX(rotateY(pos, rot_y), rot_x);
        
        // Simple normal estimation
        let e = vec2<f32>(0.001, 0.0);
        let n = normalize(vec3<f32>(
            sdPyramid(rotateX(rotateY(pos + e.xyy, rot_y), rot_x) - vec3<f32>(0.0, -0.3, 0.0), 1.0) - sdPyramid(p_rot - vec3<f32>(0.0, -0.3, 0.0), 1.0),
            sdPyramid(rotateX(rotateY(pos + e.yxy, rot_y), rot_x) - vec3<f32>(0.0, -0.3, 0.0), 1.0) - sdPyramid(p_rot - vec3<f32>(0.0, -0.3, 0.0), 1.0),
            sdPyramid(rotateX(rotateY(pos + e.yyx, rot_y), rot_x) - vec3<f32>(0.0, -0.3, 0.0), 1.0) - sdPyramid(p_rot - vec3<f32>(0.0, -0.3, 0.0), 1.0)
        ));
        
        let light_dir = normalize(vec3<f32>(1.0, 2.0, -2.0));
        let diff = max(dot(n, light_dir), 0.0);
        
        // Dark wave palette
        let color_top = vec3<f32>(1.0, 0.0, 1.0); // Magenta
        let color_bottom = vec3<f32>(0.0, 1.0, 1.0); // Cyan
        let base_color = mix(color_bottom, color_top, uv.y);
        
        let lit_color = base_color * (diff + 0.2);
        let brightness = (lit_color.r + lit_color.g + lit_color.b) / 3.0;
        
        // Dithering
        let dithered = dither4x4(uv * u.resolution, brightness);
        let final_color = lit_color * dithered;
        
        return vec4<f32>(final_color, 1.0);
    }
    
    // Background (Void)
    let bg_color = vec3<f32>(0.02, 0.0, 0.05); // Very dark purple
    let bg_dither = dither4x4(uv * u.resolution, 0.05);
    return vec4<f32>(bg_color * bg_dither, 0.0); // Alpha 0 for transparency in MOV
}

// VCR Standard Library Registry
// These are bundled as strings for offline availability.

pub const COMMON_WGSL: &str = include_str!("common.wgsl");
pub const NOISE_WGSL: &str = include_str!("noise.wgsl");
pub const SDF_WGSL: &str = include_str!("sdf.wgsl");
pub const RAYMARCH_WGSL: &str = include_str!("raymarch.wgsl");

pub fn get_module(name: &str) -> Option<&'static str> {
    match name {
        "common" => Some(COMMON_WGSL),
        "noise" => Some(NOISE_WGSL),
        "sdf" => Some(SDF_WGSL),
        "raymarch" => Some(RAYMARCH_WGSL),
        _ => None,
    }
}

pub fn preprocess_wgsl(source: &str) -> String {
    let mut output = String::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if (trimmed.starts_with("#include \"vcr:") || trimmed.starts_with("@import \"vcr:")) && trimmed.ends_with("\"")
        {
            let parts: Vec<&str> = trimmed.split('"').collect();
            if parts.len() >= 2 {
                let path = parts[1];
                if let Some(module_name) = path.strip_prefix("vcr:") {
                    if let Some(content) = get_module(module_name) {
                        output.push_str("// --- BEGIN VCR STD: ");
                        output.push_str(module_name);
                        output.push_str(" ---\n");
                        output.push_str(content);
                        output.push_str("// --- END VCR STD: ");
                        output.push_str(module_name);
                        output.push_str(" ---\n");
                        continue;
                    }
                }
            }
        }
        output.push_str(line);
        output.push('\n');
    }
    output
}

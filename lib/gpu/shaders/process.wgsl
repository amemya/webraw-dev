// WebGPU Compute Shader: Image Processing Pipeline
// Combined shader for all processing stages to minimize dispatch overhead.
//
// Pipeline: Black level → White Balance → Exposure → Contrast → 
//           Highlights/Shadows → Saturation → Tone curve → sRGB Gamma

struct Params {
  width: u32,
  height: u32,
  wb_r: f32,
  wb_g: f32,
  wb_b: f32,
  exposure: f32,       // EV stops
  contrast: f32,       // -1.0 to 1.0
  highlights: f32,     // -1.0 to 1.0
  shadows: f32,        // -1.0 to 1.0
  saturation: f32,     // -1.0 to 1.0
  _padding1: f32,
  _padding2: f32,
}

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;

// sRGB gamma encoding
fn linear_to_srgb(c: f32) -> f32 {
  if (c <= 0.0031308) {
    return c * 12.92;
  }
  return 1.055 * pow(c, 1.0 / 2.4) - 0.055;
}

// Apply tone curve with highlight/shadow recovery
fn apply_tone_curve(x: f32, highlights: f32, shadows: f32) -> f32 {
  var v = x;

  // Shadow lift: boost dark values
  if (shadows > 0.0) {
    let shadow_mask = 1.0 - smoothstep(0.0, 0.5, v);
    v = v + shadow_mask * shadows * 0.3;
  } else if (shadows < 0.0) {
    let shadow_mask = 1.0 - smoothstep(0.0, 0.5, v);
    v = v * (1.0 + shadow_mask * shadows * 0.5);
  }

  // Highlight recovery: pull down bright values
  if (highlights < 0.0) {
    let highlight_mask = smoothstep(0.5, 1.0, v);
    v = v + highlight_mask * highlights * 0.3;
  } else if (highlights > 0.0) {
    let highlight_mask = smoothstep(0.5, 1.0, v);
    v = v + highlight_mask * highlights * 0.2;
  }

  return clamp(v, 0.0, 1.0);
}

// Apply contrast using S-curve
fn apply_contrast(x: f32, amount: f32) -> f32 {
  if (abs(amount) < 0.001) {
    return x;
  }
  // S-curve contrast centered at 0.5
  let centered = x - 0.5;
  let factor = 1.0 + amount;
  return clamp(centered * factor + 0.5, 0.0, 1.0);
}

// Luminance for saturation calculation (Rec.709)
fn luminance(r: f32, g: f32, b: f32) -> f32 {
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let x = gid.x;
  let y = gid.y;

  if (x >= params.width || y >= params.height) {
    return;
  }

  let idx = (y * params.width + x) * 3u;

  // Read linear RGB
  var r = input[idx];
  var g = input[idx + 1u];
  var b = input[idx + 2u];

  // 1. White Balance
  r = r * params.wb_r;
  g = g * params.wb_g;
  b = b * params.wb_b;

  // 2. Exposure (EV stops)
  let exp_mul = pow(2.0, params.exposure);
  r = r * exp_mul;
  g = g * exp_mul;
  b = b * exp_mul;

  // 3. Clamp after exposure
  r = clamp(r, 0.0, 1.0);
  g = clamp(g, 0.0, 1.0);
  b = clamp(b, 0.0, 1.0);

  // 4. Contrast
  r = apply_contrast(r, params.contrast);
  g = apply_contrast(g, params.contrast);
  b = apply_contrast(b, params.contrast);

  // 5. Highlights / Shadows
  r = apply_tone_curve(r, params.highlights, params.shadows);
  g = apply_tone_curve(g, params.highlights, params.shadows);
  b = apply_tone_curve(b, params.highlights, params.shadows);

  // 6. Saturation
  if (abs(params.saturation) > 0.001) {
    let lum = luminance(r, g, b);
    let sat_factor = 1.0 + params.saturation;
    r = clamp(lum + (r - lum) * sat_factor, 0.0, 1.0);
    g = clamp(lum + (g - lum) * sat_factor, 0.0, 1.0);
    b = clamp(lum + (b - lum) * sat_factor, 0.0, 1.0);
  }

  // 7. sRGB Gamma
  r = linear_to_srgb(r);
  g = linear_to_srgb(g);
  b = linear_to_srgb(b);

  // Write output
  output[idx] = r;
  output[idx + 1u] = g;
  output[idx + 2u] = b;
}

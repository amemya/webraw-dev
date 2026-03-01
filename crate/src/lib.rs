pub mod decode;
pub mod demosaic;
pub mod dcp;
pub mod color_pipeline;

use wasm_bindgen::prelude::*;

/// Decode a RAW image file from a byte array.
///
/// Accepts the raw file bytes and returns a JS object with:
/// - `pixels`: Float32Array of RGB data (0.0-1.0), length = width * height * 3
/// - `width`: image width
/// - `height`: image height
/// - `metadata`: object with wb_coeffs, black_levels, white_levels, cfa_pattern, etc.
#[wasm_bindgen]
pub fn decode_raw(data: &[u8]) -> Result<JsValue, JsError> {
    let result = decode::decode_raw_bytes(data)
        .map_err(|e| JsError::new(&e))?;

    // Build the return object
    let obj = js_sys::Object::new();

    // Set pixel data as Float32Array
    let pixels_array = js_sys::Float32Array::from(result.pixels.as_slice());
    js_sys::Reflect::set(&obj, &"pixels".into(), &pixels_array)
        .map_err(|_| JsError::new("Failed to set pixels"))?;

    // Set dimensions
    js_sys::Reflect::set(&obj, &"width".into(), &JsValue::from_f64(result.metadata.width as f64))
        .map_err(|_| JsError::new("Failed to set width"))?;
    js_sys::Reflect::set(&obj, &"height".into(), &JsValue::from_f64(result.metadata.height as f64))
        .map_err(|_| JsError::new("Failed to set height"))?;

    // Set metadata
    let metadata = serde_wasm_bindgen::to_value(&result.metadata)
        .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))?;
    js_sys::Reflect::set(&obj, &"metadata".into(), &metadata)
        .map_err(|_| JsError::new("Failed to set metadata"))?;

    // Set display_referred flag (true = DCP tone curve applied, skip sRGB gamma)
    js_sys::Reflect::set(&obj, &"displayReferred".into(), &JsValue::from_bool(result.display_referred))
        .map_err(|_| JsError::new("Failed to set displayReferred"))?;

    Ok(obj.into())
}

/// Get the version of the raw-processor WASM module
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

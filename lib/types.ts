// lib/types.ts
// Shared type definitions for the web RAW processor

export interface RawMetadata {
  width: number;
  height: number;
  make: string;
  model: string;
  wb_coeffs: [number, number, number, number];
  black_levels: number[];
  white_levels: number[];
  cfa_pattern: string;
  color_matrix: number[];
  tone_curve: number[];
  xyz_to_cam_raw: number[];
}

export interface DecodedImage {
  pixels: Float32Array;
  width: number;
  height: number;
  metadata: RawMetadata;
}

export interface ProcessingParams {
  whiteBalance: [number, number, number]; // R, G, B multipliers
  exposure: number;                       // EV (-5 to +5)
  contrast: number;                       // -100 to +100
  highlights: number;                     // -100 to +100
  shadows: number;                        // -100 to +100
  temperature: number;                    // Kelvin (2000-12000)
  tint: number;                           // -150 to +150
  saturation: number;                     // -100 to +100
}

export const DEFAULT_PARAMS: ProcessingParams = {
  whiteBalance: [1.0, 1.0, 1.0],
  exposure: 0,
  contrast: 0,
  highlights: 0,
  shadows: 0,
  temperature: 6500,
  tint: 0,
  saturation: 0,
};

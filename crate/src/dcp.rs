/// DCP (DNG Camera Profile) parser
///
/// Parses TIFF-based DCP files to extract color profile data:
/// - ColorMatrix1/2 and ForwardMatrix1/2 (3x3 SRATIONAL)
/// - HueSatMapDims and HueSatMapData (3D LUT)
/// - ProfileLookTableDims and ProfileLookTableData
/// - ProfileToneCurve
/// - CalibrationIlluminant1/2

use std::io::{Cursor, Read, Seek, SeekFrom};

/// DCP Tag IDs (DNG spec)
const TAG_CALIBRATION_ILLUMINANT_1: u16 = 50778; // C612
const TAG_CALIBRATION_ILLUMINANT_2: u16 = 50779;
const TAG_COLOR_MATRIX_1: u16 = 50721; // C621
const TAG_COLOR_MATRIX_2: u16 = 50722; // C622
const TAG_FORWARD_MATRIX_1: u16 = 50964; // C714
const TAG_FORWARD_MATRIX_2: u16 = 50965; // C715
const TAG_PROFILE_HUE_SAT_MAP_DIMS: u16 = 50937; // C725
const TAG_PROFILE_HUE_SAT_MAP_DATA_1: u16 = 50938; // C726
const TAG_PROFILE_HUE_SAT_MAP_DATA_2: u16 = 50939;
const TAG_PROFILE_TONE_CURVE: u16 = 50940; // C6FC
const TAG_PROFILE_LOOK_TABLE_DIMS: u16 = 50981;
const TAG_PROFILE_LOOK_TABLE_DATA: u16 = 50982;
const TAG_PROFILE_NAME: u16 = 50936;
const TAG_PROFILE_HUE_SAT_MAP_ENCODING: u16 = 51107;

/// TIFF type IDs
const TYPE_SHORT: u16 = 3;  // u16
const TYPE_LONG: u16 = 4;   // u32
const TYPE_SRATIONAL: u16 = 10; // two i32 (numerator/denominator)
const TYPE_FLOAT: u16 = 11; // f32
const TYPE_DOUBLE: u16 = 12; // f64

/// Standard illuminant IDs
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Illuminant {
    StdA = 17,         // 2856K (tungsten)
    D50 = 23,
    D55 = 20,
    D65 = 21,
    D75 = 22,
    Unknown = 0,
}

impl From<u16> for Illuminant {
    fn from(v: u16) -> Self {
        match v {
            17 => Illuminant::StdA,
            20 => Illuminant::D55,
            21 => Illuminant::D65,
            22 => Illuminant::D75,
            23 => Illuminant::D50,
            _ => Illuminant::Unknown,
        }
    }
}

impl Illuminant {
    /// Approximate correlated color temperature
    pub fn temperature(&self) -> f64 {
        match self {
            Illuminant::StdA => 2856.0,
            Illuminant::D50 => 5003.0,
            Illuminant::D55 => 5503.0,
            Illuminant::D65 => 6504.0,
            Illuminant::D75 => 7504.0,
            Illuminant::Unknown => 6504.0, // default to D65
        }
    }
}

/// HueSatMap dimensions
#[derive(Debug, Clone, Copy, Default)]
pub struct HueSatMapDims {
    pub hue_divs: u32,
    pub sat_divs: u32,
    pub val_divs: u32,
}

/// Single entry in a HueSatMap
#[derive(Debug, Clone, Copy, Default)]
pub struct HueSatMapEntry {
    pub hue_shift: f32,  // added to hue (in degrees or wrapping)
    pub sat_scale: f32,  // multiplied to saturation
    pub val_scale: f32,  // multiplied to value
}

/// Parsed DCP profile data
#[derive(Debug, Clone)]
pub struct DcpProfile {
    pub name: String,

    pub illuminant1: Illuminant,
    pub illuminant2: Illuminant,

    /// ColorMatrix1: XYZ → camera (3x3, row-major)
    pub color_matrix_1: Option<[f64; 9]>,
    pub color_matrix_2: Option<[f64; 9]>,

    /// ForwardMatrix1: camera → XYZ (3x3, row-major)
    pub forward_matrix_1: Option<[f64; 9]>,
    pub forward_matrix_2: Option<[f64; 9]>,

    /// HueSatMap dimensions and data
    pub hue_sat_map_dims: Option<HueSatMapDims>,
    pub hue_sat_map_data_1: Option<Vec<HueSatMapEntry>>,
    pub hue_sat_map_data_2: Option<Vec<HueSatMapEntry>>,

    /// HueSatMap encoding (0 = linear, 1 = sRGB)
    pub hue_sat_map_encoding: u32,

    /// Profile Look Table (secondary 3D LUT)
    pub look_table_dims: Option<HueSatMapDims>,
    pub look_table_data: Option<Vec<HueSatMapEntry>>,

    /// Tone curve as (input, output) pairs
    pub tone_curve: Option<Vec<(f32, f32)>>,
}

impl Default for DcpProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            illuminant1: Illuminant::StdA,
            illuminant2: Illuminant::D65,
            color_matrix_1: None,
            color_matrix_2: None,
            forward_matrix_1: None,
            forward_matrix_2: None,
            hue_sat_map_dims: None,
            hue_sat_map_data_1: None,
            hue_sat_map_data_2: None,
            hue_sat_map_encoding: 0,
            look_table_dims: None,
            look_table_data: None,
            tone_curve: None,
        }
    }
}

/// Parse a DCP file from bytes
pub fn parse_dcp(data: &[u8]) -> Result<DcpProfile, String> {
    if data.len() < 8 {
        return Err("DCP file too short".into());
    }

    // Check byte order: 'II' = little-endian, 'MM' = big-endian
    let little_endian = match (data[0], data[1]) {
        (b'I', b'I') => true,
        (b'M', b'M') => false,
        _ => return Err(format!("Invalid TIFF byte order: {:02x}{:02x}", data[0], data[1])),
    };

    // DCP magic number: 0x4352 ('CR') instead of TIFF's 0x002A
    let magic = read_u16(data, 2, little_endian);
    if magic != 0x4352 {
        return Err(format!("Not a DCP file (magic: 0x{:04x})", magic));
    }

    // IFD offset
    let ifd_offset = read_u32(data, 4, little_endian) as usize;
    if ifd_offset >= data.len() {
        return Err("IFD offset out of bounds".into());
    }

    let mut profile = DcpProfile::default();
    parse_ifd(data, ifd_offset, little_endian, &mut profile)?;

    Ok(profile)
}

/// Parse a TIFF IFD and extract DCP tags
fn parse_ifd(data: &[u8], offset: usize, le: bool, profile: &mut DcpProfile) -> Result<(), String> {
    if offset + 2 > data.len() {
        return Err("IFD offset out of bounds".into());
    }

    let entry_count = read_u16(data, offset, le) as usize;
    let entries_start = offset + 2;

    for i in 0..entry_count {
        let entry_offset = entries_start + i * 12;
        if entry_offset + 12 > data.len() {
            break;
        }

        let tag = read_u16(data, entry_offset, le);
        let typ = read_u16(data, entry_offset + 2, le);
        let count = read_u32(data, entry_offset + 4, le) as usize;
        let value_offset_raw = read_u32(data, entry_offset + 8, le);

        // For data that fits in 4 bytes, value is inline; otherwise it's an offset
        let value_size = type_size(typ) * count;
        let value_ptr = if value_size <= 4 {
            entry_offset + 8
        } else {
            value_offset_raw as usize
        };

        match tag {
            TAG_PROFILE_NAME => {
                if value_ptr + count <= data.len() {
                    profile.name = String::from_utf8_lossy(&data[value_ptr..value_ptr + count])
                        .trim_end_matches('\0')
                        .to_string();
                }
            }
            TAG_CALIBRATION_ILLUMINANT_1 => {
                let v = read_short_or_long(data, value_ptr, typ, le);
                profile.illuminant1 = Illuminant::from(v as u16);
            }
            TAG_CALIBRATION_ILLUMINANT_2 => {
                let v = read_short_or_long(data, value_ptr, typ, le);
                profile.illuminant2 = Illuminant::from(v as u16);
            }
            TAG_COLOR_MATRIX_1 => {
                profile.color_matrix_1 = Some(read_matrix_3x3(data, value_ptr, typ, count, le));
            }
            TAG_COLOR_MATRIX_2 => {
                profile.color_matrix_2 = Some(read_matrix_3x3(data, value_ptr, typ, count, le));
            }
            TAG_FORWARD_MATRIX_1 => {
                profile.forward_matrix_1 = Some(read_matrix_3x3(data, value_ptr, typ, count, le));
            }
            TAG_FORWARD_MATRIX_2 => {
                profile.forward_matrix_2 = Some(read_matrix_3x3(data, value_ptr, typ, count, le));
            }
            TAG_PROFILE_HUE_SAT_MAP_DIMS => {
                profile.hue_sat_map_dims = Some(read_hue_sat_dims(data, value_ptr, typ, le));
            }
            TAG_PROFILE_HUE_SAT_MAP_DATA_1 => {
                profile.hue_sat_map_data_1 = Some(read_hue_sat_data(data, value_ptr, typ, count, le));
            }
            TAG_PROFILE_HUE_SAT_MAP_DATA_2 => {
                profile.hue_sat_map_data_2 = Some(read_hue_sat_data(data, value_ptr, typ, count, le));
            }
            TAG_PROFILE_HUE_SAT_MAP_ENCODING => {
                profile.hue_sat_map_encoding = read_short_or_long(data, value_ptr, typ, le);
            }
            TAG_PROFILE_LOOK_TABLE_DIMS => {
                profile.look_table_dims = Some(read_hue_sat_dims(data, value_ptr, typ, le));
            }
            TAG_PROFILE_LOOK_TABLE_DATA => {
                profile.look_table_data = Some(read_hue_sat_data(data, value_ptr, typ, count, le));
            }
            TAG_PROFILE_TONE_CURVE => {
                profile.tone_curve = Some(read_tone_curve(data, value_ptr, typ, count, le));
            }
            _ => {} // ignore unknown tags
        }
    }

    Ok(())
}

// ---- Binary reading helpers ----

fn read_u16(data: &[u8], offset: usize, le: bool) -> u16 {
    if offset + 2 > data.len() { return 0; }
    if le {
        u16::from_le_bytes([data[offset], data[offset + 1]])
    } else {
        u16::from_be_bytes([data[offset], data[offset + 1]])
    }
}

fn read_u32(data: &[u8], offset: usize, le: bool) -> u32 {
    if offset + 4 > data.len() { return 0; }
    if le {
        u32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]])
    } else {
        u32::from_be_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]])
    }
}

fn read_i32(data: &[u8], offset: usize, le: bool) -> i32 {
    if offset + 4 > data.len() { return 0; }
    if le {
        i32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]])
    } else {
        i32::from_be_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]])
    }
}

fn read_f32(data: &[u8], offset: usize, le: bool) -> f32 {
    if offset + 4 > data.len() { return 0.0; }
    if le {
        f32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]])
    } else {
        f32::from_be_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]])
    }
}

fn read_f64(data: &[u8], offset: usize, le: bool) -> f64 {
    if offset + 8 > data.len() { return 0.0; }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&data[offset..offset+8]);
    if le { f64::from_le_bytes(bytes) } else { f64::from_be_bytes(bytes) }
}

fn read_short_or_long(data: &[u8], offset: usize, typ: u16, le: bool) -> u32 {
    match typ {
        TYPE_SHORT => read_u16(data, offset, le) as u32,
        TYPE_LONG => read_u32(data, offset, le),
        _ => read_u32(data, offset, le),
    }
}

fn type_size(typ: u16) -> usize {
    match typ {
        1 | 2 | 6 | 7 => 1,   // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8 => 2,            // SHORT, SSHORT
        4 | 9 | 11 => 4,      // LONG, SLONG, FLOAT
        5 | 10 | 12 => 8,     // RATIONAL, SRATIONAL, DOUBLE
        _ => 1,
    }
}

/// Read a SRATIONAL value (numerator/denominator as i32 pair)
fn read_srational(data: &[u8], offset: usize, le: bool) -> f64 {
    let num = read_i32(data, offset, le);
    let den = read_i32(data, offset + 4, le);
    if den == 0 { return 0.0; }
    num as f64 / den as f64
}

/// Read a 3x3 matrix from SRATIONAL or FLOAT data
fn read_matrix_3x3(data: &[u8], offset: usize, typ: u16, count: usize, le: bool) -> [f64; 9] {
    let mut matrix = [0.0f64; 9];
    let n = count.min(9);
    for i in 0..n {
        matrix[i] = match typ {
            TYPE_SRATIONAL => read_srational(data, offset + i * 8, le),
            TYPE_FLOAT => read_f32(data, offset + i * 4, le) as f64,
            TYPE_DOUBLE => read_f64(data, offset + i * 8, le),
            _ => 0.0,
        };
    }
    matrix
}

/// Read HueSatMap dimensions [hue_divs, sat_divs, val_divs]
fn read_hue_sat_dims(data: &[u8], offset: usize, typ: u16, le: bool) -> HueSatMapDims {
    HueSatMapDims {
        hue_divs: read_short_or_long(data, offset, typ, le),
        sat_divs: read_short_or_long(data, offset + type_size(typ), typ, le),
        val_divs: read_short_or_long(data, offset + type_size(typ) * 2, typ, le),
    }
}

/// Read HueSatMap data (array of float triplets: hue_shift, sat_scale, val_scale)
fn read_hue_sat_data(data: &[u8], offset: usize, typ: u16, count: usize, le: bool) -> Vec<HueSatMapEntry> {
    let num_entries = count / 3;
    let mut entries = Vec::with_capacity(num_entries);
    let elem_size = type_size(typ);

    for i in 0..num_entries {
        let base = offset + i * 3 * elem_size;
        let (h, s, v) = match typ {
            TYPE_FLOAT => (
                read_f32(data, base, le),
                read_f32(data, base + 4, le),
                read_f32(data, base + 8, le),
            ),
            TYPE_DOUBLE => (
                read_f64(data, base, le) as f32,
                read_f64(data, base + 8, le) as f32,
                read_f64(data, base + 16, le) as f32,
            ),
            _ => (0.0, 0.0, 0.0),
        };
        entries.push(HueSatMapEntry {
            hue_shift: h,
            sat_scale: s,
            val_scale: v,
        });
    }
    entries
}

/// Read tone curve as (input, output) float pairs
fn read_tone_curve(data: &[u8], offset: usize, typ: u16, count: usize, le: bool) -> Vec<(f32, f32)> {
    let num_points = count / 2;
    let mut curve = Vec::with_capacity(num_points);
    let elem_size = type_size(typ);

    for i in 0..num_points {
        let base = offset + i * 2 * elem_size;
        let (input, output) = match typ {
            TYPE_FLOAT => (
                read_f32(data, base, le),
                read_f32(data, base + 4, le),
            ),
            TYPE_DOUBLE => (
                read_f64(data, base, le) as f32,
                read_f64(data, base + 8, le) as f32,
            ),
            _ => (0.0, 0.0),
        };
        curve.push((input, output));
    }
    curve
}

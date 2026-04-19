// Auto-generated stub
// src/header.rs
//! MPX file header — fixed 32 bytes.
//!
//! Layout:
//!   [0..4]   magic:       0x4D 0x50 0x58 0x00  ("MPX\0")
//!   [4]      version:     u8 = 1
//!   [5]      color_type:  u8
//!   [6]      bit_depth:   u8  (8 or 16)
//!   [7]      filter_type: u8
//!   [8..12]  width:       u32 LE
//!   [12..16] height:      u32 LE
//!   [16]     channel_count: u8  (derived, stored for fast reads)
//!   [17]     flags:       u8
//!   [18..32] reserved:    zeros
//!
//! After the header: one compressed block per channel.
//! Each block: [compressed_len: u32 LE] [MBFA-compressed bytes]

pub const MAGIC:       [u8; 4] = [0x4D, 0x50, 0x58, 0x00];
pub const VERSION:     u8      = 1;
pub const HEADER_SIZE: usize   = 32;

/// Bit in `flags`: split 16-bit samples into high-byte and low-byte planes
/// before MBFA compression. Dramatically improves LZ efficiency on smooth
/// gradients. Always set when bit_depth=16. Ignored for 8-bit images.
pub const FLAG_BYTE_PLANE_SPLIT: u8 = 0b0000_0001;

// ── Color type ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorType {
    Gray  = 0,
    GrayA = 1,
    Rgb   = 2,
    Rgba  = 3,
}

impl ColorType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Gray),
            1 => Some(Self::GrayA),
            2 => Some(Self::Rgb),
            3 => Some(Self::Rgba),
            _ => None,
        }
    }

    pub fn channel_count(self) -> usize {
        match self {
            Self::Gray  => 1,
            Self::GrayA => 2,
            Self::Rgb   => 3,
            Self::Rgba  => 4,
        }
    }
}

// ── Filter type ───────────────────────────────────────────────────────────────

/// Spatial prediction filter applied per row before MBFA compression.
/// Converts pixel values into near-zero residuals that LZ matches more
/// densely. Paeth is the best general-purpose choice (same as PNG).
///
/// NOTE: `Adaptive` in v1 selects Paeth for all rows. True per-row
/// adaptive mode (store 1 filter byte per row, pick best per row like
/// PNG's adaptive mode) is a v2 TODO — requires changing the plane
/// serialisation format to prefix each row with its filter byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterType {
    None     = 0,
    Sub      = 1,
    Up       = 2,
    Average  = 3,
    Paeth    = 4,
    /// v1: auto-selects Paeth. v2: per-row best-filter selection.
    Adaptive = 5,
}

impl FilterType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Sub),
            2 => Some(Self::Up),
            3 => Some(Self::Average),
            4 => Some(Self::Paeth),
            5 => Some(Self::Adaptive),
            _ => None,
        }
    }

    /// Resolve the effective filter for encode/decode.
    /// Adaptive maps to Paeth in v1.
    pub fn effective(self) -> FilterType {
        match self {
            Self::Adaptive => Self::Paeth,
            other          => other,
        }
    }
}

// ── Header ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MpxHeader {
    pub color_type:  ColorType,
    pub bit_depth:   u8,
    pub filter_type: FilterType,
    pub width:       u32,
    pub height:      u32,
    pub flags:       u8,
}

impl MpxHeader {
    pub fn channel_count(&self) -> usize {
        self.color_type.channel_count()
    }

    /// True when the 16-bit byte-plane split flag is set.
    pub fn byte_plane_split(&self) -> bool {
        self.bit_depth == 16 && (self.flags & FLAG_BYTE_PLANE_SPLIT) != 0
    }

    /// Bytes per sample (1 for 8-bit, 2 for 16-bit).
    pub fn bytes_per_sample(&self) -> usize {
        (self.bit_depth / 8) as usize
    }

    /// Row stride in bytes for one channel plane.
    pub fn plane_row_bytes(&self) -> usize {
        self.width as usize * self.bytes_per_sample()
    }

    /// Total bytes for one channel plane (uncompressed).
    pub fn plane_bytes(&self) -> usize {
        self.height as usize * self.plane_row_bytes()
    }

    pub fn serialize(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        buf[0..4].copy_from_slice(&MAGIC);
        buf[4]  = VERSION;
        buf[5]  = self.color_type as u8;
        buf[6]  = self.bit_depth;
        buf[7]  = self.filter_type as u8;
        buf[8..12].copy_from_slice(&self.width.to_le_bytes());
        buf[12..16].copy_from_slice(&self.height.to_le_bytes());
        buf[16] = self.channel_count() as u8;
        buf[17] = self.flags;
        // [18..32] reserved, zero
        buf
    }

    pub fn parse(data: &[u8]) -> std::io::Result<Self> {
        if data.len() < HEADER_SIZE {
            return Err(err("MPX header too short"));
        }
        if data[0..4] != MAGIC {
            return Err(err("not an MPX file (bad magic)"));
        }
        if data[4] != VERSION {
            return Err(err(format!("unsupported MPX version: {}", data[4])));
        }

        let color_type = ColorType::from_u8(data[5])
            .ok_or_else(|| err(format!("unknown color type: {}", data[5])))?;

        let bit_depth = data[6];
        if bit_depth != 8 && bit_depth != 16 {
            return Err(err(format!("unsupported bit depth: {} (must be 8 or 16)", bit_depth)));
        }

        let filter_type = FilterType::from_u8(data[7])
            .ok_or_else(|| err(format!("unknown filter type: {}", data[7])))?;

        let width  = u32::from_le_bytes(data[8..12].try_into().unwrap());
        let height = u32::from_le_bytes(data[12..16].try_into().unwrap());
        let flags  = data[17];

        if width == 0 || height == 0 {
            return Err(err("zero-dimension image"));
        }

        // Sanity: width * height must not overflow usize
        let _ = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(color_type.channel_count()))
            .and_then(|n| n.checked_mul((bit_depth / 8) as usize))
            .ok_or_else(|| err("image dimensions overflow usize"))?;

        Ok(MpxHeader { color_type, bit_depth, filter_type, width, height, flags })
    }
}

#[inline]
fn err(msg: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.into())
         }

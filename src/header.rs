// src/header.rs

pub const MAGIC:       [u8; 4] = [0x4D, 0x50, 0x58, 0x00];
pub const VERSION:     u8      = 1;
pub const HEADER_SIZE: usize   = 32;

/// bit 0: split 16-bit samples into hi/lo byte planes before MBFA.
pub const FLAG_BYTE_PLANE_SPLIT:    u8 = 0b0000_0001;

/// bit 1: inter-channel delta — G=(G-R), B=(B-G), applied BEFORE spatial filter.
/// Kept for single-channel GrayA images; RGB/RGBA prefers YCoCg-R (bit 2).
pub const FLAG_INTER_CHANNEL_DELTA: u8 = 0b0000_0010;

/// bit 2: YCoCg-R lossless color transform (RGB/RGBA only).
/// Better decorrelation than simple ICD for natural photos.
///   Forward:  Co = R-B,  t = B + (Co>>1),  Cg = G-t,  Y = t + (Cg>>1)
///   Inverse:  t = Y-(Cg>>1), G = Cg+t, B = t-(Co>>1), R = B+Co
/// Planes stored in order: Y, Co, Cg [, A].
pub const FLAG_YCOCG_R:             u8 = 0b0000_0100;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterType {
    None     = 0,
    Sub      = 1,
    Up       = 2,
    Average  = 3,
    Paeth    = 4,
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

    pub fn effective(self) -> FilterType {
        match self {
            Self::Adaptive => Self::Paeth,
            other          => other,
        }
    }
}

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
    pub fn channel_count(&self)    -> usize { self.color_type.channel_count() }
    pub fn bytes_per_sample(&self) -> usize { (self.bit_depth / 8) as usize }
    pub fn plane_row_bytes(&self)  -> usize { self.width as usize * self.bytes_per_sample() }
    pub fn plane_bytes(&self)      -> usize { self.height as usize * self.plane_row_bytes() }

    pub fn byte_plane_split(&self) -> bool {
        self.bit_depth == 16 && (self.flags & FLAG_BYTE_PLANE_SPLIT) != 0
    }

    /// True when using simple G-R / B-G inter-channel delta (GrayA only now).
    pub fn inter_channel_delta(&self) -> bool {
        (self.flags & FLAG_INTER_CHANNEL_DELTA) != 0
    }

    /// True when using YCoCg-R lossless color transform (RGB/RGBA).
    pub fn use_ycocg(&self) -> bool {
        (self.flags & FLAG_YCOCG_R) != 0
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
        buf
    }

    pub fn parse(data: &[u8]) -> std::io::Result<Self> {
        if data.len() < HEADER_SIZE {
            return Err(e("MPX header too short"));
        }
        if data[0..4] != MAGIC   { return Err(e("not an MPX file")); }
        if data[4] != VERSION    { return Err(e(format!("unsupported version {}", data[4]))); }

        let color_type = ColorType::from_u8(data[5])
            .ok_or_else(|| e(format!("unknown color_type {}", data[5])))?;
        let bit_depth = data[6];
        if bit_depth != 8 && bit_depth != 16 {
            return Err(e(format!("unsupported bit_depth {}", bit_depth)));
        }
        let filter_type = FilterType::from_u8(data[7])
            .ok_or_else(|| e(format!("unknown filter_type {}", data[7])))?;

        let width  = u32::from_le_bytes(data[8..12].try_into().unwrap());
        let height = u32::from_le_bytes(data[12..16].try_into().unwrap());
        let flags  = data[17];

        if width == 0 || height == 0 { return Err(e("zero dimension")); }

        (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(color_type.channel_count()))
            .and_then(|n| n.checked_mul((bit_depth / 8) as usize))
            .ok_or_else(|| e("image dimensions overflow"))?;

        Ok(MpxHeader { color_type, bit_depth, filter_type, width, height, flags })
    }
}

#[inline]
fn e(msg: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.into())
}

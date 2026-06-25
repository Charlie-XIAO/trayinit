use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconError {
    EmptyDimensions,
    DimensionsOverflow,
    InvalidRgbaLength { expected: usize, actual: usize },
}

impl Icon {
    pub fn from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Result<Self, IconError> {
        validate_rgba(&rgba, width, height)?;
        Ok(Self {
            rgba,
            width,
            height,
        })
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

pub(crate) fn validate_rgba(rgba: &[u8], width: u32, height: u32) -> Result<(), IconError> {
    if width == 0 || height == 0 {
        return Err(IconError::EmptyDimensions);
    }

    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or(IconError::DimensionsOverflow)?;

    if rgba.len() != expected {
        return Err(IconError::InvalidRgbaLength {
            expected,
            actual: rgba.len(),
        });
    }

    Ok(())
}

impl fmt::Display for IconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDimensions => write!(f, "icon dimensions must be non-zero"),
            Self::DimensionsOverflow => write!(f, "icon dimensions overflow supported size"),
            Self::InvalidRgbaLength { expected, actual } => {
                write!(f, "icon RGBA data has length {actual}, expected {expected}")
            },
        }
    }
}

impl std::error::Error for IconError {}

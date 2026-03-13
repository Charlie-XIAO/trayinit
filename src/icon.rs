use crate::IconError;

/// An RGBA icon payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

impl Icon {
    pub fn from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Result<Self, IconError> {
        if width == 0 || height == 0 {
            return Err(IconError::ZeroDimensions);
        }

        let expected_len = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .map(|len| len as usize);

        if expected_len != Some(rgba.len()) {
            return Err(IconError::PixelCountMismatch {
                width,
                height,
                rgba_len: rgba.len(),
            });
        }

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

    pub fn into_rgba(self) -> Vec<u8> {
        self.rgba
    }
}

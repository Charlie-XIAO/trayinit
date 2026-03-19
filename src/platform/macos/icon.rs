use std::io::Cursor;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_app_kit::NSImage;
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSData, NSSize};

use crate::{Error, Icon};

pub fn to_png(icon: &Icon) -> Result<Vec<u8>, Error> {
    // Reference: muda/src/platform_impl/macos/icon.rs:25.
    let mut png = Vec::new();

    {
        let mut encoder =
            png::Encoder::new(Cursor::new(&mut png), icon.width() as _, icon.height() as _);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);

        let mut writer = encoder
            .write_header()
            .map_err(|error| Error::Backend(error.to_string()))?;
        writer
            .write_image_data(icon.rgba())
            .map_err(|error| Error::Backend(error.to_string()))?;
    }

    Ok(png)
}

pub fn to_nsimage(icon: &Icon, fixed_height: Option<f64>) -> Result<Retained<NSImage>, Error> {
    // Reference:
    // tray-icon/src/platform_impl/macos/mod.rs:266.
    // muda/src/platform_impl/macos/icon.rs:41.
    let png = to_png(icon)?;

    let (icon_width, icon_height) = match fixed_height {
        Some(fixed_height) => {
            let icon_height: CGFloat = fixed_height as CGFloat;
            let icon_width: CGFloat =
                (icon.width() as CGFloat) / (icon.height() as CGFloat / icon_height);
            (icon_width, icon_height)
        },
        None => (icon.width() as CGFloat, icon.height() as CGFloat),
    };

    let nsdata = NSData::with_bytes(&png);
    let nsimage = NSImage::initWithData(NSImage::alloc(), &nsdata)
        .ok_or_else(|| Error::Backend("failed to construct NSImage from tray icon data".into()))?;
    nsimage.setSize(NSSize::new(icon_width, icon_height));

    Ok(nsimage)
}

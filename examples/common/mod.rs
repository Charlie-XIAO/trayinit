use anyhow::{Ok, Result};
use image::ImageFormat;
use trayinit::Icon;

pub fn icon() -> Result<Icon> {
    let bytes = include_bytes!("icon.png");
    let img = image::load_from_memory_with_format(bytes, ImageFormat::Png)?.to_rgba8();
    let (width, height) = img.dimensions();
    let icon = Icon::from_rgba(img.into_raw(), width, height)?;
    Ok(icon)
}

use trayinit::Icon;

pub fn checker_icon() -> Result<Icon, trayinit::IconError> {
    Icon::from_rgba(checker_icon_rgba(32), 32, 32)
}

fn checker_icon_rgba(size: usize) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let light = ((x / 8) + (y / 8)) % 2 == 0;
            let (r, g, b) = if light {
                (0x26, 0xa6, 0x9a)
            } else {
                (0x24, 0x2a, 0x32)
            };
            rgba.extend_from_slice(&[r, g, b, 0xff]);
        }
    }
    rgba
}

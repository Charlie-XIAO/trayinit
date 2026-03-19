use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use png::{ColorType, Decoder, Transformations};
use trayinit::menu::{CheckItem, MenuItem, RadioGroup, RadioItem, StandardItem, Submenu};
use trayinit::{Icon, Tray, TrayEvent, TrayMethods, TrayStatus};

#[derive(Debug, Copy, Clone)]
enum Message {
    TrayWarm,
    TrayCool,
    TrayAsset,
    TrayNamed,
    ToggleOverlay,
    ToggleAttention,
    Quit,
}

#[derive(Debug, Copy, Clone)]
enum TrayIconKind {
    Warm,
    Cool,
    Asset,
    Named,
}

struct IconsTray {
    tray_icon: TrayIconKind,
    show_overlay: bool,
    needs_attention: bool,
    keep_running: Arc<AtomicBool>,
}

impl Tray for IconsTray {
    type Message = Message;

    fn id(&self) -> &str {
        "dev.trayinit.examples.icons"
    }

    fn icon(&self) -> Option<Icon> {
        match self.tray_icon {
            TrayIconKind::Warm => Some(make_tray_icon(0xD7, 0xB7, 0x4A)),
            TrayIconKind::Cool => Some(make_tray_icon(0x66, 0x8F, 0xD8)),
            TrayIconKind::Asset => Some(asset_icon()),
            TrayIconKind::Named => None,
        }
    }

    fn icon_name(&self) -> Option<String> {
        match self.tray_icon {
            TrayIconKind::Named => Some("folder".into()),
            _ => None,
        }
    }

    fn title(&self) -> Option<String> {
        Some(match self.tray_icon {
            TrayIconKind::Warm => "Icon demo: warm tray icon".into(),
            TrayIconKind::Cool => "Icon demo: cool tray icon".into(),
            TrayIconKind::Asset => "Icon demo: asset tray icon".into(),
            TrayIconKind::Named => "Icon demo: themed tray icon name".into(),
        })
    }

    fn tooltip(&self) -> Option<String> {
        Some("Demonstrates tray icons, menu icons, overlay icons, and attention icons.".into())
    }

    fn overlay_icon_name(&self) -> Option<String> {
        self.show_overlay.then(|| "emblem-ok".into())
    }

    fn overlay_icon(&self) -> Option<Icon> {
        self.show_overlay
            .then(|| make_overlay_icon(0x2D, 0xA5, 0x61))
    }

    fn attention_icon_name(&self) -> Option<String> {
        if self.needs_attention && matches!(self.tray_icon, TrayIconKind::Named) {
            Some("dialog-warning".into())
        } else {
            None
        }
    }

    fn attention_icon(&self) -> Option<Icon> {
        if self.needs_attention && !matches!(self.tray_icon, TrayIconKind::Named) {
            Some(make_tray_icon(0xD4, 0x55, 0x3D))
        } else {
            None
        }
    }

    fn status(&self) -> TrayStatus {
        if self.needs_attention {
            TrayStatus::Attention
        } else {
            TrayStatus::Active
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self::Message>> {
        let selected_tray_icon = match self.tray_icon {
            TrayIconKind::Warm => 0,
            TrayIconKind::Cool => 1,
            TrayIconKind::Asset => 2,
            TrayIconKind::Named => 3,
        };

        vec![
            RadioGroup::new(vec![
                RadioItem::new("Tray icon: Warm", Message::TrayWarm),
                RadioItem::new("Tray icon: Cool", Message::TrayCool),
                RadioItem::new("Tray icon: Asset PNG", Message::TrayAsset),
                RadioItem::new("Tray icon: Theme name (Linux)", Message::TrayNamed),
            ])
            .with_selected(selected_tray_icon)
            .into(),
            MenuItem::Separator,
            CheckItem::new(
                "Show overlay badge (Linux)",
                self.show_overlay,
                Message::ToggleOverlay,
            )
            .into(),
            CheckItem::new(
                "Request attention (Linux)",
                self.needs_attention,
                Message::ToggleAttention,
            )
            .into(),
            MenuItem::Separator,
            StandardItem::without_message("Menu icon: Warm")
                .with_icon(make_menu_icon(0xD7, 0xB7, 0x4A))
                .into(),
            StandardItem::without_message("Menu icon: Cool")
                .with_icon(make_menu_icon(0x66, 0x8F, 0xD8))
                .into(),
            StandardItem::without_message("Menu icon: Asset PNG")
                .with_icon(asset_icon())
                .into(),
            StandardItem::without_message("Menu icon: Theme name (Linux)")
                .with_icon_name("document-open")
                .into(),
            MenuItem::Submenu(
                Submenu::new(
                    "More icons",
                    vec![
                        StandardItem::new("Quit", Message::Quit)
                            .with_icon(make_menu_icon(0xA2, 0x3B, 0x3B))
                            .into(),
                    ],
                )
                .with_icon(make_menu_icon(0x4C, 0x56, 0x67)),
            ),
        ]
    }

    fn event(&mut self, event: TrayEvent<Self::Message>) {
        match event {
            TrayEvent::Menu(Message::TrayWarm) => {
                self.tray_icon = TrayIconKind::Warm;
                self.needs_attention = false;
            },
            TrayEvent::Menu(Message::TrayCool) => {
                self.tray_icon = TrayIconKind::Cool;
                self.needs_attention = false;
            },
            TrayEvent::Menu(Message::TrayAsset) => {
                self.tray_icon = TrayIconKind::Asset;
                self.needs_attention = false;
            },
            TrayEvent::Menu(Message::TrayNamed) => {
                self.tray_icon = TrayIconKind::Named;
                self.needs_attention = false;
            },
            TrayEvent::Menu(Message::ToggleOverlay) => {
                self.show_overlay = !self.show_overlay;
            },
            TrayEvent::Menu(Message::ToggleAttention) => {
                self.needs_attention = !self.needs_attention;
            },
            TrayEvent::Menu(Message::Quit) => {
                self.keep_running.store(false, Ordering::Relaxed);
            },
            TrayEvent::Interaction(_) | TrayEvent::Scroll(_) => {},
            _ => {},
        }
    }

    fn should_exit(&self) -> bool {
        !self.keep_running.load(Ordering::Relaxed)
    }
}

fn main() {
    tracing_subscriber::fmt::init();

    let tray = IconsTray {
        tray_icon: TrayIconKind::Asset,
        show_overlay: false,
        needs_attention: false,
        keep_running: Arc::new(AtomicBool::new(true)),
    };

    println!("Running icons tray example.");
    println!("Startup mode: run() standalone tray.");
    println!("Features in this example:");
    println!("- embedded PNG tray icon loading via include_bytes!(\"icon.png\")");
    println!("- generated tray icons");
    println!("- menu item icons");
    println!("- submenu icon");
    println!("- tray icon switching from menu actions");
    println!("- Linux-specific theme-name/overlay/attention properties remain host-dependent");

    tray.run().expect("run icons tray example");
}

fn make_tray_icon(r: u8, g: u8, b: u8) -> Icon {
    let width = 32usize;
    let height = 32usize;
    let mut rgba = vec![0; width * height * 4];

    for y in 0..height {
        for x in 0..width {
            let offset = (y * width + x) * 4;
            let border = x < 2 || y < 2 || x >= width - 2 || y >= height - 2;
            let highlight = x > 8 && x < 24 && y > 8 && y < 24;

            let (pr, pg, pb, pa) = if border {
                (0x18, 0x1A, 0x1F, 0xFF)
            } else if highlight {
                (0xF8, 0xFB, 0xFF, 0xFF)
            } else {
                (r, g, b, 0xFF)
            };

            rgba[offset] = pr;
            rgba[offset + 1] = pg;
            rgba[offset + 2] = pb;
            rgba[offset + 3] = pa;
        }
    }

    Icon::from_rgba(rgba, width as u32, height as u32).expect("valid generated tray icon")
}

fn make_menu_icon(r: u8, g: u8, b: u8) -> Icon {
    let width = 16usize;
    let height = 16usize;
    let mut rgba = vec![0; width * height * 4];

    for y in 0..height {
        for x in 0..width {
            let offset = (y * width + x) * 4;
            let border = x == 0 || y == 0 || x == width - 1 || y == height - 1;
            let inset = x > 3 && x < width - 4 && y > 3 && y < height - 4;

            let (pr, pg, pb, pa) = if border {
                (0x14, 0x16, 0x1B, 0xC0)
            } else if inset {
                (r, g, b, 0xFF)
            } else {
                (r / 2, g / 2, b / 2, 0xE8)
            };

            rgba[offset] = pr;
            rgba[offset + 1] = pg;
            rgba[offset + 2] = pb;
            rgba[offset + 3] = pa;
        }
    }

    Icon::from_rgba(rgba, width as u32, height as u32).expect("valid generated menu icon")
}

fn make_overlay_icon(r: u8, g: u8, b: u8) -> Icon {
    let width = 12usize;
    let height = 12usize;
    let mut rgba = vec![0; width * height * 4];

    for y in 0..height {
        for x in 0..width {
            let offset = (y * width + x) * 4;
            let dx = x as i32 - 5;
            let dy = y as i32 - 5;
            let radius_sq = dx * dx + dy * dy;

            let (pr, pg, pb, pa) = if radius_sq <= 20 {
                (r, g, b, 0xFF)
            } else if radius_sq <= 30 {
                (0x14, 0x16, 0x1B, 0xC0)
            } else {
                (0, 0, 0, 0)
            };

            rgba[offset] = pr;
            rgba[offset + 1] = pg;
            rgba[offset + 2] = pb;
            rgba[offset + 3] = pa;
        }
    }

    Icon::from_rgba(rgba, width as u32, height as u32).expect("valid generated overlay icon")
}

fn asset_icon() -> Icon {
    let png_bytes = include_bytes!("icon.png");
    let mut decoder = Decoder::new(Cursor::new(png_bytes));
    decoder.set_transformations(Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().expect("decode example icon.png");

    let mut rgba = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut rgba)
        .expect("read example icon.png frame");
    rgba.truncate(info.buffer_size());

    let rgba = match info.color_type {
        ColorType::Rgba => rgba,
        ColorType::Rgb => rgb_to_rgba(&rgba),
        ColorType::GrayscaleAlpha => grayscale_alpha_to_rgba(&rgba),
        ColorType::Grayscale => grayscale_to_rgba(&rgba),
        ColorType::Indexed => panic!("indexed PNG should have been expanded"),
    };

    Icon::from_rgba(rgba, info.width, info.height).expect("valid icon from example icon.png")
}

fn rgb_to_rgba(rgb: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(rgb.len() / 3 * 4);
    for chunk in rgb.chunks_exact(3) {
        rgba.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 0xFF]);
    }
    rgba
}

fn grayscale_alpha_to_rgba(data: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(data.len() / 2 * 4);
    for chunk in data.chunks_exact(2) {
        rgba.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
    }
    rgba
}

fn grayscale_to_rgba(data: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(data.len() * 4);
    for &value in data {
        rgba.extend_from_slice(&[value, value, value, 0xFF]);
    }
    rgba
}

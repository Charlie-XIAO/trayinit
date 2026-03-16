use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use png::{ColorType, Decoder, Transformations};
use trayinit::menu::{MenuItem, StandardItem, Submenu};
use trayinit::{Icon, Tray, TrayEvent, TrayMethods};

#[derive(Debug, Copy, Clone)]
enum Message {
    TrayWarm,
    TrayCool,
    TrayAsset,
    TrayNamed,
    MenuWarm,
    MenuCool,
    MenuAsset,
    MenuNamed,
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
        Some("Demonstrates tray icons and menu item icons.".into())
    }

    fn menu(&self) -> Vec<MenuItem<Self::Message>> {
        vec![
            StandardItem::new("Tray icon: Warm", Message::TrayWarm)
                .with_icon(make_menu_icon(0xD7, 0xB7, 0x4A))
                .into(),
            StandardItem::new("Tray icon: Cool", Message::TrayCool)
                .with_icon(make_menu_icon(0x66, 0x8F, 0xD8))
                .into(),
            StandardItem::new("Tray icon: Asset PNG", Message::TrayAsset)
                .with_icon(asset_icon())
                .into(),
            StandardItem::new("Tray icon: Theme name (Linux)", Message::TrayNamed)
                .with_icon_name("folder")
                .into(),
            MenuItem::Separator,
            StandardItem::new("Menu icon: Warm", Message::MenuWarm)
                .with_icon(make_menu_icon(0xD7, 0xB7, 0x4A))
                .into(),
            StandardItem::new("Menu icon: Cool", Message::MenuCool)
                .with_icon(make_menu_icon(0x66, 0x8F, 0xD8))
                .into(),
            StandardItem::new("Menu icon: Asset PNG", Message::MenuAsset)
                .with_icon(asset_icon())
                .into(),
            StandardItem::new("Menu icon: Theme name (Linux)", Message::MenuNamed)
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
            },
            TrayEvent::Menu(Message::TrayCool) => {
                self.tray_icon = TrayIconKind::Cool;
            },
            TrayEvent::Menu(Message::TrayAsset) => {
                self.tray_icon = TrayIconKind::Asset;
            },
            TrayEvent::Menu(Message::TrayNamed) => {
                self.tray_icon = TrayIconKind::Named;
            },
            TrayEvent::Menu(
                Message::MenuWarm | Message::MenuCool | Message::MenuAsset | Message::MenuNamed,
            ) => {},
            TrayEvent::Menu(Message::Quit) => {
                self.keep_running.store(false, Ordering::Relaxed);
            },
            TrayEvent::Interaction(_) | TrayEvent::Scroll(_) => {},
            _ => {},
        }
    }
}

fn main() {
    tracing_subscriber::fmt::init();

    let keep_running = Arc::new(AtomicBool::new(true));
    let tray = IconsTray {
        tray_icon: TrayIconKind::Asset,
        keep_running: Arc::clone(&keep_running),
    };
    let handle = tray.spawn().expect("spawn icons tray example");

    println!("Running icons tray example.");
    println!("Startup mode: spawn() self-hosted tray.");
    println!("Features in this example:");
    println!("- embedded PNG tray icon loading via include_bytes!(\"icon.png\")");
    println!("- generated tray icons");
    println!("- Linux theme icon names for tray and menu when supported by the host");
    println!("- menu item icons");
    println!("- submenu icon");
    println!("- tray icon switching from menu actions");

    while keep_running.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(250));
    }

    handle.shutdown().expect("shutdown icons tray example");
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

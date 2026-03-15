use std::io;

use trayinit::{Icon, InteractionEvent, ScrollEvent, Tray, TrayEvent, TrayMethods};

struct EventProbeTray;

impl Tray for EventProbeTray {
    type Message = Never;

    fn id(&self) -> &str {
        "dev.trayinit.examples.event-probe"
    }

    fn icon(&self) -> Option<Icon> {
        Some(make_icon())
    }

    fn tooltip(&self) -> Option<String> {
        Some("trayinit event probe".into())
    }

    fn event(&mut self, event: TrayEvent<Self::Message>) {
        match event {
            TrayEvent::Interaction(interaction) => {
                log_interaction(interaction);
            },
            TrayEvent::Scroll(scroll) => {
                log_scroll(scroll);
            },
            TrayEvent::Menu(message) => match message {},
            _ => {},
        }
    }
}

#[derive(Clone, Debug)]
enum Never {}

fn main() {
    let tray = EventProbeTray;
    let handle = tray.spawn().expect("spawn event probe example");

    println!("Running event probe example.");
    println!("This example intentionally has no tray menu.");
    println!("That keeps right click observable as InteractionKind::ContextMenu.");
    println!("Current Windows backend support to probe:");
    println!("- left click -> PrimaryActivate");
    println!("- middle click -> SecondaryActivate");
    println!("- right click -> ContextMenu");
    println!("- double-click shape exists in the API, but is not emitted yet");
    println!("- scroll shape exists in the API, but is not emitted on Windows yet");
    println!("Press Enter in this terminal to shut the tray down.");

    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);

    handle.shutdown().expect("shutdown event probe example");
}

fn log_interaction(interaction: InteractionEvent) {
    println!(
        "interaction: kind={:?} trigger={:?} position={:?} area={:?}",
        interaction.kind, interaction.trigger, interaction.position, interaction.area
    );
}

fn log_scroll(scroll: ScrollEvent) {
    println!(
        "scroll: delta={} axis={:?} position={:?} area={:?}",
        scroll.delta, scroll.axis, scroll.position, scroll.area
    );
}

fn make_icon() -> Icon {
    let width = 32usize;
    let height = 32usize;
    let mut rgba = vec![0; width * height * 4];

    for y in 0..height {
        for x in 0..width {
            let offset = (y * width + x) * 4;
            let border = x < 2 || y < 2 || x >= width - 2 || y >= height - 2;
            let cross = (x >= 14 && x <= 17) || (y >= 14 && y <= 17);

            let (r, g, b, a) = if border {
                (0x18, 0x1A, 0x1F, 0xFF)
            } else if cross {
                (0xF6, 0xF7, 0xFB, 0xFF)
            } else {
                (0x2D, 0x7A, 0xD6, 0xFF)
            };

            rgba[offset] = r;
            rgba[offset + 1] = g;
            rgba[offset + 2] = b;
            rgba[offset + 3] = a;
        }
    }

    Icon::from_rgba(rgba, width as u32, height as u32).expect("valid generated icon")
}

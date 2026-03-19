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
    tracing_subscriber::fmt::init();

    #[cfg(target_os = "macos")]
    {
        eprintln!("This example uses spawn(), which is not implemented on macOS yet.");
        eprintln!("Use the attach()-based examples like winit_window or winit_no_window instead.");
        return;
    }

    let tray = EventProbeTray;
    let handle = tray.spawn().expect("spawn event probe example");

    println!("Running event probe example.");
    println!("This example intentionally has no tray menu.");
    println!("That can keep right click observable as InteractionKind::ContextMenu,");
    println!("but actual click behavior is platform and host dependent.");
    println!("Useful things to probe:");
    println!("- primary activation");
    println!("- secondary activation");
    println!("- context-menu requests");
    println!("- scroll");
    println!("Notes:");
    println!("- Linux hosts may not emit single-click left/right interactions explicitly.");
    println!("- Linux tray geometry often has position only and no area.");
    println!("- Double-click is not part of the current core event API.");
    println!("Press Enter in this terminal to shut the tray down.");

    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);

    handle.shutdown().expect("shutdown event probe example");
}

fn log_interaction(interaction: InteractionEvent) {
    println!(
        "interaction: kind={:?} position={:?} area={:?}",
        interaction.kind, interaction.position, interaction.area
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

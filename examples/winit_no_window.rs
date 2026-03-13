#[cfg(windows)]
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(windows)]
use trayinit::{
    ActionItem, CheckItem, Handle, MenuItem, Tooltip, Tray, TrayEvent, TrayMethods, TrayView,
};
#[cfg(windows)]
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::WindowId,
};

#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum MenuId {
    ToggleTicks,
    Quit,
}

#[cfg(windows)]
struct WinitTray {
    ticking: bool,
    ticks: u32,
    keep_running: Arc<AtomicBool>,
}

#[cfg(windows)]
impl Tray for WinitTray {
    type MenuId = MenuId;

    fn id(&self) -> &str {
        "dev.trayinit.examples.winit_no_window"
    }

    fn view(&self) -> TrayView<Self::MenuId> {
        TrayView {
            title: Some(format!("winit host loop, ticks={}", self.ticks)),
            tooltip: Some(Tooltip::new(
                "trayinit + winit",
                format!("timer={}, ticks={}", on_off(self.ticking), self.ticks),
            )),
            menu: vec![
                CheckItem::new(MenuId::ToggleTicks, "Tick once per second", self.ticking).into(),
                MenuItem::Separator,
                ActionItem::new(MenuId::Quit, "Quit").into(),
            ],
            ..Default::default()
        }
    }

    fn event(&mut self, event: TrayEvent<Self::MenuId>) {
        match event {
            TrayEvent::Menu(MenuId::ToggleTicks) => {
                self.ticking = !self.ticking;
            }
            TrayEvent::Menu(MenuId::Quit) => {
                self.keep_running.store(false, Ordering::Relaxed);
            }
            TrayEvent::Activate(_) | TrayEvent::SecondaryActivate(_) | TrayEvent::Scroll(_) => {}
        }
    }
}

#[cfg(windows)]
struct App {
    tray: Option<Handle<WinitTray>>,
    keep_running: Arc<AtomicBool>,
    next_tick: Instant,
}

#[cfg(windows)]
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.tray.is_none() {
            let tray = WinitTray {
                ticking: true,
                ticks: 0,
                keep_running: Arc::clone(&self.keep_running),
            }
            .spawn()
            .expect("spawn winit tray example");
            self.tray = Some(tray);
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_tick));
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.keep_running.load(Ordering::Relaxed) {
            if let Some(tray) = self.tray.take() {
                let _ = tray.shutdown();
            }
            event_loop.exit();
            return;
        }

        let now = Instant::now();
        if now >= self.next_tick {
            if let Some(tray) = &self.tray {
                let _ = tray.update(|tray| {
                    if tray.ticking {
                        tray.ticks = tray.ticks.saturating_add(1);
                    }
                });
            }
            self.next_tick = now + Duration::from_secs(1);
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_tick));
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }
}

#[cfg(windows)]
fn main() {
    println!("Running winit no-window tray example.");
    println!("No window is created. winit only owns the application loop.");
    println!("Use the tray menu to toggle ticking or quit.");

    let event_loop = EventLoop::new().expect("create winit event loop");
    let mut app = App {
        tray: None,
        keep_running: Arc::new(AtomicBool::new(true)),
        next_tick: Instant::now() + Duration::from_secs(1),
    };

    event_loop.run_app(&mut app).expect("run winit app");
}

#[cfg(windows)]
fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This example currently requires Windows.");
}

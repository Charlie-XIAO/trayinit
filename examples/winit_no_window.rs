use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use trayinit::menu::{CheckItem, MenuItem, StandardItem};
use trayinit::{Handle, Tray, TrayEvent, TrayMethods};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

#[derive(Debug, Copy, Clone)]
enum Message {
    ToggleTicks,
    Quit,
}

struct WinitTray {
    ticking: bool,
    ticks: u32,
    keep_running: Arc<AtomicBool>,
}

impl Tray for WinitTray {
    type Message = Message;

    fn id(&self) -> &str {
        "dev.trayinit.examples.winit_no_window"
    }

    fn title(&self) -> Option<String> {
        Some(format!("winit host loop, ticks={}", self.ticks))
    }

    fn tooltip(&self) -> Option<String> {
        Some(format!(
            "trayinit + winit: timer={}, ticks={}",
            on_off(self.ticking),
            self.ticks
        ))
    }

    fn menu(&self) -> Vec<MenuItem<Self::Message>> {
        vec![
            CheckItem::new("Tick once per second", self.ticking, Message::ToggleTicks).into(),
            MenuItem::Separator,
            StandardItem::new("Quit", Message::Quit).into(),
        ]
    }

    fn event(&mut self, event: TrayEvent<Self::Message>) {
        match event {
            TrayEvent::Menu(Message::ToggleTicks) => {
                self.ticking = !self.ticking;
            },
            TrayEvent::Menu(Message::Quit) => {
                self.keep_running.store(false, Ordering::Relaxed);
            },
            TrayEvent::Activate(_) | TrayEvent::SecondaryActivate(_) | TrayEvent::Scroll(_) => {},
        }
    }
}

struct App {
    tray: Option<Handle<WinitTray>>,
    keep_running: Arc<AtomicBool>,
    ticker_started: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.tray.is_none() {
            let tray = WinitTray {
                ticking: true,
                ticks: 0,
                keep_running: Arc::clone(&self.keep_running),
            };
            let handle = tray.attach().expect("attach winit tray example");
            let ticker_handle = handle.clone();
            let ticker_running = Arc::clone(&self.keep_running);

            if !self.ticker_started {
                self.ticker_started = true;
                thread::spawn(move || {
                    while ticker_running.load(Ordering::Relaxed) {
                        thread::sleep(Duration::from_secs(1));

                        if !ticker_running.load(Ordering::Relaxed) {
                            break;
                        }

                        if ticker_handle
                            .update(|tray| {
                                if tray.ticking {
                                    tray.ticks = tray.ticks.saturating_add(1);
                                }
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                });
            }

            self.tray = Some(handle);
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(100),
        ));
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.keep_running.load(Ordering::Relaxed) {
            if let Some(tray) = self.tray.take() {
                let _ = tray.shutdown();
            }
            event_loop.exit();
            return;
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(100),
        ));
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }
}

fn main() {
    println!("Running winit no-window tray example.");
    println!("Startup mode: attach() host-integrated tray.");
    println!("No window is created. winit only owns the application loop.");
    println!("Use the tray menu to toggle ticking or quit.");

    let event_loop = EventLoop::new().expect("create winit event loop");
    let mut app = App {
        tray: None,
        keep_running: Arc::new(AtomicBool::new(true)),
        ticker_started: false,
    };

    event_loop.run_app(&mut app).expect("run winit app");
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use softbuffer::{Context, Surface};
use trayinit::menu::{Accelerator, CMD_OR_CTRL, CheckItem, Code, MenuItem, StandardItem};
use trayinit::{Handle, Tray, TrayEvent, TrayMethods};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle};
#[cfg(target_os = "windows")]
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowAttributes, WindowId};

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
        "dev.trayinit.examples.winit_window"
    }

    fn title(&self) -> Option<String> {
        Some(format!("winit window host, ticks={}", self.ticks))
    }

    fn tooltip(&self) -> Option<String> {
        Some(format!(
            "trayinit + winit window: timer={}, ticks={}",
            on_off(self.ticking),
            self.ticks
        ))
    }

    fn menu(&self) -> Vec<MenuItem<Self::Message>> {
        vec![
            CheckItem::new("Tick once per second", self.ticking, Message::ToggleTicks).into(),
            MenuItem::Separator,
            StandardItem::new("Quit", Message::Quit)
                .with_accelerator(Accelerator::new(Some(CMD_OR_CTRL), Code::KeyQ))
                .into(),
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
            TrayEvent::Interaction(_) | TrayEvent::Scroll(_) => {},
            _ => {},
        }
    }
}

struct App {
    tray: Option<Handle<WinitTray>>,
    window: Option<Rc<Window>>,
    surface: Option<Surface<OwnedDisplayHandle, Rc<Window>>>,
    context: Context<OwnedDisplayHandle>,
    hook_handle: Arc<Mutex<Option<Handle<WinitTray>>>>,
    keep_running: Arc<AtomicBool>,
    ticker_started: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let attributes =
                WindowAttributes::default().with_title("trayinit winit accelerator host");
            let window = event_loop
                .create_window(attributes)
                .expect("create winit host window");
            let window = Rc::new(window);
            let surface =
                Surface::new(&self.context, Rc::clone(&window)).expect("create softbuffer surface");
            window.request_redraw();
            self.surface = Some(surface);
            self.window = Some(window);
        }

        if self.tray.is_none() {
            let tray = WinitTray {
                ticking: true,
                ticks: 0,
                keep_running: Arc::clone(&self.keep_running),
            };
            let handle = tray.attach().expect("attach winit tray example");

            #[cfg(target_os = "windows")]
            {
                let window = self.window.as_ref().expect("host window available");
                let raw = window.window_handle().expect("window handle").as_raw();
                let RawWindowHandle::Win32(window_handle) = raw else {
                    panic!("expected Win32 window handle");
                };
                unsafe {
                    trayinit::windows::register_accelerator_window(
                        &handle,
                        window_handle.hwnd.get() as _,
                    )
                    .expect("register accelerator window");
                }
            }

            *self
                .hook_handle
                .lock()
                .expect("lock accelerator hook handle") = Some(handle.clone());

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
                #[cfg(target_os = "windows")]
                if let Some(window) = self.window.as_ref() {
                    let raw = window.window_handle().expect("window handle").as_raw();
                    let RawWindowHandle::Win32(window_handle) = raw else {
                        panic!("expected Win32 window handle");
                    };
                    unsafe {
                        let _ = trayinit::windows::unregister_accelerator_window(
                            &tray,
                            window_handle.hwnd.get() as _,
                        );
                    }
                }
                let _ = tray.shutdown();
            }

            *self
                .hook_handle
                .lock()
                .expect("lock accelerator hook handle") = None;
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
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.keep_running.store(false, Ordering::Relaxed);
            },
            WindowEvent::Resized(_) => {
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            },
            WindowEvent::RedrawRequested => {
                self.draw_window();
            },
            _ => {},
        }
    }
}

fn main() {
    tracing_subscriber::fmt::init();

    println!("Running winit window tray example.");
    println!("Startup mode: attach() host-integrated tray.");
    println!("A real winit window is created as a visible host window.");
    println!("On Windows, Ctrl+Q should activate the tray Quit item while the window is focused.");
    println!("On Linux, accelerators are exported as menu shortcut metadata only.");

    let hook_handle = Arc::new(Mutex::new(None::<Handle<WinitTray>>));
    let mut event_loop_builder = EventLoop::<()>::with_user_event();

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::MSG;
        use winit::platform::windows::EventLoopBuilderExtWindows;

        let hook_handle = Arc::clone(&hook_handle);
        event_loop_builder.with_msg_hook(move |msg| {
            let guard = hook_handle.lock().expect("lock accelerator hook handle");
            let Some(handle) = guard.as_ref() else {
                return false;
            };
            unsafe { trayinit::windows::process_message(handle, msg as *const MSG) }
        });
    }

    let event_loop = event_loop_builder.build().expect("create winit event loop");
    let context = Context::new(event_loop.owned_display_handle()).expect("create softbuffer");
    let mut app = App {
        tray: None,
        window: None,
        surface: None,
        context,
        hook_handle,
        keep_running: Arc::new(AtomicBool::new(true)),
        ticker_started: false,
    };

    event_loop.run_app(&mut app).expect("run winit app");
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

impl App {
    fn draw_window(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let Some(surface) = self.surface.as_mut() else {
            return;
        };

        let size = window.inner_size();
        let width = NonZeroU32::new(size.width.max(1)).expect("non-zero width");
        let height = NonZeroU32::new(size.height.max(1)).expect("non-zero height");

        window.pre_present_notify();

        surface
            .resize(width, height)
            .expect("resize softbuffer surface");

        let mut buffer = surface.buffer_mut().expect("acquire softbuffer buffer");
        buffer.fill(0xff181818);
        buffer.present().expect("present softbuffer frame");
    }
}

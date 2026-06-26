mod common;

use anyhow::Result;
use trayinit::{Tray, TrayEvent};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

const OPEN_ID: &str = "open";
const SYNC_ID: &str = "sync";
const QUIT_ID: &str = "quit";

#[derive(Debug)]
enum UserEvent {
    Tray(TrayEvent),
}

struct App {
    tray: Option<Tray>,
    window: Option<Window>,
    sync_enabled: bool,
    proxy: EventLoopProxy<UserEvent>,
}

impl App {
    fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            tray: None,
            window: None,
            sync_enabled: false,
            proxy,
        }
    }

    fn create_tray(&mut self) -> Result<()> {
        let tray = Tray::new(
            tray_state(self.sync_enabled)?,
            ProxySink::new(self.proxy.clone()),
        )?;
        self.tray = Some(tray);
        Ok(())
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = Window::default_attributes().with_title("trayinit winit example");
        match event_loop.create_window(attrs) {
            Ok(window) => self.window = Some(window),
            Err(err) => eprintln!("failed to create window: {err}"),
        }
    }

    fn update_tray(&self) -> Result<()> {
        if let Some(tray) = &self.tray {
            tray.set_state(tray_state(self.sync_enabled)?)?;
        }
        Ok(())
    }

    fn shutdown_tray(&mut self) {
        if let Some(tray) = self.tray.take()
            && let Err(err) = tray.shutdown()
        {
            eprintln!("failed to shut down tray: {err}");
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.create_window(event_loop);

        if self.tray.is_none()
            && let Err(err) = self.create_tray()
        {
            eprintln!("failed to create tray: {err}");
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        println!("{event:?}");
        match event {
            UserEvent::Tray(TrayEvent::MenuItemActivated { item_id })
                if item_id.as_str() == SYNC_ID =>
            {
                self.sync_enabled = !self.sync_enabled;
                if let Err(err) = self.update_tray() {
                    eprintln!("failed to update tray: {err}");
                }
            },
            UserEvent::Tray(TrayEvent::MenuItemActivated { item_id })
                if item_id.as_str() == QUIT_ID =>
            {
                self.shutdown_tray();
                event_loop.exit();
            },
            _ => {},
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::CloseRequested) {
            self.shutdown_tray();
            event_loop.exit();
        }
    }
}

#[derive(Clone)]
struct ProxySink {
    proxy: EventLoopProxy<UserEvent>,
}

impl ProxySink {
    fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self { proxy }
    }
}

impl trayinit::EventSink for ProxySink {
    fn send(&self, event: TrayEvent) {
        let _ = self.proxy.send_event(UserEvent::Tray(event));
    }
}

fn main() -> Result<()> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let mut app = App::new(proxy);
    event_loop.run_app(&mut app)?;
    Ok(())
}

fn tray_state(sync_enabled: bool) -> Result<trayinit::TrayState> {
    Ok(trayinit::TrayState::new()
        .with_title("trayinit")
        .with_icon(common::checker_icon()?)
        .with_tooltip("trayinit")
        .with_menu(trayinit::Menu::new([
            trayinit::MenuNode::item(OPEN_ID, "Open"),
            trayinit::MenuNode::check(SYNC_ID, "Sync enabled", sync_enabled),
            trayinit::MenuNode::separator(),
            trayinit::MenuNode::item(QUIT_ID, "Quit"),
        ])))
}

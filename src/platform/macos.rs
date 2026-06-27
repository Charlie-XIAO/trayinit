use std::cell::RefCell;
use std::io::Cursor;
use std::rc::Rc;
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, DeclaredClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSCellImagePosition, NSControlStateValueOff, NSControlStateValueOn, NSImage,
    NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{MainThreadMarker, NSData, NSObject, NSSize, NSString};

use crate::backend::{BackendCommand, BackendCommandSender, BackendRuntime};
use crate::{
    EventSink, Icon, Menu, MenuNode, TrayError, TrayEvent, TrayIconEventKind, TrayResult, TrayState,
};

pub(crate) fn spawn(
    initial_state: TrayState,
    sink: Arc<dyn EventSink>,
) -> TrayResult<BackendRuntime> {
    let mtm = MainThreadMarker::new().ok_or(TrayError::NotMainThread)?;
    // Standalone tray-only processes may not have initialized AppKit before
    // constructing the tray. Do this without changing app activation policy.
    let _ = NSApplication::sharedApplication(mtm);
    let backend = Rc::new(RefCell::new(MacosBackend::new(initial_state, sink, mtm)?));
    let dispatch_backend = backend.clone();

    let sender = BackendCommandSender::new(Rc::new(move |command| {
        let mtm = MainThreadMarker::new().ok_or(TrayError::NotMainThread)?;
        dispatch_backend.borrow_mut().handle_command(command, mtm)
    }));

    Ok(BackendRuntime::new(sender))
}

struct MacosBackend {
    state: TrayState,
    sink: Arc<dyn EventSink>,
    status_item: Option<Retained<NSStatusItem>>,
    button_target: Option<Retained<ButtonTarget>>,
    menu: Option<Retained<NSMenu>>,
}

impl MacosBackend {
    fn new(state: TrayState, sink: Arc<dyn EventSink>, mtm: MainThreadMarker) -> TrayResult<Self> {
        let mut backend = Self {
            state,
            sink,
            status_item: None,
            button_target: None,
            menu: None,
        };
        backend.rebuild(mtm)?;
        Ok(backend)
    }

    fn handle_command(&mut self, command: BackendCommand, mtm: MainThreadMarker) -> TrayResult<()> {
        match command {
            BackendCommand::SetState(state) => {
                self.state = state;
                self.rebuild(mtm)
            },
            BackendCommand::Close => {
                self.remove();
                Ok(())
            },
        }
    }

    fn rebuild(&mut self, mtm: MainThreadMarker) -> TrayResult<()> {
        if !self.state.visible {
            self.remove();
            return Ok(());
        }

        if self.status_item.is_none() {
            let status_item =
                NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
            self.status_item = Some(status_item);
        }

        let status_item = self.status_item.as_ref().expect("status item is present");
        set_status_icon(status_item, self.state.icon.as_ref(), mtm)?;
        set_status_title(status_item, self.state.title.as_deref(), mtm)?;
        set_status_tooltip(status_item, self.state.tooltip.as_deref(), mtm)?;
        self.rebuild_menu(mtm)?;

        Ok(())
    }

    fn rebuild_menu(&mut self, mtm: MainThreadMarker) -> TrayResult<()> {
        let Some(status_item) = &self.status_item else {
            self.menu = None;
            self.button_target = None;
            return Ok(());
        };

        if let Some(menu) = &self.state.menu {
            let ns_menu = build_menu(menu, self.sink.clone(), mtm)?;
            unsafe {
                ns_menu.setAutoenablesItems(false);
                status_item.setMenu(Some(&ns_menu));
                if let Some(button) = status_item.button(mtm) {
                    button.setTarget(None);
                    button.setAction(None);
                }
            }
            self.menu = Some(ns_menu);
            self.button_target = None;
        } else {
            status_item.setMenu(None);
            self.menu = None;

            let target = ButtonTarget::new(mtm, self.sink.clone());
            if let Some(button) = status_item.button(mtm) {
                unsafe {
                    button.setTarget(Some(&target));
                    button.setAction(Some(sel!(performTrayAction:)));
                }
            }
            self.button_target = Some(target);
        }

        Ok(())
    }

    fn remove(&mut self) {
        if let Some(status_item) = self.status_item.take() {
            NSStatusBar::systemStatusBar().removeStatusItem(&status_item);
        }
        self.button_target = None;
        self.menu = None;
    }
}

fn set_status_icon(
    status_item: &NSStatusItem,
    icon: Option<&Icon>,
    mtm: MainThreadMarker,
) -> TrayResult<()> {
    let Some(button) = status_item.button(mtm) else {
        return Ok(());
    };

    if let Some(icon) = icon {
        let image = ns_image_from_icon(icon)?;
        let height = 18.0;
        let width = icon.width() as f64 / (icon.height() as f64 / height);
        image.setSize(NSSize::new(width, height));
        button.setImage(Some(&image));
        button.setImagePosition(NSCellImagePosition::ImageLeft);
    } else {
        button.setImage(None);
    }

    Ok(())
}

fn set_status_title(
    status_item: &NSStatusItem,
    title: Option<&str>,
    mtm: MainThreadMarker,
) -> TrayResult<()> {
    let Some(button) = status_item.button(mtm) else {
        return Ok(());
    };
    button.setTitle(&NSString::from_str(title.unwrap_or_default()));
    Ok(())
}

fn set_status_tooltip(
    status_item: &NSStatusItem,
    tooltip: Option<&str>,
    mtm: MainThreadMarker,
) -> TrayResult<()> {
    let Some(button) = status_item.button(mtm) else {
        return Ok(());
    };
    let tooltip = tooltip.map(NSString::from_str);
    button.setToolTip(tooltip.as_deref());
    Ok(())
}

fn build_menu(
    menu: &Menu,
    sink: Arc<dyn EventSink>,
    mtm: MainThreadMarker,
) -> TrayResult<Retained<NSMenu>> {
    let ns_menu = NSMenu::new(mtm);
    ns_menu.setAutoenablesItems(false);

    for node in menu.nodes() {
        let item = build_menu_node(node, sink.clone(), mtm)?;
        ns_menu.addItem(&item);
    }

    Ok(ns_menu)
}

fn build_menu_node(
    node: &MenuNode,
    sink: Arc<dyn EventSink>,
    mtm: MainThreadMarker,
) -> TrayResult<Retained<NSMenuItem>> {
    match node {
        MenuNode::Item(item) => {
            let ns_item = TrayMenuItem::new(mtm, &item.label, item.id.clone(), sink);
            ns_item.setEnabled(item.enabled);
            Ok(Retained::into_super(ns_item))
        },
        MenuNode::Check(item) => {
            let ns_item = TrayMenuItem::new(mtm, &item.label, item.id.clone(), sink);
            ns_item.setEnabled(item.enabled);
            ns_item.setState(if item.checked {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            Ok(Retained::into_super(ns_item))
        },
        MenuNode::Submenu(submenu) => {
            let title = NSString::from_str(&submenu.label);
            let ns_item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    mtm.alloc(),
                    &title,
                    None,
                    &NSString::new(),
                )
            };
            let ns_submenu = NSMenu::new(mtm);
            ns_submenu.setTitle(&title);
            ns_submenu.setAutoenablesItems(false);
            ns_item.setEnabled(submenu.enabled);
            for child in &submenu.children {
                let child = build_menu_node(child, sink.clone(), mtm)?;
                ns_submenu.addItem(&child);
            }
            ns_item.setSubmenu(Some(&ns_submenu));
            Ok(ns_item)
        },
        MenuNode::Separator => Ok(NSMenuItem::separatorItem(mtm)),
    }
}

fn ns_image_from_icon(icon: &Icon) -> TrayResult<Retained<NSImage>> {
    let mut png_data = Vec::new();
    {
        let mut encoder =
            png::Encoder::new(Cursor::new(&mut png_data), icon.width(), icon.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|err| TrayError::BackendUnavailable(err.to_string()))?;
        writer
            .write_image_data(icon.rgba())
            .map_err(|err| TrayError::BackendUnavailable(err.to_string()))?;
    }

    let data = NSData::from_vec(png_data);
    let image = NSImage::initWithData(NSImage::alloc(), &data)
        .ok_or_else(|| TrayError::BackendUnavailable("failed to create NSImage".into()))?;
    Ok(image)
}

struct ButtonTargetIvars {
    sink: Arc<dyn EventSink>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TrayinitButtonTarget"]
    #[thread_kind = MainThreadOnly]
    #[ivars = ButtonTargetIvars]
    struct ButtonTarget;

    impl ButtonTarget {
        #[unsafe(method(performTrayAction:))]
        fn perform_tray_action(&self, _sender: Option<&AnyObject>) {
            self.ivars().sink.send(TrayEvent::IconActivated {
                kind: TrayIconEventKind::PrimaryClick,
                position: None,
                rect: None,
            });
        }
    }
);

impl ButtonTarget {
    fn new(mtm: MainThreadMarker, sink: Arc<dyn EventSink>) -> Retained<Self> {
        let target = mtm.alloc().set_ivars(ButtonTargetIvars { sink });
        unsafe { msg_send![super(target), init] }
    }
}

struct TrayMenuItemIvars {
    item_id: crate::MenuItemId,
    sink: Arc<dyn EventSink>,
}

define_class!(
    #[unsafe(super(NSMenuItem))]
    #[name = "TrayinitMenuItem"]
    #[thread_kind = MainThreadOnly]
    #[ivars = TrayMenuItemIvars]
    struct TrayMenuItem;

    impl TrayMenuItem {
        #[unsafe(method(performTrayAction:))]
        fn perform_tray_action(&self, _sender: Option<&AnyObject>) {
            self.ivars().sink.send(TrayEvent::MenuItemActivated {
                item_id: self.ivars().item_id.clone(),
            });
        }
    }
);

impl TrayMenuItem {
    fn new(
        mtm: MainThreadMarker,
        label: &str,
        item_id: crate::MenuItemId,
        sink: Arc<dyn EventSink>,
    ) -> Retained<Self> {
        let title = NSString::from_str(label);
        let key_equivalent = NSString::new();
        let target = mtm.alloc().set_ivars(TrayMenuItemIvars { item_id, sink });
        let item: Retained<Self> = unsafe {
            msg_send![
                super(target),
                initWithTitle: &*title,
                action: Some(sel!(performTrayAction:)),
                keyEquivalent: &*key_equivalent
            ]
        };
        unsafe {
            item.setTarget(Some(&item));
        }
        item
    }
}

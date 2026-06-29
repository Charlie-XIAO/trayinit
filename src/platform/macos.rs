use std::cell::RefCell;
use std::io::Cursor;
use std::rc::Rc;
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, DeclaredClass, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSCellImagePosition, NSControlStateValueOff, NSControlStateValueOn, NSEvent,
    NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength, NSView,
};
use objc2_foundation::{MainThreadMarker, NSData, NSSize, NSString};

use crate::backend::{BackendCommand, BackendCommandSender, BackendRuntime};
use crate::{
    ActivationMode, EventSink, Icon, Menu, MenuNode, TrayError, TrayEvent, TrayIconEventKind,
    TrayId, TrayResult, TrayState,
};

#[derive(Debug, Default)]
pub struct PlatformOptions;

pub(crate) fn spawn(
    initial_state: TrayState,
    sink: Arc<dyn EventSink>,
    _options: PlatformOptions,
    tray_id: TrayId,
) -> TrayResult<BackendRuntime> {
    let mtm = MainThreadMarker::new().ok_or(TrayError::NotMainThread)?;
    // Standalone tray-only processes may not have initialized AppKit before
    // constructing the tray. Do this without changing app activation policy.
    let _ = NSApplication::sharedApplication(mtm);
    let backend = Rc::new(RefCell::new(Backend::new(
        initial_state,
        sink,
        tray_id,
        mtm,
    )?));
    let dispatch_backend = backend.clone();

    let sender = BackendCommandSender::new(Rc::new(move |command| {
        let mtm = MainThreadMarker::new().ok_or(TrayError::NotMainThread)?;
        dispatch_backend.borrow_mut().handle_command(command, mtm)
    }));

    Ok(BackendRuntime::new(sender))
}

struct Backend {
    state: TrayState,
    sink: Arc<dyn EventSink>,
    tray_id: TrayId,
    status_item: Option<Retained<NSStatusItem>>,
    tray_target: Option<Retained<TrayTarget>>,
    menu: Option<Retained<NSMenu>>,
}

impl Backend {
    fn new(
        state: TrayState,
        sink: Arc<dyn EventSink>,
        tray_id: TrayId,
        mtm: MainThreadMarker,
    ) -> TrayResult<Self> {
        let mut backend = Self {
            state,
            sink,
            tray_id,
            status_item: None,
            tray_target: None,
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
        self.sync_tray_target(mtm);

        Ok(())
    }

    fn rebuild_menu(&mut self, mtm: MainThreadMarker) -> TrayResult<()> {
        let Some(status_item) = &self.status_item else {
            self.menu = None;
            self.tray_target = None;
            return Ok(());
        };

        if let Some(menu) = &self.state.menu {
            let ns_menu = build_menu(menu, self.sink.clone(), self.tray_id.clone(), mtm)?;
            ns_menu.setAutoenablesItems(false);
            status_item.setMenu(Some(&ns_menu));
            self.menu = Some(ns_menu);
        } else {
            status_item.setMenu(None);
            self.menu = None;
        }

        Ok(())
    }

    fn sync_tray_target(&mut self, mtm: MainThreadMarker) {
        let Some(status_item) = &self.status_item else {
            self.tray_target = None;
            return;
        };
        let Some(button) = status_item.button(mtm) else {
            return;
        };

        let target = match &self.tray_target {
            Some(target) => target.clone(),
            None => {
                let target = TrayTarget::new(
                    mtm,
                    status_item,
                    self.sink.clone(),
                    self.tray_id.clone(),
                    self.state.activation_mode,
                );
                button.addSubview(&target);
                self.tray_target = Some(target.clone());
                target
            },
        };

        target.update(
            status_item,
            self.menu.as_ref(),
            self.state.activation_mode,
            mtm,
        );
    }

    fn remove(&mut self) {
        if let Some(tray_target) = self.tray_target.take() {
            tray_target.removeFromSuperview();
        }
        if let Some(status_item) = self.status_item.take() {
            NSStatusBar::systemStatusBar().removeStatusItem(&status_item);
        }
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
    tray_id: TrayId,
    mtm: MainThreadMarker,
) -> TrayResult<Retained<NSMenu>> {
    let ns_menu = NSMenu::new(mtm);
    ns_menu.setAutoenablesItems(false);

    for node in menu.nodes() {
        let item = build_menu_node(node, sink.clone(), tray_id.clone(), mtm)?;
        ns_menu.addItem(&item);
    }

    Ok(ns_menu)
}

fn build_menu_node(
    node: &MenuNode,
    sink: Arc<dyn EventSink>,
    tray_id: TrayId,
    mtm: MainThreadMarker,
) -> TrayResult<Retained<NSMenuItem>> {
    match node {
        MenuNode::Item(item) => {
            let ns_item = TrayMenuItem::new(mtm, &item.label, tray_id, item.id.clone(), sink);
            ns_item.setEnabled(item.enabled);
            Ok(Retained::into_super(ns_item))
        },
        MenuNode::Check(item) => {
            let ns_item = TrayMenuItem::new(mtm, &item.label, tray_id, item.id.clone(), sink);
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
                let child = build_menu_node(child, sink.clone(), tray_id.clone(), mtm)?;
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

struct TrayTargetIvars {
    sink: Arc<dyn EventSink>,
    tray_id: TrayId,
    status_item: Retained<NSStatusItem>,
    menu: RefCell<Option<Retained<NSMenu>>>,
    activation_mode: std::cell::Cell<ActivationMode>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "TrayinitTargetView"]
    #[thread_kind = MainThreadOnly]
    #[ivars = TrayTargetIvars]
    struct TrayTarget;

    impl TrayTarget {
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.handle_click(TrayIconEventKind::PrimaryClick);
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, _event: &NSEvent) {
            self.handle_click(TrayIconEventKind::SecondaryClick);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            self.set_button_highlight(false);
        }

        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, _event: &NSEvent) {
            self.set_button_highlight(false);
        }
    }
);

impl TrayTarget {
    fn new(
        mtm: MainThreadMarker,
        status_item: &Retained<NSStatusItem>,
        sink: Arc<dyn EventSink>,
        tray_id: TrayId,
        activation_mode: ActivationMode,
    ) -> Retained<Self> {
        let frame = status_item
            .button(mtm)
            .map(|button| button.frame())
            .unwrap_or_default();
        let target = mtm.alloc().set_ivars(TrayTargetIvars {
            sink,
            tray_id,
            status_item: status_item.retain(),
            menu: RefCell::new(None),
            activation_mode: std::cell::Cell::new(activation_mode),
        });
        let target: Retained<Self> = unsafe { msg_send![super(target), initWithFrame: frame] };
        target.setWantsLayer(true);
        target
    }

    fn update(
        &self,
        status_item: &NSStatusItem,
        menu: Option<&Retained<NSMenu>>,
        activation_mode: ActivationMode,
        mtm: MainThreadMarker,
    ) {
        *self.ivars().menu.borrow_mut() = menu.map(|menu| menu.retain());
        self.ivars().activation_mode.set(activation_mode);
        if let Some(button) = status_item.button(mtm) {
            self.setFrame(button.frame());
        }
    }

    fn handle_click(&self, kind: TrayIconEventKind) {
        self.ivars().sink.send(TrayEvent::IconActivated {
            tray_id: self.ivars().tray_id.clone(),
            kind,
            position: None,
            rect: None,
        });

        if should_open_menu(self.ivars().activation_mode.get(), kind)
            && self.ivars().menu.borrow().is_some()
        {
            self.perform_status_item_click();
        } else {
            self.set_button_highlight(true);
        }
    }

    fn perform_status_item_click(&self) {
        let mtm = MainThreadMarker::from(self);
        if let Some(button) = self.ivars().status_item.button(mtm) {
            unsafe {
                button.performClick(None);
            }
        }
    }

    fn set_button_highlight(&self, highlighted: bool) {
        let mtm = MainThreadMarker::from(self);
        if let Some(button) = self.ivars().status_item.button(mtm) {
            button.highlight(highlighted);
        }
    }
}

fn should_open_menu(activation_mode: ActivationMode, kind: TrayIconEventKind) -> bool {
    match activation_mode {
        ActivationMode::PlatformDefault | ActivationMode::MenuOnPrimaryClick => {
            kind == TrayIconEventKind::PrimaryClick
        },
        ActivationMode::MenuOnSecondaryClick => kind == TrayIconEventKind::SecondaryClick,
    }
}

struct TrayMenuItemIvars {
    tray_id: TrayId,
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
                tray_id: self.ivars().tray_id.clone(),
                item_id: self.ivars().item_id.clone(),
            });
        }
    }
);

impl TrayMenuItem {
    fn new(
        mtm: MainThreadMarker,
        label: &str,
        tray_id: TrayId,
        item_id: crate::MenuItemId,
        sink: Arc<dyn EventSink>,
    ) -> Retained<Self> {
        let title = NSString::from_str(label);
        let key_equivalent = NSString::new();
        let target = mtm.alloc().set_ivars(TrayMenuItemIvars {
            tray_id,
            item_id,
            sink,
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_default_opens_menu_on_primary_click() {
        assert!(should_open_menu(
            ActivationMode::PlatformDefault,
            TrayIconEventKind::PrimaryClick
        ));
        assert!(!should_open_menu(
            ActivationMode::PlatformDefault,
            TrayIconEventKind::SecondaryClick
        ));
    }

    #[test]
    fn menu_on_primary_click_opens_menu_on_primary_click_only() {
        assert!(should_open_menu(
            ActivationMode::MenuOnPrimaryClick,
            TrayIconEventKind::PrimaryClick
        ));
        assert!(!should_open_menu(
            ActivationMode::MenuOnPrimaryClick,
            TrayIconEventKind::SecondaryClick
        ));
    }

    #[test]
    fn menu_on_secondary_click_opens_menu_on_secondary_click_only() {
        assert!(!should_open_menu(
            ActivationMode::MenuOnSecondaryClick,
            TrayIconEventKind::PrimaryClick
        ));
        assert!(should_open_menu(
            ActivationMode::MenuOnSecondaryClick,
            TrayIconEventKind::SecondaryClick
        ));
    }
}

use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::mpsc;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

pub enum Action {
    Quit,
}

pub struct Tray {
    _icon: TrayIcon,
    events: mpsc::Receiver<Action>,
}

impl Tray {
    pub fn new(ctx: &egui::Context, window: &eframe::Frame) -> Result<Self, String> {
        let RawWindowHandle::Win32(handle) = window
            .window_handle()
            .map_err(|err| err.to_string())?
            .as_raw()
        else {
            return Err("Не найдено окно Windows.".into());
        };
        let hwnd = handle.hwnd.get();
        let tree = resvg::usvg::Tree::from_data(
            include_bytes!("../assets/flow-22.svg"),
            &Default::default(),
        )
        .map_err(|e| e.to_string())?;
        let mut image = resvg::tiny_skia::Pixmap::new(32, 32).ok_or("Invalid tray icon")?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(
                32.0 / tree.size().width(),
                32.0 / tree.size().height(),
            ),
            &mut image.as_mut(),
        );
        let icon = tray_icon::Icon::from_rgba(image.take(), 32, 32).map_err(|e| e.to_string())?;
        let menu = Menu::new();
        let show = MenuItem::new("Открыть SSH Tunnel", true, None);
        let quit = MenuItem::new("Выйти", true, None);
        menu.append_items(&[&show, &quit])
            .map_err(|e| e.to_string())?;
        let icon = TrayIconBuilder::new()
            .with_tooltip("SSH Tunnel")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .map_err(|e| e.to_string())?;
        let (events, rx) = mpsc::channel();
        let ctx_menu = ctx.clone();
        let menu_events = events.clone();
        let show_id = show.id().clone();
        let quit_id = quit.id().clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == quit_id {
                let _ = menu_events.send(Action::Quit);
            } else if event.id != show_id {
                return;
            }
            restore(hwnd);
            ctx_menu.request_repaint();
        }));
        let ctx = ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if matches!(
                event,
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                restore(hwnd);
                ctx.request_repaint();
            }
        }));
        Ok(Self {
            _icon: icon,
            events: rx,
        })
    }

    pub fn next(&self) -> Option<Action> {
        self.events.try_recv().ok()
    }
}

pub(crate) fn restore(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsIconic, SetForegroundWindow, ShowWindowAsync, SW_RESTORE, SW_SHOW,
    };
    let hwnd = hwnd as _;
    unsafe {
        let command = if IsIconic(hwnd) != 0 {
            SW_RESTORE
        } else {
            SW_SHOW
        };
        ShowWindowAsync(hwnd, command);
        SetForegroundWindow(hwnd);
    }
}

pub(crate) fn hide(window: &eframe::Frame) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    if let Ok(handle) = window.window_handle() {
        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
            unsafe { ShowWindow(handle.hwnd.get() as _, SW_HIDE) };
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        TrayIconEvent::set_event_handler(None::<fn(TrayIconEvent)>);
        MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
    }
}

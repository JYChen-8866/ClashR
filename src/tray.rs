use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, MenuId},
    TrayIcon, TrayIconBuilder,
};

pub struct SystemTray {
    _tray: TrayIcon,
    show_id: MenuId,
    quit_id: MenuId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    Show,
    Quit,
}

impl SystemTray {
    pub fn new() -> anyhow::Result<Self> {
        let tray_menu = Menu::new();

        let show_item = MenuItem::new("Show Window", true, None);
        let quit_item = MenuItem::new("Quit", true, None);

        let show_id = show_item.id().clone();
        let quit_id = quit_item.id().clone();

        tray_menu.append(&show_item)?;
        tray_menu.append(&quit_item)?;

        // Load and resize icon
        let icon_bytes = include_bytes!("../icons/app/app-icon-256.png");
        let icon_image = image::load_from_memory(icon_bytes)?;
        let icon_rgba = icon_image.resize_exact(32, 32, image::imageops::FilterType::Lanczos3).to_rgba8();
        let (width, height) = (icon_rgba.width(), icon_rgba.height());
        let rgba_data = icon_rgba.into_raw();
        let icon = tray_icon::Icon::from_rgba(rgba_data, width, height)?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip("ClashR")
            .with_icon(icon)
            .build()?;

        Ok(Self {
            _tray: tray,
            show_id,
            quit_id,
        })
    }

    /// Poll for tray menu events. Returns None if no event.
    pub fn poll_event(&self) -> Option<TrayEvent> {
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.show_id {
                return Some(TrayEvent::Show);
            } else if event.id == self.quit_id {
                return Some(TrayEvent::Quit);
            }
        }
        None
    }
}

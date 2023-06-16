#[cfg(feature = "wayland")]
use crate::backend::wayland;
#[cfg(feature = "x11")]
use crate::backend::x11;
use crate::HotKey;

pub enum Menu {
    #[cfg(feature = "x11")]
    X11(x11::menu::Menu),
    #[cfg(feature = "wayland")]
    Wayland(wayland::menu::Menu),
}

impl Menu {
    pub fn new() -> Self {
        let app = crate::Application::try_global().unwrap();
        match &app.backend_app {
            #[cfg(feature = "x11")]
            super::application::Application::X11(_) => Self::X11(x11::menu::Menu::new()),
            #[cfg(feature = "wayland")]
            super::application::Application::Wayland(_) => {
                Self::Wayland(wayland::menu::Menu::new())
            }
        }
    }

    pub fn new_for_popup() -> Menu {
        let app = crate::Application::try_global().unwrap();
        match &app.backend_app {
            #[cfg(feature = "x11")]
            super::application::Application::X11(_) => Self::X11(x11::menu::Menu::new_for_popup()),
            #[cfg(feature = "wayland")]
            super::application::Application::Wayland(_) => {
                Self::Wayland(wayland::menu::Menu::new_for_popup())
            }
        }
    }

    pub fn add_dropdown(&mut self, menu: Menu, text: &str, enabled: bool) {
        match self {
            #[cfg(feature = "x11")]
            Menu::X11(m) => {
                match menu {
                    Menu::X11(menu) => {
                        m.add_dropdown(menu, text, enabled);
                    }
                    #[cfg(feature = "wayland")]
                    Menu::Wayland(_) => {}
                };
            }
            #[cfg(feature = "wayland")]
            Menu::Wayland(m) => {
                match menu {
                    #[cfg(feature = "x11")]
                    Menu::X11(_) => {}
                    Menu::Wayland(menu) => {
                        m.add_dropdown(menu, text, enabled);
                    }
                };
            }
        }
    }

    pub fn add_item(
        &mut self,
        id: u32,
        text: &str,
        key: Option<&HotKey>,
        selected: Option<bool>,
        enabled: bool,
    ) {
        match self {
            #[cfg(feature = "x11")]
            Menu::X11(menu) => {
                menu.add_item(id, text, key, selected, enabled);
            }
            #[cfg(feature = "wayland")]
            Menu::Wayland(menu) => {
                menu.add_item(id, text, key, selected, enabled);
            }
        }
    }

    pub fn add_separator(&mut self) {
        match self {
            #[cfg(feature = "x11")]
            Menu::X11(menu) => {
                menu.add_separator();
            }
            #[cfg(feature = "wayland")]
            Menu::Wayland(menu) => {
                menu.add_separator();
            }
        }
    }
}

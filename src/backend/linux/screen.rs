#[cfg(feature = "wayland")]
use crate::backend::wayland;
#[cfg(feature = "x11")]
use crate::backend::x11;
use crate::Monitor;

pub fn get_monitors() -> Vec<Monitor> {
    // TODO: Is there any reason to not just have this be a method on Application?
    let app = crate::Application::try_global().expect("Cannot get monitors without an app on X11");
    match &app.backend_app {
        super::application::Application::X11(app) => x11::screen::get_monitors(app),
        super::application::Application::Wayland(_) => wayland::screen::get_monitors(),
    }
}

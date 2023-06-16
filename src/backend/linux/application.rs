#[cfg(feature = "wayland")]
use crate::backend::wayland;
#[cfg(feature = "x11")]
use crate::backend::x11;
use crate::AppHandler;

use super::clipboard::Clipboard;

#[derive(Clone)]
pub(crate) enum Application {
    #[cfg(feature = "x11")]
    X11(x11::application::Application),
    #[cfg(feature = "wayland")]
    Wayland(wayland::application::Application),
}

impl Application {
    pub fn new() -> Result<Self, anyhow::Error> {
        #[cfg(feature = "wayland")]
        if let Ok(app) = wayland::application::Application::new() {
            return Ok(Application::Wayland(app));
        }

        #[cfg(feature = "x11")]
        if let Ok(app) = x11::application::Application::new() {
            return Ok(Application::X11(app));
        }

        Err(anyhow::anyhow!("can't create application"))
    }

    pub fn quit(&self) {
        match self {
            #[cfg(feature = "x11")]
            Application::X11(app) => {
                app.quit();
            }
            #[cfg(feature = "wayland")]
            Application::Wayland(app) => {
                app.quit();
            }
        }
    }

    pub fn clipboard(&self) -> Clipboard {
        match self {
            #[cfg(feature = "x11")]
            Application::X11(app) => Clipboard::X11(app.clipboard()),
            #[cfg(feature = "wayland")]
            Application::Wayland(app) => Clipboard::Wayland(app.clipboard()),
        }
    }

    pub fn get_locale() -> String {
        let app = crate::Application::try_global().unwrap();
        match &app.backend_app {
            #[cfg(feature = "x11")]
            Application::X11(_app) => x11::application::Application::get_locale(),
            #[cfg(feature = "wayland")]
            Application::Wayland(_app) => wayland::application::Application::get_locale(),
        }
    }

    pub fn run(self, handler: Option<Box<dyn AppHandler>>) {
        match self {
            #[cfg(feature = "x11")]
            Application::X11(app) => {
                app.run(handler);
            }
            #[cfg(feature = "wayland")]
            Application::Wayland(app) => {
                app.run(handler);
            }
        }
    }
    pub fn get_handle(&self) -> Option<AppHandle> {
        match self {
            #[cfg(feature = "x11")]
            Application::X11(app) => app.get_handle().map(AppHandle::X11),
            #[cfg(feature = "wayland")]
            Application::Wayland(app) => app.get_handle().map(AppHandle::Wayland),
        }
    }
}

#[derive(Clone)]
pub(crate) enum AppHandle {
    #[cfg(feature = "x11")]
    X11(x11::application::AppHandle),
    #[cfg(feature = "wayland")]
    Wayland(wayland::application::AppHandle),
}

impl AppHandle {
    pub fn run_on_main<F>(&self, callback: F)
    where
        F: FnOnce(Option<&mut dyn AppHandler>) + Send + 'static,
    {
        match self {
            #[cfg(feature = "x11")]
            AppHandle::X11(app) => app.run_on_main(callback),
            #[cfg(feature = "wayland")]
            AppHandle::Wayland(app) => app.run_on_main(callback),
        }
    }
}

impl crate::platform::linux::ApplicationExt for crate::Application {
    fn primary_clipboard(&self) -> crate::Clipboard {
        match &self.backend_app {
            #[cfg(feature = "x11")]
            Application::X11(it) => crate::Clipboard(Clipboard::X11(it.primary.clone())),
            #[cfg(feature = "wayland")]
            Application::Wayland(_) => unimplemented!(),
        }
    }
}

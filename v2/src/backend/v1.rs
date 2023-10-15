use std::collections::HashMap;

use crate::{WindowBuilder, WindowId};

pub struct Glazier {
    app: glazier::Application,
    windows: HashMap<WindowId, glazier::WindowHandle>,
}

impl Glazier {
    pub(crate) fn stop(&self) {
        self.app.quit()
    }
    pub(crate) fn new_window(&mut self, builder: WindowBuilder) -> WindowId {
        let bld = glazier::WindowBuilder::new(self.app.clone());
        let bld = bld.title(builder.title);
        let bld = bld.resizable(builder.resizable);
        let bld = bld.show_titlebar(builder.show_titlebar);
        let bld = bld.transparent(builder.transparent);
        let window = bld.build().unwrap();
        let id = WindowId::next();
        self.windows.insert(id, window);
        id
    }
}

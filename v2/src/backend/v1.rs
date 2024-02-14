use std::{cell::RefCell, collections::HashMap, marker::PhantomData, rc::Rc};

use glazier::{AppHandler, Application, Error, WinHandler};

use crate::{Glazier, PlatformHandler, WindowBuilder, WindowId};

pub fn launch(
    handler: Box<dyn PlatformHandler>,
    on_init: impl FnOnce(Glazier),
) -> Result<(), Error> {
    let app = Application::new()?;
    // Current glazier's design forces
    let state = Rc::new(RefCell::new(V1SharedState {
        glz: GlazierState {
            app: app.clone(),
            windows: Default::default(),
            operations: Default::default(),
        },
        handler,
    }));
    with_glz(&state, |_, glz| on_init(glz));
    let handler = V1AppHandler { state };
    app.run(Some(Box::new(handler)));
    Ok(())
}
pub type GlazierImpl<'a> = &'a mut GlazierState;

struct V1AppHandler {
    state: Rc<RefCell<V1SharedState>>,
}

impl AppHandler for V1AppHandler {
    fn command(&mut self, id: u32) {
        with_glz(&self.state, |handler, glz| {
            handler.app_menu_item_selected(glz, id)
        });
    }
}

struct V1WindowHandler {
    state: Rc<RefCell<V1SharedState>>,
    window: WindowId,
}

impl V1WindowHandler {
    fn with_glz<R>(
        &mut self,
        f: impl FnOnce(&mut Box<dyn PlatformHandler>, Glazier, WindowId) -> R,
    ) -> R {
        with_glz(&self.state, |handler, glz| f(handler, glz, self.window))
    }
}

impl WinHandler for V1WindowHandler {
    fn connect(&mut self, _: &glazier::WindowHandle) {
        self.with_glz(|handler, glz, win| handler.surface_available(glz, win))
    }

    fn prepare_paint(&mut self) {
        self.with_glz(|handler, glz, win| handler.prepare_paint(glz, win))
    }

    fn paint(&mut self, invalid: &glazier::Region) {
        self.with_glz(|handler, glz, win| handler.paint(glz, win, invalid))
    }

    fn command(&mut self, id: u32) {
        self.with_glz(|handler, glz, win| handler.menu_item_selected(glz, win, id))
    }

    fn as_any(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

struct V1SharedState {
    glz: GlazierState,
    handler: Box<dyn PlatformHandler>,
}

pub(crate) struct GlazierState {
    app: glazier::Application,
    windows: HashMap<WindowId, glazier::WindowHandle>,
    operations: Vec<Command>,
}

fn with_glz<R>(
    outer_state: &Rc<RefCell<V1SharedState>>,
    f: impl FnOnce(&mut Box<dyn PlatformHandler>, Glazier) -> R,
) -> R {
    let mut state = outer_state.borrow_mut();
    let state = &mut *state;
    let glz = Glazier(&mut state.glz, PhantomData);
    let res = f(&mut state.handler, glz);
    let mut create_window_failures = Vec::<(WindowId, Error)>::new();
    for op in state.glz.operations.drain(..) {
        match op {
            Command::NewWindow(builder) => {
                // let state = outer_state.clone();
                let bld = glazier::WindowBuilder::new(state.glz.app.clone());
                let bld = bld
                    .title(builder.title)
                    .resizable(builder.resizable)
                    .show_titlebar(builder.show_titlebar)
                    .transparent(builder.transparent);
                let id = builder.id.unwrap_or_else(WindowId::next);
                let window_id = builder.id.unwrap_or_else(WindowId::next);
                let bld = bld.handler(Box::new(V1WindowHandler {
                    state: outer_state.clone(),
                    window: window_id,
                }));
                let window = bld.build();

                match window {
                    Ok(window) => {
                        state.glz.windows.insert(id, window);
                    }
                    Err(e) => create_window_failures.push((window_id, e)),
                }
            }
        }
    }
    for (win, error) in create_window_failures.drain(..) {
        let glz = Glazier(&mut state.glz, PhantomData);
        state.handler.creating_window_failed(glz, win, error)
    }
    res
}
pub enum Command {
    NewWindow(WindowBuilder),
}

impl GlazierState {
    pub(crate) fn stop(&mut self) {
        self.app.quit()
    }

    pub(crate) fn new_window(&mut self, mut builder: WindowBuilder) -> WindowId {
        let id = builder.id.unwrap_or_else(WindowId::next);
        builder.id = Some(id);
        self.operations.push(Command::NewWindow(builder));
        id
    }
}

use std::num::NonZeroU64;

use glazier::Counter;

pub struct WindowDescription {
    pub title: String,
    // menu: Option<Menu>,
    // size: Size,
    // min_size: Option<Size>,
    // position: Option<Point>,
    // level: Option<WindowLevel>,
    // window_state: Option<WindowState>,
    pub resizable: bool,
    pub show_titlebar: bool,
    pub transparent: bool,
    // TODO: Should the id live in the builder,
    // and/or should these be Glazier specific?
    // That would allow using the struct initialisation syntax (i.e. ..Default::default),
    // which is tempting
    pub(crate) id: Option<WindowId>,
}

impl WindowDescription {
    pub fn new(title: impl Into<String>) -> Self {
        WindowDescription {
            title: title.into(),
            resizable: true,
            show_titlebar: true,
            transparent: false,
            id: None,
        }
    }

    pub fn id(&self) -> Option<WindowId> {
        self.id
    }

    pub fn assign_id(&mut self) -> WindowId {
        *self.id.get_or_insert_with(WindowId::next)
    }
}

impl Default for WindowDescription {
    fn default() -> Self {
        Self::new("Glazier Application Window")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WindowId(NonZeroU64);

static WINDOW_ID_COUNTER: Counter = Counter::new();

impl WindowId {
    pub(crate) fn next() -> Self {
        Self(WINDOW_ID_COUNTER.next_nonzero())
    }
}

// pub struct NativeWindowHandle(backend::NativeWindowHandle);

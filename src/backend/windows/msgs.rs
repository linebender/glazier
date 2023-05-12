use once_cell::race::OnceBox;
use winapi::shared::minwindef::UINT;
use winapi::um::winuser::RegisterWindowMessageW;

pub(crate) static WM_RUN_MAIN_CB_QUEUE: LazyMsg = LazyMsg::new("WM_RUN_MAIN_CB_QUEUE");

pub(crate) struct LazyMsg {
    // NOTE: we are fine to use the `race` variant of `OnceBox` here because `RegisterWindowMessage`
    // is thread-safe and idempotent.
    msg: OnceBox<UINT>,
    name: &'static str,
}

impl LazyMsg {
    const fn new(name: &'static str) -> Self {
        Self {
            msg: OnceBox::new(),
            name,
        }
    }

    pub fn get(&self) -> UINT {
        *self
            .msg
            .get_or_init(|| unsafe { RegisterWindowMessageW(self.name.to_wide().as_ptr()) })
    }
}

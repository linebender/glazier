use v2::{
    window::{WindowDescription, WindowId},
    *,
};

fn main() {
    let mut plat = GlazierBuilder::new();
    let my_window = plat.new_window(WindowDescription {
        ..WindowDescription::new("Testing App For Glazier v2")
    });
    plat.launch(EventHandler {
        main_window_id: my_window,
    })
}

struct EventHandler {
    main_window_id: WindowId,
}

impl PlatformHandler for EventHandler {
    fn surface_available(&mut self, glz: Glazier, win: WindowId) {}

    fn paint(&mut self, glz: Glazier, win: WindowId, invalid: &window::Region) {}

    fn as_any(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

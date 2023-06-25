use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use wayland_client as wlc;
use wayland_client::protocol::wl_surface;
use wayland_protocols::xdg_shell::client::xdg_popup;
use wayland_protocols::xdg_shell::client::xdg_positioner;
use wayland_protocols::xdg_shell::client::xdg_surface;

use crate::kurbo;
use crate::window;
use crate::{region::Region, scale::Scale, TextFieldToken};

use super::super::Changed;

use super::super::outputs;
use super::buffers;
use super::error;
use super::idle;
use super::Popup;
use super::{Compositor, CompositorHandle, Decor, Handle, Outputs};

pub enum DeferredTask {
    Paint,
    AnimationClear,
}

#[derive(Clone)]
pub struct Surface {
    pub(super) inner: std::sync::Arc<Data>,
}

impl From<std::sync::Arc<Data>> for Surface {
    fn from(d: std::sync::Arc<Data>) -> Self {
        Self { inner: d }
    }
}

impl Surface {
    pub fn new(
        c: impl Into<CompositorHandle>,
        handler: Box<dyn window::WinHandler>,
        initial_size: kurbo::Size,
    ) -> Self {
        let compositor = CompositorHandle::new(c);
        let wl_surface = match compositor.create_surface() {
            None => panic!("unable to create surface"),
            Some(v) => v,
        };

        let current = std::sync::Arc::new(Data {
            compositor,
            wl_surface: RefCell::new(wl_surface),
            outputs: RefCell::new(std::collections::HashSet::new()),
            logical_size: Cell::new(initial_size),
            scale: Cell::new(1),
            anim_frame_requested: Cell::new(false),
            handler: RefCell::new(handler),
            idle_queue: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            active_text_input: Cell::new(None),
            damaged_region: RefCell::new(Region::EMPTY),
            deferred_tasks: RefCell::new(std::collections::VecDeque::new()),
        });

        // register to receive wl_surface events.
        Surface::initsurface(&current);

        Self { inner: current }
    }

    pub(super) fn output(&self) -> Option<outputs::Meta> {
        self.inner.output()
    }

    pub(super) fn request_paint(&self) {
        self.inner.paint();
    }

    pub(super) fn update_dimensions(&self, dim: impl Into<kurbo::Size>) -> kurbo::Size {
        self.inner.update_dimensions(dim)
    }

    pub(super) fn commit(&self) {
        self.inner.wl_surface.borrow().commit()
    }

    pub(super) fn replace(current: &std::sync::Arc<Data>) -> Surface {
        current
            .wl_surface
            .replace(match current.compositor.create_surface() {
                None => panic!("unable to create surface"),
                Some(v) => v,
            });
        Surface::initsurface(current);
        Self {
            inner: current.clone(),
        }
    }

    fn initsurface(current: &std::sync::Arc<Data>) {
        current.wl_surface.borrow().quick_assign({
            let current = current.clone();
            move |a, event, b| {
                tracing::debug!("wl_surface event {:?} {:?} {:?}", a, event, b);
                Surface::consume_surface_event(&current, &a, &event, &b);
            }
        });
    }

    pub(super) fn consume_surface_event(
        current: &std::sync::Arc<Data>,
        surface: &wlc::Main<wlc::protocol::wl_surface::WlSurface>,
        event: &wlc::protocol::wl_surface::Event,
        data: &wlc::DispatchData,
    ) {
        tracing::debug!("wl_surface event {:?} {:?} {:?}", surface, event, data);
        match event {
            wl_surface::Event::Enter { output } => {
                let proxy = wlc::Proxy::from(output.clone());
                current.outputs.borrow_mut().insert(proxy.id());
            }
            wl_surface::Event::Leave { output } => {
                let proxy = wlc::Proxy::from(output.clone());
                current.outputs.borrow_mut().remove(&proxy.id());
            }
            _ => tracing::warn!("unhandled wayland surface event {:?}", event),
        }

        if current.wl_surface.borrow().as_ref().version() >= wl_surface::REQ_SET_BUFFER_SCALE_SINCE
        {
            let new_scale = current.recompute_scale();
            if current.set_scale(new_scale).is_changed() {
                // always repaint, because the scale changed.
                current.schedule_deferred_task(DeferredTask::Paint);
            }
        }
    }
}

impl Outputs for Surface {
    fn removed(&self, o: &outputs::Meta) {
        self.inner.outputs.borrow_mut().remove(&o.id());
    }

    fn inserted(&self, _: &outputs::Meta) {
        // nothing to do here.
    }
}

impl Handle for Surface {
    fn get_size(&self) -> kurbo::Size {
        self.inner.get_size()
    }

    fn set_size(&self, _: kurbo::Size) {
        todo!("Wayland doesn't allow setting surface size");
    }

    fn request_anim_frame(&self) {
        self.inner.request_anim_frame()
    }

    fn remove_text_field(&self, token: TextFieldToken) {
        self.inner.remove_text_field(token)
    }

    fn set_focused_text_field(&self, active_field: Option<TextFieldToken>) {
        self.inner.set_focused_text_field(active_field)
    }

    fn get_idle_handle(&self) -> idle::Handle {
        self.inner.get_idle_handle()
    }

    fn get_scale(&self) -> Scale {
        self.inner.get_scale()
    }

    fn invalidate(&self) {
        self.inner.invalidate()
    }

    fn invalidate_rect(&self, rect: kurbo::Rect) {
        self.inner.invalidate_rect(rect)
    }

    fn run_idle(&self) {
        self.inner.run_idle();
    }

    fn release(&self) {
        self.inner.release()
    }

    fn data(&self) -> Option<std::sync::Arc<Data>> {
        Some(Into::into(self))
    }
}

impl From<Surface> for std::sync::Arc<Data> {
    fn from(s: Surface) -> std::sync::Arc<Data> {
        s.inner
    }
}

impl From<&Surface> for std::sync::Arc<Data> {
    fn from(s: &Surface) -> std::sync::Arc<Data> {
        s.inner.clone()
    }
}

pub struct Data {
    pub(super) compositor: CompositorHandle,
    pub(super) wl_surface: RefCell<wlc::Main<wl_surface::WlSurface>>,

    /// The outputs that our surface is present on (we should get the first enter event early).
    pub(super) outputs: RefCell<std::collections::HashSet<u32>>,

    /// The logical size of the next frame.
    pub(crate) logical_size: Cell<kurbo::Size>,
    /// The scale we are rendering to (defaults to 1)
    pub(crate) scale: Cell<i32>,

    /// Contains the callbacks from user code.
    pub(crate) handler: RefCell<Box<dyn window::WinHandler>>,
    pub(crate) active_text_input: Cell<Option<TextFieldToken>>,

    /// Whether we have requested an animation frame. This stops us requesting more than 1.
    anim_frame_requested: Cell<bool>,
    /// Rects of the image that are damaged and need repainting in the logical coordinate space.
    ///
    /// This lives outside `data` because they can be borrowed concurrently without re-entrancy.
    damaged_region: RefCell<Region>,
    /// Tasks that were requested in user code.
    ///
    /// These call back into user code, and so should only be run after all user code has returned,
    /// to avoid possible re-entrancy.
    deferred_tasks: RefCell<std::collections::VecDeque<DeferredTask>>,

    idle_queue: std::sync::Arc<std::sync::Mutex<Vec<idle::Kind>>>,
}

impl Data {
    pub(crate) fn output(&self) -> Option<outputs::Meta> {
        match self.outputs.borrow().iter().find(|_| true) {
            None => None,
            Some(id) => self.compositor.output(*id),
        }
    }

    #[track_caller]
    pub(crate) fn with_handler<T, F: FnOnce(&mut dyn window::WinHandler) -> T>(
        &self,
        f: F,
    ) -> Option<T> {
        let ret = self.with_handler_and_dont_check_the_other_borrows(f);
        self.run_deferred_tasks();
        ret
    }

    #[track_caller]
    fn with_handler_and_dont_check_the_other_borrows<
        T,
        F: FnOnce(&mut dyn window::WinHandler) -> T,
    >(
        &self,
        f: F,
    ) -> Option<T> {
        match self.handler.try_borrow_mut() {
            Ok(mut h) => Some(f(&mut **h)),
            Err(_) => {
                tracing::error!(
                    "failed to borrow WinHandler at {}",
                    std::panic::Location::caller()
                );
                None
            }
        }
    }

    pub(super) fn update_dimensions(&self, dim: impl Into<kurbo::Size>) -> kurbo::Size {
        let dim = dim.into();
        if self.logical_size.get() != dim {
            self.logical_size.set(dim);
            match self.handler.try_borrow_mut() {
                Ok(mut handler) => handler.size(dim),
                Err(cause) => tracing::warn!("unhable to borrow handler {:?}", cause),
            };
        }

        dim
    }

    /// Recompute the scale to use (the maximum of all the scales for the different outputs this
    /// surface is drawn to).
    fn recompute_scale(&self) -> i32 {
        tracing::debug!("recompute initiated");
        self.compositor.recompute_scale(&self.outputs.borrow())
    }

    /// Sets the scale
    ///
    /// Up to the caller to make sure `physical_size`, `logical_size` and `scale` are consistent.
    fn set_scale(&self, new_scale: i32) -> Changed {
        tracing::debug!("set_scale initiated");
        if self.scale.get() != new_scale {
            self.scale.set(new_scale);
            // (re-entrancy) Report change to client
            self.handler
                .borrow_mut()
                .scale(Scale::new(new_scale as f64, new_scale as f64));
            Changed::Changed
        } else {
            Changed::Unchanged
        }
    }

    /// Paint the next frame.
    ///
    /// The buffers object is responsible for calling this function after we called
    /// `request_paint`.
    ///
    /// - `size` is the physical size in pixels we are drawing.
    /// - `force` means draw the whole frame, even if it wasn't all invalidated.
    ///
    /// This calls into user code. To avoid re-entrancy, ensure that we are not already in user
    /// code (defer this call if necessary).
    pub(super) fn paint(&self) {
        // We don't care about obscure pre version 4 compositors
        // and just damage the whole surface instead of
        // translating from buffer coordinates to surface coordinates
        let damage_buffer_supported =
            self.wl_surface.borrow().as_ref().version() >= wl_surface::REQ_DAMAGE_BUFFER_SINCE;

        if !damage_buffer_supported {
            self.invalidate();
            self.wl_surface.borrow().damage(0, 0, i32::MAX, i32::MAX);
        } else {
            let damaged_region = self.damaged_region.borrow_mut();
            for rect in damaged_region.rects() {
                // Convert it to physical coordinate space.
                let rect = buffers::RawRect::from(*rect).scale(self.scale.get());

                self.wl_surface.borrow().damage_buffer(
                    rect.x0,
                    rect.y0,
                    rect.x1 - rect.x0,
                    rect.y1 - rect.y0,
                );
            }
            if damaged_region.is_empty() {
                // Nothing to draw, so we can finish here!
                return;
            }
        }
        let mut swap_region = Region::EMPTY;
        {
            let mut region = self.damaged_region.borrow_mut();
            // reset damage ready for next frame.
            std::mem::swap(&mut *region, &mut swap_region);
        }

        self.handler.borrow_mut().paint(&swap_region);
        self.wl_surface.borrow().commit();
    }

    /// Request invalidation of the entire window contents.
    fn invalidate(&self) {
        tracing::trace!("invalidate initiated");
        // This is one of 2 methods the user can use to schedule a repaint, the other is
        // `invalidate_rect`.
        let window_rect = self.logical_size.get().to_rect();
        self.damaged_region.borrow_mut().add_rect(window_rect);
        self.schedule_deferred_task(DeferredTask::Paint);
    }

    /// Request invalidation of one rectangle, which is given in display points relative to the
    /// drawing area.
    fn invalidate_rect(&self, rect: kurbo::Rect) {
        tracing::trace!("invalidate_rect initiated {:?}", rect);
        // Quick check to see if we can skip the rect entirely (if it is outside the visible
        // screen).
        if rect.intersect(self.logical_size.get().to_rect()).is_empty() {
            return;
        }
        /* this would be useful for debugging over-keen invalidation by clients.
        println!(
            "{:?} {:?}",
            rect,
            self.with_data(|data| data.logical_size.to_rect())
        );
        */
        self.damaged_region.borrow_mut().add_rect(rect);
        self.schedule_deferred_task(DeferredTask::Paint);
    }

    pub fn schedule_deferred_task(&self, task: DeferredTask) {
        tracing::trace!("scedule_deferred_task initiated");
        self.deferred_tasks.borrow_mut().push_back(task);
    }

    pub fn run_deferred_tasks(&self) {
        tracing::trace!("run_deferred_tasks initiated");
        let tasks = std::mem::take(&mut *self.deferred_tasks.borrow_mut());
        let mut should_paint = false;
        for task in tasks {
            match task {
                DeferredTask::Paint => {
                    should_paint = true;
                }
                DeferredTask::AnimationClear => {
                    self.anim_frame_requested.set(false);
                }
            }
        }
        if should_paint {
            self.paint();
        }
    }

    pub(super) fn get_size(&self) -> kurbo::Size {
        // size in pixels, so we must apply scale.
        let logical_size = self.logical_size.get();
        let scale = self.scale.get() as f64;
        kurbo::Size::new(logical_size.width * scale, logical_size.height * scale)
    }

    pub(super) fn request_anim_frame(&self) {
        if self.anim_frame_requested.replace(true) {
            return;
        }

        let idle = self.get_idle_handle();
        idle.add_idle_callback(move |winhandle| {
            winhandle.prepare_paint();
        });
        self.schedule_deferred_task(DeferredTask::AnimationClear);
    }

    pub(super) fn remove_text_field(&self, token: TextFieldToken) {
        if self.active_text_input.get() == Some(token) {
            self.active_text_input.set(None);
        }
    }

    pub(super) fn set_focused_text_field(&self, active_field: Option<TextFieldToken>) {
        self.active_text_input.set(active_field);
    }

    pub(super) fn get_idle_handle(&self) -> idle::Handle {
        idle::Handle {
            queue: self.idle_queue.clone(),
        }
    }

    pub(super) fn get_scale(&self) -> Scale {
        let scale = self.scale.get() as f64;
        Scale::new(scale, scale)
    }

    pub(super) fn run_idle(&self) {
        self.with_handler(|winhandle| {
            idle::run(&self.get_idle_handle(), winhandle);
        });
    }

    pub(super) fn release(&self) {
        self.wl_surface.borrow().destroy();
    }

    pub(crate) fn get_surface(&self) -> *mut c_void {
        self.wl_surface.borrow().as_ref().c_ptr().cast::<c_void>()
    }
    pub(crate) fn get_display(&self) -> *mut c_void {
        self.compositor.display_as_ptr()
    }
}

#[derive(Default)]
pub struct Dead;

impl From<Dead> for Box<dyn Decor> {
    fn from(d: Dead) -> Box<dyn Decor> {
        Box::new(d) as Box<dyn Decor>
    }
}

impl From<Dead> for Box<dyn Outputs> {
    fn from(d: Dead) -> Box<dyn Outputs> {
        Box::new(d) as Box<dyn Outputs>
    }
}

impl Decor for Dead {
    fn inner_set_title(&self, title: String) {
        tracing::warn!("set_title not implemented for this surface: {:?}", title);
    }
}

impl Outputs for Dead {
    fn removed(&self, _: &outputs::Meta) {}

    fn inserted(&self, _: &outputs::Meta) {}
}

impl Popup for Dead {
    fn surface<'a>(
        &self,
        _: &'a wlc::Main<xdg_surface::XdgSurface>,
        _: &'a wlc::Main<xdg_positioner::XdgPositioner>,
    ) -> Result<wlc::Main<xdg_popup::XdgPopup>, error::Error> {
        tracing::warn!("popup invoked on a dead surface");
        Err(error::Error::InvalidParent(0))
    }
}

impl Handle for Dead {
    fn get_size(&self) -> kurbo::Size {
        kurbo::Size::ZERO
    }

    fn set_size(&self, dim: kurbo::Size) {
        tracing::warn!("set_size invoked on a dead surface {:?}", dim);
    }

    fn request_anim_frame(&self) {
        tracing::warn!("request_anim_frame invoked on a dead surface")
    }

    fn remove_text_field(&self, _token: TextFieldToken) {
        tracing::warn!("remove_text_field invoked on a dead surface")
    }

    fn set_focused_text_field(&self, _active_field: Option<TextFieldToken>) {
        tracing::warn!("set_focused_text_field invoked on a dead surface")
    }

    fn get_idle_handle(&self) -> idle::Handle {
        panic!("get_idle_handle invoked on a dead surface")
    }

    fn get_scale(&self) -> Scale {
        Scale::new(1., 1.)
    }

    fn invalidate(&self) {
        tracing::warn!("invalidate invoked on a dead surface")
    }

    fn invalidate_rect(&self, _rect: kurbo::Rect) {
        tracing::warn!("invalidate_rect invoked on a dead surface")
    }

    fn run_idle(&self) {
        tracing::warn!("run_idle invoked on a dead surface")
    }

    fn release(&self) {
        tracing::warn!("release invoked on a dead surface");
    }

    fn data(&self) -> Option<std::sync::Arc<Data>> {
        tracing::warn!("data invoked on a dead surface");
        None
    }
}

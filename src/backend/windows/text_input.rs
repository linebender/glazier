#![allow(non_snake_case)]
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::ops::Range;
use std::os::windows::prelude::OsStrExt;
use std::rc::Rc;
use std::{ptr, slice};

use crate::text::{Affinity, Event, InputHandler, Selection};
use crate::{Scalable, TextFieldToken};

use kurbo::{Point, Rect};
use winapi::shared::windef as wapi;
use winapi::um::winuser::{MapWindowPoints, SendMessageW};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{
            Com::*,
            Ole::{CONNECT_E_ADVISELIMIT, CONNECT_E_NOCONNECTION},
        },
        UI::TextServices::*,
    },
};
use wio::wide::FromWide;

struct Document {
    manager: ITfDocumentMgr,
    text_store: ITextStoreACP,
}

pub struct TsfIntegration {
    mgr: ITfThreadMgr,
    id: u32,
    docs: HashMap<TextFieldToken, Rc<Document>>,
}

impl TsfIntegration {
    pub fn new() -> Result<Self> {
        Ok(unsafe {
            let mgr: ITfThreadMgr =
                CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER)?;

            Self {
                id: mgr.Activate()?,
                mgr,
                docs: HashMap::new(),
            }
        })
    }

    pub fn register_text_field(&mut self, hwnd: wapi::HWND, token: TextFieldToken) -> Result<()> {
        Ok(unsafe {
            let manager = self.mgr.CreateDocumentMgr()?;

            let text_store = TextStore::new(HWND(hwnd as isize), token).into();
            let mut context = None;
            let mut edit_cookie = 0;
            manager.CreateContext(self.id, 0, &text_store, &mut context, &mut edit_cookie)?;
            manager.Push(context)?;

            self.docs.insert(
                token,
                Rc::new(Document {
                    text_store,
                    manager,
                }),
            );
        })
    }

    pub fn unregister_text_field(&mut self, token: TextFieldToken) {
        self.docs.remove(&token);
    }

    pub fn focus(&self, token: TextFieldToken) -> Result<()> {
        if let Some(document) = self.docs.get(&token) {
            unsafe {
                self.mgr.SetFocus(&document.manager)?;
                self.update(token, Event::SelectionChanged)?;
            }
            Ok(())
        } else {
            Err(E_UNEXPECTED.into())
        }
    }

    pub fn update(&self, token: TextFieldToken, event: Event) -> Result<()> {
        if let Some(document) = self.docs.get(&token) {
            unsafe {
                let text_store = TextStore::to_impl(&document.text_store);
                if let Some(sink) = text_store.sink.borrow().as_ref() {
                    println!("{event:?}");
                    match event {
                        Event::LayoutChanged => {
                            sink.OnLayoutChange(TS_LC_CHANGE, text_store.view_cookie)?
                        }
                        Event::SelectionChanged => println!("----------SELECTION CHANGED {:?}", sink.OnSelectionChange()),
                        Event::Reset => {
                            sink.OnTextChange(TS_ST_NONE, &TS_TEXTCHANGE {
                                ..Default::default()
                            })?;
                            sink.OnSelectionChange()?;
                            sink.OnLayoutChange(TS_LC_CHANGE, text_store.view_cookie)?;
                        }
                    }
                }
            }
            Ok(())
        } else {
            Err(E_UNEXPECTED.into())
        }
    }
}

#[implement(ITextStoreACP)]
struct TextStore {
    hwnd: HWND,
    view_cookie: u32,
    token: TextFieldToken,
    sink_punk: RefCell<Option<IUnknown>>,
    sink: RefCell<Option<ITextStoreACPSink>>,
    sink_mask: Cell<u32>,
    lock: RefCell<Option<Box<dyn InputHandler>>>,
}

impl TextStore {
    pub fn new(hwnd: HWND, token: TextFieldToken) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static VIEW_COOKIE: AtomicU32 = AtomicU32::new(0);

        Self {
            hwnd,
            token,
            view_cookie: VIEW_COOKIE.fetch_add(1, Ordering::Relaxed),
            sink: RefCell::new(None),
            sink_punk: RefCell::new(None),
            sink_mask: Cell::new(0),
            lock: RefCell::new(None),
        }
    }

    fn with_lock<T, F: Fn(&mut Box<dyn InputHandler>) -> Result<T>>(
        &self,
        dwlockflags: u32,
        func: F,
    ) -> Result<T> {
        unsafe {
            let res = SendMessageW(
                self.hwnd.0 as wapi::HWND,
                super::window::DS_REQUEST_LOCK,
                self.token.into_raw() as usize,
                dwlockflags as isize,
            );

            let result = {
                let mut lock = Box::from_raw(res as *mut Box<dyn InputHandler>);

                func(&mut *lock)
            };

            SendMessageW(
                self.hwnd.0 as wapi::HWND,
                super::window::DS_DESTROY_LOCK,
                self.token.into_raw() as usize,
                0,
            );
            result
        }
    }

    unsafe fn require_lock<T, F: Fn(&mut Box<dyn InputHandler>) -> Result<T>>(
        &self,
        dwlockflags: u32,
        func: F,
    ) -> Result<T> {
        // check for lock
        self.with_lock(dwlockflags, func)
    }
}

macro_rules! function {
    () => {{
        fn f() {}
        fn type_name_of<T>(_: T) -> &'static str {
            std::any::type_name::<T>()
        }
        let name = type_name_of(f);
        &name[..name.len() - 3]
    }};
}

impl ITextStoreACP_Impl for TextStore {
    fn AdviseSink(
        &self,
        riid: *const GUID,
        punk: &Option<IUnknown>,
        dwmask: u32,
    ) -> Result<()> {
        if let Some(punk_id) = punk {
            if self.sink_punk.borrow().as_ref() == Some(punk_id) {
                self.sink_mask.set(dwmask);
                Ok(())
            } else if self.sink.borrow().is_some() {
                Err(CONNECT_E_ADVISELIMIT.into())
            } else if unsafe { *riid == ITextStoreACPSink::IID } {
                self.sink_mask.set(dwmask);
                self.sink_punk.replace(Some(punk_id.clone()));
                self.sink
                    .replace(Some(punk_id.cast::<ITextStoreACPSink>()?));

                Ok(())
            } else {
                Err(E_INVALIDARG.into())
            }
        } else {
            Err(E_INVALIDARG.into())
        }
    }

    fn UnadviseSink(&self, punk: &Option<IUnknown>) -> Result<()> {
        let mut_borrow = self.sink_punk.borrow_mut();
        if mut_borrow.is_some() && mut_borrow.as_ref() == punk.as_ref() {
            drop(mut_borrow);

            self.sink.replace(None);
            self.sink_punk.replace(None);
            self.sink_mask.replace(0);

            Ok(())
        } else {
            Err(CONNECT_E_NOCONNECTION.into())
        }
    }

    fn RequestLock(&self, dwlockflags: u32) -> Result<HRESULT> {
        if let Some(sink) = self.sink.borrow().as_ref() {
            unsafe {
                sink.OnLockGranted(TEXT_STORE_LOCK_FLAGS(dwlockflags)).unwrap();
            }

            Ok(S_OK)
        } else {
            // TODO: Crash I guess - its pretty much an invalid state without sink
            panic!()
        }
    }

    fn GetStatus(&self) -> Result<TS_STATUS> {
        Ok(TS_STATUS {
            dwDynamicFlags: 0,
            dwStaticFlags: 0,
        })
    }

    fn QueryInsert(
        &self,
        acpteststart: i32,
        acptestend: i32,
        _cch: u32,
        pacpresultstart: *mut i32,
        pacpresultend: *mut i32,
    ) -> Result<()> {
        println!("\x1B[34m{}\x1B[39m", function!());
        unsafe {
            *pacpresultstart = acpteststart;
            *pacpresultend = acptestend;
        }

        Ok(())
    }

    fn GetSelection(
        &self,
        ulindex: u32,
        _ulcount: u32,
        pselection: *mut TS_SELECTION_ACP,
        pcfetched: *mut u32,
    ) -> Result<()> {
        if ulindex != !0 && ulindex != 0 {
            return Err(E_INVALIDARG.into())
        }

        self.with_lock(TS_LF_READ.0, |lock| {
            println!("\x1B[34m{}\x1B[39m", function!());
            let selection = lock.selection();
            let sel_min = selection.anchor.min(selection.active);
            let sel_max = selection.anchor.max(selection.active);
            let (acpStart, acpEnd) = range_to_acp(&lock, sel_min..sel_max);

            unsafe {
                *pcfetched = 1;
                *pselection = TS_SELECTION_ACP {
                    acpStart,
                    acpEnd,
                    style: TS_SELECTIONSTYLE {
                        ase: if selection.active >= selection.anchor {
                            TS_AE_END
                        } else if selection.active < selection.anchor {
                            TS_AE_START
                        } else {
                            TS_AE_NONE
                        },
                        fInterimChar: false.into(),
                    },
                };

                println!("{:#?}", *pselection);
            }

            Ok(())
        })
    }

    fn SetSelection(&self, ulcount: u32, pselection: *const TS_SELECTION_ACP) -> Result<()> {
        println!("\x1B[34m{}\x1B[39m", function!());
        self.with_lock(TS_LF_READWRITE.0, |lock| {
            let selections = unsafe { slice::from_raw_parts(pselection, ulcount as usize) };

            if ulcount > 0 {
                let range = acp_to_range(&lock, selections[0].acpStart, selections[0].acpEnd);
                let (anchor, active) = if selections[0].style.ase == TS_AE_END {
                    (range.start, range.end)
                } else {
                    (range.end, range.start)
                };

                lock.set_selection(Selection::new(anchor, active));
            }

            Ok(())
        })
    }

    fn GetText(
        &self,
        acpstart: i32,
        acpend: i32,
        pchplain: PWSTR,
        cchplainreq: u32,
        pcchplainret: *mut u32,
        prgruninfo: *mut TS_RUNINFO,
        cruninforeq: u32,
        pcruninforet: *mut u32,
        pacpnext: *mut i32,
    ) -> Result<()> {
        println!("\x1B[34m{}\x1B[39m {acpstart} {acpend}", function!());
        self.with_lock(0, |lock| {
            let (_, acp_len) = range_to_acp(&lock, 0..lock.len());
            println!("{acpstart} {acp_len}");

            unsafe {
                *pcchplainret = 0;
                *pcruninforet = 0;
                *pchplain.0 = 0;
                *pacpnext = acpend;
            }

            if acpstart >= acp_len || acpend >= acp_len {
                unsafe {
                    *pacpnext = acpstart;
                }

                return Err(TF_E_INVALIDPOS.into());
            }

            let range = acp_to_range(&lock, acpstart, acpend);
            if cchplainreq > 0 {
                let mut requested_range = acp_to_range(&lock, acpstart, (acpstart + (cchplainreq as i32) - 1).min(acpend));
                requested_range.end = requested_range.end.min(lock.len());

                let string = lock.slice(range);

                let mut char_ptr = pchplain.0 as *mut u16;
                let os_str = OsStr::new(&string.as_ref()[0..requested_range.len()]).encode_wide();
                unsafe {
                    os_str.for_each(|v| {
                        ptr::write(char_ptr, v);
                        char_ptr = char_ptr.offset(1);
                    });

                    let len = char_ptr.offset_from(pchplain.0);
                    println!("{string} {len} {cchplainreq}");
                    ptr::write_bytes(char_ptr, 0, (cchplainreq - len as u32) as usize);

                    *pcchplainret = len as u32;
                    *pacpnext = len as i32;
                }
            } else {
                unsafe {
                    *pcchplainret = 0;
                }
            }

            if cruninforeq > 0 {
                unsafe {
                    *prgruninfo = TS_RUNINFO {
                        r#type: TS_RT_PLAIN,
                        uCount: (acpend - acpstart) as u32,
                    };
                    *pcruninforet = 1;
                }
            } else {
                unsafe {
                    *pcruninforet = 0;
                }
            }

            Ok(())
        })
    }

    fn SetText(
        &self,
        _dwflags: u32,
        acpstart: i32,
        acpend: i32,
        pchtext: &PCWSTR,
        cch: u32,
    ) -> Result<TS_TEXTCHANGE> {
        println!("\x1B[34m{}\x1B[39m", function!());
        self.with_lock(TS_LF_READWRITE.0, |lock| {
            let str = unsafe { OsString::from_wide_ptr(pchtext.0, cch as usize) };

            if let Some(text) = str.to_str() {
                let range = acp_to_range(&lock, acpstart, acpend);
                println!("replace {range:?} {text}");
                lock.replace_range(range.clone(), text);

                Ok(TS_TEXTCHANGE {
                    acpStart: acpstart,
                    acpOldEnd: acpend,
                    acpNewEnd: acpstart + cch as i32,
                })
            } else {
                Err(E_UNEXPECTED.into())
            }
        })
    }

    fn GetFormattedText(&self, _acpstart: i32, _acpend: i32) -> Result<IDataObject> {
        println!("\x1B[34m{}\x1B[39m", function!());
        Err(E_NOTIMPL.into())
    }

    fn GetEmbedded(
        &self,
        _acppos: i32,
        _rguidservice: *const GUID,
        _riid: *const GUID,
    ) -> Result<IUnknown> {
        println!("\x1B[34m{}\x1B[39m", function!());
        Err(E_NOTIMPL.into())
    }

    fn QueryInsertEmbedded(
        &self,
        _pguidservice: *const GUID,
        _pformatetc: *const FORMATETC,
    ) -> Result<BOOL> {
        println!("\x1B[34m{}\x1B[39m", function!());
        Ok(false.into())
    }

    fn InsertEmbedded(
        &self,
        _dwflags: u32,
        _acpstart: i32,
        _acpend: i32,
        _pdataobject: &Option<IDataObject>,
    ) -> Result<TS_TEXTCHANGE> {
        println!("\x1B[34m{}\x1B[39m", function!());
        Err(E_NOTIMPL.into())
    }

    fn InsertTextAtSelection(
        &self,
        dwflags: u32,
        pchtext: &PCWSTR,
        cch: u32,
        pacpstart: *mut i32,
        pacpend: *mut i32,
        pchange: *mut TS_TEXTCHANGE,
    ) -> Result<()> {
        println!("{}", function!());
        const _TF_IAS_NOQUERY: u32 = TF_IAS_NOQUERY.0;
        const _TF_IAS_QUERYONLY: u32 = TF_IAS_QUERYONLY.0;

        self.with_lock(TS_LF_READWRITE.0, |lock| {
            let selection_range = lock.selection().range();
            let (sel_acp_start, sel_acp_end) = range_to_acp(&lock, selection_range.clone());

            unsafe {
                *pacpstart = sel_acp_start;
                *pacpend = sel_acp_end + cch as i32;
            }

            match dwflags {
                0 | _TF_IAS_NOQUERY => {
                    let os_string = unsafe { OsString::from_wide_ptr(pchtext.0, cch as usize) };

                    if let Some(text) = os_string.to_str() {
                        let start = selection_range.start;
                        lock.replace_range(selection_range, text);

                        lock.set_selection(Selection::new(start, start + text.len()));

                        unsafe {
                            *pchange = TS_TEXTCHANGE {
                                acpStart: *pacpstart,
                                acpOldEnd: sel_acp_end,
                                acpNewEnd: *pacpend,
                            };
                        }

                        Ok(())
                    } else {
                        Err(E_UNEXPECTED.into())
                    }
                }
                _TF_IAS_QUERYONLY => Ok(()),
                _ => Err(E_INVALIDARG.into()),
            }
        })
    }

    fn InsertEmbeddedAtSelection(
        &self,
        _dwflags: u32,
        _pdataobject: &Option<IDataObject>,
        _pacpstart: *mut i32,
        _pacpend: *mut i32,
        _pchange: *mut TS_TEXTCHANGE,
    ) -> Result<()> {
        println!("\x1B[34m{}\x1B[39m", function!());
        Err(E_NOTIMPL.into())
    }

    fn RequestSupportedAttrs(
        &self,
        _dwflags: u32,
        _cfilterattrs: u32,
        _pafilterattrs: *const GUID,
    ) -> Result<()> {
        println!("\x1B[34m{}\x1B[39m", function!());
        Ok(())
    }

    fn RequestAttrsAtPosition(
        &self,
        _acppos: i32,
        _cfilterattrs: u32,
        _pafilterattrs: *const GUID,
        _dwflags: u32,
    ) -> Result<()> {
        println!("\x1B[34m{}\x1B[39m", function!());
        todo!()
    }

    fn RequestAttrsTransitioningAtPosition(
        &self,
        _acppos: i32,
        _cfilterattrs: u32,
        _pafilterattrs: *const GUID,
        _dwflags: u32,
    ) -> Result<()> {
        println!("\x1B[34m{}\x1B[39m", function!());
        todo!()
    }

    fn FindNextAttrTransition(
        &self,
        _acpstart: i32,
        _acphalt: i32,
        _cfilterattrs: u32,
        _pafilterattrs: *const GUID,
        _dwflags: u32,
        _pacpnext: *mut i32,
        _pffound: *mut BOOL,
        _plfoundoffset: *mut i32,
    ) -> Result<()> {
        println!("\x1B[34m{}\x1B[39m", function!());
        Err(E_NOTIMPL.into())
    }

    fn RetrieveRequestedAttrs(
        &self,
        _ulcount: u32,
        _paattrvals: *mut TS_ATTRVAL,
        pcfetched: *mut u32,
    ) -> Result<()> {
        println!("\x1B[34m{}\x1B[39m", function!());
        unsafe {
            *pcfetched = 0;
        }
        Ok(())
    }

    fn GetEndACP(&self) -> Result<i32> {
        println!("{}", function!());
        self.with_lock(0, |lock| Ok(lock.utf8_to_utf16(0..lock.len()) as i32))
    }

    fn GetActiveView(&self) -> Result<u32> {
        Ok(self.view_cookie)
    }

    fn GetACPFromPoint(
        &self,
        _vcview: u32,
        ptscreen: *const POINT,
        _dwflags: u32,
    ) -> Result<i32> {
        println!("{}", function!());
        self.with_lock(0, |lock| unsafe {
            let mut point = *ptscreen;
            MapWindowPoints(
                ptr::null_mut(),
                self.hwnd.0 as wapi::HWND,
                &mut point as *mut POINT as *mut () as *mut wapi::POINT,
                1,
            );

            let hit_test = lock.hit_test_point(
                Point::new(point.x as f64, point.y as f64)
                    .to_dp(super::window::get_hwnd_scale(self.hwnd.0 as wapi::HWND)),
            );
            if hit_test.is_inside {
                Ok(lock.utf8_to_utf16(0..hit_test.idx) as i32)
            } else {
                Err(TF_E_INVALIDPOINT.into())
            }
        })
    }

    fn GetTextExt(
        &self,
        _vcview: u32,
        acpstart: i32,
        acpend: i32,
        prc: *mut RECT,
        pfclipped: *mut BOOL,
    ) -> Result<()> {
        self.with_lock(0, |lock| {
            let range = acp_to_range(&lock, acpstart, acpend);

            let first_line_range = lock.line_range(range.start, Affinity::Downstream);
            let end = first_line_range.end.min(range.end);

            let bounding_box_start = lock.slice_bounding_box(range.start..range.start);
            let bounding_box_end = lock.slice_bounding_box(end..end);

            let bounding_box = bounding_box_start
                .map(|rect| {
                    if let Some(rect2) = bounding_box_end {
                        rect.union(rect2)
                    } else {
                        rect
                    }
                })
                .or(bounding_box_end);


            unsafe {
                *prc = bounding_box
                    .map(|rect| into_screen_rect(self.hwnd, rect))
                    .ok_or::<Error>(TF_E_NOLAYOUT.into())?;
                *pfclipped = BOOL(0);
            }

            Ok(())
        })
    }

    fn GetScreenExt(&self, _vcview: u32) -> Result<RECT> {
        self.with_lock(0, |lock| {
            lock.bounding_box()
                .map(|rect| unsafe { into_screen_rect(self.hwnd, rect) })
                .ok_or(TF_E_NOLAYOUT.into())
        })
    }

    fn GetWnd(&self, _vcview: u32) -> Result<HWND> {
        Ok(self.hwnd)
    }
}

unsafe fn into_screen_rect(hwnd: HWND, rect: Rect) -> RECT {
    let rect_px = rect.to_px(super::window::get_hwnd_scale(hwnd.0 as wapi::HWND));
    let mut win_rect = RECT {
        top: rect_px.y0 as i32,
        left: rect_px.x0 as i32,
        bottom: rect_px.y1 as i32,
        right: rect_px.x1 as i32,
    };

    MapWindowPoints(
        hwnd.0 as wapi::HWND,
        ptr::null_mut(),
        &mut win_rect as *mut RECT as *mut () as *mut wapi::POINT,
        2,
    );

    win_rect
}

fn acp_to_range(lock: &Box<dyn InputHandler>, acpstart: i32, mut acpend: i32) -> Range<usize> {
    if acpend == -1 {
        acpend = lock.len() as i32;
    }

    let start = lock.utf16_to_utf8(0..acpstart as usize);
    let end = lock.utf16_to_utf8(acpstart as usize..acpend as usize) + start;

    start..end
}

fn range_to_acp(lock: &Box<dyn InputHandler>, range: Range<usize>) -> (i32, i32) {
    let start = lock.utf8_to_utf16(0..range.start);
    let end = lock.utf8_to_utf16(range) + start;

    (start as i32, end as i32)
}

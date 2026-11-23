use std::cell::RefCell;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::rc::Rc;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Error, PCWSTR, Ref, Result, implement, w};

use crate::{trace, wide};

const TRAY: u32 = WM_APP + 1;
const CLOSE_OVERLAY: u32 = WM_APP + 2;
const DROPPED: u32 = WM_APP + 3;
const HOTKEY: i32 = 1;
const OVERLAY_TIMER: usize = 1;
const APP_NAME: PCWSTR = w!("ConvertAnythings");
const CONTROLLER_CLASS: PCWSTR = w!("ConvertAnythings.Controller");
const OVERLAY_CLASS: PCWSTR = w!("ConvertAnythings.Wheel");

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

struct App {
    host: HWND,
    module: HINSTANCE,
    tray: NOTIFYICONDATAW,
    taskbar_created: u32,
    overlay: Option<Overlay>,
    started: Option<std::time::Instant>,
}

struct Overlay {
    window: HWND,
    drag: Rc<RefCell<Drag>>,
}

#[derive(Default)]
struct Drag {
    file: Option<PathBuf>,
    dropped: Option<PathBuf>,
    entered: bool,
}

#[implement(IDropTarget)]
struct FileDrop {
    host: HWND,
    window: HWND,
    drag: Rc<RefCell<Drag>>,
}

impl IDropTarget_Impl for FileDrop_Impl {
    fn DragEnter(&self, data: Ref<IDataObject>, _: MODIFIERKEYS_FLAGS, _: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let file = data.as_ref().and_then(file_from_drop);
        if let Some(path) = &file {
            trace(format!("drag-enter {}", path.display()));
        } else {
            trace("drag-enter unsupported");
        }
        let mut drag = self.drag.borrow_mut();
        drag.entered = true;
        drag.file = file;
        set_drop_effect(effect, drag.file.is_some());
        unsafe { let _ = InvalidateRect(Some(self.window), None, true); }
        Ok(())
    }

    fn DragOver(&self, _: MODIFIERKEYS_FLAGS, _: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        set_drop_effect(effect, self.drag.borrow().file.is_some());
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        trace("drag-leave");
        self.drag.borrow_mut().file = None;
        unsafe { PostMessageW(Some(self.host), CLOSE_OVERLAY, WPARAM(self.window.0 as usize), LPARAM(0)) }
    }

    fn Drop(&self, _: Ref<IDataObject>, _: MODIFIERKEYS_FLAGS, _: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let mut drag = self.drag.borrow_mut();
        let accepted = set_drop_effect(effect, drag.file.is_some());
        if accepted {
            drag.dropped = drag.file.take();
        }
        drop(drag);
        unsafe { PostMessageW(Some(self.host), DROPPED, WPARAM(self.window.0 as usize), LPARAM(0)) }
    }
}

fn set_drop_effect(effect: *mut DROPEFFECT, accept: bool) -> bool {
    if effect.is_null() {
        return false;
    }
    unsafe {
        let accept = accept && (*effect).0 & DROPEFFECT_COPY.0 != 0;
        *effect = if accept { DROPEFFECT_COPY } else { DROPEFFECT_NONE };
        accept
    }
}

fn file_from_drop(data: &IDataObject) -> Option<PathBuf> {
    let format = FORMATETC {
        cfFormat: CF_HDROP.0 as u16,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    unsafe {
        let mut medium = data.GetData(&format).ok()?;
        let drop = HDROP(medium.u.hGlobal.0);
        let path = if DragQueryFileW(drop, u32::MAX, None) == 1 {
            let size = DragQueryFileW(drop, 0, None);
            let mut name = vec![0u16; size as usize + 1];
            let copied = DragQueryFileW(drop, 0, Some(&mut name));
            (copied > 0).then(|| PathBuf::from(OsString::from_wide(&name[..copied as usize])))
        } else {
            None
        };
        ReleaseStgMedium(&mut medium);
        path
    }
}

impl App {
    fn close_overlay(&mut self) {
        if let Some(overlay) = self.overlay.take() {
            unsafe {
                let _ = KillTimer(Some(self.host), OVERLAY_TIMER);
                let _ = RevokeDragDrop(overlay.window);
                let _ = DestroyWindow(overlay.window);
            }
            trace("overlay-closed");
        }
    }

    fn open_overlay(&mut self) -> Result<()> {
        if self.overlay.is_some() {
            self.close_overlay();
            return Ok(());
        }
        self.started = Some(std::time::Instant::now());
        unsafe {
            let mut pointer = POINT::default();
            GetCursorPos(&mut pointer)?;
            let monitor = MonitorFromPoint(pointer, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            GetMonitorInfoW(monitor, &mut info).ok()?;
            let (mut dpi, mut dpi_y) = (96, 96);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y);
            let bounds = info.rcWork;
            let size = ((280 * dpi / 96) as i32).min((bounds.right - bounds.left).min(bounds.bottom - bounds.top));
            let x = (pointer.x - size / 2).clamp(bounds.left, bounds.right - size);
            let y = (pointer.y - size / 2).clamp(bounds.top, bounds.bottom - size);

            let window = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                OVERLAY_CLASS, APP_NAME, WS_POPUP, x, y, size, size, None, None, Some(self.module), None,
            )?;
            let drag = Rc::new(RefCell::new(Drag::default()));
            let target: IDropTarget = FileDrop { host: self.host, window, drag: Rc::clone(&drag) }.into();
            if let Err(error) = RegisterDragDrop(window, &target) {
                let _ = DestroyWindow(window);
                return Err(error);
            }
            self.overlay = Some(Overlay { window, drag });
            let region = CreateEllipticRgn(0, 0, size, size);
            if SetWindowRgn(window, Some(region), false) == 0 {
                let _ = DeleteObject(HGDIOBJ(region.0));
            }
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
            trace(format!("overlay-open {x} {y} {size} {dpi}"));
            SetTimer(Some(self.host), OVERLAY_TIMER, 100, None);
            Ok(())
        }
    }

    fn finish_drop(&mut self) {
        let path = self.overlay.as_ref().and_then(|overlay| overlay.drag.borrow_mut().dropped.take());
        self.close_overlay();
        if let Some(path) = path {
            trace(format!("drop {}", path.display()));
        }
    }

    fn timer(&mut self) {
        let mouse_down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0 || GetAsyncKeyState(VK_RBUTTON.0 as i32) < 0 };
        if let Some(overlay) = &self.overlay {
            let expired = self.started.is_some_and(|instant| instant.elapsed().as_secs() >= 12);
            let ended = overlay.drag.borrow().entered && !mouse_down;
            if (expired && !mouse_down) || ended {
                self.finish_drop();
            }
        }
    }
}

fn with_app(action: impl FnOnce(&mut App)) {
    APP.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut() && let Some(app) = slot.as_mut() {
            action(app);
        }
    });
}

unsafe extern "system" fn controller(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match message {
        WM_HOTKEY => {
            with_app(|app| if let Err(error) = app.open_overlay() { trace(format!("overlay-error {error}")); });
        }
        WM_TIMER if wp.0 == OVERLAY_TIMER => with_app(App::timer),
        CLOSE_OVERLAY | DROPPED => with_app(|app| {
            if app.overlay.as_ref().is_some_and(|overlay| overlay.window.0 as usize == wp.0) {
                if message == DROPPED { app.finish_drop(); } else { app.close_overlay(); }
            }
        }),
        TRAY if lp.0 as u32 == WM_RBUTTONUP => tray_menu(hwnd),
        TRAY if lp.0 as u32 == WM_LBUTTONUP => unsafe { let _ = PostMessageW(Some(hwnd), WM_HOTKEY, WPARAM(HOTKEY as usize), LPARAM(0)); },
        WM_CLOSE => unsafe { let _ = DestroyWindow(hwnd); },
        WM_DESTROY => {
            with_app(|app| {
                app.close_overlay();
                unsafe {
                    let _ = Shell_NotifyIconW(NIM_DELETE, &app.tray);
                    let _ = UnregisterHotKey(Some(hwnd), HOTKEY);
                }
            });
            unsafe { PostQuitMessage(0); }
        }
        _ => {
            with_app(|app| {
                if message == app.taskbar_created {
                    unsafe { let _ = Shell_NotifyIconW(NIM_ADD, &app.tray); }
                }
            });
            return unsafe { DefWindowProcW(hwnd, message, wp, lp) };
        }
    }
    LRESULT(0)
}

fn tray_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return; };
        let _ = AppendMenuW(menu, MF_STRING, 1, w!("Mở vùng thả"));
        let _ = AppendMenuW(menu, MF_STRING, 2, w!("Thoát"));
        let mut pointer = POINT::default();
        let _ = GetCursorPos(&mut pointer);
        let _ = SetForegroundWindow(hwnd);
        let selected = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, pointer.x, pointer.y, None, hwnd, None).0;
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        match selected {
            1 => { let _ = PostMessageW(Some(hwnd), WM_HOTKEY, WPARAM(HOTKEY as usize), LPARAM(0)); }
            2 => { let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)); }
            _ => {}
        }
    }
}

unsafe extern "system" fn wheel(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if message == WM_PAINT {
        unsafe {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut paint);
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            let brush = CreateSolidBrush(COLORREF(0x34261b));
            FillRect(dc, &rect, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, COLORREF(0xffffff));
            let mut title: Vec<_> = "ConvertAnythings".encode_utf16().collect();
            DrawTextW(dc, &mut title, &mut rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            let _ = EndPaint(hwnd, &paint);
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, message, wp, lp) }
}

struct OleGuard;

impl Drop for OleGuard {
    fn drop(&mut self) { unsafe { OleUninitialize(); } }
}

struct MutexGuard(HANDLE);

impl Drop for MutexGuard {
    fn drop(&mut self) { unsafe { let _ = CloseHandle(self.0); } }
}

pub fn run() -> Result<()> {
    unsafe {
        let mutex = MutexGuard(CreateMutexW(None, false, w!("Local\\ConvertAnythings.UI.v1"))?);
        if GetLastError() == ERROR_ALREADY_EXISTS { return Ok(()); }
        let _mutex = mutex;
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        OleInitialize(None)?;
        let _ole = OleGuard;
        let module = HINSTANCE(GetModuleHandleW(None)?.0);
        let icon = LoadIconW(None, IDI_APPLICATION)?;
        let cursor = LoadCursorW(None, IDC_ARROW)?;
        let classes: [(PCWSTR, WNDPROC); 2] = [(CONTROLLER_CLASS, Some(controller)), (OVERLAY_CLASS, Some(wheel))];
        for (class, proc) in classes {
            let info = WNDCLASSW { lpfnWndProc: proc, hInstance: module, hCursor: cursor, hIcon: icon, lpszClassName: class, ..Default::default() };
            if RegisterClassW(&info) == 0 { return Err(Error::from_thread()); }
        }
        let host = CreateWindowExW(WS_EX_TOOLWINDOW, CONTROLLER_CLASS, APP_NAME, WS_POPUP, 0, 0, 0, 0, None, None, Some(module), None)?;
        let mut tray = NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: host, uID: 1, uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP, uCallbackMessage: TRAY, hIcon: icon, ..Default::default() };
        let tip = wide("ConvertAnythings (F8)");
        tray.szTip[..tip.len()].copy_from_slice(&tip);
        Shell_NotifyIconW(NIM_ADD, &tray).ok()?;
        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        APP.with(|slot| *slot.borrow_mut() = Some(App { host, module, tray, taskbar_created, overlay: None, started: None }));
        if let Err(error) = RegisterHotKey(Some(host), HOTKEY, MOD_NOREPEAT, VK_F8.0 as u32) {
            trace(format!("hotkey-registration-error {error}"));
        }
        trace("ready");
        let mut message = MSG::default();
        let result = loop {
            match GetMessageW(&mut message, None, 0, 0).0 {
                -1 => break Err(Error::from_thread()),
                0 => break Ok(()),
                _ => { let _ = TranslateMessage(&message); DispatchMessageW(&message); }
            }
        };
        APP.with(|slot| slot.borrow_mut().take());
        result
    }
}

pub fn show_error(message: &str) {
    let message = wide(message);
    unsafe { MessageBoxW(None, PCWSTR(message.as_ptr()), APP_NAME, MB_OK | MB_ICONERROR); }
}

use std::cell::RefCell;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender};

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

use crate::convert::{self, Engines, Format};
use crate::menu::Menu;
use crate::render::{Bounds, Renderer};
use crate::{trace, wide};

const TRAY: u32 = WM_APP + 1;
const CLOSE_OVERLAY: u32 = WM_APP + 2;
const DROPPED: u32 = WM_APP + 3;
const REDRAW: u32 = WM_APP + 4;
const JOB_DONE: u32 = WM_APP + 5;
const OPEN_OVERLAY: u32 = WM_APP + 6;
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
    completions: Receiver<std::result::Result<String, String>>,
    send_completion: Sender<std::result::Result<String, String>>,
    busy: bool,
    exit_requested: bool,
    hotkey: Option<(i32, Hotkey)>,
    hotkey_file: Option<PathBuf>,
}

struct Overlay {
    window: HWND,
    drag: Rc<RefCell<Drag>>,
    renderer: Renderer,
}

#[derive(Default)]
struct Drag {
    file: Option<PathBuf>,
    dropped: Option<(PathBuf, Format)>,
    menu: Menu,
}

#[implement(IDropTarget)]
struct FileDrop {
    host: HWND,
    window: HWND,
    drag: Rc<RefCell<Drag>>,
    bounds: Bounds,
    engines: Engines,
}

impl IDropTarget_Impl for FileDrop_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let file = data.as_ref().and_then(file_from_drop);
        if let Some(path) = &file {
            trace(format!("drag-enter {}", path.display()));
        } else {
            trace("drag-enter unsupported");
        }
        let mut drag = self.drag.borrow_mut();
        let (choices, reason) = file
            .as_deref()
            .map(|path| self.engines.choices(path))
            .unwrap_or((Vec::new(), Some("Chọn một tệp")));
        drag.menu.set_choices(choices, reason);
        drag.file = file;
        self.pick(&mut drag, point, effect);
        self.signal(REDRAW)
    }

    fn DragOver(
        &self,
        _: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        if self.pick(&mut self.drag.borrow_mut(), point, effect) {
            self.signal(REDRAW)?;
        }
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        trace("drag-leave");
        self.drag.borrow_mut().file = None;
        self.signal(CLOSE_OVERLAY)
    }

    fn Drop(
        &self,
        _: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let mut drag = self.drag.borrow_mut();
        self.pick(&mut drag, point, effect);
        let accepted = !effect.is_null() && unsafe { *effect == DROPEFFECT_COPY };
        drag.dropped = if accepted {
            drag.file.take().zip(drag.menu.output())
        } else {
            None
        };
        drop(drag);
        self.signal(DROPPED)
    }
}

impl FileDrop {
    fn pick(&self, drag: &mut Drag, point: &POINTL, effect: *mut DROPEFFECT) -> bool {
        let (x, y) = self.bounds.relative(point.x, point.y);
        let changed = drag.menu.point(x, y);
        set_drop_effect(effect, drag.file.is_some() && drag.menu.output().is_some());
        changed
    }

    fn signal(&self, message: u32) -> Result<()> {
        unsafe {
            PostMessageW(
                Some(self.host),
                message,
                WPARAM(self.window.0 as usize),
                LPARAM(0),
            )
        }
    }
}

fn set_drop_effect(effect: *mut DROPEFFECT, accept: bool) -> bool {
    if effect.is_null() {
        return false;
    }
    unsafe {
        let accept = accept && (*effect).0 & DROPEFFECT_COPY.0 != 0;
        *effect = if accept {
            DROPEFFECT_COPY
        } else {
            DROPEFFECT_NONE
        };
        accept
    }
}

fn file_from_drop(data: &IDataObject) -> Option<PathBuf> {
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
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
    fn notify(&self, message: &str, error: bool) {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.host,
            uID: 1,
            uFlags: NIF_INFO,
            dwInfoFlags: if error { NIIF_ERROR } else { NIIF_INFO },
            ..Default::default()
        };
        copy_notification(&mut data.szInfoTitle, "ConvertAnythings");
        copy_notification(&mut data.szInfo, message);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }

    fn register_hotkey(&mut self, hotkey: Hotkey, persist: bool) -> Result<()> {
        if self.hotkey.is_some_and(|(_, current)| current == hotkey) {
            return Ok(());
        }
        let id = if self.hotkey.is_some_and(|(id, _)| id == 1) {
            2
        } else {
            1
        };
        unsafe {
            RegisterHotKey(
                Some(self.host),
                id,
                hotkey.modifiers() | MOD_NOREPEAT,
                hotkey.key(),
            )?;
            if let Some((old, _)) = self.hotkey {
                let _ = UnregisterHotKey(Some(self.host), old);
            }
        }
        self.hotkey = Some((id, hotkey));
        copy_notification(
            &mut self.tray.szTip,
            &format!("ConvertAnythings ({})", hotkey.label()),
        );
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.tray);
        }
        if persist && let Some(path) = &self.hotkey_file {
            let result = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|_| std::fs::write(path, hotkey.label()));
            if let Err(error) = result {
                self.notify(
                    &format!("Phím tắt đã đổi cho phiên này. Không lưu được: {error}"),
                    true,
                );
            }
        }
        trace(format!("hotkey {}", hotkey.label()));
        Ok(())
    }

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
        if self.busy {
            self.notify("Đang chuyển một tệp. Hãy đợi tác vụ này hoàn tất.", false);
            return Ok(());
        }
        if self.overlay.is_some() {
            self.close_overlay();
            return Ok(());
        }
        self.started = Some(std::time::Instant::now());
        unsafe {
            let mut pointer = POINT::default();
            GetCursorPos(&mut pointer)?;
            let monitor = MonitorFromPoint(pointer, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            GetMonitorInfoW(monitor, &mut info).ok()?;
            let window = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_LAYERED,
                OVERLAY_CLASS,
                APP_NAME,
                WS_POPUP,
                pointer.x,
                pointer.y,
                1,
                1,
                None,
                None,
                Some(self.module),
                None,
            )?;
            let dpi = match GetDpiForWindow(window) {
                0 => 96,
                value => value,
            };
            let work = info.rcWork;
            let size =
                ((280 * dpi / 96) as i32).min((work.right - work.left).min(work.bottom - work.top));
            let x = (pointer.x - size / 2).clamp(work.left, work.right - size);
            let y = (pointer.y - size / 2).clamp(work.top, work.bottom - size);
            let bounds = Bounds { x, y, size };
            let renderer = match Renderer::new(bounds) {
                Ok(renderer) => renderer,
                Err(error) => {
                    let _ = DestroyWindow(window);
                    return Err(error);
                }
            };
            let drag = Rc::new(RefCell::new(Drag::default()));
            let target: IDropTarget = FileDrop {
                host: self.host,
                window,
                drag: Rc::clone(&drag),
                bounds,
                engines: Engines::detect(),
            }
            .into();
            if let Err(error) = renderer
                .draw(window, &drag.borrow().menu)
                .and_then(|_| RegisterDragDrop(window, &target))
            {
                let _ = DestroyWindow(window);
                return Err(error);
            }
            self.overlay = Some(Overlay {
                window,
                drag,
                renderer,
            });
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
            trace(format!("overlay-open {x} {y} {size} {dpi}"));
            if let Some(start) = self.started {
                trace(format!("menu-latency-us {}", start.elapsed().as_micros()));
            }
            SetTimer(Some(self.host), OVERLAY_TIMER, 100, None);
            Ok(())
        }
    }

    fn finish_drop(&mut self) {
        let job = self
            .overlay
            .as_ref()
            .and_then(|overlay| overlay.drag.borrow_mut().dropped.take());
        self.close_overlay();
        if let Some((path, format)) = job {
            trace(format!("drop {} -> {}", path.display(), format.extension()));
            self.busy = true;
            let host = self.host.0 as usize;
            let send = self.send_completion.clone();
            let task = std::thread::Builder::new()
                .name("convert-task".to_owned())
                .spawn(move || {
                    let result = convert::child().and_then(|mut command| {
                        command
                            .args(["--convert", "--to", format.extension()])
                            .arg(path)
                            .output()
                    });
                    let result = match result {
                        Ok(output) if output.status.success() => {
                            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
                        }
                        Ok(output) => {
                            Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
                        }
                        Err(error) => Err(error.to_string()),
                    };
                    let _ = send.send(result);
                    unsafe {
                        let _ = PostMessageW(
                            Some(HWND(host as *mut _)),
                            JOB_DONE,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                });
            if let Err(error) = task {
                self.busy = false;
                self.notify(&error.to_string(), true);
            }
        }
    }

    fn finish_job(&mut self) {
        let Ok(result) = self.completions.try_recv() else {
            return;
        };
        self.busy = false;
        match result {
            Ok(path) => {
                trace(format!("job-ok {path}"));
                let name = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or(&path);
                self.notify(&format!("Đã lưu {name}"), false);
            }
            Err(error) => {
                trace(format!("job-error {error}"));
                self.notify(&error, true);
            }
        }
        if self.exit_requested {
            unsafe {
                let _ = PostMessageW(Some(self.host), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }

    fn request_exit(&mut self) -> bool {
        if self.busy {
            self.exit_requested = true;
            self.notify("Ứng dụng sẽ thoát sau khi chuyển tệp xong.", false);
        }
        !self.busy
    }

    fn redraw(&mut self, window: usize) {
        if let Some(overlay) = &self.overlay
            && overlay.window.0 as usize == window
        {
            if let Err(error) = overlay
                .renderer
                .draw(overlay.window, &overlay.drag.borrow().menu)
            {
                trace(format!("render-error {error}"));
            } else {
                trace("overlay-redraw");
            }
        }
    }

    fn timer(&mut self) {
        let mouse_down = unsafe {
            GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0 || GetAsyncKeyState(VK_RBUTTON.0 as i32) < 0
        };
        if self.overlay.is_some() {
            let expired = self
                .started
                .is_some_and(|instant| instant.elapsed().as_secs() >= 12);
            if expired && !mouse_down {
                self.close_overlay();
            }
        }
    }
}

fn copy_notification(destination: &mut [u16], value: &str) {
    destination.fill(0);
    let capacity = destination.len() - 1;
    for (index, unit) in value.encode_utf16().take(capacity).enumerate() {
        destination[index] = unit;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hotkey {
    F8,
    F9,
    CtrlAltC,
}

impl Hotkey {
    const OPTIONS: [Self; 3] = [Self::F8, Self::F9, Self::CtrlAltC];

    const fn label(self) -> &'static str {
        match self {
            Self::F8 => "F8",
            Self::F9 => "F9",
            Self::CtrlAltC => "Ctrl+Alt+C",
        }
    }

    fn key(self) -> u32 {
        match self {
            Self::F8 => VK_F8.0.into(),
            Self::F9 => VK_F9.0.into(),
            Self::CtrlAltC => VK_C.0.into(),
        }
    }

    fn modifiers(self) -> HOT_KEY_MODIFIERS {
        match self {
            Self::CtrlAltC => MOD_CONTROL | MOD_ALT,
            _ => HOT_KEY_MODIFIERS(0),
        }
    }
}

fn hotkey_file() -> Option<PathBuf> {
    std::env::var_os("CONVERTANYTHINGS_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("ConvertAnythings"))
        })
        .map(|dir| dir.join("hotkey.txt"))
}

fn with_app(action: impl FnOnce(&mut App)) {
    APP.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut()
            && let Some(app) = slot.as_mut()
        {
            action(app);
        }
    });
}

unsafe extern "system" fn controller(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match message {
        WM_HOTKEY | OPEN_OVERLAY => {
            with_app(|app| {
                if let Err(error) = app.open_overlay() {
                    trace(format!("overlay-error {error}"));
                }
            });
        }
        REDRAW => with_app(|app| app.redraw(wp.0)),
        JOB_DONE => with_app(App::finish_job),
        WM_TIMER if wp.0 == OVERLAY_TIMER => with_app(App::timer),
        CLOSE_OVERLAY | DROPPED => with_app(|app| {
            if app
                .overlay
                .as_ref()
                .is_some_and(|overlay| overlay.window.0 as usize == wp.0)
            {
                if message == DROPPED {
                    app.finish_drop();
                } else {
                    app.close_overlay();
                }
            }
        }),
        TRAY if lp.0 as u32 == WM_RBUTTONUP => tray_menu(hwnd),
        TRAY if lp.0 as u32 == WM_LBUTTONUP => unsafe {
            let _ = PostMessageW(Some(hwnd), OPEN_OVERLAY, WPARAM(0), LPARAM(0));
        },
        WM_CLOSE => {
            let mut close = true;
            with_app(|app| close = app.request_exit());
            if close {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
        }
        WM_DESTROY => {
            with_app(|app| {
                app.close_overlay();
                unsafe {
                    let _ = Shell_NotifyIconW(NIM_DELETE, &app.tray);
                    if let Some((id, _)) = app.hotkey {
                        let _ = UnregisterHotKey(Some(hwnd), id);
                    }
                }
            });
            unsafe {
                PostQuitMessage(0);
            }
        }
        _ => {
            with_app(|app| {
                if message == app.taskbar_created {
                    unsafe {
                        let _ = Shell_NotifyIconW(NIM_ADD, &app.tray);
                    }
                }
            });
            return unsafe { DefWindowProcW(hwnd, message, wp, lp) };
        }
    }
    LRESULT(0)
}

fn tray_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let _ = AppendMenuW(menu, MF_STRING, 1, w!("Mở vùng thả"));
        let _ = AppendMenuW(menu, MF_STRING, 2, w!("Thoát"));
        if let Ok(keys) = CreatePopupMenu() {
            let mut current = None;
            with_app(|app| current = app.hotkey.map(|(_, key)| key));
            for (index, key) in Hotkey::OPTIONS.into_iter().enumerate() {
                let label = wide(key.label());
                let style = if current == Some(key) {
                    MF_CHECKED
                } else {
                    MF_UNCHECKED
                };
                let _ = AppendMenuW(keys, MF_STRING | style, 10 + index, PCWSTR(label.as_ptr()));
            }
            if AppendMenuW(menu, MF_POPUP, keys.0 as usize, w!("Đổi phím tắt")).is_err() {
                let _ = DestroyMenu(keys);
            }
        }
        let mut pointer = POINT::default();
        let _ = GetCursorPos(&mut pointer);
        let _ = SetForegroundWindow(hwnd);
        let selected = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            pointer.x,
            pointer.y,
            None,
            hwnd,
            None,
        )
        .0;
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        match selected {
            1 => {
                let _ = PostMessageW(Some(hwnd), OPEN_OVERLAY, WPARAM(0), LPARAM(0));
            }
            2 => {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            10..=12 => with_app(|app| {
                let key = Hotkey::OPTIONS[(selected - 10) as usize];
                if let Err(error) = app.register_hotkey(key, true) {
                    app.notify(
                        &format!("Không đăng ký được {}. {error}", key.label()),
                        true,
                    );
                }
            }),
            _ => {}
        }
    }
}

unsafe extern "system" fn wheel(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, message, wp, lp) }
}

struct OleGuard;

impl Drop for OleGuard {
    fn drop(&mut self) {
        unsafe {
            OleUninitialize();
        }
    }
}

struct MutexGuard(HANDLE);

impl Drop for MutexGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub fn run() -> Result<()> {
    unsafe {
        let mutex = MutexGuard(CreateMutexW(
            None,
            false,
            w!("Local\\ConvertAnythings.UI.v1"),
        )?);
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }
        let _mutex = mutex;
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        OleInitialize(None)?;
        let _ole = OleGuard;
        let module = HINSTANCE(GetModuleHandleW(None)?.0);
        let icon = LoadIconW(None, IDI_APPLICATION)?;
        let cursor = LoadCursorW(None, IDC_ARROW)?;
        let classes: [(PCWSTR, WNDPROC); 2] = [
            (CONTROLLER_CLASS, Some(controller)),
            (OVERLAY_CLASS, Some(wheel)),
        ];
        for (class, proc) in classes {
            let info = WNDCLASSW {
                lpfnWndProc: proc,
                hInstance: module,
                hCursor: cursor,
                hIcon: icon,
                lpszClassName: class,
                ..Default::default()
            };
            if RegisterClassW(&info) == 0 {
                return Err(Error::from_thread());
            }
        }
        let host = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CONTROLLER_CLASS,
            APP_NAME,
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module),
            None,
        )?;
        let mut tray = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: host,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY,
            hIcon: icon,
            ..Default::default()
        };
        copy_notification(&mut tray.szTip, "ConvertAnythings");
        Shell_NotifyIconW(NIM_ADD, &tray).ok()?;
        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        let (send_completion, completions) = std::sync::mpsc::channel();
        let hotkey_file = hotkey_file();
        let saved = hotkey_file
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok());
        let saved = Hotkey::OPTIONS
            .into_iter()
            .find(|key| Some(key.label()) == saved.as_deref().map(str::trim))
            .unwrap_or(Hotkey::F8);
        let mut app = App {
            host,
            module,
            tray,
            taskbar_created,
            overlay: None,
            started: None,
            completions,
            send_completion,
            busy: false,
            exit_requested: false,
            hotkey: None,
            hotkey_file,
        };
        if let Err(error) = app.register_hotkey(saved, false) {
            trace(format!("hotkey-registration-error {error}"));
            app.notify(
                &format!(
                    "Không đăng ký được {}. Hãy chọn phím khác từ khay hệ thống.",
                    saved.label()
                ),
                true,
            );
        }
        APP.with(|slot| *slot.borrow_mut() = Some(app));
        trace("ready");
        let mut message = MSG::default();
        let result = loop {
            match GetMessageW(&mut message, None, 0, 0).0 {
                -1 => break Err(Error::from_thread()),
                0 => break Ok(()),
                _ => {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        };
        APP.with(|slot| slot.borrow_mut().take());
        result
    }
}

pub fn show_error(message: &str) {
    let message = wide(message);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            APP_NAME,
            MB_OK | MB_ICONERROR,
        );
    }
}

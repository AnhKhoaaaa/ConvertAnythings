use std::cell::RefCell;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::FileSystem::GetDriveTypeW;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Registry::*;
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::System::Threading::{CREATE_NO_WINDOW, CreateMutexW};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Error, PCWSTR, Ref, Result, implement, w};

use crate::convert::{self, Engines, Format};
use crate::menu::Menu;
use crate::render::{Bounds, Renderer, Theme};
use crate::{trace, wide};

const TRAY: u32 = WM_APP + 1;
const CLOSE_OVERLAY: u32 = WM_APP + 2;
const DROPPED: u32 = WM_APP + 3;
const REDRAW: u32 = WM_APP + 4;
const JOB_DONE: u32 = WM_APP + 5;
const OPEN_OVERLAY: u32 = WM_APP + 6;
const CLOSE_OVERLAY_ANY: u32 = WM_APP + 7;

const OVERLAY_TIMER: usize = 1;
const ANIM_TIMER: usize = 2;
const ANIM_DURATION_MS: f32 = 115.0;
const MAX_ACTIVE_JOBS: usize = 4;

const APP_NAME: PCWSTR = w!("ConvertAnythings");
const CONTROLLER_CLASS: PCWSTR = w!("ConvertAnythings.Controller");
const OVERLAY_CLASS: PCWSTR = w!("ConvertAnythings.Wheel");
const RUN_SUBKEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE_NAME: PCWSTR = w!("ConvertAnythings");

static HOST_HWND: AtomicUsize = AtomicUsize::new(0);
static OVERLAY_OPEN: AtomicBool = AtomicBool::new(false);
static SHIFT_DRAG_ENABLED: AtomicBool = AtomicBool::new(true);

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

const DRIVE_FIXED_TYPE: u32 = 3;

type CompletionMsg = std::result::Result<(Vec<PathBuf>, Vec<PathBuf>, Format), String>;

struct App {
    host: HWND,
    module: HINSTANCE,
    tray: NOTIFYICONDATAW,
    taskbar_created: u32,
    overlay: Option<Overlay>,
    started: Option<Instant>,
    completions: Receiver<CompletionMsg>,
    send_completion: Sender<CompletionMsg>,
    active_jobs: usize,
    running_pids: Arc<Mutex<Vec<u32>>>,
    cancelled: Arc<AtomicBool>,
    exit_requested: bool,
    hotkey: Option<(i32, Hotkey)>,
    config_dir: Option<PathBuf>,
    engines: Engines,
    theme: Theme,
    shift_drag: bool,
    recycle_originals: bool,
    last_output: Option<PathBuf>,
    kb_hook: Option<HHOOK>,
}

struct Overlay {
    window: HWND,
    drag: Rc<RefCell<Drag>>,
    renderer: Renderer,
    anim_start: Instant,
    anim_done: bool,
}

impl Overlay {
    fn anim_state(&self) -> (f32, f32) {
        if self.anim_done {
            return (1.0, 1.0);
        }
        let t = (self.anim_start.elapsed().as_secs_f32() * 1000.0 / ANIM_DURATION_MS).clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - t).powi(3);
        (0.88 + 0.12 * ease, 0.45 + 0.55 * ease)
    }
}

#[derive(Default)]
struct Drag {
    files: Vec<PathBuf>,
    dropped: Option<(Vec<PathBuf>, Format)>,
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
        let files = data.as_ref().map(files_from_drop).unwrap_or_default();
        if let Some(first) = files.first() {
            trace(format!("drag-enter {} ({})", first.display(), files.len()));
        } else {
            trace("drag-enter unsupported");
        }
        let mut drag = self.drag.borrow_mut();
        let (choices, reason) = self.engines.choices_for_files(&files);
        let subtitle = if files.is_empty() {
            "Chỉ hỗ trợ tệp".to_owned()
        } else if files.len() == 1 {
            files[0]
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_uppercase())
                .unwrap_or_else(|| "TỆP".to_owned())
        } else {
            format!("{} tệp", files.len())
        };
        drag.menu.set_choices_with_subtitle(choices, reason, subtitle);
        drag.files = files;
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
        self.drag.borrow_mut().files.clear();
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
            let files = std::mem::take(&mut drag.files);
            drag.menu.output().filter(|_| !files.is_empty()).map(|f| (files, f))
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
        set_drop_effect(effect, !drag.files.is_empty() && drag.menu.output().is_some());
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

fn files_from_drop(data: &IDataObject) -> Vec<PathBuf> {
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let mut paths = Vec::new();
    unsafe {
        let Ok(mut medium) = data.GetData(&format) else {
            return paths;
        };
        let drop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(drop, u32::MAX, None).min(200);
        for index in 0..count {
            let size = DragQueryFileW(drop, index, None);
            let mut name = vec![0u16; size as usize + 1];
            let copied = DragQueryFileW(drop, index, Some(&mut name));
            if copied > 0 {
                let path = PathBuf::from(OsString::from_wide(&name[..copied as usize]));
                if path.is_file() {
                    paths.push(path);
                }
            }
        }
        ReleaseStgMedium(&mut medium);
    }
    paths
}

unsafe extern "system" fn keyboard_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let msg = wp.0 as u32;
        if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
            let kb = unsafe { &*(lp.0 as *const KBDLLHOOKSTRUCT) };
            let vk = kb.vkCode;
            if (vk == u32::from(VK_LSHIFT.0) || vk == u32::from(VK_RSHIFT.0))
                && SHIFT_DRAG_ENABLED.load(Ordering::Relaxed)
                && !OVERLAY_OPEN.load(Ordering::Relaxed)
                && unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0
            {
                let host = HOST_HWND.load(Ordering::Relaxed);
                if host != 0 {
                    unsafe {
                        let _ = PostMessageW(
                            Some(HWND(host as *mut _)),
                            OPEN_OVERLAY,
                            WPARAM(1),
                            LPARAM(0),
                        );
                    }
                }
            } else if vk == u32::from(VK_ESCAPE.0) && OVERLAY_OPEN.load(Ordering::Relaxed) {
                let host = HOST_HWND.load(Ordering::Relaxed);
                if host != 0 {
                    unsafe {
                        let _ = PostMessageW(
                            Some(HWND(host as *mut _)),
                            CLOSE_OVERLAY_ANY,
                            WPARAM(0),
                            LPARAM(0),
                        );
                    }
                }
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
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

    fn write_config(&self, filename: &str, value: &str) -> std::io::Result<()> {
        if let Some(dir) = &self.config_dir {
            std::fs::create_dir_all(dir)?;
            std::fs::write(dir.join(filename), value)?;
        }
        Ok(())
    }

    fn install_hook(&mut self) {
        if self.kb_hook.is_some() {
            return;
        }
        unsafe {
            if let Ok(hook) =
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), Some(self.module), 0)
            {
                self.kb_hook = Some(hook);
            }
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
            &format!("ConvertAnythings ({} / Kéo+Shift)", hotkey.label()),
        );
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.tray);
        }
        if persist && let Err(error) = self.write_config("hotkey.txt", hotkey.label()) {
            self.notify(
                &format!("Phím tắt đã đổi cho phiên này. Không lưu được: {error}"),
                true,
            );
        }
        trace(format!("hotkey {}", hotkey.label()));
        Ok(())
    }

    fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        let _ = self.write_config("theme.txt", theme.as_str());
    }

    fn toggle_shift_drag(&mut self) {
        self.shift_drag = !self.shift_drag;
        SHIFT_DRAG_ENABLED.store(self.shift_drag, Ordering::Relaxed);
        let _ = self.write_config("shift_drag.txt", if self.shift_drag { "1" } else { "0" });
    }

    fn toggle_recycle_originals(&mut self) {
        self.recycle_originals = !self.recycle_originals;
        let _ = self.write_config(
            "recycle.txt",
            if self.recycle_originals { "1" } else { "0" },
        );
    }

    fn cancel_active_jobs(&self) {
        if self.active_jobs == 0 {
            return;
        }
        self.cancelled.store(true, Ordering::Relaxed);
        let pids: Vec<u32> = self
            .running_pids
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        for pid in pids {
            let mut cmd = std::process::Command::new("taskkill.exe");
            cmd.creation_flags(CREATE_NO_WINDOW.0)
                .args(["/F", "/T", "/PID", &pid.to_string()]);
            let _ = cmd.status();
        }
    }

    fn close_overlay(&mut self) {
        OVERLAY_OPEN.store(false, Ordering::Relaxed);
        if let Some(overlay) = self.overlay.take() {
            unsafe {
                let _ = KillTimer(Some(self.host), OVERLAY_TIMER);
                let _ = KillTimer(Some(self.host), ANIM_TIMER);
                let _ = RevokeDragDrop(overlay.window);
                let _ = DestroyWindow(overlay.window);
            }
            trace("overlay-closed");
        }
    }

    fn open_overlay(&mut self, from_shift: bool) -> Result<()> {
        if self.active_jobs >= MAX_ACTIVE_JOBS {
            if !from_shift {
                self.notify("Đang xử lý tối đa 4 tác vụ. Hãy đợi hoàn tất.", false);
            }
            return Ok(());
        }
        if self.overlay.is_some() {
            if !from_shift {
                self.close_overlay();
            }
            return Ok(());
        }
        self.started = Some(Instant::now());
        unsafe {
            let mut pointer = POINT { x: 400, y: 300 };
            let _ = GetCursorPos(&mut pointer);
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
                ((320 * dpi / 96) as i32).min((work.right - work.left).min(work.bottom - work.top));
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
                engines: self.engines.clone(),
            }
            .into();
            let anim_start = Instant::now();
            if let Err(error) = renderer
                .draw(window, &drag.borrow().menu, self.theme, 0.88, 0.45)
                .and_then(|_| RegisterDragDrop(window, &target))
            {
                let _ = DestroyWindow(window);
                return Err(error);
            }
            self.overlay = Some(Overlay {
                window,
                drag,
                renderer,
                anim_start,
                anim_done: false,
            });
            OVERLAY_OPEN.store(true, Ordering::Relaxed);
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
            trace(format!("overlay-open {x} {y} {size} {dpi}"));
            if let Some(start) = self.started {
                trace(format!("menu-latency-us {}", start.elapsed().as_micros()));
            }
            SetTimer(Some(self.host), OVERLAY_TIMER, 100, None);
            SetTimer(Some(self.host), ANIM_TIMER, 16, None);
            Ok(())
        }
    }

    fn finish_drop(&mut self) {
        let job = self
            .overlay
            .as_ref()
            .and_then(|overlay| overlay.drag.borrow_mut().dropped.take());
        self.close_overlay();
        if let Some((files, format)) = job {
            trace(format!("drop {} files -> {}", files.len(), format.extension()));
            self.active_jobs += 1;
            let host = self.host.0 as usize;
            let send = self.send_completion.clone();
            let running_pids = Arc::clone(&self.running_pids);
            let cancelled = Arc::clone(&self.cancelled);
            let task = std::thread::Builder::new()
                .name("convert-task".to_owned())
                .spawn(move || {
                    let result = convert::child().and_then(|mut command| {
                        command
                            .args(["--convert", "--to", format.extension()])
                            .args(&files)
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::piped());
                        let child = command.spawn()?;
                        let pid = child.id();
                        if let Ok(mut guard) = running_pids.lock() {
                            guard.push(pid);
                        }
                        let output = child.wait_with_output();
                        if let Ok(mut guard) = running_pids.lock() {
                            guard.retain(|&id| id != pid);
                        }
                        output
                    });
                    let result = match result {
                        Ok(output) if output.status.success() && !cancelled.load(Ordering::Relaxed) => {
                            let paths: Vec<PathBuf> = String::from_utf8_lossy(&output.stdout)
                                .lines()
                                .map(str::trim)
                                .filter(|line| !line.is_empty())
                                .map(PathBuf::from)
                                .collect();
                            Ok((files, paths, format))
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
                self.active_jobs = self.active_jobs.saturating_sub(1);
                self.notify(&error.to_string(), true);
            }
        }
    }

    fn finish_job(&mut self) {
        let Ok(result) = self.completions.try_recv() else {
            return;
        };
        self.active_jobs = self.active_jobs.saturating_sub(1);
        let was_cancelled = self.cancelled.load(Ordering::Relaxed);
        if self.active_jobs == 0 {
            self.cancelled.store(false, Ordering::Relaxed);
        }
        match result {
            Ok((inputs, paths, format)) => {
                if let Some(last) = paths.last() {
                    self.last_output = Some(last.clone());
                }
                if self.recycle_originals
                    && !paths.is_empty()
                    && paths.iter().all(|p| p.exists())
                {
                    for input in &inputs {
                        if !paths.contains(input) {
                            let _ = move_to_recycle_bin(input);
                        }
                    }
                }
                let fallback_note = inputs
                    .first()
                    .and_then(|i| i.parent())
                    .zip(paths.first().and_then(|o| o.parent()))
                    .filter(|(src_dir, out_dir)| src_dir != out_dir)
                    .and_then(|(_, out_dir)| out_dir.file_name().and_then(|n| n.to_str()))
                    .map(|folder| format!(" ({folder})"))
                    .unwrap_or_default();

                if paths.len() == 1 {
                    let path = &paths[0];
                    trace(format!("job-ok {}", path.display()));
                    let name = path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .map(str::to_owned)
                        .unwrap_or_else(|| path.display().to_string());
                    self.notify(&format!("Đã lưu {name}{fallback_note}"), false);
                } else {
                    trace(format!("job-ok {} files", paths.len()));
                    self.notify(
                        &format!(
                            "Đã chuyển {} tệp → {}{fallback_note}",
                            paths.len(),
                            format.label()
                        ),
                        false,
                    );
                }
            }
            Err(_) if was_cancelled => {
                trace("job-cancelled");
                self.notify("Đã hủy tác vụ chuyển đổi.", false);
            }
            Err(error) => {
                trace(format!("job-error {error}"));
                self.notify(&error, true);
            }
        }
        if self.exit_requested && self.active_jobs == 0 {
            unsafe {
                let _ = PostMessageW(Some(self.host), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }

    fn request_exit(&mut self) -> bool {
        if self.active_jobs > 0 {
            self.exit_requested = true;
            self.notify("Ứng dụng sẽ thoát sau khi chuyển tệp xong.", false);
        }
        self.active_jobs == 0
    }

    fn redraw(&mut self, window: usize) {
        if let Some(overlay) = &self.overlay
            && overlay.window.0 as usize == window
        {
            let (scale, alpha) = overlay.anim_state();
            if let Err(error) = overlay.renderer.draw(
                overlay.window,
                &overlay.drag.borrow().menu,
                self.theme,
                scale,
                alpha,
            ) {
                trace(format!("render-error {error}"));
            } else {
                trace("overlay-redraw");
            }
        }
    }

    fn anim_tick(&mut self) {
        let Some(overlay) = &mut self.overlay else {
            unsafe {
                let _ = KillTimer(Some(self.host), ANIM_TIMER);
            }
            return;
        };
        let elapsed = overlay.anim_start.elapsed().as_secs_f32() * 1000.0;
        let finished = elapsed >= ANIM_DURATION_MS;
        if finished {
            overlay.anim_done = true;
            unsafe {
                let _ = KillTimer(Some(self.host), ANIM_TIMER);
            }
        }
        let (scale, alpha) = overlay.anim_state();
        let _ = overlay.renderer.draw(
            overlay.window,
            &overlay.drag.borrow().menu,
            self.theme,
            scale,
            alpha,
        );
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

    fn open_last_output_in_explorer(&self) {
        let Some(path) = &self.last_output else {
            return;
        };
        let arg = format!("/select,\"{}\"", path.display());
        let mut cmd = std::process::Command::new("explorer.exe");
        cmd.creation_flags(CREATE_NO_WINDOW.0).raw_arg(arg);
        let _ = cmd.spawn();
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

fn move_to_recycle_bin(path: &Path) -> bool {
    let Ok(abs) = std::path::absolute(path) else {
        return false;
    };
    let Some(root) = abs.components().next() else {
        return false;
    };
    let mut root_str = root.as_os_str().to_os_string();
    root_str.push("\\");
    let root_wide = wide(&root_str);
    unsafe {
        if GetDriveTypeW(PCWSTR(root_wide.as_ptr())) != DRIVE_FIXED_TYPE {
            return false;
        }
    }
    let mut from = wide(abs.as_os_str());
    from.push(0);
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        ..Default::default()
    };
    unsafe { SHFileOperationW(&mut op) == 0 && !op.fAnyOperationsAborted.as_bool() }
}

fn autostart_enabled() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_SUBKEY, Some(0), KEY_READ, &mut hkey).is_err() {
            return false;
        }
        let exists = RegQueryValueExW(hkey, RUN_VALUE_NAME, None, None, None, None).is_ok();
        let _ = RegCloseKey(hkey);
        exists
    }
}

fn set_autostart(enable: bool) -> std::result::Result<(), String> {
    unsafe {
        let mut hkey = HKEY::default();
        RegOpenKeyExW(HKEY_CURRENT_USER, RUN_SUBKEY, Some(0), KEY_SET_VALUE, &mut hkey)
            .ok()
            .map_err(|e| e.to_string())?;
        let res = if enable {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let quoted = wide(format!("\"{}\"", exe.display()));
            let byte_len = quoted.len() * 2;
            let bytes = std::slice::from_raw_parts(quoted.as_ptr() as *const u8, byte_len);
            RegSetValueExW(hkey, RUN_VALUE_NAME, Some(0), REG_SZ, Some(bytes))
                .ok()
                .map_err(|e| e.to_string())
        } else {
            let _ = RegDeleteValueW(hkey, RUN_VALUE_NAME);
            Ok(())
        };
        let _ = RegCloseKey(hkey);
        res
    }
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
            let from_shift = message == OPEN_OVERLAY && wp.0 == 1;
            with_app(|app| {
                if let Err(error) = app.open_overlay(from_shift) {
                    trace(format!("overlay-error {error}"));
                }
            });
        }
        REDRAW => with_app(|app| app.redraw(wp.0)),
        JOB_DONE => with_app(App::finish_job),
        WM_TIMER if wp.0 == OVERLAY_TIMER => with_app(App::timer),
        WM_TIMER if wp.0 == ANIM_TIMER => with_app(App::anim_tick),
        CLOSE_OVERLAY_ANY => with_app(App::close_overlay),
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
                    if let Some(hook) = app.kb_hook.take() {
                        let _ = UnhookWindowsHookEx(hook);
                    }
                    let _ = Shell_NotifyIconW(NIM_DELETE, &app.tray);
                    if let Some((id, _)) = app.hotkey {
                        let _ = UnregisterHotKey(Some(hwnd), id);
                    }
                }
            });
            HOST_HWND.store(0, Ordering::Relaxed);
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
        let mut has_last = false;
        let mut has_active = false;
        let mut current_hotkey = None;
        let mut current_theme = Theme::Tangerine;
        let mut shift_drag = true;
        let mut recycle_originals = false;
        with_app(|app| {
            has_last = app.last_output.as_ref().is_some_and(|p| p.exists());
            has_active = app.active_jobs > 0;
            current_hotkey = app.hotkey.map(|(_, key)| key);
            current_theme = app.theme;
            shift_drag = app.shift_drag;
            recycle_originals = app.recycle_originals;
        });

        let _ = AppendMenuW(menu, MF_STRING, 1, w!("Mở vùng thả"));
        let _ = AppendMenuW(
            menu,
            MF_STRING | if has_last { MF_ENABLED } else { MF_GRAYED },
            3,
            w!("Mở vị trí tệp vừa chuyển"),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING | if has_active { MF_ENABLED } else { MF_GRAYED },
            7,
            w!("Hủy tác vụ đang chạy"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);

        if let Ok(themes) = CreatePopupMenu() {
            for (index, theme) in Theme::OPTIONS.into_iter().enumerate() {
                let label = wide(theme.label());
                let style = if current_theme == theme {
                    MF_CHECKED
                } else {
                    MF_UNCHECKED
                };
                let _ = AppendMenuW(themes, MF_STRING | style, 20 + index, PCWSTR(label.as_ptr()));
            }
            if AppendMenuW(menu, MF_POPUP, themes.0 as usize, w!("Giao diện")).is_err() {
                let _ = DestroyMenu(themes);
            }
        }

        if let Ok(keys) = CreatePopupMenu() {
            for (index, key) in Hotkey::OPTIONS.into_iter().enumerate() {
                let label = wide(key.label());
                let style = if current_hotkey == Some(key) {
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

        let _ = AppendMenuW(
            menu,
            MF_STRING | if shift_drag { MF_CHECKED } else { MF_UNCHECKED },
            4,
            w!("Mở khi kéo + Shift"),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING
                | if recycle_originals {
                    MF_CHECKED
                } else {
                    MF_UNCHECKED
                },
            8,
            w!("Đưa tệp gốc vào Thùng rác sau khi chuyển"),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING
                | if autostart_enabled() {
                    MF_CHECKED
                } else {
                    MF_UNCHECKED
                },
            5,
            w!("Khởi động cùng Windows"),
        );
        let _ = AppendMenuW(menu, MF_STRING, 6, w!("Quét lại bộ chuyển đổi"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, 2, w!("Thoát"));

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
            3 => with_app(|app| app.open_last_output_in_explorer()),
            4 => with_app(App::toggle_shift_drag),
            5 => {
                let enable = !autostart_enabled();
                if let Err(err) = set_autostart(enable) {
                    with_app(|app| app.notify(&format!("Không đổi được khởi động cùng Windows: {err}"), true));
                }
            }
            6 => with_app(|app| {
                app.engines = Engines::detect();
                app.notify("Đã quét lại các bộ chuyển đổi trên máy.", false);
            }),
            7 => with_app(|app| app.cancel_active_jobs()),
            8 => with_app(App::toggle_recycle_originals),
            10..=12 => with_app(|app| {
                let key = Hotkey::OPTIONS[(selected - 10) as usize];
                if let Err(error) = app.register_hotkey(key, true) {
                    app.notify(
                        &format!("Không đăng ký được {}. {error}", key.label()),
                        true,
                    );
                }
            }),
            20..=21 => with_app(|app| {
                let theme = Theme::OPTIONS[(selected - 20) as usize];
                app.set_theme(theme);
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
        let cx_icon = GetSystemMetrics(SM_CXICON).max(32);
        let cy_icon = GetSystemMetrics(SM_CYICON).max(32);
        let cx_sm = GetSystemMetrics(SM_CXSMICON).max(16);
        let cy_sm = GetSystemMetrics(SM_CYSMICON).max(16);
        let fallback_icon = LoadIconW(None, IDI_APPLICATION)?;
        #[allow(clippy::manual_dangling_ptr)]
        let res_icon = PCWSTR(1usize as *const u16);
        let app_icon = LoadImageW(
            Some(module),
            res_icon,
            IMAGE_ICON,
            cx_icon,
            cy_icon,
            LR_DEFAULTCOLOR,
        )
        .map(|h| HICON(h.0))
        .unwrap_or(fallback_icon);
        let tray_icon = LoadImageW(
            Some(module),
            res_icon,
            IMAGE_ICON,
            cx_sm,
            cy_sm,
            LR_DEFAULTCOLOR,
        )
        .map(|h| HICON(h.0))
        .unwrap_or(app_icon);
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
                hIcon: app_icon,
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
        HOST_HWND.store(host.0 as usize, Ordering::Relaxed);
        let mut tray = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: host,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY,
            hIcon: tray_icon,
            ..Default::default()
        };
        copy_notification(&mut tray.szTip, "ConvertAnythings");
        if Shell_NotifyIconW(NIM_ADD, &tray).ok().is_err() {
            trace("tray-add-deferred");
        }
        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        let (send_completion, completions) = std::sync::mpsc::channel();
        let cfg = crate::config_dir();
        let read_cfg = |name: &str| -> Option<String> {
            cfg.as_ref()
                .and_then(|dir| std::fs::read_to_string(dir.join(name)).ok())
        };
        let saved_hotkey = read_cfg("hotkey.txt");
        let saved_hotkey = Hotkey::OPTIONS
            .into_iter()
            .find(|key| Some(key.label()) == saved_hotkey.as_deref().map(str::trim))
            .unwrap_or(Hotkey::F8);
        let saved_theme = read_cfg("theme.txt")
            .as_deref()
            .and_then(Theme::parse)
            .unwrap_or_default();
        let shift_drag = read_cfg("shift_drag.txt")
            .map(|v| v.trim() != "0")
            .unwrap_or(true);
        let recycle_originals = read_cfg("recycle.txt")
            .map(|v| v.trim() == "1")
            .unwrap_or(false);
        SHIFT_DRAG_ENABLED.store(shift_drag, Ordering::Relaxed);

        let mut app = App {
            host,
            module,
            tray,
            taskbar_created,
            overlay: None,
            started: None,
            completions,
            send_completion,
            active_jobs: 0,
            running_pids: Arc::new(Mutex::new(Vec::new())),
            cancelled: Arc::new(AtomicBool::new(false)),
            exit_requested: false,
            hotkey: None,
            config_dir: cfg,
            engines: Engines::detect(),
            theme: saved_theme,
            shift_drag,
            recycle_originals,
            last_output: None,
            kb_hook: None,
        };
        app.install_hook();
        if let Err(error) = app.register_hotkey(saved_hotkey, false) {
            trace(format!("hotkey-registration-error {error}"));
            app.notify(
                &format!(
                    "Không đăng ký được {}. Hãy chọn phím khác từ khay hệ thống.",
                    saved_hotkey.label()
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

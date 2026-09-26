#![windows_subsystem = "windows"]

mod nvml;
mod pawnio;
mod sensors;
mod settings;

use std::cell::RefCell;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use nvml::GpuReading;

// ---------------------------------------------------------------- constants

const TIMER_ID: usize = 1;
const INTERVAL_MS: u32 = 1500;
const WM_TRAY: u32 = WM_APP + 1;
const WM_SHOW_MENU: u32 = WM_APP + 2;

const ID_TOGGLE: usize = 1;
const ID_TOPMOST: usize = 2;
const ID_AUTOSTART: usize = 3;
const ID_RESET: usize = 4;
const ID_EXIT: usize = 9;

// Layout, in DIPs (scaled by monitor DPI)
const WIDTH: f32 = 280.0;
const PAD_X: f32 = 16.0;
const PAD_TOP: f32 = 14.0;
const PAD_BOTTOM: f32 = 14.0;
const LABEL_H: f32 = 16.0;
const BIG_H: f32 = 32.0;
const BAR_GAP: f32 = 4.0;
const BAR_H: f32 = 5.0;
const DETAIL_H: f32 = 20.0;
const SECTION_GAP: f32 = 14.0;
const HINT_H: f32 = 40.0;
const EDGE_MARGIN: f32 = 12.0;

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

const BG: COLORREF = rgb(0x16, 0x18, 0x1D);
const BORDER: COLORREF = rgb(0x2E, 0x31, 0x38);
const TRACK: COLORREF = rgb(0x39, 0x3B, 0x3F);
const LABEL: COLORREF = rgb(0x9A, 0xA1, 0xAB);
const MUTED: COLORREF = rgb(0x6E, 0x76, 0x81);
const TEXT: COLORREF = rgb(0xF0, 0xF3, 0xF6);
const CPU_ACCENT: COLORREF = rgb(0x4C, 0xC2, 0xFF);
const GPU_ACCENT: COLORREF = rgb(0x7E, 0xE7, 0x87);
const RAM_ACCENT: COLORREF = rgb(0xD2, 0xA8, 0xFF);
const COOL: COLORREF = rgb(0x7E, 0xE7, 0x87); // < 65 C
const WARM: COLORREF = rgb(0xE3, 0xB3, 0x41); // 65-79 C
const HOT: COLORREF = rgb(0xFF, 0x7B, 0x72); // >= 80 C
const HINT: COLORREF = rgb(0xD2, 0x99, 0x22);

// ---------------------------------------------------------------- state

struct State {
    dpi: u32,
    fonts: Fonts,
    settings: settings::Settings,
    autostart: bool,
    tray_icon: HICON,

    cpu_name: String,
    cpu_load: sensors::CpuLoad,
    cpu_temp_sensor: Option<pawnio::IntelTemp>,
    gpu: nvml::Gpu,

    cpu_load_pct: Option<f32>,
    cpu_temp: Option<f32>,
    gpu_reading: GpuReading,
    ram: sensors::Ram,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// Borrow the state briefly. Never call anything that can re-enter the window
/// procedure (SetWindowPos, TrackPopupMenu, MessageBox...) inside the closure.
fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------- fonts

struct Fonts {
    label: HFONT,
    small: HFONT,
    big: HFONT,
    temp: HFONT,
    mid: HFONT,
    hint: HFONT,
}

impl Fonts {
    fn new(dpi: u32) -> Self {
        Fonts {
            label: font("Segoe UI Semibold", 11.0, dpi),
            small: font("Segoe UI", 11.0, dpi),
            big: font("Segoe UI Semibold", 24.0, dpi),
            temp: font("Segoe UI Semibold", 18.0, dpi),
            mid: font("Segoe UI Semibold", 14.0, dpi),
            hint: font("Segoe UI", 10.5, dpi),
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        for f in [self.label, self.small, self.big, self.temp, self.mid, self.hint] {
            unsafe { DeleteObject(f) };
        }
    }
}

fn font(face: &str, size_dip: f32, dpi: u32) -> HFONT {
    unsafe {
        let mut lf: LOGFONTW = zeroed();
        lf.lfHeight = -((size_dip * dpi as f32 / 96.0).round() as i32);
        lf.lfWeight = FW_NORMAL as i32;
        lf.lfQuality = CLEARTYPE_QUALITY as u8;
        for (dst, src) in lf.lfFaceName.iter_mut().zip(face.encode_utf16().take(31)) {
            *dst = src;
        }
        CreateFontIndirectW(&lf)
    }
}

// ---------------------------------------------------------------- main

fn main() {
    unsafe {
        let _mutex = CreateMutexW(null(), 1, wide(r"Local\SysWidget.SingleInstance").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        TASKBAR_CREATED.store(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()), Ordering::Relaxed);

        let hinstance = GetModuleHandleW(null());
        let class = wide("SysWidgetWindow");
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: class.as_ptr(),
            ..zeroed()
        };
        RegisterClassExW(&wc);

        let settings = settings::Settings::load();
        let ex_style = WS_EX_TOOLWINDOW | if settings.topmost { WS_EX_TOPMOST } else { 0 };
        let hwnd = CreateWindowExW(
            ex_style,
            class.as_ptr(),
            wide("SysWidget").as_ptr(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            null_mut(),
            null_mut(),
            hinstance,
            null(),
        );
        if hwnd.is_null() {
            return;
        }

        // Windows 11 rounded corners + thin border
        let corner = DWMWCP_ROUND;
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE as _, (&corner as *const i32).cast(), 4);
        let border = BORDER;
        DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR as _, (&border as *const u32).cast(), 4);

        let dpi = GetDpiForWindow(hwnd);
        let saved_pos = settings.pos;
        let state = State {
            dpi,
            fonts: Fonts::new(dpi),
            settings,
            autostart: settings::autostart_enabled(),
            tray_icon: load_tray_icon(dpi),
            cpu_name: sensors::cpu_name(),
            cpu_load: sensors::CpuLoad::default(),
            cpu_temp_sensor: pawnio::IntelTemp::open(),
            gpu: nvml::Gpu::new(),
            cpu_load_pct: None,
            cpu_temp: None,
            gpu_reading: GpuReading::Unavailable,
            ram: sensors::Ram::default(),
        };
        let icon = state.tray_icon;
        STATE.with(|s| *s.borrow_mut() = Some(state));

        refresh(hwnd);
        let (w, h) = with(size_for).unwrap_or((1, 1));
        let (x, y) = match saved_pos {
            Some((x, y)) if !MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONULL).is_null() => (x, y),
            _ => default_position(w, dpi),
        };
        SetWindowPos(hwnd, null_mut(), x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        tray(hwnd, icon, NIM_ADD);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(hwnd, TIMER_ID, INTERVAL_MS, None);

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        STATE.with(|s| s.borrow_mut().take()); // closes NVML / PawnIO cleanly
    }
}

// ---------------------------------------------------------------- window procedure

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == TASKBAR_CREATED.load(Ordering::Relaxed) && msg != 0 {
        // Explorer restarted - put the tray icon back
        if let Some(icon) = with(|st| st.tray_icon) {
            tray(hwnd, icon, NIM_ADD);
        }
        return 0;
    }

    match msg {
        WM_TIMER => {
            if IsWindowVisible(hwnd) != 0 {
                refresh(hwnd);
            }
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc: RECT = zeroed();
            GetClientRect(hwnd, &mut rc);
            // Double-buffered to avoid flicker
            let mem = CreateCompatibleDC(hdc);
            let bmp = CreateCompatibleBitmap(hdc, rc.right, rc.bottom);
            let old = SelectObject(mem, bmp);
            with(|st| paint(st, mem, rc.right, rc.bottom));
            BitBlt(hdc, 0, 0, rc.right, rc.bottom, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old);
            DeleteObject(bmp);
            DeleteDC(mem);
            EndPaint(hwnd, &ps);
            0
        }
        WM_ERASEBKGND => 1,
        // The whole widget acts as a title bar, so it can be dragged anywhere.
        WM_NCHITTEST => {
            let hit = DefWindowProcW(hwnd, msg, wparam, lparam);
            if hit == HTCLIENT as LRESULT { HTCAPTION as LRESULT } else { hit }
        }
        WM_NCLBUTTONDBLCLK => 0, // no maximise on double-click
        // The widget reports itself as a caption, so right-clicks arrive as non-client
        // messages. Swallow the button-down (otherwise Windows starts its own caption
        // handling) and open our menu on button-up. WM_CONTEXTMENU covers the keyboard.
        WM_NCRBUTTONDOWN => 0,
        // The menu is opened from a posted message, after mouse handling has finished.
        WM_NCRBUTTONUP | WM_CONTEXTMENU => {
            PostMessageW(hwnd, WM_SHOW_MENU, 0, 0);
            0
        }
        WM_SHOW_MENU => {
            show_menu(hwnd);
            0
        }
        WM_EXITSIZEMOVE => {
            save_position(hwnd);
            0
        }
        WM_DPICHANGED => {
            let dpi = (wparam & 0xFFFF) as u32;
            let suggested = &*(lparam as *const RECT);
            if let Some((w, h)) = with(|st| {
                st.dpi = dpi;
                st.fonts = Fonts::new(dpi);
                size_for(st)
            }) {
                SetWindowPos(hwnd, null_mut(), suggested.left, suggested.top, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
            }
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_TRAY => {
            match (lparam as u32) & 0xFFFF {
                WM_RBUTTONUP => show_menu(hwnd),
                WM_LBUTTONDBLCLK => toggle_visible(hwnd),
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            save_position(hwnd);
            if let Some(icon) = with(|st| st.tray_icon) {
                tray(hwnd, icon, NIM_DELETE);
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ---------------------------------------------------------------- sensors -> screen

unsafe fn refresh(hwnd: HWND) {
    let Some((w, h)) = with(|st| {
        st.cpu_load_pct = st.cpu_load.sample();
        st.cpu_temp = st.cpu_temp_sensor.as_ref().and_then(|t| t.package_temp());
        st.gpu_reading = st.gpu.poll(sensors::on_battery());
        st.ram = sensors::ram();
        size_for(st)
    }) else {
        return;
    };

    let mut rc: RECT = zeroed();
    GetWindowRect(hwnd, &mut rc);
    if rc.right - rc.left != w || rc.bottom - rc.top != h {
        SetWindowPos(hwnd, null_mut(), 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
    InvalidateRect(hwnd, null(), 0);
}

fn scale(dpi: u32, v: f32) -> i32 {
    (v * dpi as f32 / 96.0).round() as i32
}

fn size_for(st: &mut State) -> (i32, i32) {
    let section = LABEL_H + BIG_H + BAR_GAP + BAR_H;
    let mut h = PAD_TOP + 3.0 * section + DETAIL_H + 2.0 * SECTION_GAP + PAD_BOTTOM;
    if st.cpu_temp.is_none() {
        h += HINT_H;
    }
    (scale(st.dpi, WIDTH), scale(st.dpi, h))
}

struct Section<'a> {
    label: &'a str,
    device: &'a str,
    big: String,
    right: String,
    right_color: COLORREF,
    right_font: HFONT,
    bar: Option<f32>,
    accent: COLORREF,
    detail: Option<String>,
}

unsafe fn paint(st: &State, dc: HDC, w: i32, h: i32) {
    let px = |v: f32| scale(st.dpi, v);
    fill(dc, RECT { left: 0, top: 0, right: w, bottom: h }, BG);
    SetBkMode(dc, TRANSPARENT as _);

    let (x0, x1) = (px(PAD_X), w - px(PAD_X));
    let mut y = px(PAD_TOP);
    let f = &st.fonts;

    draw_section(dc, st, &mut y, x0, x1, Section {
        label: "CPU",
        device: &st.cpu_name,
        big: percent(st.cpu_load_pct),
        right: temp_text(st.cpu_temp),
        right_color: temp_color(st.cpu_temp),
        right_font: f.temp,
        bar: st.cpu_load_pct,
        accent: CPU_ACCENT,
        detail: None,
    });
    y += px(SECTION_GAP);

    let gpu_name = if st.gpu.name.is_empty() { "NVIDIA GPU" } else { st.gpu.name.as_str() };
    let (load, temp, detail) = match st.gpu_reading {
        GpuReading::Active { load, temp, vram } => (
            load,
            temp,
            match vram {
                Some((used, total)) => format!("VRAM  {used:.1} / {total:.1} GB"),
                None => "VRAM  --".to_string(),
            },
        ),
        GpuReading::Sleeping => (None, None, "Sleeping (idle)".to_string()),
        GpuReading::PausedOnBattery => (None, None, "Paused on battery".to_string()),
        GpuReading::Unavailable => (None, None, "Not available (GPU off / Eco mode)".to_string()),
    };
    draw_section(dc, st, &mut y, x0, x1, Section {
        label: "GPU",
        device: gpu_name,
        big: percent(load),
        right: temp_text(temp),
        right_color: temp_color(temp),
        right_font: f.temp,
        bar: load,
        accent: GPU_ACCENT,
        detail: Some(detail),
    });
    y += px(SECTION_GAP);

    draw_section(dc, st, &mut y, x0, x1, Section {
        label: "MEMORY",
        device: "",
        big: percent(Some(st.ram.load)),
        right: format!("{:.1} / {:.1} GB", st.ram.used_gb, st.ram.total_gb),
        right_color: LABEL,
        right_font: f.mid,
        bar: Some(st.ram.load),
        accent: RAM_ACCENT,
        detail: None,
    });

    if st.cpu_temp.is_none() {
        let text = if st.cpu_temp_sensor.is_none() {
            "CPU temperature unavailable - PawnIO driver not found (pawnio.eu)."
        } else {
            "CPU temperature could not be read."
        };
        let rc = RECT { left: x0, top: y + px(10.0), right: x1, bottom: h };
        draw_text(dc, text, rc, f.hint, HINT, DT_LEFT | DT_WORDBREAK);
    }
}

unsafe fn draw_section(dc: HDC, st: &State, y: &mut i32, x0: i32, x1: i32, s: Section) {
    let px = |v: f32| scale(st.dpi, v);
    let f = &st.fonts;

    let row = RECT { left: x0, top: *y, right: x1, bottom: *y + px(LABEL_H) };
    draw_text(dc, s.label, row, f.label, LABEL, DT_LEFT | DT_SINGLELINE);
    if !s.device.is_empty() {
        let rc = RECT { left: x0 + px(70.0), ..row };
        draw_text(dc, s.device, rc, f.small, MUTED, DT_RIGHT | DT_SINGLELINE | DT_END_ELLIPSIS);
    }
    *y += px(LABEL_H);

    let big = RECT { left: x0, top: *y, right: x1, bottom: *y + px(BIG_H) };
    draw_text(dc, &s.big, big, f.big, TEXT, DT_LEFT | DT_SINGLELINE | DT_BOTTOM);
    let right = RECT { bottom: big.bottom - px(3.0), ..big };
    draw_text(dc, &s.right, right, s.right_font, s.right_color, DT_RIGHT | DT_SINGLELINE | DT_BOTTOM);
    *y += px(BIG_H + BAR_GAP);

    let track = RECT { left: x0, top: *y, right: x1, bottom: *y + px(BAR_H) };
    rounded(dc, track, TRACK, px(BAR_H));
    if let Some(p) = s.bar {
        let fill_w = ((x1 - x0) as f32 * p.clamp(0.0, 100.0) / 100.0).round() as i32;
        if fill_w > 0 {
            rounded(dc, RECT { right: x0 + fill_w.max(px(BAR_H)), ..track }, s.accent, px(BAR_H));
        }
    }
    *y += px(BAR_H);

    if let Some(detail) = s.detail {
        let rc = RECT { left: x0, top: *y + px(5.0), right: x1, bottom: *y + px(DETAIL_H) };
        draw_text(dc, &detail, rc, f.small, MUTED, DT_LEFT | DT_SINGLELINE);
        *y += px(DETAIL_H);
    }
}

fn percent(v: Option<f32>) -> String {
    v.map_or_else(|| "--".into(), |v| format!("{v:.0}%"))
}

fn temp_text(t: Option<f32>) -> String {
    match t {
        Some(t) if t > 0.0 => format!("{t:.0}°C"),
        _ => "--°C".into(),
    }
}

fn temp_color(t: Option<f32>) -> COLORREF {
    match t {
        Some(t) if t >= 80.0 => HOT,
        Some(t) if t >= 65.0 => WARM,
        Some(t) if t > 0.0 => COOL,
        _ => MUTED,
    }
}

// ---------------------------------------------------------------- GDI helpers

unsafe fn draw_text(dc: HDC, text: &str, mut rc: RECT, font: HFONT, color: COLORREF, flags: DRAW_TEXT_FORMAT) {
    let mut w: Vec<u16> = text.encode_utf16().collect();
    let old = SelectObject(dc, font);
    SetTextColor(dc, color);
    DrawTextW(dc, w.as_mut_ptr(), w.len() as i32, &mut rc, flags | DT_NOPREFIX);
    SelectObject(dc, old);
}

unsafe fn fill(dc: HDC, rc: RECT, color: COLORREF) {
    let brush = CreateSolidBrush(color);
    FillRect(dc, &rc, brush);
    DeleteObject(brush);
}

unsafe fn rounded(dc: HDC, rc: RECT, color: COLORREF, radius: i32) {
    let brush = CreateSolidBrush(color);
    let old_brush = SelectObject(dc, brush);
    let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
    RoundRect(dc, rc.left, rc.top, rc.right + 1, rc.bottom + 1, radius, radius);
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(brush);
}

// ---------------------------------------------------------------- position

unsafe fn default_position(width: i32, dpi: u32) -> (i32, i32) {
    let mut work: RECT = zeroed();
    SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut work as *mut RECT).cast(), 0);
    let margin = scale(dpi, EDGE_MARGIN);
    (work.right - width - margin, work.top + margin)
}

unsafe fn save_position(hwnd: HWND) {
    let mut rc: RECT = zeroed();
    GetWindowRect(hwnd, &mut rc);
    with(|st| {
        st.settings.pos = Some((rc.left, rc.top));
        st.settings.save();
    });
}

unsafe fn toggle_visible(hwnd: HWND) {
    if IsWindowVisible(hwnd) != 0 {
        ShowWindow(hwnd, SW_HIDE);
    } else {
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        refresh(hwnd);
    }
}

// ---------------------------------------------------------------- tray + menu

unsafe fn tray(hwnd: HWND, icon: HICON, action: NOTIFY_ICON_MESSAGE) {
    let mut nid: NOTIFYICONDATAW = zeroed();
    nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = icon;
    for (dst, src) in nid.szTip.iter_mut().zip("SysWidget".encode_utf16()) {
        *dst = src;
    }
    Shell_NotifyIconW(action, &nid);
}

unsafe fn show_menu(hwnd: HWND) {
    let Some((topmost, autostart)) = with(|st| (st.settings.topmost, st.autostart)) else { return };
    let visible = IsWindowVisible(hwnd) != 0;

    let menu = CreatePopupMenu();
    let add = |id: usize, text: &str, checked: bool| {
        let flags = MF_STRING | if checked { MF_CHECKED } else { MF_UNCHECKED };
        AppendMenuW(menu, flags, id, wide(text).as_ptr());
    };
    add(ID_TOGGLE, if visible { "Hide widget" } else { "Show widget" }, false);
    add(ID_TOPMOST, "Always on top", topmost);
    add(ID_AUTOSTART, "Start with Windows", autostart);
    add(ID_RESET, "Reset position", false);
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    add(ID_EXIT, "Exit", false);

    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd); // required so the menu closes when clicking elsewhere
    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, 0, hwnd, null());
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(menu);

    match cmd as usize {
        ID_TOGGLE => toggle_visible(hwnd),
        ID_TOPMOST => {
            let on = !topmost;
            with(|st| {
                st.settings.topmost = on;
                st.settings.save();
            });
            let after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
            SetWindowPos(hwnd, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        ID_AUTOSTART => {
            let result = if autostart {
                settings::disable_autostart();
                Ok(())
            } else {
                settings::enable_autostart()
            };
            let now = settings::autostart_enabled();
            with(|st| st.autostart = now);
            if let Err(e) = result {
                MessageBoxW(hwnd, wide(&e).as_ptr(), wide("SysWidget").as_ptr(), MB_OK | MB_ICONWARNING);
            }
        }
        ID_RESET => {
            let mut rc: RECT = zeroed();
            GetWindowRect(hwnd, &mut rc);
            let dpi = GetDpiForWindow(hwnd);
            let (x, y) = default_position(rc.right - rc.left, dpi);
            SetWindowPos(hwnd, null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            save_position(hwnd);
        }
        ID_EXIT => {
            DestroyWindow(hwnd);
        }
        _ => {}
    }
}

/// The app icon embedded from assets/syswidget.ico, at the tray's size for this DPI.
unsafe fn load_tray_icon(dpi: u32) -> HICON {
    let size = GetSystemMetricsForDpi(SM_CXSMICON, dpi);
    let icon = LoadImageW(GetModuleHandleW(null()), 1 as *const u16, IMAGE_ICON, size, size, LR_DEFAULTCOLOR);
    if icon.is_null() { LoadIconW(null_mut(), IDI_APPLICATION) } else { icon as HICON }
}

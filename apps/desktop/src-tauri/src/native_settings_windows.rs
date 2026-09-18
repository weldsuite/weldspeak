//! Win32 Hub window. Sidebar + Home / Dictionary / Snippets / Settings.

use std::collections::BTreeSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Manager};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateRoundRectRgn, CreateSolidBrush, DeleteObject, FillRect, FillRgn, InvalidateRect,
    SetBkColor, SetBkMode, SetTextColor, HBRUSH, HDC, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, LVS_REPORT,
    LVS_SHOWSELALWAYS, LVS_SINGLESEL, WC_LISTVIEWW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetDlgCtrlID, GetDlgItem, GetWindowLongPtrW,
    IsWindow, LoadCursorW, MoveWindow, PostMessageW, RegisterClassW, SendMessageW,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW, ShowWindow, CS_HREDRAW, CS_VREDRAW,
    CW_USEDEFAULT, GWLP_USERDATA, HMENU, IDC_ARROW, MINMAXINFO, SW_HIDE, SW_SHOWNORMAL,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN,
    WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_DESTROY, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN,
    WM_KEYUP, WM_SIZE, WNDCLASSW, WS_BORDER, WS_CHILD, WS_CLIPSIBLINGS, WS_OVERLAPPEDWINDOW,
    WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const BST_CHECKED: u32 = 1;
const BST_UNCHECKED: u32 = 0;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_RESETCONTENT: u32 = 0x014B;
const CB_SETCURSEL: u32 = 0x014E;
const CBS_DROPDOWNLIST: u32 = 0x0003;
const BS_AUTOCHECKBOX: u32 = 0x0003;
const BS_PUSHBUTTON: u32 = 0x0000;
const LVM_FIRST: u32 = 0x1000;
const LVM_GETITEMCOUNT: u32 = LVM_FIRST + 4;
const LVM_DELETEALLITEMS: u32 = LVM_FIRST + 9;
const LVM_INSERTITEMA: u32 = LVM_FIRST + 77; // Unicode: LVM_INSERTITEMW = LVM_FIRST+77
const LVM_SETITEMTEXTA: u32 = LVM_FIRST + 116; // LVM_SETITEMTEXTW
const LVM_INSERTCOLUMNW: u32 = LVM_FIRST + 97;
const LVM_GETNEXTITEM: u32 = LVM_FIRST + 12;
const LVM_SETEXTENDEDLISTVIEWSTYLE: u32 = LVM_FIRST + 54;
const LVS_EX_FULLROWSELECT: u32 = 0x0000_0020;
const LVS_EX_DOUBLEBUFFER: u32 = 0x0001_0000;
const LVIF_TEXT: u32 = 0x0001;
const LVCF_FMT: u32 = 0x0001;
const LVCF_WIDTH: u32 = 0x0002;
const LVCF_TEXT: u32 = 0x0004;
const LVCFMT_LEFT: i32 = 0;
const LVNI_SELECTED: u32 = 0x0002;
const EM_SETCUEBANNER: u32 = 0x1501;

use crate::hotkey;
use crate::native_settings::{
    self, theme, Page, DASHBOARD_URL, LOCALES, SIDEBAR_WIDTH, WINDOW_HEIGHT, WINDOW_MIN_HEIGHT,
    WINDOW_MIN_WIDTH, WINDOW_WIDTH,
};
use crate::settings::InjectionPreference;

const ID_NAV_HOME: i32 = 200;
const ID_NAV_DICT: i32 = 201;
const ID_NAV_SNIP: i32 = 202;
const ID_NAV_SET: i32 = 203;
const ID_HOME_LIST: i32 = 210;
const ID_HOME_COPY: i32 = 211;
const ID_HOME_DELETE: i32 = 212;
const ID_HOME_SIGN_IN: i32 = 213;
const ID_HOME_HEADER: i32 = 214;
const ID_HOME_STATUS: i32 = 215;
const ID_DICT_LIST: i32 = 220;
const ID_DICT_TERM: i32 = 221;
const ID_DICT_SOUND: i32 = 222;
const ID_DICT_ADD: i32 = 223;
const ID_DICT_DELETE: i32 = 224;
const ID_DICT_SEARCH: i32 = 225;
const ID_DICT_STATUS: i32 = 226;
const ID_DICT_SIGN_IN: i32 = 227;
const ID_SNIP_LIST: i32 = 230;
const ID_SNIP_TRIG: i32 = 231;
const ID_SNIP_EXP: i32 = 232;
const ID_SNIP_ADD: i32 = 233;
const ID_SNIP_DELETE: i32 = 234;
const ID_SNIP_STATUS: i32 = 235;
const ID_SIGN_IN: i32 = 101;
const ID_SIGN_OUT: i32 = 102;
const ID_BIND: i32 = 103;
const ID_MIC: i32 = 104;
const ID_CLEANUP: i32 = 105;
const ID_PAUSE: i32 = 106;
const ID_LOCALE: i32 = 107;
const ID_INJECTION: i32 = 108;
const ID_DASHBOARD: i32 = 109;
const ID_UPDATE: i32 = 110;
const ID_GRANT: i32 = 111;
const ID_KEEP_HISTORY: i32 = 112;
const ID_ORG: i32 = 113;
const ID_ACCOUNT: i32 = 114;
const ID_FOOTER: i32 = 115;
const ID_SIDEBAR: i32 = 51;
const ID_BRAND: i32 = 50;

/// COLORREF is 0x00BBGGRR.
fn rgb(c: (u8, u8, u8)) -> COLORREF {
    COLORREF(u32::from(c.2) << 16 | u32::from(c.1) << 8 | u32::from(c.0))
}

const WM_REFRESH: u32 = WM_APP + 20;
const WM_TRANSCRIPTS: u32 = WM_APP + 21;
const WM_DICTIONARY: u32 = WM_APP + 22;
const WM_STATUS: u32 = WM_APP + 23;

static HWND_BITS: AtomicIsize = AtomicIsize::new(0);
static APP: OnceLock<AppHandle> = OnceLock::new();
static CAPTURING: AtomicBool = AtomicBool::new(false);
static HELD: Mutex<BTreeSet<u16>> = Mutex::new(BTreeSet::new());
static PEAK: Mutex<Vec<String>> = Mutex::new(Vec::new());
static PAGE: AtomicUsize = AtomicUsize::new(0);
static TRANSCRIPTS: Mutex<Vec<crate::commands::TranscriptRecord>> = Mutex::new(Vec::new());
static DICTIONARY: Mutex<Vec<crate::commands::DictionaryTerm>> = Mutex::new(Vec::new());
static DICT_FILTER: Mutex<String> = Mutex::new(String::new());
static STATUS: Mutex<Option<crate::commands::Status>> = Mutex::new(None);
static CONTENT_BRUSH: OnceLock<isize> = OnceLock::new();
static SIDEBAR_BRUSH: OnceLock<isize> = OnceLock::new();

fn content_brush() -> HBRUSH {
    let bits = *CONTENT_BRUSH
        .get_or_init(|| unsafe { CreateSolidBrush(rgb(theme::CONTENT_BG_RGB)).0 as isize });
    HBRUSH(bits as *mut c_void)
}

fn sidebar_brush() -> HBRUSH {
    let bits = *SIDEBAR_BRUSH
        .get_or_init(|| unsafe { CreateSolidBrush(rgb(theme::SIDEBAR_BG_RGB)).0 as isize });
    HBRUSH(bits as *mut c_void)
}

#[repr(C)]
struct LvColumnW {
    mask: u32,
    fmt: i32,
    cx: i32,
    psz_text: *mut u16,
    cch_text_max: i32,
    i_sub_item: i32,
    i_image: i32,
    i_order: i32,
}

#[repr(C)]
struct LvItemW {
    mask: u32,
    i_item: i32,
    i_sub_item: i32,
    state: u32,
    state_mask: u32,
    psz_text: *mut u16,
    cch_text_max: i32,
    i_image: i32,
    l_param: isize,
    i_indent: i32,
    i_group_id: i32,
    c_columns: u32,
    pu_columns: *mut u32,
    pi_col_fmt: *mut i32,
    i_group: i32,
}

fn module() -> HINSTANCE {
    unsafe { HINSTANCE(GetModuleHandleW(None).expect("module").0) }
}

fn set_text(hwnd: HWND, text: &str) {
    let wide = wide_str(text);
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
    }
}

fn hwnd_from(bits: isize) -> HWND {
    HWND(bits as *mut c_void)
}

fn bits(hwnd: HWND) -> isize {
    hwnd.0 as isize
}

pub fn show(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let existing = hwnd_from(HWND_BITS.load(Ordering::Relaxed));
    unsafe {
        if !existing.0.is_null() && IsWindow(existing).as_bool() {
            let _ = ShowWindow(existing, SW_SHOWNORMAL);
            let _ = SetForegroundWindow(existing);
            refresh();
            return;
        }
        create_window(app);
    }
}

pub fn refresh() {
    let hwnd = hwnd_from(HWND_BITS.load(Ordering::Relaxed));
    if hwnd.0.is_null() {
        return;
    }
    unsafe {
        let _ = PostMessageW(hwnd, WM_REFRESH, WPARAM(0), LPARAM(0));
    }
}

unsafe fn create_window(app: &AppHandle) {
    let _ = InitCommonControlsEx(&INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_LISTVIEW_CLASSES,
    });

    let class = w!("WeldSpeakHub");
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        hInstance: module(),
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
        hbrBackground: CreateSolidBrush(rgb(theme::CONTENT_BG_RGB)),
        lpszClassName: class,
        ..Default::default()
    };
    let _ = RegisterClassW(&wc);
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class,
        w!("WeldSpeak"),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
        HWND::default(),
        HMENU::default(),
        module(),
        None,
    )
    .expect("hub window");
    HWND_BITS.store(bits(window), Ordering::Relaxed);
    spawn_shell(window);
    switch_page(window, Page::Home);
    load_async_data(app);
    let _ = ShowWindow(window, SW_SHOWNORMAL);
    let _ = SetForegroundWindow(window);
}

#[allow(clippy::too_many_arguments)]
fn child(
    parent: HWND,
    class: PCWSTR,
    text: PCWSTR,
    style: WINDOW_STYLE,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: i32,
) -> HWND {
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            text,
            style | WS_CHILD | WS_CLIPSIBLINGS,
            x,
            y,
            w,
            h,
            parent,
            HMENU(id as *mut c_void),
            module(),
            None,
        )
        .expect("child")
    }
}

fn show_child(parent: HWND, id: i32, visible: bool) {
    unsafe {
        let _ = ShowWindow(
            find_child(parent, id),
            if visible { SW_SHOWNORMAL } else { SW_HIDE },
        );
    }
}

fn find_child(parent: HWND, id: i32) -> HWND {
    unsafe { GetDlgItem(parent, id).unwrap_or_default() }
}

fn spawn_shell(parent: HWND) {
    child(
        parent,
        w!("STATIC"),
        w!(""),
        WINDOW_STYLE(0),
        0,
        0,
        SIDEBAR_WIDTH,
        WINDOW_HEIGHT,
        ID_SIDEBAR,
    );
    child(
        parent,
        w!("STATIC"),
        w!("WeldSpeak"),
        WINDOW_STYLE(0),
        20,
        22,
        SIDEBAR_WIDTH - 32,
        28,
        ID_BRAND,
    );
    let nav = [
        (ID_NAV_HOME, "Home"),
        (ID_NAV_DICT, "Dictionary"),
        (ID_NAV_SNIP, "Snippets"),
        (ID_NAV_SET, "Settings"),
    ];
    for (i, (id, label)) in nav.iter().enumerate() {
        let wide = wide_str(label);
        child(
            parent,
            w!("BUTTON"),
            PCWSTR(wide.as_ptr()),
            WINDOW_STYLE(WS_TABSTOP.0 | BS_PUSHBUTTON | 0x8000), // BS_FLAT
            14,
            72 + (i as i32) * 44,
            SIDEBAR_WIDTH - 28,
            36,
            *id,
        );
    }

    // Home
    child(
        parent,
        w!("STATIC"),
        w!(""),
        WINDOW_STYLE(0),
        SIDEBAR_WIDTH + 24,
        20,
        700,
        24,
        ID_HOME_HEADER,
    );
    child(
        parent,
        w!("STATIC"),
        w!(""),
        WINDOW_STYLE(0),
        SIDEBAR_WIDTH + 24,
        48,
        700,
        20,
        ID_HOME_STATUS,
    );
    child(
        parent,
        w!("BUTTON"),
        w!("Sign in"),
        WS_TABSTOP,
        SIDEBAR_WIDTH + 24,
        76,
        110,
        28,
        ID_HOME_SIGN_IN,
    );
    create_listview(parent, ID_HOME_LIST, SIDEBAR_WIDTH + 24, 116, 700, 460);
    child(
        parent,
        w!("BUTTON"),
        w!("Copy"),
        WS_TABSTOP,
        SIDEBAR_WIDTH + 24,
        590,
        90,
        28,
        ID_HOME_COPY,
    );
    child(
        parent,
        w!("BUTTON"),
        w!("Delete"),
        WS_TABSTOP,
        SIDEBAR_WIDTH + 124,
        590,
        90,
        28,
        ID_HOME_DELETE,
    );

    // Dictionary
    child(
        parent,
        w!("STATIC"),
        w!(""),
        WINDOW_STYLE(0),
        SIDEBAR_WIDTH + 24,
        20,
        700,
        20,
        ID_DICT_STATUS,
    );
    child(
        parent,
        w!("BUTTON"),
        w!("Sign in"),
        WS_TABSTOP,
        SIDEBAR_WIDTH + 24,
        48,
        110,
        28,
        ID_DICT_SIGN_IN,
    );
    child(
        parent,
        w!("EDIT"),
        w!(""),
        WINDOW_STYLE(WS_TABSTOP.0 | WS_BORDER.0 | 0x80), // ES_AUTOHSCROLL
        SIDEBAR_WIDTH + 24,
        48,
        220,
        26,
        ID_DICT_SEARCH,
    );
    child(
        parent,
        w!("EDIT"),
        w!(""),
        WINDOW_STYLE(WS_TABSTOP.0 | WS_BORDER.0 | 0x80),
        SIDEBAR_WIDTH + 24,
        84,
        200,
        26,
        ID_DICT_TERM,
    );
    child(
        parent,
        w!("EDIT"),
        w!(""),
        WINDOW_STYLE(WS_TABSTOP.0 | WS_BORDER.0 | 0x80),
        SIDEBAR_WIDTH + 236,
        84,
        200,
        26,
        ID_DICT_SOUND,
    );
    child(
        parent,
        w!("BUTTON"),
        w!("Add"),
        WS_TABSTOP,
        SIDEBAR_WIDTH + 448,
        82,
        80,
        28,
        ID_DICT_ADD,
    );
    create_listview(parent, ID_DICT_LIST, SIDEBAR_WIDTH + 24, 124, 700, 450);
    child(
        parent,
        w!("BUTTON"),
        w!("Delete"),
        WS_TABSTOP,
        SIDEBAR_WIDTH + 24,
        590,
        90,
        28,
        ID_DICT_DELETE,
    );

    // Snippets
    child(
        parent,
        w!("STATIC"),
        w!("Say the cue, get the saved text."),
        WINDOW_STYLE(0),
        SIDEBAR_WIDTH + 24,
        20,
        700,
        20,
        ID_SNIP_STATUS,
    );
    child(
        parent,
        w!("EDIT"),
        w!(""),
        WINDOW_STYLE(WS_TABSTOP.0 | WS_BORDER.0 | 0x80),
        SIDEBAR_WIDTH + 24,
        52,
        200,
        26,
        ID_SNIP_TRIG,
    );
    child(
        parent,
        w!("EDIT"),
        w!(""),
        WINDOW_STYLE(WS_TABSTOP.0 | WS_BORDER.0 | 0x80),
        SIDEBAR_WIDTH + 236,
        52,
        320,
        26,
        ID_SNIP_EXP,
    );
    child(
        parent,
        w!("BUTTON"),
        w!("Add"),
        WS_TABSTOP,
        SIDEBAR_WIDTH + 568,
        50,
        80,
        28,
        ID_SNIP_ADD,
    );
    create_listview(parent, ID_SNIP_LIST, SIDEBAR_WIDTH + 24, 92, 700, 480);
    child(
        parent,
        w!("BUTTON"),
        w!("Delete"),
        WS_TABSTOP,
        SIDEBAR_WIDTH + 24,
        590,
        90,
        28,
        ID_SNIP_DELETE,
    );

    // Settings
    child(
        parent,
        w!("STATIC"),
        w!(""),
        WINDOW_STYLE(0),
        SIDEBAR_WIDTH + 24,
        20,
        700,
        36,
        ID_ACCOUNT,
    );
    child(
        parent,
        w!("BUTTON"),
        w!("Sign in"),
        WS_TABSTOP,
        SIDEBAR_WIDTH + 24,
        64,
        110,
        28,
        ID_SIGN_IN,
    );
    child(
        parent,
        w!("BUTTON"),
        w!("Sign out"),
        WS_TABSTOP,
        SIDEBAR_WIDTH + 144,
        64,
        110,
        28,
        ID_SIGN_OUT,
    );
    child(
        parent,
        w!("BUTTON"),
        w!("Keyboard access…"),
        WS_TABSTOP,
        SIDEBAR_WIDTH + 268,
        64,
        160,
        28,
        ID_GRANT,
    );
    child(
        parent,
        w!("STATIC"),
        w!("Organization"),
        WINDOW_STYLE(0),
        SIDEBAR_WIDTH + 24,
        108,
        160,
        22,
        301,
    );
    child(
        parent,
        w!("COMBOBOX"),
        w!(""),
        WINDOW_STYLE(WS_TABSTOP.0 | WS_VSCROLL.0 | CBS_DROPDOWNLIST),
        SIDEBAR_WIDTH + 200,
        104,
        280,
        200,
        ID_ORG,
    );
    child(
        parent,
        w!("STATIC"),
        w!("Hold to talk"),
        WINDOW_STYLE(0),
        SIDEBAR_WIDTH + 24,
        148,
        160,
        22,
        302,
    );
    child(
        parent,
        w!("BUTTON"),
        w!("…"),
        WS_TABSTOP,
        SIDEBAR_WIDTH + 200,
        144,
        200,
        28,
        ID_BIND,
    );
    child(
        parent,
        w!("STATIC"),
        w!("Microphone"),
        WINDOW_STYLE(0),
        SIDEBAR_WIDTH + 24,
        188,
        160,
        22,
        303,
    );
    child(
        parent,
        w!("COMBOBOX"),
        w!(""),
        WINDOW_STYLE(WS_TABSTOP.0 | WS_VSCROLL.0 | CBS_DROPDOWNLIST),
        SIDEBAR_WIDTH + 200,
        184,
        280,
        220,
        ID_MIC,
    );
    child(
        parent,
        w!("BUTTON"),
        w!("Clean up speech"),
        WINDOW_STYLE(WS_TABSTOP.0 | BS_AUTOCHECKBOX),
        SIDEBAR_WIDTH + 24,
        228,
        280,
        24,
        ID_CLEANUP,
    );
    child(
        parent,
        w!("BUTTON"),
        w!("Pause media while talking"),
        WINDOW_STYLE(WS_TABSTOP.0 | BS_AUTOCHECKBOX),
        SIDEBAR_WIDTH + 24,
        256,
        280,
        24,
        ID_PAUSE,
    );
    child(
        parent,
        w!("BUTTON"),
        w!("Keep transcript history"),
        WINDOW_STYLE(WS_TABSTOP.0 | BS_AUTOCHECKBOX),
        SIDEBAR_WIDTH + 24,
        284,
        280,
        24,
        ID_KEEP_HISTORY,
    );
    child(
        parent,
        w!("STATIC"),
        w!("Language"),
        WINDOW_STYLE(0),
        SIDEBAR_WIDTH + 24,
        320,
        160,
        22,
        304,
    );
    child(
        parent,
        w!("COMBOBOX"),
        w!(""),
        WINDOW_STYLE(WS_TABSTOP.0 | WS_VSCROLL.0 | CBS_DROPDOWNLIST),
        SIDEBAR_WIDTH + 200,
        316,
        280,
        200,
        ID_LOCALE,
    );
    child(
        parent,
        w!("STATIC"),
        w!("Insert by"),
        WINDOW_STYLE(0),
        SIDEBAR_WIDTH + 24,
        360,
        160,
        22,
        305,
    );
    child(
        parent,
        w!("COMBOBOX"),
        w!(""),
        WINDOW_STYLE(WS_TABSTOP.0 | WS_VSCROLL.0 | CBS_DROPDOWNLIST),
        SIDEBAR_WIDTH + 200,
        356,
        280,
        120,
        ID_INJECTION,
    );
    child(
        parent,
        w!("BUTTON"),
        w!("Open team dashboard"),
        WS_TABSTOP,
        SIDEBAR_WIDTH + 24,
        408,
        180,
        30,
        ID_DASHBOARD,
    );
    child(
        parent,
        w!("BUTTON"),
        w!("Check for update"),
        WS_TABSTOP,
        SIDEBAR_WIDTH + 216,
        408,
        160,
        30,
        ID_UPDATE,
    );
    child(
        parent,
        w!("STATIC"),
        w!(""),
        WINDOW_STYLE(0),
        SIDEBAR_WIDTH + 24,
        460,
        700,
        24,
        ID_FOOTER,
    );

    set_cue(find_child(parent, ID_DICT_SEARCH), "Search dictionary");
    set_cue(find_child(parent, ID_DICT_TERM), "Word or phrase");
    set_cue(find_child(parent, ID_DICT_SOUND), "Sounds like (optional)");
    set_cue(find_child(parent, ID_SNIP_TRIG), "Cue, e.g. my address");
    set_cue(find_child(parent, ID_SNIP_EXP), "Text to insert");
}

fn set_cue(hwnd: HWND, text: &str) {
    let mut wide = wide_str(text);
    unsafe {
        SendMessageW(
            hwnd,
            EM_SETCUEBANNER,
            WPARAM(1),
            LPARAM(wide.as_mut_ptr() as isize),
        );
    }
}

fn create_listview(parent: HWND, id: i32, x: i32, y: i32, w: i32, h: i32) -> HWND {
    let hwnd = child(
        parent,
        WC_LISTVIEWW,
        w!(""),
        WINDOW_STYLE(
            WS_TABSTOP.0
                | WS_BORDER.0
                | WS_VISIBLE.0
                | LVS_REPORT
                | LVS_SINGLESEL
                | LVS_SHOWSELALWAYS,
        ),
        x,
        y,
        w,
        h,
        id,
    );
    unsafe {
        SendMessageW(
            hwnd,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            WPARAM((LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as usize),
            LPARAM((LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize),
        );
    }
    hwnd
}

fn insert_column(list: HWND, index: i32, title: &str, width: i32) {
    let mut wide = wide_str(title);
    let mut col = LvColumnW {
        mask: LVCF_FMT | LVCF_WIDTH | LVCF_TEXT,
        fmt: LVCFMT_LEFT,
        cx: width,
        psz_text: wide.as_mut_ptr(),
        cch_text_max: 0,
        i_sub_item: 0,
        i_image: 0,
        i_order: 0,
    };
    unsafe {
        SendMessageW(
            list,
            LVM_INSERTCOLUMNW,
            WPARAM(index as usize),
            LPARAM(&mut col as *mut _ as isize),
        );
    }
}

fn clear_list(list: HWND) {
    unsafe {
        SendMessageW(list, LVM_DELETEALLITEMS, WPARAM(0), LPARAM(0));
    }
}

fn insert_row(list: HWND, index: i32, cols: &[&str]) {
    let mut first = wide_str(cols.first().copied().unwrap_or(""));
    let mut item = LvItemW {
        mask: LVIF_TEXT,
        i_item: index,
        i_sub_item: 0,
        state: 0,
        state_mask: 0,
        psz_text: first.as_mut_ptr(),
        cch_text_max: 0,
        i_image: 0,
        l_param: 0,
        i_indent: 0,
        i_group_id: 0,
        c_columns: 0,
        pu_columns: std::ptr::null_mut(),
        pi_col_fmt: std::ptr::null_mut(),
        i_group: 0,
    };
    unsafe {
        SendMessageW(
            list,
            LVM_INSERTITEMA,
            WPARAM(0),
            LPARAM(&mut item as *mut _ as isize),
        );
        for (sub, text) in cols.iter().enumerate().skip(1) {
            let mut wide = wide_str(text);
            let mut sub_item = LvItemW {
                mask: LVIF_TEXT,
                i_item: index,
                i_sub_item: sub as i32,
                state: 0,
                state_mask: 0,
                psz_text: wide.as_mut_ptr(),
                cch_text_max: 0,
                i_image: 0,
                l_param: 0,
                i_indent: 0,
                i_group_id: 0,
                c_columns: 0,
                pu_columns: std::ptr::null_mut(),
                pi_col_fmt: std::ptr::null_mut(),
                i_group: 0,
            };
            SendMessageW(
                list,
                LVM_SETITEMTEXTA,
                WPARAM(index as usize),
                LPARAM(&mut sub_item as *mut _ as isize),
            );
        }
    }
}

fn selected_index(list: HWND) -> Option<usize> {
    let idx = unsafe {
        SendMessageW(
            list,
            LVM_GETNEXTITEM,
            WPARAM((-1i32) as usize),
            LPARAM(LVNI_SELECTED as isize),
        )
        .0
    };
    if idx < 0 {
        None
    } else {
        Some(idx as usize)
    }
}

fn current_page() -> Page {
    Page::from_index(PAGE.load(Ordering::Relaxed))
}

fn page_ids(page: Page) -> &'static [i32] {
    match page {
        Page::Home => &[
            ID_HOME_HEADER,
            ID_HOME_STATUS,
            ID_HOME_SIGN_IN,
            ID_HOME_LIST,
            ID_HOME_COPY,
            ID_HOME_DELETE,
        ],
        Page::Dictionary => &[
            ID_DICT_STATUS,
            ID_DICT_SIGN_IN,
            ID_DICT_SEARCH,
            ID_DICT_TERM,
            ID_DICT_SOUND,
            ID_DICT_ADD,
            ID_DICT_LIST,
            ID_DICT_DELETE,
        ],
        Page::Snippets => &[
            ID_SNIP_STATUS,
            ID_SNIP_TRIG,
            ID_SNIP_EXP,
            ID_SNIP_ADD,
            ID_SNIP_LIST,
            ID_SNIP_DELETE,
        ],
        Page::Settings => &[
            ID_ACCOUNT,
            ID_SIGN_IN,
            ID_SIGN_OUT,
            ID_GRANT,
            301,
            ID_ORG,
            302,
            ID_BIND,
            303,
            ID_MIC,
            ID_CLEANUP,
            ID_PAUSE,
            ID_KEEP_HISTORY,
            304,
            ID_LOCALE,
            305,
            ID_INJECTION,
            ID_DASHBOARD,
            ID_UPDATE,
            ID_FOOTER,
        ],
    }
}

fn switch_page(parent: HWND, page: Page) {
    PAGE.store(page.index(), Ordering::Relaxed);
    for p in Page::ALL {
        let show = p == page;
        for id in page_ids(p) {
            show_child(parent, *id, show);
        }
    }
    // Nav highlight via enable state is weak; just ensure visibility of shell.
    show_child(parent, ID_SIDEBAR, true);
    show_child(parent, ID_BRAND, true);
    for (i, id) in [ID_NAV_HOME, ID_NAV_DICT, ID_NAV_SNIP, ID_NAV_SET]
        .iter()
        .enumerate()
    {
        show_child(parent, *id, true);
        set_text(find_child(parent, *id), Page::from_index(i).label());
    }
    unsafe {
        let _ = InvalidateRect(parent, None, true);
    }
    if let Some(app) = APP.get() {
        apply_page(parent, page, app);
    }
}

fn layout(parent: HWND) {
    let mut rect = RECT::default();
    unsafe {
        let _ = GetClientRect(parent, &mut rect);
    }
    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;
    let content_w = (w - SIDEBAR_WIDTH - 48).max(200);
    let list_h = (h - 160).max(120);
    unsafe {
        let _ = MoveWindow(find_child(parent, ID_SIDEBAR), 0, 0, SIDEBAR_WIDTH, h, true);
        let _ = MoveWindow(
            find_child(parent, ID_BRAND),
            20,
            22,
            SIDEBAR_WIDTH - 32,
            28,
            true,
        );
        for (i, id) in [ID_NAV_HOME, ID_NAV_DICT, ID_NAV_SNIP, ID_NAV_SET]
            .iter()
            .enumerate()
        {
            let _ = MoveWindow(
                find_child(parent, *id),
                14,
                72 + (i as i32) * 44,
                SIDEBAR_WIDTH - 28,
                36,
                true,
            );
        }
        let _ = MoveWindow(
            find_child(parent, ID_HOME_LIST),
            SIDEBAR_WIDTH + 24,
            116,
            content_w,
            list_h,
            true,
        );
        let _ = MoveWindow(
            find_child(parent, ID_HOME_COPY),
            SIDEBAR_WIDTH + 24,
            h - 48,
            90,
            28,
            true,
        );
        let _ = MoveWindow(
            find_child(parent, ID_HOME_DELETE),
            SIDEBAR_WIDTH + 124,
            h - 48,
            90,
            28,
            true,
        );
        let _ = MoveWindow(
            find_child(parent, ID_DICT_LIST),
            SIDEBAR_WIDTH + 24,
            124,
            content_w,
            list_h - 20,
            true,
        );
        let _ = MoveWindow(
            find_child(parent, ID_DICT_DELETE),
            SIDEBAR_WIDTH + 24,
            h - 48,
            90,
            28,
            true,
        );
        let _ = MoveWindow(
            find_child(parent, ID_SNIP_LIST),
            SIDEBAR_WIDTH + 24,
            92,
            content_w,
            list_h + 20,
            true,
        );
        let _ = MoveWindow(
            find_child(parent, ID_SNIP_DELETE),
            SIDEBAR_WIDTH + 24,
            h - 48,
            90,
            28,
            true,
        );
    }
}

fn load_async_data(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let status = native_settings::fetch_status(app.clone()).await;
        if let Ok(mut slot) = STATUS.lock() {
            *slot = Some(status);
        }

        if native_settings::is_signed_in(&app) {
            if let Ok(list) = native_settings::fetch_transcripts(app.clone()).await {
                if let Ok(mut slot) = TRANSCRIPTS.lock() {
                    *slot = list;
                }
            }
            if let Ok(list) = native_settings::fetch_dictionary(app.clone()).await {
                if let Ok(mut slot) = DICTIONARY.lock() {
                    *slot = list;
                }
            }
        } else {
            if let Ok(mut slot) = TRANSCRIPTS.lock() {
                slot.clear();
            }
            if let Ok(mut slot) = DICTIONARY.lock() {
                slot.clear();
            }
        }

        let hwnd = hwnd_from(HWND_BITS.load(Ordering::Relaxed));
        if !hwnd.0.is_null() {
            unsafe {
                let _ = PostMessageW(hwnd, WM_STATUS, WPARAM(0), LPARAM(0));
                let _ = PostMessageW(hwnd, WM_TRANSCRIPTS, WPARAM(0), LPARAM(0));
                let _ = PostMessageW(hwnd, WM_DICTIONARY, WPARAM(0), LPARAM(0));
            }
        }
    });
}

fn apply_page(parent: HWND, page: Page, app: &AppHandle) {
    match page {
        Page::Home => apply_home(parent, app),
        Page::Dictionary => apply_dictionary(parent, app),
        Page::Snippets => apply_snippets(parent, app),
        Page::Settings => apply_settings(parent, app),
    }
}

fn ensure_home_columns(list: HWND) {
    let count = unsafe { SendMessageW(list, LVM_GETITEMCOUNT, WPARAM(0), LPARAM(0)).0 };
    // Columns only once: use user data flag on list.
    let flagged = unsafe { GetWindowLongPtrW(list, GWLP_USERDATA) };
    if flagged == 0 {
        insert_column(list, 0, "Transcript", 420);
        insert_column(list, 1, "App", 140);
        insert_column(list, 2, "When", 140);
        unsafe {
            SetWindowLongPtrW(list, GWLP_USERDATA, 1);
        }
    }
    let _ = count;
}

fn apply_home(parent: HWND, app: &AppHandle) {
    let signed_in = native_settings::is_signed_in(app);
    let key = native_settings::hotkey_label(app);
    let words = native_settings::words_dictated(app);
    set_text(
        find_child(parent, ID_HOME_HEADER),
        &format!("{words} words · Hold {key} to talk"),
    );
    if signed_in {
        set_text(find_child(parent, ID_HOME_STATUS), "Recent dictations");
        show_child(parent, ID_HOME_SIGN_IN, false);
    } else {
        set_text(
            find_child(parent, ID_HOME_STATUS),
            "Sign in to sync and view transcript history.",
        );
        show_child(parent, ID_HOME_SIGN_IN, true);
    }
    let list = find_child(parent, ID_HOME_LIST);
    ensure_home_columns(list);
    clear_list(list);
    if let Ok(rows) = TRANSCRIPTS.lock() {
        for (i, row) in rows.iter().enumerate() {
            let text = native_settings::truncate(&row.formatted, 80);
            let app_name = row.app_name.clone().unwrap_or_default();
            let when = native_settings::truncate(&row.created_at, 19);
            insert_row(list, i as i32, &[&text, &app_name, &when]);
        }
    }
}

fn ensure_dict_columns(list: HWND) {
    let flagged = unsafe { GetWindowLongPtrW(list, GWLP_USERDATA) };
    if flagged == 0 {
        insert_column(list, 0, "Term", 260);
        insert_column(list, 1, "Sounds like", 220);
        insert_column(list, 2, "Scope", 100);
        unsafe {
            SetWindowLongPtrW(list, GWLP_USERDATA, 1);
        }
    }
}

fn apply_dictionary(parent: HWND, app: &AppHandle) {
    let signed_in = native_settings::is_signed_in(app);
    let list = find_child(parent, ID_DICT_LIST);
    ensure_dict_columns(list);
    if !signed_in {
        set_text(
            find_child(parent, ID_DICT_STATUS),
            "Sign in to manage your dictionary.",
        );
        show_child(parent, ID_DICT_SIGN_IN, true);
        show_child(parent, ID_DICT_SEARCH, false);
        show_child(parent, ID_DICT_TERM, false);
        show_child(parent, ID_DICT_SOUND, false);
        show_child(parent, ID_DICT_ADD, false);
        show_child(parent, ID_DICT_DELETE, false);
        show_child(parent, ID_DICT_LIST, false);
        return;
    }
    show_child(parent, ID_DICT_SIGN_IN, false);
    show_child(parent, ID_DICT_SEARCH, true);
    show_child(parent, ID_DICT_TERM, true);
    show_child(parent, ID_DICT_SOUND, true);
    show_child(parent, ID_DICT_ADD, true);
    show_child(parent, ID_DICT_DELETE, true);
    show_child(parent, ID_DICT_LIST, true);
    set_text(
        find_child(parent, ID_DICT_STATUS),
        "Words the mic should not guess.",
    );
    let filter = DICT_FILTER
        .lock()
        .ok()
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    clear_list(list);
    if let Ok(rows) = DICTIONARY.lock() {
        let mut i = 0i32;
        for row in rows.iter() {
            if !filter.is_empty() {
                let hay = format!("{} {}", row.term, row.sounds_like.as_deref().unwrap_or(""))
                    .to_lowercase();
                if !hay.contains(&filter) {
                    continue;
                }
            }
            let sound = row.sounds_like.clone().unwrap_or_default();
            insert_row(list, i, &[&row.term, &sound, &row.scope]);
            i += 1;
        }
    }
}

fn ensure_snip_columns(list: HWND) {
    let flagged = unsafe { GetWindowLongPtrW(list, GWLP_USERDATA) };
    if flagged == 0 {
        insert_column(list, 0, "Cue", 220);
        insert_column(list, 1, "Expansion", 460);
        unsafe {
            SetWindowLongPtrW(list, GWLP_USERDATA, 1);
        }
    }
}

fn apply_snippets(parent: HWND, app: &AppHandle) {
    let list = find_child(parent, ID_SNIP_LIST);
    ensure_snip_columns(list);
    clear_list(list);
    let snippets = native_settings::load_snippets(app);
    for (i, snip) in snippets.iter().enumerate() {
        let expansion = native_settings::truncate(&snip.expansion, 100);
        insert_row(list, i as i32, &[&snip.trigger, &expansion]);
    }
}

fn apply_settings(parent: HWND, app: &AppHandle) {
    let settings = native_settings::load_settings(app);
    let signed_in = native_settings::is_signed_in(app);
    let status = STATUS.lock().ok().and_then(|s| s.clone());
    let account = if signed_in {
        let email = status
            .as_ref()
            .and_then(|s| s.email.clone())
            .unwrap_or_else(|| "Signed in".into());
        format!("Signed in as {email}")
    } else {
        "Sign in with WeldSuite. A short code in the browser confirms this computer.".into()
    };
    set_text(find_child(parent, ID_ACCOUNT), &account);
    show_child(parent, ID_SIGN_IN, !signed_in);
    show_child(parent, ID_SIGN_OUT, signed_in);
    let can_inject = crate::inject::can_synthesise_input();
    show_child(parent, ID_GRANT, !can_inject);
    set_text(
        find_child(parent, ID_BIND),
        &hotkey::label(&settings.hotkey.accelerator),
    );
    check(find_child(parent, ID_CLEANUP), settings.clean_up_text);
    check(find_child(parent, ID_PAUSE), settings.pause_media);
    check(find_child(parent, ID_KEEP_HISTORY), settings.keep_history);
    fill_mics(find_child(parent, ID_MIC), settings.microphone.as_deref());
    fill_locales(find_child(parent, ID_LOCALE), settings.locale.as_deref());
    fill_injection(find_child(parent, ID_INJECTION), settings.injection);
    fill_orgs(
        find_child(parent, ID_ORG),
        status.as_ref().map(|s| s.orgs.as_slice()).unwrap_or(&[]),
        settings.org_id.as_deref(),
    );
    set_text(
        find_child(parent, ID_FOOTER),
        &native_settings::version_footer(app),
    );
}

fn check(hwnd: HWND, on: bool) {
    unsafe {
        SendMessageW(
            hwnd,
            BM_SETCHECK,
            WPARAM(if on {
                BST_CHECKED as usize
            } else {
                BST_UNCHECKED as usize
            }),
            LPARAM(0),
        );
    }
}

fn is_checked(hwnd: HWND) -> bool {
    unsafe { SendMessageW(hwnd, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 as u32 == BST_CHECKED }
}

fn fill_mics(combo: HWND, selected: Option<&str>) {
    unsafe {
        SendMessageW(combo, CB_RESETCONTENT, WPARAM(0), LPARAM(0));
        add_combo(combo, "System default");
        let mut index = 0i32;
        for (i, mic) in crate::audio::list_input_devices().iter().enumerate() {
            let label = if mic.is_default {
                format!("{} (default)", mic.name)
            } else {
                mic.name.clone()
            };
            add_combo(combo, &label);
            if selected == Some(mic.name.as_str()) {
                index = (i + 1) as i32;
            }
        }
        SendMessageW(combo, CB_SETCURSEL, WPARAM(index as usize), LPARAM(0));
    }
}

fn fill_locales(combo: HWND, selected: Option<&str>) {
    unsafe {
        SendMessageW(combo, CB_RESETCONTENT, WPARAM(0), LPARAM(0));
        let mut index = 0usize;
        for (i, (value, label)) in LOCALES.iter().enumerate() {
            add_combo(combo, label);
            if selected.unwrap_or("") == *value {
                index = i;
            }
        }
        SendMessageW(combo, CB_SETCURSEL, WPARAM(index), LPARAM(0));
    }
}

fn fill_injection(combo: HWND, selected: InjectionPreference) {
    unsafe {
        SendMessageW(combo, CB_RESETCONTENT, WPARAM(0), LPARAM(0));
        add_combo(combo, "Automatic");
        add_combo(combo, "Typing");
        add_combo(combo, "Pasting");
        let index = match selected {
            InjectionPreference::Automatic => 0,
            InjectionPreference::AlwaysType => 1,
            InjectionPreference::AlwaysPaste => 2,
        };
        SendMessageW(combo, CB_SETCURSEL, WPARAM(index), LPARAM(0));
    }
}

fn fill_orgs(combo: HWND, orgs: &[crate::commands::OrgSummary], selected: Option<&str>) {
    unsafe {
        SendMessageW(combo, CB_RESETCONTENT, WPARAM(0), LPARAM(0));
        if orgs.is_empty() {
            add_combo(combo, "Personal");
            SendMessageW(combo, CB_SETCURSEL, WPARAM(0), LPARAM(0));
            return;
        }
        let mut index = 0usize;
        for (i, org) in orgs.iter().enumerate() {
            add_combo(combo, &org.name);
            if Some(org.org_id.as_str()) == selected {
                index = i;
            }
        }
        SendMessageW(combo, CB_SETCURSEL, WPARAM(index), LPARAM(0));
    }
}

fn add_combo(combo: HWND, text: &str) {
    let mut wide = wide_str(text);
    unsafe {
        SendMessageW(
            combo,
            CB_ADDSTRING,
            WPARAM(0),
            LPARAM(wide.as_mut_ptr() as isize),
        );
    }
}

fn combo_index(combo: HWND) -> i32 {
    unsafe { SendMessageW(combo, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 as i32 }
}

fn wide_str(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn edit_text(parent: HWND, id: i32) -> String {
    let hwnd = find_child(parent, id);
    let mut buf = vec![0u16; 4096];
    let len = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut buf) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

unsafe extern "system" fn wnd_proc(
    window: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => {
            let hdc = HDC(wparam.0 as *mut c_void);
            let mut rect = RECT::default();
            let _ = GetClientRect(window, &mut rect);
            unsafe {
                let brush = CreateSolidBrush(rgb(theme::CONTENT_BG_RGB));
                let _ = FillRect(hdc, &rect, brush);
                let sidebar = RECT {
                    left: 0,
                    top: 0,
                    right: SIDEBAR_WIDTH,
                    bottom: rect.bottom,
                };
                let side = CreateSolidBrush(rgb(theme::SIDEBAR_BG_RGB));
                let _ = FillRect(hdc, &sidebar, side);

                // Soft teal accent rail + rounded selected nav chip.
                let accent = CreateSolidBrush(rgb(theme::BRAND_RGB));
                let accent_bar = RECT {
                    left: 0,
                    top: 0,
                    right: 3,
                    bottom: rect.bottom,
                };
                let _ = FillRect(hdc, &accent_bar, accent);

                let page = Page::from_index(PAGE.load(Ordering::Relaxed));
                let chip_y = 72 + (page.index() as i32) * 44;
                let chip_brush = CreateSolidBrush(rgb(theme::SIDEBAR_CHIP_ACCENT_RGB));
                let chip_rgn =
                    CreateRoundRectRgn(12, chip_y, SIDEBAR_WIDTH - 12, chip_y + 36, 14, 14);
                let _ = FillRgn(hdc, chip_rgn, chip_brush);

                let _ = DeleteObject(brush);
                let _ = DeleteObject(side);
                let _ = DeleteObject(accent);
                let _ = DeleteObject(chip_brush);
                let _ = DeleteObject(chip_rgn);
            }
            LRESULT(1)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLORLISTBOX => {
            let hdc = HDC(wparam.0 as *mut c_void);
            let ctrl = HWND(lparam.0 as *mut c_void);
            let id = unsafe { GetDlgCtrlID(ctrl) };
            let nav = matches!(
                id,
                ID_NAV_HOME | ID_NAV_DICT | ID_NAV_SNIP | ID_NAV_SET | ID_BRAND | ID_SIDEBAR
            );
            unsafe {
                SetBkMode(hdc, TRANSPARENT);
                if nav {
                    let page = Page::from_index(PAGE.load(Ordering::Relaxed));
                    let selected = matches!(
                        (id, page),
                        (ID_NAV_HOME, Page::Home)
                            | (ID_NAV_DICT, Page::Dictionary)
                            | (ID_NAV_SNIP, Page::Snippets)
                            | (ID_NAV_SET, Page::Settings)
                    );
                    let fg = if selected {
                        theme::OVERLAY_LISTEN_RGB
                    } else {
                        theme::SIDEBAR_TEXT_RGB
                    };
                    SetTextColor(hdc, rgb(fg));
                    SetBkColor(hdc, rgb(theme::SIDEBAR_BG_RGB));
                    return LRESULT(sidebar_brush().0 as isize);
                }
                SetTextColor(hdc, rgb(theme::TEXT_RGB));
                SetBkColor(hdc, rgb(theme::CONTENT_BG_RGB));
            }
            LRESULT(content_brush().0 as isize)
        }
        WM_GETMINMAXINFO => {
            let info = lparam.0 as *mut MINMAXINFO;
            if !info.is_null() {
                unsafe {
                    (*info).ptMinTrackSize = POINT {
                        x: WINDOW_MIN_WIDTH,
                        y: WINDOW_MIN_HEIGHT,
                    };
                }
            }
            LRESULT(0)
        }
        WM_SIZE => {
            layout(window);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 as u32) & 0xFFFF;
            let notify = ((wparam.0 as u32) >> 16) & 0xFFFF;
            if notify == 0 || notify == 1 || notify == 0x0300 {
                // EN_CHANGE = 0x0300
                on_command(window, id as i32, notify);
            }
            LRESULT(0)
        }
        WM_KEYDOWN | WM_KEYUP => {
            if CAPTURING.load(Ordering::Relaxed) {
                on_capture_key(window, msg, wparam, lparam);
                LRESULT(0)
            } else {
                unsafe { DefWindowProcW(window, msg, wparam, lparam) }
            }
        }
        WM_REFRESH => {
            if let Some(app) = APP.get() {
                load_async_data(app);
                apply_page(window, current_page(), app);
            }
            LRESULT(0)
        }
        WM_TRANSCRIPTS => {
            if current_page() == Page::Home {
                if let Some(app) = APP.get() {
                    apply_home(window, app);
                }
            }
            LRESULT(0)
        }
        WM_DICTIONARY => {
            if current_page() == Page::Dictionary {
                if let Some(app) = APP.get() {
                    apply_dictionary(window, app);
                }
            }
            LRESULT(0)
        }
        WM_STATUS => {
            if current_page() == Page::Settings || current_page() == Page::Home {
                if let Some(app) = APP.get() {
                    apply_page(window, current_page(), app);
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            stop_capture();
            let _ = ShowWindow(window, SW_HIDE);
            LRESULT(0)
        }
        WM_DESTROY => {
            HWND_BITS.store(0, Ordering::Relaxed);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, msg, wparam, lparam) },
    }
}

fn on_command(window: HWND, id: i32, notify: u32) {
    let Some(app) = APP.get() else {
        return;
    };
    match id {
        ID_NAV_HOME => switch_page(window, Page::Home),
        ID_NAV_DICT => switch_page(window, Page::Dictionary),
        ID_NAV_SNIP => switch_page(window, Page::Snippets),
        ID_NAV_SET => switch_page(window, Page::Settings),
        ID_HOME_SIGN_IN | ID_DICT_SIGN_IN | ID_SIGN_IN => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                match crate::commands::begin_sign_in(app.clone()).await {
                    Ok(started) => {
                        crate::overlay::show_status(
                            &app,
                            &format!("Confirm {}", started.user_code),
                        );
                    }
                    Err(error) => crate::overlay::show_notice(&app, &error),
                }
            });
        }
        ID_SIGN_OUT => {
            let _ = crate::commands::sign_out(app.state());
            load_async_data(app);
            apply_page(window, current_page(), app);
        }
        ID_HOME_COPY => {
            if let Some(idx) = selected_index(find_child(window, ID_HOME_LIST)) {
                if let Ok(rows) = TRANSCRIPTS.lock() {
                    if let Some(row) = rows.get(idx) {
                        let _ = native_settings::copy_transcript_text(&row.formatted);
                        crate::overlay::show_notice(app, "Copied");
                    }
                }
            }
        }
        ID_HOME_DELETE => {
            if let Some(idx) = selected_index(find_child(window, ID_HOME_LIST)) {
                let id = TRANSCRIPTS
                    .lock()
                    .ok()
                    .and_then(|rows| rows.get(idx).map(|r| r.id.clone()));
                if let Some(id) = id {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if crate::commands::delete_transcript(app.clone(), id)
                            .await
                            .is_ok()
                        {
                            load_async_data(&app);
                        }
                    });
                }
            }
        }
        ID_DICT_ADD => {
            let term = edit_text(window, ID_DICT_TERM);
            let sound = edit_text(window, ID_DICT_SOUND);
            let sounds = if sound.trim().is_empty() {
                None
            } else {
                Some(sound)
            };
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                match crate::commands::add_dictionary_term(app.clone(), term, sounds).await {
                    Ok(_) => {
                        if let Ok(list) = native_settings::fetch_dictionary(app.clone()).await {
                            if let Ok(mut slot) = DICTIONARY.lock() {
                                *slot = list;
                            }
                        }
                        let hwnd = hwnd_from(HWND_BITS.load(Ordering::Relaxed));
                        unsafe {
                            let _ = PostMessageW(hwnd, WM_DICTIONARY, WPARAM(0), LPARAM(0));
                        }
                    }
                    Err(err) => crate::overlay::show_notice(&app, &err),
                }
            });
            set_text(find_child(window, ID_DICT_TERM), "");
            set_text(find_child(window, ID_DICT_SOUND), "");
        }
        ID_DICT_DELETE => {
            if let Some(vis_idx) = selected_index(find_child(window, ID_DICT_LIST)) {
                let filter = DICT_FILTER
                    .lock()
                    .ok()
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default();
                let id = DICTIONARY.lock().ok().and_then(|rows| {
                    let mut i = 0usize;
                    for row in rows.iter() {
                        if !filter.is_empty() {
                            let hay = format!(
                                "{} {}",
                                row.term,
                                row.sounds_like.as_deref().unwrap_or("")
                            )
                            .to_lowercase();
                            if !hay.contains(&filter) {
                                continue;
                            }
                        }
                        if i == vis_idx {
                            return Some(row.id.clone());
                        }
                        i += 1;
                    }
                    None
                });
                if let Some(id) = id {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if crate::commands::delete_dictionary_term(app.clone(), id)
                            .await
                            .is_ok()
                        {
                            if let Ok(list) = native_settings::fetch_dictionary(app.clone()).await {
                                if let Ok(mut slot) = DICTIONARY.lock() {
                                    *slot = list;
                                }
                            }
                            let hwnd = hwnd_from(HWND_BITS.load(Ordering::Relaxed));
                            unsafe {
                                let _ = PostMessageW(hwnd, WM_DICTIONARY, WPARAM(0), LPARAM(0));
                            }
                        }
                    });
                }
            }
        }
        ID_DICT_SEARCH if notify == 0x0300 => {
            if let Ok(mut filter) = DICT_FILTER.lock() {
                *filter = edit_text(window, ID_DICT_SEARCH);
            }
            apply_dictionary(window, app);
        }
        ID_SNIP_ADD => {
            let trigger = edit_text(window, ID_SNIP_TRIG);
            let expansion = edit_text(window, ID_SNIP_EXP);
            match native_settings::add_snippet(app, trigger, expansion) {
                Ok(()) => {
                    set_text(find_child(window, ID_SNIP_TRIG), "");
                    set_text(find_child(window, ID_SNIP_EXP), "");
                    apply_snippets(window, app);
                }
                Err(err) => crate::overlay::show_notice(app, &err),
            }
        }
        ID_SNIP_DELETE => {
            if let Some(idx) = selected_index(find_child(window, ID_SNIP_LIST)) {
                native_settings::remove_snippet(app, idx);
                apply_snippets(window, app);
            }
        }
        ID_BIND => toggle_capture(window),
        ID_CLEANUP => {
            native_settings::set_clean_up(app, is_checked(find_child(window, ID_CLEANUP)));
        }
        ID_PAUSE => {
            native_settings::set_pause_media(app, is_checked(find_child(window, ID_PAUSE)));
        }
        ID_KEEP_HISTORY => {
            native_settings::set_keep_history(app, is_checked(find_child(window, ID_KEEP_HISTORY)));
        }
        ID_MIC => {
            let idx = combo_index(find_child(window, ID_MIC));
            let name = if idx <= 0 {
                None
            } else {
                crate::audio::list_input_devices()
                    .get((idx as usize).saturating_sub(1))
                    .map(|m| m.name.clone())
            };
            native_settings::set_microphone(app, name);
        }
        ID_LOCALE => {
            let idx = combo_index(find_child(window, ID_LOCALE)).max(0) as usize;
            let value = LOCALES.get(idx).map(|(v, _)| *v).unwrap_or("");
            native_settings::set_locale(
                app,
                if value.is_empty() {
                    None
                } else {
                    Some(value.into())
                },
            );
        }
        ID_INJECTION => {
            let pref = match combo_index(find_child(window, ID_INJECTION)) {
                1 => InjectionPreference::AlwaysType,
                2 => InjectionPreference::AlwaysPaste,
                _ => InjectionPreference::Automatic,
            };
            native_settings::set_injection(app, pref);
        }
        ID_ORG => {
            let idx = combo_index(find_child(window, ID_ORG)).max(0) as usize;
            let org_id = STATUS
                .lock()
                .ok()
                .and_then(|s| s.clone())
                .and_then(|s| s.orgs.get(idx).map(|o| o.org_id.clone()));
            native_settings::set_org(app, org_id);
        }
        ID_DASHBOARD => {
            let _ = crate::commands::open_in_browser(DASHBOARD_URL);
        }
        ID_UPDATE => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                match crate::updater::install_update(app.clone()).await {
                    Ok(msg) => crate::overlay::show_notice(&app, &msg),
                    Err(err) => crate::overlay::show_notice(&app, &err),
                }
            });
        }
        ID_GRANT => {
            let _ = crate::inject::open_permission_settings();
        }
        _ => {}
    }
}

fn toggle_capture(window: HWND) {
    if CAPTURING.load(Ordering::Relaxed) {
        stop_capture();
        if let Some(app) = APP.get() {
            apply_settings(window, app);
        }
        return;
    }
    CAPTURING.store(true, Ordering::Relaxed);
    if let Ok(mut held) = HELD.lock() {
        held.clear();
    }
    if let Ok(mut peak) = PEAK.lock() {
        peak.clear();
    }
    hotkey::suspend(true);
    unsafe {
        set_text(find_child(window, ID_BIND), "Press a key…");
        let _ = SetFocus(window);
    }
}

fn stop_capture() {
    CAPTURING.store(false, Ordering::Relaxed);
    hotkey::suspend(false);
}

fn on_capture_key(window: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) {
    let vk = wparam.0 as u16;
    let extended = (lparam.0 as u32) & (1 << 24) != 0;
    let scan = ((lparam.0 as u32) >> 16) & 0xFF;
    if vk == 0x1B {
        stop_capture();
        if let Some(app) = APP.get() {
            apply_settings(window, app);
        }
        return;
    }
    let Some(code) = hotkey::code_from_windows_vk(vk, extended, scan as u16) else {
        return;
    };
    if msg == WM_KEYDOWN {
        if let Ok(mut held) = HELD.lock() {
            held.insert(vk);
        }
        if let Ok(mut peak) = PEAK.lock() {
            if !peak.contains(&code) && peak.len() < 2 {
                peak.push(code);
            }
            let label = hotkey::label(&peak.join("+"));
            set_text(find_child(window, ID_BIND), &label);
        }
        return;
    }
    if msg == WM_KEYUP {
        if let Ok(mut held) = HELD.lock() {
            held.remove(&vk);
            if held.is_empty() {
                if let Ok(mut peak) = PEAK.lock() {
                    if !peak.is_empty() {
                        let accelerator = peak.join("+");
                        peak.clear();
                        if let Some(app) = APP.get() {
                            if let Err(err) = native_settings::set_hotkey(app, accelerator) {
                                crate::overlay::show_notice(app, &err);
                            }
                        }
                    }
                }
                stop_capture();
                if let Some(app) = APP.get() {
                    apply_settings(window, app);
                }
            }
        }
    }
}

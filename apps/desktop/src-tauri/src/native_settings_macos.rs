//! AppKit Hub window. Sidebar + Home / Dictionary / Snippets / Settings.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, NSObject, Sel};
use objc2::{msg_send, msg_send_id, sel, ClassType};
use objc2_app_kit::{
    NSBackingStoreType, NSBezelStyle, NSButton, NSButtonType, NSColor, NSFont, NSPopUpButton,
    NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSInteger, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Manager};

use crate::native_settings::{
    self, Page, DASHBOARD_URL, LOCALES, SIDEBAR_WIDTH, WINDOW_HEIGHT, WINDOW_MIN_HEIGHT,
    WINDOW_MIN_WIDTH, WINDOW_WIDTH,
};
use crate::settings::InjectionPreference;

static APP: OnceLock<AppHandle> = OnceLock::new();
static ACTION_TARGET: OnceLock<Retained<NSObject>> = OnceLock::new();
static PAGE: Mutex<Page> = Mutex::new(Page::Home);
static TRANSCRIPTS: Mutex<Vec<crate::commands::TranscriptRecord>> = Mutex::new(Vec::new());
static DICTIONARY: Mutex<Vec<crate::commands::DictionaryTerm>> = Mutex::new(Vec::new());
static STATUS: Mutex<Option<crate::commands::Status>> = Mutex::new(None);

thread_local! {
    static WINDOW: RefCell<Option<Retained<NSWindow>>> = const { RefCell::new(None) };
    static CONTENT: RefCell<Option<Retained<NSView>>> = const { RefCell::new(None) };
    static NAV_BUTTONS: RefCell<Vec<Retained<NSButton>>> = const { RefCell::new(Vec::new()) };
}

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("AppKit hub on the main thread")
}

pub fn show(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let _ = install_actions();
    WINDOW.with(|slot| {
        if let Some(window) = slot.borrow().as_ref() {
            window.makeKeyAndOrderFront(None);
            refresh();
            return;
        }
        *slot.borrow_mut() = Some(build(app));
    });
}

pub fn refresh() {
    WINDOW.with(|slot| {
        if slot.borrow().is_some() {
            if let Some(app) = APP.get() {
                load_async_data(app);
                rebuild_content();
            }
        }
    });
}

fn build(app: &AppHandle) -> Retained<NSWindow> {
    let mtm = mtm();
    let rect = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(WINDOW_WIDTH as f64, WINDOW_HEIGHT as f64),
    );
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            rect,
            style,
            NSBackingStoreType::NSBackingStoreBuffered,
            false,
        )
    };
    unsafe {
        window.setReleasedWhenClosed(false);
        window.setMinSize(NSSize::new(
            WINDOW_MIN_WIDTH as f64,
            WINDOW_MIN_HEIGHT as f64,
        ));
    }
    window.setTitle(&NSString::from_str("WeldSpeak"));
    window.setBackgroundColor(Some(&unsafe {
        NSColor::colorWithCalibratedRed_green_blue_alpha(0.98, 0.976, 0.965, 1.0)
    }));
    window.center();

    let root = window.contentView().expect("content view");
    let sidebar = unsafe { NSView::new(mtm) };
    unsafe {
        sidebar.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(SIDEBAR_WIDTH as f64, WINDOW_HEIGHT as f64),
        ));
        sidebar.setWantsLayer(true);
        if let Some(layer) = sidebar.layer() {
            let _ = layer;
        }
        root.addSubview(&sidebar);
    }

    let mut nav = Vec::new();
    for (i, page) in Page::ALL.iter().enumerate() {
        let button = unsafe { NSButton::new(mtm) };
        unsafe {
            button.setTitle(&NSString::from_str(page.label()));
            button.setBezelStyle(NSBezelStyle::Rounded);
            button.setButtonType(NSButtonType::MomentaryPushIn);
            button.setFrame(NSRect::new(
                NSPoint::new(12.0, (WINDOW_HEIGHT as f64) - 60.0 - (i as f64) * 40.0),
                NSSize::new((SIDEBAR_WIDTH - 24) as f64, 32.0),
            ));
            let action = match page {
                Page::Home => sel!(navHome:),
                Page::Dictionary => sel!(navDict:),
                Page::Snippets => sel!(navSnip:),
                Page::Settings => sel!(navSet:),
            };
            button.setAction(Some(action));
            if let Some(target) = ACTION_TARGET.get() {
                button.setTarget(Some(AsRef::<AnyObject>::as_ref(&**target)));
            }
            sidebar.addSubview(&button);
        }
        nav.push(button);
    }
    NAV_BUTTONS.with(|slot| *slot.borrow_mut() = nav);

    let content = unsafe { NSView::new(mtm) };
    unsafe {
        content.setFrame(NSRect::new(
            NSPoint::new(SIDEBAR_WIDTH as f64, 0.0),
            NSSize::new(
                (WINDOW_WIDTH - SIDEBAR_WIDTH) as f64,
                WINDOW_HEIGHT as f64,
            ),
        ));
        root.addSubview(&content);
    }
    CONTENT.with(|slot| *slot.borrow_mut() = Some(content));

    load_async_data(app);
    rebuild_content();
    window.makeKeyAndOrderFront(None);
    window
}

fn clear_content() {
    CONTENT.with(|slot| {
        if let Some(content) = slot.borrow().as_ref() {
            unsafe {
                let subviews = content.subviews();
                for view in subviews.iter() {
                    view.removeFromSuperview();
                }
            }
        }
    });
}

fn rebuild_content() {
    clear_content();
    update_nav_titles();
    let page = PAGE.lock().ok().map(|p| *p).unwrap_or(Page::Home);
    match page {
        Page::Home => build_home(),
        Page::Dictionary => build_dictionary(),
        Page::Snippets => build_snippets(),
        Page::Settings => build_settings(),
    }
}

fn update_nav_titles() {
    let page = PAGE.lock().ok().map(|p| *p).unwrap_or(Page::Home);
    NAV_BUTTONS.with(|slot| {
        for (i, button) in slot.borrow().iter().enumerate() {
            let p = Page::from_index(i);
            let title = if p == page {
                format!("› {}", p.label())
            } else {
                p.label().to_string()
            };
            unsafe {
                button.setTitle(&NSString::from_str(&title));
            }
        }
    });
}

fn content_view() -> Option<Retained<NSView>> {
    CONTENT.with(|slot| slot.borrow().clone())
}

fn add_label(parent: &NSView, text: &str, size: f64, x: f64, y: f64, w: f64, h: f64) {
    let label = unsafe { NSTextField::new(mtm()) };
    unsafe {
        label.setEditable(false);
        label.setBezeled(false);
        label.setDrawsBackground(false);
        label.setSelectable(false);
        label.setFont(Some(&NSFont::systemFontOfSize(size)));
        label.setStringValue(&NSString::from_str(text));
        label.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)));
        parent.addSubview(&label);
    }
}

fn add_button(parent: &NSView, title: &str, x: f64, y: f64, w: f64, action: Sel) -> Retained<NSButton> {
    let button = unsafe { NSButton::new(mtm()) };
    unsafe {
        button.setTitle(&NSString::from_str(title));
        button.setBezelStyle(NSBezelStyle::Rounded);
        button.setButtonType(NSButtonType::MomentaryPushIn);
        button.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, 28.0)));
        button.setAction(Some(action));
        if let Some(target) = ACTION_TARGET.get() {
            button.setTarget(Some(AsRef::<AnyObject>::as_ref(&**target)));
        }
        parent.addSubview(&button);
    }
    button
}

fn add_checkbox(parent: &NSView, title: &str, x: f64, y: f64, w: f64, on: bool, action: Sel) {
    let button = unsafe { NSButton::new(mtm()) };
    unsafe {
        button.setTitle(&NSString::from_str(title));
        button.setButtonType(NSButtonType::Switch);
        let state: isize = if on { 1 } else { 0 };
        let _: () = msg_send![&*button, setState: state];
        button.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, 24.0)));
        button.setAction(Some(action));
        if let Some(target) = ACTION_TARGET.get() {
            button.setTarget(Some(AsRef::<AnyObject>::as_ref(&**target)));
        }
        parent.addSubview(&button);
    }
}

fn add_field(parent: &NSView, placeholder: &str, x: f64, y: f64, w: f64, tag: isize) -> Retained<NSTextField> {
    let field = unsafe { NSTextField::new(mtm()) };
    unsafe {
        field.setEditable(true);
        field.setBezeled(true);
        field.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, 26.0)));
        let _: () = msg_send![&*field, setPlaceholderString: &*NSString::from_str(placeholder)];
        let _: () = msg_send![&*field, setTag: tag];
        parent.addSubview(&field);
    }
    field
}

fn add_popup(parent: &NSView, items: &[&str], selected: usize, x: f64, y: f64, w: f64, action: Sel) {
    let popup = unsafe { NSPopUpButton::new(mtm()) };
    unsafe {
        popup.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, 28.0)));
        for item in items {
            popup.addItemWithTitle(&NSString::from_str(item));
        }
        if !items.is_empty() {
            popup.selectItemAtIndex(selected as NSInteger);
        }
        popup.setAction(Some(action));
        if let Some(target) = ACTION_TARGET.get() {
            popup.setTarget(Some(AsRef::<AnyObject>::as_ref(&**target)));
        }
        parent.addSubview(&popup);
    }
}

fn view_height() -> f64 {
    CONTENT
        .with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|v| unsafe { v.frame().size.height })
        })
        .unwrap_or(WINDOW_HEIGHT as f64)
}

fn build_home() {
    let Some(content) = content_view() else {
        return;
    };
    let Some(app) = APP.get() else {
        return;
    };
    let h = view_height();
    let signed_in = native_settings::is_signed_in(app);
    let key = native_settings::hotkey_label(app);
    let words = native_settings::words_dictated(app);
    add_label(
        &content,
        &format!("{words} words · Hold {key} to talk"),
        16.0,
        24.0,
        h - 40.0,
        700.0,
        24.0,
    );
    if signed_in {
        add_label(&content, "Recent dictations", 13.0, 24.0, h - 68.0, 700.0, 20.0);
    } else {
        add_label(
            &content,
            "Sign in to sync and view transcript history.",
            13.0,
            24.0,
            h - 68.0,
            700.0,
            20.0,
        );
        add_button(&content, "Sign in", 24.0, h - 104.0, 110.0, sel!(signIn:));
    }

    let rows = TRANSCRIPTS.lock().ok().map(|r| r.clone()).unwrap_or_default();
    let mut y = h - if signed_in { 100.0 } else { 140.0 };
    for (i, row) in rows.iter().take(12).enumerate() {
        let line = format!(
            "{}  ·  {}",
            native_settings::truncate(&row.formatted, 70),
            row.app_name.as_deref().unwrap_or("")
        );
        add_label(&content, &line, 12.0, 24.0, y, 560.0, 36.0);
        add_button(
            &content,
            "Copy",
            600.0,
            y + 4.0,
            70.0,
            match i {
                0 => sel!(copy0:),
                1 => sel!(copy1:),
                2 => sel!(copy2:),
                3 => sel!(copy3:),
                4 => sel!(copy4:),
                5 => sel!(copy5:),
                6 => sel!(copy6:),
                7 => sel!(copy7:),
                8 => sel!(copy8:),
                9 => sel!(copy9:),
                10 => sel!(copy10:),
                _ => sel!(copy11:),
            },
        );
        add_button(
            &content,
            "Delete",
            680.0,
            y + 4.0,
            70.0,
            match i {
                0 => sel!(del0:),
                1 => sel!(del1:),
                2 => sel!(del2:),
                3 => sel!(del3:),
                4 => sel!(del4:),
                5 => sel!(del5:),
                6 => sel!(del6:),
                7 => sel!(del7:),
                8 => sel!(del8:),
                9 => sel!(del9:),
                10 => sel!(del10:),
                _ => sel!(del11:),
            },
        );
        y -= 44.0;
        if y < 40.0 {
            break;
        }
    }
}

fn build_dictionary() {
    let Some(content) = content_view() else {
        return;
    };
    let Some(app) = APP.get() else {
        return;
    };
    let h = view_height();
    if !native_settings::is_signed_in(app) {
        add_label(
            &content,
            "Sign in to manage your dictionary.",
            14.0,
            24.0,
            h - 48.0,
            700.0,
            24.0,
        );
        add_button(&content, "Sign in", 24.0, h - 88.0, 110.0, sel!(signIn:));
        return;
    }
    add_label(
        &content,
        "Words the mic should not guess.",
        14.0,
        24.0,
        h - 40.0,
        700.0,
        22.0,
    );
    add_field(&content, "Word or phrase", 24.0, h - 80.0, 200.0, 401);
    add_field(&content, "Sounds like (optional)", 236.0, h - 80.0, 200.0, 402);
    add_button(&content, "Add", 448.0, h - 82.0, 80.0, sel!(addDict:));

    let rows = DICTIONARY.lock().ok().map(|r| r.clone()).unwrap_or_default();
    let mut y = h - 120.0;
    for (i, row) in rows.iter().take(14).enumerate() {
        let sound = row.sounds_like.as_deref().unwrap_or("");
        let line = if sound.is_empty() {
            format!("{} ({})", row.term, row.scope)
        } else {
            format!("{} · {} ({})", row.term, sound, row.scope)
        };
        add_label(&content, &line, 12.0, 24.0, y, 620.0, 28.0);
        add_button(
            &content,
            "Delete",
            660.0,
            y,
            70.0,
            match i {
                0 => sel!(ddel0:),
                1 => sel!(ddel1:),
                2 => sel!(ddel2:),
                3 => sel!(ddel3:),
                4 => sel!(ddel4:),
                5 => sel!(ddel5:),
                6 => sel!(ddel6:),
                7 => sel!(ddel7:),
                8 => sel!(ddel8:),
                9 => sel!(ddel9:),
                10 => sel!(ddel10:),
                11 => sel!(ddel11:),
                12 => sel!(ddel12:),
                _ => sel!(ddel13:),
            },
        );
        y -= 34.0;
        if y < 40.0 {
            break;
        }
    }
}

fn build_snippets() {
    let Some(content) = content_view() else {
        return;
    };
    let Some(app) = APP.get() else {
        return;
    };
    let h = view_height();
    add_label(
        &content,
        "Say the cue, get the saved text.",
        14.0,
        24.0,
        h - 40.0,
        700.0,
        22.0,
    );
    add_field(&content, "Cue, e.g. my address", 24.0, h - 80.0, 200.0, 501);
    add_field(&content, "Text to insert", 236.0, h - 80.0, 320.0, 502);
    add_button(&content, "Add", 568.0, h - 82.0, 80.0, sel!(addSnip:));

    let snippets = native_settings::load_snippets(app);
    let mut y = h - 120.0;
    for (i, snip) in snippets.iter().take(14).enumerate() {
        let line = format!(
            "{} → {}",
            snip.trigger,
            native_settings::truncate(&snip.expansion, 60)
        );
        add_label(&content, &line, 12.0, 24.0, y, 620.0, 28.0);
        add_button(
            &content,
            "Delete",
            660.0,
            y,
            70.0,
            match i {
                0 => sel!(sdel0:),
                1 => sel!(sdel1:),
                2 => sel!(sdel2:),
                3 => sel!(sdel3:),
                4 => sel!(sdel4:),
                5 => sel!(sdel5:),
                6 => sel!(sdel6:),
                7 => sel!(sdel7:),
                8 => sel!(sdel8:),
                9 => sel!(sdel9:),
                10 => sel!(sdel10:),
                11 => sel!(sdel11:),
                12 => sel!(sdel12:),
                _ => sel!(sdel13:),
            },
        );
        y -= 34.0;
        if y < 40.0 {
            break;
        }
    }
}

fn build_settings() {
    let Some(content) = content_view() else {
        return;
    };
    let Some(app) = APP.get() else {
        return;
    };
    let h = view_height();
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
    add_label(&content, &account, 13.0, 24.0, h - 40.0, 700.0, 36.0);
    if signed_in {
        add_button(&content, "Sign out", 24.0, h - 84.0, 110.0, sel!(signOut:));
    } else {
        add_button(&content, "Sign in", 24.0, h - 84.0, 110.0, sel!(signIn:));
    }
    if !crate::inject::can_synthesise_input() {
        add_button(
            &content,
            "Open Accessibility",
            144.0,
            h - 84.0,
            160.0,
            sel!(grant:),
        );
    }

    let orgs = status.as_ref().map(|s| s.orgs.clone()).unwrap_or_default();
    let org_names: Vec<String> = if orgs.is_empty() {
        vec!["Personal".into()]
    } else {
        orgs.iter().map(|o| o.name.clone()).collect()
    };
    let org_refs: Vec<&str> = org_names.iter().map(|s| s.as_str()).collect();
    let org_sel = orgs
        .iter()
        .position(|o| Some(o.org_id.as_str()) == settings.org_id.as_deref())
        .unwrap_or(0);
    add_label(&content, "Organization", 12.0, 24.0, h - 128.0, 160.0, 20.0);
    add_popup(
        &content,
        &org_refs,
        org_sel,
        200.0,
        h - 132.0,
        280.0,
        sel!(orgChanged:),
    );

    add_label(&content, "Hold to talk", 12.0, 24.0, h - 168.0, 160.0, 20.0);
    add_button(
        &content,
        &crate::hotkey::label(&settings.hotkey.accelerator),
        200.0,
        h - 172.0,
        200.0,
        sel!(bindKey:),
    );

    let mics = crate::audio::list_input_devices();
    let mut mic_labels = vec!["System default".to_string()];
    let mut mic_sel = 0usize;
    for (i, mic) in mics.iter().enumerate() {
        mic_labels.push(if mic.is_default {
            format!("{} (default)", mic.name)
        } else {
            mic.name.clone()
        });
        if settings.microphone.as_deref() == Some(mic.name.as_str()) {
            mic_sel = i + 1;
        }
    }
    let mic_refs: Vec<&str> = mic_labels.iter().map(|s| s.as_str()).collect();
    add_label(&content, "Microphone", 12.0, 24.0, h - 208.0, 160.0, 20.0);
    add_popup(
        &content,
        &mic_refs,
        mic_sel,
        200.0,
        h - 212.0,
        280.0,
        sel!(micChanged:),
    );

    add_checkbox(
        &content,
        "Clean up speech",
        24.0,
        h - 252.0,
        280.0,
        settings.clean_up_text,
        sel!(toggleCleanup:),
    );
    add_checkbox(
        &content,
        "Pause media while talking",
        24.0,
        h - 280.0,
        280.0,
        settings.pause_media,
        sel!(togglePause:),
    );
    add_checkbox(
        &content,
        "Keep transcript history",
        24.0,
        h - 308.0,
        280.0,
        settings.keep_history,
        sel!(toggleHistory:),
    );

    let locale_labels: Vec<&str> = LOCALES.iter().map(|(_, l)| *l).collect();
    let locale_sel = LOCALES
        .iter()
        .position(|(v, _)| *v == settings.locale.as_deref().unwrap_or(""))
        .unwrap_or(0);
    add_label(&content, "Language", 12.0, 24.0, h - 348.0, 160.0, 20.0);
    add_popup(
        &content,
        &locale_labels,
        locale_sel,
        200.0,
        h - 352.0,
        280.0,
        sel!(localeChanged:),
    );

    let inj_sel = match settings.injection {
        InjectionPreference::Automatic => 0,
        InjectionPreference::AlwaysType => 1,
        InjectionPreference::AlwaysPaste => 2,
    };
    add_label(&content, "Insert by", 12.0, 24.0, h - 388.0, 160.0, 20.0);
    add_popup(
        &content,
        &["Automatic", "Typing", "Pasting"],
        inj_sel,
        200.0,
        h - 392.0,
        280.0,
        sel!(injectionChanged:),
    );

    add_button(
        &content,
        "Open team dashboard",
        24.0,
        h - 440.0,
        180.0,
        sel!(dashboard:),
    );
    add_button(
        &content,
        "Check for update",
        216.0,
        h - 440.0,
        160.0,
        sel!(update:),
    );
    add_label(
        &content,
        &native_settings::version_footer(app),
        12.0,
        24.0,
        h - 480.0,
        700.0,
        20.0,
    );
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
        // Rebuild on main thread via performSelector - simplest: schedule through overlay path.
        // AppKit UI must be touched from main; use dispatch via NSObject performSelectorOnMainThread.
        dispatch_rebuild();
    });
}

fn dispatch_rebuild() {
    // Post to main by using the action target if available.
    if let Some(target) = ACTION_TARGET.get() {
        unsafe {
            let _: () = msg_send![&**target, performSelectorOnMainThread: sel!(rebuild:) withObject: Option::<&AnyObject>::None waitUntilDone: false];
        }
    }
}

fn field_by_tag(tag: isize) -> Option<String> {
    CONTENT.with(|slot| {
        let content = slot.borrow();
        let content = content.as_ref()?;
        unsafe {
            let subviews = content.subviews();
            for view in subviews.iter() {
                let view_tag: isize = msg_send![&**view, tag];
                if view_tag == tag {
                    let value: Retained<NSString> = msg_send_id![&**view, stringValue];
                    return Some(value.to_string());
                }
            }
        }
        None
    })
}

fn set_page(page: Page) {
    if let Ok(mut slot) = PAGE.lock() {
        *slot = page;
    }
    rebuild_content();
}

fn install_actions() -> Retained<NSObject> {
    ACTION_TARGET.get_or_init(register_controller).clone()
}

fn register_controller() -> Retained<NSObject> {
    static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
    let class = CLASS.get_or_init(|| {
        let mut builder =
            ClassBuilder::new("WeldSpeakHubController", NSObject::class()).expect("class");
        unsafe {
            builder.add_method(sel!(rebuild:), rebuild_action as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(navHome:), nav_home as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(navDict:), nav_dict as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(navSnip:), nav_snip as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(navSet:), nav_set as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(signIn:), sign_in as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(signOut:), sign_out as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(bindKey:), bind_key as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(grant:), grant as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(dashboard:), dashboard as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(update:), update as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(addDict:), add_dict as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(addSnip:), add_snip as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(toggleCleanup:), toggle_cleanup as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(togglePause:), toggle_pause as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(toggleHistory:), toggle_history as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(orgChanged:), org_changed as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(micChanged:), mic_changed as unsafe extern "C" fn(_, _, _));
            builder.add_method(sel!(localeChanged:), locale_changed as unsafe extern "C" fn(_, _, _));
            builder.add_method(
                sel!(injectionChanged:),
                injection_changed as unsafe extern "C" fn(_, _, _),
            );
            macro_rules! copy_del {
                ($(($copy:ident, $del:ident, $idx:expr)),* $(,)?) => {
                    $(
                        builder.add_method(sel!($copy:), paste_copy::<$idx> as unsafe extern "C" fn(_, _, _));
                        builder.add_method(sel!($del:), paste_del::<$idx> as unsafe extern "C" fn(_, _, _));
                    )*
                };
            }
            copy_del!(
                (copy0, del0, 0),
                (copy1, del1, 1),
                (copy2, del2, 2),
                (copy3, del3, 3),
                (copy4, del4, 4),
                (copy5, del5, 5),
                (copy6, del6, 6),
                (copy7, del7, 7),
                (copy8, del8, 8),
                (copy9, del9, 9),
                (copy10, del10, 10),
                (copy11, del11, 11),
            );
            macro_rules! ddel {
                ($(($name:ident, $idx:expr)),* $(,)?) => {
                    $(
                        builder.add_method(sel!($name:), dict_del::<$idx> as unsafe extern "C" fn(_, _, _));
                    )*
                };
            }
            ddel!(
                (ddel0, 0), (ddel1, 1), (ddel2, 2), (ddel3, 3), (ddel4, 4), (ddel5, 5),
                (ddel6, 6), (ddel7, 7), (ddel8, 8), (ddel9, 9), (ddel10, 10), (ddel11, 11),
                (ddel12, 12), (ddel13, 13),
            );
            macro_rules! sdel {
                ($(($name:ident, $idx:expr)),* $(,)?) => {
                    $(
                        builder.add_method(sel!($name:), snip_del::<$idx> as unsafe extern "C" fn(_, _, _));
                    )*
                };
            }
            sdel!(
                (sdel0, 0), (sdel1, 1), (sdel2, 2), (sdel3, 3), (sdel4, 4), (sdel5, 5),
                (sdel6, 6), (sdel7, 7), (sdel8, 8), (sdel9, 9), (sdel10, 10), (sdel11, 11),
                (sdel12, 12), (sdel13, 13),
            );
        }
        builder.register()
    });
    unsafe { msg_send_id![*class, new] }
}

unsafe extern "C" fn rebuild_action(_this: &NSObject, _cmd: Sel, _sender: Option<&AnyObject>) {
    rebuild_content();
}

unsafe extern "C" fn nav_home(_this: &NSObject, _cmd: Sel, _sender: Option<&AnyObject>) {
    set_page(Page::Home);
}
unsafe extern "C" fn nav_dict(_this: &NSObject, _cmd: Sel, _sender: Option<&AnyObject>) {
    set_page(Page::Dictionary);
}
unsafe extern "C" fn nav_snip(_this: &NSObject, _cmd: Sel, _sender: Option<&AnyObject>) {
    set_page(Page::Snippets);
}
unsafe extern "C" fn nav_set(_this: &NSObject, _cmd: Sel, _sender: Option<&AnyObject>) {
    set_page(Page::Settings);
}

unsafe extern "C" fn sign_in(_this: &NSObject, _cmd: Sel, _sender: Option<&AnyObject>) {
    let Some(app) = APP.get() else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match crate::commands::begin_sign_in(app.clone()).await {
            Ok(started) => {
                crate::overlay::show_status(&app, &format!("Confirm {}", started.user_code))
            }
            Err(error) => crate::overlay::show_notice(&app, &error),
        }
    });
}

unsafe extern "C" fn sign_out(_this: &NSObject, _cmd: Sel, _sender: Option<&AnyObject>) {
    if let Some(app) = APP.get() {
        let _ = crate::commands::sign_out(app.state());
        load_async_data(app);
        rebuild_content();
    }
}

unsafe extern "C" fn bind_key(_this: &NSObject, _cmd: Sel, _sender: Option<&AnyObject>) {
    let Some(app) = APP.get() else {
        return;
    };
    let app = app.clone();
    crate::hotkey::suspend(true);
    crate::overlay::show_status(&app, "Press a key…");
    tauri::async_runtime::spawn(async move {
        let mut found = None;
        for _ in 0..200 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            if let Some(code) = crate::hotkey::first_held_code() {
                found = Some(code);
                break;
            }
        }
        crate::hotkey::suspend(false);
        if let Some(code) = found {
            match native_settings::set_hotkey(&app, code.clone()) {
                Ok(()) => {
                    crate::overlay::show_notice(&app, &format!("Hold {}", crate::hotkey::label(&code)));
                    dispatch_rebuild();
                }
                Err(err) => crate::overlay::show_notice(&app, &err),
            }
        } else {
            crate::overlay::dismiss(&app);
        }
    });
}

unsafe extern "C" fn grant(_this: &NSObject, _cmd: Sel, _sender: Option<&AnyObject>) {
    let _ = crate::inject::open_permission_settings();
}

unsafe extern "C" fn dashboard(_this: &NSObject, _cmd: Sel, _sender: Option<&AnyObject>) {
    let _ = crate::commands::open_in_browser(DASHBOARD_URL);
}

unsafe extern "C" fn update(_this: &NSObject, _cmd: Sel, _sender: Option<&AnyObject>) {
    let Some(app) = APP.get() else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match crate::updater::install_update(app.clone()).await {
            Ok(msg) => crate::overlay::show_notice(&app, &msg),
            Err(err) => crate::overlay::show_notice(&app, &err),
        }
    });
}

unsafe extern "C" fn add_dict(_this: &NSObject, _cmd: Sel, _sender: Option<&AnyObject>) {
    let Some(app) = APP.get() else {
        return;
    };
    let term = field_by_tag(401).unwrap_or_default();
    let sound = field_by_tag(402).unwrap_or_default();
    let sounds = if sound.trim().is_empty() {
        None
    } else {
        Some(sound)
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match crate::commands::add_dictionary_term(app.clone(), term, sounds).await {
            Ok(_) => {
                load_async_data(&app);
            }
            Err(err) => crate::overlay::show_notice(&app, &err),
        }
    });
}

unsafe extern "C" fn add_snip(_this: &NSObject, _cmd: Sel, _sender: Option<&AnyObject>) {
    let Some(app) = APP.get() else {
        return;
    };
    let trigger = field_by_tag(501).unwrap_or_default();
    let expansion = field_by_tag(502).unwrap_or_default();
    match native_settings::add_snippet(app, trigger, expansion) {
        Ok(()) => rebuild_content(),
        Err(err) => crate::overlay::show_notice(app, &err),
    }
}

unsafe extern "C" fn toggle_cleanup(_this: &NSObject, _cmd: Sel, sender: Option<&AnyObject>) {
    if let (Some(app), Some(sender)) = (APP.get(), sender) {
        let on: isize = msg_send![sender, state];
        native_settings::set_clean_up(app, on == 1);
    }
}

unsafe extern "C" fn toggle_pause(_this: &NSObject, _cmd: Sel, sender: Option<&AnyObject>) {
    if let (Some(app), Some(sender)) = (APP.get(), sender) {
        let on: isize = msg_send![sender, state];
        native_settings::set_pause_media(app, on == 1);
    }
}

unsafe extern "C" fn toggle_history(_this: &NSObject, _cmd: Sel, sender: Option<&AnyObject>) {
    if let (Some(app), Some(sender)) = (APP.get(), sender) {
        let on: isize = msg_send![sender, state];
        native_settings::set_keep_history(app, on == 1);
    }
}

unsafe extern "C" fn org_changed(_this: &NSObject, _cmd: Sel, sender: Option<&AnyObject>) {
    let Some(app) = APP.get() else {
        return;
    };
    let Some(sender) = sender else {
        return;
    };
    let idx: isize = msg_send![sender, indexOfSelectedItem];
    let org_id = STATUS
        .lock()
        .ok()
        .and_then(|s| s.clone())
        .and_then(|s| s.orgs.get(idx as usize).map(|o| o.org_id.clone()));
    native_settings::set_org(app, org_id);
}

unsafe extern "C" fn mic_changed(_this: &NSObject, _cmd: Sel, sender: Option<&AnyObject>) {
    let Some(app) = APP.get() else {
        return;
    };
    let Some(sender) = sender else {
        return;
    };
    let idx: isize = msg_send![sender, indexOfSelectedItem];
    let name = if idx <= 0 {
        None
    } else {
        crate::audio::list_input_devices()
            .get((idx as usize) - 1)
            .map(|m| m.name.clone())
    };
    native_settings::set_microphone(app, name);
}

unsafe extern "C" fn locale_changed(_this: &NSObject, _cmd: Sel, sender: Option<&AnyObject>) {
    let Some(app) = APP.get() else {
        return;
    };
    let Some(sender) = sender else {
        return;
    };
    let idx: isize = msg_send![sender, indexOfSelectedItem];
    let value = LOCALES
        .get(idx.max(0) as usize)
        .map(|(v, _)| *v)
        .unwrap_or("");
    native_settings::set_locale(
        app,
        if value.is_empty() {
            None
        } else {
            Some(value.into())
        },
    );
}

unsafe extern "C" fn injection_changed(_this: &NSObject, _cmd: Sel, sender: Option<&AnyObject>) {
    let Some(app) = APP.get() else {
        return;
    };
    let Some(sender) = sender else {
        return;
    };
    let idx: isize = msg_send![sender, indexOfSelectedItem];
    let pref = match idx {
        1 => InjectionPreference::AlwaysType,
        2 => InjectionPreference::AlwaysPaste,
        _ => InjectionPreference::Automatic,
    };
    native_settings::set_injection(app, pref);
}

unsafe extern "C" fn paste_copy<const I: usize>(
    _this: &NSObject,
    _cmd: Sel,
    _sender: Option<&AnyObject>,
) {
    if let Ok(rows) = TRANSCRIPTS.lock() {
        if let Some(row) = rows.get(I) {
            let _ = native_settings::copy_transcript_text(&row.formatted);
            if let Some(app) = APP.get() {
                crate::overlay::show_notice(app, "Copied");
            }
        }
    }
}

unsafe extern "C" fn paste_del<const I: usize>(
    _this: &NSObject,
    _cmd: Sel,
    _sender: Option<&AnyObject>,
) {
    let id = TRANSCRIPTS
        .lock()
        .ok()
        .and_then(|rows| rows.get(I).map(|r| r.id.clone()));
    let Some(app) = APP.get() else {
        return;
    };
    if let Some(id) = id {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if crate::commands::delete_transcript(app.clone(), id).await.is_ok() {
                load_async_data(&app);
            }
        });
    }
}

unsafe extern "C" fn dict_del<const I: usize>(
    _this: &NSObject,
    _cmd: Sel,
    _sender: Option<&AnyObject>,
) {
    let id = DICTIONARY
        .lock()
        .ok()
        .and_then(|rows| rows.get(I).map(|r| r.id.clone()));
    let Some(app) = APP.get() else {
        return;
    };
    if let Some(id) = id {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if crate::commands::delete_dictionary_term(app.clone(), id)
                .await
                .is_ok()
            {
                load_async_data(&app);
            }
        });
    }
}

unsafe extern "C" fn snip_del<const I: usize>(
    _this: &NSObject,
    _cmd: Sel,
    _sender: Option<&AnyObject>,
) {
    if let Some(app) = APP.get() {
        native_settings::remove_snippet(app, I);
        rebuild_content();
    }
}

//! The window's menus where a Mac keeps them: in the system menu bar, not in
//! the window.
//!
//! macOS gives every application a bar of its own, and a window that draws its
//! own strip of File/View/Help instead reads as a port of something else. So
//! on macOS the strip is not drawn at all and these menus stand in its place —
//! the same three, in the same order, from the same `Command` lists, with the
//! same chords. Every other platform keeps the strip, because it is the only
//! bar those platforms have.
//!
//! Edit is the one menu here that the strip has no counterpart for: a Mac
//! application is expected to carry one, and a menu's key equivalent is
//! answered before the window sees the key, which is what lets ⌘V reach the
//! editor at all — see [`EDIT_MENU`].
//!
//! Clicks come back as commands rather than as calls: AppKit hands an action
//! to an Objective-C object on the main thread, which pushes the item's tag —
//! its index into [`ALL`] — onto a queue that the next frame drains. The
//! alternative is handing AppKit a pointer to the app itself, which nothing
//! about a menu click can promise is still there.
//!
//! The Dock icon's own menu is here too, and answers the same way. It is the
//! menu Zed shows: the projects opened before, then the window that is up,
//! then New Window, with the system's own rows under them. One window is one
//! process here, so New Window starts another copy of the editor — through
//! `open -n` where this one is a bundled application, since that is what gives
//! the new copy a Dock tile and a menu bar of its own.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, msg_send, sel, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOn, NSEventModifierFlags, NSImage, NSImageNameFolder, NSMenu,
    NSMenuItem,
};
use objc2_foundation::{MainThreadMarker, NSString};

use crate::core::command::{Chord, Command, ALL, EDIT_MENU, FILE_MENU, HELP_MENU, VIEW_MENU};
use crate::core::settings::Settings;

/// Commands the bar has asked for and the window has not yet run. A queue
/// rather than a slot: a menu can be clicked twice before a frame is drawn.
static PENDING: Mutex<Vec<Command>> = Mutex::new(Vec::new());

/// Folders the Dock menu has asked for, drained the same way.
static OPENING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// How many windows the Dock menu has asked for. Starting another copy is
/// left to the frame, which has somewhere to say so when it fails.
static WANTED_WINDOWS: Mutex<usize> = Mutex::new(0);

/// What the Dock menu lists. It is built when the icon is clicked, which is
/// long after the frame that last knew any of this.
struct Dock {
    recent: Vec<PathBuf>,
    open: Option<PathBuf>,
}

static DOCK: Mutex<Dock> = Mutex::new(Dock {
    recent: Vec::new(),
    open: None,
});

/// The object every menu row points at. The Dock menu is built outside any
/// frame and needs the same one, which is why it is kept rather than made
/// again: a row whose target has been freed does nothing at all.
static TARGET: AtomicPtr<MenuTarget> = AtomicPtr::new(std::ptr::null_mut());

define_class!(
    // The object AppKit sends every one of our menu actions to. It holds
    // nothing: the item that was clicked carries its own command in its tag.
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "YaraMenuTarget"]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(runCommand:))]
        fn run_command(&self, sender: Option<&AnyObject>) {
            let Some(sender) = sender else { return };
            let tag: isize = unsafe { msg_send![sender, tag] };
            if let Some(command) = command_of(tag) {
                if let Ok(mut pending) = PENDING.lock() {
                    pending.push(command);
                }
            }
        }

        #[unsafe(method(newWindow:))]
        fn new_window(&self, _sender: Option<&AnyObject>) {
            if let Ok(mut wanted) = WANTED_WINDOWS.lock() {
                *wanted += 1;
            }
        }

        #[unsafe(method(openFolder:))]
        fn open_folder(&self, sender: Option<&AnyObject>) {
            let Some(sender) = sender else { return };
            let tag: isize = unsafe { msg_send![sender, tag] };
            let path = DOCK
                .lock()
                .ok()
                .and_then(|dock| usize::try_from(tag).ok().and_then(|i| dock.recent.get(i).cloned()));
            if let (Some(path), Ok(mut opening)) = (path, OPENING.lock()) {
                opening.push(path);
            }
        }
    }
);

/// The tag a row carries, and the command it stands for. A menu item can hold
/// one integer and nothing else, so the two have to agree; they are each
/// other's inverse over [`ALL`].
fn tag_of(command: Command) -> isize {
    ALL.iter().position(|c| *c == command).unwrap_or(0) as isize
}

fn command_of(tag: isize) -> Option<Command> {
    ALL.get(usize::try_from(tag).ok()?).copied()
}

/// What the bar has asked for since this was last called.
pub fn drain() -> Vec<Command> {
    PENDING
        .lock()
        .map(|mut pending| std::mem::take(&mut *pending))
        .unwrap_or_default()
}

/// The folders the Dock menu has been asked to open since this was last
/// called.
pub fn drain_folders() -> Vec<PathBuf> {
    OPENING
        .lock()
        .map(|mut opening| std::mem::take(&mut *opening))
        .unwrap_or_default()
}

/// How many new windows the Dock menu has asked for since this was last
/// called.
pub fn drain_new_windows() -> usize {
    WANTED_WINDOWS
        .lock()
        .map(|mut wanted| std::mem::replace(&mut *wanted, 0))
        .unwrap_or(0)
}

/// Starts another copy of the editor, with no folder in it — the Dock menu's
/// New Window. One window is one process, so a window is what a process is.
pub fn open_new_window() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let (program, args) = relaunch(&exe);
    // The child is left to itself: it is another editor, not a job of this
    // one, and it outlives the window that asked for it.
    std::process::Command::new(program).args(args).spawn()?;
    Ok(())
}

/// How to start that copy: a bundled application goes through `open -n`, which
/// is what makes macOS treat it as another running copy rather than as a
/// stray binary; anything else is simply run again.
fn relaunch(exe: &Path) -> (PathBuf, Vec<PathBuf>) {
    match bundle_of(exe) {
        Some(bundle) => (
            PathBuf::from("/usr/bin/open"),
            vec![PathBuf::from("-n"), bundle],
        ),
        None => (exe.to_path_buf(), Vec::new()),
    }
}

/// The `.app` a binary is inside, if it is inside one at all:
/// `…/Yara Code.app/Contents/MacOS/ycode-gui`.
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app")
        .then(|| bundle.to_path_buf())
}

/// Tells the Dock menu what to list. Called on every frame that has a bar to
/// keep, and does nothing on the frames where nothing has moved.
pub fn set_dock(recent: &[PathBuf], open: Option<&Path>) {
    let Ok(mut dock) = DOCK.lock() else {
        return;
    };
    if dock.recent != recent || dock.open.as_deref() != open {
        dock.recent = recent.to_vec();
        dock.open = open.map(Path::to_path_buf);
    }
    // The delegate the hook needs may not have been up when the bar was
    // built, and the bar is not built again unless a menu changes.
    if let Some(mtm) = MainThreadMarker::new() {
        dock_menu_hook(&NSApplication::sharedApplication(mtm));
    }
}

/// Builds the bar. Call once, from the main thread, with the window already
/// up: before `NSApplication` exists there is nothing to hang a menu on.
///
/// `has_update` is asked the same question the in-window menu asks — Install
/// Update is offered only once a check has found one — so the bar is rebuilt
/// when the answer changes.
pub fn install(settings: &Settings, has_update: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let target = MenuTarget::alloc(mtm);
    let target: Retained<MenuTarget> = unsafe { msg_send![target, init] };

    let bar = NSMenu::new(mtm);
    bar.addItem(&submenu(mtm, "Yara Code", app_menu(mtm, &target, settings)));
    for (title, entries) in [
        ("File", FILE_MENU),
        ("Edit", EDIT_MENU),
        ("View", VIEW_MENU),
        ("Help", HELP_MENU),
    ] {
        let menu = NSMenu::new(mtm);
        menu.setTitle(&NSString::from_str(title));
        let mut last_was_separator = true;
        for entry in entries {
            match entry {
                None if last_was_separator => continue,
                None => {
                    menu.addItem(&NSMenuItem::separatorItem(mtm));
                    last_was_separator = true;
                }
                Some(command) => {
                    if skip(*command, has_update) {
                        continue;
                    }
                    menu.addItem(&command_item(mtm, &target, settings, *command));
                    last_was_separator = false;
                }
            }
        }
        bar.addItem(&submenu(mtm, title, menu));
    }

    // The target is AppKit's from here on: the bar outlives every frame, and
    // dropping it would leave the menu items pointing at freed memory.
    TARGET.store(Retained::as_ptr(&target).cast_mut(), Ordering::Release);
    std::mem::forget(target);
    app.setMainMenu(Some(&bar));
    dock_menu_hook(&app);
}

/// Teaches the running application's delegate to answer `applicationDockMenu:`
/// — the one way AppKit asks for the menu behind the Dock icon. The delegate
/// belongs to the window library rather than to this crate, so the method is
/// added to its class at runtime instead of being written on it; nothing else
/// implements it, so nothing is being taken over.
fn dock_menu_hook(app: &NSApplication) {
    use std::sync::atomic::AtomicBool;
    static HOOKED: AtomicBool = AtomicBool::new(false);
    if HOOKED.load(Ordering::Acquire) {
        return;
    }
    // On the frames before the delegate is up there is nothing to teach, and
    // the next rebuild of the bar tries again.
    let Some(delegate) = app.delegate() else {
        return;
    };
    let class: *const AnyClass = unsafe { msg_send![&*delegate, class] };
    let types = c"@@:@";
    let added = unsafe {
        objc2::ffi::class_addMethod(
            class.cast_mut(),
            sel!(applicationDockMenu:),
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) -> *mut NSMenu,
                objc2::runtime::Imp,
            >(dock_menu),
            types.as_ptr(),
        )
    };
    HOOKED.store(added.as_bool(), Ordering::Release);
}

/// What AppKit calls when the Dock icon is held. The menu is built here and
/// not kept: what it lists — the recent projects, the folder in the window —
/// is whatever the last frame left behind.
unsafe extern "C-unwind" fn dock_menu(
    _this: &AnyObject,
    _cmd: Sel,
    _sender: *mut AnyObject,
) -> *mut NSMenu {
    let Some(mtm) = MainThreadMarker::new() else {
        return std::ptr::null_mut();
    };
    Retained::autorelease_return(dock_menu_for(mtm))
}

fn dock_menu_for(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    let target = TARGET.load(Ordering::Acquire);
    let Ok(dock) = DOCK.lock() else {
        return menu;
    };
    if target.is_null() {
        return menu;
    }
    // Safety: the target was leaked into AppKit's hands when the bar was
    // built, and nothing takes it back.
    let target: &MenuTarget = unsafe { &*target };
    let folder = unsafe { NSImage::imageNamed(NSImageNameFolder) };
    for (i, path) in dock.recent.iter().enumerate().take(8) {
        let item = NSMenuItem::new(mtm);
        item.setTitle(&NSString::from_str(&name_of(path)));
        unsafe {
            item.setAction(Some(sel!(openFolder:)));
            item.setTarget(Some(target));
            item.setTag(i as isize);
        }
        item.setImage(folder.as_deref());
        menu.addItem(&item);
    }
    // The window itself, under the projects it could be showing instead — a
    // tick on the one it is showing, and a click that brings it forward.
    if let Some(open) = &dock.open {
        if !dock.recent.is_empty() {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
        }
        let item = system_item(mtm, &name_of(open), sel!(arrangeInFront:), "");
        item.setState(NSControlStateValueOn);
        menu.addItem(&item);
    }
    if !dock.recent.is_empty() || dock.open.is_some() {
        menu.addItem(&NSMenuItem::separatorItem(mtm));
    }
    let new_window = NSMenuItem::new(mtm);
    new_window.setTitle(&NSString::from_str("New Window"));
    unsafe {
        new_window.setAction(Some(sel!(newWindow:)));
        new_window.setTarget(Some(target));
    }
    menu.addItem(&new_window);
    menu
}

/// A folder as a menu names it: the folder itself, not the road to it.
fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Whether an entry of one of the four menus is left out of it on macOS.
/// Settings and Quit belong to the application menu here, and putting them in
/// both would give two rows the same chord — with Quit that matters, because
/// the row AppKit would reach first is its own `terminate:`, which does not
/// stop to ask about unsaved work.
fn skip(command: Command, has_update: bool) -> bool {
    match command {
        Command::Settings | Command::Quit => true,
        // Offered only once a check has found something to install, as in the
        // window's own menu.
        Command::InstallUpdate => !has_update,
        _ => false,
    }
}

/// The menu every Mac application carries first, in the order every Mac
/// application carries it. Hiding and the About panel are AppKit's own, so
/// they are wired to AppKit's own selectors; Settings and Quit are ours, and
/// Quit especially has to be — it is the one that asks about unsaved work.
fn app_menu(mtm: MainThreadMarker, target: &MenuTarget, settings: &Settings) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    menu.addItem(&system_item(
        mtm,
        "About Yara Code",
        sel!(orderFrontStandardAboutPanel:),
        "",
    ));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&command_item(mtm, target, settings, Command::Settings));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&system_item(mtm, "Hide Yara Code", sel!(hide:), "h"));
    let hide_others = system_item(mtm, "Hide Others", sel!(hideOtherApplications:), "h");
    hide_others
        .setKeyEquivalentModifierMask(NSEventModifierFlags::Command | NSEventModifierFlags::Option);
    menu.addItem(&hide_others);
    menu.addItem(&system_item(
        mtm,
        "Show All",
        sel!(unhideAllApplications:),
        "",
    ));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let quit = command_item(mtm, target, settings, Command::Quit);
    quit.setTitle(&NSString::from_str("Quit Yara Code"));
    menu.addItem(&quit);
    menu
}

/// A row that runs one of ours, carrying its command as the tag AppKit hands
/// back and its chord as the key equivalent the bar prints on the right.
fn command_item(
    mtm: MainThreadMarker,
    target: &MenuTarget,
    settings: &Settings,
    command: Command,
) -> Retained<NSMenuItem> {
    let item = NSMenuItem::new(mtm);
    unsafe {
        item.setTitle(&NSString::from_str(command.label()));
        item.setAction(Some(sel!(runCommand:)));
        item.setTarget(Some(target));
        item.setTag(tag_of(command));
        if let Some(chord) = settings.gui_chord(command) {
            let (key, mask) = key_equivalent(chord);
            item.setKeyEquivalent(&NSString::from_str(&key));
            item.setKeyEquivalentModifierMask(mask);
        }
    }
    item
}

/// A row that runs one of AppKit's, which needs no target: an action with none
/// walks the responder chain until something answers to it, and for these the
/// application itself does.
fn system_item(mtm: MainThreadMarker, title: &str, action: Sel, key: &str) -> Retained<NSMenuItem> {
    let item = NSMenuItem::new(mtm);
    item.setTitle(&NSString::from_str(title));
    unsafe { item.setAction(Some(action)) };
    item.setKeyEquivalent(&NSString::from_str(key));
    item
}

/// A menu bar holds items, and each item holds the menu that drops from it —
/// the title on the bar is the item's, not the menu's.
fn submenu(mtm: MainThreadMarker, title: &str, menu: Retained<NSMenu>) -> Retained<NSMenuItem> {
    let item = NSMenuItem::new(mtm);
    item.setTitle(&NSString::from_str(title));
    item.setSubmenu(Some(&menu));
    item
}

/// A chord as AppKit spells it: the key on its own, and the modifiers beside
/// it as a mask. Shift is left out of the mask where the key is a letter —
/// AppKit reads an upper-case key equivalent as shifted already, and setting
/// both draws the arrow twice.
fn key_equivalent(chord: &Chord) -> (String, NSEventModifierFlags) {
    let mut mask = NSEventModifierFlags::empty();
    if chord.mods.cmd {
        mask |= NSEventModifierFlags::Command;
    }
    if chord.mods.ctrl {
        mask |= NSEventModifierFlags::Control;
    }
    if chord.mods.alt {
        mask |= NSEventModifierFlags::Option;
    }
    let key = match &chord.key {
        crate::core::command::Key::Char(c) => c.to_string(),
        crate::core::command::Key::Named(name) => match name.as_str() {
            "left" => "\u{f702}".to_string(),
            "right" => "\u{f703}".to_string(),
            "up" => "\u{f700}".to_string(),
            "down" => "\u{f701}".to_string(),
            "enter" => "\r".to_string(),
            "tab" => "\t".to_string(),
            "delete" => "\u{f728}".to_string(),
            "home" => "\u{f729}".to_string(),
            "end" => "\u{f72b}".to_string(),
            "pageup" => "\u{f72c}".to_string(),
            "pagedown" => "\u{f72d}".to_string(),
            // Function keys start at F1, which is 0xF704.
            other if other.starts_with('f') => match other[1..].parse::<u32>() {
                Ok(n) if (1..=24).contains(&n) => char::from_u32(0xf704 + n - 1)
                    .map(String::from)
                    .unwrap_or_default(),
                _ => String::new(),
            },
            _ => String::new(),
        },
    };
    if chord.mods.shift && !key.chars().all(|c| c.is_ascii_lowercase()) {
        mask |= NSEventModifierFlags::Shift;
    }
    let key = if chord.mods.shift {
        key.to_uppercase()
    } else {
        key
    };
    (key, mask)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::command::{Key, Mods};

    #[test]
    fn a_bundled_editor_starts_its_next_window_through_open() {
        let bundled = Path::new("/Applications/Yara Code.app/Contents/MacOS/ycode-gui");
        assert_eq!(
            bundle_of(bundled).as_deref(),
            Some(Path::new("/Applications/Yara Code.app"))
        );
        let (program, args) = relaunch(bundled);
        assert_eq!(program, Path::new("/usr/bin/open"));
        assert_eq!(
            args,
            ["-n", "/Applications/Yara Code.app"].map(PathBuf::from)
        );

        // A binary that is not in a bundle — a Homebrew install, a build in
        // the tree — is the thing to run again.
        let plain = Path::new("/opt/homebrew/bin/ycode-gui");
        assert_eq!(bundle_of(plain), None);
        assert_eq!(relaunch(plain), (plain.to_path_buf(), Vec::new()));
    }

    #[test]
    fn a_chord_reads_as_appkit_spells_it() {
        let of = |text: &str| key_equivalent(&text.parse::<Chord>().expect("a chord"));

        let (key, mask) = of("Cmd+S");
        assert_eq!(key, "s");
        assert_eq!(mask, NSEventModifierFlags::Command);

        // A shifted letter is upper-cased rather than flagged: AppKit reads an
        // upper-case equivalent as shifted already, and doing both draws the
        // arrow twice.
        let (key, mask) = of("Cmd+Shift+P");
        assert_eq!(key, "P");
        assert_eq!(mask, NSEventModifierFlags::Command);

        // A named key has no case to carry it, so the flag is what says Shift.
        let (key, mask) = of("Cmd+Shift+Left");
        assert_eq!(key, "\u{f702}");
        assert_eq!(
            mask,
            NSEventModifierFlags::Command | NSEventModifierFlags::Shift
        );

        let (key, mask) = of("Alt+Left");
        assert_eq!(key, "\u{f702}");
        assert_eq!(mask, NSEventModifierFlags::Option);

        // F1 is where the function keys start.
        assert_eq!(of("F1").0, "\u{f704}");
        assert_eq!(of("F12").0, "\u{f70f}");
    }

    #[test]
    fn a_key_appkit_has_no_name_for_binds_nothing_rather_than_something_wrong() {
        let chord = Chord {
            mods: Mods {
                cmd: true,
                ctrl: false,
                alt: false,
                shift: false,
            },
            key: Key::Named("scrolllock".into()),
        };
        assert_eq!(key_equivalent(&chord).0, "");
    }

    #[test]
    fn every_row_finds_its_way_back_to_the_command_it_stands_for() {
        // A menu item holds one integer, so the round trip through it is the
        // whole of what a click carries.
        for command in ALL {
            assert_eq!(command_of(tag_of(*command)), Some(*command));
        }
        assert_eq!(command_of(-1), None);
        assert_eq!(command_of(ALL.len() as isize), None);
    }

    #[test]
    fn the_application_menu_owns_settings_and_quit_alone() {
        // Both are in the File menu the other platforms draw; on a Mac they
        // belong to the application menu, and a second row with the same chord
        // would be the one AppKit reaches first.
        assert!(FILE_MENU.contains(&Some(Command::Quit)));
        assert!(FILE_MENU.contains(&Some(Command::Settings)));
        assert!(skip(Command::Quit, false));
        assert!(skip(Command::Settings, false));
        assert!(!skip(Command::Save, false));
        assert!(skip(Command::InstallUpdate, false));
        assert!(!skip(Command::InstallUpdate, true));
    }
}

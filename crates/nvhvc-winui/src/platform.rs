//! Windows-side helpers: native file dialogs, Explorer, the system accent colour and the Esc key.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Foundation::TypedEventHandler;
use windows::UI::ViewManagement::{UIColorType, UISettings};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetActiveWindow, VK_ESCAPE};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_PICKFOLDERS, FileOpenDialog, FileSaveDialog, IFileDialog, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HC_ACTION, MSG, PM_REMOVE, SetWindowsHookExW, WH_GETMESSAGE, WM_KEYDOWN,
};
use windows::core::{HSTRING, PCWSTR, w};
use windows_reactor::Color;

use crate::model::Container;
use crate::palette::Accent;

/// Shows a native Open file dialog (source/encode picks), showing only the video files the
/// pipeline reads (one generic filter, not a per-container choice). Returns the chosen path,
/// or `None` if cancelled.
pub fn pick_file() -> Option<String> {
    unsafe {
        let dialog: IFileDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let types = [COMDLG_FILTERSPEC {
            pszName: w!("Video files"),
            pszSpec: w!("*.mkv;*.mp4"),
        }];
        let _ = dialog.SetFileTypes(&types);
        show_dialog(&dialog)
    }
}

/// Shows the native Save dialog for the output file, offering only the containers the
/// pipeline can write (.mkv / .mp4). The preselected type follows the wizard's current
/// container, and the dialog appends the matching extension to a typed bare name.
pub fn pick_save_file(container: Container, initial: Option<&str>) -> Option<String> {
    unsafe {
        let dialog: IFileDialog =
            CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        // Prefill the name the wizard already shows on the output card (mirrors GTK).
        if let Some(name) = initial {
            let name = HSTRING::from(name);
            let _ = dialog.SetFileName(PCWSTR(name.as_ptr()));
        }
        // Same order as `Container::ALL`; the type index below is 1-based.
        let types = [
            COMDLG_FILTERSPEC {
                pszName: w!("Matroska video"),
                pszSpec: w!("*.mkv"),
            },
            COMDLG_FILTERSPEC {
                pszName: w!("MP4 video"),
                pszSpec: w!("*.mp4"),
            },
        ];
        let _ = dialog.SetFileTypes(&types);
        let (index, ext) = match container {
            Container::Mkv => (1, w!("mkv")),
            Container::Mp4 => (2, w!("mp4")),
        };
        let _ = dialog.SetFileTypeIndex(index);
        let _ = dialog.SetDefaultExtension(ext);
        show_dialog(&dialog)
    }
}

/// Shows a native folder picker; returns the chosen folder, or `None` if cancelled.
pub fn pick_folder() -> Option<String> {
    unsafe {
        let dialog: IFileDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().ok()?;
        dialog.SetOptions(options | FOS_PICKFOLDERS).ok()?;
        show_dialog(&dialog)
    }
}

/// Runs a common-item dialog, modal to the app window, and extracts the picked filesystem path.
///
/// # Safety
/// Must be called on the UI thread (every caller runs from `Component::update`).
unsafe fn show_dialog(dialog: &IFileDialog) -> Option<String> {
    unsafe {
        // The app window is the active one when a click opened the dialog. Owning the dialog
        // keeps the window disabled until it closes.
        let owner = GetActiveWindow();
        let owner = (owner != HWND::default()).then_some(owner);
        // `Show` returns an error when the user cancels.
        FILE_DIALOG_OPEN.store(true, Ordering::Relaxed);
        let shown = dialog.Show(owner);
        FILE_DIALOG_OPEN.store(false, Ordering::Relaxed);
        shown.ok()?;
        let item = dialog.GetResult().ok()?;
        let pwstr = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = pwstr.to_string().ok()?;
        CoTaskMemFree(Some(pwstr.0 as *const _));
        Some(path)
    }
}

/// Opens a file with its default handler (e.g. plays the output video).
pub fn open_path(path: &str) {
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

/// Opens Explorer with the file selected.
///
/// The argument must be passed raw: std quotes an argument containing spaces as one token
/// (`"/select,C:\out dir\x.mp4"`), which Explorer misparses and silently falls back to
/// opening the Documents folder. The path itself is quoted, the `/select,` switch is not.
pub fn reveal_in_folder(path: &str) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{path}\""))
        .spawn();
}

/// The system accent shades, or Windows' default blue if they can't be read.
pub fn accent() -> Accent {
    let read = || -> windows::core::Result<Accent> {
        let settings = UISettings::new()?;
        let get = |kind| {
            settings
                .GetColorValue(kind)
                .map(|c| Color::argb(c.A, c.R, c.G, c.B))
        };
        Ok(Accent {
            light2: get(UIColorType::AccentLight2)?,
            light3: get(UIColorType::AccentLight3)?,
            dark1: get(UIColorType::AccentDark1)?,
            dark2: get(UIColorType::AccentDark2)?,
        })
    };
    read().unwrap_or_default()
}

/// Whether Windows is in dark app mode (its app background colour is black then).
pub fn system_is_dark() -> bool {
    UISettings::new()
        .and_then(|settings| settings.GetColorValue(UIColorType::Background))
        .is_ok_and(|c| c.R < 128)
}

/// Calls `notify` on a worker thread whenever the system colours change (accent, light/dark
/// mode). The subscription lasts as long as the returned object lives.
pub fn watch_colours(notify: impl Fn() + Send + 'static) -> Option<UISettings> {
    let settings = UISettings::new().ok()?;
    settings
        .ColorValuesChanged(&TypedEventHandler::new(move |_, _| {
            notify();
            Ok(())
        }))
        .ok()?;
    Some(settings)
}

/// A file dialog runs its message loop on the UI thread, so its Esc reaches [`message_proc`].
static FILE_DIALOG_OPEN: AtomicBool = AtomicBool::new(false);
static ON_ESCAPE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Calls `notify` each time Esc is pressed in the app's window. Call once, on the UI thread.
///
/// reactor 0.100 has no key events and its accelerators have no Escape, so this is a message
/// hook. It is on the UI thread only, not system-wide, and lasts until the thread exits.
pub fn watch_escape(notify: impl Fn() + Send + Sync + 'static) {
    if ON_ESCAPE.set(Box::new(notify)).is_ok() {
        // SAFETY: `message_proc` is a valid hook procedure for the life of the process.
        let _ = unsafe {
            SetWindowsHookExW(
                WH_GETMESSAGE,
                Some(message_proc),
                None,
                GetCurrentThreadId(),
            )
        };
    }
}

unsafe extern "system" fn message_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // A message is only looked at when it leaves the queue, or a peek would count it twice.
    if code == HC_ACTION as i32 && wparam.0 == PM_REMOVE.0 as usize {
        // SAFETY: for WH_GETMESSAGE, `lparam` points at the message being retrieved.
        let msg = unsafe { &*(lparam.0 as *const MSG) };
        // Bit 30 of a key message's lParam is set for an auto-repeat.
        if msg.message == WM_KEYDOWN
            && msg.wParam.0 == usize::from(VK_ESCAPE.0)
            && msg.lParam.0 & (1 << 30) == 0
            && !FILE_DIALOG_OPEN.load(Ordering::Relaxed)
            && let Some(notify) = ON_ESCAPE.get()
        {
            notify();
        }
    }
    // The message is passed on unchanged, so a text box still gets its own Esc handling.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

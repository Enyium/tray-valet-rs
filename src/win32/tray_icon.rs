// Note: This module was transferred to the `windows-helpers` crate and improved there. When refactoring, that crate should be used.

use std::{mem::size_of, time::Instant};
use windows::{
    core::HSTRING,
    Win32::{
        Foundation::{E_FAIL, HWND, LPARAM, WPARAM},
        UI::{
            Input::KeyboardAndMouse::GetDoubleClickTime,
            Shell::{
                Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD,
                NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NINF_KEY, NIN_SELECT, NOTIFYICONDATAW,
                NOTIFYICON_VERSION_4, NOTIFY_ICON_DATA_FLAGS,
            },
            WindowsAndMessaging::{DestroyIcon, HICON, WM_CONTEXTMENU},
        },
    },
};

const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

/// A tray icon to be used with a window. To prevent a low-quality icon, The app needs to be declared in its manifest as DPI-aware in the same way that the operating system is.
pub struct TrayIcon {
    notify_icon_data: NOTIFYICONDATAW,
    last_activation_time: Instant,
}

impl TrayIcon {
    pub fn with_primary_id(
        event_hwnd: HWND,
        window_msg_id: u32,
    ) -> Result<Self, windows::core::Error> {
        //! Creates a tray icon with ID 0. If you need more than one tray icon, don't use this function repeatedly.

        Self::with_id(0, event_hwnd, window_msg_id)
    }

    pub fn with_id(
        id: u32,
        event_hwnd: HWND,
        window_msg_id: u32,
    ) -> Result<Self, windows::core::Error> {
        let mut notify_icon_data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as _,
            hWnd: event_hwnd,
            uID: id,
            uFlags: NOTIFY_ICON_DATA_FLAGS(0),
            ..Default::default()
        };

        notify_icon_data.uFlags |= NIF_MESSAGE;
        notify_icon_data.uCallbackMessage = window_msg_id;

        notify_icon_data.uFlags |= NIF_ICON;
        notify_icon_data.hIcon = HICON(0); // Transparent until revised.

        notify_icon_data.uFlags |= NIF_TIP | NIF_SHOWTIP;
        // (Empty tooltip through default zero-initialization.)

        notify_icon_data.Anonymous.uVersion = NOTIFYICON_VERSION_4;

        for action in [NIM_ADD, NIM_SETVERSION] {
            if unsafe { !Shell_NotifyIconW(action, &notify_icon_data).as_bool() } {
                unsafe { Shell_NotifyIconW(NIM_DELETE, &notify_icon_data) };

                // `Shell_NotifyIconW()` isn't documented to provide an error code via `GetLastError()`.
                return Err(E_FAIL.into());
            }
        }

        Ok(Self {
            notify_icon_data,
            last_activation_time: Instant::now(),
        })
    }

    pub fn set_tooltip<T>(&mut self, tooltip: T) -> Result<(), windows::core::Error>
    where
        T: Into<HSTRING>,
    {
        let tooltip: HSTRING = tooltip.into();
        let len = tooltip.len().min(self.notify_icon_data.szTip.len() - 1);

        self.notify_icon_data.szTip[..len].copy_from_slice(&tooltip.as_wide()[..len]);
        self.notify_icon_data.szTip[len] = 0;

        if unsafe { Shell_NotifyIconW(NIM_MODIFY, &self.notify_icon_data).as_bool() } {
            Ok(())
        } else {
            Err(E_FAIL.into())
        }
    }

    pub fn set_icon(&mut self, hicon: HICON) -> Result<(), windows::core::Error> {
        let _ = unsafe { DestroyIcon(self.notify_icon_data.hIcon) };
        self.notify_icon_data.hIcon = hicon;

        if unsafe { Shell_NotifyIconW(NIM_MODIFY, &self.notify_icon_data).as_bool() } {
            Ok(())
        } else {
            Err(E_FAIL.into())
        }
    }

    pub fn translate_window_msg(
        &mut self,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<TrayIconEvent> {
        let msg_id = lparam.0 & 0xffff;
        match msg_id as _ {
            NIN_SELECT | NIN_KEYSELECT => {
                // NIN_SELECT - After every up-event of the primary mouse button.
                // NIN_KEYSELECT - Once on Space, twice on Enter (when not holding the key).
                //
                // Since Space and Enter key presses can't be distinguished, and an Enter key press sends two undistinguishable events, the logic of reacting only once on double-click is also applied to the keyboard events.

                if self.last_activation_time.elapsed().as_millis()
                    > unsafe { GetDoubleClickTime() } as _
                {
                    self.last_activation_time = Instant::now();
                    Some(TrayIconEvent::Activated)
                } else {
                    None
                }
            }
            // Context menu request via mouse or keyboard.
            WM_CONTEXTMENU => {
                let wparam_loword = (wparam.0 & 0xffff) as i16;
                let wparam_hiword = (wparam.0 >> 16 & 0xffff) as i16;
                Some(TrayIconEvent::ContextMenuRequested {
                    x: wparam_loword,
                    y: wparam_hiword,
                })
            }
            _ => None,
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &self.notify_icon_data);
            let _ = DestroyIcon(self.notify_icon_data.hIcon);
        }
    }
}

pub enum TrayIconEvent {
    /// Tray icon was clicked or double-clicked, or Space or Enter was pressed on a keyboard-focused icon.
    Activated,
    /// With x-and-y virtual-screen coordinates.
    ContextMenuRequested { x: i16, y: i16 },
}

use std::{borrow::Cow, marker::PhantomData};

use anyhow::Result;
use num_traits::{FromPrimitive, ToPrimitive};
use windows::{
    core::{HSTRING, PCWSTR},
    Win32::{
        Foundation::{E_FAIL, HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::{
            CreatePopupMenu, DestroyMenu, GetSystemMetrics, InsertMenuW, PostMessageW,
            SetForegroundWindow, SetMenuDefaultItem, TrackPopupMenuEx, HMENU, MF_BYPOSITION,
            MF_STRING, SM_MENUDROPALIGNMENT, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTALIGN,
            TPM_RIGHTBUTTON, WM_NULL,
        },
    },
};

pub struct ContextMenu<T>
where
    T: FromPrimitive + ToPrimitive,
{
    hmenu: HMENU,
    event_hwnd: HWND,
    _phantom_data: PhantomData<T>,
}

impl<T> ContextMenu<T>
where
    T: FromPrimitive + ToPrimitive,
{
    pub fn new(
        items: Vec<(T, Cow<str>)>,
        default_item: T,
        event_hwnd: HWND,
    ) -> Result<Self, windows::core::Error> {
        let hmenu = unsafe { CreatePopupMenu()? };

        let mut result = Ok(());
        for (enum_variant, text) in items {
            let id = match enum_variant.to_u32() {
                Some(id) => id,
                None => {
                    result = Err(E_FAIL.into());
                    break;
                }
            };

            if let Err(error) = unsafe {
                InsertMenuW(
                    hmenu,
                    u32::MAX,
                    MF_BYPOSITION | MF_STRING,
                    id as _,
                    PCWSTR(HSTRING::from(&*text).as_ptr()),
                )
            } {
                result = Err(error);
                break;
            }
        }

        if let Ok(()) = result {
            if let Some(id) = default_item.to_u32() {
                result = unsafe { SetMenuDefaultItem(hmenu, id, false.into()) };
            }
        }

        if let Err(error) = result {
            let _ = unsafe { DestroyMenu(hmenu) };
            return Err(error);
        }

        Ok(Self {
            hmenu,
            event_hwnd,
            _phantom_data: PhantomData,
        })
    }

    pub fn show(&mut self, x: i32, y: i32) {
        //! Shows the context menu at the specified virtual-screen coordinates and blocks the call site until the menu is hidden. The event window will receive a `WM_COMMAND` message with the result.

        unsafe {
            SetForegroundWindow(self.event_hwnd); // Doesn't seem to matter whether it's invisible.

            //TODO: See <https://github.com/microsoft/win32metadata/issues/1783>.
            let _ = TrackPopupMenuEx(
                self.hmenu,
                (if GetSystemMetrics(SM_MENUDROPALIGNMENT) != 0 {
                    TPM_RIGHTALIGN
                } else {
                    TPM_LEFTALIGN
                } | TPM_BOTTOMALIGN
                    | TPM_RIGHTBUTTON)
                    .0,
                x,
                y,
                self.event_hwnd,
                None,
            );

            let _ = PostMessageW(self.event_hwnd, WM_NULL, WPARAM(0), LPARAM(0));

            // (For reasons for `SetForegroundWindow()` and `PostMessageW()`, see: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-trackpopupmenu#remarks.)
        }
    }
}

impl<T> Drop for ContextMenu<T>
where
    T: FromPrimitive + ToPrimitive,
{
    fn drop(&mut self) {
        let _ = unsafe { DestroyMenu(self.hmenu) };
    }
}

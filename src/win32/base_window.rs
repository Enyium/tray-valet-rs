// Note: This module was transferred to the `windows-helpers` crate and improved there. When refactoring, that crate should be used.

use std::marker::PhantomPinned;
use std::pin::Pin;
use windows::{
    core::{HSTRING, PCWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, Performance::QueryPerformanceCounter},
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW, RegisterClassW,
            SetWindowLongPtrW, UnregisterClassW, CREATESTRUCTW, GWLP_USERDATA, HMENU,
            WINDOW_EX_STYLE, WINDOW_STYLE, WM_NCCREATE, WNDCLASSW,
        },
    },
};

/// Structs using this type may never implement `Unpin`!
pub struct BaseWindow<'a, T>
where
    T: 'a + OnWindowMsg,
{
    class_atom: u16,
    hwnd: HWND,
    msg_callback_with_this_arg: Option<(
        Box<dyn 'a + Fn(Pin<&'a mut T>, HWND, u32, WPARAM, LPARAM) -> Option<LRESULT>>,
        Pin<&'a mut T>,
    )>,
    /// Prevent unpinning of structs using this struct as a field as long as they don't explicitly implement `Unpin`.
    _phantom_pinned: PhantomPinned,
}

impl<'a, T> BaseWindow<'a, T>
where
    T: 'a + OnWindowMsg,
{
    pub fn new() -> Result<Pin<Box<Self>>, windows::core::Error> {
        let hmodule = unsafe { GetModuleHandleW(PCWSTR::null())? };

        let mut precise_time_value = 0;
        let _ = unsafe { QueryPerformanceCounter(&mut precise_time_value) };

        let class_atom = unsafe {
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(Self::window_procedure),
                hInstance: hmodule.into(),
                lpszClassName: PCWSTR(
                    HSTRING::from(format!("Win32WindowByRust_{precise_time_value:x}")).as_ptr(),
                ),
                ..Default::default()
            })
        };
        if class_atom == 0 {
            return Err(windows::core::Error::from_win32());
        }

        let instance = Self {
            class_atom,
            hwnd: HWND(0),                    // Set in window procedure.
            msg_callback_with_this_arg: None, // Set by setter.
            _phantom_pinned: PhantomPinned,
        };
        let boxed_instance_ptr = Box::into_raw(Box::new(instance));

        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class_atom as _),
                PCWSTR(HSTRING::new().as_ptr()),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                HWND(0),
                HMENU(0),
                hmodule,
                Some(boxed_instance_ptr as _),
            )
        };
        if hwnd.0 == 0 {
            drop(unsafe { Box::from_raw(boxed_instance_ptr) });
            return Err(windows::core::Error::from_win32());
        }

        Ok(unsafe { Pin::new_unchecked(Box::from_raw(boxed_instance_ptr)) })
    }

    pub fn set_msg_callback_with_this_arg<F>(
        this_ptr: *mut Pin<Box<Self>>,
        msg_callback: F,
        msg_callback_this_arg: Box<T>,
    ) -> Pin<Box<T>>
    where
        F: 'a + Fn(Pin<&mut T>, HWND, u32, WPARAM, LPARAM) -> Option<LRESULT>,
    {
        let this = unsafe { (&mut *this_ptr).as_mut().get_unchecked_mut() };

        let msg_callback = Box::new(msg_callback);

        let boxed_msg_callback_this_arg_ptr = Box::into_raw(msg_callback_this_arg);
        let msg_callback_this_arg =
            unsafe { Pin::new_unchecked(&mut *boxed_msg_callback_this_arg_ptr) };

        this.msg_callback_with_this_arg = Some((msg_callback, msg_callback_this_arg));

        unsafe { Pin::new_unchecked(Box::from_raw(boxed_msg_callback_this_arg_ptr)) }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    extern "system" fn window_procedure(
        hwnd: HWND,
        msg_id: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // Initialize when receiving what should be the first message ever.
        if msg_id == WM_NCCREATE {
            // Retrieve Rust struct.
            let create_struct = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
            let this = unsafe { &mut *(create_struct.lpCreateParams as *mut Self) };

            // Complete it.
            this.hwnd = hwnd;

            // Make it available to subsequent calls.
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, create_struct.lpCreateParams as _) };

            // "Many of the messages sent during window creation are kind of important to pass through to Def­Window­Proc. For example, if you neglect to pass WM_NC­CREATE to Def­Window­Proc, your window will not be properly initialized." (https://devblogs.microsoft.com/oldnewthing/20191014-00/?p=102992)
            return unsafe { DefWindowProcW(hwnd, msg_id, wparam, lparam) };
        }

        // Invoke callback.
        let this_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Self };
        if !this_ptr.is_null() {
            if let Some((callback, this_arg)) = unsafe { &mut *this_ptr }
                .msg_callback_with_this_arg
                .as_mut()
            {
                if let Some(lresult) = callback(this_arg.as_mut(), hwnd, msg_id, wparam, lparam) {
                    return lresult;
                }
            }
        }

        // Invoke default message handler.
        unsafe { DefWindowProcW(hwnd, msg_id, wparam, lparam) }
    }
}

impl<T> Drop for BaseWindow<'_, T>
where
    T: OnWindowMsg,
{
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.hwnd) };

        if let Ok(hmodule) = unsafe { GetModuleHandleW(PCWSTR::null()) } {
            let _ = unsafe { UnregisterClassW(PCWSTR(self.class_atom as _), hmodule) };
        }
    }
}

pub trait OnWindowMsg {
    fn on_window_msg(
        this: Pin<&mut Self>,
        hwnd: HWND,
        msg_id: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT>;
}

pub fn translate_command_msg(wparam: WPARAM, lparam: LPARAM) -> CommandMsg {
    let wparam_hiword = (wparam.0 >> 16 & 0xffff) as u16;
    let wparam_loword = (wparam.0 & 0xffff) as u16;

    match wparam_hiword {
        0 => CommandMsg::MenuItem { id: wparam_loword },
        1 => CommandMsg::Accelerator { id: wparam_loword },
        _ => CommandMsg::ControlMsg {
            msg_id: wparam_hiword,
            control_id: wparam_loword,
            control_hwnd: HWND(lparam.0),
        },
    }
}

pub enum CommandMsg {
    MenuItem {
        id: u16,
    },
    Accelerator {
        id: u16,
    },
    ControlMsg {
        msg_id: u16,
        control_id: u16,
        control_hwnd: HWND,
    },
}

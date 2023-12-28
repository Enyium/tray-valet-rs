// Note: This module was transferred to the `windows-helpers` crate and improved there. When refactoring, that crate should be used.

use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_QUIT},
};

/// A Win32 message loop runner.
pub struct Win32MsgLoop;

impl Win32MsgLoop {
    pub fn run() -> Result<usize, windows::core::Error> {
        //! Runs the message loop and sends window messages to the corresponding window procedures. If successful, returns the exit code received via `WM_QUIT` from `PostQuitMessage()` that the process should return. If unsuccessful and you can handle the error, the function can be rerun in a loop.

        loop {
            let msg = Self::run_till_thread_msg()?;
            if msg.message == WM_QUIT {
                break Ok(msg.wParam.0);
            }
        }
    }

    pub fn run_till_thread_msg() -> Result<MSG, windows::core::Error> {
        //! Runs the message loop until a thread message is received, sending window messages to the corresponding window procedures in between. In most programs, the only thread message will be `WM_QUIT` (sent via `PostQuitMessage()`); but others are possible via `PostThreadMessageW()` and `PostMessageW()`.

        let mut msg = MSG::default();
        loop {
            match unsafe { GetMessageW(&mut msg, HWND(0), 0, 0).0 } {
                -1 => break Err(windows::core::Error::from_win32()),

                // Received `WM_QUIT` thread message. Caller must check `msg.message` against `WM_QUIT`.
                // (`GetMessageW()` return value is checked instead of treating `WM_QUIT` like all thread messages, in case abusive behavior caused `msg.hwnd` to be non-zero, which is possible via `PostMessageW()`.)
                0 => break Ok(msg),

                _ => {
                    // Propagate window message to window procedure.
                    // (The docs say something about `WM_TIMER`. In case `msg.hwnd` can be zero when having received a `WM_TIMER` message, these functions are also called for thread messages. Custom thread messages will be ignored.)
                    unsafe {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }

                    // Return thread message.
                    if msg.hwnd.0 == 0 {
                        break Ok(msg);
                    }
                }
            }
        }
    }
}

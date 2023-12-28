use anyhow::Result;
use std::{
    ffi::{OsStr, OsString},
    io,
    mem::size_of,
    os::windows::prelude::OsStringExt,
    path::PathBuf,
    process::Command,
    time::Instant,
};
use windows::{
    core::PWSTR,
    Win32::{
        Foundation::{
            CloseHandle, SetLastError, BOOL, ERROR_INSUFFICIENT_BUFFER,
            ERROR_INVALID_WINDOW_HANDLE, E_FAIL, HWND, LPARAM, MAX_PATH, S_OK, WIN32_ERROR, WPARAM,
        },
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
        UI::WindowsAndMessaging::{
            DestroyIcon, EnumWindows, GetClassNameW, GetWindowPlacement, GetWindowTextLengthW,
            GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, KillTimer, PostMessageW,
            SetForegroundWindow, SetTimer, ShowWindow, CHILDID_SELF, EVENT_OBJECT_CREATE,
            EVENT_OBJECT_DESTROY, EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_SHOW,
            EVENT_SYSTEM_MINIMIZESTART, HICON, ICON_BIG, ICON_SMALL, OBJID_WINDOW, SW_HIDE,
            SW_RESTORE, SW_SHOW, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, WINDOWPLACEMENT, WM_CLOSE,
            WM_SETICON, WPF_RESTORETOMAXIMIZED,
        },
    },
};

use crate::{
    background_window::TimerId,
    win32::win_event_hook::{ProcessThreadSet, WinEvent, WinEventHook},
};

const TIMEOUT_MILLIS: u128 = 2000;

pub struct ForeignProcessTree {
    known_process_ids: Vec<u32>,

    event_hwnd: HWND,

    win_event_hook: WinEventHook,
    win_event_window_msg_id: u32,

    time_waited: Instant,
    error_window_msg_id: u32,

    window_class: String,
    hwnd: Option<HWND>,
    hook_process_thread_id: Option<(u32, u32)>,
    window_exe_path: Option<PathBuf>,
    small_hicon: Option<HICON>,
    large_hicon: Option<HICON>,
}

impl ForeignProcessTree {
    pub unsafe fn new<I, S>(
        args: I,
        window_class: &str,
        event_hwnd: HWND,
        win_event_window_msg_id: u32,
        error_window_msg_id: u32,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        //! The `WM_TIMER` window message must be handled by calling through to the appropriate method.
        //!
        //! # Safety
        //! The win event hook window message must be handled appropriately by the window procedure by retrieving the `Box` from the raw pointer.

        let mut args_iter = args.into_iter();
        let program = args_iter
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, ""))?;
        let process = Command::new(program).args(args_iter).spawn()?;
        let process_id = process.id();

        let mut win_event_hook = unsafe {
            WinEventHook::new(ProcessThreadSet::All, event_hwnd, win_event_window_msg_id)
        };
        win_event_hook
            .add_filtered_event(EVENT_OBJECT_CREATE, ProcessThreadSet::Process(process_id))?;
        win_event_hook
            .add_filtered_event(EVENT_OBJECT_SHOW, ProcessThreadSet::Process(process_id))?;

        let mut instance = Self {
            known_process_ids: vec![process_id],

            event_hwnd,

            win_event_hook: win_event_hook,
            win_event_window_msg_id,

            time_waited: Instant::now(),
            error_window_msg_id,

            window_class: window_class.to_string(),
            hwnd: None,
            hook_process_thread_id: None,
            window_exe_path: None,
            small_hicon: None,
            large_hicon: None,
        };

        if let Some(foreign_hwnd) = instance.find_window_in_process(process_id) {
            instance.hwnd = Some(foreign_hwnd);
            instance.init_hwnd_monitoring()?;
        } else {
            let _ = unsafe {
                SetTimer(
                    event_hwnd,
                    TimerId::ForeignProcessTreeCheckForNewProcesses as _,
                    100, /*ms*/
                    None,
                )
            };
        }

        Ok(instance)
    }

    pub fn handle_timer_window_msg(&mut self, wparam: WPARAM, _lparam: LPARAM) -> bool {
        //! Returns `true`, if the message was handled.

        let timer_id = wparam.0;
        if timer_id != TimerId::ForeignProcessTreeCheckForNewProcesses as _ {
            return false;
        }

        let mut has_error = false;
        let mut must_stop_timer = false;

        if let Ok(h_snapshot) = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
            let mut process_entry = PROCESSENTRY32W::default();
            process_entry.dwSize = size_of::<PROCESSENTRY32W>() as _;
            let mut next_process_result =
                unsafe { Process32FirstW(h_snapshot, &mut process_entry) };

            while let Ok(()) = next_process_result {
                if self
                    .known_process_ids
                    .contains(&process_entry.th32ParentProcessID)
                    && !self
                        .known_process_ids
                        .contains(&process_entry.th32ProcessID)
                {
                    self.known_process_ids.push(process_entry.th32ProcessID);

                    let _ = self.win_event_hook.add_filtered_event(
                        EVENT_OBJECT_CREATE,
                        ProcessThreadSet::Process(process_entry.th32ProcessID),
                    );
                    let _ = self.win_event_hook.add_filtered_event(
                        EVENT_OBJECT_SHOW,
                        ProcessThreadSet::Process(process_entry.th32ProcessID),
                    );

                    if let Some(foreign_hwnd) =
                        self.find_window_in_process(process_entry.th32ProcessID)
                    {
                        self.hwnd = Some(foreign_hwnd);

                        if let Err(_) = self.init_hwnd_monitoring() {
                            has_error = true;
                        }

                        must_stop_timer = true;
                        break;
                    }
                }

                next_process_result = unsafe { Process32NextW(h_snapshot, &mut process_entry) };
            }

            // (Since there isn't a guarantee about the order of the returned processes, grandchild processes of known processes could be returned before child processes. But the grandchild processes would be noticed in a later snapshot.)

            let _ = unsafe { CloseHandle(h_snapshot) };
        }

        if self.hwnd == None && self.time_waited.elapsed().as_millis() > TIMEOUT_MILLIS {
            has_error = true;
            must_stop_timer = true;
        }

        if has_error {
            let _ = unsafe {
                PostMessageW(
                    self.event_hwnd,
                    self.error_window_msg_id,
                    WPARAM(0),
                    LPARAM(0),
                )
            };
        }

        if must_stop_timer {
            let _ = unsafe {
                KillTimer(
                    self.event_hwnd,
                    TimerId::ForeignProcessTreeCheckForNewProcesses as _,
                )
            };
        }

        true
    }

    fn find_window_in_process(&self, process_id: u32) -> Option<HWND> {
        let mut hwnd = None;
        let mut exchange_tuple = (self, process_id, &mut hwnd);
        let _ = unsafe {
            EnumWindows(
                Some(Self::enum_windows_callback),
                LPARAM(&mut exchange_tuple as *mut _ as _),
            )
        };

        hwnd
    }

    extern "system" fn enum_windows_callback(top_level_hwnd: HWND, lparam: LPARAM) -> BOOL {
        let (this, required_process_id, out_hwnd) =
            unsafe { &mut *(lparam.0 as *mut (&Self, u32, &mut Option<HWND>)) };

        let mut process_id = 0;
        unsafe { GetWindowThreadProcessId(top_level_hwnd, Some(&mut process_id)) };

        if process_id == *required_process_id
            && unsafe { IsWindowVisible(top_level_hwnd).as_bool() }
            && this.verify_window_class(top_level_hwnd)
        {
            **out_hwnd = Some(top_level_hwnd);

            // Stop enumeration.
            false.into()
        } else {
            // Continue.
            true.into()
        }
    }

    fn verify_window_class(&self, hwnd: HWND) -> bool {
        let mut buffer = vec![0; 256];
        let len = unsafe { GetClassNameW(hwnd, &mut buffer) } as usize;
        if len != 0 {
            let class_name = String::from_utf16_lossy(&buffer[..len]);
            class_name == self.window_class
        } else {
            false
        }
    }

    pub fn translate_win_event(
        &mut self,
        _wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<ForeignWindowEvent> {
        let win_event = unsafe { *Box::from_raw(lparam.0 as *mut WinEvent) };

        match self.hwnd {
            // When `conhost.exe` is run with the parameter `powershell.exe`, `GetWindowThreadProcessId()` reports `conhost.exe` as the owning process on `EVENT_OBJECT_CREATE`. But starting with `EVENT_OBJECT_SHOW` at the latest, `powershell.exe` is reported as the owning process (which is also the information you see in spy tools). However, when using the process and thread ID from `GetWindowThreadProcessId()` on `EVENT_OBJECT_SHOW` for `SetWinEventHook()`, `GetLastError()` after `SetWinEventHook()` reports `ERROR_INVALID_THREAD_ID`. `EVENT_OBJECT_SHOW` is even sent with command `conhost powershell -WindowStyle Hidden`, because the window briefly appears. (`conhost.exe` may possibly use `ConsoleControl()` to change the window owner.)
            None if win_event.event_id == EVENT_OBJECT_CREATE
                && win_event.object_id == OBJID_WINDOW.0
                && win_event.child_id == CHILDID_SELF as _ =>
            {
                if self.verify_window_class(win_event.hwnd) {
                    let mut process_id = 0;
                    let thread_id =
                        unsafe { GetWindowThreadProcessId(win_event.hwnd, Some(&mut process_id)) };
                    if thread_id != 0 {
                        self.hwnd = Some(win_event.hwnd);
                        self.hook_process_thread_id = Some((process_id, thread_id));
                    }
                }

                Some(ForeignWindowEvent::Internal)
            }
            Some(hwnd) if hwnd == win_event.hwnd => {
                match win_event.event_id {
                    EVENT_OBJECT_SHOW
                        if win_event.object_id == OBJID_WINDOW.0
                            && win_event.child_id == CHILDID_SELF as _ =>
                    {
                        let return_value = match self.init_hwnd_monitoring() {
                            Ok(()) => Some(ForeignWindowEvent::Found),
                            Err(_) => {
                                let _ = unsafe {
                                    PostMessageW(
                                        self.event_hwnd,
                                        self.error_window_msg_id,
                                        WPARAM(0),
                                        LPARAM(0),
                                    )
                                };

                                Some(ForeignWindowEvent::Internal)
                            }
                        };

                        let _ = unsafe {
                            KillTimer(
                                self.event_hwnd,
                                TimerId::ForeignProcessTreeCheckForNewProcesses as _,
                            )
                        };

                        return_value
                    }
                    // Start of time of being minimized, not start of minimization animation.
                    EVENT_SYSTEM_MINIMIZESTART => Some(ForeignWindowEvent::Minimized),
                    EVENT_OBJECT_NAMECHANGE
                        if win_event.object_id == OBJID_WINDOW.0
                            && win_event.child_id == CHILDID_SELF as _ =>
                    {
                        Some(ForeignWindowEvent::TitleChanged)
                    }
                    EVENT_OBJECT_DESTROY
                        if win_event.object_id == OBJID_WINDOW.0
                            && win_event.child_id == CHILDID_SELF as _ =>
                    {
                        Some(ForeignWindowEvent::Destroyed)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn init_hwnd_monitoring(&mut self) -> Result<(), windows::core::Error> {
        let (foreign_hwnd, (hook_process_id, hook_thread_id)) =
            if let (Some(hwnd), Some(hook_process_thread_id)) =
                (self.hwnd, self.hook_process_thread_id)
            {
                (hwnd, hook_process_thread_id)
            } else {
                return Err(E_FAIL.into());
            };

        // Set up win event hook.
        self.win_event_hook = unsafe {
            WinEventHook::new(
                ProcessThreadSet::ProcessAndThread(hook_process_id, hook_thread_id),
                self.event_hwnd,
                self.win_event_window_msg_id,
            )
        };
        self.win_event_hook.add_event(EVENT_SYSTEM_MINIMIZESTART)?;
        self.win_event_hook.add_event(EVENT_OBJECT_NAMECHANGE)?;
        self.win_event_hook.add_event(EVENT_OBJECT_DESTROY)?;

        // Find .exe path.
        let mut window_process_id = 0;
        unsafe { GetWindowThreadProcessId(foreign_hwnd, Some(&mut window_process_id)) };

        let h_process =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, true, window_process_id)? };

        let mut buffer = vec![0; MAX_PATH as _];
        let mut result = Ok(());
        let mut buffer_len_then_string_len: u32 = 0;
        for _ in 0..8 {
            buffer_len_then_string_len = buffer.len() as _;
            result = unsafe {
                QueryFullProcessImageNameW(
                    h_process,
                    PROCESS_NAME_FORMAT(0),
                    PWSTR(buffer.as_mut_ptr()),
                    &mut buffer_len_then_string_len,
                )
            };

            match &result {
                Ok(()) => break,
                Err(error) if error.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => {
                    buffer.reserve(buffer.len() * 2 - buffer.len());
                }
                Err(_) => break,
            }
        }

        let _ = unsafe { CloseHandle(h_process) };

        if let Err(error) = result {
            return Err(error);
        }

        self.window_exe_path =
            Some(OsString::from_wide(&buffer[..buffer_len_then_string_len as usize]).into());

        Ok(())
    }

    pub fn set_icon(&mut self, small_hicon: HICON, large_hicon: HICON) {
        if let Some(hwnd) = self.hwnd {
            for (size, hicon) in [(ICON_SMALL, small_hicon), (ICON_BIG, large_hicon)] {
                let _ =
                    unsafe { PostMessageW(hwnd, WM_SETICON, WPARAM(size as _), LPARAM(hicon.0)) };
            }
        }
    }

    pub fn window_visible(&self) -> bool {
        if let Some(hwnd) = self.hwnd {
            unsafe { IsWindowVisible(hwnd).as_bool() }
        } else {
            false
        }
    }

    pub fn set_window_visible(&mut self, new_visible: bool) {
        let currently_visible = self.window_visible();
        if new_visible == currently_visible {
            return;
        }

        let hwnd = if let Some(hwnd) = self.hwnd {
            hwnd
        } else {
            return;
        };

        let show_cmd = if currently_visible {
            SW_HIDE
        } else {
            let mut window_placement = WINDOWPLACEMENT::default();
            window_placement.length = size_of::<WINDOWPLACEMENT>() as _;
            let _ = unsafe { GetWindowPlacement(hwnd, &mut window_placement) };

            let is_minimized = window_placement.showCmd == SW_SHOWMINIMIZED.0 as _;
            if is_minimized {
                let was_maximized = (window_placement.flags & WPF_RESTORETOMAXIMIZED).0 != 0;
                if was_maximized {
                    SW_SHOWMAXIMIZED
                } else {
                    //TODO: SOMETIME: Report Windows 10 bug: After `SW_RESTORE` and `SW_SHOWNORMAL`, a previously invisible - but not minimized - arranged window is clearly not arranged anymore, but visually in a *restored* state, while `IsWindowArranged()` erroneously continues to return `TRUE` (at least after hiding and showing a few times). Only after moving the window just a tiny bit, `IsWindowArranged()` returns `FALSE`. The documentation of both of the flags as well as the remarks on `IsWindowArranged()` also object to the experienced behavior. (This code avoids the bug by using `SW_SHOW` instead of `SW_RESTORE` for unminimized windows.)
                    SW_RESTORE
                }
            } else {
                // As opposed to `SW_RESTORE`, prevents a not minimized window in arranged state from becoming not arranged anymore. (This branch also runs when `GetWindowPlacement()` fails.)
                SW_SHOW
            }
        };

        unsafe {
            ShowWindow(hwnd, show_cmd);
            SetForegroundWindow(hwnd);
        }
    }

    pub fn toggle_window_visible(&mut self) {
        let visible = self.window_visible();
        self.set_window_visible(!visible);
    }

    pub fn window_exe_path(&self) -> Option<PathBuf> {
        self.window_exe_path.clone()
    }

    pub fn window_title(&self) -> Result<String, windows::core::Error> {
        let hwnd = if let Some(hwnd) = self.hwnd {
            hwnd
        } else {
            return Err(ERROR_INVALID_WINDOW_HANDLE.into());
        };

        unsafe { SetLastError(WIN32_ERROR(0)) };
        let len = unsafe { GetWindowTextLengthW(hwnd) } as usize;
        if len == 0 {
            let error = windows::core::Error::from_win32();
            return if error.code() == S_OK {
                Ok("".to_string())
            } else {
                Err(error)
            };
        }

        let mut buffer = vec![0; len + 1];
        let copied_len = unsafe { GetWindowTextW(hwnd, &mut buffer) } as usize;
        if copied_len == len {
            Ok(String::from_utf16_lossy(&buffer[..len]))
        } else {
            Err(windows::core::Error::from_win32())
        }
    }

    pub fn close_window(&mut self) {
        if let Some(hwnd) = self.hwnd {
            let _ = unsafe { PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
    }
}

impl Drop for ForeignProcessTree {
    fn drop(&mut self) {
        self.set_window_visible(true);

        for hicon in [self.small_hicon, self.large_hicon] {
            if let Some(hicon) = hicon {
                let _ = unsafe { DestroyIcon(hicon) };
            }
        }
    }
}

pub enum ForeignWindowEvent {
    Found,
    Minimized,
    TitleChanged,
    Destroyed,
    Internal,
}

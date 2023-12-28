#![allow(dead_code)]

use nohash_hasher::IntMap;
use std::{cell::RefCell, marker::PhantomData};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::Threading::GetCurrentProcessId,
    UI::{
        Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
        WindowsAndMessaging::{
            SendMessageW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WINEVENT_SKIPOWNTHREAD,
        },
    },
};

thread_local! {
    static HOOK_DATA: RefCell<IntMap<isize, (HWND, u32)>> = RefCell::new(IntMap::default());
}

/// An out-of-context win event hook (using the flag `WINEVENT_OUTOFCONTEXT`). See the [Windows API documentation on `SetWinEventHook()`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook). Unhooked on drop.
pub struct WinEventHook {
    process_thread_set: ProcessThreadSet,
    h_win_event_hooks: Vec<HWINEVENTHOOK>,

    event_hwnd: HWND,
    window_msg_id: u32,

    _phantom_unsend: PhantomUnsend,
    _phantom_unsync: PhantomUnsync,
}

impl WinEventHook {
    pub unsafe fn new(
        process_thread_set: ProcessThreadSet,
        event_hwnd: HWND,
        window_msg_id: u32,
    ) -> Self {
        //! Prepares hooks for the specified set of processes and threads. Actual registering of hooks happens when specifying event ranges via the methods. When the current thread runs a Win32 event loop, the window procedure of the specified window will be called with the message ID.
        //!
        //! Just pass `All` for the set, if you want to specify multiple filters via the method.
        //!
        //! # Safety
        //! Every event leaks a `Box<WinEvent>`. Therefore, you *must* handle the window procedure event with the window handle and the window message and call `Box::from_raw()` on the `lparam` parameter, so that the `Box` will be dropped.

        Self {
            process_thread_set,
            h_win_event_hooks: Vec::new(),

            event_hwnd,
            window_msg_id,

            _phantom_unsend: PhantomData,
            _phantom_unsync: PhantomData,
        }
    }

    pub fn add_event(&mut self, event_id: u32) -> Result<(), windows::core::Error> {
        self.add_event_range(event_id, event_id)
    }

    pub fn add_filtered_event(
        &mut self,
        event_id: u32,
        process_thread_set: ProcessThreadSet,
    ) -> Result<(), windows::core::Error> {
        self.add_filtered_event_range(event_id, event_id, process_thread_set)
    }

    pub fn add_event_range(
        &mut self,
        min_event_id: u32,
        max_event_id: u32,
    ) -> Result<(), windows::core::Error> {
        self.add_filtered_event_range(min_event_id, max_event_id, self.process_thread_set)
    }

    pub fn add_filtered_event_range(
        &mut self,
        min_event_id: u32,
        max_event_id: u32,
        process_thread_set: ProcessThreadSet,
    ) -> Result<(), windows::core::Error> {
        //! Every call to this or one of the similar methods calls the `SetWinEventHook()` Windows API function to actually register a hook for the specified event range.

        let mut process_id = 0;
        let mut thread_id = 0;
        let mut flags = WINEVENT_OUTOFCONTEXT;

        match process_thread_set {
            ProcessThreadSet::All => {}
            ProcessThreadSet::AllProcessesExclCurrent => {
                flags |= WINEVENT_SKIPOWNPROCESS;
            }
            ProcessThreadSet::AllThreadsExclCurrent => {
                flags |= WINEVENT_SKIPOWNTHREAD;
            }
            ProcessThreadSet::CurrentProcessExclCurrentThread => {
                process_id = unsafe { GetCurrentProcessId() };
                flags |= WINEVENT_SKIPOWNTHREAD;
            }
            ProcessThreadSet::Process(id) => {
                process_id = id;
            }
            ProcessThreadSet::ProcessAndThread(id_1, id_2) => {
                process_id = id_1;
                thread_id = id_2;
            }
        }

        let h_win_event_hook = unsafe {
            SetWinEventHook(
                min_event_id,
                max_event_id,
                None,
                Some(Self::win_event_procedure),
                process_id,
                thread_id,
                flags,
            )
        };
        if h_win_event_hook.0 == 0 {
            // `SetWinEventHook()` isn't documented to set the last error, but practically it can be experienced (as of Nov. 2023).
            return Err(windows::core::Error::from_win32());
        }

        self.h_win_event_hooks.push(h_win_event_hook);
        HOOK_DATA.with_borrow_mut(|hook_data| {
            hook_data.insert(h_win_event_hook.0, (self.event_hwnd, self.window_msg_id));
        });

        Ok(())
    }

    extern "system" fn win_event_procedure(
        h_win_event_hook: HWINEVENTHOOK,
        event_id: u32,
        hwnd: HWND,
        object_id: i32,
        child_id: i32,
        thread_id: u32,
        time_millis: u32,
    ) {
        let hook_data =
            HOOK_DATA.with_borrow_mut(|data| data.get(&h_win_event_hook.0).map(Clone::clone));
        let (event_hwnd, window_msg_id) = if let Some(data) = hook_data {
            data
        } else {
            return;
        };

        let boxed_win_event_ptr = Box::into_raw(Box::new(WinEvent {
            event_id,
            hwnd,
            object_id,
            child_id,
            thread_id,
            time_millis,
        }));

        // Synchronously call window procedure.
        unsafe {
            SendMessageW(
                event_hwnd,
                window_msg_id,
                WPARAM(0),
                LPARAM(boxed_win_event_ptr as _),
            )
        };
    }
}

impl Drop for WinEventHook {
    fn drop(&mut self) {
        HOOK_DATA.with_borrow_mut(|hook_data| {
            for h_win_event_hook in self.h_win_event_hooks.iter() {
                unsafe { UnhookWinEvent(*h_win_event_hook) };
                hook_data.remove(&h_win_event_hook.0);
            }
        });
    }
}

/// An abstract and/or concrete set of processes and threads.
#[derive(Clone, Copy)]
pub enum ProcessThreadSet {
    All,
    AllProcessesExclCurrent,
    /// The threads of all other processes as well as the threads of the current process excluding the current thread.
    AllThreadsExclCurrent,
    CurrentProcessExclCurrentThread,
    /// All threads in the process with the given ID.
    Process(u32),
    /// In the process with the first ID, only the thread with the second ID. A thread ID is unique system-wide while the thread lives; the process ID must still be specified, however, because the thread ID may be recyled for a thread of another process.
    ProcessAndThread(u32, u32),
}

pub struct WinEvent {
    pub event_id: u32,
    pub hwnd: HWND,
    pub object_id: i32,
    pub child_id: i32,
    pub thread_id: u32,
    pub time_millis: u32,
}

type PhantomUnsend = PhantomData<std::sync::MutexGuard<'static, ()>>;
type PhantomUnsync = PhantomData<std::cell::Cell<()>>;

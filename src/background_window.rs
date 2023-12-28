use anyhow::Result;
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::FromPrimitive;
use std::{borrow::Cow, pin::Pin, ptr};
use windows::{
    core::{h, HSTRING},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::WindowsAndMessaging::{
            DestroyIcon, DestroyWindow, PostQuitMessage, HICON, WM_APP, WM_COMMAND, WM_DESTROY,
            WM_TIMER,
        },
    },
};

use crate::{
    cli::Cli,
    foreign_process_tree::{ForeignProcessTree, ForeignWindowEvent},
    win32::{
        base_window::{self, BaseWindow, CommandMsg, OnWindowMsg},
        context_menu::ContextMenu,
        icon::{duplicate_hicon, load_tray_monitor_icon},
        tray_icon::{TrayIcon, TrayIconEvent},
    },
    APP_NAME,
};

pub struct BackgroundWindow<'a> {
    base_window: Pin<Box<BaseWindow<'a, BackgroundWindow<'a>>>>,
    tray_icon: TrayIcon,
    context_menu: ContextMenu<ContextMenuItem>,
    foreign_process_tree: ForeignProcessTree,
    hide_after_start: bool,
    small_hicon: Option<HICON>,
    large_hicon: Option<HICON>,
    foreign_window_needs_icon: bool,
}

impl<'a> BackgroundWindow<'a> {
    pub fn new(cli: Cli) -> Result<Pin<Box<Self>>> {
        // Create objects.
        let base_window = BaseWindow::new()?;
        let mut tray_icon =
            TrayIcon::with_primary_id(base_window.hwnd(), CustomWindowMsg::TrayIcon as _)?;

        let context_menu = ContextMenu::new(
            vec![
                (
                    ContextMenuItem::ToggleForeignWindowVisible,
                    Cow::Borrowed("&Show/Hide"),
                ),
                (
                    ContextMenuItem::ReleaseForeignWindowAndExit,
                    Cow::Borrowed("&Release"),
                ),
                (
                    ContextMenuItem::CloseForeignWindowAndExit,
                    Cow::Borrowed("&Close"),
                ),
            ],
            ContextMenuItem::ToggleForeignWindowVisible,
            base_window.hwnd(),
        )?;

        let foreign_process_tree = unsafe {
            ForeignProcessTree::new(
                cli.foreign_process_tree_args,
                &cli.win_class,
                base_window.hwnd(),
                CustomWindowMsg::WinEventHook as _,
                CustomWindowMsg::WaitingForForeignWindowError as _,
            )?
        };

        // Early configuration.
        let (small_hicon, large_hicon) = if let Some(icon_path) = cli.icon.as_ref() {
            let small_hicon = load_tray_monitor_icon(icon_path, false).ok();
            let large_hicon = load_tray_monitor_icon(icon_path, true).ok();

            if let Some(small_hicon) = small_hicon {
                let second_small_icon = duplicate_hicon(small_hicon);
                if let Ok(hicon) = second_small_icon {
                    let _ = tray_icon.set_icon(hicon);
                }
            }

            (small_hicon, large_hicon)
        } else {
            (None, None)
        };

        // Create `Self` instance.
        let mut instance = Box::new(Self {
            base_window,
            tray_icon,
            context_menu,
            foreign_process_tree,
            hide_after_start: !cli.dont_hide,
            small_hicon,
            large_hicon,
            foreign_window_needs_icon: cli.set_win_icon,
        });

        // Configure base window.
        Ok(BaseWindow::set_msg_callback_with_this_arg(
            ptr::addr_of_mut!(instance.base_window),
            Self::on_window_msg,
            instance,
        ))
    }

    fn destroy(&mut self) {
        let _ = unsafe { DestroyWindow(self.base_window.hwnd()) };
    }
}

impl Drop for BackgroundWindow<'_> {
    fn drop(&mut self) {
        for hicon in [self.small_hicon, self.large_hicon] {
            if let Some(hicon) = hicon {
                let _ = unsafe { DestroyIcon(hicon) };
            }
        }
    }
}

impl<'a> OnWindowMsg for BackgroundWindow<'a> {
    fn on_window_msg(
        mut this: Pin<&mut Self>,
        _hwnd: HWND,
        msg_id: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        match msg_id {
            WM_TIMER => this
                .foreign_process_tree
                .handle_timer_window_msg(wparam, lparam)
                .then_some(LRESULT(0)),
            id if id == CustomWindowMsg::WinEventHook as _ => this
                .foreign_process_tree
                .translate_win_event(wparam, lparam)
                .map(|event| {
                    match event {
                        ForeignWindowEvent::Found => {
                            // Configure tray icon.
                            let must_load_icon =
                                this.small_hicon.is_none() && this.large_hicon.is_none();

                            let exe_path = if must_load_icon {
                                let exe_path = this.foreign_process_tree.window_exe_path();
                                if let Some(path) = exe_path.as_ref() {
                                    this.small_hicon = load_tray_monitor_icon(path, false).ok();
                                    if let Some(hicon) = this.small_hicon {
                                        let _ = this.tray_icon.set_icon(hicon);
                                    }
                                }

                                exe_path
                            } else {
                                None
                            };

                            let window_title = this
                                .foreign_process_tree
                                .window_title()
                                .unwrap_or_else(|_| "".to_string());
                            let _ = this.tray_icon.set_tooltip(window_title);

                            // Set window's icon.
                            if this.foreign_window_needs_icon {
                                if let (true, Some(exe_path)) = (must_load_icon, exe_path) {
                                    this.large_hicon = load_tray_monitor_icon(exe_path, true).ok();
                                }

                                if let (Some(small_hicon), Some(large_hicon)) =
                                    (this.small_hicon, this.large_hicon)
                                {
                                    this.foreign_process_tree.set_icon(small_hicon, large_hicon);
                                }
                            }

                            // Hide window.
                            if this.hide_after_start {
                                this.foreign_process_tree.set_window_visible(false);
                            }
                        }
                        ForeignWindowEvent::Minimized => {
                            this.foreign_process_tree.set_window_visible(false)
                        }
                        ForeignWindowEvent::TitleChanged => {
                            let foreign_window_title = this
                                .foreign_process_tree
                                .window_title()
                                .unwrap_or_else(|_| "".to_string());
                            let _ = this.tray_icon.set_tooltip(foreign_window_title);
                        }
                        ForeignWindowEvent::Destroyed => this.destroy(),
                        ForeignWindowEvent::Internal => {}
                    }

                    LRESULT(0)
                }),
            id if id == CustomWindowMsg::WaitingForForeignWindowError as _ => {
                win_msgbox::error::<win_msgbox::Okay>(
                    h!("Couldn't find the window with the specified class.").as_ptr(),
                )
                .title(HSTRING::from(APP_NAME).as_ptr())
                .show()
                .expect("improbable");

                this.destroy();

                Some(LRESULT(0))
            }
            id if id == CustomWindowMsg::TrayIcon as _ => this
                .tray_icon
                .translate_window_msg(wparam, lparam)
                .map(|event| {
                    match event {
                        TrayIconEvent::Activated => {
                            this.foreign_process_tree.toggle_window_visible();
                        }
                        TrayIconEvent::ContextMenuRequested { x, y } => {
                            this.context_menu.show(x as _, y as _)
                        }
                    }

                    LRESULT(0)
                }),
            WM_COMMAND => match base_window::translate_command_msg(wparam, lparam) {
                CommandMsg::MenuItem { id } => ContextMenuItem::from_u16(id).map(|item| {
                    match item {
                        ContextMenuItem::ToggleForeignWindowVisible => {
                            this.foreign_process_tree.toggle_window_visible();
                        }
                        ContextMenuItem::ReleaseForeignWindowAndExit => {
                            this.destroy();
                        }
                        ContextMenuItem::CloseForeignWindowAndExit => {
                            this.foreign_process_tree.close_window();
                            // (This should cause this app to exit also.)
                        }
                    }

                    LRESULT(0)
                }),
                _ => None,
            },
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}

#[repr(u32)]
pub enum CustomWindowMsg {
    TrayIcon = WM_APP + 0,
    WinEventHook = WM_APP + 1,
    /// An error or timeout happened while waiting for the foreign window.
    WaitingForForeignWindowError = WM_APP + 3,
}

#[repr(usize)]
pub enum TimerId {
    ForeignProcessTreeCheckForNewProcesses = 100, // Strangely, 0 and 1 are sent via `WM_TIMER` without calling `SetTimer()`.
}

#[derive(FromPrimitive, ToPrimitive)]
enum ContextMenuItem {
    ToggleForeignWindowVisible,
    ReleaseForeignWindowAndExit,
    CloseForeignWindowAndExit,
}

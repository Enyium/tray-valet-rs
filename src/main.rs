// No console window in release build. (An alternative would be to call `FreeConsole()` in release builds, in which case a console window is briefly shown, however.)
#![cfg_attr(all(not(debug_assertions), not(test)), windows_subsystem = "windows")]

mod background_window;
mod cli;
mod foreign_process_tree;
mod win32;

use anyhow::anyhow;
use clap::Parser;
use cli::Cli;
use std::process;
use windows::core::HSTRING;

use background_window::BackgroundWindow;
use win32::msg_loop::Win32MsgLoop;

static APP_NAME: &str = "Tray Valet";

fn main() {
    let exit_result = 'block: {
        let cli = {
            let parse_result = Cli::try_parse()
                .map_err(|error| {
                    let has_info_error = matches!(
                        error.kind(),
                        clap::error::ErrorKind::DisplayHelp
                            | clap::error::ErrorKind::DisplayVersion
                    );

                    (anyhow!(error), has_info_error)
                })
                .and_then(|cli| {
                    if cli.foreign_process_tree_args.len() < 1 {
                        Err((
                            anyhow!(
                                "Missing command or command arguments after separating ` -- `."
                            ),
                            false,
                        ))
                    } else {
                        Ok(cli)
                    }
                });

            match parse_result {
                Ok(cli) => cli,
                Err(data) => break 'block Err(data),
            }
        };

        let _background_window = match BackgroundWindow::new(cli) {
            Ok(window) => window,
            Err(error) => break 'block Err((error, false)),
        };

        Win32MsgLoop::run().map_err(|error| (anyhow!(error), false))
    };

    process::exit(match exit_result {
        // May still be an error.
        Ok(exit_code) => exit_code as _,
        Err((error, has_info_error)) => {
            win_msgbox::MessageBox::<win_msgbox::Okay>::new(
                HSTRING::from(error.to_string()).as_ptr(),
            )
            .icon(if has_info_error {
                win_msgbox::Icon::Information
            } else {
                win_msgbox::Icon::Error
            })
            .title(HSTRING::from(APP_NAME).as_ptr())
            .show()
            .expect("improbable");

            if has_info_error {
                0
            } else {
                1
            }
        }
    });
}

use std::{io, mem::size_of, path::Path};
use windows::{
    core::{h, HSTRING, PCWSTR},
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, E_FAIL, HANDLE},
        Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTOPRIMARY},
        Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES,
        UI::{
            HiDpi::{GetDpiForMonitor, GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI},
            Shell::{
                SHDefExtractIconW, SHGetFileInfoW, SHGetStockIconInfo, SHFILEINFOW, SHGFI_ICON,
                SHGFI_LARGEICON, SHGFI_SMALLICON, SHGSI_ICON, SHGSI_LARGEICON, SHGSI_SMALLICON,
                SHSTOCKICONINFO, SIID_DOCNOASSOC,
            },
            WindowsAndMessaging::{
                CopyImage, FindWindowW, HICON, IMAGE_FLAGS, IMAGE_ICON, SM_CXICON, SM_CXSMICON,
                SM_CYICON, SM_CYSMICON,
            },
        },
    },
};

pub fn load_tray_monitor_icon<T>(file_path: T, large: bool) -> Result<HICON, windows::core::Error>
where
    T: AsRef<Path>,
{
    //! Returned `HICON` must be destroyed with `DestroyIcon()`.
    //!
    //! Paths longer than `MAX_PATH` don't work. More on the problem: https://www.zabkat.com/blog/max-path-programmers-cookbook.htm.

    let file_path = match dunce::canonicalize(file_path) {
        Ok(path) => HSTRING::from(&*path),
        Err(io_error) => {
            return Err(match io_error.kind() {
                io::ErrorKind::NotFound => ERROR_FILE_NOT_FOUND.to_hresult(),
                _ => E_FAIL,
            }
            .into());
        }
    };

    // Get icon size - specifically for monitor with main taskbar that displays the tray.
    let dpi = get_tray_monitor_dpi();

    let small_icon_width =
        unsafe { GetSystemMetricsForDpi(if large { SM_CXICON } else { SM_CXSMICON }, dpi) };
    if small_icon_width == 0 {
        return Err(windows::core::Error::from_win32());
    }

    let small_icon_height =
        unsafe { GetSystemMetricsForDpi(if large { SM_CYICON } else { SM_CYSMICON }, dpi) };
    if small_icon_height == 0 {
        return Err(windows::core::Error::from_win32());
    }

    let small_icon_size = (small_icon_width + small_icon_height) / 2;

    // Obtain icon from file, with best size for monitor.
    let mut hicon = HICON(0);

    let _ = unsafe {
        SHDefExtractIconW(
            PCWSTR(file_path.as_ptr()),
            /*icon index*/ 0,
            0,
            Some(&mut hicon),
            None,
            small_icon_size as _,
        )
    };

    //TODO: See <https://github.com/microsoft/win32metadata/issues/1754> ("Functions missing CanReturnMultipleSuccessValuesAttribute or needing other treatment").
    // //. `Result` is currently useless. This is a workaround.
    let def_extract_icon_error = if hicon.is_invalid() {
        windows::core::Error::from_win32()
    } else {
        return Ok(hicon);
    };

    // ...or from a function that returns a file-type-based fallback icon when there are no icons in the file.
    let mut file_info = SHFILEINFOW::default();

    if unsafe {
        SHGetFileInfoW(
            PCWSTR(file_path.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut file_info),
            size_of::<SHFILEINFOW>() as _,
            SHGFI_ICON
                | if large {
                    SHGFI_LARGEICON
                } else {
                    SHGFI_SMALLICON
                },
        )
    } != 0
    {
        return Ok(file_info.hIcon);
    }

    // ...or a fallback stock icon.
    let mut stock_icon_info = SHSTOCKICONINFO::default();
    stock_icon_info.cbSize = size_of::<SHSTOCKICONINFO>() as _;

    match unsafe {
        SHGetStockIconInfo(
            SIID_DOCNOASSOC,
            SHGSI_ICON
                | if large {
                    SHGSI_LARGEICON
                } else {
                    SHGSI_SMALLICON
                },
            &mut stock_icon_info,
        )
    } {
        Ok(()) => Ok(stock_icon_info.hIcon),
        Err(_) => Err(def_extract_icon_error),
    }
}

fn get_tray_monitor_dpi() -> u32 {
    let hwnd = unsafe {
        FindWindowW(
            // Other taskbars have class `Shell_SecondaryTrayWnd`.
            PCWSTR(h!("Shell_TrayWnd").as_ptr()),
            PCWSTR(0 as _),
        )
    };
    let hmonitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY) }; // `HWND(0)` should yield primary.

    let mut dpi_x = 0;
    let mut dpi_y = 0;
    match unsafe { GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) } {
        Ok(()) => (dpi_x + dpi_y) / 2,
        Err(_) => 96,
    }
}

pub fn duplicate_hicon(hicon: HICON) -> Result<HICON, windows::core::Error> {
    unsafe { CopyImage(HANDLE(hicon.0), IMAGE_ICON, 0, 0, IMAGE_FLAGS(0)) }
        .map(|handle| HICON(handle.0))
}

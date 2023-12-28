# Tray Valet

This Windows program starts another program and hides a window from its process tree in the taskbar tray. It was designed to run console programs in the background by calling something like `conhost.exe powershell.exe -File "C:\path\to\script.ps1"`. Calling `conhost.exe` directly prevents Windows Terminal from capturing the console process in a tab besides others, i.e., it ensures the script has a dedicated window.

The window is identified by its window class, which can be found with spy tools like [WinSpy++](https://www.catch22.net/projects/winspy/) or [System Informer](https://systeminformer.sourceforge.io/).

Example call:

```
"C:\path\to\tray-valet.exe" --win-class ConsoleWindowClass --set-win-icon -- conhost powershell -File "C:\path\to\long-running-script.ps1"
```

(Since the Windows UI only allows you to specify 259 characters for the command line, you may have to use relative paths and set the working directory for the shortcut.)

For a quick test without a script, omit the arguments after `powershell`.

Run `tray-valet.exe --help` to see a help message box.

# Code Quality

As I noticed during development, the app would actually have needed more sophisticated abstractions for the Windows API. Now, there's a bit of spaghetti code. My [`windows-helpers`](https://crates.io/crates/windows-helpers) crate could help with this, and it could also be extended with code from this repository. But, for the time being, the code works well enough and I currently don't plan to refactor the code base.

# License

Licensed under either of

* Apache License, Version 2.0
  ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license
  ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

# Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

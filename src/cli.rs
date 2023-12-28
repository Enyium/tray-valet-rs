use clap::Parser;

#[derive(Parser)]
#[command(version)]
pub struct Cli {
    /// The foreign top-level window's class name that'll be searched for in the foreign process tree. Can be found out with spy tools.
    #[arg(long, required = true)]
    pub win_class: String,

    /// A path to the file with the icon that should be used instead of the icon from the executable file that's associated with the foreign window.
    #[arg(long)]
    pub icon: Option<String>,

    /// When there's a discrepancy between the tray and the window icon, this switch can be used to apply the tray icon to the window.
    #[arg(long)]
    pub set_win_icon: bool,

    /// Whether the foreign window should not automatically be hidden at start.
    #[arg(long)]
    pub dont_hide: bool,

    /// The command and arguments to start the foreign process tree. Should always be used after a separating ` -- ` (surrounded by spaces). Not allowed to be empty.
    pub foreign_process_tree_args: Vec<String>,
}

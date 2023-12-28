use anyhow::Result;

fn main() -> Result<()> {
    // Compile Win32 resource file with app manifest.
    embed_resource::compile("src/main.rc", embed_resource::NONE);

    // Embed information from `Cargo.toml` like the version.
    winres::WindowsResource::new()
        .compile()
        .expect("`winres` resource should've compiled");

    //     // Copy assets.
    //     let assets_dir = Path::new("assets");
    //
    //     let mut target_dir = PathBuf::new();
    //     target_dir.push("target");
    //     target_dir.push(env::var("PROFILE")?);
    //
    //     for entry in fs::read_dir(assets_dir)? {
    //         let entry = entry?;
    //         let dest_path = target_dir.join(entry.file_name());
    //         if entry.file_type()?.is_file() {
    //             fs::copy(entry.path(), dest_path)?;
    //         }
    //     }

    Ok(())
}

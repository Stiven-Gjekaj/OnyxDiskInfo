fn main() {
    slint_build::compile("ui/main.slint").expect("the .slint files compile");
    windows_resources();
}

/// Puts the manifest into the .exe file, so that Windows asks for
/// administrator rights when the app starts. The build script runs on the
/// computer that builds, so this happens when Windows builds for Windows.
#[cfg(windows)]
fn windows_resources() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_manifest_file("windows.manifest")
        .set("ProductName", "OnyxDiskInfo")
        .set("FileDescription", "OnyxDiskInfo");
    resource.compile().expect("the Windows resources compile");
}

#[cfg(not(windows))]
fn windows_resources() {}

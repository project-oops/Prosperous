//! Embeds the project icon in the Windows executable, as `pros-cli/build.rs` does.
//!
//! Explorer and the taskbar read the executable's resource table; the title bar reads the
//! runtime icon `main.rs` sets from `assets/logo.png`. Both are needed.
//!
//! `assets/logo.ico` holds sizes 16 through 256 so Windows picks one instead of filtering a
//! large one down, which blurs the pixel-art mark at 16.

fn main() {
    // Without this a changed logo is not re-embedded.
    println!("cargo:rerun-if-changed=../assets/logo.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../assets/logo.ico");
        // Failing the build is deliberate: a binary missing its icon looks correct otherwise.
        resource
            .compile()
            .expect("could not embed assets/logo.ico in the executable");
    }
}

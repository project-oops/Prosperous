//! Put the project's own icon on the Windows executable.
//!
//! The same job `pros-cli/build.rs` does, for the window rather than the console. A GUI
//! binary needs it more than a CLI one: this is the executable somebody pins to a taskbar,
//! so the icon Explorer shows is the icon the program is known by.
//!
//! This is separate from the icon the *window* wears at runtime, which `main.rs` sets from
//! `assets/logo.png` through eframe. Windows reads the two from different places - the
//! executable's resource table for Explorer and the taskbar, the running process for the
//! title bar - so both are needed and neither substitutes for the other.
//!
//! `assets/logo.ico` holds six sizes (16 through 256). Windows picks the one it wants rather
//! than rescaling a single large image, which matters for pixel art: the mark is a blocky
//! drawing, and a filtered downscale of the 256 turns it to mush at 16.

fn main() {
    // The asset is the only input, so a change to it has to trigger a relink. Without this the
    // icon is embedded once and a later change to the logo silently ships the old one.
    println!("cargo:rerun-if-changed=../assets/logo.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../assets/logo.ico");
        // Failing the build is right. A silent fallback would produce a binary that looks correct
        // and is missing the thing this file exists to add, and nobody checks an icon on purpose.
        resource
            .compile()
            .expect("could not embed assets/logo.ico in the executable");
    }
}

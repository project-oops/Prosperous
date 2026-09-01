//! Put the project's own icon on the Windows executable.
//!
//! One logo, in every place it can appear. Windows is the only one of the three targets where an
//! executable can carry one: a Linux ELF and a macOS command-line binary have nowhere to put it -
//! theirs live in packaging metadata, a `.desktop` entry or an `.app` bundle, and a bare CLI has
//! neither. So there is nothing to do off Windows, and the dependency is gated in `Cargo.toml`
//! rather than here, so it is not fetched or built on those platforms at all.
//!
//! `assets/logo.ico` holds six sizes (16 through 256). Windows picks the one it wants rather than
//! rescaling a single large image, which matters for pixel art: the mark is a blocky drawing, and
//! a filtered downscale of the 256 turns it to mush at 16.

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

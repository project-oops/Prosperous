//! Embeds the project icon in the Windows executable.
//!
//! Only a Windows executable carries an icon; Linux and macOS keep theirs in packaging
//! metadata, which a bare CLI has none of. The dependency is target-gated in `Cargo.toml`.
//!
//! `assets/logo.ico` holds sizes 16 through 256 so Windows picks one instead of downscaling
//! the pixel-art mark with a filter.

fn main() {
    // A change to the icon must trigger a relink, or the old one stays embedded.
    println!("cargo:rerun-if-changed=../assets/logo.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../assets/logo.ico");
        // Fails the build: a silent fallback ships a binary without its icon, unnoticed.
        resource
            .compile()
            .expect("could not embed assets/logo.ico in the executable");
    }
}

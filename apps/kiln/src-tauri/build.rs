fn main() {
    // The libvips crate emits `-lvips` but not where to find it, which fails on
    // any machine where vips is not on the default library path — Homebrew on
    // Apple silicon, for one. pkg-config knows, so ask it.
    for library in ["vips", "glib-2.0", "gobject-2.0"] {
        if let Err(error) = pkg_config::Config::new().probe(library) {
            println!("cargo:warning=pkg-config could not find {library}: {error}");
        }
    }

    tauri_build::build()
}

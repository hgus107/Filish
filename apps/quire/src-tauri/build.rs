fn main() {
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rerun-if-changed=src/native_pdf.m");
        cc::Build::new()
            .file("src/native_pdf.m")
            .flag("-fobjc-arc")
            .flag("-Wno-deprecated-declarations")
            .compile("quire_native_pdf");
        println!("cargo:rustc-link-lib=framework=PDFKit");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
        println!("cargo:rustc-link-lib=framework=ImageIO");
    }
    tauri_build::build()
}

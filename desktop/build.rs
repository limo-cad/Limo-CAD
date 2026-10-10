#[path = "../crates/occt/sdk.rs"]
mod occt_sdk;

fn main() {
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=windows.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        for name in [
            "OCCT_ROOT",
            "LIMO_CAD_OCCT_LIB_DIR",
            "VCPKG_INSTALLED_DIR",
            "VCPKG_TARGET_TRIPLET",
        ] {
            println!("cargo:rerun-if-env-changed={name}");
        }
        println!("cargo:rerun-if-changed=../crates/occt/sdk.rs");
        let arch = std::env::var("CARGO_CFG_TARGET_ARCH").expect("Cargo target architecture");
        let roots = occt_sdk::roots(
            "macos",
            &arch,
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."),
            std::env::var_os("OCCT_ROOT").map(Into::into),
            std::env::var_os("VCPKG_INSTALLED_DIR").map(Into::into),
            std::env::var("VCPKG_TARGET_TRIPLET").ok(),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let sdk = occt_sdk::resolve(
            &roots,
            "macos",
            &arch,
            std::env::var_os("LIMO_CAD_OCCT_LIB_DIR")
                .as_deref()
                .map(std::path::Path::new),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        println!(
            "cargo:rerun-if-changed={}",
            sdk.include.join("Standard_Version.hxx").display()
        );
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
        if std::env::var_os("LIMO_CAD_OCCT_LIB_DIR").is_none() {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", sdk.lib.display());
        }
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("icons/icon.ico")
            .set("ProductName", "Limo CAD")
            .set("FileDescription", "Limo CAD")
            .set("OriginalFilename", "Limo-CAD.exe")
            .set_manifest_file("windows.manifest")
            .compile()
            .expect("compile native Windows icon, version and DPI manifest");
    }
}

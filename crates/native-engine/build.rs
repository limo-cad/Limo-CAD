#[cfg(feature = "native-occt")]
#[path = "../occt/sdk.rs"]
mod occt_sdk;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "native-occt")]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        for name in [
            "OCCT_ROOT",
            "LIMO_CAD_OCCT_LIB_DIR",
            "VCPKG_INSTALLED_DIR",
            "VCPKG_TARGET_TRIPLET",
        ] {
            println!("cargo:rerun-if-env-changed={name}");
        }
        println!("cargo:rerun-if-changed=../occt/sdk.rs");
        let arch = std::env::var("CARGO_CFG_TARGET_ARCH").expect("Cargo target architecture");
        let roots = occt_sdk::roots(
            "macos",
            &arch,
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
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
        // Link arguments from the OCCT dependency do not reach this package's
        // integration-test executables. Give only those tests the checked SDK
        // runtime path; desktop packaging keeps its own relocatable rpaths.
        println!(
            "cargo:rustc-link-arg-tests=-Wl,-rpath,{}",
            sdk.lib.display()
        );
    }
}

#[cfg(feature = "native-occt")]
#[path = "sdk.rs"]
mod sdk;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=sdk.rs");
    #[cfg(feature = "native-occt")]
    native();
}

#[cfg(feature = "native-occt")]
fn native() {
    use std::{env, path::PathBuf};
    for path in [
        "src/native.rs",
        "src/shim.cpp",
        "include/shim.hpp",
        "include/refinement_budget.hpp",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    for name in [
        "OCCT_ROOT",
        "LIMO_CAD_OCCT_LIB_DIR",
        "LIMO_CAD_OCCT_INPUTS_SHA256",
        "VCPKG_INSTALLED_DIR",
        "VCPKG_TARGET_TRIPLET",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("Cargo target OS");
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").expect("Cargo target architecture");
    let roots = sdk::roots(
        &target_os,
        &target_arch,
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        env::var_os("OCCT_ROOT").map(PathBuf::from),
        env::var_os("VCPKG_INSTALLED_DIR").map(PathBuf::from),
        env::var("VCPKG_TARGET_TRIPLET").ok(),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let sdk = sdk::resolve(
        &roots,
        &target_os,
        &target_arch,
        env::var_os("LIMO_CAD_OCCT_LIB_DIR")
            .as_deref()
            .map(std::path::Path::new),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    for header in ["Standard_Version.hxx", "math_DoubleTab.lxx"] {
        println!(
            "cargo:rerun-if-changed={}",
            sdk.include.join(header).display()
        );
    }
    let mut bridge = cxx_build::bridge("src/native.rs");
    bridge
        .file("src/shim.cpp")
        .include("include")
        .include(&sdk.include)
        .std("c++17")
        .warnings(true);
    if target_os == "windows" {
        bridge
            .define("NOMINMAX", None)
            .define("WIN32_LEAN_AND_MEAN", None)
            .flag_if_supported("/EHsc");
    }
    bridge.compile("limo_cad_occt_bridge");
    println!("cargo:rustc-link-search=native={}", sdk.lib.display());
    for library in sdk::LIBRARIES {
        println!("cargo:rustc-link-lib=dylib={library}");
    }
    if target_os == "macos" {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", sdk.lib.display());
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    } else if target_os == "linux" {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", sdk.lib.display());
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/limo-cad");
    }
}

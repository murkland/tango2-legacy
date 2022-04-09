extern crate bindgen;

use std::env;
use std::path::PathBuf;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS");

    println!("cargo:rustc-link-search=external/mgba/build");
    println!("cargo:rustc-link-lib=mgba");
    match target_os.as_ref().map(|x| &**x) {
        Ok("macos") => println!("cargo:rustc-link-lib=framework=Cocoa"),
        tos => panic!("unknown target os {:?}!", tos),
    }
    println!("cargo:rerun-if-changed=mgba_wrapper.h");
    let bindings = bindgen::Builder::default()
        .header("mgba_wrapper.h")
        .clang_args(&["-Iexternal/mgba/include"])
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .generate()
        .expect("Unable to generate bindings");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("mgba_bindings.rs"))
        .expect("Couldn't write bindings!");

    tonic_build::configure()
        .build_server(false)
        .compile(&["external/signor/pb/api.proto"], &["external/signor/pb"])
        .unwrap();
}

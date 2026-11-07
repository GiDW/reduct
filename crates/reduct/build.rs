//! Compiles resources/reduct.rc (icon, application manifest, version info and
//! the settings dialog) into Reduct.exe.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    for file in ["reduct.rc", "resource.h", "reduct.exe.manifest"] {
        println!("cargo::rerun-if-changed=resources/{file}");
    }
    println!("cargo::rerun-if-changed=../../icon/ico/reduct.ico");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    fs::write(out_dir.join("version.h"), version_header(&version)).expect("write version.h");

    // embed-resource passes OUT_DIR to rc.exe as an include path (for
    // version.h). The resources folder is added so the icon and manifest
    // paths in reduct.rc resolve relative to the .rc file, whatever rc.exe's
    // working directory is.
    let resources = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("resources");
    embed_resource::compile("resources/reduct.rc", embed_resource::ParamsIncludeDirs([resources.as_os_str()]))
        .manifest_required()
        .unwrap();
}

/// `x.y.z` becomes `x,y,z,0` (binary) and `"x.y.z.0"` (string); the fourth
/// number is the Store's, as in the package version.
fn version_header(version: &str) -> String {
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let parts: Vec<u16> = core
        .split('.')
        .map(|p| p.parse().unwrap_or_else(|_| panic!("version part {p:?} in {version:?} is not a number")))
        .collect();
    let &[major, minor, patch] = &parts[..] else {
        panic!("version {version:?} is not x.y.z");
    };
    format!(
        "#define REDUCT_VERSION_BIN {major},{minor},{patch},0\n#define REDUCT_VERSION_STR \"{major}.{minor}.{patch}.0\"\n"
    )
}

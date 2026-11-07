//! Silences LNK4104 ("export of symbol 'DllGetClassObject' should be
//! PRIVATE"). The warning is about the import library, which nobody links
//! against: COM finds the exports with GetProcAddress. rustc writes its own
//! .def file, so the PRIVATE keyword can't be added there.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo::rustc-cdylib-link-arg=/IGNORE:4104");
    }
}

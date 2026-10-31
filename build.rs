//! Gives `fallow.exe` its icon on Windows (MSVC), for its notifications and Explorer: the linker
//! takes the prebuilt resource file `assets/icon/fallow.res` (made by `assets/icon/make_res.py`)
//! as an input, so no resource compiler is needed.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/fallow.res");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows && msvc {
        let res = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icon/fallow.res");
        println!("cargo:rustc-link-arg-bin=fallow={}", res.display());
    }
}

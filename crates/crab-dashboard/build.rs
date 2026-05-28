fn main() {
    println!("cargo:rustc-env=DASHBOARD_PKG_VERSION={}", env!("CARGO_PKG_VERSION"));
}

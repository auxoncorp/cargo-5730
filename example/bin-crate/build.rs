#[cfg(not(RealBuild))]
fn main() {
    cargo_5730::run_build_script();
}

#[cfg(RealBuild)]
fn main() {
    println!("Build script says: the sum is {}", lib_crate::add(1, 2));
}

fn main() {
    println!("cargo:rerun-if-changed=src/tests/"); // rebuild if a test changes
    println!("cargo:rerun-if-changed=build.rs"); // rebuild if build.rs changes
}
fn main() {
    // Cargo otherwise reuses a linked ARM image after memory.x is edited.
    println!("cargo:rerun-if-changed=memory.x");
}

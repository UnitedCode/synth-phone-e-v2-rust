extern crate bindgen;

use std::{env, fs};
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    build_daisysp();
    generate_bindings();
}

fn build_daisysp() {
    let current_dir = env::current_dir().expect("Failed to get current directory");

    // Define paths
    let daisysp_dir = current_dir.join("daisysp");
    let lib_dir = current_dir.join("lib");

    let lib_name = if cfg!(target_os = "windows") {
        "daisysp.lib"
    } else {
        "libdaisysp.a"
    };

    // Run make command in the DaisySP directory
    run_make(&daisysp_dir);

    // Copy the built library to the lib directory
    copy_lib(&daisysp_dir, &lib_dir);

    println!("DaisySP library built and copied successfully.");
}

fn run_make(daisysp_dir: &Path) {
    let status = Command::new("make")
        .current_dir(daisysp_dir)
        .status()
        .expect("Failed to run make");

    if !status.success() {
        panic!("make command failed with status: {}", status);
    }
}

fn copy_lib(daisysp_dir: &Path, lib_dir: &Path) {
    let build_dir = daisysp_dir.join("build");  // Assuming the build artifacts are placed in a 'build' directory
    let lib_name = if cfg!(target_os = "windows") {
        "daisysp.lib"
    } else {
        "libdaisysp.a"
    };

    let built_lib_path = build_dir.join(lib_name);
    let target_lib_path = lib_dir.join(lib_name);

    if !lib_dir.exists() {
        fs::create_dir(&lib_dir).expect("Failed to create lib directory");
    }

    fs::copy(&built_lib_path, &target_lib_path)
        .expect("Failed to copy library file");
}

fn generate_bindings() {
    let current_dir = env::current_dir().expect("Failed to get current directory");

    // Path to the DaisySP headers
    let daisysp_include_path = current_dir.join("daisysp/Source");

    // Determine the target
    let target = env::var("TARGET").expect("TARGET environment variable not set");

    // Add include paths for Clang to find the C++ standard library headers
    let mut clang_args = if target.contains("linux") {
        vec![
            "-I/usr/include/c++/11",  // Adjust this path according to your GCC version
            "-I/usr/include/x86_64-linux-gnu/c++/11",  // Adjust this path according to your system
            "-I/usr/lib/llvm-10/include",
            "-I/usr/local/include",  // Additional includes for good measure
            "-I/usr/include"         // Fallback include path
        ]
    } else if target.contains("apple-darwin") {
        vec![
            "-I/usr/local/opt/llvm/include",  // Homebrew LLVM on macOS
            "-I/Library/Developer/CommandLineTools/usr/include/c++/v1",  // Standard C++ library
            "-I/usr/local/include",  // Additional includes for good measure
            "-I/usr/include"         // Fallback include path
        ]
    } else if target.contains("windows") {
        vec![
            "-IC:/Program Files/LLVM/include",  // Adjust this path according to your LLVM installation on Windows
        ]
    } else {
        vec![]
    };

    // Add the include path for DaisySP
    let daisysp_include_arg = format!("-I{}", daisysp_include_path.display());
    clang_args.push(&daisysp_include_arg);

    // Print the clang arguments for debugging
    println!("Clang arguments: {:?}", clang_args);

    // Generate bindings
    let bindings = bindgen::Builder::default()
        .header("bindings.h")
        .clang_args(&clang_args)
        .clang_arg("-xc++")  // Treat input files as C++
        .clang_arg("-std=c++11")  // Use C++11 standard
        .ctypes_prefix("cty")  // Make the generated code #![no_std] compatible
        .use_core();

    // Print the bindgen command for debugging
    println!("Running bindgen with the following settings:");
    println!("Header: bindings.h");
    for arg in &clang_args {
        println!("Clang argument: {}", arg);
    }

    // Generate the bindings and handle potential errors
    let bindings = bindings.generate().expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}

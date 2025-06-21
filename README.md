# How to Compile Your Rust Project

This guide details the steps to compile and run the Rust project on your system.

## 1. Prerequisites: Install Rust and Cargo

First, you'll need to install `rustup`, the Rust toolchain installer. `rustup` allows you to manage different Rust versions and associated tools.

Download `rustup` from the official Rust website: [https://rustup.rs/](https://rustup.rs/) and follow the on-screen instructions for your operating system.

Windows details [rustup-windows](https://rust-lang.github.io/rustup/installation/windows.html)


## 2. Install Necessary Toolchain

After installing `rustup`, open your terminal or command prompt and install the stable Rust toolchain. This command ensures you have the standard compiler and `cargo` (Rust's build system and package manager) available.

```bash
rustup toolchain install stable
```

## 3. Compile project

Compile

```bash
cargo build --release .
```

Now you just gotta run the executable located at **./target/release/md_ratatui.exe** or only md.ratatui

use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/ssh-tunnel.rc");
    println!("cargo:rerun-if-changed=assets/ssh-tunnel.ico");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let (mut compiler, resource) = match env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("gnu") => {
            let resource = out.join("ssh-tunnel-icon.o");
            let mut compiler = Command::new("windres");
            let format = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
                Ok("x86_64") => "pe-x86-64",
                Ok("x86") => "pe-i386",
                arch => panic!("Unsupported GNU Windows architecture: {arch:?}"),
            };
            compiler
                .args(["-F", format, "-O", "coff", "-i", "ssh-tunnel.rc", "-o"])
                .arg(&resource);
            (compiler, resource)
        }
        Ok("msvc") => {
            let resource = out.join("ssh-tunnel-icon.res");
            let mut compiler = Command::new("rc.exe");
            compiler
                .arg("/nologo")
                .arg("/fo")
                .arg(&resource)
                .arg("ssh-tunnel.rc");
            (compiler, resource)
        }
        target_env => panic!("Unsupported Windows toolchain: {target_env:?}"),
    };

    let status = compiler
        .current_dir("assets")
        .status()
        .unwrap_or_else(|error| panic!("Could not run {:?}: {error}", compiler.get_program()));
    assert!(
        status.success(),
        "Windows icon resource compilation failed: {status}"
    );
    println!("cargo:rustc-link-arg-bin=ssh-tunnel={}", resource.display());
}

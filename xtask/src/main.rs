use std::{
    env,
    io::Write as _,
    path::{Path, PathBuf},
    process,
};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    BuildUnpacker {
        #[arg(env = "WASI_SDK_PATH")]
        wasi_sdk: PathBuf,
    },
}

fn main() -> process::ExitCode {
    let Args {
        command: Commands::BuildUnpacker { wasi_sdk },
    } = Args::parse();

    let cargo = std::env::var_os("CARGO");
    let cargo = cargo.as_deref().unwrap_or("cargo".as_ref());
    let locate_project = process::Command::new(cargo)
        .args(["locate-project", "--workspace", "--message-format=plain"])
        .stderr(process::Stdio::inherit())
        .output()
        .unwrap();
    assert!(
        locate_project.status.success(),
        "Command `cargo locate-project` has failed: {:?}",
        locate_project.status
    );
    let workspace_manifest = String::from_utf8(locate_project.stdout).unwrap();
    let workspace_manifest = Path::new(workspace_manifest.trim());
    let workspace_root = workspace_manifest.parent().unwrap();

    let source_file = workspace_root.join("src/upkr_unpacker.c");
    let output_wasm = workspace_root.join("src/upkr_unpacker.wasm");
    let clang = wasi_sdk.join("bin/clang");
    let sysroot = wasi_sdk.join("share/wasi-sysroot");

    let cflags = [
        "-W",
        "-Wall",
        "-Wextra",
        // "-Werror",
        "-Wno-unused",
        "-Wconversion",
        "-Wsign-conversion",
        // "-MMD",
        "-MP",
        // "-mcpu=bleeding-edge",
        "-msign-ext",
        "-mbulk-memory",
        "-mmutable-globals",
        "-fno-exceptions",
        "-DNDEBUG",
        "-Oz",
        "-nostdlib",
        // "-flto",
        "-Wl,-zstack-size=14752,--no-entry",
        "-Wl,--import-memory",
        "-mexec-model=reactor",
        "-Wl,--initial-memory=65536,--max-memory=65536,--stack-first",
        // "-Wl,--lto-O3",
        "-Wl,--strip-debug,--gc-sections",
        "-Wl,--strip-all",
    ];

    let clang_status = process::Command::new(clang)
        .args(["--sysroot".as_ref(), sysroot.as_os_str()])
        .args(cflags)
        .arg(format!("-DCONTEXT_SIZE={}", common::CONTEXT_SIZE))
        .arg(source_file)
        .args(["-o".as_ref(), output_wasm.as_os_str()])
        .status()
        .unwrap();

    assert!(clang_status.success());

    // Stripping out unneeded stuff

    let mut module = walrus::Module::from_file(&output_wasm).unwrap();
    module.start = None;
    let unused_exports: Vec<_> = module
        .exports
        .iter()
        .filter(|export| export.name != "upkr_unpack")
        .map(|export| export.id())
        .collect();
    for unused_export in unused_exports {
        module.exports.delete(unused_export)
    }
    module.producers.clear();
    let custom_ids: Vec<_> = module.customs.iter().map(|(i, _s)| i).collect();
    for custom_id in custom_ids {
        module.customs.delete(custom_id);
    }
    walrus::passes::gc::run(&mut module);
    let module = module.emit_wasm();

    let wasm_opt = env::var_os("WASM_OPT");
    let wasm_opt = wasm_opt.as_deref().unwrap_or("wasm-opt".as_ref());
    let mut wasm_opt = process::Command::new(wasm_opt)
        .args(["-Oz", "--zero-filled-memory", "--strip-producers"])
        .arg("-")
        .args(["-o".as_ref(), output_wasm.as_os_str()])
        .stdin(process::Stdio::piped())
        .spawn()
        .unwrap();

    wasm_opt.stdin.take().unwrap().write_all(&module).unwrap();
    let status = wasm_opt.wait().unwrap();

    assert!(
        status.success(),
        "`wasm-opt` failed with status: {status:?}",
    );

    process::ExitCode::SUCCESS
}

use std::{
    env,
    io::Write as _,
    path::{Path, PathBuf},
    process,
};

const USAGE: &str = "\
USAGE: xtask build-unpacker [WASI_SDK_PATH]

`WASI_SDK_PATH` argument may also be passed as an environment variable
";

enum Args {
    BuildUnpacker { wasi_sdk: PathBuf },
}

impl Args {
    fn parse_args() -> Result<Self, pico_args::Error> {
        let mut args = pico_args::Arguments::from_env();
        let subcommand = args.subcommand()?;
        let Some(subcommand) = subcommand else {
            return Err(pico_args::Error::MissingArgument);
        };
        if subcommand != "build-unpacker" {
            return Err(pico_args::Error::ArgumentParsingFailed {
                cause: format!("Unknown subcommand: {subcommand}"),
            });
        }
        Ok(Args::BuildUnpacker {
            wasi_sdk: args
                .opt_free_from_os_str(|s| Result::<_, std::convert::Infallible>::Ok(s.to_owned()))?
                .or_else(|| env::var_os("WASI_SDK_PATH"))
                .ok_or(pico_args::Error::MissingArgument)?
                .into(),
        })
    }
}

fn main() -> process::ExitCode {
    let Args::BuildUnpacker { wasi_sdk } = match Args::parse_args() {
        Ok(a) => a,
        Err(err) => {
            eprintln!("Error: {err}\n");
            eprintln!("{}", USAGE);
            return process::ExitCode::FAILURE;
        }
    };

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

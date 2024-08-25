use std::{
    fs::File,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
};

use anyhow::Context;
use clap::Parser;
use wasm_encoder as we;
use wasmparser as wp;

#[derive(Parser)]
struct Args {
    // Input wasm file path. Use `-` or don't specify to use stdin.
    #[clap(default_value = "-")]
    input: PathBuf,
    // Output wasm file path. Use `-` or don't specify to use stdout.
    #[clap(short, long, default_value = "-")]
    output: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let input = if args.input == Path::new("-") {
        Box::new(io::stdin().lock()) as Box<dyn io::Read>
    } else {
        Box::new(io::BufReader::new(File::open(args.input)?))
    };

    let mut info = RelevantInfoBuilder::new();
    let input = parse_stream_and_save(input, |payload| info.add_payload(payload))
        .context("parsing input as wasm module")?;
    let info = info.build()?;
    dbg!(info);

    if args.output == Path::new("-") && io::stdout().is_terminal() {
        anyhow::bail!("stdout is a terminal, cannot print the output wasm binary file");
    }

    Ok(())
}

fn parse_stream_and_save<R, F>(mut reader: R, mut consumer: F) -> anyhow::Result<Vec<u8>>
where
    R: io::Read,
    F: FnMut(wp::Payload) -> anyhow::Result<()>,
{
    let mut input_buffer = Vec::new();

    let mut consumed_bytes = 0;
    let mut eof = false;
    let mut parser = wasmparser::Parser::new(0);
    loop {
        let chunk = parser.parse(&input_buffer[consumed_bytes..], eof)?;

        let payload = match chunk {
            wasmparser::Chunk::NeedMoreData(more_bytes) => {
                let len = input_buffer.len();
                input_buffer.resize(
                    len.checked_add(more_bytes.try_into()?)
                        .context("parser asks for too much bytes")?,
                    0,
                );
                match reader.read(&mut input_buffer[len..]) {
                    Ok(filled_bytes) => {
                        if filled_bytes == 0 {
                            eof = true;
                        }
                        input_buffer.resize_with(len + filled_bytes, || unreachable!())
                    }
                    Err(err) => match err.kind() {
                        io::ErrorKind::Interrupted => {
                            input_buffer.resize_with(len, || unreachable!())
                        }
                        _ => return Err(err.into()),
                    },
                }
                continue;
            }
            wasmparser::Chunk::Parsed { consumed, payload } => {
                consumed_bytes = consumed_bytes + consumed;
                payload
            }
        };

        let is_end = matches!(payload, wp::Payload::End(_));
        consumer(payload).context("payload `consumer` error")?;
        if is_end {
            break;
        }
    }

    Ok(input_buffer)
}

#[derive(Debug)]
struct RelevantInfo {
    stack_global: u32,
    old_function_count: u32,
    start_function: u32,
    start_export: u32,
}

struct RelevantInfoBuilder {
    stack_global: Option<u32>,
    old_function_count: Option<u32>,
    start_function_and_export: Option<(u32, u32)>,
}

impl RelevantInfoBuilder {
    fn new() -> Self {
        Self {
            stack_global: None,
            old_function_count: None,
            start_function_and_export: None,
        }
    }

    fn add_payload(&mut self, payload: wp::Payload) -> anyhow::Result<()> {
        match payload {
            wp::Payload::GlobalSection(globals) => {
                for (i, global) in globals.into_iter().enumerate() {
                    let global = global?;
                    if global.ty.mutable {
                        anyhow::ensure!(
                            global.ty.content_type == wp::ValType::I32,
                            "encountered a mutable global with unexpected type: {:?}",
                            global.ty.content_type
                        );
                        anyhow::ensure!(
                            self.stack_global.is_none(),
                            "encountered a second mutable global"
                        );
                        self.stack_global = Some(i.try_into().unwrap());
                    }
                }
            }
            wp::Payload::FunctionSection(functions) => {
                anyhow::ensure!(
                    self.old_function_count.is_none(),
                    "encountered two function sections"
                );
                self.old_function_count = Some(functions.count());
            }
            wp::Payload::ExportSection(exports) => {
                for (i, export) in exports.into_iter().enumerate() {
                    let export = export?;
                    if export.name != "start" {
                        continue;
                    }
                    anyhow::ensure!(
                        self.start_function_and_export.is_none(),
                        "found multiple `start` exports"
                    );
                    self.start_function_and_export = Some((export.index, i.try_into().unwrap()));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn build(self) -> anyhow::Result<RelevantInfo> {
        let (start_function, start_export) = self
            .start_function_and_export
            .context("`start` export was not found")?;
        Ok(RelevantInfo {
            stack_global: self
                .stack_global
                .context("No stack global variable was found")?,
            old_function_count: self
                .old_function_count
                .context("No function sections in the module")?,
            start_function,
            start_export,
        })
    }
}

use std::{
    error::Error,
    fs::File,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};

use anyhow::Context;
use clap::Parser;
use wasm_encoder::{
    self as we,
    reencode::{self, Reencode},
};
use wasmparser as wp;

const UNPACKER_WASM: &[u8] = include_bytes!("upkr_unpacker.wasm");

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
    let unpacker = UnpackerComponents::parse(UNPACKER_WASM).unwrap();

    let module = reencode_with_unpacker(&input, info, unpacker)?;
    let output = module.finish();

    if args.output == Path::new("-") {
        anyhow::ensure!(
            !io::stdout().is_terminal(),
            "stdout is a terminal, cannot print the output wasm binary file"
        );
        io::stdout()
            .lock()
            .write_all(&output)
            .context("unable to write an output wasm module")?;
    } else {
        std::fs::write(args.output, output).context("unable to write an output wasm module")?;
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
    let mut parser = wp::Parser::new(0);
    loop {
        let chunk = parser.parse(&input_buffer[consumed_bytes..], eof)?;

        let payload = match chunk {
            wp::Chunk::NeedMoreData(more_bytes) => {
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
            wp::Chunk::Parsed { consumed, payload } => {
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
    old_type_count: u32,
    start_function: u32,
    start_export: u32,
}

impl RelevantInfo {
    fn unpacker_reencoder(&self) -> AdaptUnpacker {
        AdaptUnpacker {
            old_function_count: self.old_function_count,
            old_type_count: self.old_type_count,
        }
    }
}

struct RelevantInfoBuilder {
    stack_global: Option<u32>,
    old_function_count: Option<u32>,
    old_type_count: Option<u32>,
    start_function_and_export: Option<(u32, u32)>,
}

impl RelevantInfoBuilder {
    fn new() -> Self {
        Self {
            stack_global: None,
            old_function_count: None,
            old_type_count: None,
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
            wp::Payload::TypeSection(types) => {
                anyhow::ensure!(
                    self.old_type_count.is_none(),
                    "encountered two type sections"
                );
                self.old_type_count = Some(types.count());
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
            old_type_count: self
                .old_type_count
                .context("No type sections in the module")?,
            start_function,
            start_export,
        })
    }
}

struct UnpackerComponents<'a> {
    types: wp::TypeSectionReader<'a>,
    functions: wp::FunctionSectionReader<'a>,
    function_bodies: Vec<wp::FunctionBody<'a>>,
}

impl<'a> UnpackerComponents<'a> {
    fn parse(data: &'a [u8]) -> anyhow::Result<Self> {
        let mut types = None;
        let mut functions = None;
        let mut function_bodies = Vec::new();
        let parser = wp::Parser::new(0);

        for payload in parser.parse_all(data) {
            match payload? {
                wp::Payload::TypeSection(t) => {
                    anyhow::ensure!(types.is_none(), "multiple type sections found");
                    types = Some(t);
                }
                wp::Payload::FunctionSection(f) => {
                    anyhow::ensure!(functions.is_none(), "multiple function sections found");
                    functions = Some(f);
                }
                wp::Payload::CodeSectionEntry(function) => function_bodies.push(function),
                _ => (),
            }
        }
        Ok(UnpackerComponents {
            types: types.unwrap(),
            functions: functions.unwrap(),
            function_bodies,
        })
    }
}

fn reencode_with_unpacker<'a>(
    input_module: &[u8],
    info: RelevantInfo,
    unpacker: UnpackerComponents<'a>,
) -> anyhow::Result<we::Module> {
    let mut module = we::Module::new();
    let mut merger = Merger {
        function_bodies_left: info.old_function_count,
        info,
        unpacker,
    };
    merger.parse_core_module(&mut module, wp::Parser::new(0), input_module)?;

    return Ok(module);

    struct Merger<'a> {
        info: RelevantInfo,
        unpacker: UnpackerComponents<'a>,
        function_bodies_left: u32,
    }

    impl<'a> Reencode for Merger<'a> {
        type Error = io::Error;

        fn parse_type_section(
            &mut self,
            types: &mut we::TypeSection,
            section: wp::TypeSectionReader<'_>,
        ) -> Result<(), reencode::Error<Self::Error>> {
            reencode::utils::parse_type_section(self, types, section)?;
            reencode::utils::parse_type_section(
                &mut self.info.unpacker_reencoder(),
                types,
                self.unpacker.types.clone(),
            )?;
            Ok(())
        }

        fn parse_function_section(
            &mut self,
            functions: &mut wasm_encoder::FunctionSection,
            section: wasmparser::FunctionSectionReader<'_>,
        ) -> Result<(), reencode::Error<Self::Error>> {
            reencode::utils::parse_function_section(self, functions, section)?;
            reencode::utils::parse_function_section(
                &mut self.info.unpacker_reencoder(),
                functions,
                self.unpacker.functions.clone(),
            )?;
            Ok(())
        }

        fn parse_function_body(
            &mut self,
            code: &mut wasm_encoder::CodeSection,
            func: wasmparser::FunctionBody<'_>,
        ) -> Result<(), reencode::Error<Self::Error>> {
            reencode::utils::parse_function_body(self, code, func)?;
            self.function_bodies_left -= 1;
            if self.function_bodies_left == 0 {
                let mut unpacker_reencoder = self.info.unpacker_reencoder();
                for func in &self.unpacker.function_bodies {
                    reencode::utils::parse_function_body(
                        &mut unpacker_reencoder,
                        code,
                        func.clone(),
                    )?;
                }
            }
            Ok(())
        }
    }
}

struct AdaptUnpacker {
    old_function_count: u32,
    old_type_count: u32,
}

impl Reencode for AdaptUnpacker {
    type Error = io::Error;

    fn type_index(&mut self, ty: u32) -> u32 {
        ty.checked_add(self.old_type_count).expect("too many types")
    }

    fn function_index(&mut self, func: u32) -> u32 {
        func.checked_add(self.old_function_count)
            .expect("too many functions")
    }
}

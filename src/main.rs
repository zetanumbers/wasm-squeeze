use std::{
    error::Error,
    fmt,
    fs::File,
    io::{self, IsTerminal, Write},
    iter,
    ops::Range,
    path::{Path, PathBuf},
    process,
};

use anyhow::Context;
use clap::Parser;
use wasm_encoder::{
    self as we,
    reencode::{self, Reencode},
};
use wasmparser::{self as wp, FromReader};

/// Supported wasm features
const WASM_FEATURES: wp::WasmFeatures = {
    use wp::WasmFeatures as Ft;

    Ft::BULK_MEMORY
        .union(Ft::EXCEPTIONS)
        .union(Ft::FLOATS)
        .union(Ft::FUNCTION_REFERENCES)
        .union(Ft::GC)
        .union(Ft::LEGACY_EXCEPTIONS)
        .union(Ft::MULTI_VALUE)
        .union(Ft::MUTABLE_GLOBAL)
        .union(Ft::REFERENCE_TYPES)
        .union(Ft::RELAXED_SIMD)
        .union(Ft::SATURATING_FLOAT_TO_INT)
        .union(Ft::SIGN_EXTENSION)
        .union(Ft::SIMD)
        .union(Ft::TAIL_CALL)
};
const UNPACKER_WASM: &[u8] = include_bytes!("upkr_unpacker.wasm");

const MEM_SIZE: i32 = 0x10000;
const CONTEXT_OFFSET: i32 = 0;
const COMPRESSED_DATA_OFFSET: i32 = common::CONTEXT_SIZE;

#[derive(Parser)]
struct Args {
    // Input wasm file path. Use `-` or don't specify to use stdin.
    #[clap(default_value = "-")]
    input: PathBuf,
    // Output wasm file path. Use `-` or don't specify to use stdout.
    #[clap(short, long, default_value = "-")]
    output: PathBuf,
    // The compression level (0-9)
    #[clap(short, long, default_value = "9")]
    level: u8,
}

fn main() -> process::ExitCode {
    match try_main() {
        Ok(()) => process::ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e:?}");
            process::ExitCode::FAILURE
        }
    }
}

fn try_main() -> anyhow::Result<()> {
    env_logger::try_init_from_env(
        env_logger::Env::new()
            .filter_or("WASM_SQUEEZE_LOG", "info")
            .write_style("WASM_SQUEEZE_LOG_STYLE"),
    )?;
    let args = Args::parse();
    let input = if args.input == Path::new("-") {
        Box::new(io::stdin().lock()) as Box<dyn io::Read>
    } else {
        Box::new(io::BufReader::new(File::open(&args.input)?))
    };

    let mut info = RelevantInfoBuilder::new();
    let input = parse_stream_and_save(input, |payload| info.add_payload(payload))
        .context("parsing input as wasm module")?;
    let info = match info.build(&input) {
        Ok(info) => info,
        Err(err) => {
            for cause in err.chain() {
                if cause.is::<NoDataError>() {
                    log::warn!("No data to compress, simply passing through the input");
                    write_output(&args, &input).context("writing an output wasm module")?;
                    return Ok(());
                }
            }
            return Err(err);
        }
    };
    log::debug!("Retrieved relevant info from the input module:\n{info:#?}");
    let unpacker = UnpackerComponents::parse();

    let module = reencode_with_unpacker(&input, info, unpacker, args.level)?;
    let output = module.finish();

    let reduced_bytes = input.len() as isize - output.len() as isize;
    if reduced_bytes <= 0 {
        log::warn!(
            "Compression did not reduce wasm module's size, simply passing through the input"
        );
        write_output(&args, &input).context("writing an output wasm module")?;
    } else {
        log::info!(
            "Reduced wasm module size by {} bytes ({:.2}%)",
            reduced_bytes,
            (100.0 * reduced_bytes as f64 / input.len() as f64)
        );
        write_output(&args, &output).context("writing an output wasm module")?;
    }
    Ok(())
}

fn write_output(args: &Args, output: &[u8]) -> Result<(), anyhow::Error> {
    Ok(if args.output == Path::new("-") {
        anyhow::ensure!(
            !io::stdout().is_terminal(),
            "stdout is a terminal, cannot print the output wasm binary file"
        );
        io::stdout().lock().write_all(output)?;
    } else {
        std::fs::write(&args.output, output)?;
    })
}

fn parse_stream_and_save<'a, R, F>(mut reader: R, mut consumer: F) -> anyhow::Result<Vec<u8>>
where
    R: io::Read,
    F: FnMut(wp::Payload) -> anyhow::Result<()>,
{
    let mut input_buffer = Vec::new();

    let mut consumed_bytes = 0;
    let mut eof = false;
    let mut parser = wp::Parser::new(0);
    parser.set_features(WASM_FEATURES);

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
    stack: Stack,
    start: Option<Start>,
    data: Data<Vec<u8>>,
    old_function_count: u32,
    old_type_count: u32,
    import_function_count: u32,
}

#[derive(Debug)]
struct Stack {
    global_idx: u32,
    mem_range: Range<i32>,
}

#[derive(Debug)]
struct Start {
    function_idx: u32,
    export_idx: u32,
}

#[derive(Clone, Copy)]
struct Data<D> {
    offset: i32,
    data: D,
}

impl fmt::Debug for Data<Vec<u8>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Data")
            .field("offset", &self.offset)
            .field("data", &format_args!("[u8; {}]", self.data.len()))
            .finish()
    }
}

impl Data<Range<usize>> {
    fn parse_slice<'a>(&self, module: &'a [u8]) -> anyhow::Result<Data<&'a [u8]>> {
        let mut reader =
            wp::BinaryReader::new(&module[self.data.clone()], self.data.start, WASM_FEATURES);
        let data = wp::Data::from_reader(&mut reader)?;

        #[cfg(debug_assertions)]
        {
            let wp::DataKind::Active {
                memory_index,
                offset_expr,
            } = data.kind
            else {
                panic!("parsed data kind mismatch")
            };
            debug_assert_eq!(memory_index, 0, "multimemory is not supported");
            debug_assert_eq!(
                eval_i32(&offset_expr).context("evaluating data offset")?,
                self.offset,
                "parsed data offset mismatch"
            );
        }

        Ok(Data {
            data: data.data,
            offset: self.offset,
        })
    }
}

impl Data<&[u8]> {
    fn to_vec(&self) -> Data<Vec<u8>> {
        Data {
            offset: self.offset,
            data: self.data.to_owned(),
        }
    }
}

impl RelevantInfo {
    fn unpacker_reencoder(&self) -> AdaptUnpacker {
        AdaptUnpacker {
            functions_index_base: self.old_function_count + self.import_function_count,
            types_index_base: self.old_type_count,
        }
    }
}

struct RelevantInfoBuilder {
    stack: Option<Stack>,
    start: Option<Start>,
    data: Vec<Data<Range<usize>>>,
    old_functions: Option<Vec<u32>>,
    old_type_count: Option<u32>,
    import_function_count: Option<u32>,
}

impl RelevantInfoBuilder {
    fn new() -> Self {
        Self {
            stack: None,
            start: None,
            data: Vec::new(),
            old_functions: None,
            old_type_count: None,
            import_function_count: None,
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
                            self.stack.is_none(),
                            "encountered a second mutable global"
                        );
                        self.stack = Some(Stack {
                            global_idx: i.try_into().unwrap(),
                            mem_range: 0..eval_i32(&global.init_expr)
                                .context("evaluating presumed stack global")?,
                        });
                    }
                }
            }
            wp::Payload::DataSection(data) => {
                anyhow::ensure!(self.data.is_empty(), "encountered multiple data sections");
                self.data.reserve(data.count().try_into()?);
                for data in data {
                    let data = data?;
                    let wp::DataKind::Active {
                        memory_index,
                        offset_expr,
                    } = &data.kind
                    else {
                        continue;
                    };
                    anyhow::ensure!(*memory_index == 0, "multi memory is not supported");
                    let offset =
                        eval_i32(&offset_expr).context("evaluating a data offset expression")?;
                    self.data.push(Data {
                        data: data.range,
                        offset,
                    })
                }
            }
            wp::Payload::ImportSection(imports) => {
                anyhow::ensure!(
                    self.import_function_count.is_none(),
                    "encountered multiple import sections"
                );
                anyhow::ensure!(
                    self.old_functions.is_none(),
                    "encountered imports after the function section"
                );
                let mut import_function_count = 0;
                for import in imports {
                    let import = import?;
                    if let wp::TypeRef::Func(_) = import.ty {
                        import_function_count += 1;
                    }
                }
                self.import_function_count = Some(import_function_count);
            }
            wp::Payload::FunctionSection(functions) => {
                anyhow::ensure!(
                    self.old_functions.is_none(),
                    "encountered multiple function sections"
                );
                self.old_functions = Some(functions.into_iter().collect::<Result<_, _>>()?);
            }
            wp::Payload::TypeSection(types) => {
                anyhow::ensure!(
                    self.old_type_count.is_none(),
                    "encountered multiple type sections"
                );
                self.old_type_count = Some(types.count());
            }
            wp::Payload::ExportSection(exports) => {
                for (i, export) in exports.into_iter().enumerate() {
                    let export = export?;
                    if export.name != "start" {
                        continue;
                    }
                    anyhow::ensure!(self.start.is_none(), "found multiple `start` exports");
                    self.start = Some(Start {
                        export_idx: i.try_into().unwrap(),
                        function_idx: export.index,
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn build(mut self, input: &[u8]) -> anyhow::Result<RelevantInfo> {
        if self.data.is_empty() {
            return Err(NoDataError.into());
        }
        // zero sized data is't supported
        self.data.sort_unstable_by_key(|d| d.offset);

        // Merge data sections
        let mut data = self.data.iter();
        let mut output_data = data.next().unwrap().parse_slice(&input)?.to_vec();
        let mut init_bytes = 0;

        for data in data {
            init_bytes += data.data.len();
            let new_len = (data.offset - output_data.offset) as usize;
            anyhow::ensure!(output_data.data.len() <= new_len, "data sections overlap");
            output_data.data.resize(new_len, 0);
            output_data
                .data
                .extend_from_slice(data.parse_slice(&input)?.data);
        }
        log::info!(
            "Data section's memory has {:.2}% of initialized bytes",
            100.0 * init_bytes as f64 / output_data.data.len() as f64
        );

        let stack = self.stack.context("No stack global variable was found")?;
        anyhow::ensure!(
            stack.mem_range.start.max(output_data.offset) as usize
                > (stack.mem_range.end as usize)
                    .min(output_data.offset as usize + output_data.data.len()),
            "stack space intersects initialized memory"
        );

        let old_functions = self
            .old_functions
            .context("no function section encountered")?;
        Ok(RelevantInfo {
            stack,
            old_function_count: old_functions.len().try_into().unwrap(),
            import_function_count: self.import_function_count.unwrap_or(0),
            old_type_count: self.old_type_count.context("no type section was found")?,
            start: self.start,
            data: output_data,
        })
    }
}

#[derive(Debug)]
struct NoDataError;

impl fmt::Display for NoDataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        "no data to compress".fmt(f)
    }
}

impl Error for NoDataError {}

struct UnpackerComponents<'a> {
    types: wp::TypeSectionReader<'a>,
    functions: wp::FunctionSectionReader<'a>,
    function_bodies: Vec<wp::FunctionBody<'a>>,
    unpack_fn_idx: u32,
}

impl<'a> UnpackerComponents<'a> {
    fn parse() -> Self {
        let data = UNPACKER_WASM;
        let mut types = None;
        let mut functions = None;
        let mut function_bodies = Vec::new();
        let mut parser = wp::Parser::new(0);
        let mut unpack_fn_idx = None;
        parser.set_features(WASM_FEATURES);

        for payload in parser.parse_all(data) {
            match payload.unwrap() {
                wp::Payload::TypeSection(t) => {
                    assert!(types.is_none(), "multiple type sections found");
                    types = Some(t);
                }
                wp::Payload::FunctionSection(f) => {
                    assert!(functions.is_none(), "multiple function sections found");
                    functions = Some(f);
                }
                wp::Payload::CodeSectionStart { count, .. } => {
                    function_bodies.reserve(count.try_into().unwrap())
                }
                wp::Payload::CodeSectionEntry(function) => function_bodies.push(function),
                wp::Payload::ExportSection(exports) => {
                    let mut exports = exports.into_iter();
                    let export = exports.next().unwrap().unwrap();
                    assert!(unpack_fn_idx.is_none());
                    unpack_fn_idx = Some(export.index);
                    assert!(exports.next().is_none());
                }
                _ => (),
            }
        }
        UnpackerComponents {
            types: types.unwrap(),
            functions: functions.unwrap(),
            unpack_fn_idx: unpack_fn_idx.unwrap(),
            function_bodies,
        }
    }
}

fn reencode_with_unpacker<'a>(
    input_module: &[u8],
    info: RelevantInfo,
    unpacker: UnpackerComponents<'a>,
    compression_level: u8,
) -> anyhow::Result<we::Module> {
    let mut module = we::Module::new();

    let packed_data = upkr::pack(
        &info.data.data,
        compression_level,
        &upkr::Config::default(),
        None,
    );
    let packed_data = if info.data.data.len() <= packed_data.len() {
        log::warn!("Could not compress data into less bytes, writing old");
        None
    } else if usize::try_from(MEM_SIZE).unwrap()
        < packed_data.len() + usize::try_from(common::CONTEXT_SIZE).unwrap() + info.data.data.len()
    {
        log::warn!("Decompression requires more than 64KiB space, writing old");
        None
    } else {
        Some(packed_data)
    };

    let mut merger = Merger {
        function_bodies_left: info.old_function_count,
        unpack_fn_idx: info.import_function_count
            + info.old_function_count
            + unpacker.unpack_fn_idx,
        subroutine_fn_type_idx: info.old_type_count + unpacker.types.count(),
        new_start_fn_idx: info.import_function_count
            + info.old_function_count
            + unpacker.functions.count(),
        info,
        packed_data,
        unpacker,
    };
    merger.parse_core_module(&mut module, wp::Parser::new(0), input_module)?;

    return Ok(module);

    struct Merger<'a> {
        info: RelevantInfo,
        unpacker: UnpackerComponents<'a>,
        function_bodies_left: u32,
        subroutine_fn_type_idx: u32,
        new_start_fn_idx: u32,
        unpack_fn_idx: u32,
        packed_data: Option<Vec<u8>>,
    }

    impl<'a> Reencode for Merger<'a> {
        type Error = io::Error;

        fn parse_type_section(
            &mut self,
            types: &mut we::TypeSection,
            section: wp::TypeSectionReader<'_>,
        ) -> Result<(), reencode::Error<Self::Error>> {
            reencode::utils::parse_type_section(self, types, section)?;
            assert_eq!(types.len(), self.info.old_type_count);
            reencode::utils::parse_type_section(
                &mut self.info.unpacker_reencoder(),
                types,
                self.unpacker.types.clone(),
            )?;
            assert_eq!(types.len(), self.subroutine_fn_type_idx);
            types.function(iter::empty(), iter::empty());
            Ok(())
        }

        fn parse_function_section(
            &mut self,
            functions: &mut we::FunctionSection,
            section: wp::FunctionSectionReader<'_>,
        ) -> Result<(), reencode::Error<Self::Error>> {
            reencode::utils::parse_function_section(self, functions, section)?;
            assert_eq!(functions.len(), self.info.old_function_count);
            reencode::utils::parse_function_section(
                &mut self.info.unpacker_reencoder(),
                functions,
                self.unpacker.functions.clone(),
            )?;
            functions.function(self.subroutine_fn_type_idx);
            Ok(())
        }

        fn parse_function_body(
            &mut self,
            code: &mut we::CodeSection,
            func: wp::FunctionBody<'_>,
        ) -> Result<(), reencode::Error<Self::Error>> {
            reencode::utils::parse_function_body(self, code, func)?;
            self.function_bodies_left -= 1;
            if self.function_bodies_left == 0 {
                // Last function body parsed
                assert_eq!(code.len(), self.info.old_function_count);
                let mut unpacker_reencoder = self.info.unpacker_reencoder();
                for func in &self.unpacker.function_bodies {
                    reencode::utils::parse_function_body(
                        &mut unpacker_reencoder,
                        code,
                        func.clone(),
                    )?;
                }
                if self.packed_data.is_some() {
                    assert_eq!(
                        self.info.import_function_count + code.len(),
                        self.new_start_fn_idx
                    );
                    let mut func = we::Function::new(iter::empty());

                    let original_data_len = self.info.data.data.len().try_into().unwrap();
                    let destination_offset = MEM_SIZE.checked_sub(original_data_len).unwrap();
                    let original_data_offset = self.info.data.offset.try_into().unwrap();
                    assert!(destination_offset >= 0);

                    func.instruction(&we::Instruction::I32Const(CONTEXT_OFFSET))
                        .instruction(&we::Instruction::I32Const(destination_offset))
                        .instruction(&we::Instruction::I32Const(COMPRESSED_DATA_OFFSET))
                        .instruction(&we::Instruction::Call(self.unpack_fn_idx))
                        .instruction(&we::Instruction::Drop);

                    func.instruction(&we::Instruction::I32Const(original_data_offset))
                        .instruction(&we::Instruction::I32Const(destination_offset))
                        .instruction(&we::Instruction::I32Const(original_data_len))
                        .instruction(&we::Instruction::MemoryCopy {
                            src_mem: 0,
                            dst_mem: 0,
                        });

                    func.instruction(&we::Instruction::I32Const(0))
                        .instruction(&we::Instruction::I32Const(0))
                        .instruction(&we::Instruction::I32Const(original_data_offset))
                        .instruction(&we::Instruction::MemoryFill(0));

                    let original_data_end = original_data_offset + original_data_len;
                    func.instruction(&we::Instruction::I32Const(original_data_end))
                        .instruction(&we::Instruction::I32Const(0))
                        .instruction(&we::Instruction::I32Const(MEM_SIZE - original_data_end))
                        .instruction(&we::Instruction::MemoryFill(0));

                    if let Some(start) = &self.info.start {
                        func.instruction(&we::Instruction::Call(start.function_idx));
                    }
                    func.instruction(&we::Instruction::End);
                    code.function(&func);
                }
            }
            Ok(())
        }

        fn parse_data_section(
            &mut self,
            data: &mut we::DataSection,
            _section: wp::DataSectionReader<'_>,
        ) -> Result<(), reencode::Error<Self::Error>> {
            if let Some(packed) = self.packed_data.as_deref() {
                let offset = we::ConstExpr::i32_const(COMPRESSED_DATA_OFFSET);
                data.active(0, &offset, packed.iter().copied());
            } else {
                let offset = we::ConstExpr::i32_const(self.info.data.offset as i32);
                data.active(0, &offset, self.info.data.data.iter().copied());
            }
            Ok(())
        }

        fn parse_export_section(
            &mut self,
            exports: &mut wasm_encoder::ExportSection,
            section: wasmparser::ExportSectionReader<'_>,
        ) -> Result<(), reencode::Error<Self::Error>> {
            if self.packed_data.is_none() {
                return reencode::utils::parse_export_section(self, exports, section);
            }

            let mut start_encoded = false;
            for export in section {
                let export = export?;
                if export.name == "start" {
                    assert!(!start_encoded);
                    exports.export("start", we::ExportKind::Func, self.new_start_fn_idx);
                    start_encoded = true;
                } else {
                    reencode::utils::parse_export(self, exports, export)
                }
            }
            if !start_encoded {
                exports.export("start", we::ExportKind::Func, self.new_start_fn_idx);
            }
            Ok(())
        }
    }
}

struct AdaptUnpacker {
    functions_index_base: u32,
    types_index_base: u32,
}

impl Reencode for AdaptUnpacker {
    type Error = io::Error;

    fn type_index(&mut self, ty: u32) -> u32 {
        ty.checked_add(self.types_index_base)
            .expect("too many types")
    }

    fn function_index(&mut self, func: u32) -> u32 {
        func.checked_add(self.functions_index_base)
            .expect("too many functions")
    }
}

fn eval_i32(expr: &wp::ConstExpr) -> anyhow::Result<i32> {
    let mut reader = expr.get_operators_reader();
    let wp::Operator::I32Const { value } = reader.read()? else {
        anyhow::bail!("Expected expression to be a single `I32Const`");
    };
    anyhow::ensure!(
        matches!(reader.read()?, wp::Operator::End),
        "Expression has unexpected succeeding operators"
    );
    Ok(value as i32)
}

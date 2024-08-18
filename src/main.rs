use std::io::{self, Write};

use anyhow::Context;

fn main() {
    let input = parse_stream_and_save(io::stdin().lock(), |_| Ok(()))
        .expect("parsing input as wasm module");
}

fn parse_stream_and_save<R, F>(mut reader: R, mut consumer: F) -> anyhow::Result<Vec<u8>>
where
    R: io::Read,
    F: FnMut(wasmparser::Payload) -> anyhow::Result<()>,
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

        let is_end = matches!(payload, wasmparser::Payload::End(_));
        consumer(payload).context("payload `consumer` error")?;
        if is_end {
            break;
        }
    }

    Ok(input_buffer)
}

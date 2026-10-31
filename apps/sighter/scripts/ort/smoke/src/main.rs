//! `sighter-ort-smoke <cpu|coreml|directml>`: proves a built ONNX Runtime archive links, registers the named execution
//! provider (a failed registration is an error, never a silent CPU run) and computes a known answer. Exit 0 and one line
//! `ok <ep> <ms>` on success; exit 1 with the reason otherwise. ORT_DISABLE_TELEMETRY=1 must be set by the caller (the
//! second line of defence of ADR 0001 decision 4; this program checks it instead of setting it, which would need unsafe).

use ort::session::Session;
use ort::value::Tensor;
use std::process::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    let ep = std::env::args().nth(1).unwrap_or_default();
    match run(&ep) {
        Ok(ms) => {
            println!("ok {ep} {ms}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("sighter-ort-smoke: {ep}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(ep: &str) -> Result<u128, String> {
    if std::env::var("ORT_DISABLE_TELEMETRY").as_deref() != Ok("1") {
        return Err("ORT_DISABLE_TELEMETRY=1 is not set".into());
    }
    let started = Instant::now();
    let env = ort::init()
        .with_name("sighter-ort-smoke")
        .with_telemetry(false)
        .build()
        .map_err(|e| e.to_string())?;
    let mut builder = Session::builder(&env).map_err(|e| e.to_string())?;
    builder = match ep {
        "cpu" => builder,
        #[cfg(target_os = "macos")]
        "coreml" => builder
            .with_execution_providers([ort::ep::CoreML::default().build().error_on_failure()])
            .map_err(|e| e.to_string())?,
        #[cfg(windows)]
        "directml" => builder
            .with_execution_providers([ort::ep::DirectML::default().with_device_id(0).build().error_on_failure()])
            .map_err(|e| e.to_string())?,
        other => return Err(format!("unknown or unsupported execution provider '{other}'")),
    };
    let mut session = builder.commit_from_memory(&relu_model()).map_err(|e| e.to_string())?;
    let x = Tensor::from_array(([1usize, 4], vec![-1.0f32, 0.0, 2.0, -3.5])).map_err(|e| e.to_string())?;
    let outputs = session.run(ort::inputs!["x" => x]).map_err(|e| e.to_string())?;
    let (_, y) = outputs["y"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
    if y != [0.0, 0.0, 2.0, 0.0] {
        return Err(format!("Relu gave {y:?}"));
    }
    Ok(started.elapsed().as_millis())
}

/// A one-node ONNX model, `y = Relu(x)` on float[1, 4], IR 8, opset 17, encoded here so the program needs no model file
/// and no Python. Field numbers from onnx/onnx.proto (ModelProto, GraphProto, NodeProto, ValueInfoProto, TypeProto).
fn relu_model() -> Vec<u8> {
    let value_info = |name: &str| {
        let dim = |n: u64| field(1, &varint_field(1, n));
        let shape = [dim(1), dim(4)].concat();
        let tensor = [varint_field(1, 1), field(2, &shape)].concat(); // elem_type FLOAT, shape
        [field(1, name.as_bytes()), field(2, &field(1, &tensor))].concat()
    };
    let node = [field(1, b"x"), field(2, b"y"), field(3, b"relu"), field(4, b"Relu")].concat();
    let graph = [
        field(1, &node),
        field(2, b"smoke"),
        field(11, &value_info("x")),
        field(12, &value_info("y")),
    ]
    .concat();
    let opset = [field(1, b""), varint_field(2, 17)].concat();
    [
        varint_field(1, 8),
        field(2, b"sighter-ort-smoke"),
        field(7, &graph),
        field(8, &opset),
    ]
    .concat()
}

fn varint(mut v: u64, out: &mut Vec<u8>) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn varint_field(number: u64, v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    varint(number << 3, &mut out);
    varint(v, &mut out);
    out
}

fn field(number: u64, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    varint((number << 3) | 2, &mut out);
    varint(bytes.len() as u64, &mut out);
    out.extend_from_slice(bytes);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_model_starts_with_ir_version_8() {
        assert_eq!(&super::relu_model()[..2], &[0x08, 0x08]);
    }
}

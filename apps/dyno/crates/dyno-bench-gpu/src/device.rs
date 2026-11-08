//! The graphics API side: adapters, buffers, pipelines and submissions. Everything that touches `wgpu` is in this file.

use crate::measure::{measure, MIN_MEASURE};
use dyno_bench::emit::{kind_phase, Progress};
use dyno_core::bench::gpu::{default_adapter, is_software, AdapterKind, GpuAdapter, REPETITIONS, SOFTWARE_NOTE};
use dyno_core::bench::BenchMetric;
use std::time::{Duration, Instant};

/// What the runner was asked.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    /// The adapter's index in [`list_adapters`] (the default: the discrete one, else the integrated one).
    pub adapter: Option<u32>,
    /// One repetition, tiny work (the mechanics tests).
    pub smoke: bool,
}

/// Why no numbers.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Not possible here.
    Unavailable(String),
    /// It failed.
    Failed(String),
}

const WIDTH: u32 = 3840;
const HEIGHT: u32 = 2160;
/// Invocations of the compute kernel per dispatch.
const INVOCATIONS: u32 = 1 << 20;
/// Bytes in each copy buffer: 128 MiB keeps the dispatch inside every device's workgroup limit.
const COPY_BYTES: u64 = 128 * 1024 * 1024;

fn instance() -> wgpu::Instance {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = backends();
    wgpu::Instance::new(desc)
}

/// Metal on macOS, DX12 then Vulkan on Windows, Vulkan on Linux; never GL.
fn backends() -> wgpu::Backends {
    if cfg!(target_os = "macos") {
        wgpu::Backends::METAL
    } else if cfg!(windows) {
        wgpu::Backends::DX12 | wgpu::Backends::VULKAN
    } else {
        wgpu::Backends::VULKAN
    }
}

fn kind_of(t: wgpu::DeviceType) -> AdapterKind {
    match t {
        wgpu::DeviceType::DiscreteGpu => AdapterKind::Discrete,
        wgpu::DeviceType::IntegratedGpu => AdapterKind::Integrated,
        wgpu::DeviceType::VirtualGpu => AdapterKind::Virtual,
        wgpu::DeviceType::Cpu => AdapterKind::Cpu,
        wgpu::DeviceType::Other => AdapterKind::Other,
    }
}

fn describe(i: usize, a: &wgpu::Adapter) -> GpuAdapter {
    let info = a.get_info();
    GpuAdapter {
        index: u32::try_from(i).unwrap_or(u32::MAX),
        name: info.name.clone(),
        kind: kind_of(info.device_type),
        backend: format!("{:?}", info.backend),
        driver: format!("{} {}", info.driver, info.driver_info).trim().to_string(),
    }
}

fn adapters() -> Vec<wgpu::Adapter> {
    pollster::block_on(instance().enumerate_adapters(backends()))
}

/// Every adapter the graphics API sees, integrated and discrete.
pub fn list_adapters() -> Vec<GpuAdapter> {
    adapters().iter().enumerate().map(|(i, a)| describe(i, a)).collect()
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Gpu {
    /// Submits and waits for the queue; the wall-clock time of that.
    fn run(&self, cmd: wgpu::CommandBuffer) -> Duration {
        let t = Instant::now();
        self.queue.submit([cmd]);
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        t.elapsed()
    }

    fn encoder(&self) -> wgpu::CommandEncoder {
        self.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("dyno-bench") })
    }
}

fn params(words: [u32; 4]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

const COMPUTE_WGSL: &str = r#"
struct P { iters: u32, n: u32, a: u32, b: u32 }
@group(0) @binding(0) var<storage, read_write> out: array<vec4<f32>>;
@group(0) @binding(1) var<uniform> p: P;
@compute @workgroup_size(256)
fn fma_chain(@builtin(global_invocation_id) id: vec3<u32>) {
  if (id.x >= p.n) { return; }
  var a = vec4<f32>(f32(id.x) * 1e-6 + 0.5);
  let b = vec4<f32>(0.999, 1.001, 0.9995, 1.0005);
  for (var i = 0u; i < p.iters; i = i + 1u) {
    a = fma(a, b, b); a = fma(a, b, b); a = fma(a, b, b); a = fma(a, b, b);
    a = fma(a, b, b); a = fma(a, b, b); a = fma(a, b, b); a = fma(a, b, b);
  }
  out[id.x] = a;
}
"#;

const COPY_WGSL: &str = r#"
struct P { iters: u32, n: u32, a: u32, b: u32 }
@group(0) @binding(0) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> dst: array<vec4<f32>>;
@group(0) @binding(2) var<uniform> p: P;
@compute @workgroup_size(256)
fn copy(@builtin(global_invocation_id) id: vec3<u32>) {
  if (id.x >= p.n) { return; }
  dst[id.x] = src[id.x];
}
"#;

const RENDER_WGSL: &str = r#"
@vertex fn fill_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
  var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
  return vec4<f32>(p[i], 0.0, 1.0);
}
@vertex fn geo_vs(@builtin(vertex_index) v: u32, @builtin(instance_index) inst: u32) -> @builtin(position) vec4<f32> {
  let gx = f32(inst % 1024u) / 1024.0 * 2.0 - 1.0;
  let gy = f32((inst / 1024u) % 1024u) / 1024.0 * 2.0 - 1.0;
  let s = 0.0015;
  var o = array<vec2<f32>, 3>(vec2<f32>(0.0, 0.0), vec2<f32>(s, 0.0), vec2<f32>(0.0, s));
  return vec4<f32>(gx + o[v].x, gy + o[v].y, 0.0, 1.0);
}
@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(0.01, 0.01, 0.01, 0.01); }
"#;

impl Gpu {
    fn buffer(&self, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dyno-bench"),
            size,
            usage,
            mapped_at_creation: false,
        })
    }

    fn uniform(&self, words: [u32; 4]) -> wgpu::Buffer {
        let b = self.buffer(16, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        self.queue.write_buffer(&b, 0, &params(words));
        b
    }

    fn compute_pipeline(&self, wgsl: &str, entry: &str) -> wgpu::ComputePipeline {
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(entry),
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });
        self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: None,
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    }

    /// GFLOPS of the FMA chain: `iters` loops of 8 `vec4` FMAs per invocation, 2 FLOPs per lane.
    fn compute_gflops(&self, smoke: bool) -> Option<f64> {
        let pipeline = self.compute_pipeline(COMPUTE_WGSL, "fma_chain");
        let out = self.buffer(u64::from(INVOCATIONS) * 16, wgpu::BufferUsages::STORAGE);
        let flops_per_iter = 8.0 * 4.0 * 2.0 * f64::from(INVOCATIONS);
        let rate = measure(if smoke { 1 } else { 64 }, flops_per_iter, min_measure(smoke), |iters| {
            let p = self.uniform([u32::try_from(iters).unwrap_or(u32::MAX), INVOCATIONS, 0, 0]);
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: out.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: p.as_entire_binding(),
                    },
                ],
            });
            let mut enc = self.encoder();
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(INVOCATIONS / 256, 1, 1);
            }
            self.run(enc.finish())
        })?;
        Some(rate / 1e9)
    }

    /// GB/s of a storage-buffer copy (each byte read once and written once).
    fn copy_gbps(&self, bytes: u64, smoke: bool) -> Option<f64> {
        let pipeline = self.compute_pipeline(COPY_WGSL, "copy");
        let src = self.buffer(bytes, wgpu::BufferUsages::STORAGE);
        let dst = self.buffer(bytes, wgpu::BufferUsages::STORAGE);
        let n = u32::try_from(bytes / 16).unwrap_or(u32::MAX);
        let p = self.uniform([0, n, 0, 0]);
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: src.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: dst.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: p.as_entire_binding(),
                },
            ],
        });
        let rate = measure(if smoke { 1 } else { 4 }, 2.0 * bytes as f64, min_measure(smoke), |copies| {
            let mut enc = self.encoder();
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                for _ in 0..copies {
                    pass.dispatch_workgroups(n.div_ceil(256), 1, 1);
                }
            }
            self.run(enc.finish())
        })?;
        Some(rate / 1e9)
    }

    fn render_pipeline(&self, vs: &str, blend: bool) -> wgpu::RenderPipeline {
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("render"),
            source: wgpu::ShaderSource::Wgsl(RENDER_WGSL.into()),
        });
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(vs),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(vs),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: blend.then_some(wgpu::BlendState { color: add, alpha: add }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    /// Draws `draws` times `per_draw` instances of three vertices into the off-screen target; the time of that.
    fn draw(&self, pipeline: &wgpu::RenderPipeline, target: &wgpu::TextureView, draws: u32, per_draw: u32) -> Duration {
        let mut enc = self.encoder();
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            for _ in 0..draws {
                pass.draw(0..3, 0..per_draw);
            }
        }
        self.run(enc.finish())
    }

    fn target(&self) -> wgpu::TextureView {
        self.device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("target"),
                size: wgpu::Extent3d {
                    width: WIDTH,
                    height: HEIGHT,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }

    /// Gigapixels per second of full-screen blended triangles.
    fn fill_gpix(&self, smoke: bool) -> Option<f64> {
        let (pipeline, target) = (self.render_pipeline("fill_vs", true), self.target());
        let per = f64::from(WIDTH) * f64::from(HEIGHT);
        let rate = measure(if smoke { 1 } else { 8 }, per, min_measure(smoke), |draws| {
            self.draw(&pipeline, &target, u32::try_from(draws).unwrap_or(u32::MAX), 1)
        })?;
        Some(rate / 1e9)
    }

    /// Million small triangles per second.
    fn geometry_mtri(&self, smoke: bool) -> Option<f64> {
        let (pipeline, target) = (self.render_pipeline("geo_vs", false), self.target());
        let rate = measure(if smoke { 1_000 } else { 100_000 }, 1.0, min_measure(smoke), |tris| {
            self.draw(&pipeline, &target, 1, u32::try_from(tris).unwrap_or(u32::MAX))
        })?;
        Some(rate / 1e6)
    }
}

fn min_measure(smoke: bool) -> Duration {
    if smoke {
        Duration::from_millis(50)
    } else {
        MIN_MEASURE
    }
}

/// Runs the GPU test on the chosen (or default) adapter.
pub fn run(s: &Settings, progress: &dyn Progress) -> Result<Vec<BenchMetric>, Outcome> {
    let all = adapters();
    if all.is_empty() {
        return Err(Outcome::Unavailable(
            "No graphics adapter was found, so there is no GPU to measure. A virtual machine without a virtual GPU has none.".into(),
        ));
    }
    let listed: Vec<GpuAdapter> = all.iter().enumerate().map(|(i, a)| describe(i, a)).collect();
    let chosen = match s.adapter {
        Some(i) => listed
            .iter()
            .find(|a| a.index == i)
            .ok_or_else(|| Outcome::Failed(format!("There is no graphics adapter number {i}.")))?,
        None => match default_adapter(&listed) {
            Some(a) => a,
            None => {
                let sw = listed.first().map_or("none", |a| a.name.as_str());
                return Err(Outcome::Unavailable(format!("{SOFTWARE_NOTE} The only adapter here is {sw}.")));
            }
        },
    };
    let software = is_software(chosen.kind, &chosen.name);
    progress.note(&format!(
        "Graphics adapter: {} ({}, {}).",
        chosen.name,
        chosen.backend,
        if chosen.driver.is_empty() {
            "driver not reported"
        } else {
            &chosen.driver
        }
    ));
    if software {
        progress.note(SOFTWARE_NOTE);
    }
    let adapter = &all[chosen.index as usize];
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("dyno-bench-gpu"),
        ..Default::default()
    }))
    .map_err(|e| Outcome::Failed(format!("The graphics adapter {} did not open ({e}).", chosen.name)))?;
    let gpu = Gpu { device, queue };
    let max_binding: u64 = gpu.device.limits().max_storage_buffer_binding_size;
    let copy_bytes = COPY_BYTES.min(max_binding) / 4096 * 4096;
    let reps = if s.smoke { 1 } else { REPETITIONS };
    let mut series: [Vec<f64>; 4] = Default::default();
    for n in 1..=reps {
        for (i, (what, f)) in [
            (
                "Compute",
                Box::new(|| gpu.compute_gflops(s.smoke)) as Box<dyn Fn() -> Option<f64> + '_>,
            ),
            ("Memory copy", Box::new(|| gpu.copy_gbps(copy_bytes, s.smoke))),
            ("Fill rate", Box::new(|| gpu.fill_gpix(s.smoke))),
            ("Geometry", Box::new(|| gpu.geometry_mtri(s.smoke))),
        ]
        .into_iter()
        .enumerate()
        {
            progress.report(kind_phase("gpu", &format!("Pass {n} of {reps} · {what}"), n - 1, reps));
            series[i].extend(f());
        }
        progress.report(kind_phase("gpu", &format!("Pass {n} of {reps} done"), n, reps));
    }
    let sw = if software { "-software" } else { "" };
    let tag = if software { " (software renderer)" } else { "" };
    let defs = [
        ("compute", "Compute throughput", "GFLOPS"),
        ("copy", "Memory copy bandwidth", "GB/s"),
        ("fill", "Fill rate", "Gpixel/s"),
        ("geometry", "Geometry throughput", "Mtri/s"),
    ];
    let out: Vec<BenchMetric> = defs
        .iter()
        .zip(series.iter())
        .filter(|(_, v)| !v.is_empty())
        .map(|((id, label, unit), v)| BenchMetric::measured(&format!("gpu{sw}-{id}"), &format!("{label}{tag}"), v, unit, true))
        .collect();
    if out.is_empty() {
        return Err(Outcome::Failed("The graphics adapter did not give a timing.".into()));
    }
    Ok(out)
}

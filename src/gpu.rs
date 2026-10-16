#![forbid(unsafe_code)]
// Made by Claude
//! gpu.rs - Kona on the graphics card. Built in with `cargo build --release --features gpu` (wgpu 30: safe Rust
//! API; it talks to the driver through Vulkan / DirectX 12). Without the feature these verbs answer
//! "GPU support not built in" and Kona is the plain CPU build.
//!
//! K verbs (all monadic, data stays on the card between calls):
//!   h: _gup list            put a float list (as 32-bit floats) or int list (32-bit ints) on the GPU -> handle
//!   _gup (h; list)          overwrite buffer h with a new list of the same length and type
//!   _gdown h                copy buffer h back -> float list / int list
//!   k: _gk "file.wgsl"      compile a WGSL compute kernel (entry point `main`, @workgroup_size(256)) -> id
//!   _grun (k; handles; params; n)
//!                           run kernel k on n threads. Buffers `handles` are bound at @binding(0), (1), ... in
//!                           order; the float list `params` (if not empty) comes next as
//!                           var<storage, read> array<f32>. Thread index in WGSL: gid.x + gid.y * 16776960u
//!                           (= 65535 * 256, for more than 16.7M threads). Returns n.
//!   _gfree h                drop buffer h
//! GPU mistakes (bad WGSL, wrong binding count, missing handle) come back as Kona errors, not crashes:
//! every call runs inside a wgpu validation error scope.

#[cfg(not(feature = "gpu"))]
use crate::errors::KonaError;
#[cfg(not(feature = "gpu"))]
use crate::types::K;

#[cfg(feature = "gpu")]
mod imp {
    use crate::errors::KonaError;
    use crate::types::{K, KData};
    use std::collections::HashMap;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use wgpu::util::DeviceExt;

    struct Buf {
        buf: wgpu::Buffer,
        n: usize,
        int: bool,
    }

    pub struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
        bufs: Vec<Option<Buf>>,
        kernels: Vec<wgpu::ComputePipeline>,
        by_source: HashMap<String, usize>,
        staging: Option<(wgpu::Buffer, u64)>,
        params: Option<(wgpu::Buffer, u64)>,
    }

    static GPU: OnceLock<Result<Mutex<Gpu>, String>> = OnceLock::new();

    fn start() -> Result<Mutex<Gpu>, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU found: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|e| format!("GPU would not start: {e}"))?;
        // anything not caught by a scope below is printed, never a crash
        device.on_uncaptured_error(std::sync::Arc::new(|e| eprintln!("GPU error: {e}")));
        Ok(Mutex::new(Gpu {
            device,
            queue,
            bufs: Vec::new(),
            kernels: Vec::new(),
            by_source: HashMap::new(),
            staging: None,
            params: None,
        }))
    }

    fn gpu() -> Result<MutexGuard<'static, Gpu>, KonaError> {
        match GPU.get_or_init(start) {
            Ok(m) => m.lock().map_err(|_| KonaError::System("GPU lock poisoned".to_string())),
            Err(e) => Err(KonaError::System(e.clone())),
        }
    }

    fn checked<T>(g: &Gpu, what: &str, f: impl FnOnce(&Gpu) -> T) -> Result<T, KonaError> {
        let scope = g.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let out = f(g);
        match pollster::block_on(scope.pop()) {
            None => Ok(out),
            Some(e) => Err(KonaError::System(format!("GPU {what}: {e}"))),
        }
    }

    fn handle(k: &K) -> Result<usize, KonaError> {
        match &**k {
            KData::IntAtom(h) if *h >= 0 => Ok(*h as usize),
            _ => Err(KonaError::Type),
        }
    }

    fn bytes_of(k: &K) -> Result<(Vec<u8>, usize, bool), KonaError> {
        match &**k {
            KData::FloatList(v) => {
                let f: Vec<f32> = v.iter().map(|&x| x as f32).collect();
                Ok((bytemuck::cast_slice(&f).to_vec(), v.len(), false))
            }
            KData::IntList(v) => {
                let i: Result<Vec<i32>, KonaError> =
                    v.iter().map(|&x| i32::try_from(x).map_err(|_| KonaError::Domain)).collect();
                Ok((bytemuck::cast_slice(&i?).to_vec(), v.len(), true))
            }
            _ => Err(KonaError::Type),
        }
    }

    const USE: wgpu::BufferUsages = wgpu::BufferUsages::STORAGE
        .union(wgpu::BufferUsages::COPY_SRC)
        .union(wgpu::BufferUsages::COPY_DST);

    pub fn up(x: &K) -> Result<K, KonaError> {
        let mut g = gpu()?;
        if let KData::GeneralList(parts) = &**x {
            // (h; list): overwrite
            if parts.len() != 2 {
                return Err(KonaError::Length);
            }
            let h = handle(&parts[0])?;
            let (bytes, n, int) = bytes_of(&parts[1])?;
            let b = g.bufs.get(h).and_then(|b| b.as_ref()).ok_or(KonaError::Index)?;
            if b.n != n || b.int != int {
                return Err(KonaError::Length);
            }
            if n > 0 {
                g.queue.write_buffer(&b.buf, 0, &bytes);
            }
            return Ok(K::int(h as i64));
        }
        let (mut bytes, n, int) = bytes_of(x)?;
        if bytes.is_empty() {
            bytes = vec![0u8; 4]; // buffers can't be 0 bytes
        }
        let buf = checked(&g, "upload", |g| {
            g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: &bytes, usage: USE })
        })?;
        let slot = g.bufs.iter().position(|b| b.is_none()).unwrap_or(g.bufs.len());
        let entry = Some(Buf { buf, n, int });
        if slot == g.bufs.len() {
            g.bufs.push(entry);
        } else {
            g.bufs[slot] = entry;
        }
        Ok(K::int(slot as i64))
    }

    pub fn down(x: &K) -> Result<K, KonaError> {
        let mut g = gpu()?;
        let h = handle(x)?;
        let (n, int) = {
            let b = g.bufs.get(h).and_then(|b| b.as_ref()).ok_or(KonaError::Index)?;
            (b.n, b.int)
        };
        if n == 0 {
            return Ok(if int { K::int_list(vec![]) } else { K::float_list_raw(vec![]) });
        }
        let size = (n * 4) as u64;
        if g.staging.as_ref().map_or(true, |(_, s)| *s < size) {
            let buf = g.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            g.staging = Some((buf, size));
        }
        let g = &*g;
        let staging = &g.staging.as_ref().ok_or(KonaError::System("no staging buffer".into()))?.0;
        let src = &g.bufs[h].as_ref().ok_or(KonaError::Index)?.buf;
        let mut enc = g.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        enc.copy_buffer_to_buffer(src, 0, staging, 0, size);
        g.queue.submit([enc.finish()]);
        let slice = staging.slice(0..size);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        g.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| KonaError::System(format!("GPU wait: {e}")))?;
        rx.recv()
            .map_err(|_| KonaError::System("GPU map lost".into()))?
            .map_err(|e| KonaError::System(format!("GPU map: {e}")))?;
        let out = {
            let view = slice.get_mapped_range().map_err(|e| KonaError::System(format!("GPU map: {e}")))?;
            if int {
                let v: &[i32] = bytemuck::cast_slice(&view);
                K::int_list(v.iter().map(|&x| x as i64).collect())
            } else {
                let v: &[f32] = bytemuck::cast_slice(&view);
                let mut o = crate::pool::take::<f64>(n);
                for (d, &s) in o.iter_mut().zip(v) {
                    *d = s as f64;
                }
                K::float_list_raw(o)
            }
        };
        staging.unmap();
        Ok(out)
    }

    pub fn kernel(x: &K) -> Result<K, KonaError> {
        let mut g = gpu()?;
        let path = match &**x {
            KData::CharList(v) => String::from_utf8_lossy(v).into_owned(),
            KData::SymbolAtom(s) => s.to_string(),
            _ => return Err(KonaError::Type),
        };
        let source = std::fs::read_to_string(&path).map_err(|e| KonaError::File(format!("{path}: {e}")))?;
        if let Some(&id) = g.by_source.get(&source) {
            return Ok(K::int(id as i64));
        }
        let pipe = checked(&g, &format!("kernel {path}"), |g| {
            let module = g.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(source.clone().into()),
            });
            g.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: None,
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            })
        })?;
        g.kernels.push(pipe);
        let id = g.kernels.len() - 1;
        g.by_source.insert(source, id);
        Ok(K::int(id as i64))
    }

    pub fn run(x: &K) -> Result<K, KonaError> {
        let mut g = gpu()?;
        let parts = crate::verbs::math::to_vec_of_atoms(x)?;
        if parts.len() != 4 {
            return Err(KonaError::Length);
        }
        let k = handle(&parts[0])?;
        let hs: Vec<usize> = match &*parts[1] {
            KData::IntList(v) => v.iter().map(|&h| usize::try_from(h).map_err(|_| KonaError::Index)).collect::<Result<_, _>>()?,
            KData::IntAtom(h) if *h >= 0 => vec![*h as usize],
            _ => return Err(KonaError::Type),
        };
        let params: Vec<f32> = match &*parts[2] {
            KData::FloatList(v) => v.iter().map(|&p| p as f32).collect(),
            KData::IntList(v) if v.is_empty() => Vec::new(),
            KData::GeneralList(v) if v.is_empty() => Vec::new(),
            _ => return Err(KonaError::Type),
        };
        let n = match &*parts[3] {
            KData::IntAtom(n) if *n >= 0 => *n as u64,
            _ => return Err(KonaError::Type),
        };
        if k >= g.kernels.len() {
            return Err(KonaError::Index);
        }
        if hs.iter().any(|&h| g.bufs.get(h).map_or(true, |b| b.is_none())) {
            return Err(KonaError::Index);
        }
        if !params.is_empty() {
            let size = (params.len() * 4) as u64;
            if g.params.as_ref().map_or(true, |(_, s)| *s < size) {
                let buf = g.device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: size.max(256),
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                g.params = Some((buf, size.max(256)));
            }
            let pb = &g.params.as_ref().ok_or(KonaError::System("no params buffer".into()))?.0;
            g.queue.write_buffer(pb, 0, bytemuck::cast_slice(&params));
        }
        let groups = n.div_ceil(256);
        if groups > 65535 * 65535 {
            return Err(KonaError::Limit);
        }
        let g = &*g;
        checked(g, "run", |g| {
            let pipe = &g.kernels[k];
            let mut entries: Vec<wgpu::BindGroupEntry> = hs
                .iter()
                .enumerate()
                .map(|(i, &h)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: g.bufs[h].as_ref().map(|b| b.buf.as_entire_binding()).unwrap_or_else(|| unreachable!()),
                })
                .collect();
            if let (false, Some((pb, _))) = (params.is_empty(), g.params.as_ref()) {
                entries.push(wgpu::BindGroupEntry { binding: hs.len() as u32, resource: pb.as_entire_binding() });
            }
            let bind = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipe.get_bind_group_layout(0),
                entries: &entries,
            });
            let mut enc = g.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
                pass.set_pipeline(pipe);
                pass.set_bind_group(0, &bind, &[]);
                if groups > 0 {
                    pass.dispatch_workgroups(groups.min(65535) as u32, groups.div_ceil(65535) as u32, 1);
                }
            }
            g.queue.submit([enc.finish()]);
        })?;
        g.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| KonaError::System(format!("GPU wait: {e}")))?;
        Ok(K::int(n as i64))
    }

    pub fn free(x: &K) -> Result<K, KonaError> {
        let mut g = gpu()?;
        let h = handle(x)?;
        match g.bufs.get_mut(h) {
            Some(slot) if slot.is_some() => {
                *slot = None;
                Ok(K::nil())
            }
            _ => Err(KonaError::Index),
        }
    }
}

#[cfg(feature = "gpu")]
pub use imp::{down, free, kernel, run, up};

#[cfg(not(feature = "gpu"))]
fn off() -> Result<K, KonaError> {
    Err(KonaError::FeatureDisabled(
        "GPU support not built in: cargo build --release --features gpu".to_string(),
    ))
}
#[cfg(not(feature = "gpu"))]
pub fn up(_: &K) -> Result<K, KonaError> {
    off()
}
#[cfg(not(feature = "gpu"))]
pub fn down(_: &K) -> Result<K, KonaError> {
    off()
}
#[cfg(not(feature = "gpu"))]
pub fn kernel(_: &K) -> Result<K, KonaError> {
    off()
}
#[cfg(not(feature = "gpu"))]
pub fn run(_: &K) -> Result<K, KonaError> {
    off()
}
#[cfg(not(feature = "gpu"))]
pub fn free(_: &K) -> Result<K, KonaError> {
    off()
}
// Made by Claude

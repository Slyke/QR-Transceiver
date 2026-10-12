use crate::*;
use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Clone, Default)]
pub struct CameraState {
    running: Arc<AtomicBool>,
    preview: Arc<Mutex<Value>>,
    controls: Arc<Mutex<Vec<(String, Value)>>>,
    generation: Arc<std::sync::atomic::AtomicU64>,
}
impl CameraState {
    #[cfg(feature = "camera")]
    fn active(&self, generation: u64) -> bool {
        self.running.load(Ordering::Relaxed)
            && self.generation.load(Ordering::Relaxed) == generation
    }
    #[cfg(feature = "camera")]
    fn publish(&self, generation: u64, value: Value) {
        if let Ok(mut preview) = self.preview.lock()
            && self.generation.load(Ordering::Relaxed) == generation
        {
            *preview = value;
        }
    }
    pub fn snapshot(&self) -> Result<Value> {
        Ok(self
            .preview
            .lock()
            .map_err(|_| anyhow::anyhow!("camera unavailable"))?
            .clone())
    }
    pub fn set_control(&self, name: String, value: Value) -> Result<()> {
        let mut q = self
            .controls
            .lock()
            .map_err(|_| anyhow::anyhow!("camera unavailable"))?;
        ensure!(q.len() < 32, "camera control queue full");
        q.push((name, value));
        Ok(())
    }
    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.running.store(false, Ordering::Relaxed);
        if let Ok(mut preview) = self.preview.lock() {
            *preview = json!({"running": false});
        }
    }
    pub fn start(&self, runtime: Runtime) -> Result<()> {
        #[cfg(not(feature = "camera"))]
        {
            let _ = runtime;
            anyhow::bail!("build with the camera feature to capture USB webcams");
        }
        #[cfg(feature = "camera")]
        {
            if self.running.swap(true, Ordering::Relaxed) {
                return Ok(());
            }
            let state = self.clone();
            let generation = state.generation.fetch_add(1, Ordering::Relaxed) + 1;
            state.publish(generation, json!({"running":true}));
            std::thread::Builder::new().name("qrtx-camera".into()).spawn(move||{
                while state.running.load(Ordering::Relaxed) && state.generation.load(Ordering::Relaxed)==generation{
                    if let Err(e)=capture(&state,&runtime,generation){
                        state.publish(generation,json!({"running":false,"error":e.to_string(),"reconnecting":true}));
                        tracing::warn!(errorKey="CAMERA_CAPTURE_FAILED",error=%e,"Camera will reconnect");
                        std::thread::sleep(std::time::Duration::from_secs(2));
                    }
                }
                state.publish(generation,json!({"running":false}));
            })?;
            Ok(())
        }
    }
}
pub fn devices() -> Result<Value> {
    #[cfg(feature = "camera")]
    {
        Ok(json!(nokhwa::query(nokhwa::utils::ApiBackend::Auto)?.into_iter().map(|c|json!({"index":c.index().to_string(),"name":c.human_name(),"description":c.description()})).collect::<Vec<_>>()))
    }
    #[cfg(not(feature = "camera"))]
    {
        Ok(json!([]))
    }
}
#[cfg(feature = "camera")]
fn capture(state: &CameraState, runtime: &Runtime, generation: u64) -> Result<()> {
    use base64::Engine as _;
    use nokhwa::{
        Camera,
        pixel_format::RgbFormat,
        utils::{
            CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType,
            Resolution,
        },
    };
    use qrtx_protocol::image::{self, DynamicImage};
    use std::time::{Duration, Instant};
    let p = runtime
        .engine
        .lock()
        .map_err(|_| anyhow::anyhow!("engine unavailable"))?
        .profile
        .clone();
    let requested =
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(CameraFormat::new(
            Resolution::new(p.camera_width, p.camera_height),
            FrameFormat::MJPEG,
            p.camera_fps,
        )));
    let mut camera = Camera::new(CameraIndex::Index(p.camera_index), requested)?;
    camera.open_stream()?;
    let format = camera.camera_format();
    let mut controls = camera_controls(&camera);
    let mut control_error: Option<String> = None;
    let mut count = 0u64;
    let mut capture_rate = 0.0;
    let mut measured_count = 0u64;
    let mut measured_at = Instant::now();
    let mut preview_at = Instant::now() - Duration::from_secs(1);
    // Keep scanning off the capture thread. A single queued frame bounds memory and latency.
    let (sender, receiver) = std::sync::mpsc::sync_channel::<image::GrayImage>(1);
    let scan_stats = Mutex::new((0u64, 0.0f64, 0.0f64));
    let result = std::thread::scope(|scope| {
        let scan_stats_ref = &scan_stats;
        scope.spawn(move || {
            let mut decoded = 0u64;
            let mut scanned = 0u64;
            let mut rate = 0.0;
            let mut measured_at = Instant::now();
            while let Ok(gray) = receiver.recv() {
                if !state.active(generation) { break; }
                let started = Instant::now();
                let mut texts = qrtx_protocol::scan(gray.clone());
                if texts.is_empty() {
                    let mut inverse = gray;
                    image::imageops::invert(&mut inverse);
                    texts = qrtx_protocol::scan(inverse);
                }
                let scan_ms = started.elapsed().as_secs_f64() * 1000.0;
                for text in texts {
                    if !state.active(generation) { break; }
                    if let Ok(mut engine) = runtime.engine.lock() {
                        match engine.ingest(&text) {
                            Ok(()) => decoded += 1,
                            Err(e) => tracing::debug!(errorKey="CAMERA_FRAME_REJECTED",error=%e,"Frame rejected"),
                        }
                    }
                }
                scanned += 1;
                if measured_at.elapsed() >= Duration::from_secs(1) {
                    rate = scanned as f64 / measured_at.elapsed().as_secs_f64();
                    scanned = 0;
                    measured_at = Instant::now();
                }
                if let Ok(mut stats) = scan_stats_ref.lock() { *stats = (decoded, rate, scan_ms); }
            }
        });
        let result = (|| -> Result<()> {
            while state.active(generation) {
                let requests = std::mem::take(
                    &mut *state
                        .controls
                        .lock()
                        .map_err(|_| anyhow::anyhow!("camera control queue unavailable"))?,
                );
                let changed = !requests.is_empty();
                for (name, value) in requests {
                    let result = (|| -> Result<()> {
                        let c = camera
                            .camera_controls()?
                            .into_iter()
                            .find(|c| c.control().to_string() == name)
                            .context("unsupported camera control")?;
                        camera.set_camera_control(c.control(), serde_json::from_value(value)?)?;
                        Ok(())
                    })();
                    control_error = result.err().map(|e| e.to_string());
                }
                if changed {
                    controls = camera_controls(&camera);
                }
                let frame = camera.frame()?;
                let rgb = if frame.source_frame_format() == FrameFormat::MJPEG {
                    image::load_from_memory_with_format(frame.buffer(), image::ImageFormat::Jpeg)?
                        .into_rgb8()
                } else {
                    frame.decode_image::<RgbFormat>()?
                };
                let mut image = DynamicImage::ImageRgb8(rgb);
                if p.camera_mirror {
                    image = image.fliph();
                }
                image = match p.camera_rotation {
                    90 => image.rotate90(),
                    180 => image.rotate180(),
                    270 => image.rotate270(),
                    _ => image,
                };
                if let Some([x, y, w, h]) = p.camera_crop {
                    ensure!(
                        w > 0
                            && h > 0
                            && x.checked_add(w).is_some_and(|v| v <= image.width())
                            && y.checked_add(h).is_some_and(|v| v <= image.height()),
                        "camera crop is outside image"
                    );
                    image = image.crop_imm(x, y, w, h);
                }
                if p.camera_zoom > 1.0 {
                    let w = (image.width() as f32 / p.camera_zoom) as u32;
                    let h = (image.height() as f32 / p.camera_zoom) as u32;
                    image = image.crop_imm((image.width() - w) / 2, (image.height() - h) / 2, w, h);
                }
                let _ = sender.try_send(image.to_luma8());
                count += 1;
                if measured_at.elapsed() >= Duration::from_secs(1) {
                    capture_rate =
                        (count - measured_count) as f64 / measured_at.elapsed().as_secs_f64();
                    measured_count = count;
                    measured_at = Instant::now();
                }
                if preview_at.elapsed() >= Duration::from_millis(80) {
                    let thumbnail = image.thumbnail(640, 360);
                    let mut bytes = Vec::new();
                    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 70)
                        .encode_image(&thumbnail)?;
                    let url = format!(
                        "data:image/jpeg;base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(bytes)
                    );
                    let (decoded, scan_fps, scan_ms) = *scan_stats
                        .lock()
                        .map_err(|_| anyhow::anyhow!("scan stats unavailable"))?;
                    state.publish(generation, json!({"running":true,"image":url,"captured":count,"decoded":decoded,
                        "capture_fps":capture_rate,"scan_fps":scan_fps,"scan_ms":scan_ms,"device_fps":format.frame_rate(),
                        "width":format.width(),"height":format.height(),"format":format.format().to_string(),
                        "timestamp":qrtx_core::now(),"controls":controls,"controlError":control_error}));
                    preview_at = Instant::now();
                }
            }
            Ok(())
        })();
        drop(sender);
        result
    });
    let stopped = camera.stop_stream();
    result?;
    stopped?;
    Ok(())
}
#[cfg(feature = "camera")]
fn camera_controls(camera: &nokhwa::Camera) -> Vec<Value> {
    camera.camera_controls().unwrap_or_default().into_iter().map(|c| json!({"name":c.control().to_string(),"label":c.name(),"value":c.value(),"description":c.description(),"flags":c.flag()})).collect()
}

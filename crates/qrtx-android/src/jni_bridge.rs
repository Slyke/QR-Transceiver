use super::*;
use jni::{
    JNIEnv,
    objects::{JByteArray, JObject, JString},
    sys::{jint, jlong, jstring},
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicI64, Ordering},
    },
};

type Shared = Arc<Mutex<MobileEngine>>;
static ENGINES: OnceLock<Mutex<HashMap<i64, Shared>>> = OnceLock::new();
static NEXT: AtomicI64 = AtomicI64::new(1);

fn engines() -> &'static Mutex<HashMap<i64, Shared>> {
    ENGINES.get_or_init(Mutex::default)
}
fn lookup(handle: jlong) -> Result<Shared> {
    engines()
        .lock()
        .map_err(|_| anyhow::anyhow!("engine registry unavailable"))?
        .get(&handle)
        .cloned()
        .context("engine is closed")
}
fn reply(env: &mut JNIEnv<'_>, result: Result<Value>) -> jstring {
    let value = match result {
        Ok(value) => json!({"ok":true,"result":value}),
        Err(e) => json!({"ok":false,"error":e.to_string()}),
    };
    match env.new_string(value.to_string()) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(), // JNI retains the pending allocation exception
    }
}
fn guarded(f: impl FnOnce() -> Result<Value>) -> Result<Value> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err(anyhow::anyhow!("native engine failed; restart the app")))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_qrtx_transceiver_NativeEngine_nativeOpen(
    mut env: JNIEnv,
    _object: JObject,
    root: JString,
) -> jstring {
    let result = guarded(|| {
        let root: String = env.get_string(&root)?.into();
        let engine = MobileEngine::open(Path::new(&root))?;
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        engines()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine registry unavailable"))?
            .insert(id, Arc::new(Mutex::new(engine)));
        Ok(json!(id))
    });
    reply(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_qrtx_transceiver_NativeEngine_nativeCall(
    mut env: JNIEnv,
    _object: JObject,
    handle: jlong,
    method: JString,
    params: JString,
) -> jstring {
    let result = guarded(|| {
        let method: String = env.get_string(&method)?.into();
        let params: String = env.get_string(&params)?.into();
        ensure!(params.len() <= 1_048_576, "request too large");
        lookup(handle)?
            .lock()
            .map_err(|_| anyhow::anyhow!("engine unavailable"))?
            .dispatch(&method, serde_json::from_str(&params)?)
    });
    reply(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_qrtx_transceiver_NativeEngine_nativeScan(
    mut env: JNIEnv,
    _object: JObject,
    handle: jlong,
    bytes: JByteArray,
    width: jint,
    height: jint,
) -> jstring {
    let result = guarded(|| {
        ensure!(
            width > 0
                && height > 0
                && width <= 4096
                && height <= 4096
                && i64::from(width) * i64::from(height) <= 8_847_360,
            "invalid camera dimensions"
        );
        ensure!(
            env.get_array_length(&bytes)? == width * height,
            "invalid camera plane"
        );
        let texts = scan_luma(env.convert_byte_array(&bytes)?, width as u32, height as u32)?;
        Ok(lookup(handle)?
            .lock()
            .map_err(|_| anyhow::anyhow!("engine unavailable"))?
            .scan(texts))
    });
    reply(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_qrtx_transceiver_NativeEngine_nativeClose(
    _env: JNIEnv,
    _object: JObject,
    handle: jlong,
) {
    if let Ok(mut engines) = engines().lock() {
        engines.remove(&handle);
    }
}

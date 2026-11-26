use std::net::TcpStream;
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use pvpn_core::{ClientConfig, Psk};
use tokio::sync::watch;

struct Running {
    shutdown: watch::Sender<bool>,
    handle: JoinHandle<()>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn read_string(env: &mut JNIEnv, value: &JString) -> Option<String> {
    env.get_string(value).ok().map(|s| s.into())
}

fn stop_locked(state: &mut Option<Running>) {
    if let Some(running) = state.take() {
        let _ = running.shutdown.send(true);
        let _ = running.handle.join();
    }
}

#[no_mangle]
pub extern "system" fn Java_com_pvpn_app_NativeBridge_start(
    mut env: JNIEnv,
    _class: JClass,
    server: JString,
    psk: JString,
    listen: JString,
) -> jboolean {
    let Some(server) = read_string(&mut env, &server) else {
        return JNI_FALSE;
    };
    let Some(psk_hex) = read_string(&mut env, &psk) else {
        return JNI_FALSE;
    };
    let Some(listen) = read_string(&mut env, &listen) else {
        return JNI_FALSE;
    };
    let Ok(psk) = Psk::from_hex(&psk_hex) else {
        return JNI_FALSE;
    };

    let listen_probe = listen.clone();
    let config = ClientConfig {
        listen,
        server,
        psk,
        connect_timeout_secs: 15,
        http_listen: None,
    };
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    if let Ok(mut slot) = LAST_ERROR.lock() {
        *slot = None;
    }
    let handle = match std::thread::Builder::new()
        .name("pvpn-android".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    if let Ok(mut slot) = LAST_ERROR.lock() {
                        *slot = Some(format!("runtime: {error}"));
                    }
                    return;
                }
            };
            if let Err(error) =
                runtime.block_on(pvpn_client::client::run_until(config, shutdown_rx))
            {
                if let Ok(mut slot) = LAST_ERROR.lock() {
                    *slot = Some(error.to_string());
                }
            }
        }) {
        Ok(handle) => handle,
        Err(error) => {
            if let Ok(mut slot) = LAST_ERROR.lock() {
                *slot = Some(format!("spawn: {error}"));
            }
            return JNI_FALSE;
        }
    };

    let mut state = match RUNNING.lock() {
        Ok(state) => state,
        Err(_) => return JNI_FALSE,
    };
    stop_locked(&mut state);
    *state = Some(Running {
        shutdown: shutdown_tx,
        handle,
    });

    for _ in 0..20 {
        if TcpStream::connect(&listen_probe).is_ok() {
            return JNI_TRUE;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    stop_locked(&mut state);
    JNI_FALSE
}

#[no_mangle]
pub extern "system" fn Java_com_pvpn_app_NativeBridge_stop(
    _env: JNIEnv,
    _class: JClass,
) {
    if let Ok(mut state) = RUNNING.lock() {
        stop_locked(&mut state);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_pvpn_app_NativeBridge_lastError(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let text = LAST_ERROR
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .unwrap_or_default();
    env.new_string(text)
        .map(|value| value.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_pvpn_app_NativeBridge_isRunning(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    let running = RUNNING
        .lock()
        .ok()
        .and_then(|state| state.as_ref().map(|running| !running.handle.is_finished()))
        .unwrap_or(false);
    if running {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

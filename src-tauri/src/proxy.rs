use crate::{
    certificate::{ensure_certificate, mobileconfig_profile, regenerate_certificate},
    export::write_profile,
    protocol::decode_profile,
    setup::{self, SetupSettings},
};
use flate2::read::{GzDecoder, ZlibDecoder};
use http_body_util::BodyExt;
use hudsucker::{
    Body, HttpContext, HttpHandler, Proxy, RequestOrResponse,
    certificate_authority::RcgenAuthority,
    hyper::{Method, Request, Response, StatusCode, header},
    rustls::crypto::aws_lc_rs,
};
use serde::Serialize;
use std::{
    io::Read,
    net::{IpAddr, Ipv4Addr, UdpSocket},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Manager, State};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

const PREFERRED_PROXY_PORT: u16 = 8080;
const PROFILE_PATH: &str = "/api/gateway_c2.php";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusSnapshot {
    pub phase: String,
    pub message: String,
    pub certificate_setup_completed: bool,
    pub proxy_setup_completed: bool,
    pub local_ip: Option<String>,
    pub port: Option<u16>,
    pub certificate_url: Option<String>,
    pub certificate_was_created: bool,
    pub export_path: Option<String>,
    pub profile_name: Option<String>,
}

impl StatusSnapshot {
    fn idle(settings: SetupSettings) -> Self {
        Self {
            phase: "idle".into(),
            message: "Préparez le proxy Wi‑Fi de l’iPhone pour commencer.".into(),
            certificate_setup_completed: settings.certificate_setup_completed,
            proxy_setup_completed: settings.proxy_setup_completed,
            local_ip: None,
            port: None,
            certificate_url: None,
            certificate_was_created: false,
            export_path: None,
            profile_name: None,
        }
    }
}

struct SharedState {
    status: Mutex<StatusSnapshot>,
}

impl SharedState {
    fn snapshot(&self) -> StatusSnapshot {
        self.status.lock().expect("status mutex poisoned").clone()
    }

    fn replace(&self, status: StatusSnapshot) {
        *self.status.lock().expect("status mutex poisoned") = status;
    }

    fn record_export(&self, path: PathBuf, display_name: String) {
        let mut status = self.status.lock().expect("status mutex poisoned");
        status.phase = "captured".into();
        status.message = "Le JSON a été créé.".into();
        status.export_path = Some(path.to_string_lossy().into_owned());
        status.profile_name = Some(display_name);
    }

    fn record_proxy_error(&self, error: impl std::fmt::Display) {
        let mut status = self.status.lock().expect("status mutex poisoned");
        if status.phase != "captured" {
            status.phase = "error".into();
            status.message = format!("Le proxy s'est arrêté : {error}");
        }
    }
}

pub struct AppState {
    shared: Arc<SharedState>,
    cancel: Mutex<Option<CancellationToken>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            shared: Arc::new(SharedState {
                status: Mutex::new(StatusSnapshot::idle(SetupSettings::default())),
            }),
            cancel: Mutex::new(None),
        }
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.lock().ok().and_then(|mut value| value.take()) {
            cancel.cancel();
        }
    }
}

#[derive(Clone)]
struct CaptureHandler {
    capture_enabled: bool,
    is_profile_request: bool,
    certificate_der: Arc<Vec<u8>>,
    certificate_profile: Arc<Vec<u8>>,
    output_directory: Arc<PathBuf>,
    shared: Arc<SharedState>,
    cancel: CancellationToken,
}

impl CaptureHandler {
    fn certificate_profile_response(&self) -> Response<Body> {
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/x-apple-aspen-config")
            .header(
                header::CONTENT_DISPOSITION,
                "attachment; filename=SwagEx.mobileconfig",
            )
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(self.certificate_profile.as_ref().clone()))
            .expect("valid certificate profile response")
    }

    fn certificate_der_response(&self) -> Response<Body> {
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/x-x509-ca-cert")
            .header(
                header::CONTENT_DISPOSITION,
                "attachment; filename=SwagEx-CA.cer",
            )
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(self.certificate_der.as_ref().clone()))
            .expect("valid certificate response")
    }
}

impl HttpHandler for CaptureHandler {
    async fn handle_request(
        &mut self,
        _context: &HttpContext,
        request: Request<Body>,
    ) -> RequestOrResponse {
        if request.method() == Method::GET && request.uri().path() == "/certificate" {
            return self.certificate_profile_response().into();
        }
        if request.method() == Method::GET && request.uri().path() == "/SwagEx-CA.cer" {
            return self.certificate_der_response().into();
        }

        self.is_profile_request = request.uri().path() == PROFILE_PATH;
        request.into()
    }

    async fn handle_response(
        &mut self,
        _context: &HttpContext,
        response: Response<Body>,
    ) -> Response<Body> {
        if !self.capture_enabled || !self.is_profile_request {
            return response;
        }

        let (parts, body) = response.into_parts();
        let collected = match body.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(_) => return Response::from_parts(parts, Body::empty()),
        };
        let forwarded_body = collected.clone();
        let capture_body =
            decode_http_body(&parts.headers, &collected).unwrap_or_else(|| collected.to_vec());

        let output_directory = Arc::clone(&self.output_directory);
        let result = tokio::task::spawn_blocking(move || {
            let profile = decode_profile(&capture_body).ok()?;
            let command = profile.get("command").and_then(serde_json::Value::as_str)?;
            if !matches!(command, "HubUserLogin" | "GuestLogin") {
                return None;
            }
            Some(write_profile(&profile, &output_directory))
        })
        .await;

        if let Ok(Some(Ok(exported))) = result {
            self.shared
                .record_export(exported.path, exported.display_name);
            let cancel = self.cancel.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
                cancel.cancel();
            });
        }

        Response::from_parts(parts, Body::from(forwarded_body))
    }

    async fn should_intercept_connect(
        &mut self,
        _context: &HttpContext,
        request: &Request<Body>,
    ) -> bool {
        request
            .uri()
            .authority()
            .is_some_and(|authority| is_game_host(authority.host()))
    }

    async fn should_intercept_tls(
        &mut self,
        _context: &HttpContext,
        client_hello: hudsucker::rustls::server::ClientHello<'_>,
    ) -> bool {
        client_hello.server_name().is_some_and(is_game_host)
    }
}

fn is_game_host(host: &str) -> bool {
    host == "qpyou.cn" || host.ends_with(".qpyou.cn")
}

fn decode_http_body(headers: &header::HeaderMap, body: &[u8]) -> Option<Vec<u8>> {
    let encoding = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())?
        .trim()
        .to_ascii_lowercase();

    let mut decoded = Vec::new();
    match encoding.as_str() {
        "gzip" | "x-gzip" => GzDecoder::new(body).read_to_end(&mut decoded).ok()?,
        "deflate" => ZlibDecoder::new(body).read_to_end(&mut decoded).ok()?,
        "identity" => return Some(body.to_vec()),
        _ => return None,
    };
    Some(decoded)
}

async fn bind_listener(address: Ipv4Addr) -> anyhow::Result<TcpListener> {
    TcpListener::bind((address, PREFERRED_PROXY_PORT))
        .await
        .map_err(|error| {
            anyhow::anyhow!("le port {PREFERRED_PROXY_PORT} est indisponible : {error}")
        })
}

fn local_ipv4() -> anyhow::Result<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.connect("1.1.1.1:80")?;
    match socket.local_addr()?.ip() {
        IpAddr::V4(address) if !address.is_loopback() => Ok(address),
        _ => anyhow::bail!("aucune adresse Wi-Fi locale n'a été trouvée"),
    }
}

fn app_data_directory(app: &AppHandle) -> anyhow::Result<PathBuf> {
    Ok(app.path().app_data_dir()?)
}

fn configured(app: &AppHandle) -> anyhow::Result<SetupSettings> {
    setup::read(&app_data_directory(app)?)
}

fn cancel_running_proxy(state: &AppState) -> anyhow::Result<()> {
    if let Some(previous) = state
        .cancel
        .lock()
        .map_err(|_| anyhow::anyhow!("état interne indisponible"))?
        .take()
    {
        previous.cancel();
    }
    Ok(())
}

async fn start_proxy(
    app: AppHandle,
    state: &AppState,
    capture_enabled: bool,
    phase: &str,
    force_regenerate_certificate: bool,
) -> anyhow::Result<StatusSnapshot> {
    cancel_running_proxy(state)?;
    // Let the previous listener release port 8080 before binding the next
    // setup/capture phase. This matters when moving from certificate setup to
    // proxy setup in the same button action.
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    let app_data = app_data_directory(&app)?;
    let output_directory = app.path().download_dir()?;
    let local_ip = local_ipv4()?;
    let settings = configured(&app)?;
    let certificate_directory = app_data.join("certificate");
    if !force_regenerate_certificate
        && settings.certificate_setup_completed
        && !certificate_directory.exists()
    {
        anyhow::bail!(
            "Le certificat SwagEx installé précédemment est introuvable. Aucune nouvelle CA n’a été créée ; utilisez « Nouveau certificat ? » uniquement si vous acceptez de le réinstaller sur l’iPhone."
        );
    }
    let certificate = if force_regenerate_certificate {
        regenerate_certificate(&certificate_directory)?
    } else {
        ensure_certificate(&certificate_directory)?
    };
    let certificate_was_created = certificate.was_created;
    let certificate_der = Arc::new(certificate.der);
    let certificate_profile = Arc::new(mobileconfig_profile(certificate_der.as_ref()));
    let listener = bind_listener(local_ip).await?;
    let port = listener.local_addr()?.port();
    let certificate_url = format!("http://{local_ip}:{port}/certificate");
    let cancel = CancellationToken::new();

    let handler = CaptureHandler {
        capture_enabled,
        is_profile_request: false,
        certificate_der,
        certificate_profile,
        output_directory: Arc::new(output_directory),
        shared: Arc::clone(&state.shared),
        cancel: cancel.clone(),
    };
    let authority = RcgenAuthority::new(certificate.issuer, 128, aws_lc_rs::default_provider());
    let proxy = Proxy::builder()
        .with_listener(listener)
        .with_ca(authority)
        .with_rustls_connector(aws_lc_rs::default_provider())
        .with_http_handler(handler)
        .with_graceful_shutdown(cancel.clone().cancelled_owned())
        .build()?;

    let status = StatusSnapshot {
        phase: phase.into(),
        message: match phase {
            "certificate_setup" => {
                "Installez puis activez le certificat SwagEx sur l’iPhone.".into()
            }
            "proxy_setup" => "Saisissez ces valeurs dans le proxy Wi‑Fi de l’iPhone.".into(),
            "listening" => "Ouvrez Summoners War et connectez-vous sur l’iPhone configuré.".into(),
            _ => "".into(),
        },
        certificate_setup_completed: settings.certificate_setup_completed,
        proxy_setup_completed: settings.proxy_setup_completed,
        local_ip: Some(local_ip.to_string()),
        port: Some(port),
        certificate_url: Some(certificate_url),
        certificate_was_created,
        export_path: None,
        profile_name: None,
    };
    state.shared.replace(status.clone());
    *state
        .cancel
        .lock()
        .map_err(|_| anyhow::anyhow!("état interne indisponible"))? = Some(cancel);

    let shared = Arc::clone(&state.shared);
    tauri::async_runtime::spawn(async move {
        if let Err(error) = proxy.start().await {
            shared.record_proxy_error(error);
        }
    });

    Ok(status)
}

#[tauri::command]
pub fn export_status(app: AppHandle, state: State<'_, AppState>) -> Result<StatusSnapshot, String> {
    let settings = configured(&app).map_err(|error| error.to_string())?;
    let mut status = state.shared.snapshot();
    status.certificate_setup_completed = settings.certificate_setup_completed;
    status.proxy_setup_completed = settings.proxy_setup_completed;
    if status.phase == "idle" {
        status.message = StatusSnapshot::idle(settings).message;
    }
    Ok(status)
}

#[tauri::command]
pub async fn start_certificate_setup(
    app: AppHandle,
    state: State<'_, AppState>,
    regenerate: bool,
) -> Result<StatusSnapshot, String> {
    if regenerate {
        let app_data = app_data_directory(&app).map_err(|error| error.to_string())?;
        setup::write(
            &app_data,
            SetupSettings {
                certificate_setup_completed: false,
                proxy_setup_completed: false,
            },
        )
        .map_err(|error| error.to_string())?;
    }
    start_proxy(app, &state, false, "certificate_setup", regenerate)
        .await
        .map_err(|error| {
            let message = error.to_string();
            let settings = state.shared.snapshot();
            state.shared.replace(StatusSnapshot {
                phase: "error".into(),
                message: message.clone(),
                ..StatusSnapshot::idle(SetupSettings {
                    certificate_setup_completed: settings.certificate_setup_completed,
                    proxy_setup_completed: settings.proxy_setup_completed,
                })
            });
            message
        })
}

#[tauri::command]
pub async fn start_proxy_setup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StatusSnapshot, String> {
    let settings = configured(&app).map_err(|error| error.to_string())?;
    if !settings.certificate_setup_completed {
        return Err("Installez d’abord le certificat SwagEx sur l’iPhone.".into());
    }

    start_proxy(app, &state, false, "proxy_setup", false)
        .await
        .map_err(|error| {
            let message = error.to_string();
            state.shared.replace(StatusSnapshot {
                phase: "error".into(),
                message: message.clone(),
                ..StatusSnapshot::idle(settings)
            });
            message
        })
}

#[tauri::command]
pub async fn complete_certificate_setup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StatusSnapshot, String> {
    let app_data = app_data_directory(&app).map_err(|error| error.to_string())?;
    let current = setup::read(&app_data).map_err(|error| error.to_string())?;
    setup::write(
        &app_data,
        SetupSettings {
            certificate_setup_completed: true,
            proxy_setup_completed: current.proxy_setup_completed,
        },
    )
    .map_err(|error| error.to_string())?;
    start_proxy(app, &state, false, "proxy_setup", false)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn complete_proxy_setup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StatusSnapshot, String> {
    let app_data = app_data_directory(&app).map_err(|error| error.to_string())?;
    let current = setup::read(&app_data).map_err(|error| error.to_string())?;
    if !current.certificate_setup_completed {
        return Err("Installez d’abord le certificat SwagEx sur l’iPhone.".into());
    }
    setup::write(
        &app_data,
        SetupSettings {
            certificate_setup_completed: true,
            proxy_setup_completed: true,
        },
    )
    .map_err(|error| error.to_string())?;
    start_proxy(app, &state, true, "listening", false)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn cancel_export(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StatusSnapshot, String> {
    cancel_running_proxy(&state).map_err(|error| error.to_string())?;
    let settings = configured(&app).map_err(|error| error.to_string())?;
    if settings.certificate_setup_completed {
        start_proxy(app, &state, false, "proxy_setup", false)
            .await
            .map_err(|error| error.to_string())
    } else {
        let status = StatusSnapshot::idle(settings);
        state.shared.replace(status.clone());
        Ok(status)
    }
}

#[tauri::command]
pub fn reset_setup(app: AppHandle, state: State<'_, AppState>) -> Result<StatusSnapshot, String> {
    cancel_running_proxy(&state).map_err(|error| error.to_string())?;
    let app_data = app_data_directory(&app).map_err(|error| error.to_string())?;
    setup::reset(&app_data).map_err(|error| error.to_string())?;
    let status = StatusSnapshot::idle(SetupSettings::default());
    state.shared.replace(status.clone());
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_intercepts_the_game_domain() {
        assert!(is_game_host("summonerswar-eu-lb.qpyou.cn"));
        assert!(is_game_host("qpyou.cn"));
        assert!(!is_game_host("example.com"));
        assert!(!is_game_host("qpyou.cn.example.com"));
    }
}

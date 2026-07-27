use crate::{
    certificate::{
        certificate_der_path, ensure_certificate, mobileconfig_profile, regenerate_certificate,
    },
    export::write_profile,
    protocol::decode_profile,
    setup::{self, SetupSettings},
    steam::SteamRouteState,
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
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Manager, State};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "windows")]
use hudsucker::{
    hyper_util::client::legacy::connect::{HttpConnector, dns::Name},
    tokio_tungstenite::Connector,
};
#[cfg(target_os = "windows")]
use hyper_rustls::{ConfigBuilderExt, HttpsConnector, HttpsConnectorBuilder};
#[cfg(target_os = "windows")]
use std::{
    collections::HashMap,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};
#[cfg(target_os = "windows")]
use tower_service::Service;

const PREFERRED_PROXY_PORT: u16 = 8080;
const PROFILE_PATH: &str = "/api/gateway_c2.php";
const CERTIFICATE_PROFILE_PATH: &str = "/SwagEx.mobileconfig";

#[cfg(target_os = "windows")]
#[derive(Clone, Default)]
struct UpstreamResolver {
    overrides: Arc<HashMap<String, IpAddr>>,
}

#[cfg(target_os = "windows")]
impl UpstreamResolver {
    fn new(overrides: HashMap<String, IpAddr>) -> Self {
        Self {
            overrides: Arc::new(overrides),
        }
    }
}

#[cfg(target_os = "windows")]
impl Service<Name> for UpstreamResolver {
    type Response = std::vec::IntoIter<SocketAddr>;
    type Error = std::io::Error;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, name: Name) -> Self::Future {
        let hostname = name.as_str().to_string();
        let overridden = self.overrides.get(&hostname).copied();
        Box::pin(async move {
            if let Some(address) = overridden {
                return Ok(vec![SocketAddr::new(address, 0)].into_iter());
            }

            let addresses = tokio::net::lookup_host((hostname.as_str(), 0))
                .await?
                .collect::<Vec<_>>();
            Ok(addresses.into_iter())
        })
    }
}

#[cfg(target_os = "windows")]
fn upstream_connector(
    overrides: HashMap<String, IpAddr>,
) -> anyhow::Result<(HttpsConnector<HttpConnector<UpstreamResolver>>, Connector)> {
    let rustls_config = hudsucker::rustls::ClientConfig::builder_with_provider(Arc::new(
        aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_webpki_roots()
    .with_no_client_auth();
    let websocket_connector = Connector::Rustls(Arc::new(rustls_config.clone()));
    let mut http_connector = HttpConnector::new_with_resolver(UpstreamResolver::new(overrides));
    http_connector.enforce_http(false);
    let connector = HttpsConnectorBuilder::new()
        .with_tls_config(rustls_config)
        .https_or_http()
        .enable_http1()
        .wrap_connector(http_connector);

    Ok((connector, websocket_connector))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusSnapshot {
    pub phase: String,
    pub message: String,
    pub certificate_setup_completed: bool,
    pub windows_certificate_setup_completed: bool,
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
            message: "Préparez le proxy Wi‑Fi de votre appareil Apple pour commencer.".into(),
            certificate_setup_completed: settings.certificate_setup_completed,
            windows_certificate_setup_completed: settings.windows_certificate_setup_completed,
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
    steam_route: Arc<Mutex<SteamRouteState>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            shared: Arc::new(SharedState {
                status: Mutex::new(StatusSnapshot::idle(SetupSettings::default())),
            }),
            cancel: Mutex::new(None),
            steam_route: Arc::new(Mutex::new(SteamRouteState::default())),
        }
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.lock().ok().and_then(|mut value| value.take()) {
            cancel.cancel();
        }
        if let Ok(mut steam_route) = self.steam_route.lock() {
            let _ = steam_route.stop();
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
        if request.method() == Method::GET
            && matches!(
                request.uri().path(),
                CERTIFICATE_PROFILE_PATH | "/certificate"
            )
        {
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

/// Keeps captured exports in SwagEx's private application-data directory.
/// Writing directly to Downloads would require an operating-system permission
/// before the capture confirmation can be displayed.
fn exports_directory(app_data: &Path) -> PathBuf {
    app_data.join("exports")
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
    state
        .steam_route
        .lock()
        .map_err(|_| anyhow::anyhow!("état Steam indisponible"))?
        .stop()?;
    Ok(())
}

async fn start_proxy(
    app: AppHandle,
    state: &AppState,
    capture_enabled: bool,
    phase: &str,
    force_regenerate_certificate: bool,
    steam_mode: bool,
) -> anyhow::Result<StatusSnapshot> {
    cancel_running_proxy(state)?;
    // Let the previous listener release port 8080 before binding the next
    // setup/capture phase. This matters when moving from certificate setup to
    // proxy setup in the same button action.
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    let app_data = app_data_directory(&app)?;
    let output_directory = exports_directory(&app_data);
    let local_ip = if steam_mode {
        Ipv4Addr::LOCALHOST
    } else {
        local_ipv4()?
    };
    let settings = configured(&app)?;
    let certificate_directory = app_data.join("certificate");
    if !force_regenerate_certificate
        && (settings.certificate_setup_completed || settings.windows_certificate_setup_completed)
        && !certificate_directory.exists()
    {
        anyhow::bail!(
            "Le certificat SwagEx installé précédemment est introuvable. Aucune nouvelle autorité n’a été créée."
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
    #[cfg(target_os = "windows")]
    let proxy_address = listener.local_addr()?;
    let certificate_url =
        (!steam_mode).then(|| format!("http://{local_ip}:{port}{CERTIFICATE_PROFILE_PATH}"));
    let cancel = CancellationToken::new();

    #[cfg(target_os = "windows")]
    let prepared_steam_route = if steam_mode {
        Some(crate::steam::prepare().await?)
    } else {
        None
    };
    #[cfg(not(target_os = "windows"))]
    if steam_mode {
        anyhow::bail!("Le parcours Steam est disponible uniquement sous Windows.");
    }

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

    if steam_mode {
        #[cfg(target_os = "windows")]
        {
            let prepared = prepared_steam_route
                .ok_or_else(|| anyhow::anyhow!("préparation Steam indisponible"))?;
            let (connector, websocket_connector) = upstream_connector(prepared.upstream_hosts())?;
            let proxy = Proxy::builder()
                .with_listener(listener)
                .with_ca(authority)
                .with_http_connector(connector)
                .with_websocket_connector(websocket_connector)
                .with_http_handler(handler)
                .with_graceful_shutdown(cancel.clone().cancelled_owned())
                .build()?;
            let mut steam_route = state
                .steam_route
                .lock()
                .map_err(|_| anyhow::anyhow!("état Steam indisponible"))?;
            crate::steam::activate(&mut steam_route, prepared, proxy_address, cancel.clone())?;

            let shared = Arc::clone(&state.shared);
            let proxy_cancel = cancel.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = proxy.start().await {
                    shared.record_proxy_error(error);
                    proxy_cancel.cancel();
                }
            });
        }
    } else {
        let proxy = Proxy::builder()
            .with_listener(listener)
            .with_ca(authority)
            .with_rustls_connector(aws_lc_rs::default_provider())
            .with_http_handler(handler)
            .with_graceful_shutdown(cancel.clone().cancelled_owned())
            .build()?;
        let shared = Arc::clone(&state.shared);
        let proxy_cancel = cancel.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = proxy.start().await {
                shared.record_proxy_error(error);
                proxy_cancel.cancel();
            }
        });
    }

    let status = StatusSnapshot {
        phase: phase.into(),
        message: match phase {
            "certificate_setup" => {
                "Installez puis activez le certificat SwagEx sur votre appareil Apple.".into()
            }
            "proxy_setup" => {
                "Saisissez ces valeurs dans le proxy Wi‑Fi de votre appareil Apple.".into()
            }
            "listening" if steam_mode => "Lancez Summoners War depuis Steam.".into(),
            "listening" => "Ouvrez Summoners War sur l’appareil Apple configuré.".into(),
            _ => "".into(),
        },
        certificate_setup_completed: settings.certificate_setup_completed,
        windows_certificate_setup_completed: settings.windows_certificate_setup_completed,
        proxy_setup_completed: settings.proxy_setup_completed,
        local_ip: Some(local_ip.to_string()),
        port: Some(port),
        certificate_url,
        certificate_was_created,
        export_path: None,
        profile_name: None,
    };
    state.shared.replace(status.clone());
    *state
        .cancel
        .lock()
        .map_err(|_| anyhow::anyhow!("état interne indisponible"))? = Some(cancel);

    #[cfg(target_os = "windows")]
    if steam_mode {
        let cleanup_cancel = state
            .cancel
            .lock()
            .map_err(|_| anyhow::anyhow!("état interne indisponible"))?
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("arrêt Steam indisponible"))?
            .clone();
        let steam_route = Arc::clone(&state.steam_route);
        tauri::async_runtime::spawn(async move {
            cleanup_cancel.cancelled().await;
            if let Ok(mut route) = steam_route.lock() {
                let _ = route.stop();
            }
        });
    }

    Ok(status)
}

#[tauri::command]
pub fn export_status(app: AppHandle, state: State<'_, AppState>) -> Result<StatusSnapshot, String> {
    let settings = configured(&app).map_err(|error| error.to_string())?;
    let mut status = state.shared.snapshot();
    status.certificate_setup_completed = settings.certificate_setup_completed;
    status.windows_certificate_setup_completed = settings.windows_certificate_setup_completed;
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
                windows_certificate_setup_completed: false,
                proxy_setup_completed: false,
            },
        )
        .map_err(|error| error.to_string())?;
    }
    start_proxy(app, &state, false, "certificate_setup", regenerate, false)
        .await
        .map_err(|error| {
            let message = error.to_string();
            let settings = state.shared.snapshot();
            state.shared.replace(StatusSnapshot {
                phase: "error".into(),
                message: message.clone(),
                ..StatusSnapshot::idle(SetupSettings {
                    certificate_setup_completed: settings.certificate_setup_completed,
                    windows_certificate_setup_completed: settings
                        .windows_certificate_setup_completed,
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
        return Err("Installez d’abord le certificat SwagEx sur votre appareil Apple.".into());
    }

    start_proxy(app, &state, false, "proxy_setup", false, false)
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
            windows_certificate_setup_completed: current.windows_certificate_setup_completed,
            proxy_setup_completed: current.proxy_setup_completed,
        },
    )
    .map_err(|error| error.to_string())?;
    start_proxy(app, &state, false, "proxy_setup", false, false)
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
        return Err("Installez d’abord le certificat SwagEx sur votre appareil Apple.".into());
    }
    setup::write(
        &app_data,
        SetupSettings {
            certificate_setup_completed: true,
            windows_certificate_setup_completed: current.windows_certificate_setup_completed,
            proxy_setup_completed: true,
        },
    )
    .map_err(|error| error.to_string())?;
    start_proxy(app, &state, true, "listening", false, false)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn start_windows_certificate_setup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StatusSnapshot, String> {
    cancel_running_proxy(&state).map_err(|error| error.to_string())?;
    let app_data = app_data_directory(&app).map_err(|error| error.to_string())?;
    let settings = setup::read(&app_data).map_err(|error| error.to_string())?;
    let certificate_directory = app_data.join("certificate");
    if (settings.certificate_setup_completed || settings.windows_certificate_setup_completed)
        && !certificate_directory.exists()
    {
        return Err(
            "Le certificat SwagEx installé précédemment est introuvable. Aucune nouvelle autorité n’a été créée."
                .into(),
        );
    }
    let certificate =
        ensure_certificate(&certificate_directory).map_err(|error| error.to_string())?;
    let status = StatusSnapshot {
        phase: "windows_certificate_setup".into(),
        message: "Installez le certificat SwagEx dans Windows.".into(),
        certificate_setup_completed: settings.certificate_setup_completed,
        windows_certificate_setup_completed: settings.windows_certificate_setup_completed,
        proxy_setup_completed: settings.proxy_setup_completed,
        local_ip: None,
        port: None,
        certificate_url: None,
        certificate_was_created: certificate.was_created,
        export_path: None,
        profile_name: None,
    };
    state.shared.replace(status.clone());
    Ok(status)
}

#[tauri::command]
pub fn open_windows_certificate(app: AppHandle) -> Result<(), String> {
    let app_data = app_data_directory(&app).map_err(|error| error.to_string())?;
    let path = certificate_der_path(&app_data.join("certificate"));
    if !path.is_file() {
        return Err("Le certificat DER SwagEx est introuvable.".into());
    }

    open_certificate_in_native_windows_viewer(&path)
}

#[cfg(target_os = "windows")]
fn open_certificate_in_native_windows_viewer(path: &Path) -> Result<(), String> {
    use std::{iter, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{UI::Shell::ShellExecuteW, UI::WindowsAndMessaging::SW_SHOWNORMAL};

    let operation = std::ffi::OsStr::new("open")
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let file = path
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    if result as isize > 32 {
        Ok(())
    } else {
        Err(format!(
            "Windows n’a pas pu ouvrir le certificat DER (code {}).",
            result as isize
        ))
    }
}

#[cfg(not(target_os = "windows"))]
fn open_certificate_in_native_windows_viewer(_path: &Path) -> Result<(), String> {
    Err("L’installation du certificat Steam est disponible uniquement sous Windows.".into())
}

#[tauri::command]
pub async fn complete_windows_certificate_setup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StatusSnapshot, String> {
    let app_data = app_data_directory(&app).map_err(|error| error.to_string())?;
    let current = setup::read(&app_data).map_err(|error| error.to_string())?;
    setup::write(
        &app_data,
        SetupSettings {
            certificate_setup_completed: current.certificate_setup_completed,
            windows_certificate_setup_completed: true,
            proxy_setup_completed: current.proxy_setup_completed,
        },
    )
    .map_err(|error| error.to_string())?;
    start_proxy(app, &state, true, "listening", false, true)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn start_steam_capture(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StatusSnapshot, String> {
    let settings = configured(&app).map_err(|error| error.to_string())?;
    if !settings.windows_certificate_setup_completed {
        return Err("Installez d’abord le certificat SwagEx dans Windows.".into());
    }
    start_proxy(app, &state, true, "listening", false, true)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn cancel_steam_export(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StatusSnapshot, String> {
    cancel_running_proxy(&state).map_err(|error| error.to_string())?;
    let settings = configured(&app).map_err(|error| error.to_string())?;
    let status = StatusSnapshot::idle(settings);
    state.shared.replace(status.clone());
    Ok(status)
}

#[tauri::command]
pub fn stop_steam_capture(state: State<'_, AppState>) -> Result<(), String> {
    cancel_running_proxy(&state).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn cancel_export(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StatusSnapshot, String> {
    cancel_running_proxy(&state).map_err(|error| error.to_string())?;
    let settings = configured(&app).map_err(|error| error.to_string())?;
    if settings.certificate_setup_completed {
        start_proxy(app, &state, false, "proxy_setup", false, false)
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

    #[test]
    fn exports_stay_in_the_app_private_directory() {
        let app_data = Path::new("/tmp/swagex-app-data");

        assert_eq!(exports_directory(app_data), app_data.join("exports"),);
    }
}

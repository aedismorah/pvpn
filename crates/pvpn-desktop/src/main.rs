#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::collections::VecDeque;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use pvpn_client::pac::{self, PacState};
use pvpn_core::{ClientConfig, Psk};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

const BG: egui::Color32 = egui::Color32::from_rgb(7, 14, 20);
const CARD: egui::Color32 = egui::Color32::from_rgb(15, 24, 30);
const BORDER: egui::Color32 = egui::Color32::from_rgb(34, 52, 58);
const MINT: egui::Color32 = egui::Color32::from_rgb(139, 255, 209);
const AQUA: egui::Color32 = egui::Color32::from_rgb(61, 232, 196);
const MUTED: egui::Color32 = egui::Color32::from_rgb(139, 162, 158);
const RED: egui::Color32 = egui::Color32::from_rgb(255, 126, 126);

const DEFAULT_DOMAINS: &[&str] = &[
    "chatgpt.com",
    "openai.com",
    "oaistatic.com",
    "oaiusercontent.com",
    "youtube.com",
    "youtu.be",
    "youtube-nocookie.com",
    "googlevideo.com",
    "ytimg.com",
    "ggpht.com",
    "googleusercontent.com",
    "youtubei.googleapis.com",
    "gstatic.com",
    "discord.com",
    "discord.gg",
    "telegram.org",
    "claude.ai",
    "anthropic.com",
    "github.com",
    "x.com",
    "twitter.com",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UiConfig {
    server: String,
    psk: String,
    socks_listen: String,
    http_listen: String,
    pac_listen: String,
    domains: Vec<String>,
    #[serde(default)]
    disabled_domains: Vec<String>,
    #[serde(default)]
    blacklist: bool,
    #[serde(default)]
    proxy_all: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            server: "87.242.102.100:9331".into(),
            psk: String::new(),
            socks_listen: "127.0.0.1:11080".into(),
            http_listen: "127.0.0.1:11081".into(),
            pac_listen: "127.0.0.1:7892".into(),
            domains: DEFAULT_DOMAINS.iter().map(|s| s.to_string()).collect(),
            disabled_domains: Vec::new(),
            blacklist: false,
            proxy_all: false,
        }
    }
}

impl UiConfig {
    fn path() -> PathBuf {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("pvpn").join("ui.toml")
    }

    fn load() -> Self {
        let path = Self::path();
        let mut config: Self = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default();
        if let Some((host, port)) = config.server.rsplit_once(':') {
            if host == "87.242.102.100" && (port == "443" || port == "9443") {
                config.server = format!("{host}:9331");
            }
        }
        for domain in [
            "youtube-nocookie.com",
            "ggpht.com",
            "googleusercontent.com",
            "youtubei.googleapis.com",
            "gstatic.com",
        ] {
            if !config.domains.iter().any(|existing| existing == domain) {
                config.domains.push(domain.to_string());
            }
        }
        config
    }

    fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create config dir: {e}"))?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| format!("serialize config: {e}"))?;
        std::fs::write(path, text).map_err(|e| format!("write config: {e}"))
    }
}

struct Tunnel {
    shutdown: watch::Sender<bool>,
    handle: std::thread::JoinHandle<()>,
}

struct PacRuntime {
    state: PacState,
    shutdown: watch::Sender<bool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

struct PvpnApp {
    config: UiConfig,
    psk: String,
    new_domain: String,
    tunnel: Option<Tunnel>,
    pac: PacRuntime,
    logo: Option<egui::TextureHandle>,
    tunnel_error: Arc<Mutex<Option<String>>>,
    pac_error: Arc<Mutex<Option<String>>>,
    logs: VecDeque<String>,
    proxy_on: bool,
    pac_ok: bool,
    show_psk: bool,
    status: String,
    error: String,
    screen: Screen,
    mode_anim: f32,
    test_rx: Option<mpsc::Receiver<TestResult>>,
    test_busy: bool,
    ping_text: String,
    speed_text: String,
    last_proxy_repair: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TestKind {
    Ping,
    Speed,
}

struct TestResult {
    kind: TestKind,
    text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Main,
    SplitTunneling,
    Log,
}

impl PvpnApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG;
        visuals.window_fill = CARD;
        visuals.faint_bg_color = CARD;
        visuals.extreme_bg_color = egui::Color32::from_rgb(8, 17, 22);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(20, 32, 38);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(29, 48, 53);
        visuals.selection.bg_fill = egui::Color32::from_rgb(24, 92, 76);
        cc.egui_ctx.set_visuals(visuals);

        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        cc.egui_ctx.set_style(style);

        let config = UiConfig::load();
        let psk = config.psk.clone();
        let logo = eframe::icon_data::from_png_bytes(include_bytes!(
            "../../../assets/brand/desktop/pvpn-128.png"
        ))
        .ok()
        .map(|icon| {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [icon.width as usize, icon.height as usize],
                &icon.rgba,
            );
            cc.egui_ctx
                .load_texture("pvpn-logo", image, egui::TextureOptions::LINEAR)
        });

        let state = PacState::new(proxy_addr(&config.http_listen));
        let pac_error = Arc::new(Mutex::new(None));
        let (shutdown, rx) = watch::channel(false);
        let listen = config.pac_listen.clone();
        let pac_state = state.clone();
        let thread_error = pac_error.clone();
        let handle = std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    *thread_error.lock().unwrap() = Some(format!("runtime: {error}"));
                    return;
                }
            };
            if let Err(error) = runtime.block_on(pac::serve(listen, pac_state, rx)) {
                *thread_error.lock().unwrap() = Some(error.to_string());
            }
        });

        let mut logs = VecDeque::new();
        logs.push_back(format!(
            "{}  UI started; PAC on {}",
            clock(),
            config.pac_listen
        ));
        Self {
            config,
            psk,
            new_domain: String::new(),
            tunnel: None,
            pac: PacRuntime {
                state,
                shutdown,
                handle: Some(handle),
            },
            logo,
            tunnel_error: Arc::new(Mutex::new(None)),
            pac_error,
            logs,
            proxy_on: false,
            pac_ok: true,
            show_psk: false,
            status: "Stopped".into(),
            error: String::new(),
            screen: Screen::Main,
            mode_anim: 0.0,
            test_rx: None,
            test_busy: false,
            ping_text: String::new(),
            speed_text: String::new(),
            last_proxy_repair: Instant::now(),
        }
    }

    fn log(&mut self, message: impl Into<String>) {
        self.logs
            .push_back(format!("{}  {}", clock(), message.into()));
        while self.logs.len() > 200 {
            self.logs.pop_front();
        }
    }

    fn fail(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.error = message.clone();
        self.log(format!("ERROR: {message}"));
    }

    fn clear_error(&mut self) {
        self.error.clear();
    }

    fn persist(&mut self) {
        if let Err(error) = self.config.save() {
            self.fail(format!("Could not save settings: {error}"));
        }
    }

    fn validate(&mut self) -> bool {
        if Psk::from_hex(self.psk.trim()).is_err() {
            self.fail("PSK must be exactly 64 hex characters");
            return false;
        }
        if !valid_host_port(&self.config.server) {
            self.fail("Ingress must be host:port (for example 87.242.102.100:9331)");
            return false;
        }
        let endpoints = [
            ("SOCKS5", self.config.socks_listen.clone()),
            ("HTTP proxy", self.config.http_listen.clone()),
            ("PAC", self.config.pac_listen.clone()),
        ];
        for (label, value) in endpoints {
            if value.parse::<SocketAddr>().is_err() {
                self.fail(format!("{label} must be an IP:port, got {value}"));
                return false;
            }
        }
        if self.config.socks_listen == self.config.http_listen
            || self.config.socks_listen == self.config.pac_listen
            || self.config.http_listen == self.config.pac_listen
        {
            self.fail("SOCKS5, HTTP proxy and PAC ports must be different");
            return false;
        }
        true
    }

    fn start_tunnel(&mut self) {
        if self.tunnel.is_some() {
            return;
        }
        self.clear_error();
        if !self.validate() {
            return;
        }
        let psk = match Psk::from_hex(self.psk.trim()) {
            Ok(psk) => psk,
            Err(error) => {
                self.fail(format!("Invalid PSK: {error}"));
                return;
            }
        };
        let config = ClientConfig {
            listen: self.config.socks_listen.clone(),
            server: self.config.server.clone(),
            psk,
            connect_timeout_secs: 15,
            http_listen: Some(self.config.http_listen.clone()),
        };
        let socks = config.listen.clone();
        let http = self.config.http_listen.clone();
        let (shutdown, rx) = watch::channel(false);
        let tunnel_error = Arc::new(Mutex::new(None));
        let thread_error = tunnel_error.clone();
        let handle = std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    *thread_error.lock().unwrap() = Some(format!("runtime: {error}"));
                    return;
                }
            };
            if let Err(error) = runtime.block_on(pvpn_client::client::run_with_listeners(
                config,
                socks,
                Some(http),
                rx,
            )) {
                *thread_error.lock().unwrap() = Some(error.to_string());
            }
        });
        self.tunnel = Some(Tunnel { shutdown, handle });
        self.tunnel_error = tunnel_error;
        self.status = "Starting...".into();
        self.log(format!(
            "Starting tunnel to {}; SOCKS {}; HTTP {}",
            self.config.server, self.config.socks_listen, self.config.http_listen
        ));
        self.wait_ready();
    }

    fn wait_ready(&mut self) {
        let socks_ok = wait_listen(&self.config.socks_listen, 20);
        let http_ok = wait_listen(&self.config.http_listen, 20);
        if socks_ok && http_ok {
            self.status = "Tunnel running".into();
            self.log("Tunnel is ready");
            self.persist();
        } else {
            let detail = take_error(&self.tunnel_error).unwrap_or_else(|| {
                "listeners did not open; check whether the ports are already in use".into()
            });
            self.stop_tunnel();
            self.fail(format!("Tunnel failed: {detail}"));
        }
    }

    fn stop_tunnel(&mut self) {
        if self.proxy_on {
            if let Err(error) = clear_pac() {
                self.fail(format!("Could not disable system proxy: {error}"));
            }
            self.proxy_on = false;
        }
        if let Some(tunnel) = self.tunnel.take() {
            let _ = tunnel.shutdown.send(true);
            let _ = tunnel.handle.join();
            self.log("Tunnel stopped");
        }
        self.tunnel_error = Arc::new(Mutex::new(None));
        self.status = "Stopped".into();
    }

    fn enable_proxy(&mut self) {
        if self.tunnel.is_none() {
            self.fail("Start the tunnel first");
            return;
        }
        if !wait_listen(&self.config.socks_listen, 2)
            || !wait_listen(&self.config.http_listen, 2)
        {
            self.fail("Tunnel ports are not listening anymore");
            return;
        }
        if !wait_listen(&self.config.pac_listen, 5) {
            let detail = take_error(&self.pac_error)
                .unwrap_or_else(|| "PAC server is not listening".into());
            self.fail(format!("PAC server unavailable: {detail}"));
            return;
        }
        self.pac.state.set_proxy(proxy_addr(&self.config.http_listen));
        self.pac.state.set_domains(self.active_domains());
        self.pac.state.set_blacklist(self.config.blacklist);
        self.pac.state.set_proxy_all(self.config.proxy_all);
        let proxy = proxy_addr(&self.config.http_listen);
        if let Err(error) = capture_proxy_backup() {
            self.fail(format!("Could not save the existing proxy settings: {error}"));
            return;
        }
        match set_pac_url(&self.pac_url(), &proxy) {
            Ok(()) => {
                self.proxy_on = true;
                self.status = "System proxy enabled".into();
                self.clear_error();
                self.log("System proxy enabled (PAC + WinHTTP fallback)");
                self.persist();
            }
            Err(error) => self.fail(format!("Could not enable system proxy: {error}")),
        }
    }

    fn disable_proxy(&mut self) {
        if !self.proxy_on {
            return;
        }
        match clear_pac() {
            Ok(()) => {
                self.proxy_on = false;
                self.status = "System proxy disabled".into();
                self.log("System proxy disabled");
            }
            Err(error) => self.fail(format!("Could not disable system proxy: {error}")),
        }
    }

    fn repair_static_proxy(&mut self) {
        if self.tunnel.is_none() {
            return;
        }
        if self.last_proxy_repair.elapsed() < Duration::from_secs(10) {
            return;
        }
        self.last_proxy_repair = Instant::now();
        let pac_configured = pac_configured().unwrap_or(false);
        if !self.proxy_on && !pac_configured {
            return;
        }
        if !self.proxy_on {
            self.proxy_on = true;
            self.status = "System proxy enabled".into();
            self.log("Recovered active system proxy");
        }
        let proxy = proxy_addr(&self.config.http_listen);
        let _ = set_pac_url(&self.pac_url(), &proxy);
    }

    fn pac_url(&self) -> String {
        let version = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let port = self
            .config
            .pac_listen
            .rsplit_once(':')
            .map(|(_, p)| p)
            .unwrap_or("7892");
        format!("http://127.0.0.1:{port}/proxy.pac?v={version}")
    }

    fn apply_pac(&mut self) {
        self.pac.state.set_proxy(proxy_addr(&self.config.http_listen));
        self.pac.state.set_domains(self.active_domains());
        self.pac.state.set_blacklist(self.config.blacklist);
        self.pac.state.set_proxy_all(self.config.proxy_all);
        if self.proxy_on {
            let proxy = proxy_addr(&self.config.http_listen);
            if let Err(error) = set_pac_url(&self.pac_url(), &proxy) {
                self.fail(format!("Could not refresh PAC: {error}"));
            }
        }
    }

    fn active_domains(&self) -> Vec<String> {
        self.config
            .domains
            .iter()
            .filter(|domain| !self.config.disabled_domains.contains(domain))
            .cloned()
            .collect()
    }

    fn add_domain(&mut self) {
        let domain = self.new_domain.trim().to_lowercase();
        if domain.is_empty() {
            return;
        }
        if !domain.contains('.') || domain.contains('/') || domain.contains(' ') {
            self.fail("Enter a domain like example.com");
            return;
        }
        if !self.config.domains.iter().any(|d| d == &domain) {
            self.config.domains.push(domain.clone());
            self.config.disabled_domains.retain(|d| d != &domain);
            self.new_domain.clear();
            self.apply_pac();
            self.persist();
            self.log(format!("Added split domain {domain}"));
        }
    }

    fn remove_domain(&mut self, index: usize) {
        if index >= self.config.domains.len() {
            return;
        }
        let removed = self.config.domains.remove(index);
        self.config.disabled_domains.retain(|d| d != &removed);
        self.apply_pac();
        self.persist();
        self.log(format!("Removed split domain {removed}"));
    }

    fn set_domain_enabled(&mut self, domain: &str, enabled: bool) {
        if enabled {
            self.config.disabled_domains.retain(|d| d != domain);
        } else if !self.config.disabled_domains.iter().any(|d| d == domain) {
            self.config.disabled_domains.push(domain.to_string());
        }
        self.apply_pac();
        self.persist();
        self.log(format!(
            "Split domain {domain} {}",
            if enabled { "enabled" } else { "disabled" }
        ));
    }

    fn poll_tests(&mut self, ctx: &egui::Context) {
        let outcome = self
            .test_rx
            .as_ref()
            .map(|receiver| receiver.try_recv());
        match outcome {
            Some(Ok(result)) => {
                match result.kind {
                    TestKind::Ping => self.ping_text = result.text.clone(),
                    TestKind::Speed => self.speed_text = result.text.clone(),
                }
                self.log(result.text);
                self.test_busy = false;
                self.test_rx = None;
                ctx.request_repaint();
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.test_busy = false;
                self.test_rx = None;
                self.ping_text = "Test worker stopped unexpectedly".into();
                self.speed_text = "Test worker stopped unexpectedly".into();
                ctx.request_repaint();
            }
            Some(Err(mpsc::TryRecvError::Empty)) => {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            None => {}
        }
    }

    fn start_test(&mut self, kind: TestKind) {
        if self.test_busy {
            return;
        }
        self.test_busy = true;
        self.test_rx = None;
        match kind {
            TestKind::Ping => {
                self.ping_text = "Pinging ingress and tunnel...".into();
            }
            TestKind::Speed => {
                self.speed_text = "Downloading 10 MB through the tunnel...".into();
            }
        }
        let server = self.config.server.clone();
        let http_proxy = self.config.http_listen.clone();
        let tunnel_on = self.tunnel.is_some();
        let (sender, receiver) = mpsc::channel();
        self.test_rx = Some(receiver);
        std::thread::spawn(move || {
            let text = match kind {
                TestKind::Ping => run_ping_test(&server, &http_proxy, tunnel_on),
                TestKind::Speed => run_speed_test(&http_proxy, tunnel_on),
            };
            let _ = sender.send(TestResult { kind, text });
        });
    }

    fn poll_background_errors(&mut self) {
        if let Some(error) = take_error(&self.pac_error) {
            self.pac_ok = false;
            self.fail(format!("PAC server stopped: {error}"));
        }
        if self.tunnel.is_some() {
            if let Some(error) = take_error(&self.tunnel_error) {
                self.stop_tunnel();
                self.fail(format!("Tunnel stopped: {error}"));
            }
        }
    }

    fn draw_split(&mut self, ui: &mut egui::Ui) {
        ui.visuals_mut().override_text_color = Some(egui::Color32::WHITE);
        ui.visuals_mut().widgets.inactive.fg_stroke.color = egui::Color32::WHITE;
        ui.visuals_mut().widgets.hovered.fg_stroke.color = egui::Color32::WHITE;
        ui.visuals_mut().widgets.active.fg_stroke.color = egui::Color32::WHITE;

        ui.label(
            egui::RichText::new(
                "Choose which domains use the HTTP proxy. Everything else goes direct.",
            )
            .size(12.0)
            .color(MUTED),
        );
        ui.add_space(8.0);

        card(ui, "Mode", |ui| {
            ui.label(
                egui::RichText::new("Use the domain list as:")
                    .size(12.0)
                    .color(egui::Color32::WHITE),
            );
            let mut changed = false;
            changed |= list_mode_switch(
                ui,
                &mut self.config.blacklist,
                &mut self.mode_anim,
            );
            ui.label(
                egui::RichText::new(if self.config.blacklist {
                    "Black list: listed domains go direct, everything else uses the proxy."
                } else {
                    "White list: listed domains use the proxy, everything else goes direct."
                })
                .size(12.0)
                .color(egui::Color32::WHITE),
            );
            if changed {
                self.apply_pac();
                self.persist();
                self.log(format!(
                    "Split list mode changed to {}",
                    if self.config.blacklist {
                        "black list"
                    } else {
                        "white list"
                    }
                ));
            }
        });

        ui.add_space(10.0);
        card(ui, "Domains", |ui| {
            ui.horizontal(|ui| {
                ui.add_sized(
                    egui::vec2(460.0, 36.0),
                    egui::TextEdit::singleline(&mut self.new_domain)
                        .font(egui::FontId::proportional(20.0))
                        .hint_text(
                            egui::RichText::new("example.com")
                                .color(egui::Color32::WHITE),
                        )
                        .text_color(egui::Color32::WHITE),
                );
                if ui
                    .add_sized(
                        egui::vec2(130.0, 34.0),
                        egui::Button::new(
                            egui::RichText::new("Add domain")
                                .strong()
                                .color(egui::Color32::WHITE),
                        ),
                    )
                    .clicked()
                {
                    self.add_domain();
                }
                let active = self.active_domains().len();
                ui.label(
                    egui::RichText::new(format!(
                        "{} rules, {} active",
                        self.config.domains.len(),
                        active
                    ))
                    .size(12.0)
                    .color(egui::Color32::WHITE),
                );
            });

            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("× removes a domain. The square toggle enables/disables its routing.")
                    .size(11.0)
                    .color(egui::Color32::WHITE),
            );

            let mut remove = None;
            let mut toggle = None;
            for (index, domain) in self.config.domains.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("×")
                                    .size(18.0)
                                    .color(RED),
                            )
                            .frame(false)
                            .min_size(egui::vec2(24.0, 24.0)),
                        )
                        .clicked()
                    {
                        remove = Some(index);
                    }
                    ui.label(
                        egui::RichText::new(domain)
                            .monospace()
                            .size(14.0)
                            .color(egui::Color32::WHITE),
                    );
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            let mut enabled =
                                !self.config.disabled_domains.contains(domain);
                            if ui
                                .checkbox(&mut enabled, "")
                                .on_hover_text("Enable or disable this domain")
                                .changed()
                            {
                                toggle = Some((domain.clone(), enabled));
                            }
                        },
                    );
                });
            }
            if let Some(index) = remove {
                self.remove_domain(index);
            } else if let Some((domain, enabled)) = toggle {
                self.set_domain_enabled(&domain, enabled);
            }
        });

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Changes apply immediately and are saved.")
                .size(11.0)
                .color(MUTED),
        );
    }

    fn draw_log(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{} events", self.logs.len()))
                    .size(11.0)
                    .color(MUTED),
            );
            if ui.button("Copy log").clicked() {
                let text = self.logs.iter().cloned().collect::<Vec<_>>().join("\n");
                ui.ctx().copy_text(text);
            }
            if ui.button("Clear").clicked() {
                self.logs.clear();
            }
        });
        ui.add_space(6.0);
        if self.logs.is_empty() {
            ui.label(
                egui::RichText::new("No events yet.")
                    .size(11.0)
                    .color(MUTED),
            );
        }
        for line in &self.logs {
            ui.label(
                egui::RichText::new(line)
                    .monospace()
                    .size(10.5)
                    .color(egui::Color32::from_rgb(170, 198, 192)),
            );
        }
    }
}

impl eframe::App for PvpnApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_background_errors();
        self.poll_tests(ctx);
        self.repair_static_proxy();
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.screen = Screen::Main;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F2)) {
            self.screen = Screen::SplitTunneling;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F3)) {
            self.screen = Screen::Log;
        }
        let running = self.tunnel.is_some();
        let pac_label = format!("PAC {}", self.config.pac_listen);

        egui::TopBottomPanel::top("pvpn-nav")
            .frame(
                egui::Frame::NONE
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(22, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    match self.screen {
                        Screen::Main => {
                            ui.label(
                                egui::RichText::new("pvpn")
                                    .size(13.0)
                                    .strong()
                                    .color(MUTED),
                            );
                        }
                        Screen::SplitTunneling => {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("< Back")
                                            .strong()
                                            .color(egui::Color32::WHITE),
                                    )
                                    .fill(egui::Color32::from_rgb(0, 92, 82))
                                    .min_size(egui::vec2(110.0, 36.0)),
                                )
                                .clicked()
                            {
                                self.screen = Screen::Main;
                            }
                            ui.label(
                                egui::RichText::new("Split tunneling")
                                    .strong()
                                    .color(egui::Color32::from_rgb(232, 255, 248)),
                            );
                        }
                        Screen::Log => {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("< Back")
                                            .strong()
                                            .color(egui::Color32::WHITE),
                                    )
                                    .fill(egui::Color32::from_rgb(0, 92, 82))
                                    .min_size(egui::vec2(110.0, 36.0)),
                                )
                                .clicked()
                            {
                                self.screen = Screen::Main;
                            }
                            ui.label(
                                egui::RichText::new("Event log")
                                    .strong()
                                    .color(egui::Color32::from_rgb(232, 255, 248)),
                            );
                        }
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BG).inner_margin(egui::Margin::same(22)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.screen == Screen::Main {
                        ui.horizontal(|ui| {
                            if let Some(logo) = &self.logo {
                                ui.add(egui::Image::new((
                                    logo.id(),
                                    egui::vec2(87.0, 87.0),
                                )));
                            }
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new("pvpn")
                                        .size(31.0)
                                        .strong()
                                        .color(egui::Color32::from_rgb(232, 255, 248)),
                                );
                                ui.label(
                                    egui::RichText::new("PRIVATE TUNNEL")
                                        .size(11.0)
                                        .strong()
                                        .color(AQUA),
                                );
                                ui.label(
                                    egui::RichText::new("Cloud.ru -> VDSina")
                                        .size(12.0)
                                        .color(MUTED),
                                );
                            });
                        });
                        ui.add_space(12.0);

                        ui.horizontal(|ui| {
                            chip(
                                ui,
                                if running { "TUNNEL ON" } else { "TUNNEL OFF" },
                                running,
                                MINT,
                            );
                            chip(
                                ui,
                                if self.proxy_on {
                                    "SYSTEM PROXY ON"
                                } else {
                                    "SYSTEM PROXY OFF"
                                },
                                self.proxy_on,
                                AQUA,
                            );
                            chip(ui, &pac_label, self.pac_ok, AQUA);
                        });
                        ui.add_space(10.0);

                        if !self.error.is_empty() {
                            let error = self.error.clone();
                            egui::Frame::NONE
                                .fill(egui::Color32::from_rgb(48, 22, 24))
                                .stroke(egui::Stroke::new(
                                    1.0_f32,
                                    egui::Color32::from_rgb(110, 45, 48),
                                ))
                                .corner_radius(10)
                                .inner_margin(egui::Margin::same(12))
                                .show(ui, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(
                                            egui::RichText::new("ERROR")
                                                .strong()
                                                .color(RED),
                                        );
                                        ui.label(
                                            egui::RichText::new(&error).color(
                                                egui::Color32::from_rgb(255, 190, 190),
                                            ),
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        if ui.button("Copy error").clicked() {
                                            ui.ctx().copy_text(error.clone());
                                        }
                                        if ui.button("Dismiss").clicked() {
                                            self.error.clear();
                                        }
                                    });
                                });
                            ui.add_space(10.0);
                        }
                        }

                        if self.screen == Screen::SplitTunneling {
                            self.draw_split(ui);
                        } else if self.screen == Screen::Log {
                            self.draw_log(ui);
                        } else {
                        card(ui, "Connection", |ui| {
                            egui::Grid::new("connection_grid")
                                .num_columns(2)
                                .spacing([16.0, 10.0])
                                .show(ui, |ui| {
                                    ui.label("Ingress");
                                    ui.add_enabled(
                                        !running,
                                        egui::TextEdit::singleline(
                                            &mut self.config.server,
                                        )
                                        .desired_width(430.0),
                                    );
                                    ui.end_row();

                                    ui.label("PSK");
                                    ui.horizontal(|ui| {
                                        ui.add_enabled(
                                            !running,
                                            egui::TextEdit::singleline(&mut self.psk)
                                                .password(!self.show_psk)
                                                .desired_width(360.0),
                                        );
                                        ui.checkbox(&mut self.show_psk, "show");
                                    });
                                    ui.end_row();

                                    ui.label("SOCKS5");
                                    ui.add_enabled(
                                        !running,
                                        egui::TextEdit::singleline(
                                            &mut self.config.socks_listen,
                                        )
                                        .desired_width(430.0),
                                    );
                                    ui.end_row();

                                    ui.label("HTTP proxy");
                                    ui.add_enabled(
                                        !running,
                                        egui::TextEdit::singleline(
                                            &mut self.config.http_listen,
                                        )
                                        .desired_width(430.0),
                                    );
                                    ui.end_row();

                                    ui.label("PAC server");
                                    ui.add_enabled(
                                        !running,
                                        egui::TextEdit::singleline(
                                            &mut self.config.pac_listen,
                                        )
                                        .desired_width(430.0),
                                    );
                                    ui.end_row();
                                });

                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                let start = ui.add_enabled(
                                    !running,
                                    egui::Button::new(
                                        egui::RichText::new("Start tunnel").strong(),
                                    )
                                    .fill(egui::Color32::from_rgb(22, 92, 72))
                                    .min_size(egui::vec2(138.0, 34.0)),
                                );
                                if start.clicked() {
                                    self.start_tunnel();
                                }
                                let stop = ui.add_enabled(
                                    running,
                                    egui::Button::new("Stop tunnel")
                                        .min_size(egui::vec2(120.0, 34.0)),
                                );
                                if stop.clicked() {
                                    self.stop_tunnel();
                                }
                                ui.separator();
                                let enable = ui.add_enabled(
                                    running && !self.proxy_on,
                                    egui::Button::new("Enable system proxy")
                                        .min_size(egui::vec2(160.0, 34.0)),
                                );
                                if enable.clicked() {
                                    self.enable_proxy();
                                }
                                let disable = ui.add_enabled(
                                    self.proxy_on,
                                    egui::Button::new("Disable system proxy")
                                        .min_size(egui::vec2(160.0, 34.0)),
                                );
                                if disable.clicked() {
                                    self.disable_proxy();
                                }
                            });

                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&self.status)
                                        .color(if running { MINT } else { MUTED }),
                                );
                                if running {
                                    ui.label(
                                        egui::RichText::new(
                                            "| stop the tunnel to edit settings",
                                        )
                                        .size(11.0)
                                        .color(MUTED),
                                    );
                                }
                            });
                        });

                        card(ui, "Split tunneling", |ui| {
                            let mode = if self.config.proxy_all {
                                "All traffic via proxy"
                            } else if self.config.blacklist {
                                "Black list"
                            } else {
                                "White list"
                            };
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} | {} rules, {} active",
                                    mode,
                                    self.config.domains.len(),
                                    self.active_domains().len()
                                ))
                                .size(12.0)
                                .color(MUTED),
                            );
                            if self.config.proxy_all {
                                ui.label(
                                    egui::RichText::new(
                                        "Split tunneling is disabled; every domain uses the proxy.",
                                    )
                                    .size(12.0)
                                    .color(MUTED),
                                );
                            }
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Manage split tunneling")
                                            .strong()
                                            .color(egui::Color32::WHITE),
                                    )
                                    .fill(egui::Color32::from_rgb(22, 92, 72))
                                    .min_size(egui::vec2(190.0, 32.0)),
                                )
                                .clicked()
                            {
                                self.screen = Screen::SplitTunneling;
                            }
                            let disable_split = !self.config.proxy_all;
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(if disable_split {
                                            "Disable split tunneling"
                                        } else {
                                            "Enable split tunneling"
                                        })
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                    )
                                    .fill(if disable_split {
                                        egui::Color32::from_rgb(78, 42, 30)
                                    } else {
                                        egui::Color32::from_rgb(22, 92, 72)
                                    })
                                    .min_size(egui::vec2(190.0, 32.0)),
                                )
                                .clicked()
                            {
                                self.config.proxy_all = disable_split;
                                self.apply_pac();
                                self.persist();
                                self.log(if disable_split {
                                    "Split tunneling disabled; proxying all traffic"
                                } else {
                                    "Split tunneling enabled"
                                });
                            }
                            });
                        });

                        card(ui, "Connection test", |ui| {
                            let busy = self.test_busy;
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(
                                        !busy,
                                        egui::Button::new(
                                            egui::RichText::new("Ping")
                                                .strong()
                                                .color(egui::Color32::WHITE),
                                        )
                                        .fill(egui::Color32::from_rgb(18, 62, 64))
                                        .min_size(egui::vec2(100.0, 32.0)),
                                    )
                                    .clicked()
                                {
                                    self.start_test(TestKind::Ping);
                                }
                                if ui
                                    .add_enabled(
                                        !busy && running,
                                        egui::Button::new(
                                            egui::RichText::new("Speed test")
                                                .strong()
                                                .color(egui::Color32::WHITE),
                                        )
                                        .fill(egui::Color32::from_rgb(22, 92, 72))
                                        .min_size(egui::vec2(130.0, 32.0)),
                                    )
                                    .clicked()
                                {
                                    self.start_test(TestKind::Speed);
                                }
                                if busy {
                                    ui.spinner();
                                    ui.label(
                                        egui::RichText::new("testing...")
                                            .size(12.0)
                                            .color(MUTED),
                                    );
                                } else if !running {
                                    ui.label(
                                        egui::RichText::new(
                                            "Start tunnel for tunnel ping and speed test",
                                        )
                                        .size(11.0)
                                        .color(MUTED),
                                    );
                                }
                            });
                            if !self.ping_text.is_empty() {
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(&self.ping_text)
                                        .monospace()
                                        .size(12.0)
                                        .color(egui::Color32::WHITE),
                                );
                            }
                            if !self.speed_text.is_empty() {
                                ui.add_space(2.0);
                                ui.label(
                                    egui::RichText::new(&self.speed_text)
                                        .monospace()
                                        .size(12.0)
                                        .color(egui::Color32::WHITE),
                                );
                            }
                        });

                        card(ui, "Telegram Desktop", |ui| {
                            ui.label(
                                egui::RichText::new(
                                    "Telegram ignores the Windows PAC. Use its own SOCKS5 proxy:",
                                )
                                .size(12.0)
                                .color(MUTED),
                            );
                            let telegram = proxy_addr(&self.config.socks_listen);
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&telegram)
                                        .monospace()
                                        .strong()
                                        .color(MINT),
                                );
                                if ui.button("Copy").clicked() {
                                    ui.ctx().copy_text(telegram.clone());
                                }
                            });
                            ui.label(
                                egui::RichText::new(
                                    "Settings > Advanced > Connection type > Use custom proxy > SOCKS5.",
                                )
                                .size(11.0)
                                .color(MUTED),
                            );
                        });

                        card(ui, "Log", |ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} events",
                                    self.logs.len()
                                ))
                                .size(11.0)
                                .color(MUTED),
                            );
                            ui.horizontal(|ui| {
                                if ui.button("Open log").clicked() {
                                    self.screen = Screen::Log;
                                }
                                if ui.button("Copy log").clicked() {
                                    let text = self
                                        .logs
                                        .iter()
                                        .cloned()
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    ui.ctx().copy_text(text);
                                }
                            });
                        });

                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "pvpn {} | settings: {}",
                                env!("CARGO_PKG_VERSION"),
                                UiConfig::path().display()
                            ))
                            .size(10.0)
                            .color(egui::Color32::from_rgb(86, 112, 108)),
                        );
                        }
                    });
            });
    }
}


impl Drop for PvpnApp {
    fn drop(&mut self) {
        self.disable_proxy();
        if let Some(tunnel) = self.tunnel.take() {
            let _ = tunnel.shutdown.send(true);
            let _ = tunnel.handle.join();
        }
        let _ = self.pac.shutdown.send(true);
        if let Some(handle) = self.pac.handle.take() {
            let _ = handle.join();
        }
    }
}

fn proxy_addr(listen: &str) -> String {
    if let Some(port) = listen.rsplit_once(':').map(|(_, p)| p) {
        format!("127.0.0.1:{port}")
    } else {
        listen.to_string()
    }
}

fn clock() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() % 86_400)
        .unwrap_or(0);
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

fn take_error(slot: &Arc<Mutex<Option<String>>>) -> Option<String> {
    slot.lock().ok().and_then(|mut value| value.take())
}

fn valid_host_port(value: &str) -> bool {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix('[') {
        let Some((host, tail)) = rest.split_once(']') else {
            return false;
        };
        return !host.is_empty()
            && tail
                .strip_prefix(':')
                .and_then(|port| port.parse::<u16>().ok())
                .map(|port| port > 0)
                .unwrap_or(false);
    }
    value
        .rsplit_once(':')
        .map(|(host, port)| {
            !host.is_empty()
                && port
                    .parse::<u16>()
                    .map(|port| port > 0)
                    .unwrap_or(false)
        })
        .unwrap_or(false)
}

fn list_mode_switch(ui: &mut egui::Ui, blacklist: &mut bool, anim: &mut f32) -> bool {
    let target = if *blacklist { 1.0 } else { 0.0 };
    let dt = ui.input(|i| i.stable_dt).min(0.05);
    *anim += (target - *anim) * (dt * 11.0).min(1.0);
    if (target - *anim).abs() > 0.001 {
        ui.ctx().request_repaint();
    } else {
        *anim = target;
    }

    let width = ui.available_width().max(300.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, 56.0), egui::Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0_f32, egui::Color32::from_rgb(8, 17, 22));
    painter.rect_stroke(
        rect,
        6.0_f32,
        egui::Stroke::new(1.0_f32, BORDER),
        egui::StrokeKind::Inside,
    );

    let half = rect.width() / 2.0;
    let left = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(rect.min.x + half, rect.max.y),
    );
    let right = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + half, rect.min.y),
        rect.max,
    );
    let highlight_width = half - 6.0;
    let travel = rect.width() - highlight_width - 6.0;
    let highlight = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + 3.0 + travel * *anim, rect.min.y + 3.0),
        egui::vec2(highlight_width, rect.height() - 6.0),
    );
    painter.rect_filled(highlight, 5.0_f32, egui::Color32::from_rgb(22, 92, 72));

    let active_color = egui::Color32::WHITE;
    let inactive_color = egui::Color32::from_rgb(170, 198, 192);
    let white_color = if *blacklist {
        inactive_color
    } else {
        active_color
    };
    let black_color = if *blacklist {
        active_color
    } else {
        inactive_color
    };
    let font = egui::FontId::proportional(21.0);
    draw_bold_text(&painter, left.center(), "White list", font.clone(), white_color);
    draw_bold_text(&painter, right.center(), "Black list", font, black_color);

    if response.clicked() {
        *blacklist = !*blacklist;
        ui.ctx().request_repaint();
        return true;
    }
    false
}

fn draw_bold_text(
    painter: &egui::Painter,
    position: egui::Pos2,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
) {
    painter.text(
        position,
        egui::Align2::CENTER_CENTER,
        text,
        font.clone(),
        color,
    );
    painter.text(
        position + egui::vec2(0.8, 0.0),
        egui::Align2::CENTER_CENTER,
        text,
        font,
        color,
    );
}

fn run_ping_test(server: &str, http_proxy: &str, tunnel_on: bool) -> String {
    let ingress = match tcp_connect_ms(server) {
        Ok(ms) => format!("{ms} ms"),
        Err(error) => format!("failed ({error})"),
    };
    let tunnel = if tunnel_on {
        match http_connect_ms(http_proxy, "example.com:80") {
            Ok(ms) => format!("{ms} ms"),
            Err(error) => format!("failed ({error})"),
        }
    } else {
        "off".to_string()
    };
    format!("Ping  |  Ingress: {ingress}  |  Tunnel: {tunnel}")
}

fn tcp_connect_ms(host_port: &str) -> Result<u128, String> {
    let address = host_port
        .to_socket_addrs()
        .map_err(|error| format!("resolve: {error}"))?
        .next()
        .ok_or_else(|| "no address".to_string())?;
    let started = Instant::now();
    TcpStream::connect_timeout(&address, Duration::from_secs(4))
        .map_err(|error| format!("connect: {error}"))?;
    Ok(started.elapsed().as_millis())
}

fn http_connect_ms(proxy: &str, target: &str) -> Result<u128, String> {
    let address = proxy
        .to_socket_addrs()
        .map_err(|error| format!("proxy resolve: {error}"))?
        .next()
        .ok_or_else(|| "proxy has no address".to_string())?;
    let started = Instant::now();
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(4))
        .map_err(|error| format!("proxy connect: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    write!(
        stream,
        "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\nProxy-Connection: keep-alive\r\n\r\n"
    )
    .map_err(|error| format!("proxy write: {error}"))?;
    let mut response = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|error| format!("proxy read: {error}"))?;
        if read == 0 {
            return Err("proxy closed the connection".to_string());
        }
        response.extend_from_slice(&chunk[..read]);
        if response.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if response.len() > 16 * 1024 {
            return Err("proxy response too large".to_string());
        }
    }
    let head = String::from_utf8_lossy(&response);
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("?");
    if status != "200" {
        return Err(format!("proxy CONNECT returned {status}"));
    }
    Ok(started.elapsed().as_millis())
}

fn run_speed_test(http_proxy: &str, tunnel_on: bool) -> String {
    if !tunnel_on {
        return "Speed  |  start the tunnel first".to_string();
    }
    let urls = [
        "http://cachefly.cachefly.net/10mb.test",
        "http://ipv4.download.thinkbroadband.com/10MB.zip",
    ];
    let mut last_error = String::new();
    for url in urls {
        match download_http_via_proxy(http_proxy, url) {
            Ok((bytes, elapsed)) => {
                let seconds = elapsed.as_secs_f64().max(0.001);
                let mbps = bytes as f64 * 8.0 / seconds / 1_000_000.0;
                let mb = bytes as f64 / 1_048_576.0;
                return format!(
                    "Speed  |  {mbps:.1} Mbps  |  {mb:.1} MB in {seconds:.2} s"
                );
            }
            Err(error) => last_error = error,
        }
    }
    format!("Speed  |  failed ({last_error})")
}

fn download_http_via_proxy(proxy: &str, url: &str) -> Result<(u64, Duration), String> {
    let (host, _) = split_http_url(url)?;
    let address = proxy
        .to_socket_addrs()
        .map_err(|error| format!("proxy resolve: {error}"))?
        .next()
        .ok_or_else(|| "proxy has no address".to_string())?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(4))
        .map_err(|error| format!("proxy connect: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(8)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    let started = Instant::now();
    write!(
        stream,
        "GET {url} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: pvpn-speed-test\r\nConnection: close\r\n\r\n"
    )
    .map_err(|error| format!("proxy write: {error}"))?;

    let mut buffer = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    let header_end = loop {
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        let read = stream
            .read(&mut chunk)
            .map_err(|error| format!("proxy read: {error}"))?;
        if read == 0 {
            return Err("proxy closed during headers".to_string());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > 64 * 1024 {
            return Err("response headers too large".to_string());
        }
    };

    let header = String::from_utf8_lossy(&buffer[..header_end]).to_ascii_lowercase();
    let status = header
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("?");
    if status != "200" {
        return Err(format!("HTTP status {status}"));
    }
    let content_length = header.lines().find_map(|line| {
        line.strip_prefix("content-length:")
            .and_then(|value| value.trim().parse::<u64>().ok())
    });
    let limit = content_length.unwrap_or(20 * 1_048_576).min(20 * 1_048_576);

    let mut received = (buffer.len() - header_end) as u64;
    while received < limit {
        let read = match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) => {
                if received > 0 {
                    break;
                }
                return Err(format!("proxy read: {error}"));
            }
        };
        received += read as u64;
    }
    let elapsed = started.elapsed();
    if received < 100_000 {
        return Err(format!("too little data ({received} bytes)"));
    }
    Ok((received, elapsed))
}

fn split_http_url(url: &str) -> Result<(&str, &str), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| "only http:// URLs are supported".to_string())?;
    match rest.find('/') {
        Some(index) => Ok((&rest[..index], &rest[index..])),
        None => Ok((rest, "/")),
    }
}

fn card(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(CARD)
        .stroke(egui::Stroke::new(1.0_f32, BORDER))
        .corner_radius(12)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new(title)
                    .strong()
                    .size(15.0)
                    .color(egui::Color32::from_rgb(220, 242, 236)),
            );
            ui.add_space(4.0);
            body(ui);
        });
    ui.add_space(10.0);
}

fn chip(ui: &mut egui::Ui, label: &str, on: bool, color: egui::Color32) {
    let background = if on {
        egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 38)
    } else {
        egui::Color32::from_rgb(22, 32, 37)
    };
    egui::Frame::NONE
        .fill(background)
        .corner_radius(20)
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(label)
                    .strong()
                    .size(12.0)
                    .color(if on { color } else { MUTED }),
            );
        });
}

fn wait_listen(addr: &str, attempts: usize) -> bool {
    let Ok(addr) = addr.parse::<SocketAddr>() else {
        return false;
    };
    for _ in 0..attempts {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(150)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[cfg(windows)]
fn pac_configured() -> Result<bool, String> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("reg")
        .args([
            "query",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            "/v",
            "AutoConfigURL",
        ])
        .creation_flags(0x08000000)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Ok(false);
    }
    Ok(String::from_utf8_lossy(&output.stdout).contains("127.0.0.1:7892"))
}

#[cfg(windows)]
#[derive(Default, Serialize, Deserialize)]
struct ProxyBackup {
    auto_config_url: Option<String>,
    proxy_server: Option<String>,
    proxy_override: Option<String>,
    proxy_enable: Option<u32>,
    auto_detect: Option<u32>,
}

#[cfg(windows)]
fn proxy_backup_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("pvpn").join("proxy-backup.json"))
}

#[cfg(windows)]
fn reg_value(name: &str) -> Result<Option<String>, String> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("reg")
        .args([
            "query",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            "/v",
            name,
        ])
        .creation_flags(0x08000000)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() == Some(name) {
            let _kind = parts.next();
            let value = parts.collect::<Vec<_>>().join(" ");
            return Ok(Some(value));
        }
    }
    Ok(None)
}

#[cfg(windows)]
fn reg_dword(name: &str) -> Result<Option<u32>, String> {
    let Some(value) = reg_value(name)? else {
        return Ok(None);
    };
    let value = value.trim();
    let parsed = if let Some(hex) = value.strip_prefix("0x") {
        u32::from_str_radix(hex, 16)
    } else {
        value.parse::<u32>()
    };
    parsed.map(Some).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn capture_proxy_backup() -> Result<(), String> {
    let Some(path) = proxy_backup_path() else {
        return Ok(());
    };
    if path.exists() {
        return Ok(());
    }
    let backup = ProxyBackup {
        auto_config_url: reg_value("AutoConfigURL")?,
        proxy_server: reg_value("ProxyServer")?,
        proxy_override: reg_value("ProxyOverride")?,
        proxy_enable: reg_dword("ProxyEnable")?,
        auto_detect: reg_dword("AutoDetect")?,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let data = serde_json::to_vec_pretty(&backup).map_err(|error| error.to_string())?;
    fs::write(&path, data).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(windows)]
fn write_reg_sz(name: &str, value: Option<&str>) -> Result<(), String> {
    match value {
        Some(value) => reg(&[
            "add",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            "/v",
            name,
            "/t",
            "REG_SZ",
            "/d",
            value,
            "/f",
        ]),
        None => {
            let _ = reg(&[
                "delete",
                "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
                "/v",
                name,
                "/f",
            ]);
            Ok(())
        }
    }
}

#[cfg(windows)]
fn write_reg_dword(name: &str, value: Option<u32>) -> Result<(), String> {
    let text = value.unwrap_or(0).to_string();
    reg(&[
        "add",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
        "/v",
        name,
        "/t",
        "REG_DWORD",
        "/d",
        text.as_str(),
        "/f",
    ])
}

#[cfg(windows)]
fn restore_proxy_backup() -> Result<bool, String> {
    let Some(path) = proxy_backup_path() else {
        return Ok(false);
    };
    let Ok(data) = fs::read(&path) else {
        return Ok(false);
    };
    let backup: ProxyBackup = serde_json::from_slice(&data).map_err(|error| error.to_string())?;
    write_reg_sz("AutoConfigURL", backup.auto_config_url.as_deref())?;
    write_reg_sz("ProxyServer", backup.proxy_server.as_deref())?;
    write_reg_sz("ProxyOverride", backup.proxy_override.as_deref())?;
    write_reg_dword("ProxyEnable", backup.proxy_enable)?;
    write_reg_dword("AutoDetect", backup.auto_detect)?;
    let _ = fs::remove_file(path);
    Ok(true)
}

#[cfg(windows)]
fn set_pac_url(url: &str, proxy: &str) -> Result<(), String> {
    reg(&[
        "add",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
        "/v",
        "AutoConfigURL",
        "/t",
        "REG_SZ",
        "/d",
        url,
        "/f",
    ])?;
    // PAC-aware applications use AutoConfigURL for split tunneling. Apps that
    // only understand a static proxy (Codex, some Electron/CLI tools) use this
    // fallback and therefore get the full tunnel.
    reg(&[
        "add",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
        "/v",
        "ProxyServer",
        "/t",
        "REG_SZ",
        "/d",
        proxy,
        "/f",
    ])?;
    reg(&[
        "add",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
        "/v",
        "ProxyOverride",
        "/t",
        "REG_SZ",
        "/d",
        "<local>",
        "/f",
    ])?;
    reg(&[
        "add",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
        "/v",
        "ProxyEnable",
        "/t",
        "REG_DWORD",
        "/d",
        "1",
        "/f",
    ])?;
    reg(&[
        "add",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
        "/v",
        "AutoDetect",
        "/t",
        "REG_DWORD",
        "/d",
        "0",
        "/f",
    ])?;
    refresh_wininet();
    Ok(())
}

#[cfg(windows)]
fn clear_pac() -> Result<(), String> {
    if restore_proxy_backup()? {
        refresh_wininet();
        return Ok(());
    }
    Err("proxy backup is missing; refusing to overwrite the original settings".into())
}

#[cfg(windows)]
fn reg(args: &[&str]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("reg")
        .args(args)
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[cfg(windows)]
fn refresh_wininet() {
    use windows_sys::Win32::Networking::WinInet::{
        InternetSetOptionW, INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED,
    };
    unsafe {
        InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_SETTINGS_CHANGED,
            std::ptr::null_mut(),
            0,
        );
        InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_REFRESH,
            std::ptr::null_mut(),
            0,
        );
    }
}

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default().with_inner_size([760.0, 680.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!(
        "../../../assets/brand/desktop/pvpn-256.png"
    )) {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "pvpn",
        options,
        Box::new(|cc| Ok(Box::new(PvpnApp::new(cc)))),
    )
}


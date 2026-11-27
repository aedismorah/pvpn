package com.pvpn.app;

import android.Manifest;
import android.app.Activity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.net.VpnService;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.text.method.PasswordTransformationMethod;
import android.view.View;
import android.view.WindowInsets;
import android.widget.Button;
import android.widget.CheckBox;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.TextView;
import android.widget.Toast;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.RandomAccessFile;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.Proxy;
import java.net.Socket;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.nio.charset.StandardCharsets;
import java.text.SimpleDateFormat;
import java.util.ArrayList;
import java.util.Date;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

import javax.net.ssl.SSLSocket;
import javax.net.ssl.SSLSocketFactory;

public class MainActivity extends Activity {
    private static final String PREFS = "pvpn-ui";
    private static final int REQ_NOTIFICATIONS = 1;
    private static final int REQ_VPN = 2;
    private static final String DEFAULT_PSK = "";

    private EditText server;
    private EditText psk;
    private EditText listen;
    private CheckBox showPsk;
    private CheckBox tcpOnly;
    private TextView statusTitle;
    private TextView statusDetail;
    private View statusDot;
    private LinearLayout errorCard;
    private TextView errorText;
    private Button startVpn;
    private Button stopVpn;
    private Button startSocks;
    private Button stopSocks;
    private Button ping;
    private Button speed;
    private Button youtubeTest;
    private Button copyTestLog;
    private TextView testResult;
    private TextView splitSummary;
    private SquareSwitchView splitEnabled;
    private Button splitSitesButton;
    private Button splitAppsButton;
    private boolean testBusy;
    private String lastYoutubeLog = "";
    private final ExecutorService testExecutor = Executors.newSingleThreadExecutor();

    private final Handler handler = new Handler(Looper.getMainLooper());
    private final Runnable ticker = new Runnable() {
        @Override
        public void run() {
            refreshUi();
            handler.postDelayed(this, 500);
        }
    };

    private String localError = "";

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_main);
        applyWindowInsets();

        server = findViewById(R.id.server);
        psk = findViewById(R.id.psk);
        listen = findViewById(R.id.listen);
        showPsk = findViewById(R.id.show_psk);
        tcpOnly = findViewById(R.id.tcp_only);
        statusTitle = findViewById(R.id.status_title);
        statusDetail = findViewById(R.id.status_detail);
        statusDot = findViewById(R.id.status_dot);
        errorCard = findViewById(R.id.error_card);
        errorText = findViewById(R.id.error_text);
        startVpn = findViewById(R.id.start_vpn);
        stopVpn = findViewById(R.id.stop_vpn);
        startSocks = findViewById(R.id.start);
        stopSocks = findViewById(R.id.stop);
        ping = findViewById(R.id.ping);
        speed = findViewById(R.id.speed);
        youtubeTest = findViewById(R.id.youtube_test);
        copyTestLog = findViewById(R.id.copy_test_log);
        testResult = findViewById(R.id.test_result);
        splitSummary = findViewById(R.id.split_summary);
        splitEnabled = findViewById(R.id.split_enabled);
        splitSitesButton = findViewById(R.id.split_sites);
        splitAppsButton = findViewById(R.id.split_apps);
        View connectionBody = findViewById(R.id.connection_body);
        TextView connectionChevron = findViewById(R.id.connection_chevron);
        findViewById(R.id.connection_header).setOnClickListener(v -> {
            boolean show = connectionBody.getVisibility() != View.VISIBLE;
            connectionBody.setVisibility(show ? View.VISIBLE : View.GONE);
            connectionChevron.setText(show ? "\u25BE" : "\u25B8");
        });

        SharedPreferences prefs = getSharedPreferences(PREFS, MODE_PRIVATE);
        String savedServer = prefs.getString("server", "87.242.102.100:9331");
        if (savedServer.equals("87.242.102.100:9443")
                || savedServer.equals("87.242.102.100:443")) {
            savedServer = "87.242.102.100:9331";
        }
        server.setText(savedServer);
        listen.setText(prefs.getString("listen", "127.0.0.1:11080"));
        psk.setText(prefs.getString("psk", DEFAULT_PSK));
        tcpOnly.setChecked(prefs.getBoolean("tcp_only", true));
        tcpOnly.setOnCheckedChangeListener((button, checked) ->
                getSharedPreferences(PREFS, MODE_PRIVATE).edit()
                        .putBoolean("tcp_only", checked).apply());

        showPsk.setOnCheckedChangeListener((button, checked) ->
                psk.setTransformationMethod(
                        checked ? null : new PasswordTransformationMethod()));
        startVpn.setOnClickListener(v -> requestVpn());
        stopVpn.setOnClickListener(v -> stopVpn());
        startSocks.setOnClickListener(v -> startSocks());
        stopSocks.setOnClickListener(v -> stopSocks());
        ping.setOnClickListener(v -> runPing());
        speed.setOnClickListener(v -> runSpeedTest());
        youtubeTest.setOnClickListener(v -> runYouTubeTest());
        copyTestLog.setOnClickListener(v -> copyTestLog());
        splitSitesButton.setOnClickListener(v ->
                startActivity(new Intent(this, SplitTunnelActivity.class)));
        splitAppsButton.setOnClickListener(v ->
                startActivity(new Intent(this, AppSplitActivity.class)));
        splitEnabled.setOnCheckedChangedListener(checked -> {
            SplitConfig split = SplitConfig.load(this);
            split.enabled = checked;
            split.save(this);
            refreshSplitSummary();
        });
        findViewById(R.id.copy_error).setOnClickListener(v -> copyError());
        findViewById(R.id.dismiss_error).setOnClickListener(v -> {
            localError = "";
            AppState.clearError();
            refreshUi();
        });

        if (Build.VERSION.SDK_INT >= 33
                && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS},
                    REQ_NOTIFICATIONS);
        }
        refreshUi();

        String autoPsk = getIntent().getStringExtra("psk");
        if (autoPsk != null) {
            psk.setText(autoPsk);
        }
        String autoServer = getIntent().getStringExtra("server");
        if (autoServer != null) {
            server.setText(autoServer);
        }
        if (getIntent().getBooleanExtra("autostart", false)) {
            handler.postDelayed(this::requestVpn, 800);
        }
        if (getIntent().getBooleanExtra("speed", false)) {
            handler.postDelayed(this::runSpeedTest, 1800);
        }
        if (getIntent().getBooleanExtra("ytdiag", false)) {
            handler.postDelayed(this::runYouTubeTest, 2200);
        }
    }

    private void refreshSplitSummary() {
        if (splitSummary == null) {
            return;
        }
        SplitConfig split = SplitConfig.load(this);
        if (splitEnabled != null) {
            splitEnabled.setChecked(split.enabled, false);
        }
        if (splitSitesButton != null) {
            splitSitesButton.setEnabled(split.enabled);
            splitSitesButton.setAlpha(split.enabled ? 1f : 0.42f);
        }
        if (splitAppsButton != null) {
            splitAppsButton.setEnabled(split.enabled);
            splitAppsButton.setAlpha(split.enabled ? 1f : 0.42f);
        }
        if (!split.enabled) {
            splitSummary.setText("Off — all traffic goes through the VPN");
        } else {
            splitSummary.setText("Sites: "
                    + (split.sitesDirect ? "black list" : "white list")
                    + " • " + split.sites.size()
                    + "   Apps: "
                    + (split.appsDirect ? "black list" : "white list")
                    + " • " + split.apps.size());
        }
    }

    private void applyWindowInsets() {
        View root = findViewById(R.id.root);
        final int left = root.getPaddingLeft();
        final int top = root.getPaddingTop();
        final int right = root.getPaddingRight();
        final int bottom = root.getPaddingBottom();
        root.setOnApplyWindowInsetsListener((view, insets) -> {
            int insetLeft;
            int insetTop;
            int insetRight;
            int insetBottom;
            if (Build.VERSION.SDK_INT >= 30) {
                android.graphics.Insets bars =
                        insets.getInsets(WindowInsets.Type.systemBars());
                insetLeft = bars.left;
                insetTop = bars.top;
                insetRight = bars.right;
                insetBottom = bars.bottom;
            } else {
                insetLeft = insets.getSystemWindowInsetLeft();
                insetTop = insets.getSystemWindowInsetTop();
                insetRight = insets.getSystemWindowInsetRight();
                insetBottom = insets.getSystemWindowInsetBottom();
            }
            view.setPadding(left + insetLeft, top + insetTop,
                    right + insetRight, bottom + insetBottom);
            return insets;
        });
        root.requestApplyInsets();
    }

    @Override
    protected void onResume() {
        super.onResume();
        refreshSplitSummary();
        handler.removeCallbacks(ticker);
        handler.post(ticker);
    }

    @Override
    protected void onPause() {
        handler.removeCallbacks(ticker);
        super.onPause();
    }

    @Override
    protected void onDestroy() {
        handler.removeCallbacks(ticker);
        super.onDestroy();
    }

    private void startSocks() {
        if (AppState.vpnRunning || PvpnVpnService.running) {
            localError = "Stop the VPN before starting SOCKS5.";
            refreshUi();
            return;
        }
        if (!validateFields()) {
            return;
        }
        String host = server.getText().toString().trim();
        String key = psk.getText().toString().trim();
        String local = listen.getText().toString().trim();
        savePrefs(host, local);

        localError = "";
        AppState.beginSocks();
        Intent intent = new Intent(this, PvpnService.class);
        intent.setAction(PvpnService.ACTION_START);
        intent.putExtra(PvpnService.EXTRA_SERVER, host);
        intent.putExtra(PvpnService.EXTRA_PSK, key);
        intent.putExtra(PvpnService.EXTRA_LISTEN, local);
        try {
            startForegroundService(intent);
        } catch (Exception error) {
            AppState.fail("Could not start SOCKS5: " + error.getMessage());
        }
        refreshUi();
    }

    private void stopSocks() {
        if (AppState.vpnRunning || PvpnVpnService.running) {
            localError = "VPN is active. Stop the VPN first.";
            refreshUi();
            return;
        }
        localError = "";
        AppState.stopped();
        AppState.clearError();
        Intent intent = new Intent(this, PvpnService.class);
        intent.setAction(PvpnService.ACTION_STOP);
        startService(intent);
        refreshUi();
    }

    private void requestVpn() {
        if (!validateFields()) {
            return;
        }
        if (AppState.vpnRunning || PvpnVpnService.running) {
            localError = "VPN is already active.";
            refreshUi();
            return;
        }
        localError = "";
        AppState.beginVpn();
        NativeBridge.stop();
        stopService(new Intent(this, PvpnService.class));

        try {
            Intent prepare = VpnService.prepare(this);
            if (prepare != null) {
                startActivityForResult(prepare, REQ_VPN);
            } else {
                handler.postDelayed(this::launchVpn, 400);
            }
        } catch (Exception error) {
            AppState.fail("VPN permission check failed: " + error.getMessage());
        }
        refreshUi();
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == REQ_VPN) {
            if (resultCode == RESULT_OK) {
                launchVpn();
            } else {
                AppState.fail("VPN permission denied.");
                refreshUi();
            }
        }
    }

    private void launchVpn() {
        if (!validateFields()) {
            return;
        }
        String host = server.getText().toString().trim();
        String key = psk.getText().toString().trim();
        String local = listen.getText().toString().trim();
        savePrefs(host, local);

        AppState.beginVpn();
        Intent intent = new Intent(this, PvpnVpnService.class);
        intent.setAction(PvpnVpnService.ACTION_START);
        intent.putExtra(PvpnVpnService.EXTRA_SERVER, host);
        intent.putExtra(PvpnVpnService.EXTRA_PSK, key);
        intent.putExtra(PvpnVpnService.EXTRA_LISTEN, local);
        intent.putExtra(PvpnVpnService.EXTRA_TCP_ONLY, tcpOnly.isChecked());
        try {
            startForegroundService(intent);
        } catch (Exception error) {
            AppState.fail("Could not start VPN: " + error.getMessage());
        }
        refreshUi();
    }

    private void stopVpn() {
        localError = "";
        AppState.clearError();
        AppState.stopped();
        Intent intent = new Intent(this, PvpnVpnService.class);
        intent.setAction(PvpnVpnService.ACTION_STOP);
        startService(intent);
        refreshUi();
    }

    private boolean validateFields() {
        String host = server.getText().toString().trim();
        String key = psk.getText().toString().trim();
        String local = listen.getText().toString().trim();

        if (!validHostPort(host)) {
            localError = "Ingress must be host:port, for example 87.242.102.100:9331.";
            server.requestFocus();
            refreshUi();
            return false;
        }
        if (!key.matches("[0-9a-fA-F]{64}")) {
            localError = "PSK must be exactly 64 hex characters.";
            psk.requestFocus();
            refreshUi();
            return false;
        }
        if (!validHostPort(local)) {
            localError = "Local SOCKS5 address must be IP:port, for example 127.0.0.1:11080.";
            listen.requestFocus();
            refreshUi();
            return false;
        }
        localError = "";
        return true;
    }

    private boolean validHostPort(String value) {
        if (value == null || value.isEmpty()) {
            return false;
        }
        if (value.startsWith("[")) {
            int close = value.indexOf(']');
            if (close <= 1 || close + 2 > value.length() || value.charAt(close + 1) != ':') {
                return false;
            }
            return validPort(value.substring(close + 2));
        }
        int colon = value.lastIndexOf(':');
        if (colon <= 0 || colon == value.length() - 1) {
            return false;
        }
        return validPort(value.substring(colon + 1));
    }

    private boolean validPort(String value) {
        try {
            int port = Integer.parseInt(value);
            return port > 0 && port <= 65535;
        } catch (NumberFormatException error) {
            return false;
        }
    }

    private void savePrefs(String host, String local) {
        getSharedPreferences(PREFS, MODE_PRIVATE).edit()
                .putString("server", host)
                .putString("listen", local)
                .apply();
    }

    private void runYouTubeTest() {
        setTestBusy(true);
        testResult.setText(R.string.youtube_test_running);
        String hostPort = server.getText().toString().trim();
        int socksPort = localSocksPort();
        testExecutor.execute(() -> {
            StringBuilder log = new StringBuilder();
            List<ProbeResult> results = new ArrayList<>();
            UdpProbeResult udpDns = new UdpProbeResult();
            String stamp = new SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.US)
                    .format(new Date());
            append(log, "=== pvpn youtube diag " + stamp + " ===");
            append(log, "server=" + hostPort + " local_socks=127.0.0.1:" + socksPort
                    + " vpn=" + (AppState.vpnRunning || PvpnVpnService.running)
                    + " socks=" + AppState.socksRunning);

            Endpoint[] endpoints = {
                    new Endpoint("www.youtube.com", "/", 450_000),
                    new Endpoint("m.youtube.com", "/", 450_000),
                    new Endpoint("i.ytimg.com", "/vi/dQw4w9WgXcQ/hqdefault.jpg", 200_000),
                    new Endpoint("www.gstatic.com", "/generate_204", 4_096),
            };
            for (Endpoint endpoint : endpoints) {
                try {
                    long started = System.nanoTime();
                    InetAddress[] addresses = InetAddress.getAllByName(endpoint.host);
                    StringBuilder dns = new StringBuilder();
                    for (InetAddress address : addresses) {
                        if (dns.length() > 0) {
                            dns.append(',');
                        }
                        dns.append(address.getHostAddress());
                    }
                    append(log, "dns " + endpoint.host + " ms="
                            + elapsedMs(started) + " addrs=" + dns);
                } catch (IOException error) {
                    append(log, "dns " + endpoint.host + " error=" + shortError(error));
                }
            }

            try {
                for (int round = 1; round <= 3; round++) {
                    append(log, "--- round " + round + " ---");
                    for (Endpoint endpoint : endpoints) {
                        ProbeResult result = probeYoutube(endpoint);
                        results.add(result);
                        append(log, "req round=" + round
                                + " host=" + endpoint.host
                                + " path=" + endpoint.path
                                + " status=" + result.status
                                + " connect_ms=" + result.connectMs
                                + " tls_ms=" + result.tlsMs
                                + " ttfb_ms=" + result.ttfbMs
                                + " total_ms=" + result.totalMs
                                + " bytes=" + result.bytes
                                + " max_gap_ms=" + result.maxGapMs
                                + " stalls=" + result.stalls
                                + (result.error.isEmpty() ? "" : " error=" + result.error));
                    }
                    Thread.sleep(600);
                }
                udpDns = udpDnsProbe(socksPort);
                append(log, udpDns.line());
                append(log, hevSummary());
            } catch (InterruptedException error) {
                Thread.currentThread().interrupt();
            } catch (Exception error) {
                append(log, "fatal=" + error);
                if (udpDns.count == 0) {
                    udpDns.count = 10;
                }
                udpDns.error = error.toString();
                udpDns.loss = udpDns.count;
            }

            int ok = 0;
            int failed = 0;
            long maxGap = 0;
            int stalls = 0;
            long totalBytes = 0;
            long speedTimeMs = 0;
            double speedSumMbps = 0;
            int speedCount = 0;
            long connectSum = 0;
            long tlsSum = 0;
            List<Long> ttfb = new ArrayList<>();
            for (ProbeResult result : results) {
                if (result.error.isEmpty() && result.status >= 200 && result.status < 400) {
                    ok++;
                } else {
                    failed++;
                }
                if (result.error.isEmpty()) {
                    ttfb.add(result.ttfbMs);
                    connectSum += result.connectMs;
                    tlsSum += result.tlsMs;
                }
                maxGap = Math.max(maxGap, result.maxGapMs);
                stalls += result.stalls;
                if (result.error.isEmpty() && result.bytes > 0 && result.totalMs > 0) {
                    totalBytes += result.bytes;
                    speedTimeMs += result.totalMs;
                    speedSumMbps += result.bytes * 8.0 / result.totalMs / 1000.0;
                    speedCount++;
                }
            }
            long medianTtfb = 0;
            long minTtfb = 0;
            long maxTtfb = 0;
            long avgTtfb = 0;
            if (!ttfb.isEmpty()) {
                java.util.Collections.sort(ttfb);
                medianTtfb = ttfb.get(ttfb.size() / 2);
                minTtfb = ttfb.get(0);
                maxTtfb = ttfb.get(ttfb.size() - 1);
                long sum = 0;
                for (long value : ttfb) {
                    sum += value;
                }
                avgTtfb = sum / ttfb.size();
            }
            long avgConnect = ttfb.isEmpty() ? 0 : connectSum / ttfb.size();
            long avgTls = ttfb.isEmpty() ? 0 : tlsSum / ttfb.size();
            double avgMbps = speedCount > 0 ? speedSumMbps / speedCount : 0;
            double aggregateMbps = speedTimeMs > 0
                    ? totalBytes * 8.0 / speedTimeMs / 1000.0
                    : 0;
            double tcpLossPct = results.isEmpty()
                    ? 0
                    : failed * 100.0 / results.size();
            double udpLossPct = udpDns.count > 0
                    ? udpDns.loss * 100.0 / udpDns.count
                    : 0;
            long udpAvg = udpDns.ok > 0 ? udpDns.sumMs / udpDns.ok : 0;
            String udpLine = udpDns.error.isEmpty()
                    ? String.format(Locale.US,
                    "UDP DNS: %d/%d, loss %.1f%%, RTT avg %d ms",
                    udpDns.ok, udpDns.count, udpLossPct, udpAvg)
                    : "UDP DNS: failed (" + udpDns.error + ")";
            String summary = String.format(
                    Locale.US,
                    "YouTube: %d/%d OK, lost %d (%.1f%%)\n"
                            + "Avg speed: %.1f Mbps (aggregate %.1f)\n"
                            + "TTFB avg %d ms, median %d ms (%d–%d)\n"
                            + "%s\n"
                            + "Stalls: %d, max gap %d ms. Log copied.",
                    ok,
                    results.size(),
                    failed,
                    tcpLossPct,
                    avgMbps,
                    aggregateMbps,
                    avgTtfb,
                    medianTtfb,
                    minTtfb,
                    maxTtfb,
                    udpLine,
                    stalls,
                    maxGap);
            append(log, String.format(
                    Locale.US,
                    "overall requests=%d ok=%d failed=%d tcp_loss_pct=%.1f",
                    results.size(), ok, failed, tcpLossPct));
            append(log, String.format(
                    Locale.US,
                    "overall avg_mbps=%.2f aggregate_mbps=%.2f bytes=%d",
                    avgMbps, aggregateMbps, totalBytes));
            append(log, String.format(
                    Locale.US,
                    "overall ttfb_avg_ms=%d ttfb_median_ms=%d ttfb_min_ms=%d ttfb_max_ms=%d",
                    avgTtfb, medianTtfb, minTtfb, maxTtfb));
            append(log, String.format(
                    Locale.US,
                    "overall connect_avg_ms=%d tls_avg_ms=%d stalls=%d max_gap_ms=%d",
                    avgConnect, avgTls, stalls, maxGap));
            append(log, udpLine);

            lastYoutubeLog = log.toString();
            writeYoutubeLog(lastYoutubeLog);
            runOnUiThread(() -> {
                testResult.setText(summary);
                setTestBusy(false);
                copyToClipboard(lastYoutubeLog);
                Toast.makeText(this, R.string.youtube_test_done, Toast.LENGTH_SHORT).show();
            });
        });
    }

    private void copyTestLog() {
        String text = lastYoutubeLog;
        if (text.isEmpty()) {
            File file = new File(getFilesDir(), "youtube-diag.log");
            if (file.exists()) {
                try {
                    text = new String(java.nio.file.Files.readAllBytes(file.toPath()),
                            StandardCharsets.UTF_8);
                } catch (IOException ignored) {
                }
            }
        }
        if (text.isEmpty()) {
            Toast.makeText(this, "No YouTube log yet", Toast.LENGTH_SHORT).show();
            return;
        }
        copyToClipboard(text);
        Toast.makeText(this, "YouTube log copied", Toast.LENGTH_SHORT).show();
    }

    private void copyToClipboard(String text) {
        ClipboardManager clipboard =
                (ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        clipboard.setPrimaryClip(ClipData.newPlainText("pvpn youtube log", text));
    }

    private static void append(StringBuilder log, String line) {
        log.append(line).append('\n');
    }

    private static final class Endpoint {
        final String host;
        final String path;
        final int maxBytes;

        Endpoint(String host, String path, int maxBytes) {
            this.host = host;
            this.path = path;
            this.maxBytes = maxBytes;
        }
    }

    private static final class ProbeResult {
        int status;
        long connectMs;
        long tlsMs;
        long ttfbMs;
        long totalMs;
        long bytes;
        long maxGapMs;
        int stalls;
        String error = "";
    }

    private static final class UdpProbeResult {
        int ok;
        int count;
        long minMs;
        long maxMs;
        long sumMs;
        long loss;
        String error = "";

        String line() {
            if (!error.isEmpty()) {
                return "udp_dns error=" + error;
            }
            long avg = ok > 0 ? sumMs / ok : 0;
            return "udp_dns ok=" + ok + "/" + count
                    + " min_ms=" + minMs
                    + " max_ms=" + maxMs
                    + " avg_ms=" + avg
                    + " loss=" + loss
                    + " loss_pct=" + String.format(Locale.US, "%.1f",
                    count > 0 ? loss * 100.0 / count : 0.0);
        }
    }

    private ProbeResult probeYoutube(Endpoint endpoint) {
        ProbeResult result = new ProbeResult();
        long started = System.nanoTime();
        Socket raw = null;
        SSLSocket ssl = null;
        try {
            raw = new Socket(new Proxy(
                    Proxy.Type.SOCKS,
                    new InetSocketAddress("127.0.0.1", localSocksPort())));
            long connectStarted = System.nanoTime();
            raw.connect(
                    InetSocketAddress.createUnresolved(endpoint.host, 443),
                    8000);
            result.connectMs = elapsedMs(connectStarted);
            raw.setSoTimeout(20000);
            ssl = (SSLSocket) ((SSLSocketFactory) SSLSocketFactory.getDefault())
                    .createSocket(raw, endpoint.host, 443, true);
            ssl.setSoTimeout(20000);
            long tlsStarted = System.nanoTime();
            ssl.startHandshake();
            result.tlsMs = elapsedMs(tlsStarted);

            OutputStream output = ssl.getOutputStream();
            output.write((
                    "GET " + endpoint.path + " HTTP/1.1\r\n"
                            + "Host: " + endpoint.host + "\r\n"
                            + "User-Agent: pvpn-youtube-diag\r\n"
                            + "Accept: */*\r\n"
                            + "Connection: close\r\n\r\n").getBytes(StandardCharsets.UTF_8));
            output.flush();
            long requestAt = System.nanoTime();

            InputStream input = ssl.getInputStream();
            StringBuilder header = new StringBuilder();
            int matched = 0;
            int value;
            while ((value = input.read()) >= 0) {
                header.append((char) value);
                if ((matched == 0 && value == '\r')
                        || (matched == 1 && value == '\n')
                        || (matched == 2 && value == '\r')
                        || (matched == 3 && value == '\n')) {
                    matched++;
                    if (matched == 4) {
                        break;
                    }
                } else {
                    matched = 0;
                }
                if (header.length() > 65536) {
                    throw new IOException("oversized HTTP header");
                }
            }
            result.status = parseStatus(header.toString());
            if (result.status == 0) {
                throw new IOException("bad HTTP status");
            }

            byte[] buffer = new byte[32768];
            long lastRead = System.nanoTime();
            boolean firstBody = true;
            int read;
            while (result.bytes < endpoint.maxBytes
                    && (read = input.read(buffer, 0,
                    (int) Math.min(buffer.length, endpoint.maxBytes - result.bytes))) > 0) {
                long now = System.nanoTime();
                if (firstBody) {
                    result.ttfbMs = elapsedMs(requestAt);
                    firstBody = false;
                }
                long gap = (now - lastRead) / 1_000_000;
                result.maxGapMs = Math.max(result.maxGapMs, gap);
                if (gap >= 1000) {
                    result.stalls++;
                }
                lastRead = now;
                result.bytes += read;
            }
            if (firstBody) {
                result.ttfbMs = elapsedMs(requestAt);
            }
            result.totalMs = elapsedMs(started);
        } catch (IOException error) {
            result.error = shortError(error);
            result.totalMs = elapsedMs(started);
        } finally {
            if (ssl != null) {
                try {
                    ssl.close();
                } catch (IOException ignored) {
                }
            } else {
                closeQuietly(raw);
            }
        }
        return result;
    }

    private static int parseStatus(String header) {
        int space = header.indexOf(' ');
        if (space < 0 || header.length() < space + 4) {
            return 0;
        }
        try {
            return Integer.parseInt(header.substring(space + 1, space + 4));
        } catch (NumberFormatException error) {
            return 0;
        }
    }

    private void writeYoutubeLog(String text) {
        try (FileOutputStream output =
                     new FileOutputStream(new File(getFilesDir(), "youtube-diag.log"), true)) {
            output.write(text.getBytes(StandardCharsets.UTF_8));
            output.write('\n');
        } catch (IOException ignored) {
        }
    }

    private UdpProbeResult udpDnsProbe(int socksPort) {
        UdpProbeResult result = new UdpProbeResult();
        result.count = 10;
        Socket control = null;
        DatagramSocket udp = null;
        try {
            control = new Socket("127.0.0.1", socksPort);
            control.setSoTimeout(5000);
            OutputStream output = control.getOutputStream();
            InputStream input = control.getInputStream();
            output.write(new byte[]{5, 1, 0});
            output.flush();
            byte[] auth = new byte[2];
            if (input.read(auth) != 2 || auth[1] != 0) {
                result.error = "socks auth";
                return result;
            }
            output.write(new byte[]{5, 3, 0, 1, 0, 0, 0, 0, 0, 0});
            output.flush();
            byte[] reply = new byte[10];
            if (input.read(reply) != 10 || reply[1] != 0) {
                result.error = "socks associate";
                return result;
            }
            int relayPort = ((reply[8] & 0xff) << 8) | (reply[9] & 0xff);
            udp = new DatagramSocket();
            udp.setSoTimeout(1800);
            InetAddress relay = InetAddress.getByName("127.0.0.1");

            result.minMs = Long.MAX_VALUE;
            result.maxMs = 0;
            for (int index = 0; index < result.count; index++) {
                int id = 0x5000 + index;
                byte[] query = buildDnsQuery(id);
                byte[] packet = new byte[10 + query.length];
                packet[3] = 1;
                packet[4] = 1;
                packet[5] = 1;
                packet[6] = 1;
                packet[7] = 1;
                packet[8] = 0;
                packet[9] = 53;
                System.arraycopy(query, 0, packet, 10, query.length);
                long started = System.nanoTime();
                udp.send(new DatagramPacket(packet, packet.length, relay, relayPort));
                byte[] response = new byte[2048];
                DatagramPacket datagram = new DatagramPacket(response, response.length);
                try {
                    udp.receive(datagram);
                    if (datagram.getLength() > 0) {
                        result.ok++;
                        long rtt = elapsedMs(started);
                        result.minMs = Math.min(result.minMs, rtt);
                        result.maxMs = Math.max(result.maxMs, rtt);
                        result.sumMs += rtt;
                    }
                } catch (java.net.SocketTimeoutException ignored) {
                    // Counted as loss below.
                }
                Thread.sleep(120);
            }
            if (result.minMs == Long.MAX_VALUE) {
                result.minMs = 0;
            }
            result.loss = result.count - result.ok;
            return result;
        } catch (Exception error) {
            result.error = error.toString();
            result.loss = result.count;
            return result;
        } finally {
            if (udp != null) {
                udp.close();
            }
            closeQuietly(control);
        }
    }

    private static byte[] buildDnsQuery(int id) {
        byte[] query = new byte[33];
        query[0] = (byte) (id >> 8);
        query[1] = (byte) id;
        query[2] = 1;
        query[4] = 1;
        int offset = 12;
        query[offset++] = 3;
        for (byte b : "www".getBytes(StandardCharsets.US_ASCII)) {
            query[offset++] = b;
        }
        query[offset++] = 7;
        for (byte b : "youtube".getBytes(StandardCharsets.US_ASCII)) {
            query[offset++] = b;
        }
        query[offset++] = 3;
        for (byte b : "com".getBytes(StandardCharsets.US_ASCII)) {
            query[offset++] = b;
        }
        query[offset++] = 0;
        query[offset++] = 0;
        query[offset++] = 1;
        query[offset++] = 0;
        query[offset] = 1;
        return query;
    }

    private String hevSummary() {
        StringBuilder summary = new StringBuilder();
        File file = new File(getFilesDir(), "hev.log");
        if (!file.exists()) {
            return "hev_log=missing";
        }
        String tail = readTail(file, 256 * 1024);
        summary.append("hev_tail udp_construct=").append(count(tail, "udp construct"))
                .append(" udp_splice=").append(count(tail, "udp splice"))
                .append(" udp_destruct=").append(count(tail, "udp destruct"))
                .append(" tcp_open=").append(count(tail, "socks5 client tcp ->"))
                .append(" errors=").append(count(tail, "error"))
                .append(" fails=").append(count(tail, "fail"));
        return summary.toString();
    }

    private static int count(String text, String needle) {
        int count = 0;
        int index = 0;
        while ((index = text.indexOf(needle, index)) >= 0) {
            count++;
            index += needle.length();
        }
        return count;
    }

    private static String readTail(File file, int bytes) {
        try (RandomAccessFile input = new RandomAccessFile(file, "r")) {
            long length = input.length();
            input.seek(Math.max(0, length - bytes));
            byte[] data = new byte[(int) Math.min(bytes, length)];
            input.readFully(data);
            return new String(data, StandardCharsets.UTF_8);
        } catch (IOException error) {
            return "";
        }
    }


    private void runPing() {
        String hostPort = server.getText().toString().trim();
        int split = hostPort.lastIndexOf(':');
        if (split <= 0) {
            testResult.setText("Ping failed: invalid ingress");
            return;
        }
        String host = hostPort.substring(0, split);
        int port;
        try {
            port = Integer.parseInt(hostPort.substring(split + 1));
        } catch (NumberFormatException error) {
            testResult.setText("Ping failed: invalid ingress port");
            return;
        }
        int socksPort = localSocksPort();
        setTestBusy(true);
        testResult.setText(R.string.test_pinging);
        testExecutor.execute(() -> {
            String ingress;
            String tunnel;
            Socket socket = new Socket();
            try {
                long started = System.nanoTime();
                socket.connect(new InetSocketAddress(host, port), 4000);
                ingress = String.format(Locale.US, "%d ms", elapsedMs(started));
            } catch (IOException error) {
                ingress = "failed (" + shortError(error) + ")";
            } finally {
                closeQuietly(socket);
            }

            Socket tunnelSocket = new Socket(
                    new Proxy(Proxy.Type.SOCKS, new InetSocketAddress("127.0.0.1", socksPort)));
            try {
                long started = System.nanoTime();
                tunnelSocket.connect(new InetSocketAddress("1.1.1.1", 443), 7000);
                tunnel = String.format(Locale.US, "%d ms", elapsedMs(started));
            } catch (IOException error) {
                tunnel = "failed (" + shortError(error) + ")";
            } finally {
                closeQuietly(tunnelSocket);
            }
            String message = "Ingress: " + ingress + "\\nTunnel: " + tunnel;
            runOnUiThread(() -> {
                testResult.setText(message);
                setTestBusy(false);
            });
        });
    }

    private void runSpeedTest() {
        int socksPort = localSocksPort();
        setTestBusy(true);
        testResult.setText(R.string.test_speed_running);
        testExecutor.execute(() -> {
            String message;
            try {
                long started = System.nanoTime();
                long bytes = downloadThroughTunnel(socksPort);
                long millis = Math.max(1, elapsedMs(started));
                if (bytes < 100_000) {
                    throw new IOException("short response");
                }
                double mbps = bytes * 8.0 / 1_000_000.0 / (millis / 1000.0);
                message = String.format(
                        Locale.US,
                        "%.1f MB in %.2f s = %.1f Mbps",
                        bytes / 1_000_000.0,
                        millis / 1000.0,
                        mbps);
            } catch (IOException error) {
                message = "Speed test failed: " + shortError(error);
            }
            String finalMessage = message;
            runOnUiThread(() -> {
                testResult.setText(finalMessage);
                setTestBusy(false);
            });
        });
    }

    /**
     * Download through the local SOCKS5 listener with an unresolved target name.
     * Java resolves URL hosts locally; that would turn mapdns' fake IP into a
     * SOCKS target. Sending the name lets pvpn resolve it on the exit server.
     */
    private long downloadThroughTunnel(int socksPort) throws IOException {
        Socket raw = new Socket(
                new Proxy(Proxy.Type.SOCKS, new InetSocketAddress("127.0.0.1", socksPort)));
        SSLSocket ssl = null;
        try {
            raw.connect(
                    InetSocketAddress.createUnresolved("speed.cloudflare.com", 443),
                    10000);
            raw.setSoTimeout(30000);
            ssl = (SSLSocket) ((SSLSocketFactory) SSLSocketFactory.getDefault()).createSocket(
                    raw, "speed.cloudflare.com", 443, true);
            ssl.setSoTimeout(30000);
            ssl.startHandshake();
            OutputStream output = ssl.getOutputStream();
            output.write((
                    "GET /__down?bytes=5000000 HTTP/1.1\r\n"
                            + "Host: speed.cloudflare.com\r\n"
                            + "User-Agent: pvpn-android-speedtest\r\n"
                            + "Connection: close\r\n\r\n").getBytes("UTF-8"));
            output.flush();

            InputStream input = ssl.getInputStream();
            long headerBytes = 0;
            int matched = 0;
            int value;
            while ((value = input.read()) >= 0) {
                if ((matched == 0 && value == '\r')
                        || (matched == 1 && value == '\n')
                        || (matched == 2 && value == '\r')
                        || (matched == 3 && value == '\n')) {
                    matched++;
                    if (matched == 4) {
                        break;
                    }
                } else {
                    matched = 0;
                }
                headerBytes++;
                if (headerBytes > 65536) {
                    throw new IOException("bad HTTP response");
                }
            }
            long body = 0;
            byte[] buffer = new byte[32768];
            int read;
            while (body < 5_500_000 && (read = input.read(buffer)) > 0) {
                body += read;
            }
            return body;
        } finally {
            if (ssl != null) {
                try {
                    ssl.close();
                } catch (IOException ignored) {
                }
            } else {
                closeQuietly(raw);
            }
        }
    }

    private int localSocksPort() {
        String value = listen.getText().toString().trim();
        int split = value.lastIndexOf(':');
        if (split < 0) {
            return 11080;
        }
        try {
            return Integer.parseInt(value.substring(split + 1));
        } catch (NumberFormatException error) {
            return 11080;
        }
    }

    private static long elapsedMs(long started) {
        return (System.nanoTime() - started) / 1_000_000;
    }

    private static String shortError(IOException error) {
        String message = error.getMessage();
        return message == null || message.isEmpty()
                ? error.getClass().getSimpleName()
                : message;
    }

    private static void closeQuietly(Socket socket) {
        if (socket == null) {
            return;
        }
        try {
            socket.close();
        } catch (IOException ignored) {
        }
    }

    private void setTestBusy(boolean busy) {
        testBusy = busy;
        boolean active = AppState.vpnRunning || AppState.socksRunning || PvpnVpnService.running;
        ping.setEnabled(!busy && active);
        speed.setEnabled(!busy && active);
        youtubeTest.setEnabled(!busy && active);
        ping.setAlpha(ping.isEnabled() ? 1f : 0.42f);
        speed.setAlpha(speed.isEnabled() ? 1f : 0.42f);
        youtubeTest.setAlpha(youtubeTest.isEnabled() ? 1f : 0.42f);
    }

    private void refreshUi() {
        boolean vpnRunning = AppState.vpnRunning || PvpnVpnService.running;
        boolean vpnStarting = AppState.vpnStarting;
        boolean socksRunning = AppState.socksRunning;
        boolean socksStarting = AppState.socksStarting;
        String error = !localError.isEmpty() ? localError : AppState.lastError;

        if (!error.isEmpty()) {
            errorCard.setVisibility(View.VISIBLE);
            errorText.setText(error);
            statusTitle.setText(R.string.status_error);
            statusDetail.setText("The tunnel is stopped. Fix the issue above and try again.");
            statusDot.setBackgroundResource(R.drawable.dot_error);
        } else {
            errorCard.setVisibility(View.GONE);
            if (vpnRunning) {
                SplitConfig split = SplitConfig.load(this);
                statusTitle.setText(split.enabled
                        ? "VPN active - split tunneling"
                        : getString(R.string.status_vpn));
                statusDetail.setText(AppState.detail.isEmpty()
                        ? "All app traffic is routed through Cloud.ru -> VDSina."
                        : AppState.detail);
                statusDot.setBackgroundResource(R.drawable.dot_on);
            } else if (vpnStarting) {
                statusTitle.setText("Starting VPN...");
                statusDetail.setText("Waiting for TUN and the pvpn client.");
                statusDot.setBackgroundResource(R.drawable.dot_busy);
            } else if (socksRunning) {
                statusTitle.setText("SOCKS5 active");
                statusDetail.setText("Listening on " + listen.getText().toString().trim()
                        + ". Use it in Telegram or other proxy-aware apps.");
                statusDot.setBackgroundResource(R.drawable.dot_on);
            } else if (socksStarting) {
                statusTitle.setText("Starting SOCKS5...");
                statusDetail.setText("Waiting for the local listener.");
                statusDot.setBackgroundResource(R.drawable.dot_busy);
            } else {
                statusTitle.setText(R.string.status_stopped);
                statusDetail.setText("Connect to route all apps, or start SOCKS5 for apps that support a proxy.");
                statusDot.setBackgroundResource(R.drawable.dot_off);
            }
        }

        boolean active = vpnRunning || vpnStarting || socksRunning || socksStarting;
        startVpn.setEnabled(!active);
        stopVpn.setEnabled(vpnRunning || vpnStarting);
        startSocks.setEnabled(!active);
        stopSocks.setEnabled(socksRunning || socksStarting);
        startVpn.setAlpha(startVpn.isEnabled() ? 1f : 0.42f);
        stopVpn.setAlpha(stopVpn.isEnabled() ? 1f : 0.42f);
        startSocks.setAlpha(startSocks.isEnabled() ? 1f : 0.42f);
        stopSocks.setAlpha(stopSocks.isEnabled() ? 1f : 0.42f);
        boolean testable = (vpnRunning || socksRunning) && !testBusy;
        ping.setEnabled(testable);
        speed.setEnabled(testable);
        youtubeTest.setEnabled(testable);
        ping.setAlpha(testable ? 1f : 0.42f);
        speed.setAlpha(testable ? 1f : 0.42f);
        youtubeTest.setAlpha(testable ? 1f : 0.42f);
        server.setEnabled(!active);
        psk.setEnabled(!active);
        listen.setEnabled(!active);
        showPsk.setEnabled(!active);
    }

    private void copyError() {
        String text = errorText.getText().toString();
        if (text.isEmpty()) {
            return;
        }
        ClipboardManager clipboard = (ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        clipboard.setPrimaryClip(ClipData.newPlainText("pvpn error", text));
        Toast.makeText(this, "Error copied", Toast.LENGTH_SHORT).show();
    }
}

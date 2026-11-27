package com.pvpn.app;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Intent;
import android.net.VpnService;
import android.os.Build;
import android.os.IBinder;
import android.os.ParcelFileDescriptor;
import android.util.Log;

import java.io.File;
import java.io.FileOutputStream;
import java.net.Inet4Address;
import java.net.InetAddress;

import hev.htproxy.TProxyService;

public class PvpnVpnService extends VpnService {
    public static final String ACTION_START = "com.pvpn.app.VPN_START";
    public static final String ACTION_STOP = "com.pvpn.app.VPN_STOP";
    public static final String EXTRA_SERVER = "server";
    public static final String EXTRA_PSK = "psk";
    public static final String EXTRA_LISTEN = "listen";
    public static final String EXTRA_TCP_ONLY = "tcp_only";

    public static volatile boolean running = false;

    private static final String TAG = "pvpn-vpn";
    private static final String CHANNEL_ID = "pvpn";
    private static final int NOTIFICATION_ID = 8;

    private static final String VPN_IP = "10.255.0.2";
    private static final String DNS_IP = "10.255.0.3";

    private ParcelFileDescriptor tun;
    private boolean clientStarted = false;
    private volatile boolean starting = false;
    private volatile boolean stopRequested = false;
    private final Object stateLock = new Object();
    private SplitConfig splitConfig;

    @Override
    public void onCreate() {
        super.onCreate();
        NotificationChannel channel = new NotificationChannel(
                CHANNEL_ID, "pvpn tunnel", NotificationManager.IMPORTANCE_LOW);
        getSystemService(NotificationManager.class).createNotificationChannel(channel);
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent == null) {
            return START_NOT_STICKY;
        }
        if (ACTION_STOP.equals(intent.getAction())) {
            AppState.stopped();
            AppState.clearError();
            stopVpn();
            return START_NOT_STICKY;
        }

        startForeground(NOTIFICATION_ID, notification("Starting VPN..."));
        if (running || starting) {
            AppState.vpnReady();
            return START_NOT_STICKY;
        }
        AppState.beginVpn();

        String server = intent.getStringExtra(EXTRA_SERVER);
        String psk = intent.getStringExtra(EXTRA_PSK);
        String listen = intent.getStringExtra(EXTRA_LISTEN);
        boolean tcpOnly = intent.getBooleanExtra(EXTRA_TCP_ONLY, true);
        if (server == null || psk == null || listen == null) {
            fail("Missing server/PSK");
            return START_NOT_STICKY;
        }
        int socksPort;
        String socksAddress;
        int colon = listen.lastIndexOf(':');
        try {
            if (colon <= 0) {
                throw new NumberFormatException("missing port");
            }
            socksAddress = listen.substring(0, colon);
            if (socksAddress.startsWith("[") && socksAddress.endsWith("]")) {
                socksAddress = socksAddress.substring(1, socksAddress.length() - 1);
            }
            socksPort = Integer.parseInt(listen.substring(colon + 1));
            if (socksPort < 1 || socksPort > 65535 || socksAddress.isEmpty()) {
                throw new NumberFormatException("bad listener");
            }
        } catch (NumberFormatException error) {
            fail("Invalid local SOCKS5 listener: " + listen);
            return START_NOT_STICKY;
        }

        starting = true;
        stopRequested = false;
        final int listenerPort = socksPort;        final String listenerAddress = socksAddress;
        new Thread(
                () -> startVpn(server, psk, listen, listenerPort, listenerAddress, tcpOnly),
                "pvpn-vpn-start").start();
        return START_NOT_STICKY;
    }

    private void startVpn(
            String server,
            String psk,
            String listen,
            int socksPort,
            String socksAddress,
            boolean tcpOnly) {
        if (stopRequested) {
            starting = false;
            return;
        }
        if (!NativeBridge.start(server, psk, listen)) {
            fail("Could not start pvpn client: " + NativeBridge.lastError());
            return;
        }
        clientStarted = true;
        if (stopRequested) {
            stopVpn();
            return;
        }

        boolean siteWhitelist = false;
        boolean useRealDns = false;
        try {
            SplitConfig split = SplitConfig.load(this);
            splitConfig = split;
            if (split.enabled && !split.appsDirect && split.apps.isEmpty()) {
                fail("App white list is empty; add an app or switch to black list");
                return;
            }
            if (split.enabled && !split.sitesDirect && split.sites.isEmpty()) {
                fail("Site white list is empty; add a site or switch to black list");
                return;
            }
            boolean appWhitelist = split.enabled && !split.appsDirect;
            siteWhitelist = split.enabled && !split.sitesDirect;
            boolean siteBlacklist = split.enabled && split.sitesDirect && !split.sites.isEmpty();
            if (siteBlacklist && Build.VERSION.SDK_INT < 33) {
                fail("Site black list requires Android 13 or newer");
                return;
            }
            useRealDns = siteWhitelist || siteBlacklist;
            Builder builder = new Builder()
                    .setSession("pvpn")
                    .setMtu(1500)
                    .addAddress(VPN_IP, 32)
                    .addDnsServer(useRealDns ? "1.1.1.1" : DNS_IP);
            if (appWhitelist) {
                // Android forbids combining allowed and disallowed application lists.
                // The VPN app itself is not in the allowed list, so it stays outside.
                int allowedApps = 0;
                for (String packageName : split.apps) {
                    if (packageName.equals(getPackageName())) {
                        continue;
                    }
                    try {
                        builder.addAllowedApplication(packageName);
                        allowedApps++;
                    } catch (Exception ignored) {
                    }
                }
                if (allowedApps == 0) {
                    fail("App white list has no installed apps; fix the list or switch to black list");
                    return;
                }
            } else {
                builder.addDisallowedApplication(getPackageName());
                if (split.enabled && split.appsDirect) {
                    for (String packageName : split.apps) {
                        try {
                            builder.addDisallowedApplication(packageName);
                        } catch (Exception ignored) {
                        }
                    }
                }
            }
            if (siteWhitelist) {
                int siteRoutes = 0;
                for (String site : split.sites) {
                    siteRoutes += addIncludedSite(builder, site);
                }
                if (siteRoutes == 0) {
                    fail("No site IPs could be resolved for the white list; check the sites or use black list");
                    return;
                }
            } else {
                builder.addRoute("0.0.0.0", 0);
                if (siteBlacklist) {
                    try {
                        builder.excludeRoute(new android.net.IpPrefix(
                                InetAddress.getByName("1.1.1.1"), 32));
                    } catch (Exception ignored) {
                    }
                    for (String site : split.sites) {
                        addExcludedSite(builder, site);
                    }
                }
            }
            tun = builder.establish();
        } catch (Exception error) {
            Log.e(TAG, "VpnService establish failed", error);
            fail("VPN permission/TUN failed");
            return;
        }
        if (tun == null) {
            fail("TUN was not established");
            return;
        }
        if (stopRequested) {
            stopVpn();
            return;
        }

        File config = new File(getFilesDir(), "hev.yaml");
        File log = new File(getFilesDir(), "hev.log");
        String yaml = "tunnel:\n"
                + "  mtu: 1500\n"
                + "  ipv4: " + VPN_IP + "\n"
                + "socks5:\n"
                + "  port: " + socksPort + "\n"
                + "  address: " + socksAddress + "\n"
                + "  udp: '" + (tcpOnly ? "tcp" : "udp") + "'\n"
                + (useRealDns ? "" : ("mapdns:\n"
                + "  address: " + DNS_IP + "\n"
                + "  port: 53\n"
                + "  network: 100.64.0.0\n"
                + "  netmask: 255.192.0.0\n"
                + "  cache-size: 10000\n"))
                + "misc:\n"
                + "  log-file: " + log.getAbsolutePath() + "\n"
                + "  log-level: debug\n"
                + "  task-stack-size: 86016\n"
                + "  tcp-buffer-size: 65536\n";
        try (FileOutputStream out = new FileOutputStream(config)) {
            out.write(yaml.getBytes());
        } catch (Exception error) {
            Log.e(TAG, "write config failed", error);
            fail("Could not write tunnel config");
            return;
        }

        boolean started;
        try {
            started = TProxyService.TProxyStartService(config.getAbsolutePath(), tun.getFd());
        } catch (Throwable error) {
            Log.e(TAG, "hev start failed", error);
            started = false;
        }
        if (!started) {
            fail("hev-socks5-tunnel failed to start");
            return;
        }
        if (stopRequested) {
            stopVpn();
            return;
        }

        synchronized (stateLock) {
            if (stopRequested) {
                starting = false;
                return;
            }
            running = true;
            starting = false;
        }
        AppState.vpnReady();
        if (splitConfig != null && splitConfig.enabled) {
            AppState.detail = "Split tunneling is active. Selected items follow their black/white list mode.";
        }
        getSystemService(NotificationManager.class)
                .notify(NOTIFICATION_ID, notification(splitConfig != null
                        && splitConfig.enabled ? "VPN active - split tunneling"
                        : "VPN active - all apps"));
        return;
    }

    private int addIncludedSite(Builder builder, String site) {
        String host = site;
        int prefix = 32;
        int slash = site.indexOf('/');
        if (slash > 0) {
            host = site.substring(0, slash);
            try {
                prefix = Integer.parseInt(site.substring(slash + 1));
            } catch (NumberFormatException ignored) {
                prefix = 32;
            }
        }
        int added = 0;
        for (String ip : SplitConfig.resolveIPv4(host)) {
            try {
                builder.addRoute(ip, Math.max(0, Math.min(32, prefix)));
                added++;
            } catch (Exception ignored) {
            }
        }
        return added;
    }

    private void addExcludedSite(Builder builder, String site) {
        String host = site;
        int prefix = 32;
        int slash = site.indexOf('/');
        if (slash > 0) {
            host = site.substring(0, slash);
            try {
                prefix = Integer.parseInt(site.substring(slash + 1));
            } catch (NumberFormatException ignored) {
                prefix = 32;
            }
        }
        for (String ip : SplitConfig.resolveIPv4(host)) {
            try {
                InetAddress address = InetAddress.getByName(ip);
                if (address instanceof Inet4Address) {
                    builder.excludeRoute(new android.net.IpPrefix(address, prefix));
                    Log.i(TAG, "excluded site route " + ip + "/" + prefix);
                }
            } catch (Exception error) {
                Log.w(TAG, "site route failed " + ip + ": " + error);
            }
        }
    }

    private void fail(String message) {
        starting = false;
        stopRequested = true;
        Log.e(TAG, message);
        AppState.fail(message);
        stopVpn();
    }

    private void stopVpn() {
        synchronized (stateLock) {
            starting = false;
            stopRequested = true;
            running = false;
        }
        running = false;
        AppState.vpnRunning = false;
        AppState.vpnStarting = false;
        try {
            TProxyService.TProxyStopService();
        } catch (Throwable ignored) {
        }
        if (tun != null) {
            try {
                tun.close();
            } catch (Exception ignored) {
            }
            tun = null;
        }
        if (clientStarted) {
            NativeBridge.stop();
            clientStarted = false;
        }
        stopForeground(true);
        stopSelf();
    }

    private Notification notification(String text) {
        Intent open = new Intent(this, MainActivity.class);
        PendingIntent pending = PendingIntent.getActivity(
                this, 0, open, PendingIntent.FLAG_IMMUTABLE);
        Intent stop = new Intent(this, PvpnVpnService.class).setAction(ACTION_STOP);
        PendingIntent stopPending = PendingIntent.getService(
                this, 2, stop, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        return new Notification.Builder(this, CHANNEL_ID)
                .setContentTitle("pvpn VPN")
                .setContentText(text)
                .setSmallIcon(android.R.drawable.stat_sys_download_done)
                .setContentIntent(pending)
                .addAction(android.R.drawable.ic_menu_close_clear_cancel, "Stop", stopPending)
                .setOngoing(true)
                .build();
    }

    @Override
    public void onRevoke() {
        AppState.fail("VPN permission was revoked");
        stopVpn();
        super.onRevoke();
    }

    @Override
    public void onDestroy() {
        stopVpn();
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        if (intent != null && VpnService.SERVICE_INTERFACE.equals(intent.getAction())) {
            return super.onBind(intent);
        }
        return null;
    }
}

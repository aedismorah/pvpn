package com.pvpn.app;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Intent;
import android.os.IBinder;

public class PvpnService extends Service {
    public static final String ACTION_START = "com.pvpn.app.START";
    public static final String ACTION_STOP = "com.pvpn.app.STOP";
    public static final String EXTRA_SERVER = "server";
    public static final String EXTRA_PSK = "psk";
    public static final String EXTRA_LISTEN = "listen";

    private static final String CHANNEL_ID = "pvpn";
    private static final int NOTIFICATION_ID = 7;

    private String listen = "127.0.0.1:11080";
    private boolean ownsClient = false;

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
            return START_STICKY;
        }
        if (ACTION_STOP.equals(intent.getAction())) {
            NativeBridge.stop();
            ownsClient = false;
            AppState.stopped();
            stopForeground(true);
            stopSelf();
            return START_NOT_STICKY;
        }

        String server = intent.getStringExtra(EXTRA_SERVER);
        String psk = intent.getStringExtra(EXTRA_PSK);
        String local = intent.getStringExtra(EXTRA_LISTEN);
        if (local != null && !local.isEmpty()) {
            listen = local;
        }
        AppState.beginSocks();
        startForeground(NOTIFICATION_ID, notification("Starting SOCKS5 on " + listen));

        boolean ok = server != null && psk != null && local != null
                && NativeBridge.start(server, psk, local);
        ownsClient = ok;
        if (ok) {
            AppState.socksReady();
            getSystemService(NotificationManager.class)
                    .notify(NOTIFICATION_ID, notification("SOCKS5 on " + listen));
        } else {
            String error = NativeBridge.lastError();
            if (error == null || error.isEmpty()) {
                error = "check PSK and port";
            }
            AppState.fail("SOCKS5 failed: " + error);
            getSystemService(NotificationManager.class)
                    .notify(NOTIFICATION_ID, notification("SOCKS5 failed: " + error));
        }
        return START_STICKY;
    }

    private Notification notification(String text) {
        Intent open = new Intent(this, MainActivity.class);
        PendingIntent pending = PendingIntent.getActivity(
                this, 0, open, PendingIntent.FLAG_IMMUTABLE);
        Intent stop = new Intent(this, PvpnService.class).setAction(ACTION_STOP);
        PendingIntent stopPending = PendingIntent.getService(
                this, 1, stop, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        return new Notification.Builder(this, CHANNEL_ID)
                .setContentTitle("pvpn SOCKS5")
                .setContentText(text)
                .setSmallIcon(android.R.drawable.stat_sys_download_done)
                .setContentIntent(pending)
                .addAction(android.R.drawable.ic_menu_close_clear_cancel, "Stop", stopPending)
                .setOngoing(true)
                .build();
    }

    @Override
    public void onDestroy() {
        // Stopping the native client is explicit (ACTION_STOP or process death).
        // A late onDestroy must not kill a tunnel now owned by PvpnVpnService.
        ownsClient = false;
        AppState.socksRunning = false;
        AppState.socksStarting = false;
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}

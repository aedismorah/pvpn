package com.pvpn.app;

/**
 * Small process-wide UI state. A service and the activity share the same
 * process, so this is enough to surface startup failures without broadcasts.
 */
public final class AppState {
    public static volatile boolean vpnStarting = false;
    public static volatile boolean vpnRunning = false;
    public static volatile boolean socksStarting = false;
    public static volatile boolean socksRunning = false;
    public static volatile String lastError = "";
    public static volatile String detail = "";

    private AppState() {
    }

    public static synchronized void beginVpn() {
        vpnStarting = true;
        vpnRunning = false;
        lastError = "";
        detail = "Opening TUN and starting the pvpn client...";
    }

    public static synchronized void beginSocks() {
        socksStarting = true;
        socksRunning = false;
        lastError = "";
        detail = "Starting the local SOCKS5 listener...";
    }

    public static synchronized void vpnReady() {
        vpnStarting = false;
        vpnRunning = true;
        socksRunning = false;
        lastError = "";
        detail = "All app traffic is routed through Cloud.ru -> VDSina.";
    }

    public static synchronized void socksReady() {
        socksStarting = false;
        socksRunning = true;
        lastError = "";
        detail = "SOCKS5 is listening on 127.0.0.1:11080.";
    }

    public static synchronized void stopped() {
        vpnStarting = false;
        vpnRunning = false;
        socksStarting = false;
        socksRunning = false;
        detail = "";
    }

    public static synchronized void fail(String message) {
        vpnStarting = false;
        vpnRunning = false;
        socksStarting = false;
        socksRunning = false;
        lastError = message == null ? "Unknown error" : message;
        detail = "";
    }

    public static synchronized void clearError() {
        lastError = "";
    }
}

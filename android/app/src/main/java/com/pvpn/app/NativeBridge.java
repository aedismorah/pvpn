package com.pvpn.app;

public final class NativeBridge {
    static {
        System.loadLibrary("pvpn");
    }

    private NativeBridge() {
    }

    public static native boolean start(String server, String psk, String listen);

    public static native void stop();

    public static native boolean isRunning();

    public static native String lastError();
}

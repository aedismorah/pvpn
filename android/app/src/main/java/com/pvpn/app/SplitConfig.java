package com.pvpn.app;

import android.content.Context;
import android.content.SharedPreferences;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.net.Inet4Address;
import java.net.InetAddress;
import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;

/** Persistent split-tunneling settings shared by the UI and VpnService. */
public final class SplitConfig {
    private static final String PREFS = "split_tunnel";
    private static final String KEY_ENABLED = "enabled";
    private static final String KEY_SITES_DIRECT = "sites_direct";
    private static final String KEY_APPS_DIRECT = "apps_direct";
    private static final String KEY_SITES = "sites";
    private static final String KEY_APPS = "apps";
    private static final String KEY_SEEDED = "seeded";

    private static final String[] DEFAULT_SITES = {
            "dnd.su",
            "old.yummyani.me",
    };

    private static final String[] DEFAULT_APPS = {
            "ru.burgerking",
            "ru.ozon.app.android",
            "ru.ozon.fintech.finance",
            "ru.vk.store",
            "by.advasoft.android.troika.app",
            "ru.dvfx.upsushi",
            "com.vkontakte.android",
            "com.wildberries.ru",
            "ru.yandex.taxi",
            "ru.yandex.mail",
            "ru.yandex.metro",
            "ru.yandex.yandexmaps",
            "com.apegroup.mcdonaldsrussia",
            "ru.rostel",
            "ru.sberbank.sberfriend.airwatch",
            "ru.instamart",
            "ru.sberbankmobile",
            "com.mobilemedia.tanuki",
    };

    public boolean enabled;
    public boolean sitesDirect;
    public boolean appsDirect;
    public final LinkedHashSet<String> sites = new LinkedHashSet<>();
    public final LinkedHashSet<String> apps = new LinkedHashSet<>();

    private SplitConfig() {
    }

    public static SplitConfig load(Context context) {
        SharedPreferences prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
        SplitConfig config = new SplitConfig();
        config.enabled = prefs.getBoolean(KEY_ENABLED, true);
        config.sitesDirect = prefs.getBoolean(KEY_SITES_DIRECT, true);
        config.appsDirect = prefs.getBoolean(KEY_APPS_DIRECT, true);
        readSet(prefs.getString(KEY_SITES, null), config.sites);
        readSet(prefs.getString(KEY_APPS, null), config.apps);
        if (!prefs.getBoolean(KEY_SEEDED, false)) {
            for (String site : DEFAULT_SITES) {
                config.sites.add(site);
            }
            for (String app : DEFAULT_APPS) {
                config.apps.add(app);
            }
            config.save(context);
            prefs.edit().putBoolean(KEY_SEEDED, true).apply();
        }
        if (!prefs.getBoolean("mode_v2", false)) {
            config.sitesDirect = true;
            config.appsDirect = true;
            config.save(context);
            prefs.edit().putBoolean("mode_v2", true).apply();
        }
        if (!prefs.getBoolean("defaults_v3", false)
                && config.sites.isEmpty() && config.apps.isEmpty()) {
            for (String site : DEFAULT_SITES) {
                config.sites.add(site);
            }
            for (String app : DEFAULT_APPS) {
                config.apps.add(app);
            }
            config.save(context);
            prefs.edit().putBoolean("defaults_v3", true).apply();
        }
        return config;
    }

    public void save(Context context) {
        SharedPreferences prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
        prefs.edit()
                .putBoolean(KEY_ENABLED, enabled)
                .putBoolean(KEY_SITES_DIRECT, sitesDirect)
                .putBoolean(KEY_APPS_DIRECT, appsDirect)
                .putString(KEY_SITES, toJson(sites))
                .putString(KEY_APPS, toJson(apps))
                .putBoolean(KEY_SEEDED, true)
                .apply();
    }

    public void addSite(String value) {
        sites.add(value);
    }

    public void removeSite(String value) {
        sites.remove(value);
    }

    public void addApp(String packageName) {
        apps.add(packageName);
    }

    public void removeApp(String packageName) {
        apps.remove(packageName);
    }

    public int appCount() {
        return apps.size();
    }

    private static void readSet(String raw, Set<String> target) {
        if (raw == null || raw.isEmpty()) {
            return;
        }
        try {
            JSONArray array = new JSONArray(raw);
            for (int i = 0; i < array.length(); i++) {
                String value = array.optString(i, "").trim();
                if (!value.isEmpty()) {
                    target.add(value);
                }
            }
        } catch (JSONException ignored) {
        }
    }

    private static String toJson(Set<String> values) {
        JSONArray array = new JSONArray();
        for (String value : values) {
            array.put(value);
        }
        return array.toString();
    }

    /** Resolve a domain or parse a literal IP / CIDR into IPv4 addresses. */
    public static List<String> resolveIPv4(String value) {
        List<String> addresses = new ArrayList<>();
        String host = value.trim();
        if (host.isEmpty()) {
            return addresses;
        }
        String[] parts = host.split("/", 2);
        try {
            if (parts[0].matches("\\d{1,3}(\\.\\d{1,3}){3}")) {
                addresses.add(parts[0]);
                return addresses;
            }
            for (InetAddress address : InetAddress.getAllByName(parts[0])) {
                if (address instanceof Inet4Address) {
                    String ip = address.getHostAddress();
                    if (!addresses.contains(ip)) {
                        addresses.add(ip);
                    }
                }
            }
        } catch (Exception ignored) {
        }
        return addresses;
    }
}

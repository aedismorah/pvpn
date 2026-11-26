package com.pvpn.app;

import android.app.Activity;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.os.Bundle;
import android.text.InputType;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public class SplitTunnelActivity extends Activity {
    private static final int BG = 0xFF071018;
    private static final int CARD = 0xFF0F181E;
    private static final int STROKE = 0xFF22343A;
    private static final int TEXT = 0xFFE8FFF8;
    private static final int MUTED = 0xFF8BA29E;
    private static final int AMBER = 0xFF3DE8C4;
    private static final int DANGER = 0xFFFF7E7E;

    private SplitConfig config;
    private LinearLayout sitesList;
    private TextView description;
    private ModeSwitchView modeSwitch;
    private final Map<String, TextView> ipViews = new LinkedHashMap<>();
    private final ExecutorService resolver = Executors.newCachedThreadPool();

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        config = SplitConfig.load(this);
        setContentView(build());
        refresh();
    }

    @Override
    protected void onResume() {
        super.onResume();
        config = SplitConfig.load(this);
        refresh();
    }

    @Override
    protected void onDestroy() {
        resolver.shutdownNow();
        super.onDestroy();
    }

    private View build() {
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.setBackgroundColor(BG);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(dp(20), dp(18), dp(20), dp(36));
        scroll.addView(root);

        LinearLayout header = new LinearLayout(this);
        header.setGravity(Gravity.CENTER_VERTICAL);
        TextView back = text("\u2190", 30, TEXT, false);
        back.setGravity(Gravity.CENTER);
        back.setPadding(dp(4), 0, dp(18), 0);
        back.setOnClickListener(v -> finish());
        header.addView(back, new LinearLayout.LayoutParams(dp(48), dp(48)));
        TextView title = text("Split tunneling", 20, TEXT, true);
        title.setGravity(Gravity.CENTER);
        header.addView(title, new LinearLayout.LayoutParams(0, dp(48), 1f));
        root.addView(header, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        modeSwitch = new ModeSwitchView(this);
        modeSwitch.setChecked(config.sitesDirect, false);
        modeSwitch.setOnModeChangedListener(black -> {
            config.sitesDirect = black;
            config.save(this);
            updateModeDescription();
        });
        LinearLayout.LayoutParams switchParams = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(56));
        switchParams.topMargin = dp(10);
        switchParams.bottomMargin = dp(4);
        root.addView(modeSwitch, switchParams);

        description = text("", 14, TEXT, false);
        description.setPadding(0, dp(14), 0, dp(22));
        description.setMinHeight(dp(58));
        root.addView(description);

        TextView sitesTitle = text("Sites and IPs", 16, TEXT, true);
        root.addView(sitesTitle, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        sitesList = new LinearLayout(this);
        sitesList.setOrientation(LinearLayout.VERTICAL);
        root.addView(sitesList, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        LinearLayout addSite = new LinearLayout(this);
        addSite.setGravity(Gravity.CENTER_VERTICAL);
        addSite.setBackground(round(0xFF121B21, STROKE, 22));
        addSite.setPadding(dp(14), 0, dp(4), 0);
        EditText siteInput = new EditText(this);
        siteInput.setHint("website or IP");
        siteInput.setHintTextColor(MUTED);
        siteInput.setTextColor(TEXT);
        siteInput.setTextSize(16);
        siteInput.setSingleLine(true);
        siteInput.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        siteInput.setBackground(null);
        addSite.addView(siteInput, new LinearLayout.LayoutParams(0,
                dp(52), 1f));
        TextView add = text("+", 28, AMBER, false);
        add.setGravity(Gravity.CENTER);
        add.setOnClickListener(v -> {
            String value = siteInput.getText().toString().trim();
            if (value.isEmpty()) {
                return;
            }
            if (!isValidSite(value)) {
                Toast.makeText(this, "Enter a domain, IP or CIDR", Toast.LENGTH_SHORT).show();
                return;
            }
            config.addSite(value);
            config.save(this);
            siteInput.setText("");
            renderSites();
        });
        addSite.addView(add, new LinearLayout.LayoutParams(dp(52), dp(52)));
        root.addView(addSite, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(56)));

        TextView note = text(
                "Site routing is applied when the VPN starts. Domain rules use resolved IPv4 addresses.",
                12, MUTED, false);
        note.setPadding(0, dp(18), 0, 0);
        root.addView(note);
        return scroll;
    }

    private void refresh() {
        if (description == null) {
            return;
        }
        description.setText(config.sitesDirect
                ? "Addresses from the list should not be accessed via VPN"
                : "Only addresses from the list should be accessed via VPN");
        if (modeSwitch != null) {
            modeSwitch.setChecked(config.sitesDirect, false);
        }
        renderSites();
    }

    private void updateModeDescription() {
        description.setText(config.sitesDirect
                ? "Addresses from the list should not be accessed via VPN"
                : "Only addresses from the list should be accessed via VPN");
    }

    private void renderSites() {
        sitesList.removeAllViews();
        ipViews.clear();
        List<String> sites = new ArrayList<>(config.sites);
        for (String site : sites) {
            LinearLayout row = new LinearLayout(this);
            row.setGravity(Gravity.CENTER_VERTICAL);
            row.setPadding(0, dp(12), 0, dp(12));

            LinearLayout names = new LinearLayout(this);
            names.setOrientation(LinearLayout.VERTICAL);
            TextView host = text(site, 17, TEXT, false);
            names.addView(host);
            TextView ips = text("resolving...", 12, MUTED, false);
            ips.setPadding(0, dp(4), 0, 0);
            names.addView(ips);
            ipViews.put(site, ips);
            row.addView(names, new LinearLayout.LayoutParams(0,
                    ViewGroup.LayoutParams.WRAP_CONTENT, 1f));

            TextView remove = text("\u2715", 22, DANGER, false);
            remove.setGravity(Gravity.CENTER);
            remove.setPadding(dp(12), 0, dp(4), 0);
            remove.setOnClickListener(v -> {
                config.removeSite(site);
                config.save(this);
                renderSites();
            });
            row.addView(remove, new LinearLayout.LayoutParams(dp(48), dp(48)));
            sitesList.addView(row);

            View divider = new View(this);
            divider.setBackgroundColor(0xFF1C2A30);
            sitesList.addView(divider, new LinearLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT, dp(1)));

            resolver.execute(() -> {
                List<String> resolved = SplitConfig.resolveIPv4(site);
                String value = resolved.isEmpty() ? "no IPv4 address" : String.join(", ", resolved);
                runOnUiThread(() -> {
                    TextView target = ipViews.get(site);
                    if (target != null) {
                        target.setText(value);
                    }
                });
            });
        }
    }

    private static boolean isValidSite(String value) {
        String host = value;
        int slash = host.indexOf('/');
        if (slash >= 0) {
            host = host.substring(0, slash);
        }
        if (host.matches("\\d{1,3}(\\.\\d{1,3}){3}")) {
            return true;
        }
        return host.matches("[A-Za-z0-9._-]+\\.[A-Za-z]{2,}");
    }

    private TextView text(String value, int size, int color, boolean bold) {
        TextView view = new TextView(this);
        view.setText(value);
        view.setTextSize(size);
        view.setTextColor(color);
        if (bold) {
            view.setTypeface(Typeface.DEFAULT_BOLD);
        }
        return view;
    }

    private GradientDrawable round(int fill, int stroke, int radiusDp) {
        GradientDrawable drawable = new GradientDrawable();
        drawable.setColor(fill);
        drawable.setCornerRadius(dp(radiusDp));
        if (stroke != 0) {
            drawable.setStroke(dp(1), stroke);
        }
        return drawable;
    }

    private int dp(int value) {
        return (int) (value * getResources().getDisplayMetrics().density + 0.5f);
    }
}

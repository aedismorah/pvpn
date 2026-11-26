package com.pvpn.app;

import android.app.Activity;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.os.Bundle;
import android.text.Editable;
import android.text.InputType;
import android.text.TextWatcher;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.widget.EditText;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

public class AppSplitActivity extends Activity {
    private static final int BG = 0xFF071018;
    private static final int CARD = 0xFF0F181E;
    private static final int STROKE = 0xFF22343A;
    private static final int TEXT = 0xFFE8FFF8;
    private static final int MUTED = 0xFF8BA29E;
    private static final int AMBER = 0xFF3DE8C4;
    private static final int DANGER = 0xFFFF7E7E;

    private SplitConfig config;
    private PackageManager packages;
    private LinearLayout selectedList;
    private LinearLayout suggestions;
    private TextView summary;
    private TextView description;
    private ModeSwitchView modeSwitch;
    private EditText search;
    private final List<AppEntry> entries = new ArrayList<>();

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        config = SplitConfig.load(this);
        packages = getPackageManager();
        loadApps();
        setContentView(build());
        refresh();
    }

    private void loadApps() {
        Set<String> seen = new HashSet<>();
        for (ApplicationInfo info : packages.getInstalledApplications(PackageManager.GET_META_DATA)) {
            if (packages.getLaunchIntentForPackage(info.packageName) == null) {
                continue;
            }
            if (!seen.add(info.packageName)) {
                continue;
            }
            entries.add(new AppEntry(info.packageName,
                    packages.getApplicationLabel(info).toString(), info));
        }
        Collections.sort(entries, Comparator.comparing(
                entry -> entry.label.toLowerCase()));
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
        TextView title = text("App split tunneling", 20, TEXT, true);
        title.setGravity(Gravity.CENTER);
        header.addView(title, new LinearLayout.LayoutParams(0, dp(48), 1f));
        root.addView(header, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        modeSwitch = new ModeSwitchView(this);
        modeSwitch.setChecked(config.appsDirect, false);
        modeSwitch.setOnModeChangedListener(black -> {
            config.appsDirect = black;
            config.save(this);
            updateModeDescription();
        });
        LinearLayout.LayoutParams switchParams = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(56));
        switchParams.topMargin = dp(16);
        switchParams.bottomMargin = dp(4);
        root.addView(modeSwitch, switchParams);

        description = text("", 14, MUTED, false);
        description.setPadding(0, dp(14), 0, dp(18));
        description.setMinHeight(dp(54));
        root.addView(description);

        summary = text("", 13, MUTED, false);
        summary.setPadding(0, 0, 0, dp(10));
        root.addView(summary);

        selectedList = new LinearLayout(this);
        selectedList.setOrientation(LinearLayout.VERTICAL);
        root.addView(selectedList);

        suggestions = new LinearLayout(this);
        suggestions.setOrientation(LinearLayout.VERTICAL);
        suggestions.setVisibility(View.GONE);
        root.addView(suggestions);

        LinearLayout addRow = new LinearLayout(this);
        addRow.setGravity(Gravity.CENTER_VERTICAL);
        addRow.setBackground(round(0xFF121B21, STROKE, 22));
        addRow.setPadding(dp(14), 0, dp(4), 0);
        search = new EditText(this);
        search.setHint("application name");
        search.setHintTextColor(MUTED);
        search.setTextColor(TEXT);
        search.setTextSize(16);
        search.setSingleLine(true);
        search.setInputType(InputType.TYPE_CLASS_TEXT);
        search.setBackground(null);
        addRow.addView(search, new LinearLayout.LayoutParams(0, dp(52), 1f));
        TextView dots = text("\u22ee", 26, MUTED, false);
        dots.setGravity(Gravity.CENTER);
        addRow.addView(dots, new LinearLayout.LayoutParams(dp(48), dp(52)));
        root.addView(addRow, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(56)));

        TextView note = text(
                "Changes apply next time the VPN starts. Selected apps follow the mode "
                        + "chosen on the Split tunneling screen.",
                12, MUTED, false);
        note.setPadding(0, dp(18), 0, 0);
        root.addView(note);

        search.addTextChangedListener(new TextWatcher() {
            @Override
            public void beforeTextChanged(CharSequence s, int start, int count, int after) {
            }

            @Override
            public void onTextChanged(CharSequence s, int start, int before, int count) {
                renderSuggestions(s.toString());
            }

            @Override
            public void afterTextChanged(Editable s) {
            }
        });
        return scroll;
    }

    private void refresh() {
        renderSelected();
        summary.setText(config.appCount() + " apps selected");
        if (description != null) {
            description.setText(config.appsDirect
                    ? "Apps from the list should not have access via VPN"
                    : "Only apps from the list should have access via VPN");
        }
        if (modeSwitch != null) {
            modeSwitch.setChecked(config.appsDirect, false);
        }
        if (search != null) {
            renderSuggestions(search.getText().toString());
        }
    }

    private void updateModeDescription() {
        if (description != null) {
            description.setText(config.appsDirect
                    ? "Apps from the list should not have access via VPN"
                    : "Only apps from the list should have access via VPN");
        }
    }

    private void renderSelected() {
        selectedList.removeAllViews();
        if (config.apps.isEmpty()) {
            TextView empty = text("No apps selected yet", 14, MUTED, false);
            empty.setPadding(0, dp(8), 0, dp(8));
            selectedList.addView(empty);
            return;
        }
        for (String packageName : config.apps) {
            AppEntry entry = find(packageName);
            if (entry == null) {
                continue;
            }
            selectedList.addView(appRow(entry, true));
        }
    }

    private void renderSuggestions(String query) {
        suggestions.removeAllViews();
        String needle = query.trim().toLowerCase();
        if (needle.isEmpty()) {
            suggestions.setVisibility(View.GONE);
            return;
        }
        int count = 0;
        for (AppEntry entry : entries) {
            if (config.apps.contains(entry.packageName)) {
                continue;
            }
            if (!entry.label.toLowerCase().contains(needle)
                    && !entry.packageName.toLowerCase().contains(needle)) {
                continue;
            }
            suggestions.addView(appRow(entry, false));
            count++;
            if (count >= 8) {
                break;
            }
        }
        suggestions.setBackground(round(CARD, STROKE, 14));
        suggestions.setPadding(dp(8), dp(4), dp(8), dp(4));
        suggestions.setVisibility(count == 0 ? View.GONE : View.VISIBLE);
    }

    private View appRow(AppEntry entry, boolean selected) {
        LinearLayout row = new LinearLayout(this);
        row.setGravity(Gravity.CENTER_VERTICAL);
        row.setPadding(0, dp(8), 0, dp(8));

        ImageView icon = new ImageView(this);
        icon.setImageDrawable(packages.getApplicationIcon(entry.info));
        row.addView(icon, new LinearLayout.LayoutParams(dp(42), dp(42)));

        TextView label = text(entry.label, 16, TEXT, false);
        label.setPadding(dp(14), 0, dp(8), 0);
        row.addView(label, new LinearLayout.LayoutParams(0,
                ViewGroup.LayoutParams.WRAP_CONTENT, 1f));

        if (selected) {
            TextView remove = text("\u2715", 22, DANGER, false);
            remove.setGravity(Gravity.CENTER);
            remove.setOnClickListener(v -> {
                config.removeApp(entry.packageName);
                config.save(this);
                refresh();
            });
            row.addView(remove, new LinearLayout.LayoutParams(dp(44), dp(44)));
        } else {
            TextView add = text("+", 26, AMBER, false);
            add.setGravity(Gravity.CENTER);
            add.setOnClickListener(v -> {
                config.addApp(entry.packageName);
                config.save(this);
                search.setText("");
                refresh();
            });
            row.addView(add, new LinearLayout.LayoutParams(dp(44), dp(44)));
        }
        return row;
    }

    private AppEntry find(String packageName) {
        for (AppEntry entry : entries) {
            if (entry.packageName.equals(packageName)) {
                return entry;
            }
        }
        return null;
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

    private static final class AppEntry {
        final String packageName;
        final String label;
        final ApplicationInfo info;

        AppEntry(String packageName, String label, ApplicationInfo info) {
            this.packageName = packageName;
            this.label = label;
            this.info = info;
        }
    }
}

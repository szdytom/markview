package io.github.szdytom.markview;

import android.content.Intent;
import android.os.SystemClock;
import android.view.KeyEvent;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import org.json.JSONArray;
import org.json.JSONObject;

public class SettingsTest extends AndroidTest {
    @Override protected void test() throws Exception {
        openReader();
        tap("Settings");
        waitFor(s -> s.optString("panel").equals("Settings(Generic)"));
        requireSettingsLayout();
        JSONObject settingsState = state();
        require(button(settingsState, "ToggleDropdown(WindowLayout") == null && button(settingsState, "SingleInstance") == null && button(settingsState, "RestoreSession") == null,
            "Desktop-only window and instance controls are absent");
        screenshot("settings");
        for (String sidebar : new String[] {phone ? "Tabs" : "Folder", "Outline"}) {
            String key = sidebar.toLowerCase(java.util.Locale.ROOT);
            double before = state().getJSONObject("sidebar_widths").getDouble(key);
            tap("SidebarWidth(" + sidebar + ", 1)");
            waitFor(s -> s.optJSONObject("sidebar_widths").optDouble(key) == before + 20);
            tap("SidebarWidth(" + sidebar + ", -1)");
            waitFor(s -> s.optJSONObject("sidebar_widths").optDouble(key) == before);
        }
        pass("Independent sidebar width controls on phone and tablet");
        for (int i = 0; i < 8 && button(state(), "Larger") == null; i++) { swipe(180, 550, 180, 270); swipe(180, 223, 180, 223); }
        double font = state().getDouble("font_size");
        tap("Larger");
        waitFor(s -> s.optDouble("font_size") > font);
        tap("SettingsTab(Fonts)");
        waitFor(s -> s.optString("panel").equals("Settings(Fonts)") && s.optInt("font_catalog") > 0);
        requireSettingsLayout();
        screenshot("fonts");
        tap("Fonts(Choosers)");
        waitFor(s -> button(s, "ToggleDropdown(Font(Serif)") != null);
        screenshot("font-choices");
        tap("SettingsTab(Styles)");
        JSONObject styles = waitFor(s -> s.optString("panel").equals("Settings(Styles)") && s.optJSONArray("styles") != null);
        JSONArray entries = styles.getJSONArray("styles");
        int dark = -1;
        for (int i = 0; i < entries.length(); i++) if ("dark".equals(entries.getString(i))) dark = i;
        require(dark >= 0, "Shared stylesheet catalogue");
        if (!styles.optString("selected_styles").contains("dark")) tap("StyleToggle(" + dark + ")");
        waitFor(s -> s.optString("selected_styles").contains("dark"));
        requireSettingsLayout();
        screenshot("dark-styles");
        tap("SettingsTab(About)");
        waitFor(s -> s.optString("panel").equals("Settings(About)"));
        requireSettingsLayout();
        screenshot("diagnostics");
        checkLayoutDiagnostics(activity.getResources().getConfiguration().smallestScreenWidthDp);
        tap("SettingsTab(Styles)");
        waitFor(s -> s.optString("panel").equals("Settings(Styles)"));
        sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
        waitFor(s -> s.optString("panel").equals("Closed"));
        require(!state().getBoolean("instance_listener"), "Android does not register the desktop instance listener");
        screenshot("dark-reader");
        checkSystemBars(false);
        if (phone) {
            tap("Tabs");
            waitFor(s -> s.optString("panel").equals("Tabs"));
            screenshot("tab-drawer");
            sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
            waitFor(s -> s.optString("panel").equals("Closed"));
        }
        pass("Shared settings, typography, font catalogue and styles without desktop file controls");

        sendKeyDownUpSync(KeyEvent.KEYCODE_HOME);
        SystemClock.sleep(600);
        getTargetContext().startActivity(new Intent(getTargetContext(), MarkviewActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        JSONObject resumed = waitFor(s -> s.optString("backend").length() > 0 && s.optBoolean("ready") && s.optJSONArray("tabs").length() == 1);
        require(resumed.getDouble("font_size") > font && resumed.optString("selected_styles").contains("dark"), "Settings survive background");
        screenshot("resumed");
        String saved = new String(Files.readAllBytes(new File(activity.getFilesDir(), "markview/settings.toml").toPath()), StandardCharsets.UTF_8);
        require(saved.contains("dark") && saved.contains("font_size = " + resumed.getDouble("font_size")), "Durable shared settings store");
        pass("Background resume, GPU surface recreation and durable settings");
    }
}

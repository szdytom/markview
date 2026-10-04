package io.github.szdytom.markview;

import android.app.Activity;
import android.app.Instrumentation;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.Color;
import android.graphics.Rect;
import android.net.Uri;
import android.os.Bundle;
import android.os.SystemClock;
import android.view.InputDevice;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.accessibility.AccessibilityNodeInfo;
import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import org.json.JSONArray;
import org.json.JSONObject;

/** Device integration tests operate the rendered reader with real touch events. */
public class Smoke extends Instrumentation {
    private Activity activity;
    private final StringBuilder results = new StringBuilder();
    private interface Check { boolean matches(JSONObject state) throws Exception; }
    @Override public void onCreate(Bundle args) { super.onCreate(args); start(); }
    @Override public void onStart() {
        Bundle result = new Bundle();
        try {
            require("Markview".contentEquals(getTargetContext().getApplicationInfo().loadLabel(getTargetContext().getPackageManager())), "Application display name");
            pass("Markview application display name");
            activity = startActivitySync(intent("reader.md", Intent.ACTION_VIEW));
            JSONObject initial = waitFor(s -> s.optBoolean("ready") && s.optInt("blocks") > 20 && loadedImages(s) >= 2);
            require(initial.getInt("math_errors") == 0, "Mathematics layout");
            require(initial.getString("backend").equals("Vulkan") || initial.getString("backend").equals("Gl"), "GPU backend");
            stableLayout();
            screenshot("reader");
            swipe(180, 590, 180, 180);
            JSONObject scrolled = waitFor(s -> s.optDouble("scroll") > 100);
            swipe(180, 300, 180, 300);
            JSONObject beforeSwitch = stableLayout();
            double position = beforeSwitch.getDouble("scroll");
            getTargetContext().startActivity(intent("second.md", Intent.ACTION_SEND));
            waitFor(s -> s.optJSONArray("tabs").length() == 2 && s.optBoolean("ready") && loadedImages(s) >= 1);
            // Reveal the first tab through the same horizontal touch routing.
            swipe(45, 20, 185, 20);
            tap("SelectTab(0)");
            JSONObject restored = waitFor(s -> s.optInt("active") == 0 && s.optBoolean("ready") && loadedImages(s) >= 2);
            restored = stableLayout();
            require(Math.abs(restored.getDouble("scroll") - position) < 2, "Tab reading position: " + position + " -> " + restored.getDouble("scroll") + "; height: " + beforeSwitch.getDouble("height") + " -> " + restored.getDouble("height"));
            pass("Markdown, CJK, math, code, Mermaid, images, touch scroll and cached tabs");

            tap("Settings");
            waitFor(s -> s.optString("panel").equals("Settings(Generic)"));
            requireFullscreenSettings();
            screenshot("settings");
            for (int i = 0; i < 8 && button(state(), "Larger") == null; i++) { swipe(180, 550, 180, 270); swipe(180, 223, 180, 223); }
            double font = state().getDouble("font_size");
            tap("Larger");
            waitFor(s -> s.optDouble("font_size") > font);
            tap("SettingsTab(Fonts)");
            waitFor(s -> s.optString("panel").equals("Settings(Fonts)") && s.optInt("font_catalog") > 0);
            requireFullscreenSettings();
            screenshot("fonts");
            long revision = state().getLong("font_revision");
            tap("Fonts(OpenFolder)");
            waitSystem("documentsui");
            sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
            waitFor(s -> s.optString("backend").length() > 0);
            Uri fontUri = Uri.parse("content://io.github.szdytom.markview.test.fixtures/Test.otf");
            runOnMainSync(() -> ((MarkviewActivity)activity).onActivityResult(13, Activity.RESULT_OK, new Intent().setData(fontUri)));
            waitFor(s -> s.optLong("font_revision") > revision && s.optInt("font_catalog") > 0);
            require(new File(activity.getFilesDir(), "markview/fonts/Test.otf").isFile(), "Shared personal font directory");
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
            requireFullscreenSettings();
            screenshot("dark-styles");
            tap("SettingsTab(About)");
            waitFor(s -> s.optString("panel").equals("Settings(About)"));
            requireFullscreenSettings();
            tap("SettingsTab(Styles)");
            waitFor(s -> s.optString("panel").equals("Settings(Styles)"));
            sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
            waitFor(s -> s.optString("panel").equals("Closed"));
            pass("Shared settings, typography, font imports, catalogue and styles");

            tap("SearchOpen");
            waitFor(s -> s.optBoolean("search_open"));
            sendStringSync("needle");
            waitFor(s -> s.optInt("matches") > 0);
            screenshot("search");
            tap("SearchClose");
            waitFor(s -> !s.optBoolean("search_open"));
            pass("Touch search and Android text input");

            getUiAutomation().setRotation(1);
            waitFor(s -> s.optJSONArray("dimensions").optDouble(0) > s.optJSONArray("dimensions").optDouble(1));
            screenshot("landscape");
            tap("Settings");
            JSONObject wideSettings = waitFor(s -> s.optString("panel").equals("Settings(Generic)"));
            JSONArray widePanel = wideSettings.getJSONArray("panel_rect");
            require(widePanel.getDouble(0) > 0 && widePanel.getDouble(1) > 0
                && widePanel.getDouble(2) <= 600 && widePanel.getDouble(3) <= 620, "Wide settings retain the centered dialog");
            screenshot("landscape-settings");
            sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
            waitFor(s -> s.optString("panel").equals("Closed"));
            getUiAutomation().setRotation(0);
            waitFor(s -> s.optJSONArray("dimensions").optDouble(0) < s.optJSONArray("dimensions").optDouble(1));
            sendKeyDownUpSync(KeyEvent.KEYCODE_HOME);
            SystemClock.sleep(600);
            getTargetContext().startActivity(new Intent(getTargetContext(), MarkviewActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
            JSONObject resumed = waitFor(s -> s.optString("backend").length() > 0 && s.optBoolean("ready") && s.optJSONArray("tabs").length() == 2);
            require(resumed.getDouble("font_size") > font && resumed.optString("selected_styles").contains("dark"), "Settings survive background");
            screenshot("resumed");
            String saved = new String(Files.readAllBytes(new File(activity.getFilesDir(), "markview/settings.toml").toPath()), StandardCharsets.UTF_8);
            require(saved.contains("dark") && saved.contains("font_size = " + resumed.getDouble("font_size")), "Durable shared settings store");
            pass("Rotation, safe areas, GPU surface recreation and durable settings");

            tap("Open");
            SystemClock.sleep(350);
            require(!getUiAutomation().getRootInActiveWindow().findAccessibilityNodeInfosByText("Markview").isEmpty(), "Markview document picker title");
            clickText("Open file");
            waitSystem("documentsui");
            sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
            waitFor(s -> s.optString("backend").length() > 0 && s.optJSONArray("tabs").length() == 2);
            pass("Android Storage Access Framework picker and cancellation");
            tap("Open");
            clickText("Open folder with images");
            waitSystem("documentsui");
            sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
            waitFor(s -> s.optString("backend").length() > 0);
            Uri tree = Uri.parse("content://io.github.szdytom.markview.test.fixtures/tree/root");
            runOnMainSync(() -> ((MarkviewActivity)activity).onActivityResult(12, Activity.RESULT_OK, new Intent().setData(tree)));
            JSONObject folder = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("README.md") && loadedImages(s) == 1);
            File directory = new File(folder.getString("path")).getParentFile();
            require(new File(directory, "images/logo.svg").isFile() && new File(directory, "chapter.md").isFile(), "Relative resources preserved");
            screenshot("folder");
            Uri grant = ReaderProvider.uri(activity, new File(directory, "README.md"));
            try (java.io.InputStream input = activity.getContentResolver().openInputStream(grant)) { require(input.read() == '#', "Read-only document grant"); }
            boolean refused = false;
            try { activity.getContentResolver().openFileDescriptor(grant, "w"); } catch (java.io.FileNotFoundException expected) { refused = true; }
            require(refused, "Provider rejects writes");
            pass("Folder import, relative SVG resources and read-only file grants");

            File pdf = new File(activity.getFilesDir(), "exports/README.pdf");
            pdf.delete();
            tap("Export");
            waitFor(s -> s.optString("panel").equals("Export"));
            tap("ExportFormat(Pdf)");
            tap("ExportRun");
            waitSystem("documentsui");
            clickText("Save");
            long deadline = SystemClock.uptimeMillis() + 20000;
            while ((!pdf.isFile() || pdf.length() < 100) && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(100);
            require(pdf.isFile() && pdf.length() > 100, "Native PDF export");
            byte[] bytes = Files.readAllBytes(pdf.toPath());
            require(new String(bytes, 0, 5, StandardCharsets.US_ASCII).equals("%PDF-"), "PDF header");
            SystemClock.sleep(800);
            java.lang.reflect.Field field = MarkviewActivity.class.getDeclaredField("exports");
            field.setAccessible(true);
            @SuppressWarnings("unchecked") java.util.Map<String, Uri> outputs = (java.util.Map<String, Uri>)field.get(activity);
            Uri destination = outputs.get(pdf.getAbsolutePath());
            require(destination != null, "System output URI");
            try (java.io.InputStream input = activity.getContentResolver().openInputStream(destination)) {
                byte[] header = new byte[5]; require(input.read(header) == 5 && new String(header, StandardCharsets.US_ASCII).equals("%PDF-"), "PDF published to selected provider");
            }
            getTargetContext().startActivity(new Intent(getTargetContext(), MarkviewActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
            waitFor(s -> s.optString("backend").length() > 0 && s.optBoolean("ready"));
            pass("Shared PDF exporter and Android system save destination");
            File png = new File(activity.getFilesDir(), "exports/README.png"); png.delete();
            tap("Export");
            waitFor(s -> s.optString("panel").equals("Export"));
            tap("ExportFormat(Png)");
            tap("ExportRun");
            waitSystem("documentsui");
            clickText("Save");
            deadline = SystemClock.uptimeMillis() + 25000;
            while ((!png.isFile() || png.length() < 100) && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(100);
            require(png.isFile(), "Native PNG export");
            Bitmap exported = android.graphics.BitmapFactory.decodeFile(png.getAbsolutePath());
            require(exported != null && exported.getWidth() > 300 && exported.getHeight() > 400, "PNG export dimensions");
            int copper = 0;
            for (int y = 0; y < exported.getHeight(); y += 8) for (int x = 0; x < exported.getWidth(); x += 8) {
                int c = exported.getPixel(x, y);
                if (Color.red(c) > 150 && Color.red(c) > Color.green(c)*1.1 && Color.green(c) > Color.blue(c)*1.1) copper++;
            }
            require(copper > 50, "GPU export contains the local SVG"); exported.recycle();
            pass("Shared PNG exporter and GPU readback with safe-area offsets");

            result.putString("stream", results.toString() + "MARKVIEW_ANDROID_INTEGRATION_OK\n");
            finish(Activity.RESULT_OK, result);
        } catch (Throwable error) {
            android.util.Log.e("MarkviewTest", "Integration failure", error);
            result.putString("stream", results.toString() + "FAIL: " + error + "\n");
            finish(Activity.RESULT_CANCELED, result);
        }
    }
    private void requireFullscreenSettings() throws Exception {
        JSONObject current = state();
        JSONArray size = current.getJSONArray("dimensions");
        JSONArray panel = current.getJSONArray("panel_rect");
        require(size.getDouble(0) < 640 && panel.getDouble(0) == 0 && panel.getDouble(1) == 0
            && panel.getDouble(2) == size.getDouble(0) && panel.getDouble(3) == size.getDouble(1), "Settings fill the app content area");
    }
    private void waitSystem(String name) throws Exception {
        long deadline = SystemClock.uptimeMillis() + 15000;
        while (SystemClock.uptimeMillis() < deadline) {
            AccessibilityNodeInfo root = getUiAutomation().getRootInActiveWindow();
            if (root != null && root.getPackageName().toString().contains(name)) return;
            SystemClock.sleep(100);
        }
        throw new AssertionError("System window: " + name);
    }
    private void clickText(String text) throws Exception {
        long deadline = SystemClock.uptimeMillis() + 10000;
        while (SystemClock.uptimeMillis() < deadline) {
            AccessibilityNodeInfo root = getUiAutomation().getRootInActiveWindow();
            if (root != null) for (AccessibilityNodeInfo node : root.findAccessibilityNodeInfosByText(text)) {
                Rect bounds = new Rect(); node.getBoundsInScreen(bounds);
                long down = SystemClock.uptimeMillis();
                MotionEvent press = MotionEvent.obtain(down, down, MotionEvent.ACTION_DOWN, bounds.centerX(), bounds.centerY(), 0);
                press.setSource(InputDevice.SOURCE_TOUCHSCREEN); getUiAutomation().injectInputEvent(press, true); press.recycle();
                MotionEvent release = MotionEvent.obtain(down, down + 60, MotionEvent.ACTION_UP, bounds.centerX(), bounds.centerY(), 0);
                release.setSource(InputDevice.SOURCE_TOUCHSCREEN); getUiAutomation().injectInputEvent(release, true); release.recycle();
                return;
            }
            SystemClock.sleep(100);
        }
        throw new AssertionError("System control: " + text);
    }
    private Intent intent(String name, String action) {
        Uri uri = Uri.parse("content://io.github.szdytom.markview.test.fixtures/" + name);
        Intent intent = new Intent(action).setClass(getTargetContext(), MarkviewActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_GRANT_READ_URI_PERMISSION);
        if (Intent.ACTION_SEND.equals(action)) intent.setType("text/markdown").putExtra(Intent.EXTRA_STREAM, uri);
        else intent.setDataAndType(uri, "text/markdown");
        return intent;
    }
    private JSONObject stableLayout() throws Exception {
        long deadline = SystemClock.uptimeMillis() + 10000, stable = SystemClock.uptimeMillis();
        JSONObject previous = state();
        while (SystemClock.uptimeMillis() < deadline) {
            SystemClock.sleep(100);
            JSONObject current = state();
            if (!current.optBoolean("ready") || Math.abs(current.optDouble("height") - previous.optDouble("height")) > 0.1 || Math.abs(current.optDouble("scroll") - previous.optDouble("scroll")) > 0.1) stable = SystemClock.uptimeMillis();
            if (SystemClock.uptimeMillis() - stable > 750) return current;
            previous = current;
        }
        throw new AssertionError("Layout did not settle");
    }
    private JSONObject state() throws Exception { return new JSONObject(MarkviewActivity.nativeSnapshot()); }
    private JSONObject waitFor(Check check) throws Exception {
        long deadline = SystemClock.uptimeMillis() + 20000;
        JSONObject latest = new JSONObject();
        while (SystemClock.uptimeMillis() < deadline) {
            latest = state();
            if (check.matches(latest)) { SystemClock.sleep(120); return latest; }
            SystemClock.sleep(100);
        }
        throw new AssertionError("Timed out: " + latest);
    }
    private JSONObject button(JSONObject state, String action) throws Exception {
        JSONArray buttons = state.optJSONArray("buttons");
        if (buttons == null) return null;
        for (int i = 0; i < buttons.length(); i++) {
            JSONObject b = buttons.getJSONObject(i);
            if (b.getString("action").startsWith(action) && b.getBoolean("enabled") && b.getDouble("w") > 0 && b.getDouble("h") > 10) return b;
        }
        return null;
    }
    private void tap(String action) throws Exception {
        JSONObject state = waitFor(s -> button(s, action) != null);
        JSONObject b = button(state, action);
        float x = (float)(b.getDouble("x") + b.getDouble("w") / 2);
        float y = (float)(b.getDouble("y") + b.getDouble("h") / 2);
        swipe(x, y, x, y);
    }
    private void swipe(float x1, float y1, float x2, float y2) throws Exception {
        JSONObject state = state();
        JSONArray dims = state.getJSONArray("dimensions"), insets = state.getJSONArray("insets");
        float scale = (float)dims.getDouble(2);
        float left = (float)insets.getDouble(0), top = (float)insets.getDouble(1);
        int[] origin = new int[2];
        runOnMainSync(() -> activity.getWindow().getDecorView().getLocationOnScreen(origin));
        long down = SystemClock.uptimeMillis();
        for (int i = 0; i <= 10; i++) {
            float t = i / 10f;
            int action = i == 0 ? MotionEvent.ACTION_DOWN : i == 10 ? MotionEvent.ACTION_UP : MotionEvent.ACTION_MOVE;
            MotionEvent event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, (x1 + (x2 - x1)*t + left)*scale + origin[0], (y1 + (y2 - y1)*t + top)*scale + origin[1], 0);
            event.setSource(InputDevice.SOURCE_TOUCHSCREEN);
            getUiAutomation().injectInputEvent(event, true);
            event.recycle();
            SystemClock.sleep(18);
        }
        SystemClock.sleep(160);
    }
    private int loadedImages(JSONObject state) throws Exception {
        int count = 0;
        JSONArray images = state.getJSONArray("images");
        for (int i = 0; i < images.length(); i++) if (!images.getJSONObject(i).isNull("size")) count++;
        return count;
    }
    private void screenshot(String name) throws Exception {
        SystemClock.sleep(250);
        Bitmap screenshot = getUiAutomation().takeScreenshot();
        require(screenshot != null, "Screenshot");
        Bitmap pixels = screenshot.copy(Bitmap.Config.ARGB_8888, false);
        int ink = 0;
        for (int y = pixels.getHeight()/6; y < pixels.getHeight()*5/6; y += 4) {
            for (int x = 20; x < pixels.getWidth()-20; x += 4) {
                int c = pixels.getPixel(x, y);
                if (Color.red(c) < 180 && Color.green(c) < 180 && Color.blue(c) < 180) ink++;
            }
        }
        require(ink > 20, "Rendered content in " + name);
        File dir = new File(activity.getFilesDir(), "test-artifacts");
        dir.mkdirs();
        try (FileOutputStream output = new FileOutputStream(new File(dir, name + ".png"))) { screenshot.compress(Bitmap.CompressFormat.PNG, 100, output); }
        pixels.recycle();
        screenshot.recycle();
    }
    private void require(boolean condition, String message) { if (!condition) throw new AssertionError(message); }
    private void pass(String message) { results.append("PASS: ").append(message).append('\n'); }
}

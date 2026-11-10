package io.github.szdytom.markview;

import android.app.Activity;
import android.app.Instrumentation;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.Color;
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

/** Shared device operations; the host resets app data before each test. */
public abstract class AndroidTest extends Instrumentation {
    protected Activity activity;
    protected Bundle arguments;
    protected String expectedLayout;
    protected boolean phone;
    protected int portraitRotation;
    private JSONObject lastState;
    private final StringBuilder results = new StringBuilder();
    protected interface Check { boolean matches(JSONObject state) throws Exception; }

    @Override public void onCreate(Bundle args) {
        super.onCreate(args);
        arguments = args;
        expectedLayout = args.getString("layout", "phone");
        start();
    }

    @Override public void onStart() {
        Bundle result = new Bundle();
        int code = Activity.RESULT_OK;
        int alwaysFinish = android.provider.Settings.Global.getInt(getTargetContext().getContentResolver(), "always_finish_activities", 0);
        try {
            // App startup must work without a writable system temporary directory.
            android.system.Os.setenv("TMPDIR", "/proc/markview-unwritable", true);
            // The emulator's `vulkan.ranchu` crashes when naming swapchain views.
            android.system.Os.setenv("WGPU_DEBUG", "0", true);
            // A previous crashed case may have left `Don't keep activities` enabled.
            alwaysFinish(false);
            getUiAutomation().setRotation(0);
            if (!"restore".equals(arguments.getString("session-phase"))) {
                File config = new File(getTargetContext().getFilesDir(), "markview/settings.toml");
                config.getParentFile().mkdirs();
                Files.write(config.toPath(), ("version = 1\nwindow-layout = \"macos\"\nsingle-instance = true\nwidth = 240\nstyle = [\"light\"]\n").getBytes(StandardCharsets.UTF_8));
            }
            test();
        } catch (Throwable error) {
            code = Activity.RESULT_CANCELED;
            android.util.Log.e("MarkviewTest", "Integration failure", error);
            try {
                File dir = new File(getTargetContext().getFilesDir(), "test-artifacts");
                dir.mkdirs();
                Bitmap pixels = getUiAutomation().takeScreenshot();
                try (FileOutputStream output = new FileOutputStream(new File(dir, "failure.png"))) {
                    pixels.compress(Bitmap.CompressFormat.PNG, 100, output);
                }
                pixels.recycle();
                if (lastState != null) results.append("STATE: ").append(lastState).append('\n');
            } catch (Throwable captureError) { android.util.Log.w("MarkviewTest", "Failure capture", captureError); }
            results.append("FAIL: ").append(error).append('\n');
        } finally {
            try {
                try { cleanup(); } finally { alwaysFinish(alwaysFinish != 0); }
                getUiAutomation().setRotation(android.app.UiAutomation.ROTATION_UNFREEZE);
            } catch (Throwable error) {
                code = Activity.RESULT_CANCELED;
                results.append("FAIL cleanup: ").append(error).append('\n');
            }
        }
        if (code == Activity.RESULT_OK) results.append("MARKVIEW_ANDROID_INTEGRATION_OK\n");
        result.putString("stream", results.toString());
        finish(code, result);
    }

    protected abstract void test() throws Exception;
    protected void cleanup() throws Exception {}

    protected JSONObject openReader() throws Exception {
        Intent opening = intent("reader.md", Intent.ACTION_VIEW);
        opening.setDataAndType(opening.getData(), "application/octet-stream");
        activity = startActivitySync(opening);
        waitFor(s -> s.optBoolean("ready") && s.optInt("blocks") > 20 && loadedImages(s) >= 2);
        JSONObject initial = stableLayout();
        phone = initial.getBoolean("phone_layout");
        require(phone == expectedLayout.equals("phone"), "Expected " + expectedLayout + " device layout");
        if (!phone && initial.getJSONArray("dimensions").getDouble(0) > initial.getJSONArray("dimensions").getDouble(1)) {
            portraitRotation = 1;
            getUiAutomation().setRotation(portraitRotation);
            waitFor(s -> s.optBoolean("ready") && s.optJSONArray("dimensions").optDouble(0) < s.optJSONArray("dimensions").optDouble(1));
            initial = stableLayout();
        }
        return initial;
    }

    protected void selectDarkStyle() throws Exception {
        tap("Settings");
        tap("SettingsTab(Styles)");
        JSONObject current = waitFor(s -> s.optString("panel").equals("Settings(Styles)") && s.optJSONArray("styles") != null);
        JSONArray styles = current.getJSONArray("styles");
        for (int i = 0; i < styles.length(); i++) {
            if ("dark".equals(styles.getString(i)) && !current.optString("selected_styles").contains("dark")) tap("StyleToggle(" + i + ")");
        }
        waitFor(s -> s.optString("selected_styles").contains("dark"));
        sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
        waitFor(s -> s.optString("panel").equals("Closed") && s.optBoolean("ready"));
    }
    protected void checkLayoutDiagnostics(int width) throws Exception {
        tap("CopyDiagnostics");
        String[] text = new String[1];
        runOnMainSync(() -> {
            android.content.ClipboardManager clipboard = (android.content.ClipboardManager)activity.getSystemService(Activity.CLIPBOARD_SERVICE);
            text[0] = clipboard.getPrimaryClip().getItemAt(0).coerceToText(activity).toString();
        });
        require(text[0].contains("Mobile Mode: " + (phone ? "Phone" : "Tablet") + " (" + width + " dp)\n"), "Diagnostics contain mobile mode and smallest width in one line");
        require(!text[0].contains("Smallest width:") && !text[0].contains("sw600dp:") && !text[0].contains("phone_layout:"), "Diagnostics omit separate layout rows");
        pass("Copied diagnostics include a single Mobile Mode line");
    }

    protected void alwaysFinish(boolean enabled) throws Exception {
        getUiAutomation().adoptShellPermissionIdentity("android.permission.SET_ALWAYS_FINISH");
        try {
            Object service = android.app.ActivityManager.class.getMethod("getService").invoke(null);
            Class.forName("android.app.IActivityManager").getMethod("setAlwaysFinish", boolean.class).invoke(service, enabled);
        } finally { getUiAutomation().dropShellPermissionIdentity(); }
    }

    protected void commitInput(String text) {
        runOnMainSync(() -> {
            android.view.View focused = activity.getCurrentFocus();
            require(focused != null && focused.onCheckIsTextEditor(), "Focused input exposes an IME connection");
            android.view.inputmethod.InputConnection connection = focused.onCreateInputConnection(new android.view.inputmethod.EditorInfo());
            require(connection != null && connection.commitText(text, 1), "IME commits clipboard text");
        });
    }
    protected void copyAsset(String asset, File file) throws Exception {
        file.getParentFile().mkdirs();
        try (java.io.InputStream input = getContext().getAssets().open(asset); FileOutputStream output = new FileOutputStream(file)) {
            byte[] buffer = new byte[8192];
            int count;
            while ((count = input.read(buffer)) > 0) output.write(buffer, 0, count);
        }
    }
    protected void deleteTree(File file) {
        File[] children = file.listFiles();
        if (children != null) for (File child : children) deleteTree(child);
        file.delete();
    }
    protected void checkToolbar() throws Exception {
        JSONObject current = state();
        require(button(current, "RevealFolder") == null, "The Android toolbar omits the desktop folder reveal");
        // The removed control must not leave its slot behind: the last one
        // still ends at the right inset the layout reserves for the group.
        JSONObject last = button(current, "Outline");
        require(last != null, "The toolbar keeps its Outline control");
        double right = current.getJSONArray("dimensions").getDouble(0) - 16;
        require(Math.abs(last.getDouble("x") + last.getDouble("w") - right) < 0.01,
            "The toolbar ends at the right inset: " + (last.getDouble("x") + last.getDouble("w")) + " vs " + right);
        pass("Android toolbar omits the desktop folder reveal and meets the right inset");
    }
    protected void requireSettingsLayout() throws Exception {
        JSONObject current = state();
        require(button(current, "ScrollSpeed(") == null, "Scroll speed controls are absent on Android");
        require(button(current, "RestoreSession") == null, "Session restore control is absent on Android");
        require(button(current, "OpenConfig") == null && button(current, "Fonts(OpenFolder)") == null && button(current, "StylesFolder") == null,
            "Desktop settings-file and folder controls are absent on Android");
        if (phone && current.getString("panel").equals("Settings(Generic)")) {
            require(button(current, "Narrower") == null && button(current, "Wider") == null, "Column width controls are hidden on phones");
        }
        JSONArray size = current.getJSONArray("dimensions");
        JSONArray panel = current.getJSONArray("panel_rect");
        if (size.getDouble(0) < 640) {
            require(panel.getDouble(0) == 0 && panel.getDouble(1) == 0 && panel.getDouble(2) == size.getDouble(0) && panel.getDouble(3) == size.getDouble(1), "Settings fill the app content area");
        } else {
            require(panel.getDouble(0) > 0 && panel.getDouble(1) > 0 && panel.getDouble(2) <= 600 && panel.getDouble(3) <= 620, "Wide settings retain the centered dialog");
        }
    }
    protected void requireTabletColumnWidthControls() throws Exception {
        // Small steps keep the width row visible; stop inertia after each step.
        for (int direction : new int[] {1, -1}) {
            for (int i = 0; i < 16 && button(state(), "Wider") == null; i++) {
                JSONArray panel = state().getJSONArray("panel_rect");
                float x = (float)(panel.getDouble(0) + panel.getDouble(2) / 2);
                float middle = (float)(panel.getDouble(1) + panel.getDouble(3) / 2);
                float end = middle - direction * 88;
                swipe(x, middle, x, end);
                swipe(x, end, x, end);
            }
        }
        JSONObject current = state();
        require(button(current, "Narrower") != null && button(current, "Wider") != null, "Column width controls are offered only on tablets");
    }
    protected void waitSystem(String name) throws Exception {
        SystemWindowWait.await(new SystemWindowWait.Probe() {
            public long now() { return SystemClock.uptimeMillis(); }
            public void sleep() { SystemClock.sleep(100); }
            public boolean closeLauncherAnr() throws Exception {
                AccessibilityNodeInfo root = getUiAutomation().getRootInActiveWindow();
                if (root == null || !"android".contentEquals(root.getPackageName())
                    || (root.findAccessibilityNodeInfosByText("Quickstep isn't responding").isEmpty()
                        && root.findAccessibilityNodeInfosByText("Pixel Launcher isn't responding").isEmpty())) return false;
                android.util.Log.i("MarkviewTest", "Closing launcher ANR; waiting for system recovery");
                clickText("Close app");
                return true;
            }
            public boolean focused() {
                AccessibilityNodeInfo root = getUiAutomation().getRootInActiveWindow();
                boolean reader = name.equals(getTargetContext().getPackageName());
                return root != null && root.getPackageName().toString().contains(name) && activity.hasWindowFocus() == reader;
            }
            public void idle(long timeout) throws Exception {
                getUiAutomation().waitForIdle(1000, timeout);
            }
        }, name);
    }
    protected void clickText(String text) throws Exception {
        android.accessibilityservice.AccessibilityServiceInfo service = getUiAutomation().getServiceInfo();
        service.flags |= android.accessibilityservice.AccessibilityServiceInfo.FLAG_RETRIEVE_INTERACTIVE_WINDOWS;
        getUiAutomation().setServiceInfo(service);
        long deadline = SystemClock.uptimeMillis() + 10000;
        while (SystemClock.uptimeMillis() < deadline) {
            java.util.List<AccessibilityNodeInfo> roots = new java.util.ArrayList<>();
            roots.add(getUiAutomation().getRootInActiveWindow());
            for (android.view.accessibility.AccessibilityWindowInfo window : getUiAutomation().getWindows()) roots.add(window.getRoot());
            for (AccessibilityNodeInfo root : roots) if (root != null) for (AccessibilityNodeInfo node : root.findAccessibilityNodeInfosByText(text)) {
                // System sheets can expose label bounds before their layout settles.
                for (AccessibilityNodeInfo control = node; control != null; control = control.getParent()) {
                    if (control.isVisibleToUser() && control.isEnabled() && control.isClickable()
                        && control.performAction(AccessibilityNodeInfo.ACTION_CLICK)) return;
                }
            }
            SystemClock.sleep(100);
        }
        throw new AssertionError("System control: " + text);
    }
    protected Intent intent(String name, String action) {
        Uri uri = Uri.parse("content://io.github.szdytom.markview.test.fixtures/" + name);
        Intent intent = new Intent(action).setPackage(getTargetContext().getPackageName()).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_GRANT_READ_URI_PERMISSION);
        if (Intent.ACTION_SEND.equals(action)) intent.setType("text/markdown").putExtra(Intent.EXTRA_STREAM, uri);
        else intent.setDataAndType(uri, "text/markdown");
        return intent;
    }
    protected boolean keyboardVisible() {
        final boolean[] visible = {false};
        runOnMainSync(() -> visible[0] = activity.getWindow().getDecorView()
            .getRootWindowInsets().isVisible(android.view.WindowInsets.Type.ime()));
        return visible[0];
    }
    protected JSONObject stableLayout() throws Exception {
        return stableLayout(true);
    }
    private JSONObject stableLayout(boolean requireReady) throws Exception {
        long deadline = SystemClock.uptimeMillis() + 10000, stable = SystemClock.uptimeMillis();
        JSONObject previous = state();
        while (SystemClock.uptimeMillis() < deadline) {
            SystemClock.sleep(100);
            JSONObject current = state();
            if ((requireReady && !current.optBoolean("ready")) || Math.abs(current.optDouble("height") - previous.optDouble("height")) > 0.1 || Math.abs(current.optDouble("scroll") - previous.optDouble("scroll")) > 0.1
                || Math.abs(current.optDouble("layout_width") - previous.optDouble("layout_width")) > 0.01
                || !current.optJSONArray("dimensions").toString().equals(previous.optJSONArray("dimensions").toString())
                || !current.optJSONArray("insets").toString().equals(previous.optJSONArray("insets").toString())) stable = SystemClock.uptimeMillis();
            if (SystemClock.uptimeMillis() - stable > 750) return current;
            previous = current;
        }
        throw new AssertionError("Layout did not settle");
    }
    protected JSONObject state() throws Exception {
        long deadline = SystemClock.uptimeMillis() + 20000;
        do {
            JSONObject current = new JSONObject(MarkviewActivity.nativeSnapshot());
            // A busy or suspended native event loop returns `{}` after three seconds.
            if (current.length() > 0) { lastState = current; return current; }
            SystemClock.sleep(100);
        } while (SystemClock.uptimeMillis() < deadline);
        throw new AssertionError("Native inspection did not respond");
    }
    protected JSONObject waitFor(Check check) throws Exception {
        long deadline = SystemClock.uptimeMillis() + 20000;
        JSONObject latest = new JSONObject();
        while (SystemClock.uptimeMillis() < deadline) {
            latest = state();
            if (check.matches(latest)) { SystemClock.sleep(120); return latest; }
            SystemClock.sleep(100);
        }
        throw new AssertionError("Timed out: " + latest);
    }
    protected JSONObject button(JSONObject state, String action) throws Exception {
        JSONArray buttons = state.optJSONArray("buttons");
        if (buttons == null) return null;
        for (int i = 0; i < buttons.length(); i++) {
            JSONObject b = buttons.getJSONObject(i);
            if (b.getString("action").startsWith(action) && b.getBoolean("enabled") && b.getDouble("w") > 0 && b.getDouble("h") > 10) return b;
        }
        return null;
    }
    protected void tap(String action) throws Exception {
        waitSystem(getTargetContext().getPackageName());
        waitFor(s -> button(s, action) != null);
        JSONObject state = stableLayout(false);
        JSONObject b = button(state, action);
        require(b != null, "Button remains visible after layout settles: " + action);
        float x = (float)(b.getDouble("x") + b.getDouble("w") / 2);
        float y = (float)(b.getDouble("y") + b.getDouble("h") / 2);
        android.util.Log.i("MarkviewTest", "Tap " + action + " at " + x + "," + y);
        injectSwipe(x, y, x, y, state);
    }
    protected void swipe(float x1, float y1, float x2, float y2) throws Exception {
        waitSystem(getTargetContext().getPackageName());
        injectSwipe(x1, y1, x2, y2, state());
    }
    private void injectSwipe(float x1, float y1, float x2, float y2, JSONObject state) throws Exception {
        JSONArray dims = state.getJSONArray("dimensions"), insets = state.getJSONArray("insets");
        float scale = (float)dims.getDouble(2);
        float left = (float)insets.getDouble(0), top = (float)insets.getDouble(1);
        int[] origin = new int[2];
        runOnMainSync(() -> activity.getWindow().getDecorView().getLocationOnScreen(origin));
        long down = SystemClock.uptimeMillis();
        // Taps use only `DOWN` and `UP`.
        int steps = x1 == x2 && y1 == y2 ? 1 : 10;
        for (int i = 0; i <= steps; i++) {
            float t = (float)i / steps;
            int action = i == 0 ? MotionEvent.ACTION_DOWN : i == steps ? MotionEvent.ACTION_UP : MotionEvent.ACTION_MOVE;
            MotionEvent event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, (x1 + (x2 - x1)*t + left)*scale + origin[0], (y1 + (y2 - y1)*t + top)*scale + origin[1], 0);
            event.setSource(InputDevice.SOURCE_TOUCHSCREEN);
            long started = SystemClock.uptimeMillis();
            boolean accepted = getUiAutomation().injectInputEvent(event, true);
            String detail = "Touch " + MotionEvent.actionToString(action) + " at " + event.getX() + "," + event.getY()
                + "; held=" + (event.getEventTime() - down) + "ms; injection=" + (SystemClock.uptimeMillis() - started) + "ms; accepted=" + accepted;
            if (i == 0 || i == steps || !accepted) android.util.Log.i("MarkviewTest", detail);
            event.recycle();
            require(accepted, detail);
            SystemClock.sleep(18);
        }
        SystemClock.sleep(160);
    }
    protected int loadedImages(JSONObject state) throws Exception {
        int count = 0;
        JSONArray images = state.getJSONArray("images");
        for (int i = 0; i < images.length(); i++) if (!images.getJSONObject(i).isNull("size")) count++;
        return count;
    }
    protected void checkSystemBars(boolean light) throws Exception {
        SystemClock.sleep(250);
        // Wait for the clipboard preview and theme transition to finish.
        long deadline = SystemClock.uptimeMillis() + 10000;
        while (true) {
            waitSystem(getTargetContext().getPackageName());
            JSONObject current = state();
            double scale = current.getJSONArray("dimensions").getDouble(2);
            int[] origin = new int[2];
            runOnMainSync(() -> activity.getWindow().getDecorView().getLocationOnScreen(origin));
            int insetTop = (int)Math.round(current.getJSONArray("insets").getDouble(1) * scale);
            int top = origin[1] + insetTop;
            int bottom = top + (int)Math.round(current.getJSONArray("dimensions").getDouble(1) * scale);
            Bitmap pixels = getUiAutomation().takeScreenshot();
            require(pixels != null, "System bar screenshot");
            try {
                int x = origin[0] + (int)Math.round(current.getJSONArray("dimensions").getDouble(0) * scale / 8);
                if (insetTop > 4) {
                    int middle = pixels.getWidth() / 2;
                    require(sameColor(pixels.getPixel(middle, origin[1] + insetTop / 2), pixels.getPixel(middle, top + 2)), "Status bar blends into the toolbar");
                }
                if (bottom + 4 < pixels.getHeight()) {
                    require(sameColor(pixels.getPixel(x, bottom - 2), pixels.getPixel(x, (bottom + pixels.getHeight()) / 2)), "Navigation bar blends into the footer");
                }
                int background = pixels.getPixel(x, bottom - 2);
                require((Color.red(background) + Color.green(background) + Color.blue(background) > 384) == light, "System bars follow the selected style");
                break;
            } catch (AssertionError error) {
                if (SystemClock.uptimeMillis() >= deadline) {
                    screenshot("system-bars");
                    throw error;
                }
            } finally { pixels.recycle(); }
            SystemClock.sleep(100);
        }
        pass((light ? "Light" : "Dark") + " system bars blend into reader chrome");
    }
    protected boolean sameColor(int a, int b) {
        return Math.abs(Color.red(a) - Color.red(b)) <= 2 && Math.abs(Color.green(a) - Color.green(b)) <= 2 && Math.abs(Color.blue(a) - Color.blue(b)) <= 2;
    }
    protected void screenshot(String name) throws Exception {
        waitSystem(getTargetContext().getPackageName());
        captureScreenshot(name);
    }
    protected void captureScreenshot(String name) throws Exception {
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
    protected void require(boolean condition, String message) { if (!condition) throw new AssertionError(message); }
    protected void pass(String message) { results.append("PASS: ").append(message).append('\n'); }
}

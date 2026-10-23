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
    private String expectedLayout;
    private boolean layoutOnly;
    private boolean lifecycleOnly;
    private String sessionPhase;
    private boolean mermaidOnly;
    private boolean linksOnly;
    private boolean phone;
    private int portraitRotation;
    private final StringBuilder results = new StringBuilder();
    private interface Check { boolean matches(JSONObject state) throws Exception; }
    @Override public void onCreate(Bundle args) { super.onCreate(args); sessionPhase = args.getString("session-phase"); expectedLayout = args.getString("layout", "phone"); layoutOnly = args.getBoolean("layout-only", false) || "true".equals(args.getString("layout-only")); lifecycleOnly = "true".equals(args.getString("lifecycle-only")); mermaidOnly = "true".equals(args.getString("mermaid-only")); linksOnly = "true".equals(args.getString("links-only")); start(); }
    @Override public void onStart() {
        Bundle result = new Bundle();
        try {
            // The emulator's `vulkan.ranchu` crashes when naming swapchain views.
            // Keep validation enabled, but disable driver debug labels in tests.
            android.system.Os.setenv("WGPU_DEBUG", "0", true);
            if (sessionPhase != null) {
                checkSession();
                result.putString("stream", results.toString() + "MARKVIEW_ANDROID_INTEGRATION_OK\n");
                finish(Activity.RESULT_OK, result);
                return;
            }
            require("Markview".contentEquals(getTargetContext().getApplicationInfo().loadLabel(getTargetContext().getPackageManager())), "Application display name");
            pass("Markview application display name");
            checkExternalIntents();
            if (linksOnly) {
                new File(getTargetContext().getFilesDir(), "markview/session.json").delete();
                checkWebLinks(true);
                result.putString("stream", results.toString() + "MARKVIEW_ANDROID_INTEGRATION_OK\n");
                finish(Activity.RESULT_OK, result);
                return;
            }
            new File(getTargetContext().getFilesDir(), "markview/session.json").delete();
            File config = new File(getTargetContext().getFilesDir(), "markview/settings.toml");
            config.getParentFile().mkdirs();
            String stored = config.exists() ? new String(Files.readAllBytes(config.toPath()), StandardCharsets.UTF_8) : "";
            stored = "window-layout = \"macos\"\nsingle-instance = true\nwidth = 240\nstyle = [\"light\"]\n" + stored.replaceAll("(?m)^(window-layout|single-instance|width|style) *=.*\\R?", "");
            Files.write(config.toPath(), stored.getBytes(StandardCharsets.UTF_8));
            getUiAutomation().setRotation(0);
            Intent opening = intent("reader.md", Intent.ACTION_VIEW);
            opening.setDataAndType(opening.getData(), "application/octet-stream");
            activity = startActivitySync(opening);
            JSONObject initial = waitFor(s -> s.optBoolean("ready") && s.optInt("blocks") > 20 && loadedImages(s) >= 2);
            initial = stableLayout();
            require(initial.getString("window_layout").equals("Macos") && initial.getBoolean("single_instance"), "Saved desktop options loaded");
            require(!initial.getString("frame_layout").equals("Macos") && !initial.getBoolean("instance_listener"), "Saved desktop options do not affect Android");
            phone = initial.getBoolean("phone_layout");
            double availableWidth = initial.getJSONArray("dimensions").getDouble(0) - 40;
            double expectedWidth = phone ? availableWidth : Math.min(initial.getDouble("column_width"), availableWidth);
            require(Math.abs(initial.getDouble("layout_width") - expectedWidth) < 0.01, "Reading width: " + initial.getDouble("layout_width") + "; expected: " + expectedWidth + "; dimensions: " + initial.getJSONArray("dimensions"));
            if (!phone && initial.getJSONArray("dimensions").getDouble(0) > initial.getJSONArray("dimensions").getDouble(1)) {
                portraitRotation = 1;
                getUiAutomation().setRotation(portraitRotation);
                initial = waitFor(s -> s.optBoolean("ready") && s.optJSONArray("dimensions").optDouble(0) < s.optJSONArray("dimensions").optDouble(1));
            }
            int smallestWidth = activity.getResources().getConfiguration().smallestScreenWidthDp;
            require(phone == (smallestWidth < 600) && phone == expectedLayout.equals("phone"), "sw600dp resource selection: " + smallestWidth);
            require(((MarkviewActivity)activity).phoneLayout() == phone, "Native and Java layout agree");
            require(activity.getRequestedOrientation() == (phone ? android.content.pm.ActivityInfo.SCREEN_ORIENTATION_PORTRAIT : android.content.pm.ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED), "Device orientation policy");
            pass("sw600dp " + expectedLayout + " layout and orientation policy (sw" + smallestWidth + "dp)");
            if (lifecycleOnly) {
                checkLifecycle();
                result.putString("stream", results.toString() + "MARKVIEW_ANDROID_INTEGRATION_OK\n");
                finish(Activity.RESULT_OK, result);
                return;
            }
            if (layoutOnly) {
                require((button(state(), "Tabs") != null) == phone, "Drawer toggle matches sw600dp");
                checkToolbar();
                screenshot("reader");
                tap("Settings");
                waitFor(s -> s.optString("panel").equals("Settings(Generic)"));
                require((button(state(), "ToggleDropdown(TabStyle") == null) == phone, "Tab style matches sw600dp");
                requireSettingsLayout();
                screenshot("settings");
                if (!phone) {
                    getUiAutomation().setRotation(0);
                    waitFor(s -> s.optJSONArray("dimensions").optDouble(0) > s.optJSONArray("dimensions").optDouble(1));
                    requireSettingsLayout();
                    requireTabletColumnWidthControls();
                    screenshot("landscape-settings");
                }
                tap("SettingsTab(About)");
                waitFor(s -> s.optString("panel").equals("Settings(About)"));
                screenshot("layout");
                checkLayoutDiagnostics(smallestWidth);
                result.putString("stream", results.toString() + "MARKVIEW_ANDROID_INTEGRATION_OK\n");
                finish(Activity.RESULT_OK, result);
                return;
            }
            if (mermaidOnly) {
                checkMermaidLabels("mermaid-light");
                tap("Settings");
                tap("SettingsTab(Styles)");
                JSONArray styles = state().getJSONArray("styles");
                for (int i = 0; i < styles.length(); i++) if ("dark".equals(styles.getString(i))) tap("StyleToggle(" + i + ")");
                sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
                waitFor(s -> s.optString("panel").equals("Closed") && s.optBoolean("ready"));
                checkMermaidLabels("mermaid-dark");
                result.putString("stream", results.toString() + "MARKVIEW_ANDROID_INTEGRATION_OK\n");
                finish(Activity.RESULT_OK, result);
                return;
            }
            require(initial.getInt("math_errors") == 0, "Mathematics layout");
            require(MarkviewActivity.nativeBridgeReferencesReleased(), "JNI local references are released on success and failure");
            pass("JNI bridge releases local references while the calling thread remains attached");
            require(initial.getString("backend").equals("Vulkan") || initial.getString("backend").equals("Gl"), "GPU backend");
            stableLayout();
            screenshot("reader");
            checkToolbar();
            checkSystemBars(true);
            swipe(280, 180, 100, 180);
            waitFor(s -> s.optBoolean("outline_open"));
            swipe(280, 180, 100, 180);
            require(state().getBoolean("outline_open"), "Left swipe keeps Contents open");
            tap("Outline");
            waitFor(s -> !s.optBoolean("outline_open"));
            swipe(180, 590, 180, 180);
            JSONObject scrolled = waitFor(s -> s.optDouble("scroll") > 100);
            swipe(180, 300, 180, 300);
            JSONObject beforeSwitch = stableLayout();
            double position = beforeSwitch.getDouble("scroll");
            Intent sharing = intent("second.md", Intent.ACTION_SEND).setType("application/x-markdown").setPackage(null);
            getTargetContext().startActivity(Intent.createChooser(sharing, "Read Markdown").addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
            clickText("Read in Markview");
            waitSystem(getTargetContext().getPackageName());
            waitFor(s -> s.optJSONArray("tabs").length() == 2 && s.optBoolean("ready") && loadedImages(s) >= 1);
            require(!activity.isDestroyed() && state().getString("path").endsWith("/second.md"), "Share reuses the running reader");
            pass("Implicit open at startup and share into the running reader");
            if (phone) {
                require(button(state(), "SelectTab(") == null, "Phone has no horizontal tab strip");
                swipe(100, 180, 280, 180);
                waitFor(s -> s.optString("panel").equals("Tabs"));
                swipe(100, 180, 220, 180);
                require(state().getString("panel").equals("Tabs"), "Right swipe keeps Tabs open");
                swipe(220, 180, 100, 180);
                waitFor(s -> s.optString("panel").equals("Closed") && !s.optBoolean("outline_open"));
                swipe(280, 180, 100, 180);
                waitFor(s -> s.optBoolean("outline_open") && s.optString("panel").equals("Closed"));
                swipe(100, 180, 280, 180);
                waitFor(s -> s.optString("panel").equals("Tabs") && !s.optBoolean("outline_open"));
                screenshot("tab-drawer-light");
            } else {
                require(button(state(), "Tabs") == null, "Tablet keeps the desktop tab strip");
                swipe(45, 20, 185, 20);
            }
            tap("SelectTab(0)");
            JSONObject restored = waitFor(s -> s.optInt("active") == 0 && s.optBoolean("ready") && loadedImages(s) >= 2);
            restored = stableLayout();
            require(Math.abs(restored.getDouble("scroll") - position) < 2, "Tab reading position: " + position + " -> " + restored.getDouble("scroll") + "; height: " + beforeSwitch.getDouble("height") + " -> " + restored.getDouble("height"));
            pass("Markdown, CJK, math, code, Mermaid, images, touch scroll and cached tabs");

            tap("Settings");
            waitFor(s -> s.optString("panel").equals("Settings(Generic)"));
            requireSettingsLayout();
            JSONObject settingsState = state();
            require(button(settingsState, "ToggleDropdown(WindowLayout") == null && button(settingsState, "SingleInstance") == null && button(settingsState, "RestoreSession") == null,
                "Desktop-only window and instance controls are absent");
            require((button(settingsState, "ToggleDropdown(TabStyle") == null) == phone, "Tab style is offered only on tablets");
            if (!phone) {
                tap("ToggleDropdown(TabStyle");
                tap("TabStyle(Connected)");
                waitFor(s -> s.optString("tab_style").equals("Connected"));
            }
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

            tap("SearchOpen");
            waitFor(s -> s.optBoolean("search_open"));
            sendStringSync("needle");
            waitFor(s -> s.optInt("matches") > 0);
            waitFor(s -> keyboardVisible());
            double searchHeight = stableLayout().getJSONArray("dimensions").getDouble(1);
            for (String action : new String[]{"SearchNext", "SearchNext", "SearchPrevious"}) {
                tap(action);
                long deadline = SystemClock.uptimeMillis() + 750;
                do {
                    require(keyboardVisible(), "Search navigation keeps the keyboard visible: " + action);
                    require(Math.abs(state().getJSONArray("dimensions").getDouble(1) - searchHeight) < 0.1, "Search navigation keeps the viewport height: " + action);
                    SystemClock.sleep(30);
                } while (SystemClock.uptimeMillis() < deadline);
            }
            screenshot("search");
            tap("SearchClose");
            waitFor(s -> !s.optBoolean("search_open") && !keyboardVisible());
            pass("Touch search, Android text input and stable keyboard during result navigation");

            getUiAutomation().setRotation(phone ? 1 : 1 - portraitRotation);
            if (phone) {
                SystemClock.sleep(1000);
                JSONObject rotated = state();
                JSONArray size = rotated.getJSONArray("dimensions");
                require(size.getDouble(0) < size.getDouble(1) && rotated.getBoolean("phone_layout"), "Phone remains portrait when the device rotates");
                tap("Tabs");
                sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
                waitFor(s -> s.optString("panel").equals("Closed"));
                pass("Phone portrait lock and Back dismissal of the tab drawer");
            } else {
                waitFor(s -> s.optJSONArray("dimensions").optDouble(0) > s.optJSONArray("dimensions").optDouble(1));
                require(!state().getBoolean("phone_layout"), "Tablet remains a tablet in landscape");
                screenshot("landscape");
                tap("Settings");
                waitFor(s -> s.optString("panel").equals("Settings(Generic)"));
                requireSettingsLayout();
                requireTabletColumnWidthControls();
                for (int i = 0; i < 9; i++) tap("Wider");
                waitFor(s -> s.optDouble("column_width") >= 760);
                screenshot("landscape-settings");
                sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
                waitFor(s -> s.optString("panel").equals("Closed"));
                stableLayout();
                screenshot("tablet-tabs");
                pass("Tablet rotation and centered settings dialog");
            }
            getUiAutomation().setRotation(portraitRotation);
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
            waitSystem(getTargetContext().getPackageName());
            stableLayout();
            pass("Android Storage Access Framework picker and cancellation");
            tap("Open");
            clickText("Open folder with images");
            waitSystem("documentsui");
            sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
            waitFor(s -> s.optString("backend").length() > 0);
            waitSystem(getTargetContext().getPackageName());
            stableLayout();
            Uri tree = Uri.parse("content://io.github.szdytom.markview.test.fixtures/tree/root");
            configureFolder(tree, true, false);
            runOnMainSync(() -> ((MarkviewActivity)activity).onActivityResult(12, Activity.RESULT_OK, new Intent().setData(tree)));
            JSONObject folder = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("README.md") && loadedImages(s) == 1);
            File directory = new File(folder.getString("path")).getParentFile();
            require(new File(directory, "images/logo.svg").isFile() && new File(directory, "chapter.md").isFile(), "Relative resources preserved");
            require(folder.optBoolean("folder_open") && folder.optString("folder_root").equals(directory.toString()), "Folder import retains its root and opens the file tree");
            require(folder.optString("folder_name").equals("Research notes"), "Folder tree displays the imported folder's name");
            screenshot("folder");
            if (phone) {
                require(folder.optString("panel").equals("Tabs") && folder.optString("sidebar_tab").equals("Folder"), "Folder imports open the combined sidebar's folder page");
                require(button(folder, "SidebarTab(Tabs)") != null && button(folder, "SidebarTab(Folder)") != null, "Both sidebar pages share their switcher");
                require(button(folder, "OpenFolder") != null && button(folder, "SelectTab(") == null, "Folder controls replace tab rows");
                tap("SidebarTab(Tabs)");
                require(button(state(), "SelectTab(") != null && button(state(), "FolderEntry(") == null, "Tab page hides folder rows");
                tap("SidebarTab(Folder)");
                sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
                waitFor(s -> s.optString("panel").equals("Closed"));
                tap("Tabs");
                waitFor(s -> s.optString("sidebar_tab").equals("Folder") && s.optBoolean("folder_open"));
                pass("Phone sidebar switches between folders and tabs and remembers its page after dismissal");
            }
            tap("FolderEntry(" + folderEntry(folder, "chapter.md") + ")");
            JSONObject chapter = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("chapter.md"));
            int chapterTab = chapter.getInt("active");
            if (phone) {
                tap("Tabs");
                tap("SidebarTab(Tabs)");
                tap("CloseTab(" + chapterTab + ")");
                waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("README.md"));
                tap("Tabs");
            } else {
                JSONObject tab = button(chapter, "SelectTab(" + chapterTab + ")");
                float x = (float)(tab.getDouble("x") + tab.getDouble("w") + 12);
                float y = (float)(tab.getDouble("y") + tab.getDouble("h") / 2);
                swipe(x, y, x, y);
            }
            waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("README.md"));
            if (!state().optBoolean("folder_open")) {
                if (phone) {
                    if (!state().optString("panel").equals("Tabs")) tap("Tabs");
                    tap("SidebarTab(Folder)");
                } else tap("Folder");
            }
            tap("FolderEntry(" + folderEntry(state(), "README.md") + ")");
            waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("README.md") && loadedImages(s) == 1);
            Uri grant = ReaderProvider.uri(activity, new File(directory, "README.md"));
            try (java.io.InputStream input = activity.getContentResolver().openInputStream(grant)) { require(input.read() == '#', "Read-only document grant"); }
            boolean refused = false;
            try { activity.getContentResolver().openFileDescriptor(grant, "w"); } catch (java.io.FileNotFoundException expected) { refused = true; }
            require(refused, "Provider rejects writes");
            pass("Folder import, relative SVG resources and read-only file grants");
            byte[] imported = Files.readAllBytes(new File(directory, "README.md").toPath());
            configureFolder(tree, true, true);
            reimportFolder(tree);
            require(java.util.Arrays.equals(imported, Files.readAllBytes(new File(directory, "README.md").toPath())) && new File(directory, "chapter.md").isFile(), "Failed folder import preserves the previous copy");
            configureFolder(tree, false, false);
            reimportFolder(tree);
            require(!new File(directory, "chapter.md").exists() && new File(directory, "images/logo.svg").isFile(), "Folder reimport removes deleted source files and retains current resources");
            for (File file : directory.getParentFile().listFiles()) require(!file.getName().startsWith(".import-") && !file.getName().startsWith(".previous-"), "Folder import removes temporary copies");
            Bundle deletedImage = new Bundle();
            deletedImage.putBoolean("chapter", false);
            deletedImage.putBoolean("image", false);
            getContext().getContentResolver().call(tree, "folder", null, deletedImage);
            reimportFolder(tree);
            require(!new File(directory, "images/logo.svg").exists(), "Folder reimport removes deleted source images");
            require(new File(directory, "images").isDirectory(), "Folder import preserves empty subdirectories");
            configureFolder(tree, false, false);
            reimportFolder(tree);
            waitFor(s -> s.optBoolean("ready") && loadedImages(s) == 1);
            pass("Folder reimport removes deleted documents and images and preserves the old copy on failure");
            if (phone && state().optString("panel").equals("Tabs")) sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);

            // Intercept export previews so third-party viewers cannot steal test input.
            android.content.IntentFilter previews = new android.content.IntentFilter(Intent.ACTION_VIEW);
            previews.addDataScheme("content");
            previews.addDataType("application/pdf");
            previews.addDataType("image/png");
            ActivityMonitor preview = addMonitor(previews, null, true);
            File pdf = new File(activity.getFilesDir(), "exports/README.pdf");
            pdf.delete();
            tap("Export");
            waitFor(s -> s.optString("panel").equals("Export"));
            tap("ExportStyles");
            JSONObject exportStyles = waitFor(s -> s.optString("panel").equals("ExportStyles"));
            require(button(exportStyles, "StylesFolder") == null, "Styles folder control is absent from Android export styles");
            tap("ExportStyles");
            waitFor(s -> s.optString("panel").equals("Export"));
            tap("ExportFormat(Pdf)");
            tap("ExportRun");
            waitSystem("documentsui");
            clickText("Save");
            long deadline = SystemClock.uptimeMillis() + 20000;
            while ((!pdf.isFile() || pdf.length() < 100) && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(100);
            require(pdf.isFile() && pdf.length() > 100, "Native PDF export");
            waitFor(s -> preview.getHits() == 1 && s.optBoolean("ready"));
            byte[] bytes = Files.readAllBytes(pdf.toPath());
            require(new String(bytes, 0, 5, StandardCharsets.US_ASCII).equals("%PDF-"), "PDF header");
            String pdfSource = new String(bytes, StandardCharsets.ISO_8859_1);
            require(pdfSource.contains("KaTeX_Main") && pdfSource.contains("KaTeX_Math") && pdfSource.contains("KaTeX_Size"), "PDF embeds bundled KaTeX fonts for inline and display mathematics");
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
            waitFor(s -> preview.getHits() == 2 && s.optBoolean("ready"));
            removeMonitor(preview);
            Bitmap exported = android.graphics.BitmapFactory.decodeFile(png.getAbsolutePath());
            require(exported != null && exported.getWidth() > 300 && exported.getHeight() > 400, "PNG export dimensions");
            int copper = 0;
            for (int y = 0; y < exported.getHeight(); y += 8) for (int x = 0; x < exported.getWidth(); x += 8) {
                int c = exported.getPixel(x, y);
                if (Color.red(c) > 150 && Color.red(c) > Color.green(c)*1.1 && Color.green(c) > Color.blue(c)*1.1) copper++;
            }
            require(copper > 50, "GPU export contains the local SVG"); exported.recycle();
            pass("Shared PNG exporter and GPU readback with safe-area offsets");

            if (phone) {
                getTargetContext().startActivity(new Intent(getTargetContext(), MarkviewActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
                waitFor(s -> s.optString("backend").length() > 0 && s.optBoolean("ready") && s.optJSONArray("dimensions") != null);
                tap("Tabs");
                tap("SidebarTab(Tabs)");
                int tabs = state().getJSONArray("tabs").length();
                int active = state().getInt("active");
                tap("CloseTab(0)");
                waitFor(s -> s.optJSONArray("tabs").length() == tabs - 1 && s.optString("panel").equals("Tabs"));
                require(state().getInt("active") == active - 1, "Closing an inactive tab preserves the active document");
                tap("CloseTab(" + (active - 1) + ")");
                waitFor(s -> s.optJSONArray("tabs").length() == tabs - 2 && s.optBoolean("ready"));
                // Dismiss through the scrim, then open another file through the drawer.
                JSONArray size = state().getJSONArray("dimensions");
                swipe((float)size.getDouble(0) - 20, 300, (float)size.getDouble(0) - 20, 300);
                waitFor(s -> s.optString("panel").equals("Closed"));
                tap("Tabs");
                tap("Open");
                clickText("Open file");
                waitSystem("documentsui");
                sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
                waitFor(s -> s.optString("panel").equals("Closed"));
                pass("Phone drawer switches, closes active/inactive tabs, dismisses on scrim and opens the system picker");
            }
            checkMermaidLabels("mermaid-dark");
            checkSharedContent();
            checkLifecycle();
            result.putString("stream", results.toString() + "MARKVIEW_ANDROID_INTEGRATION_OK\n");
            finish(Activity.RESULT_OK, result);
        } catch (Throwable error) {
            android.util.Log.e("MarkviewTest", "Integration failure", error);
            try {
                File dir = new File(activity.getFilesDir(), "test-artifacts");
                dir.mkdirs();
                Bitmap pixels = getUiAutomation().takeScreenshot();
                try (FileOutputStream output = new FileOutputStream(new File(dir, "failure.png"))) { pixels.compress(Bitmap.CompressFormat.PNG, 100, output); }
                pixels.recycle();
                results.append("STATE: ").append(state()).append('\n');
            } catch (Exception captureError) { android.util.Log.w("MarkviewTest", "Failure capture", captureError); }
            result.putString("stream", results.toString() + "FAIL: " + error + "\n");
            finish(Activity.RESULT_CANCELED, result);
        }
    }
    private void checkExternalIntents() throws Exception {
        android.content.pm.PackageManager packages = getTargetContext().getPackageManager();
        String[] types = {"text/markdown", "text/x-markdown", "text/plain", "application/markdown", "application/x-markdown", "application/octet-stream"};
        for (String action : new String[]{Intent.ACTION_VIEW, Intent.ACTION_SEND}) for (String type : types) {
            Intent external = intent("reader.md", action).setType(type);
            if (Intent.ACTION_VIEW.equals(action)) external.setDataAndType(Uri.parse("content://io.github.szdytom.markview.test.fixtures/opaque/42"), type).addCategory(Intent.CATEGORY_BROWSABLE);
            java.util.List<android.content.pm.ResolveInfo> matches = packages.queryIntentActivities(external, android.content.pm.PackageManager.MATCH_DEFAULT_ONLY);
            require(matches.size() == 1, "One read entry for " + action + " " + type);
            android.content.pm.ActivityInfo entry = matches.get(0).activityInfo;
            require(entry.name.endsWith(".ReadInMarkview") && entry.targetActivity.equals(MarkviewActivity.class.getName()) && entry.exported, "External read entry targets the reader");
            String[] labels = {"Read in Markview", "在 Markview 中阅读", "在 Markview 中閱讀"};
            java.util.Locale[] locales = {java.util.Locale.ENGLISH, java.util.Locale.SIMPLIFIED_CHINESE, java.util.Locale.TRADITIONAL_CHINESE};
            for (int i = 0; i < locales.length; i++) {
                android.content.res.Configuration configuration = new android.content.res.Configuration(getTargetContext().getResources().getConfiguration());
                configuration.setLocale(locales[i]);
                require(labels[i].contentEquals(getTargetContext().createConfigurationContext(configuration).getText(entry.labelRes)), "Localized read entry: " + locales[i]);
            }
        }
        for (Intent unrelated : new Intent[]{
            new Intent(Intent.ACTION_VIEW).setData(Uri.parse("ftp://example.com/README.md")),
            intent("reader.md", Intent.ACTION_VIEW).setDataAndType(Uri.parse("content://io.github.szdytom.markview.test.fixtures/reader.md"), "application/pdf"),
            intent("reader.md", Intent.ACTION_SEND).setType("image/png")
        }) {
            unrelated.setPackage(getTargetContext().getPackageName());
            require(packages.queryIntentActivities(unrelated, android.content.pm.PackageManager.MATCH_DEFAULT_ONLY).isEmpty(), "Unrelated intents do not offer the reader");
        }
        for (String scheme : new String[]{"http", "https"}) {
            Intent link = new Intent(Intent.ACTION_VIEW, Uri.parse(scheme + "://example.com/article?q=1#section"))
                .addCategory(Intent.CATEGORY_BROWSABLE).setPackage(getTargetContext().getPackageName());
            java.util.List<android.content.pm.ResolveInfo> matches = packages.queryIntentActivities(link, android.content.pm.PackageManager.MATCH_DEFAULT_ONLY);
            require(matches.size() == 1 && matches.get(0).activityInfo.name.endsWith(".ReadInMarkview"), "Web link read entry: " + scheme);
        }
        Intent launcher = new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER).setPackage(getTargetContext().getPackageName());
        java.util.List<android.content.pm.ResolveInfo> launchers = packages.queryIntentActivities(launcher, 0);
        require(launchers.size() == 1 && "Markview".contentEquals(launchers.get(0).loadLabel(packages)), "One launcher named Markview");
        pass("Open/share MIME matching, localized entry labels and launcher name");
    }
    private void checkSharedContent() throws Exception {
        int tabs = state().getJSONArray("tabs").length();
        Intent clipped = intent("second.md", Intent.ACTION_SEND).setType("application/octet-stream");
        clipped.removeExtra(Intent.EXTRA_STREAM);
        clipped.setClipData(android.content.ClipData.newRawUri("Markdown", Uri.parse("content://io.github.szdytom.markview.test.fixtures/second.md")));
        clipped.putExtra(Intent.EXTRA_TEXT, "https://example.com/article");
        getTargetContext().startActivity(clipped);
        JSONObject shared = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("/second.md"));
        require("Trusted".equals(shared.getString("trust")), "User-opened file retains Trusted resource permissions");
        require(shared.getJSONArray("tabs").length() == tabs, "Sharing an existing file reuses its tab");
        require(new String(Files.readAllBytes(new File(shared.getString("path")).toPath()), StandardCharsets.UTF_8).startsWith("# Second tab"), "Shared file takes precedence over accompanying text");
        String markdown = "# Shared Markdown\n\nRead without saving first.\n\n![local](private.svg)";
        Intent text = new Intent(Intent.ACTION_SEND).setPackage(getTargetContext().getPackageName()).setType("text/plain")
            .putExtra(Intent.EXTRA_TEXT, new android.text.SpannableString(markdown)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        getTargetContext().startActivity(text);
        shared = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("/Shared.md"));
        require("Untrusted".equals(shared.getString("trust")), "Shared text retains Untrusted resource permissions");
        require(shared.getJSONArray("images").getJSONObject(0).getString("error").contains("Permission required"), "Shared text cannot read local images without selection");
        require(markdown.equals(new String(Files.readAllBytes(new File(shared.getString("path")).toPath()), StandardCharsets.UTF_8)), "Shared text imports its Markdown source");
        require(!activity.isDestroyed(), "External shares retain the Activity");
        pass("ClipData file sharing and CharSequence Markdown text sharing");
        checkWebLinks(false);
    }
    private void checkWebLinks(boolean cold) throws Exception {
        try (java.net.ServerSocket server = new java.net.ServerSocket(0, 8, java.net.InetAddress.getByName("198.18.0.1"))) {
            Thread responder = new Thread(() -> {
                try {
                    while (!server.isClosed()) try (java.net.Socket client = server.accept()) {
                        java.io.BufferedReader request = new java.io.BufferedReader(new java.io.InputStreamReader(client.getInputStream(), StandardCharsets.UTF_8));
                        String first = request.readLine();
                        while (!request.readLine().isEmpty()) {}
                        String title = first.contains("/second") ? "Second web article" : "First web article";
                        StringBuilder html = new StringBuilder("<html><head><title>" + title + "</title></head><body><article><h1>" + title + "</h1>");
                        for (int i = 0; i < 12; i++) html.append("<p>Native reading preserves clear typography and readable paragraphs. This local article fixture exercises Android link delivery, article extraction and document layout without requiring public network access.</p>");
                        byte[] body = html.append("</article></body></html>").toString().getBytes(StandardCharsets.UTF_8);
                        client.getOutputStream().write(("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: " + body.length + "\r\nConnection: close\r\n\r\n").getBytes(StandardCharsets.US_ASCII));
                        client.getOutputStream().write(body);
                    }
                } catch (Exception error) {
                    if (!server.isClosed()) throw new RuntimeException(error);
                }
            }, "web-link-fixture");
            responder.setDaemon(true);
            responder.start();
            String url = "http://198.18.0.1:" + server.getLocalPort() + "/first?filter[tag]=rust";
            Intent view = new Intent(Intent.ACTION_VIEW, Uri.parse(url)).setPackage(getTargetContext().getPackageName())
                .addCategory(Intent.CATEGORY_BROWSABLE).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            if (cold) activity = startActivitySync(view);
            else getTargetContext().startActivity(view);
            JSONObject page = waitFor(s -> s.optBoolean("ready") && s.optString("path").contains("First web article"));
            String path = page.getString("path");
            require(new String(Files.readAllBytes(new File(path).toPath()), StandardCharsets.UTF_8).contains(url), "Web article imports its source URL");
            int tabs = page.getJSONArray("tabs").length();
            Intent share = new Intent(Intent.ACTION_SEND).setType("text/plain").setPackage(getTargetContext().getPackageName())
                .putExtra(Intent.EXTRA_TEXT, new android.text.SpannableString("  " + url.replace("/first", "/second") + "  ")).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            getTargetContext().startActivity(share);
            page = waitFor(s -> s.optBoolean("ready") && s.optString("path").contains("Second web article"));
            require(page.getJSONArray("tabs").length() == tabs + 1, "URL share opens another web tab");
            share.putExtra(Intent.EXTRA_TEXT, "Article title\n" + url);
            getTargetContext().startActivity(share);
            waitFor(s -> s.optBoolean("ready") && s.optString("path").equals(path));
            require(state().getJSONArray("tabs").length() == tabs + 1, "Title and URL sharing reuses the existing tab");
            getTargetContext().startActivity(view.setData(Uri.parse(url.replace("/first", "/second"))));
            waitFor(s -> s.optBoolean("ready") && s.optString("path").contains("Second web article"));
            view.setData(Uri.parse(url));
            getTargetContext().startActivity(view);
            waitFor(s -> s.optBoolean("ready") && s.optString("path").equals(path));
            require(state().getJSONArray("tabs").length() == tabs + 1 && !activity.isDestroyed(), "Opening an existing URL retains tabs and Activity");
            for (String valid : new String[]{"https://example.com/article?filter[tag]=rust", "https://[2001:db8::1]/article", "https://example.com/article%20name"}) {
                require(valid.equals(MarkviewActivity.nativeSharedUrl("  " + valid + "  ")), "Native shared URL parsing: " + valid);
                require(valid.equals(MarkviewActivity.nativeSharedUrl("Article title\r\n" + valid)), "Title and URL parsing: " + valid);
            }
            String markdown = "- Read this article\n" + url;
            for (String document : new String[]{markdown, "# Heading\n" + url, "**Article title**\n" + url, "> Article title\n" + url, "1. Read this article\n" + url, "# Shared Markdown\n\nKeep this paragraph.\n\n" + url + "\n\nAnd this paragraph too.", "Article title\n\n" + url, url + "\nhttps://example.com/", "https://[broken]/"})
                require(MarkviewActivity.nativeSharedUrl(document) == null, "Shared document stays Markdown: " + document);
            for (String source : new String[]{markdown, "     " + url + " ", "\t" + url, "Article title\n    " + url, url + " is worth reading", "Article title\n" + url + " is worth reading"}) {
                require(MarkviewActivity.nativeSharedUrl(source) == null, "Markdown source survives shared URL classification");
                share.putExtra(Intent.EXTRA_TEXT, new android.text.SpannableString(source));
                getTargetContext().startActivity(share);
                waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("/Shared.md")
                    && source.equals(new String(Files.readAllBytes(new File(s.getString("path")).toPath()), StandardCharsets.UTF_8)));
            }
            pass("Shared list items, indented URL code blocks and URL prose retain their complete Markdown source");
            if (state().getBoolean("phone_layout")) tap("Tabs");
            tap("NewPage");
            waitFor(s -> button(s, "FocusInput(Url)") != null);
            double fullHeight = state().getJSONArray("dimensions").getDouble(1);
            tap("FocusInput(Url)");
            JSONObject resized = waitFor(s -> keyboardVisible()
                && s.getJSONArray("dimensions").getDouble(1) < fullHeight - 50);
            for (String action : new String[]{"FocusInput(Url)", "OpenUrl"}) {
                JSONObject control = button(resized, action);
                require(control != null && control.getDouble("y") >= 40
                    && control.getDouble("y") + control.getDouble("h") <= resized.getJSONArray("dimensions").getDouble(1) - 28,
                    "Web control remains visible above the keyboard: " + action);
            }
            sendStringSync(url);
            tap("OpenUrl");
            waitFor(s -> s.optBoolean("ready") && s.optString("path").equals(path) && !keyboardVisible());
            pass("Empty-page URL input and Open button stay visible and usable with the Android keyboard: " + resized.getJSONArray("dimensions"));
            pass("Shared Markdown preservation and native URL parsing for bracketed queries and IPv6 hosts");
            pass("HTTP article extraction, shared URLs, title and URL shares, and link tab reuse" + (cold ? " at cold startup" : ""));
        }
    }
    private void checkSession() throws Exception {
        File directory = new File(getTargetContext().getFilesDir(), "markview");
        File session = new File(directory, "session.json");
        File expectedFile = new File(directory, "session-test.json");
        if (sessionPhase.equals("seed")) {
            directory.mkdirs();
            session.delete();
            Files.write(new File(directory, "settings.toml").toPath(), "version = 1\nstyle = [\"light\"]\n".getBytes(StandardCharsets.UTF_8));
            activity = startActivitySync(intent("reader.md", Intent.ACTION_VIEW));
            waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("reader.md"));
            swipe(180, 590, 180, 180);
            waitFor(s -> s.optDouble("scroll") > 100);
            swipe(180, 300, 180, 300);
            stableLayout();
            Activity previous = activity;
            getTargetContext().startActivity(intent("second.md", Intent.ACTION_VIEW));
            waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("second.md"));
            getTargetContext().startActivity(intent("reader.md", Intent.ACTION_VIEW));
            JSONObject expected = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("reader.md"));
            require(activity == previous && !previous.isDestroyed() && expected.getJSONArray("tabs").length() == 2, "External opens reuse the singleTask Activity");
            require(expected.getBoolean("restore_session") && expected.getBoolean("single_instance") && !expected.getBoolean("instance_listener"), "Android defaults enable session restore without desktop IPC");
            Files.write(expectedFile.toPath(), expected.toString().getBytes(StandardCharsets.UTF_8));
            sendKeyDownUpSync(KeyEvent.KEYCODE_HOME);
            long deadline = SystemClock.uptimeMillis() + 5000;
            while ((!session.exists() || new JSONObject(new String(Files.readAllBytes(session.toPath()), StandardCharsets.UTF_8)).getJSONArray("tabs").length() != 2) && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(50);
            require(session.exists(), "Background saves the session");
            JSONObject saved = new JSONObject(new String(Files.readAllBytes(session.toPath()), StandardCharsets.UTF_8));
            require(saved.getJSONArray("tabs").length() == 2 && saved.getInt("active") == expected.getInt("active"), "Background saves all tabs and the active tab");
            pass("Android background persistence and singleTask external opens");
        } else {
            JSONObject expected = new JSONObject(new String(Files.readAllBytes(expectedFile.toPath()), StandardCharsets.UTF_8));
            activity = startActivitySync(new Intent(getTargetContext(), MarkviewActivity.class).setAction(Intent.ACTION_MAIN).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
            JSONObject restored = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("reader.md"));
            restored = stableLayout();
            require(restored.getJSONArray("tabs").toString().equals(expected.getJSONArray("tabs").toString()) && restored.getInt("active") == expected.getInt("active"), "Process restart restores tab order and current tab");
            require(Math.abs(restored.getDouble("scroll") - expected.getDouble("scroll")) < 2, "Process restart restores the reading position");
            getTargetContext().startActivity(intent("reader.md", Intent.ACTION_VIEW));
            JSONObject opened = waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("reader.md"));
            require(opened.getJSONArray("tabs").length() == 2, "External opening selects the restored tab without duplicates");
            pass("Android process restart restores tabs and reading positions");
        }
    }
    private void checkLifecycle() throws Exception {
        getTargetContext().startActivity(intent("reader.md", Intent.ACTION_VIEW));
        waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("reader.md") && loadedImages(s) >= 2);
        org.json.JSONArray retained = new org.json.JSONArray();
        org.json.JSONArray openedTabs = state().getJSONArray("tabs");
        for (int i = 0; i < openedTabs.length(); i++) {
            String path = openedTabs.getString(i);
            if (path.contains("/files/documents/") || path.contains("/files/shared/")) retained.put(path);
        }
        int pid = android.os.Process.myPid();
        double fontSize = state().getDouble("font_size");
        for (int attempt = 0; attempt < 4; attempt++) {
            boolean recreate = attempt < 2;
            boolean discardOnBackground = attempt == 3;
            int alwaysFinish = android.provider.Settings.Global.getInt(getTargetContext().getContentResolver(), "always_finish_activities", 0);
            if (discardOnBackground) alwaysFinish(true);
            Activity previous = activity;
            ActivityMonitor monitor = addMonitor(MarkviewActivity.class.getName(), null, false);
            try {
                runOnMainSync(() -> {
                    if (recreate) previous.recreate();
                    else if (discardOnBackground) previous.startActivity(new Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
                    else previous.finish();
                });
                long deadline = SystemClock.uptimeMillis() + 5000;
                while (!previous.isDestroyed() && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(50);
                require(previous.isDestroyed(), "Activity destruction finishes within five seconds");
                if (!recreate) activity = startActivitySync(intent("reader.md", Intent.ACTION_VIEW));
                else activity = monitor.waitForActivityWithTimeout(10000);
                require(activity != null && activity != previous, "A new Activity is created in the same process");
                JSONObject recreated = waitFor(s -> s.optBoolean("ready") && s.optInt("blocks") > 20 && loadedImages(s) >= 2);
                require(android.os.Process.myPid() == pid, "Activity restart preserves the process");
                require(recreated.getDouble("font_size") == fontSize, "Activity restart retains preferences");
                require(recreated.getJSONArray("tabs").toString().equals(retained.toString()), "Activity restart restores the existing tab session");
                java.lang.reflect.Method deliver = MarkviewActivity.class.getDeclaredMethod("deliver", int.class, String.class);
                deliver.setAccessible(true);
                deliver.invoke(previous, 0, new File(previous.getFilesDir(), "stale.md").getAbsolutePath());
                SystemClock.sleep(250);
                require(state().getJSONArray("tabs").toString().equals(retained.toString()), "Destroyed Activity cannot deliver stale I/O results");
            } finally {
                removeMonitor(monitor);
                if (discardOnBackground) {
                    alwaysFinish(alwaysFinish != 0);
                }
            }
        }
        pass("Activity recreation, finish/reopen and Don't keep activities preserve preferences without stale callbacks");
    }
    private void alwaysFinish(boolean enabled) throws Exception {
        getUiAutomation().adoptShellPermissionIdentity("android.permission.SET_ALWAYS_FINISH");
        try {
            Object service = android.app.ActivityManager.class.getMethod("getService").invoke(null);
            Class.forName("android.app.IActivityManager").getMethod("setAlwaysFinish", boolean.class).invoke(service, enabled);
        } finally { getUiAutomation().dropShellPermissionIdentity(); }
    }
    private void checkLayoutDiagnostics(int width) throws Exception {
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
    private void configureFolder(Uri tree, boolean chapter, boolean failCopy) {
        Bundle options = new Bundle();
        options.putBoolean("chapter", chapter);
        options.putBoolean("failCopy", failCopy);
        getContext().getContentResolver().call(tree, "folder", null, options);
    }
    private int folderEntry(JSONObject current, String name) throws Exception {
        JSONArray entries = current.getJSONArray("folder_entries");
        for (int i = 0; i < entries.length(); i++) {
            if (entries.getJSONObject(i).getString("path").endsWith("/" + name)) return i;
        }
        throw new AssertionError("Missing folder entry: " + name);
    }
    private void reimportFolder(Uri tree) throws Exception {
        runOnMainSync(() -> ((MarkviewActivity)activity).onActivityResult(12, Activity.RESULT_OK, new Intent().setData(tree)));
        java.lang.reflect.Field field = MarkviewActivity.class.getDeclaredField("io");
        field.setAccessible(true);
        ((java.util.concurrent.ExecutorService)field.get(activity)).submit(() -> {}).get(20, java.util.concurrent.TimeUnit.SECONDS);
        stableLayout();
    }
    private void checkToolbar() throws Exception {
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
    private void requireSettingsLayout() throws Exception {
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
    private void requireTabletColumnWidthControls() throws Exception {
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
    private void waitSystem(String name) throws Exception {
        long deadline = SystemClock.uptimeMillis() + 15000;
        while (SystemClock.uptimeMillis() < deadline) {
            AccessibilityNodeInfo root = getUiAutomation().getRootInActiveWindow();
            if (root != null && "android".contentEquals(root.getPackageName())
                && (!root.findAccessibilityNodeInfosByText("Quickstep isn't responding").isEmpty()
                    || !root.findAccessibilityNodeInfosByText("Pixel Launcher isn't responding").isEmpty())) {
                clickText("Close app");
                continue;
            }
            boolean reader = name.equals(getTargetContext().getPackageName());
            if (root != null && root.getPackageName().toString().contains(name) && activity.hasWindowFocus() == reader) return;
            SystemClock.sleep(100);
        }
        throw new AssertionError("System window: " + name);
    }
    private void clickText(String text) throws Exception {
        long deadline = SystemClock.uptimeMillis() + 10000;
        while (SystemClock.uptimeMillis() < deadline) {
            AccessibilityNodeInfo root = getUiAutomation().getRootInActiveWindow();
            if (root != null) for (AccessibilityNodeInfo node : root.findAccessibilityNodeInfosByText(text)) {
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
    private Intent intent(String name, String action) {
        Uri uri = Uri.parse("content://io.github.szdytom.markview.test.fixtures/" + name);
        Intent intent = new Intent(action).setPackage(getTargetContext().getPackageName()).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_GRANT_READ_URI_PERMISSION);
        if (Intent.ACTION_SEND.equals(action)) intent.setType("text/markdown").putExtra(Intent.EXTRA_STREAM, uri);
        else intent.setDataAndType(uri, "text/markdown");
        return intent;
    }
    private boolean keyboardVisible() {
        final boolean[] visible = {false};
        runOnMainSync(() -> {
            android.view.View decor = activity.getWindow().getDecorView();
            if (android.os.Build.VERSION.SDK_INT >= 30) {
                visible[0] = decor.getRootWindowInsets().isVisible(android.view.WindowInsets.Type.ime());
            } else {
                Rect frame = new Rect();
                decor.getWindowVisibleDisplayFrame(frame);
                android.util.DisplayMetrics metrics = new android.util.DisplayMetrics();
                activity.getWindowManager().getDefaultDisplay().getRealMetrics(metrics);
                // System bars alone occupy less than `100 dp` below the visible frame.
                visible[0] = metrics.heightPixels - frame.bottom > 100 * metrics.density;
            }
        });
        return visible[0];
    }
    private JSONObject stableLayout() throws Exception {
        long deadline = SystemClock.uptimeMillis() + 10000, stable = SystemClock.uptimeMillis();
        JSONObject previous = state();
        while (SystemClock.uptimeMillis() < deadline) {
            SystemClock.sleep(100);
            JSONObject current = state();
            if (!current.optBoolean("ready") || Math.abs(current.optDouble("height") - previous.optDouble("height")) > 0.1 || Math.abs(current.optDouble("scroll") - previous.optDouble("scroll")) > 0.1
                || Math.abs(current.optDouble("layout_width") - previous.optDouble("layout_width")) > 0.01
                || !current.optJSONArray("dimensions").toString().equals(previous.optJSONArray("dimensions").toString())) stable = SystemClock.uptimeMillis();
            if (SystemClock.uptimeMillis() - stable > 750) return current;
            previous = current;
        }
        throw new AssertionError("Layout did not settle");
    }
    private JSONObject state() throws Exception {
        long deadline = SystemClock.uptimeMillis() + 20000;
        do {
            JSONObject current = new JSONObject(MarkviewActivity.nativeSnapshot());
            // A busy or suspended native event loop returns `{}` after three seconds.
            if (current.length() > 0) return current;
            SystemClock.sleep(100);
        } while (SystemClock.uptimeMillis() < deadline);
        throw new AssertionError("Native inspection did not respond");
    }
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
        waitSystem(getTargetContext().getPackageName());
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
    private void checkSystemBars(boolean light) throws Exception {
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
    private void checkMermaidLabels(String name) throws Exception {
        getTargetContext().startActivity(intent("mermaid.md", Intent.ACTION_VIEW));
        waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("/mermaid.md") && loadedImages(s) == 1);
        stableLayout();
        screenshot(name);
        File capture = new File(activity.getFilesDir(), "test-artifacts/" + name + ".png");
        Bitmap pixels = android.graphics.BitmapFactory.decodeFile(capture.getAbsolutePath());
        int[] ink = new int[3];
        for (int y = 0; y < pixels.getHeight(); y++) for (int x = 0; x < pixels.getWidth(); x++) {
            int c = pixels.getPixel(x, y);
            int r = Color.red(c), g = Color.green(c), b = Color.blue(c);
            if (r > 200 && g < 50 && b < 50) ink[0]++;
            if (g > 200 && r < 50 && b < 50) ink[1]++;
            if (b > 200 && r < 50 && g < 50) ink[2]++;
        }
        pixels.recycle();
        require(ink[0] > 10 && ink[1] > 10 && ink[2] > 10, "All three Mermaid labels render in " + name);
        pass("Mermaid label pixels in " + name);
    }
    private boolean sameColor(int a, int b) {
        return Math.abs(Color.red(a) - Color.red(b)) <= 2 && Math.abs(Color.green(a) - Color.green(b)) <= 2 && Math.abs(Color.blue(a) - Color.blue(b)) <= 2;
    }
    private void screenshot(String name) throws Exception {
        waitSystem(getTargetContext().getPackageName());
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

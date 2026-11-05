package io.github.szdytom.markview;

import android.content.Intent;
import android.view.KeyEvent;
import org.json.JSONArray;
import org.json.JSONObject;

public class TabsTest extends AndroidTest {
    @Override protected void test() throws Exception {
        openReader();
        swipe(180, 590, 180, 180);
        waitFor(s -> s.optDouble("scroll") > 100);
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

        if (phone) {
            getTargetContext().startActivity(intent("mermaid.md", Intent.ACTION_VIEW));
            waitFor(s -> s.optBoolean("ready") && s.optString("path").endsWith("mermaid.md"));
            tap("Tabs");
            int active = state().getInt("active");
            tap("CloseTab(0)");
            waitFor(s -> s.optJSONArray("tabs").length() == 2 && s.optString("panel").equals("Tabs"));
            require(state().getInt("active") == active - 1, "Closing an inactive tab preserves the active document");
            tap("CloseTab(" + (active - 1) + ")");
            waitFor(s -> s.optJSONArray("tabs").length() == 1 && s.optBoolean("ready"));
            JSONArray size = state().getJSONArray("dimensions");
            swipe((float)size.getDouble(0) - 20, 300, (float)size.getDouble(0) - 20, 300);
            waitFor(s -> s.optString("panel").equals("Closed"));
            tap("Tabs");
            tap("Open");
            waitSystem("documentsui");
            sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
            waitFor(s -> s.optString("panel").equals("Closed"));
            pass("Phone drawer closes active/inactive tabs, dismisses on scrim and opens the system picker");
        }
    }
}

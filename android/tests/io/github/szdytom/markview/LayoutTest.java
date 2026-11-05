package io.github.szdytom.markview;

import org.json.JSONObject;

public class LayoutTest extends AndroidTest {
    @Override protected void test() throws Exception {
        JSONObject initial = openReader();
        require(initial.getString("window_layout").equals("Macos") && initial.getBoolean("single_instance"), "Saved desktop options loaded");
        require(!initial.getString("frame_layout").equals("Macos") && !initial.getBoolean("instance_listener"), "Saved desktop options do not affect Android");
        double availableWidth = initial.getJSONArray("dimensions").getDouble(0) - 40;
        double expectedWidth = phone ? availableWidth : Math.min(initial.getDouble("column_width"), availableWidth);
        require(Math.abs(initial.getDouble("layout_width") - expectedWidth) < 0.01, "Reading width: " + initial.getDouble("layout_width") + "; expected: " + expectedWidth + "; dimensions: " + initial.getJSONArray("dimensions"));
        int smallestWidth = activity.getResources().getConfiguration().smallestScreenWidthDp;
        require(phone == (smallestWidth < 600) && phone == expectedLayout.equals("phone"), "sw600dp resource selection: " + smallestWidth);
        require(((MarkviewActivity)activity).phoneLayout() == phone, "Native and Java layout agree");
        require(activity.getRequestedOrientation() == (phone ? android.content.pm.ActivityInfo.SCREEN_ORIENTATION_PORTRAIT : android.content.pm.ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED), "Device orientation policy");
        pass("sw600dp " + expectedLayout + " layout and orientation policy (sw" + smallestWidth + "dp)");
        require((button(state(), "Tabs") != null) == phone, "Drawer toggle matches sw600dp");
        checkToolbar();
        screenshot("reader");
        tap("Settings");
        waitFor(s -> s.optString("panel").equals("Settings(Generic)"));
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
    }
}
